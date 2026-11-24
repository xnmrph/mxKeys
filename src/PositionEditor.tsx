import { useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";

export type Fit = "cover" | "contain" | "stretch";
export interface Placement { zoom: number; offset_x: number; offset_y: number; rotate: number }
interface SourceImage { url: string; width: number; height: number }
interface Preview { frames: string[] }

const VIEW_W = 460;
const VIEW_H = 380;
/** Frame height on screen in CSS px (the screen area is scaled to this). */
const FRAME_H = 300;
const MIN_ZOOM = 0.25;
const MAX_ZOOM = 8;

const clampZoom = (z: number) => Math.min(MAX_ZOOM, Math.max(MIN_ZOOM, z));

/**
 * Popup for placing a picture on the screen: the whole picture is shown, the screen
 * is the bright frame; drag to move, scroll or the slider to zoom, rotate in 90° steps.
 * The math matches the renderer (`render.rs` fit + zoom/offset/rotate), so what's in
 * the frame is exactly what ends up on the screen.
 */
export function PositionEditor({ repoRoot, picture, mode, options, budget, area, onApply, onClose }: {
  repoRoot: string;
  picture: unknown;
  mode: string;
  options: Record<string, unknown> & Placement & { fit: Fit };
  budget: number;
  /** Upright screen area in screen pixels. */
  area: [number, number];
  onApply: (p: Placement) => void;
  onClose: () => void;
}) {
  const [src, setSrc] = useState<SourceImage | null>(null);
  const [err, setErr] = useState<string | null>(null);
  const [p, setP] = useState<Placement>({ zoom: options.zoom, offset_x: options.offset_x, offset_y: options.offset_y, rotate: options.rotate });
  const [dots, setDots] = useState<string | null>(null);
  const [dragging, setDragging] = useState(false);
  const drag = useRef<{ x: number; y: number; ox: number; oy: number } | null>(null);
  const viewRef = useRef<HTMLDivElement>(null);

  const [W, H] = area;
  const K = FRAME_H / H; // CSS px per screen pixel

  useEffect(() => {
    invoke<SourceImage>("pictures_source", { repoPath: repoRoot, picture }).then(setSrc).catch((e) => setErr(String(e)));
  }, [repoRoot, picture]);

  // The real black-and-white result, a moment after the picture stops moving.
  useEffect(() => {
    const t = setTimeout(() => {
      invoke<Preview>("pictures_preview", { repoPath: repoRoot, picture, mode, options: { ...options, ...p }, budgetKib: budget })
        .then((r) => setDots(r.frames[0] ?? null))
        .catch(() => {});
    }, dragging ? 150 : 60);
    return () => clearTimeout(t);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [p, dragging]);

  // Scroll to zoom (non-passive so the page behind doesn't scroll).
  useEffect(() => {
    const el = viewRef.current;
    if (!el) return;
    const onWheel = (e: WheelEvent) => {
      e.preventDefault();
      setP((q) => ({ ...q, zoom: clampZoom(q.zoom * Math.pow(1.0015, -e.deltaY)) }));
    };
    el.addEventListener("wheel", onWheel, { passive: false });
    return () => el.removeEventListener("wheel", onWheel);
  }, []);

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") onClose();
      if (e.key === "Enter") onApply(round(p));
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [p, onApply, onClose]);

  // Displayed picture size and position (same fit as the renderer).
  let img: React.CSSProperties | null = null;
  if (src) {
    const sx = W / src.width;
    const sy = H / src.height;
    const [bx, by] = options.fit === "stretch" ? [sx, sy] : options.fit === "contain" ? [Math.min(sx, sy), Math.min(sx, sy)] : [Math.max(sx, sy), Math.max(sx, sy)];
    const w = src.width * bx * p.zoom * K;
    const h = src.height * by * p.zoom * K;
    img = {
      width: w,
      height: h,
      left: VIEW_W / 2 + p.offset_x * K - w / 2,
      top: VIEW_H / 2 + p.offset_y * K - h / 2,
      transform: `rotate(${p.rotate}deg)`,
    };
  }

  const onDown = (e: React.PointerEvent) => {
    (e.currentTarget as Element).setPointerCapture(e.pointerId);
    drag.current = { x: e.clientX, y: e.clientY, ox: p.offset_x, oy: p.offset_y };
    setDragging(true);
  };
  const onMove = (e: React.PointerEvent) => {
    const d = drag.current;
    if (!d) return;
    setP((q) => ({ ...q, offset_x: d.ox + (e.clientX - d.x) / K, offset_y: d.oy + (e.clientY - d.y) / K }));
  };
  const onUp = () => { drag.current = null; setDragging(false); };

  const rotate = () => setP((q) => ({ ...q, rotate: ((q.rotate + 90 + 180) % 360) - 180 }));
  const sliderValue = Math.log2(p.zoom);

  return (
    <div className="modal-backdrop" onPointerDown={(e) => { if (e.target === e.currentTarget) onClose(); }}>
      <div className="modal position-editor" role="dialog" aria-label="Position the picture">
        <div className="modal-head">
          <h3>Position the picture</h3>
          <button className="icon-button" onClick={onClose} aria-label="Close">✕</button>
        </div>
        <div className="pe-body">
          <div
            ref={viewRef}
            className={`pe-view${dragging ? " dragging" : ""}`}
            style={{ width: VIEW_W, height: VIEW_H, background: options.white_background ? "#ffffff" : "#000000" }}
            onPointerDown={onDown} onPointerMove={onMove} onPointerUp={onUp} onPointerCancel={onUp}
          >
            {src && img && <img src={src.url} alt="" draggable={false} className="pe-img" style={img} />}
            <div className="pe-frame" style={{ width: W * K, height: H * K, left: VIEW_W / 2 - (W * K) / 2, top: VIEW_H / 2 - (H * K) / 2 }} />
            {!src && !err && <div className="pe-loading muted">Loading…</div>}
          </div>
          <div className="pe-result">
            <span className="muted small">On the keyboard</span>
            <div className="pe-dots" style={{ width: W * 2, height: H * 2 }}>
              {dots && <img src={dots} alt="result" style={{ width: W * 2, height: H * 2 }} />}
            </div>
          </div>
        </div>
        {err && <p className="error small">{err}</p>}
        <div className="pe-tools">
          <span className="pe-icon small-icon" aria-hidden>▣</span>
          <input type="range" aria-label="Zoom" min={Math.log2(MIN_ZOOM)} max={Math.log2(MAX_ZOOM)} step={0.01} value={sliderValue}
            onChange={(e) => setP((q) => ({ ...q, zoom: clampZoom(Math.pow(2, Number(e.target.value))) }))} />
          <span className="pe-icon big-icon" aria-hidden>▣</span>
          <span className="spacer" />
          <button className="icon-button" onClick={rotate} title="Rotate 90°" aria-label="Rotate 90°">↻</button>
        </div>
        <p className="muted small">Drag to move · scroll to zoom</p>
        <div className="modal-foot">
          <button className="link" onClick={() => setP({ zoom: 1, offset_x: 0, offset_y: 0, rotate: 0 })}>Reset</button>
          <span className="spacer" />
          <button onClick={onClose}>Cancel</button>
          <button className="primary" onClick={() => onApply(round(p))}>Apply</button>
        </div>
      </div>
    </div>
  );
}

function round(p: Placement): Placement {
  return { zoom: Math.round(p.zoom * 1000) / 1000, offset_x: Math.round(p.offset_x * 10) / 10, offset_y: Math.round(p.offset_y * 10) / 10, rotate: p.rotate };
}
