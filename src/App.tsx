import { useCallback, useEffect, useRef, useState } from "react";
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
import { LivePanel, type LiveInfo } from "./LivePanel";
import { Icon } from "./icons";
import { withCode } from "./text";

type Tab = "keyboard" | "keymap" | "screen" | "settings" | "install";
const NO_LIVE: LiveInfo = { connected: false, ready: false, locked: false, name: null, unlockHint: null, unsaved: false };

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

  const reload = useCallback(async () => {
    if (!repo) return;
    try { setRepo(await invoke<RepoSummary>("open_config_repo", { path: repo.root })); } catch (e) { setError(String(e)); }
  }, [repo]);

  if (!repo) {
    return (
      <main className="welcome-page">
        <Welcome onCloned={setRepo} onOpen={loadFolder} onBrowse={openFolder} busy={busy} error={error} />
      </main>
    );
  }
  return (
    <RepoView
      repo={repo}
      error={error}
      busy={busy}
      onOpenAnother={openFolder}
      onKeymapChanged={(k) => setRepo((r) => (r ? replaceKeymap(r, k) : r))}
      onReload={reload}
    />
  );
}

type Theme = "system" | "dark" | "light";
function applyTheme(t: Theme) {
  if (t === "system") document.documentElement.removeAttribute("data-theme");
  else document.documentElement.setAttribute("data-theme", t);
}
function storedTheme(): Theme {
  try { const t = localStorage.getItem("theme"); if (t === "dark" || t === "light") return t; } catch { /* storage unavailable */ }
  return "system";
}
applyTheme(storedTheme());

function ThemeSwitch() {
  const [t, setT] = useState<Theme>(storedTheme);
  const pick = (v: Theme) => {
    setT(v);
    applyTheme(v);
    try { localStorage.setItem("theme", v); } catch { /* storage unavailable */ }
  };
  return (
    <div className="theme-switch" role="group" aria-label="Appearance">
      {(["system", "dark", "light"] as Theme[]).map((v) => (
        <button key={v} className={t === v ? "on" : ""} onClick={() => pick(v)}>{v === "system" ? "Auto" : v === "dark" ? "Dark" : "Light"}</button>
      ))}
    </div>
  );
}

function Wordmark({ big }: { big?: boolean }) {
  return <div className={`brand${big ? " big" : ""}`} aria-label="mxKeys"><span className="brand-mx">mx</span><span className="brand-keys">Keys</span></div>;
}

function replaceKeymap(repo: RepoSummary, k: KeymapSummary): RepoSummary {
  return { ...repo, keymaps: repo.keymaps.map((old) => (old.path === k.path ? k : old)) };
}

function Welcome({ onCloned, onOpen, onBrowse, busy, error }: {
  onCloned: (r: RepoSummary) => void; onOpen: (dir: string) => void; onBrowse: () => void; busy: boolean; error: string | null;
}) {
  const [recent, setRecent] = useState<string[]>([]);
  useEffect(() => { invoke<string[]>("recent_folders").then(setRecent).catch(() => {}); }, []);
  return (
    <section className="welcome">
      <Wordmark big />
      <h1>Let's set up your keyboard</h1>
      <p className="lead">
        Your keyboard's settings live in a <em>config repository</em> (often called <em>zmk-config</em>)
        on GitHub, Gitea or any other git server. Download it, or open it if it's already on this
        computer. Nothing is changed until you edit something.
      </p>
      {error && <p className="error" role="alert">{error}</p>}
      {recent.length > 0 && (
        <div className="card recent">
          <h3 className="overline">Open again</h3>
          <ul>
            {recent.map((r) => (
              <li key={r}>
                <button className="recent-item" onClick={() => onOpen(r)} disabled={busy}>
                  <Icon name="folder" />
                  <span className="recent-name">{r.split(/[\\/]/).filter(Boolean).pop()}</span>
                  <span className="mono muted recent-path">{r}</span>
                </button>
              </li>
            ))}
          </ul>
        </div>
      )}
      <div className="card">
        <h3 className="overline">Download</h3>
        <CloneForm onCloned={onCloned} />
      </div>
      <button onClick={onBrowse} disabled={busy}><Icon name="folder" size={18} /> Open a folder on this computer…</button>
    </section>
  );
}

