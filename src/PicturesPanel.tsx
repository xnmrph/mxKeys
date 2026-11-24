import { useCallback, useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { open } from "@tauri-apps/plugin-dialog";
import { confirmAction } from "./dialogs";
import { PositionEditor } from "./PositionEditor";
import { withCode } from "./text";

type Mode = "off" | "art" | "full";
type Fit = "cover" | "contain" | "stretch";
type DitherKind = "threshold" | "floyd_steinberg" | "atkinson" | "bayer4" | "bayer8";
interface Options {
  fit: Fit; dither: DitherKind; threshold: number; brightness: number; contrast: number;
  invert: boolean; white_background: boolean; min_frame_ms: number;
  /** Extra zoom (1 = as fitted), picture centre offset in screen pixels, clockwise degrees. */
  zoom: number; offset_x: number; offset_y: number; rotate: number;
}
interface HalfArt { mode: Mode; source: string | null; options: Options }
interface ArtConfig { version: number; left: HalfArt; right: HalfArt; budget_kib: number }
interface PicturesState { config: ArtConfig; saved: boolean; moduleProblem: string | null; defaultBudgetKib: number; maxBudgetKib: number }
type PictureRef = { kind: "file"; path: string } | { kind: "repo"; name: string };
interface Preview { frames: string[]; delaysMs: number[]; sourceFrames: number; sourceSize: [number, number]; maxFrames: number; bytes: number }
interface Saved { left_frames: number; right_frames: number; left_bytes: number; right_bytes: number }

const DITHERS: [DitherKind, string, string][] = [
  ["atkinson", "Soft shading", "Good for photos and drawings; light, crisp look."],
  ["floyd_steinberg", "Fine shading", "Most detail in photos; can look grainy."],
  ["bayer4", "Pattern", "Regular dot pattern; steady in animations (no flicker)."],
  ["bayer8", "Fine pattern", "Smoother pattern; steady in animations."],
  ["threshold", "Sharp (no shading)", "Pure black and white — best for logos, text and pixel art."],
];
const FITS: [Fit, string][] = [["cover", "Fill (crop the edges)"], ["contain", "Fit (show everything)"], ["stretch", "Stretch"]];
const SCALE = 3;

function kb(bytes: number) {
  return bytes < 1024 ? `${bytes} B` : `${(bytes / 1024).toFixed(1)} KB`;
}

export function PicturesPanel({ repoRoot, onChanged }: { repoRoot: string; onChanged: () => void }) {
  const [state, setState] = useState<PicturesState | null>(null);
  const [cfg, setCfg] = useState<ArtConfig | null>(null);
  const [pics, setPics] = useState<{ left: PictureRef | null; right: PictureRef | null }>({ left: null, right: null });
  const [dirty, setDirty] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);

  const load = useCallback(async () => {
    const s = await invoke<PicturesState>("pictures_state", { repoPath: repoRoot });
    setState(s);
    setCfg(s.config);
    setPics({
      left: s.config.left.source ? { kind: "repo", name: s.config.left.source } : null,
      right: s.config.right.source ? { kind: "repo", name: s.config.right.source } : null,
    });
    setDirty(false);
  }, [repoRoot]);
  useEffect(() => { load().catch((e) => setError(String(e))); }, [load]);

  if (!state || !cfg) return error ? <p className="error">{error}</p> : null;

  const update = (half: "left" | "right", patch: Partial<HalfArt>) => {
    setCfg({ ...cfg, [half]: { ...cfg[half], ...patch } });
    setDirty(true);
    setNotice(null);
  };

  async function save() {
    setBusy(true);
    setError(null);
    try {
      const s = await invoke<Saved>("pictures_save", { repoPath: repoRoot, config: cfg, left: pics.left, right: pics.right });
      await load();
      onChanged();
      const parts = [s.left_frames && `left half ${s.left_frames} frame${s.left_frames > 1 ? "s" : ""} (${kb(s.left_bytes)})`, s.right_frames && `right half ${s.right_frames} frame${s.right_frames > 1 ? "s" : ""} (${kb(s.right_bytes)})`].filter(Boolean);
      setNotice(`Saved into your config${parts.length ? `: ${parts.join(", ")}` : ""}. Build and install the firmware to see it on the keyboard.`);
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  }

  async function removeAll() {
    if (!(await confirmAction("Remove the pictures from your config? The keyboard goes back to ZMK's own screen after the next build.", "Remove"))) return;
    try {
      await invoke("pictures_remove", { repoPath: repoRoot });
      await load();
      onChanged();
      setNotice("Removed. Build and install the firmware to go back to the stock screen.");
    } catch (e) {
      setError(String(e));
    }
  }

  const nothing = cfg.left.mode === "off" && cfg.right.mode === "off";
  return (
    <section className="pictures-panel">
      {state.moduleProblem && <p className="warning">{withCode(state.moduleProblem)}</p>}
      <div className="picture-halves">
        <HalfEditor title="Left half" half="left" art={cfg.left} pic={pics.left} budget={cfg.budget_kib} repoRoot={repoRoot}
          allowArt={false} onChange={(p) => update("left", p)} onPick={(r) => { setPics({ ...pics, left: r }); setDirty(true); }} />
        <HalfEditor title="Right half" half="right" art={cfg.right} pic={pics.right} budget={cfg.budget_kib} repoRoot={repoRoot}
          allowArt onChange={(p) => update("right", p)} onPick={(r) => { setPics({ ...pics, right: r }); setDirty(true); }} />
      </div>
      <div className="row">
        <button className="primary" disabled={busy || !dirty || !!state.moduleProblem || (nothing && !state.saved)} onClick={save}>
          {busy ? "Saving…" : nothing && state.saved ? "Save (stock screens)" : "Save to config"}
        </button>
        {dirty && <button onClick={() => load()}>Discard changes</button>}
        {state.saved && <button className="link danger" onClick={removeAll}>Remove pictures</button>}
        <label className="small muted">Space per half:{" "}
          <select value={cfg.budget_kib} onChange={(e) => { setCfg({ ...cfg, budget_kib: Number(e.target.value) }); setDirty(true); }}>
            {[32, 64, 128, state.maxBudgetKib].filter((v, i, a) => a.indexOf(v) === i).map((v) => <option key={v} value={v}>{v} KB{v === state.defaultBudgetKib ? " (default)" : ""}</option>)}
          </select>
        </label>
      </div>
      <p className="muted small">Pictures need the screen switched on (Settings → Screen). Animations pause while the keyboard is idle to save battery.</p>
      {notice && <p className="note">{notice}</p>}
      {error && <p className="error">{error}</p>}
    </section>
  );
}

function HalfEditor({ title, half, art, pic, budget, repoRoot, allowArt, onChange, onPick }: {
  title: string; half: "left" | "right"; art: HalfArt; pic: PictureRef | null; budget: number; repoRoot: string; allowArt: boolean;
  onChange: (p: Partial<HalfArt>) => void; onPick: (r: PictureRef) => void;
}) {
  const [preview, setPreview] = useState<Preview | null>(null);
  const [err, setErr] = useState<string | null>(null);
  const [frame, setFrame] = useState(0);
  const seq = useRef(0);
  const o = { ...art.options, zoom: art.options.zoom ?? 1, offset_x: art.options.offset_x ?? 0, offset_y: art.options.offset_y ?? 0, rotate: art.options.rotate ?? 0 };
  const setO = (p: Partial<Options>) => onChange({ options: { ...o, ...p } });
  const [positioning, setPositioning] = useState(false);
  const area: [number, number] = [68, art.mode === "art" ? 140 : 160];
  const moved = o.zoom !== 1 || o.offset_x !== 0 || o.offset_y !== 0 || o.rotate !== 0;

  // Re-render the preview (debounced) when anything changes.
  useEffect(() => {
    if (art.mode === "off" || !pic) { setPreview(null); return; }
    const my = ++seq.current;
    const t = setTimeout(() => {
      invoke<Preview>("pictures_preview", { repoPath: repoRoot, picture: pic, mode: art.mode, options: o, budgetKib: budget })
        .then((p) => { if (my === seq.current) { setPreview(p); setErr(null); setFrame(0); } })
        .catch((e) => { if (my === seq.current) setErr(String(e)); });
    }, 120);
    return () => clearTimeout(t);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [art.mode, pic, JSON.stringify(o), budget, repoRoot]);

  // Play animations at their real speed.
  useEffect(() => {
    if (!preview || preview.frames.length < 2) return;
    const t = setTimeout(() => setFrame((f) => (f + 1) % preview.frames.length), preview.delaysMs[frame] || 100);
    return () => clearTimeout(t);
  }, [preview, frame]);

  async function pick() {
    const f = await open({ title: "Choose a picture", filters: [{ name: "Pictures", extensions: ["png", "gif", "jpg", "jpeg", "bmp", "webp"] }] });
    if (typeof f === "string") {
      onPick({ kind: "file", path: f });
      onChange({ mode: art.mode === "off" ? (allowArt ? "art" : "full") : art.mode, options: { ...o, zoom: 1, offset_x: 0, offset_y: 0, rotate: 0 } });
      setPositioning(true);
    }
  }

  const picName = pic ? (pic.kind === "repo" ? pic.name : pic.path.split(/[\\/]/).pop()) : null;
  const reduced = preview && preview.sourceFrames > preview.frames.length;
  return (
    <div className="picture-half">
      <h3>{title}</h3>
      <div className="mode-choice">
        <label className="check"><input type="radio" checked={art.mode === "off"} onChange={() => onChange({ mode: "off" })} /> ZMK's screen</label>
        {allowArt && <label className="check"><input type="radio" checked={art.mode === "art"} onChange={() => onChange({ mode: "art" })} /> Picture under the battery</label>}
        <label className="check"><input type="radio" checked={art.mode === "full"} onChange={() => onChange({ mode: "full" })} /> Whole screen</label>
      </div>
      {art.mode === "full" && <p className="muted small">{half === "left" ? "Hides the layer, battery and Bluetooth information this half normally shows." : "Hides the battery and connection icons."}</p>}
      {art.mode !== "off" && (
        <div className="picture-body">
          <div className="screen-mock" style={{ width: 68 * SCALE + 12 }}>
            <div className={`screen${pic ? " movable" : ""}`} style={{ width: 68 * SCALE, height: 160 * SCALE }}
              onClick={() => { if (pic) setPositioning(true); }}
              title={pic ? "Click to move, zoom or rotate the picture" : undefined}>
              {art.mode === "art" && <div className="status-strip" style={{ height: 20 * SCALE }}><span className="batt" /><span className="small">⌁</span></div>}
              {preview?.frames[frame] && (
                <img src={preview.frames[frame]} alt="preview" draggable={false} style={{ width: 68 * SCALE, height: (art.mode === "art" ? 140 : 160) * SCALE }} />
              )}
              {!pic && <div className="empty muted">No picture yet</div>}
            </div>
            {preview?.frames[frame] && <img className="actual" src={preview.frames[frame]} alt="actual size" title="Actual size" />}
          </div>
          <div className="picture-controls">
            <div className="row">
              <button onClick={pick}>{pic ? "Choose another picture…" : "Choose a picture…"}</button>
              {picName && <span className="muted small">{picName}</span>}
            </div>
            {err && <p className="error small">{err}</p>}
            {preview && (
              <p className="small">
                {preview.frames.length > 1 ? `${preview.frames.length} frames` : "Still picture"} · {kb(preview.bytes)} of {budget} KB
                <span className="muted"> · original {preview.sourceSize[0]}×{preview.sourceSize[1]}{preview.sourceFrames > 1 ? `, ${preview.sourceFrames} frames` : ""}</span>
                {reduced && <span className="warning-text"> — fewer frames than the original (too fast or too many for the space); it plays at the original speed, less smoothly.</span>}
              </p>
            )}
            {pic && (
              <div className="row">
                <button onClick={() => setPositioning(true)}>Move, zoom or rotate…</button>
                {moved && <button className="link" onClick={() => setO({ zoom: 1, offset_x: 0, offset_y: 0, rotate: 0 })}>Reset position</button>}
              </div>
            )}
            <label className="field small">Fit
              <select value={o.fit} onChange={(e) => setO({ fit: e.target.value as Fit })}>{FITS.map(([v, l]) => <option key={v} value={v}>{l}</option>)}</select>
            </label>
            <label className="field small">Style
              <select value={o.dither} onChange={(e) => setO({ dither: e.target.value as DitherKind })}>{DITHERS.map(([v, l]) => <option key={v} value={v}>{l}</option>)}</select>
            </label>
            <p className="muted small">{DITHERS.find((d) => d[0] === o.dither)?.[2]}</p>
            <Slider label="Brightness" value={o.brightness} min={-100} max={100} onChange={(v) => setO({ brightness: v })} />
            <Slider label="Contrast" value={o.contrast} min={-100} max={100} onChange={(v) => setO({ contrast: v })} />
            <Slider label={o.dither === "threshold" ? "Black/white cut-off" : "Mid point"} value={o.threshold} min={1} max={254} onChange={(v) => setO({ threshold: v })} />
            {preview && preview.sourceFrames > 1 && (
              <Slider label="Fastest frame (ms)" value={o.min_frame_ms} min={50} max={1000} step={10} onChange={(v) => setO({ min_frame_ms: v })} />
            )}
            <div className="row">
              <label className="check"><input type="checkbox" checked={o.invert} onChange={(e) => setO({ invert: e.target.checked })} /> Invert</label>
              <label className="check"><input type="checkbox" checked={o.white_background} onChange={(e) => setO({ white_background: e.target.checked })} /> White background</label>
            </div>
          </div>
        </div>
      )}
      {positioning && pic && art.mode !== "off" && (
        <PositionEditor repoRoot={repoRoot} picture={pic} mode={art.mode} options={o} budget={budget} area={area}
          onApply={(pl) => { setO(pl); setPositioning(false); }} onClose={() => setPositioning(false)} />
      )}
    </div>
  );
}

function Slider({ label, value, min, max, step = 1, onChange }: { label: string; value: number; min: number; max: number; step?: number; onChange: (v: number) => void }) {
  return (
    <label className="slider small">
      <span>{label}</span>
      <input type="range" min={min} max={max} step={step} value={value} onChange={(e) => onChange(Number(e.target.value))} />
      <span className="muted">{value}</span>
    </label>
  );
}
