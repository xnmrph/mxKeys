import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import type { FlashEvent, FlashView, KeyGeom } from "./types";

/** Split a full layout into its left and right halves at the widest horizontal gap. */
function halves(geometry: KeyGeom[]): { left: KeyGeom[]; right: KeyGeom[] } | null {
  if (geometry.length < 4) return null;
  const centers = geometry.map((g) => g.x + g.w / 2).sort((a, b) => a - b);
  let gap = 0, cut = 0;
  for (let i = 1; i < centers.length; i++) {
    if (centers[i] - centers[i - 1] > gap) { gap = centers[i] - centers[i - 1]; cut = (centers[i] + centers[i - 1]) / 2; }
  }
  if (gap < 150) return null; // no clear gap: a one-piece board
  return { left: geometry.filter((g) => g.x + g.w / 2 < cut), right: geometry.filter((g) => g.x + g.w / 2 >= cut) };
}

/**
 * A keyboard half (or a one-piece keyboard) drawn from ZMK's real key layout, with the
 * controller + screen at the inner edge, its USB port on top and the reset button
 * highlighted. The right half is drawn mirrored from its own keys.
 */
function HalfIllustration({ side, active, geometry, screen }: { side: "left" | "right" | "whole"; active: boolean; geometry: KeyGeom[]; screen: boolean }) {
  const h = halves(geometry);
  let keys = side === "whole" || !h ? geometry : side === "left" ? h.left : h.right;
  if (keys.length === 0) return null;
  const mirror = side === "right";
  // Mirror the right half so its inner edge is on the right too (one drawing for both).
  if (mirror) {
    const maxX = Math.max(...keys.map((g) => g.x + g.w));
    keys = keys.map((g) => ({ ...g, x: maxX - g.x - g.w, r: -g.r, rx: g.r ? maxX - g.rx : 0 }));
  }
  const minX = Math.min(...keys.map((g) => g.x)), minY = Math.min(...keys.map((g) => g.y));
  keys = keys.map((g) => ({ ...g, x: g.x - minX, y: g.y - minY, rx: g.r ? g.rx - minX : 0, ry: g.r ? g.ry - minY : 0 }));
  const kw = Math.max(...keys.map((g) => g.x + g.w));
  const kh = Math.max(...keys.map((g) => g.y + g.h));
  const PAD = 22;
  const ctrl = side === "whole"
    ? { x: kw / 2 - 66, y: -330, w: 132, h: 300 }
    : { x: kw + 12, y: -6, w: 132, h: Math.min(300, kh) };
  const RX = side === "whole" ? ctrl.x + ctrl.w + 50 : ctrl.x + ctrl.w / 2 + 22;
  const RY = side === "whole" ? ctrl.y + ctrl.h - 40 : ctrl.y + ctrl.h + 52;
  const OX = 40, OY = side === "whole" ? 400 : 70;
  const W = Math.max(kw, ctrl.x + ctrl.w + (side === "whole" ? 160 : 0)) + 120;
  const H = OY + Math.max(kh + 140, RY + 80);
  const S = Math.min(0.42, 340 / W * 1.0 + 0.0);
  const key = (g: KeyGeom, grow: number, cls: string, i: number) => (
    <rect key={`${cls}-${i}`} className={cls} x={g.x - grow} y={g.y - grow} width={g.w + 2 * grow} height={g.h + 2 * grow} rx={grow ? 18 : 14}
      transform={g.r ? `rotate(${g.r / 100} ${g.rx} ${g.ry})` : undefined} />
  );
  // Labels are drawn unmirrored, so compute where things land on screen.
  const sx = (x: number) => (mirror ? W - (OX + x) : OX + x);
  return (
    <svg className="half-illustration" viewBox={`0 0 ${W} ${H}`} width={W * S} height={H * S} aria-hidden="true">
      <g transform={`${mirror ? `translate(${W} 0) scale(-1 1) ` : ""}translate(${OX} ${OY})`}>
        <g className="hi-case">
          {keys.map((g, i) => key(g, PAD, "hi-case-part", i))}
          <rect x={ctrl.x - PAD} y={ctrl.y - PAD} width={ctrl.w + 2 * PAD} height={ctrl.h + 2 * PAD} rx={34} />
          {side !== "whole" && <rect x={kw - 60} y={0} width={80} height={Math.min(kh, ctrl.h + 40)} />}
          {side !== "whole" && <rect x={ctrl.x - PAD} y={ctrl.y + ctrl.h - 40} width={ctrl.w + 2 * PAD} height={140} rx={34} />}
          {side === "whole" && <rect x={ctrl.x - PAD} y={ctrl.y + ctrl.h - 10} width={ctrl.w + 2 * PAD} height={60} />}
        </g>
        {keys.map((g, i) => key(g, 0, "hi-key", i))}
        <rect className="hi-ctrl" x={ctrl.x} y={ctrl.y} width={ctrl.w} height={ctrl.h} rx={12} />
        {screen && <rect className="hi-screen" x={ctrl.x + 22} y={ctrl.y + 40} width={ctrl.w - 44} height={Math.min(170, ctrl.h - 80)} rx={6} />}
        <rect className="hi-usb" x={ctrl.x + 36} y={ctrl.y - 52} width={60} height={40} rx={10} />
        <circle className={`hi-reset${active ? " active" : ""}`} cx={RX} cy={RY} r={26}>
          {active && <animate attributeName="r" values="26;34;26" dur="0.6s" repeatCount="indefinite" />}
        </circle>
      </g>
      <text className="hi-label" x={sx(ctrl.x + ctrl.w / 2)} y={OY + ctrl.y - 64} textAnchor="middle">USB</text>
      <text className="hi-label strong" x={sx(RX + 52)} y={OY + RY + 10} textAnchor={mirror ? "end" : "start"}>reset ×2</text>
    </svg>
  );
}

export function FlashWizard({ repoRoot, screen }: { repoRoot: string; screen: boolean }) {
  const [view, setView] = useState<FlashView | null>(null);
  const [geometry, setGeometry] = useState<KeyGeom[]>([]);
  useEffect(() => { invoke<KeyGeom[]>("physical_layout", { repoPath: repoRoot }).then(setGeometry).catch(() => setGeometry([])); }, []);
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

  const title = view.current !== null ? view.steps[view.current]?.title ?? "" : "";
  const side = title.includes("right") ? "right" : title.includes("left") ? "left" : "whole";
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
        {phase !== "finished" && <HalfIllustration side={side} active={phase === "waiting_for_drive"} geometry={geometry} screen={screen} />}
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