const TAB_INFO: Record<Tab, { title: string; icon: string; blurb: string }> = {
  keyboard: { title: "Keyboard", icon: "keyboard", blurb: "What you have, and the connection for live editing." },
  keymap: { title: "Keymap", icon: "keymap", blurb: "What every key does, on every layer — plus combos, macros and special keys." },
  screen: { title: "Screen", icon: "screen", blurb: "Your own pictures and GIFs on the nice!view screens. The preview shows exactly the dots the screen will show." },
  settings: { title: "Settings", icon: "settings", blurb: "Battery, Bluetooth, screen, lights and typing." },
  install: { title: "Install & save", icon: "install", blurb: "Build the firmware, put it on the keyboard, and keep a version of your changes." },
};

function RepoView({ repo, error, busy, onOpenAnother, onKeymapChanged, onReload }: {
  repo: RepoSummary;
  error: string | null;
  busy: boolean;
  onOpenAnother: () => void;
  onKeymapChanged: (k: KeymapSummary) => void;
  onReload: () => Promise<void>;
}) {
  const keymap = repo.keymaps[0];
  const [tab, setTabState] = useState<Tab>("keyboard");
  // Each tab keeps its own scroll position.
  const paneRef = useRef<HTMLElement>(null);
  const scrolls = useRef<Partial<Record<Tab, number>>>({});
  const setTab = useCallback((t: Tab) => {
    setTabState((old) => {
      if (paneRef.current) scrolls.current[old] = paneRef.current.scrollTop;
      return t;
    });
  }, []);
  useEffect(() => { if (paneRef.current) paneRef.current.scrollTop = scrolls.current[tab] ?? 0; }, [tab]);
  const [profile, setProfile] = useState<HardwareProfile | null>(null);
  // While the hardware is being re-checked, keep using the last confirmed answer
  // (so tabs and the build panel don't flicker away).
  const onProfile = useCallback((p: HardwareProfile | null) => { if (p) setProfile(p); }, []);
  // Bumped on every edit so the Firmware panel can say a rebuild is needed.
  const [changes, setChanges] = useState(0);
  const bump = useCallback(() => setChanges((n) => n + 1), []);
  const [live, setLive] = useState<LiveInfo>(NO_LIVE);
  const [flash, setFlash] = useState(0);
  const onBlocked = useCallback(() => setFlash((n) => n + 1), []);
  const fileChanged = useRef<(() => Promise<void>) | null>(null);
  const registerFileChanged = useCallback((f: () => Promise<void>) => { fileChanged.current = f; }, []);

  const hasScreen = profile?.display === "nice_view";
  const tabs: Tab[] = ["keyboard", "keymap", ...(hasScreen ? (["screen"] as Tab[]) : []), "settings", "install"];
  useEffect(() => { if (!tabs.includes(tab)) setTab("keyboard"); });
  const folderName = repo.root.split(/[\\/]/).filter(Boolean).pop();
  const info = TAB_INFO[tab];

  return (
    <div className="shell">
      <aside className="rail">
        <Wordmark />
        <nav className="rail-nav" aria-label="Sections">
          {tabs.map((t) => (
            <button key={t} className={`rail-item${t === tab ? " active" : ""}`} onClick={() => setTab(t)} aria-current={t === tab ? "page" : undefined}>
              <Icon name={TAB_INFO[t].icon} />
              <span>{TAB_INFO[t].title}</span>
              {t === "keymap" && live.locked && <span className="dot warn" title="Keyboard locked" />}
            </button>
          ))}
        </nav>
        <div className="rail-foot">
          <button className={`status-chip${live.ready ? " ok" : live.locked ? " warn" : ""}${flash ? " pulse-" + (flash % 2) : ""}`} onClick={() => setTab("keyboard")}>
            <span className={`dot ${live.ready ? "ok" : live.locked ? "warn" : "off"}`} />
            <span className="status-text">
              <span className="status-title">{live.connected ? live.name ?? "Keyboard" : "Not connected"}</span>
              <span className="status-sub">{live.ready ? (live.unsaved ? "Live · not saved on keyboard" : "Live") : live.locked ? "Locked — press unlock key" : "Files only"}</span>
            </span>
          </button>
          <div className="repo-chip" title={repo.root}>
            <Icon name="branch" size={18} />
            <span className="mono">{folderName}</span>
          </div>
          <button className="ghost small-button" onClick={onOpenAnother} disabled={busy}>Open another…</button>
          <ThemeSwitch />
        </div>
      </aside>
      <main className="pane" ref={paneRef}>
        <header className="pane-head">
          <h1>{info.title}</h1>
          <p className="muted">{info.blurb}</p>
        </header>
        {error && <p className="error" role="alert">{error}</p>}

        <div hidden={tab !== "keyboard"} className="tab-body">
          <BuildYamlFix repoRoot={repo.root} revision={changes} onFixed={() => { void onReload(); bump(); }} />
          <HardwareSetup repoRoot={repo.root} onProfile={onProfile} />
          {keymap && (
            <section className="card">
              <h2>Live editing</h2>
              <LivePanel repoRoot={repo.root} keymapPath={keymap.path} revision={keymap} onStatus={setLive} flash={flash}
                onFileChanged={async () => { await fileChanged.current?.(); }} />
            </section>
          )}
          {repo.firmware && <Firmware firmware={repo.firmware} />}
          {repo.warnings.map((w) => <p key={w} className="warning">{withCode(w)}</p>)}
          <p className="muted mono small">{repo.root}</p>
        </div>

        <div hidden={tab !== "keymap"} className="tab-body">
          {keymap ? (
            <KeymapEditor keymap={keymap} repoRoot={repo.root} liveInfo={live} flash={flash} onBlocked={onBlocked} onConnect={() => setTab("keyboard")}
              registerFileChanged={registerFileChanged} onChanged={(k) => { onKeymapChanged(k); bump(); }} onReload={onReload} />
          ) : <p className="warning">No keymap file found in this config.</p>}
        </div>

        {hasScreen && (
          <div hidden={tab !== "screen"} className="tab-body">
            <PicturesPanel repoRoot={repo.root} onChanged={bump} />
          </div>
        )}

        <div hidden={tab !== "settings"} className="tab-body">
          {repo.confFiles.map((c) => <SettingsPanel key={c.path} confPath={c.path} onChanged={bump} />)}
        </div>

        <div hidden={tab !== "install"} className="tab-body">
          {profile ? <BuildPanel repoRoot={repo.root} autoSetup changes={changes} screen={profile.display !== "none"} split={profile.split !== false} />
            : <p className="warning">Confirm your hardware on the <button className="link" onClick={() => setTab("keyboard")}>Keyboard</button> tab first.</p>}
          <HistoryPanel repoRoot={repo.root} changes={changes} onFilesChanged={() => { void onReload(); bump(); }} />
          {repo.build && <BuildTargets targets={repo.build.targets} />}
        </div>
      </main>
    </div>
  );
}

