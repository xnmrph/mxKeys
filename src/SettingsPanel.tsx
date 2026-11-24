import { useCallback, useEffect, useRef, useState, type ReactElement } from "react";
import { invoke } from "@tauri-apps/api/core";
import type { NewValue, SettingsOp, SettingsPage, SettingView, Unit } from "./types";

/** Factor from the shown unit to the stored value. */
const SCALE: Record<Unit, number> = { seconds: 1000, minutes: 60000, milliseconds: 1, percent: 1 };
const UNIT_LABEL: Record<Unit, string> = { seconds: "seconds", minutes: "minutes", milliseconds: "ms", percent: "%" };

function shown(unit: Unit, stored: number): string {
  return String(Math.round((stored / SCALE[unit]) * 1000) / 1000);
}

export function SettingsPanel({ confPath, onChanged }: { confPath: string; onChanged: () => void }) {
  const [page, setPage] = useState<SettingsPage | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  const [advanced, setAdvanced] = useState(false);
  const version = useRef<string | null>(null);

  const show = useCallback((p: SettingsPage) => {
    version.current = p.version;
    setPage(p);
  }, []);

  useEffect(() => {
    invoke<SettingsPage>("settings_page", { confPath }).then(show).catch((e) => setError(String(e)));
  }, [confPath, show]);

  // Notice edits made outside the app and reload.
  useEffect(() => {
    let stop = false;
    const t = setInterval(async () => {
      try {
        const v = await invoke<string>("file_version", { path: confPath });
        if (!stop && version.current && v !== version.current) {
          await invoke("forget_history", { keymapPath: confPath });
          show(await invoke<SettingsPage>("settings_page", { confPath }));
          setNotice("Your settings file changed outside mxKeys, so it was reloaded. (Undo history was cleared.)");
          onChanged();
        }
      } catch { /* file temporarily missing */ }
    }, 2000);
    return () => { stop = true; clearInterval(t); };
  }, [confPath, show, onChanged]);

  const run = useCallback(async (cmd: string, args: Record<string, unknown>) => {
    try {
      show(await invoke<SettingsPage>(cmd, { confPath, ...args }));
      setError(null);
      setNotice(null);
      onChanged();
      return true;
    } catch (e) {
      setError(String(e));
      return false;
    }
  }, [confPath, show, onChanged]);

  const edit = useCallback((op: SettingsOp) => run("edit_settings", { op }), [run]);
  const set = useCallback((id: string, value: NewValue) => edit({ op: "set", id, value }), [edit]);

  if (!page) return error ? <p className="error">{error}</p> : null;
  const byId = new Map(page.groups.flatMap((g) => g.settings).map((s) => [s.def.id, s]));

  return (
    <section className="settings-panel">
      <div className="editor-head">
        <span className="mono muted file-name">{page.fileName}</span>
        <div className="row">
          <label className="check"><input type="checkbox" checked={advanced} onChange={(e) => setAdvanced(e.target.checked)} /> Advanced</label>
          <button onClick={() => run("undo_settings", {})} disabled={!page.canUndo}>↶ Undo</button>
          <button onClick={() => run("redo_settings", {})} disabled={!page.canRedo}>↷ Redo</button>
        </div>
      </div>
      <p className="muted">These are built into the firmware: after changing something, build and install the firmware again.</p>
      {notice && <p className="note">{notice}</p>}
      {error && <p className="error">{error}</p>}
      {page.duplicates.length > 0 && (
        <p className="warning">Some options are set more than once ({page.duplicates.join(", ")}); the last line wins. mxKeys edits that last line.</p>
      )}
      <div className="setting-groups">
        {page.groups.map((g) => (
          <div key={g.id} className="setting-group">
            <h3>{g.title}</h3>
            {g.settings.map((s) => (
              <SettingRow key={s.def.id} s={s} advanced={advanced} parent={s.def.requires ? byId.get(s.def.requires) : undefined} onSet={(v) => set(s.def.id, v)} />
            ))}
          </div>
        ))}
      </div>
      {advanced && <RawOptions page={page} edit={edit} />}
    </section>
  );
}

