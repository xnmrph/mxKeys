import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import type { FlashEvent, FlashView, KeyGeom } from "./types";

/**
 * One Corne half drawn from the real key geometry (staggered columns, angled thumbs),
 * with the controller + screen at the inner edge, its USB port on top and the reset
 * button highlighted. The right half is the mirror image.
 */
function HalfIllustration({ side, active, geometry }: { side: "left" | "right"; active: boolean; geometry: KeyGeom[] }) {
  // Left-half keys: the first six of each 12-key row, plus the first three thumbs.
  const keys = geometry.length === 42 ? [...[0, 12, 24].flatMap((r) => [0, 1, 2, 3, 4, 5].map((c) => geometry[r + c])), geometry[36], geometry[37], geometry[38]] : [];
  if (keys.length === 0) return null;
  const S = 0.42; // px per 1/100 key unit
  const PAD = 22; // case margin around each key
  const ctrl = { x: 612, y: -6, w: 132, h: 300 }; // controller + screen beside the inner column
  const W = 860, H = 560, OX = 40, OY = 70;
  const RX = ctrl.x + ctrl.w / 2 + 22, RY = ctrl.y + ctrl.h + 52; // reset switch: just below the controller
  const key = (g: KeyGeom, grow: number, cls: string, i: number) => (
    <rect key={`${cls}-${i}`} className={cls} x={g.x - grow} y={g.y - grow} width={g.w + 2 * grow} height={g.h + 2 * grow} rx={grow ? 34 : 14}
      transform={g.r ? `rotate(${g.r / 100} ${g.rx} ${g.ry})` : undefined} />
  );
  return (
    <svg className="half-illustration" viewBox={`0 0 ${W} ${H}`} width={W * S} height={H * S} aria-hidden="true">
      <g transform={`${side === "right" ? `translate(${W} 0) scale(-1 1) ` : ""}translate(${OX} ${OY})`}>
        {/* case: every key grown a little, plus the controller area */}
        <g className="hi-case">
          {keys.map((g, i) => key(g, PAD, "hi-case-part", i))}
          <rect x={ctrl.x - PAD} y={ctrl.y - PAD} width={ctrl.w + 2 * PAD} height={ctrl.h + 2 * PAD} rx={34} />
          <rect x={-PAD} y={37} width={640} height={300} />
          <rect x={300} y={12} width={340} height={340} />
          <rect x={ctrl.x - PAD} y={ctrl.y + ctrl.h - 40} width={ctrl.w + 2 * PAD} height={140} rx={34} />
        </g>
        {keys.map((g, i) => key(g, 0, "hi-key", i))}
        {/* controller with the nice!view on top */}
        <rect className="hi-ctrl" x={ctrl.x} y={ctrl.y} width={ctrl.w} height={ctrl.h} rx={12} />
        <rect className="hi-screen" x={ctrl.x + 22} y={ctrl.y + 40} width={ctrl.w - 44} height={170} rx={6} />
        {/* USB port on the top edge */}
        <rect className="hi-usb" x={ctrl.x + 36} y={ctrl.y - 52} width={60} height={40} rx={10} />
        {/* reset button just below the controller */}
        <circle className={`hi-reset${active ? " active" : ""}`} cx={RX} cy={RY} r={26}>
          {active && <animate attributeName="r" values="26;34;26" dur="0.6s" repeatCount="indefinite" />}
        </circle>
      </g>
      {/* labels stay readable (not mirrored) */}
      <text className="hi-label" x={side === "right" ? W - (OX + ctrl.x + 66) : OX + ctrl.x + 66} y={OY + ctrl.y - 64} textAnchor="middle">USB</text>
      <text className="hi-label strong" x={side === "right" ? W - (OX + RX + 52) : OX + RX + 52} y={OY + RY + 10} textAnchor={side === "right" ? "end" : "start"}>reset ×2</text>
    </svg>
  );
}

export function FlashWizard({ repoRoot }: { repoRoot: string }) {
  const [view, setView] = useState<FlashView | null>(null);
  const [geometry, setGeometry] = useState<KeyGeom[]>([]);
  useEffect(() => { invoke<KeyGeom[]>("physical_layout").then(setGeometry).catch(() => setGeometry([])); }, []);
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
          <span>Reset settings first <span className="muted">— only needed if the halves stop talking to each other or pairing is broken. You'll pair Bluetooth again afterwards.</span></span>
        </label>
        <button className="primary" onClick={start}>Install on keyboard</button>
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
        {phase !== "finished" && <HalfIllustration side={side} active={phase === "waiting_for_drive"} geometry={geometry} />}
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