/** build.yaml boards without ZMK's `zmk` variant: current ZMK (and GitHub's build of the
 * repository) fails on them. Offers the one-line-per-board repair. */
function BuildYamlFix({ repoRoot, revision, onFixed }: { repoRoot: string; revision: unknown; onFixed: () => void }) {
  const [boards, setBoards] = useState<string[]>([]);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  useEffect(() => {
    invoke<string[]>("build_yaml_variant_check", { repoPath: repoRoot }).then(setBoards).catch(() => setBoards([]));
  }, [repoRoot, revision]);
  if (boards.length === 0) return null;
  const unique = [...new Set(boards)];
  return (
    <section className="card fix-card">
      <h2><Icon name="warn" size={18} /> GitHub can't build your firmware</h2>
      <p>
        <code>build.yaml</code> builds for {unique.map((b, i) => <span key={b}>{i > 0 && ", "}<code>{b}</code></span>)}. Current ZMK needs the
        {" "}<code>zmk</code> variant (<code>{unique[0]}//zmk</code>), so every build on GitHub fails. mxKeys already uses it for its own builds.
      </p>
      <div className="row">
        <button className="primary" disabled={busy} onClick={async () => {
          setBusy(true);
          setError(null);
          try { await invoke("build_yaml_add_zmk_variant", { repoPath: repoRoot }); setBoards([]); onFixed(); }
          catch (e) { setError(String(e)); }
          finally { setBusy(false); }
        }}>Fix build.yaml</button>
        <span className="muted small">Changes {boards.length} line{boards.length === 1 ? "" : "s"}; then save and upload a version on Install &amp; save.</span>
      </div>
      {error && <p className="error">{error}</p>}
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
    <section className="card">
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
    <section className="card">
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