function SettingRow({ s, advanced, parent, onSet }: { s: SettingView; advanced: boolean; parent?: SettingView; onSet: (v: NewValue) => Promise<boolean> }) {
  const k = s.def.kind;
  const options = k.type === "toggle" ? [k.option, ...k.also] : k.type === "choice" ? k.options.map((o) => o.option!) : [k.option];
  let control: ReactElement;
  switch (k.type) {
    case "toggle":
      control = (
        <label className="switch">
          <input type="checkbox" checked={s.value.type === "bool" && s.value.value} onChange={(e) => onSet({ type: "bool", value: e.target.checked })} />
          {s.value.type === "bool" && s.value.value ? "On" : "Off"}
        </label>
      );
      break;
    case "number":
      control = <NumberInput key={`${s.value.value}`} stored={s.value.value as number} unit={k.unit} min={k.min} max={k.max} onSave={(v) => onSet({ type: "int", value: v })} />;
      break;
    case "text":
      control = <TextInput key={`${s.value.value}`} value={s.value.value as string} maxBytes={k.max_bytes} onSave={(v) => onSet({ type: "text", value: v })} />;
      break;
    case "choice":
    case "pick":
      control = (
        <select value={s.value.value as number} onChange={(e) => onSet({ type: "choice", value: Number(e.target.value) })}>
          {k.options.map((o, i) => <option key={i} value={i}>{o.label}{i === k.default ? " (default)" : ""}</option>)}
        </select>
      );
      break;
  }
  return (
    <div className={`setting${s.applies ? "" : " inactive"}`}>
      <div className="setting-main">
        <div className="setting-label">{s.def.label}</div>
        <div className="setting-control">
          {control}
          {s.isDefault ? <span className="badge">default</span> : (
            <button className="link" onClick={() => onSet({ type: "default" })} title="Remove your value so ZMK's default applies (the line is commented out, not deleted)">Use default</button>
          )}
        </div>
      </div>
      <div className="setting-help muted">
        {s.def.help}
        {!s.applies && parent && <> Only used when “{parent.def.label}” is on.</>}
      </div>
      {s.problem && <div className="warning">{s.problem}</div>}
      {advanced && (
        <div className="setting-raw muted">
          {options.map((o) => <code key={o}>{o}</code>)}
          {s.lines.length > 0 && <span> · line {s.lines.join(", ")}</span>}
          {k.type === "number" && <span> · default {k.default}</span>}
        </div>
      )}
    </div>
  );
}

function NumberInput({ stored, unit, min, max, onSave }: { stored: number; unit: Unit; min: number; max: number; onSave: (v: number) => Promise<boolean> }) {
  const [text, setText] = useState(shown(unit, stored));
  const save = () => {
    const n = Number(text.replace(",", "."));
    if (!Number.isFinite(n)) { setText(shown(unit, stored)); return; }
    const v = Math.round(n * SCALE[unit]);
    if (v !== stored) void onSave(v).then((ok) => { if (!ok) setText(shown(unit, stored)); });
  };
  return (
    <span className="number">
      <input inputMode="decimal" value={text} onChange={(e) => setText(e.target.value)} onBlur={save} onKeyDown={(e) => { if (e.key === "Enter") save(); }}
        title={`${shown(unit, min)}–${shown(unit, max)} ${UNIT_LABEL[unit]}`} />
      {UNIT_LABEL[unit]}
    </span>
  );
}

function TextInput({ value, maxBytes, onSave }: { value: string; maxBytes: number; onSave: (v: string) => Promise<boolean> }) {
  const [text, setText] = useState(value);
  const bytes = new TextEncoder().encode(text).length;
  const save = () => { if (text !== value) void onSave(text).then((ok) => { if (!ok) setText(value); }); };
  return (
    <span className="text">
      <input value={text} onChange={(e) => setText(e.target.value)} onBlur={save} onKeyDown={(e) => { if (e.key === "Enter") save(); }} />
      <span className={bytes > maxBytes ? "error" : "muted"}>{bytes}/{maxBytes}</span>
    </span>
  );
}

function RawOptions({ page, edit }: { page: SettingsPage; edit: (op: SettingsOp) => Promise<boolean> }) {
  const [name, setName] = useState("");
  const [value, setValue] = useState("");
  return (
    <div className="setting-group raw-options">
      <h3>Other options in {page.fileName}</h3>
      {page.other.length === 0 ? <p className="muted">None — everything in the file is shown above.</p> : (
        <table>
          <thead><tr><th>line</th><th>option</th><th>value</th><th></th></tr></thead>
          <tbody>
            {page.other.map((s) => (
              <tr key={`${s.name}-${s.line}`} className={s.state === "commented_out" ? "muted" : ""}>
                <td>{s.line}</td><td><code>{s.name}</code></td><td><code>{s.value}</code></td>
                <td>
                  {s.state === "commented_out"
                    ? <button className="link" onClick={() => edit({ op: "set_raw", name: s.name, value: s.value })}>Switch on</button>
                    : <button className="link" onClick={() => edit({ op: "remove_raw", name: s.name })}>Comment out</button>}
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      )}
      <form className="inline" onSubmit={async (e) => {
        e.preventDefault();
        if (await edit({ op: "set_raw", name, value })) { setName(""); setValue(""); }
      }}>
        <input placeholder="CONFIG_NAME" value={name} onChange={(e) => setName(e.target.value)} />
        <input placeholder='y, 42 or "text"' value={value} onChange={(e) => setValue(e.target.value)} />
        <button type="submit" disabled={!name.trim() || !value.trim()}>Set option</button>
      </form>
      <p className="muted">Any Kconfig option from ZMK or Zephyr. mxKeys can't check these, so a typo only shows up when building.</p>
    </div>
  );
}
