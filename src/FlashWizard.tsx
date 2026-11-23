import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import type { FlashEvent, FlashView } from "./types";

/** A schematic keyboard half with its reset button highlighted (original drawing). */
function HalfIllustration({ side, active }: { side: "left" | "right"; active: boolean }) {
  const flip = side === "right" ? "scale(-1,1) translate(-200,0)" : undefined;
  return (
    <svg viewBox="0 0 200 120" width="200" height="120" aria-hidden="true">
      <g transform={flip}>
        <rect x="10" y="20" width="150" height="80" rx="10" fill="var(--key)" stroke="var(--border)" strokeWidth="2" />
        {Array.from({ length: 3 }, (_, r) =>
          Array.from({ length: 6 }, (_, c) => (
            <rect key={`${r}-${c}`} x={18 + c * 23} y={28 + r * 22} width="19" height="18" rx="3" fill="var(--key-muted)" stroke="var(--border)" />
          )),
        )}
        <rect x="150" y="10" width="34" height="18" rx="3" fill="#9ca3af" />
        <text x="167" y="8" fontSize="9" textAnchor="middle" fill="var(--muted)">USB</text>
        <circle cx="172" cy="60" r="9" fill={active ? "var(--accent)" : "var(--key-muted)"} stroke="var(--border)">
          {active && <animate attributeName="r" values="9;11;9" dur="0.6s" repeatCount="indefinite" />}
        </circle>
        <text x="172" y="84" fontSize="9" textAnchor="middle" fill="var(--muted)">reset ×2</text>
      </g>
    </svg>
  );
}

export function FlashWizard({ repoRoot }: { repoRoot: string }) {
  const [view, setView] = useState<FlashView | null>(null);
  const [progress, setProgress] = useState<[number, number] | null>(null);
  const [ended, setEnded] = useState(false);
  const [reset, setReset] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    const un = listen<FlashEvent>("flash-event", (e) => {
      setView(e.payload.view);
      setProgress(e.payload.progress);
      if (e.payload.ended) setEnded(true);
    });
    return () => { un.then((f) => f()); };
  }, []);

  async function start() {
    setError(null);
    setEnded(false);
    try {
      setView(await invoke<FlashView>("start_flash", { repoPath: repoRoot, reset }));
    } catch (e) {
      setError(String(e));
    }
  }

  const control = (command: "retry" | "skip" | "cancel") =>
    invoke<FlashView | null>("flash_control", { command }).then((v) => v && setView(v)).catch((e) => setError(String(e)));

  if (!view || (ended && view.phase.phase !== "finished")) {
    return (
      <div className="flash-start">
        <h3>Install on the keyboard</h3>
        <label className="check">
          <input type="checkbox" checked={reset} onChange={(e) => setReset(e.target.checked)} />
          Reset settings first <span className="muted">— only needed if the halves stop talking to each other or pairing is broken. You'll pair Bluetooth again afterwards.</span>
        </label>
        <button className="primary" onClick={start}>Start</button>
        {ended && <span className="muted"> Stopped.</span>}
        {error && <p className="error">{error}</p>}
      </div>
    );
  }

  const side = view.current !== null && view.steps[view.current]?.title.includes("right") ? "right" : "left";
  const phase = view.phase.phase;
  const pct = progress ? Math.round((progress[0] / Math.max(progress[1], 1)) * 100) : null;
  return (
    <div className="flash-wizard">
      <h3>Install on the keyboard</h3>
      <ol className="flash-steps">
        {view.steps.map((s, i) => (
          <li key={i} className={s.state}>
            <span className="icon">{{ pending: "○", current: "●", done: "✓", skipped: "–" }[s.state]}</span> {s.title}
          </li>
        ))}
      </ol>
      <div className={`flash-now ${phase}`}>
        {phase !== "finished" && <HalfIllustration side={side} active={phase === "waiting_for_drive"} />}
        <div>
          <p className="instruction">{view.instruction}</p>
          {view.detail && <p className="muted">{view.detail}</p>}
          {phase === "copying" && pct !== null && (
            <div className="progress"><div className="bar"><div style={{ width: `${pct}%` }} /></div></div>
          )}
          <div className="row">
            {(phase === "failed" || phase === "refused") && <button onClick={() => control("retry")}>Try again</button>}
            {phase !== "finished" && phase !== "copying" && <button onClick={() => control("skip")}>Skip this step</button>}
            {phase !== "finished" && <button onClick={() => control("cancel")}>Stop</button>}
            {phase === "finished" && <button onClick={() => { setView(null); setEnded(false); }}>Done</button>}
          </div>
        </div>
      </div>
      {error && <p className="error">{error}</p>}
    </div>
  );
}
