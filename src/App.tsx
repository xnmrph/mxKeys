import { useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { open } from "@tauri-apps/plugin-dialog";
import type { KeymapSummary, RepoSummary, TargetSummary } from "./types";
import { CORNE_6COL } from "./corneLayout";
import { bindingLabel } from "./bindingLabel";

const UNIT = 54; // px per key unit

export default function App() {
  const [repo, setRepo] = useState<RepoSummary | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  async function openFolder() {
    setError(null);
    const dir = await open({ directory: true, title: "Choose your zmk-config folder" });
    if (typeof dir !== "string") return;
    setBusy(true);
    try {
      setRepo(await invoke<RepoSummary>("open_config_repo", { path: dir }));
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  }

  return (
    <main className="app">
      <header className="topbar">
        <h1>mxKeys</h1>
        <button onClick={openFolder} disabled={busy}>
          {repo ? "Open another folder…" : "Open config folder…"}
        </button>
      </header>
      {error && <p className="error" role="alert">{error}</p>}
      {!repo ? <Welcome /> : <RepoView repo={repo} onKeymapChanged={(k) => setRepo(replaceKeymap(repo, k))} />}
    </main>
  );
}

function replaceKeymap(repo: RepoSummary, k: KeymapSummary): RepoSummary {
  return { ...repo, keymaps: repo.keymaps.map((old) => (old.path === k.path ? k : old)) };
}

function Welcome() {
  return (
    <section className="welcome">
      <h2>Let's set up your keyboard</h2>
      <p>
        Pick the folder with your keyboard's configuration (often called <em>zmk-config</em>). It
        has a <code>config</code> folder inside. Nothing is changed until you edit something.
      </p>
      <p className="muted">Cloning from GitHub, Gitea or any git server is coming next.</p>
    </section>
  );
}

function RepoView({ repo, onKeymapChanged }: { repo: RepoSummary; onKeymapChanged: (k: KeymapSummary) => void }) {
  const keymap = repo.keymaps[0];
  return (
    <>
      <p className="muted path">{repo.root}</p>
      {repo.warnings.map((w) => (
        <p key={w} className="warning">{w}</p>
      ))}
      {keymap && <KeymapView keymap={keymap} onChanged={onKeymapChanged} />}
      {repo.build && <BuildTargets targets={repo.build.targets} />}
    </>
  );
}

function KeymapView({ keymap, onChanged }: { keymap: KeymapSummary; onChanged: (k: KeymapSummary) => void }) {
  const [layerIndex, setLayerIndex] = useState(0);
  const [selected, setSelected] = useState<number | null>(null);
  const [draft, setDraft] = useState("");
  const [error, setError] = useState<string | null>(null);
  const layer = keymap.layers[Math.min(layerIndex, keymap.layers.length - 1)];
  if (!layer) return <p className="warning">This keymap has no layers yet.</p>;

  const fitsCorne = layer.bindings.length === CORNE_6COL.length;
  const width = Math.max(...CORNE_6COL.map((g) => g.x + g.w)) / 100;
  const height = Math.max(...CORNE_6COL.map((g) => g.y + g.h)) / 100 + 0.4;

  function select(i: number) {
    setSelected(i);
    setDraft(layer.bindings[i]);
    setError(null);
  }

  async function save() {
    if (selected === null) return;
    try {
      const updated = await invoke<KeymapSummary>("set_binding", {
        keymapPath: keymap.path,
        layer: layer.index,
        position: selected,
        binding: draft,
      });
      onChanged(updated);
      setError(null);
    } catch (e) {
      setError(String(e));
    }
  }

  return (
    <section>
      <h2>Keymap <span className="muted">· {keymap.fileName}</span></h2>
      <nav className="tabs">
        {keymap.layers.map((l) => (
          <button key={l.index} className={l.index === layer.index ? "active" : ""} onClick={() => { setLayerIndex(l.index); setSelected(null); }}>
            {l.title}
          </button>
        ))}
      </nav>
      {layer.readOnlyReason && (
        <p className="warning">This layer is written by hand in a way the app can't edit yet ({layer.readOnlyReason}). You can still edit it in a text editor.</p>
      )}
      {fitsCorne ? (
        <div className="board" style={{ width: width * UNIT, height: height * UNIT }}>
          {CORNE_6COL.map((g, i) => {
            const label = bindingLabel(layer.bindings[i] ?? "");
            return (
              <button
                key={i}
                className={`key${label.muted ? " muted" : ""}${selected === i ? " selected" : ""}`}
                title={layer.bindings[i]}
                disabled={!!layer.readOnlyReason}
                onClick={() => select(i)}
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
        <ol className="binding-list">{layer.bindings.map((b, i) => <li key={i}><code>{b}</code></li>)}</ol>
      )}
      {selected !== null && (
        <div className="editor">
          <label>
            Key {selected + 1} <span className="muted">(advanced: ZMK binding)</span>
            <input value={draft} onChange={(e) => setDraft(e.target.value)} onKeyDown={(e) => e.key === "Enter" && save()} spellCheck={false} />
          </label>
          <button onClick={save}>Save</button>
          {error && <p className="error">{error}</p>}
        </div>
      )}
      <p className="muted">
        {keymap.behaviors} custom behaviors · {keymap.macros} macros · {keymap.combos} combos · {keymap.conditionalLayers} conditional layers
      </p>
      {keymap.parseProblems.length > 0 && (
        <details>
          <summary>Parts of this file the app didn't understand (kept unchanged)</summary>
          <ul>{keymap.parseProblems.map((p) => <li key={p}>{p}</li>)}</ul>
        </details>
      )}
    </section>
  );
}

function describeTarget(t: TargetSummary): string {
  if (t.settingsReset) return "Settings reset (fixes pairing problems)";
  const half = t.shields.find((s) => s.endsWith("_left")) ? "Left half" : t.shields.find((s) => s.endsWith("_right")) ? "Right half" : t.shields[0] ?? t.board;
  const extras = [t.niceView ? "with screen" : "without screen", t.studio ? "live editing on" : null].filter(Boolean);
  return `${half} — ${extras.join(", ")}`;
}

function BuildTargets({ targets }: { targets: TargetSummary[] }) {
  return (
    <section>
      <h2>Firmware your config builds</h2>
      <ul className="targets">
        {targets.map((t) => (
          <li key={t.artifactName}>{describeTarget(t)}</li>
        ))}
      </ul>
      <details>
        <summary>Advanced</summary>
        <table>
          <thead><tr><th>board</th><th>shield</th><th>snippet</th></tr></thead>
          <tbody>
            {targets.map((t) => (
              <tr key={t.artifactName}><td><code>{t.board}</code></td><td><code>{t.shields.join(" ")}</code></td><td><code>{t.snippets.join(" ")}</code></td></tr>
            ))}
          </tbody>
        </table>
      </details>
    </section>
  );
}
