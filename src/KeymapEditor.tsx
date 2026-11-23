import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import type { EditOp, EditResult, EditorCatalog, KeyGeom, KeymapSummary } from "./types";
import { Catalog, describe } from "./bindings";
import { KeyPicker } from "./KeyPicker";
import { BehaviorsPanel } from "./BehaviorsPanel";

const UNIT = 54; // px per key unit

export function KeymapEditor({ keymap, onChanged, onReload }: {
  keymap: KeymapSummary;
  onChanged: (k: KeymapSummary) => void;
  /** Re-read everything from disk (after an outside change). */
  onReload: () => Promise<void>;
}) {
  const [layerIndex, setLayerIndex] = useState(0);
  const [selected, setSelected] = useState<number | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  const [geometry, setGeometry] = useState<KeyGeom[]>([]);
  const [catalog, setCatalog] = useState<Catalog | null>(null);
  const [canUndo, setCanUndo] = useState(false);
  const [canRedo, setCanRedo] = useState(false);
  const [renaming, setRenaming] = useState<string | null>(null);
  const [adding, setAdding] = useState<string | null>(null);
  const version = useRef<string | null>(null);

  const loadCatalog = useCallback(() => {
    invoke<EditorCatalog>("editor_catalog", { keymapPath: keymap.path }).then((c) => setCatalog(new Catalog(c))).catch(() => setCatalog(null));
  }, [keymap.path]);

  useEffect(() => {
    invoke<KeyGeom[]>("physical_layout").then(setGeometry).catch(() => setGeometry([]));
  }, []);
  useEffect(loadCatalog, [loadCatalog, keymap]);

  // Notice edits made outside the app (text editor, git pull) and reload.
  useEffect(() => {
    let stop = false;
    invoke<string>("file_version", { path: keymap.path }).then((v) => { version.current ??= v; });
    const t = setInterval(async () => {
      try {
        const v = await invoke<string>("file_version", { path: keymap.path });
        if (!stop && version.current && v !== version.current) {
          version.current = v;
          await invoke("forget_history", { keymapPath: keymap.path });
          setCanUndo(false);
          setCanRedo(false);
          setSelected(null);
          await onReload();
          setNotice("Your keymap file changed outside mxKeys, so it was reloaded. (Undo history was cleared.)");
        }
      } catch { /* file temporarily missing (e.g. during git checkout) */ }
    }, 2000);
    return () => { stop = true; clearInterval(t); };
  }, [keymap.path, onReload]);

  const apply = useCallback((r: EditResult) => {
    version.current = r.version;
    setCanUndo(r.canUndo);
    setCanRedo(r.canRedo);
    onChanged(r.keymap);
    setError(null);
  }, [onChanged]);

  const edit = useCallback(async (op: EditOp) => {
    try {
      apply(await invoke<EditResult>("edit_keymap", { keymapPath: keymap.path, op }));
      setNotice(null);
      if (op.op !== "set_binding" && op.op !== "rename_layer") setSelected(null);
      return true;
    } catch (e) {
      setError(String(e));
      return false;
    }
  }, [apply, keymap.path]);

  const step = useCallback(async (which: "undo_keymap" | "redo_keymap") => {
    try {
      apply(await invoke<EditResult>(which, { keymapPath: keymap.path }));
    } catch (e) {
      setError(String(e));
    }
  }, [apply, keymap.path]);

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (!(e.ctrlKey || e.metaKey) || (e.target as HTMLElement).tagName === "INPUT") return;
      if (e.key.toLowerCase() === "z" && !e.shiftKey) { e.preventDefault(); void step("undo_keymap"); }
      if ((e.key.toLowerCase() === "z" && e.shiftKey) || e.key.toLowerCase() === "y") { e.preventDefault(); void step("redo_keymap"); }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [step]);

  const layer = keymap.layers[Math.min(layerIndex, keymap.layers.length - 1)];
  const labels = useMemo(() => layer?.bindings.map((b) => describe(b, catalog)) ?? [], [layer, catalog]);
  const baseLabels = useMemo(() => keymap.layers[0]?.bindings.map((b) => describe(b, catalog)) ?? [], [keymap, catalog]);
  if (!layer) return <p className="warning">This keymap has no layers yet.</p>;

  const fitsBoard = geometry.length > 0 && layer.bindings.length === geometry.length;
  const width = Math.max(0, ...geometry.map((g) => g.x + g.w)) / 100;
  const height = Math.max(0, ...geometry.map((g) => g.y + g.h)) / 100 + 0.4;
  const last = keymap.layers.length - 1;

  return (
    <section className="keymap-editor">
      <div className="editor-head">
        <h2>Keymap <span className="muted">· {keymap.fileName}</span></h2>
        <div className="row">
          <button onClick={() => step("undo_keymap")} disabled={!canUndo} title="Undo (Ctrl/⌘+Z)">↶ Undo</button>
          <button onClick={() => step("redo_keymap")} disabled={!canRedo} title="Redo (Ctrl/⌘+Shift+Z)">↷ Redo</button>
        </div>
      </div>
      {notice && <p className="note">{notice}</p>}
      <nav className="tabs">
        {keymap.layers.map((l) => (
          <button key={l.index} className={l.index === layer.index ? "active" : ""} onClick={() => { setLayerIndex(l.index); setSelected(null); setRenaming(null); }}>
            {l.title}
          </button>
        ))}
        {adding === null ? (
          <button className="add" onClick={() => setAdding("")} title="Add a layer">＋ Layer</button>
        ) : (
          <form className="inline" onSubmit={async (e) => {
            e.preventDefault();
            if (await edit({ op: "add_layer", name: adding })) { setLayerIndex(keymap.layers.length); setAdding(null); }
          }}>
            <input autoFocus placeholder="New layer name" value={adding} onChange={(e) => setAdding(e.target.value)} />
            <button type="submit">Add</button>
            <button type="button" onClick={() => setAdding(null)}>Cancel</button>
          </form>
        )}
      </nav>
      <div className="layer-tools">
        {renaming === null ? (
          <>
            <button onClick={() => setRenaming(layer.title)}>Rename</button>
            <button disabled={layer.index === 0} onClick={async () => { if (await edit({ op: "move_layer", from: layer.index, to: layer.index - 1 })) setLayerIndex(layer.index - 1); }} title="Move this layer one place left">◀ Move</button>
            <button disabled={layer.index === last} onClick={async () => { if (await edit({ op: "move_layer", from: layer.index, to: layer.index + 1 })) setLayerIndex(layer.index + 1); }} title="Move this layer one place right">Move ▶</button>
            <button className="danger" disabled={keymap.layers.length === 1} onClick={async () => {
              if (window.confirm(`Delete the layer “${layer.title}”? (You can undo this.)`)) {
                if (await edit({ op: "remove_layer", layer: layer.index })) setLayerIndex(Math.max(0, layer.index - 1));
              }
            }}>Delete layer</button>
            {layer.index === 0 && <span className="muted">This is the base layer — active when no other layer is.</span>}
          </>
        ) : (
          <form className="inline" onSubmit={async (e) => {
            e.preventDefault();
            if (await edit({ op: "rename_layer", layer: layer.index, name: renaming })) setRenaming(null);
          }}>
            <input autoFocus value={renaming} onChange={(e) => setRenaming(e.target.value)} />
            <button type="submit">Save name</button>
            <button type="button" onClick={() => setRenaming(null)}>Cancel</button>
          </form>
        )}
      </div>
      {error && <p className="error">{error}</p>}
      {layer.readOnlyReason && (
        <p className="warning">This layer is written by hand in a way the app can't edit yet ({layer.readOnlyReason}). You can still edit it in a text editor.</p>
      )}
      {fitsBoard ? (
        <div className="board" style={{ width: width * UNIT, height: height * UNIT }}>
          {geometry.map((g, i) => {
            const label = labels[i] ?? { tap: "" };
            return (
              <button
                key={i}
                className={`key${label.muted ? " muted" : ""}${selected === i ? " selected" : ""}`}
                title={layer.bindings[i]}
                disabled={!!layer.readOnlyReason}
                onClick={() => setSelected(i)}
                style={{
                  left: (g.x / 100) * UNIT,
                  top: (g.y / 100) * UNIT,
                  width: (g.w / 100) * UNIT - 4,
                  height: (g.h / 100) * UNIT - 4,
                  transform: g.r ? `rotate(${g.r / 100}deg)` : undefined,
                  transformOrigin: g.r ? `${((g.rx - g.x) / 100) * UNIT}px ${((g.ry - g.y) / 100) * UNIT}px` : undefined,
                }}
              >
                <span className="tap">{label.tap}</span>
                {label.hold && <span className="hold">{label.hold}</span>}
              </button>
            );
          })}
        </div>
      ) : (
        <ol className="binding-list">{layer.bindings.map((b, i) => <li key={i}><button className="link" onClick={() => setSelected(i)}><code>{b}</code></button></li>)}</ol>
      )}
      {selected !== null && catalog && (
        <KeyPicker
          key={`${layer.index}-${selected}-${layer.bindings[selected]}`}
          raw={layer.bindings[selected]}
          cat={catalog}
          title={`Key ${selected + 1} on “${layer.title}”`}
          onCancel={() => setSelected(null)}
          onSave={async (binding) => { if (await edit({ op: "set_binding", layer: layer.index, position: selected, binding })) setSelected(null); }}
        />
      )}
      {catalog && (
        <BehaviorsPanel definitions={keymap.definitions} cat={catalog} geometry={geometry.length === keymap.layers[0]?.bindings.length ? geometry : []} baseLabels={baseLabels} edit={edit} />
      )}
      {keymap.parseProblems.length > 0 && (
        <details>
          <summary>Parts of this file the app didn't understand (kept unchanged)</summary>
          <ul>{keymap.parseProblems.map((p) => <li key={p}>{p}</li>)}</ul>
        </details>
      )}
    </section>
  );
}
