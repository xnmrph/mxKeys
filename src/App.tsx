import { useCallback, useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { open } from "@tauri-apps/plugin-dialog";
import type { ConfSummary, FirmwareSummary, KeymapSummary, RepoSummary, TargetSummary } from "./types";
import { CloneForm } from "./CloneForm";
import { HardwareSetup } from "./HardwareSetup";
import { BuildPanel } from "./BuildPanel";
import type { HardwareProfile, KeyGeom } from "./types";
import { bindingLabel } from "./bindingLabel";

const UNIT = 54; // px per key unit

export default function App() {
  const [repo, setRepo] = useState<RepoSummary | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  // `mxkeys <folder>` opens that config folder straight away.
  useEffect(() => {
    invoke<string | null>("startup_path")
      .then((p) => { if (p) void loadFolder(p); })
      .catch(() => {});
  }, []);

  async function loadFolder(dir: string) {
    setBusy(true);
    setError(null);
    try {
      setRepo(await invoke<RepoSummary>("open_config_repo", { path: dir }));
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  }

  async function openFolder() {
    setError(null);
    const dir = await open({ directory: true, title: "Choose your zmk-config folder" });
    if (typeof dir === "string") await loadFolder(dir);
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
      {!repo ? <Welcome onCloned={setRepo} /> : <RepoView repo={repo} onKeymapChanged={(k) => setRepo(replaceKeymap(repo, k))} />}
    </main>
  );
}

function replaceKeymap(repo: RepoSummary, k: KeymapSummary): RepoSummary {
  return { ...repo, keymaps: repo.keymaps.map((old) => (old.path === k.path ? k : old)) };
}

function Welcome({ onCloned }: { onCloned: (r: RepoSummary) => void }) {
  return (
    <section className="welcome">
      <h2>Let's set up your keyboard</h2>
      <p>
        Your keyboard's settings live in a <em>config repository</em> (often called <em>zmk-config</em>)
        on GitHub, Gitea or any other git server. Paste its address to download it, or use
        <strong> Open config folder…</strong> above if it's already on this computer. Nothing is
        changed until you edit something.
      </p>
      <CloneForm onCloned={onCloned} />
    </section>
  );
}

function RepoView({ repo, onKeymapChanged }: { repo: RepoSummary; onKeymapChanged: (k: KeymapSummary) => void }) {
  const keymap = repo.keymaps[0];
  const [profile, setProfile] = useState<HardwareProfile | null>(null);
  const onProfile = useCallback((p: HardwareProfile | null) => setProfile(p), []);
  return (
    <>
      <p className="muted path">{repo.root}</p>
      <HardwareSetup repoRoot={repo.root} onProfile={onProfile} />
      {profile && <BuildPanel repoRoot={repo.root} autoSetup />}
      {repo.warnings.map((w) => (
        <p key={w} className="warning">{w}</p>
      ))}
      {keymap && <KeymapView keymap={keymap} onChanged={onKeymapChanged} />}
      {repo.firmware && <Firmware firmware={repo.firmware} />}
      {repo.confFiles.map((c) => <Settings key={c.path} conf={c} />)}
      {repo.build && <BuildTargets targets={repo.build.targets} />}
    </>
  );
}

function KeymapView({ keymap, onChanged }: { keymap: KeymapSummary; onChanged: (k: KeymapSummary) => void }) {
  const [layerIndex, setLayerIndex] = useState(0);
  const [selected, setSelected] = useState<number | null>(null);
  const [draft, setDraft] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [geometry, setGeometry] = useState<KeyGeom[]>([]);
  useEffect(() => {
    invoke<KeyGeom[]>("physical_layout").then(setGeometry).catch(() => setGeometry([]));
  }, []);
  const layer = keymap.layers[Math.min(layerIndex, keymap.layers.length - 1)];
  if (!layer) return <p className="warning">This keymap has no layers yet.</p>;

  const fitsCorne = geometry.length > 0 && layer.bindings.length === geometry.length;
  const width = Math.max(0, ...geometry.map((g) => g.x + g.w)) / 100;
  const height = Math.max(0, ...geometry.map((g) => g.y + g.h)) / 100 + 0.4;

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
          {geometry.map((g, i) => {
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

function describeRevision(f: FirmwareSummary): string {
  if (!f.zmkRevision) return "unknown";
  switch (f.zmkRevisionKind) {
    case "branch": return `latest development version (follows “${f.zmkRevision}”)`;
    case "version": return `release ${f.zmkRevision}`;
    case "commit": return `fixed version ${f.zmkRevision.slice(0, 7)}`;
    default: return f.zmkRevision;
  }
}

function Firmware({ firmware }: { firmware: FirmwareSummary }) {
  return (
    <section>
      <h2>ZMK version</h2>
      <p>Your config builds against the <strong>{describeRevision(firmware)}</strong> of ZMK.</p>
      {firmware.extraModules.length > 0 && (
        <p>
          Add-ons: {firmware.extraModules.map((m) => m.name).join(", ")}
        </p>
      )}
      <details>
        <summary>Advanced</summary>
        <table>
          <thead><tr><th>project</th><th>revision</th><th>url</th></tr></thead>
          <tbody>
            <tr><td><code>zmk</code></td><td><code>{firmware.zmkRevision}</code></td><td><code>{firmware.zmkUrl}</code></td></tr>
            {firmware.extraModules.map((m) => (
              <tr key={m.name}><td><code>{m.name}</code></td><td><code>{m.revision}</code></td><td><code>{m.url}</code></td></tr>
            ))}
          </tbody>
        </table>
      </details>
    </section>
  );
}

const FRIENDLY_SETTINGS: Record<string, string> = {
  CONFIG_ZMK_KEYBOARD_NAME: "Keyboard name (shown in Bluetooth)",
  CONFIG_ZMK_SLEEP: "Deep sleep when idle",
  CONFIG_ZMK_IDLE_TIMEOUT: "Idle after (ms)",
  CONFIG_ZMK_IDLE_SLEEP_TIMEOUT: "Deep sleep after (ms)",
  CONFIG_ZMK_STUDIO: "Live editing (ZMK Studio)",
  CONFIG_ZMK_DISPLAY: "Screen",
  CONFIG_ZMK_POINTING: "Mouse keys",
  CONFIG_ZMK_RGB_UNDERGLOW: "RGB underglow",
  CONFIG_BT_CTLR_TX_PWR_PLUS_8: "Stronger Bluetooth signal (+8 dBm)",
};

function friendlyValue(v: string): string {
  if (v === "y") return "on";
  if (v === "n") return "off";
  return v.replace(/^"(.*)"$/, "$1");
}

function Settings({ conf }: { conf: ConfSummary }) {
  const active = conf.settings.filter((s) => s.state !== "commented_out");
  const off = conf.settings.filter((s) => s.state === "commented_out");
  return (
    <section>
      <h2>Settings <span className="muted">· {conf.fileName}</span></h2>
      {active.length === 0 ? <p className="muted">No settings changed from ZMK's defaults.</p> : (
        <ul className="settings">
          {active.map((s) => (
            <li key={`${s.name}-${s.line}`}>
              {FRIENDLY_SETTINGS[s.name] ?? s.name}: <strong>{friendlyValue(s.value)}</strong>
            </li>
          ))}
        </ul>
      )}
      <details>
        <summary>Advanced</summary>
        <table>
          <thead><tr><th>line</th><th>option</th><th>value</th><th></th></tr></thead>
          <tbody>
            {conf.settings.map((s) => (
              <tr key={`${s.name}-${s.line}`}>
                <td>{s.line}</td><td><code>{s.name}</code></td><td><code>{s.value}</code></td>
                <td className="muted">{s.state === "commented_out" ? "commented out" : s.state === "not_set" ? "is not set" : ""}</td>
              </tr>
            ))}
          </tbody>
        </table>
        {off.length > 0 && <p className="muted">{off.length} option(s) are commented out and have no effect.</p>}
      </details>
    </section>
  );
}
