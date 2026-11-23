import { useCallback, useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { open } from "@tauri-apps/plugin-dialog";
import type { FirmwareSummary, KeymapSummary, RepoSummary, TargetSummary } from "./types";
import { CloneForm } from "./CloneForm";
import { HardwareSetup } from "./HardwareSetup";
import { BuildPanel } from "./BuildPanel";
import type { HardwareProfile } from "./types";
import { KeymapEditor } from "./KeymapEditor";
import { SettingsPanel } from "./SettingsPanel";
import { HistoryPanel } from "./HistoryPanel";
import { PicturesPanel } from "./PicturesPanel";

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
      {!repo ? <Welcome onCloned={setRepo} onOpen={loadFolder} /> : (
        <RepoView
          repo={repo}
          onKeymapChanged={(k) => setRepo((r) => (r ? replaceKeymap(r, k) : r))}
          onReload={async () => {
            try { setRepo(await invoke<RepoSummary>("open_config_repo", { path: repo.root })); } catch (e) { setError(String(e)); }
          }}
        />
      )}
    </main>
  );
}

function replaceKeymap(repo: RepoSummary, k: KeymapSummary): RepoSummary {
  return { ...repo, keymaps: repo.keymaps.map((old) => (old.path === k.path ? k : old)) };
}

function Welcome({ onCloned, onOpen }: { onCloned: (r: RepoSummary) => void; onOpen: (dir: string) => void }) {
  const [recent, setRecent] = useState<string[]>([]);
  useEffect(() => { invoke<string[]>("recent_folders").then(setRecent).catch(() => {}); }, []);
  return (
    <section className="welcome">
      <h2>Let's set up your keyboard</h2>
      <p>
        Your keyboard's settings live in a <em>config repository</em> (often called <em>zmk-config</em>)
        on GitHub, Gitea or any other git server. Paste its address to download it, or use
        <strong> Open config folder…</strong> above if it's already on this computer. Nothing is
        changed until you edit something.
      </p>
      {recent.length > 0 && (
        <div className="recent">
          <h3>Open again</h3>
          <ul>
            {recent.map((r) => (
              <li key={r}><button className="link" onClick={() => onOpen(r)}>{r.split(/[\\/]/).filter(Boolean).pop()}</button> <span className="muted small">{r}</span></li>
            ))}
          </ul>
        </div>
      )}
      <CloneForm onCloned={onCloned} />
    </section>
  );
}

function RepoView({ repo, onKeymapChanged, onReload }: {
  repo: RepoSummary;
  onKeymapChanged: (k: KeymapSummary) => void;
  onReload: () => Promise<void>;
}) {
  const keymap = repo.keymaps[0];
  const [profile, setProfile] = useState<HardwareProfile | null>(null);
  const onProfile = useCallback((p: HardwareProfile | null) => setProfile(p), []);
  // Bumped on every edit so the Firmware panel can say a rebuild is needed.
  const [changes, setChanges] = useState(0);
  const bump = useCallback(() => setChanges((n) => n + 1), []);
  return (
    <>
      <p className="muted path">{repo.root}</p>
      <HardwareSetup repoRoot={repo.root} onProfile={onProfile} />
      {profile && <BuildPanel repoRoot={repo.root} autoSetup changes={changes} />}
      <HistoryPanel repoRoot={repo.root} changes={changes} onFilesChanged={() => { void onReload(); bump(); }} />
      {repo.warnings.map((w) => (
        <p key={w} className="warning">{w}</p>
      ))}
      {keymap && <KeymapEditor keymap={keymap} onChanged={(k) => { onKeymapChanged(k); bump(); }} onReload={onReload} />}
      {repo.firmware && <Firmware firmware={repo.firmware} />}
      {repo.confFiles.map((c) => <SettingsPanel key={c.path} confPath={c.path} onChanged={bump} />)}
      {profile?.display === "nice_view" && <PicturesPanel repoRoot={repo.root} onChanged={bump} />}
      {repo.build && <BuildTargets targets={repo.build.targets} />}
    </>
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
