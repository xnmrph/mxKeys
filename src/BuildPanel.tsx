import { useCallback, useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import type {
  BuildEvent, BuildOutcome, InstallProgress, Problem, TargetId, ToolchainEvent, ToolchainInfo,
} from "./types";

const TARGETS: { id: TargetId; label: string }[] = [
  { id: "left", label: "Left half" },
  { id: "right", label: "Right half" },
  { id: "settings_reset", label: "Settings reset" },
];

type TargetState = { state: "waiting" | "building" | "done" | "failed" | "skipped"; uf2?: string | null; problems?: Problem[] };

const mb = (n: number) => `${Math.round(n / 1_048_576)} MB`;

function describeInstall(p: InstallProgress | null): string {
  if (!p) return "";
  switch (p.kind) {
    case "step":
      return { download: "Downloading", unpack: "Unpacking", trim: "Tidying up", setup: "Setting up" }[p.step] + ` ${p.component}…`;
    case "bytes":
      return p.total ? `Downloading ${p.component}… ${Math.floor((p.done / p.total) * 100)}% of ${mb(p.total)}` : `Downloading ${p.component}… ${mb(p.done)}`;
    case "done":
      return `${p.component} ready`;
    case "line":
      return "";
  }
}

function ProblemView({ p }: { p: Problem }) {
  const where = p.location
    ? p.location.layer && p.location.key
      ? `Layer “${p.location.layer}”, key ${p.location.key}${p.location.binding ? ` (${p.location.binding})` : ""}`
      : `${p.location.file.split(/[\\/]/).pop()}, line ${p.location.line}`
    : null;
  return (
    <div className="problem">
      <strong>{p.title}</strong>
      {where && <div className="muted">{where}</div>}
      {p.hint && <div>{p.hint}</div>}
      <details><summary>Details</summary><code className="raw">{p.raw}</code></details>
    </div>
  );
}

export function BuildPanel({ repoRoot, autoSetup }: { repoRoot: string; autoSetup: boolean }) {
  const [tc, setTc] = useState<ToolchainInfo | null>(null);
  const [tcProgress, setTcProgress] = useState<InstallProgress | null>(null);
  const [tcError, setTcError] = useState<string | null>(null);
  const [building, setBuilding] = useState(false);
  const [stage, setStage] = useState<string>("");
  const [targets, setTargets] = useState<Record<string, TargetState>>({});
  const [log, setLog] = useState<string[]>([]);
  const [outcome, setOutcome] = useState<BuildOutcome | null>(null);
  const [error, setError] = useState<string | null>(null);
  const startedSetup = useRef(false);
  const lastStage = useRef("");

  const refresh = useCallback(() => {
    invoke<ToolchainInfo>("toolchain_info").then(setTc).catch((e) => setTcError(String(e)));
  }, []);

  useEffect(() => {
    refresh();
    const un1 = listen<ToolchainEvent>("toolchain-event", (e) => {
      if (e.payload.type === "progress") {
        if (e.payload.data.kind !== "line") setTcProgress(e.payload.data);
      } else {
        setTcProgress(null);
        setTcError(e.payload.data.error);
        refresh();
      }
    });
    const un2 = listen<BuildEvent>("build-event", (e) => {
      const p = e.payload;
      if (p.type === "finished") {
        setBuilding(false);
        setOutcome(p.data.outcome);
        setError(p.data.error);
        setStage("");
        // Targets that never got to run weren't attempted.
        setTargets((t) => Object.fromEntries(Object.entries(t).map(([k, v]) => [k, v.state === "waiting" || v.state === "building" ? { state: "skipped" } : v])));
        refresh();
        return;
      }
      const ev = p.data;
      switch (ev.event) {
        case "stage": setStage(ev.text); lastStage.current = ev.text; break;
        case "install": if (ev.kind !== "line") setTcProgress(ev); break;
        case "line": setLog((l) => (l.length > 3000 ? [...l.slice(-2000), ev.text] : [...l, ev.text])); break;
        case "target_started": setTargets((t) => ({ ...t, [ev.target]: { state: "building" } })); break;
        case "target_finished":
          setTargets((t) => ({ ...t, [ev.target]: { state: ev.ok ? "done" : "failed", uf2: ev.uf2, problems: ev.problems } }));
          break;
      }
    });
    return () => { un1.then((f) => f()); un2.then((f) => f()); };
  }, [refresh]);

  // After the hardware is confirmed, start downloading the build tools in the background.
  useEffect(() => {
    if (autoSetup && tc && !tc.status.ready && !tc.running && !startedSetup.current) {
      startedSetup.current = true;
      invoke<boolean>("start_toolchain_setup").then(refresh).catch((e) => setTcError(String(e)));
    }
  }, [autoSetup, tc, refresh]);

  async function build() {
    setError(null);
    setOutcome(null);
    setLog([]);
    setTargets(Object.fromEntries(TARGETS.map((t) => [t.id, { state: "waiting" }])));
    setBuilding(true);
    try {
      const started = await invoke<boolean>("start_build", { repoPath: repoRoot });
      if (!started) { setBuilding(false); setError("A build is already running."); }
    } catch (e) {
      setBuilding(false);
      setError(String(e));
    }
  }

  const toolsLine = !tc ? "…" : tc.status.ready
    ? `Build tools are ready (${mb(tc.status.size)}).`
    : tc.running || tcProgress
      ? describeInstall(tcProgress) || "Getting the build tools ready…"
      : `Build tools aren't downloaded yet (about 500 MB to download, ${tc.status.missing.length} parts missing).`;
  const pct = tcProgress?.kind === "bytes" && tcProgress.total ? (tcProgress.done / tcProgress.total) * 100 : null;

  return (
    <section className="build-panel">
      <h2>Firmware</h2>
      <p className="muted">{toolsLine}</p>
      {pct !== null && <div className="progress"><div className="bar"><div style={{ width: `${pct}%` }} /></div></div>}
      {tcError && <p className="error">{tcError}</p>}
      <div className="row">
        <button className="primary" onClick={build} disabled={building}>{building ? "Building…" : "Build firmware"}</button>
        {tc && !tc.status.ready && !tc.running && !tcProgress && (
          <button onClick={() => invoke("start_toolchain_setup").then(refresh)}>Download build tools now</button>
        )}
      </div>
      {stage && <p>{stage}</p>}
      {Object.keys(targets).length > 0 && (
        <ul className="targets-status">
          {TARGETS.map(({ id, label }) => {
            const t = targets[id];
            if (!t) return null;
            const icon = { waiting: "○", building: "◐", done: "✓", failed: "✗", skipped: "–" }[t.state];
            return (
              <li key={id} className={t.state}>
                <span className="icon">{icon}</span> {label}
                {t.state === "done" && <span className="muted"> — ready</span>}
                {t.state === "skipped" && <span className="muted"> — not built</span>}
                {t.problems?.map((p) => <ProblemView key={p.title} p={p} />)}
              </li>
            );
          })}
        </ul>
      )}
      {error && (
        <div className="problem">
          <strong>Something went wrong{lastStage.current ? ` while ${lastStage.current.charAt(0).toLowerCase()}${lastStage.current.slice(1)}` : ""}.</strong>
          <div>Try again; if it keeps happening, the build log below has the details.</div>
          <details><summary>Details</summary><code className="raw">{error}</code></details>
        </div>
      )}
      {outcome && outcome.results.every((r) => r.uf2) && (
        <p className="note">All firmware files are ready in <code>{outcome.firmware_dir}</code>. Flashing them onto the keyboard comes next.</p>
      )}
      {log.length > 0 && (
        <details>
          <summary>Build log ({log.length} lines)</summary>
          <pre className="log">{log.slice(-400).join("\n")}</pre>
        </details>
      )}
      {outcome?.lock && (
        <details>
          <summary>Advanced: exact versions used</summary>
          <pre className="log">{outcome.lock}</pre>
          {tc && <p className="muted">Zephyr SDK {tc.sdkVersion} · tools in <code>{tc.status.root}</code></p>}
        </details>
      )}
    </section>
  );
}
