import { useEffect, useMemo, useState } from "react";
import type { EditOp, KeyGeom } from "./types";
import type { KeyLabel } from "./bindings";
import { MiniBoard } from "./BehaviorsPanel";
import { FAVOURITES, type FavCtx, type Favourite, type Values } from "./favourites";

/** "Community favourites": proven tricks you can add to your keymap in one step. */
export function Favourites({ ctx, labels, onApply, liveNote }: {
  ctx: FavCtx;
  /** Key labels of the base layer (for picking keys). */
  labels: KeyLabel[];
  /** Apply the edits as one change; resolves true if it worked. */
  onApply: (ops: EditOp[]) => Promise<boolean>;
  /** Shown in the popup when live editing is on (favourites go to the files). */
  liveNote: boolean;
}) {
  const [open, setOpen] = useState<Favourite | null>(null);
  return (
    <section className="favourites">
      <h2>Community favourites</h2>
      <p className="muted">Tricks many ZMK users swear by. Add one in a click — it goes into your keymap like anything else, and Undo takes it out again.</p>
      <ul className="def-grid fav-grid">
        {FAVOURITES.map((f) => {
          const added = f.added(ctx);
          return (
            <li key={f.id}>
              <button className={`def-tile fav-tile${added ? " added" : ""}`} onClick={() => setOpen(f)}>
                <span className="fav-top"><span className="overline">{f.category}</span>{added && <span className="badge strong">added</span>}</span>
                <span className="def-title">{f.title}</span>
                <span className="fav-summary">{f.summary}</span>
              </button>
            </li>
          );
        })}
      </ul>
      {open && <FavouriteDialog f={open} ctx={ctx} labels={labels} liveNote={liveNote} onClose={() => setOpen(null)}
        onApply={async (ops) => { if (await onApply(ops)) setOpen(null); }} />}
    </section>
  );
}

function FavouriteDialog({ f, ctx, labels, liveNote, onClose, onApply }: {
  f: Favourite; ctx: FavCtx; labels: KeyLabel[]; liveNote: boolean; onClose: () => void; onApply: (ops: EditOp[]) => Promise<void>;
}) {
  const [values, setValues] = useState<Values>(() => Object.fromEntries(f.params.map((p) => [
    p.id, p.kind === "keys" ? p.suggest(ctx) : p.kind === "choice" ? p.initial : p.initial(ctx),
  ])));
  const [busy, setBusy] = useState(false);
  const result = useMemo(() => f.build(ctx, values), [f, ctx, values]);
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => { if (e.key === "Escape") onClose(); };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [onClose]);
  const added = f.added(ctx);
  return (
    <div className="modal-backdrop" onPointerDown={(e) => { if (e.target === e.currentTarget) onClose(); }}>
      <div className="modal fav-modal" role="dialog" aria-label={f.title}>
        <div className="modal-head">
          <span className="overline">Community favourite · {f.category}</span>
          <button className="icon-button" onClick={onClose} aria-label="Close">✕</button>
        </div>
        <h3 className="fav-title">{f.title}</h3>
        <p>{f.details}</p>
        <p className="muted small">Idea from: {f.credit}</p>
        {added && <p className="note">You already have this (or something like it). Adding it again makes a second copy.</p>}
        {f.params.map((p) => (
          <div key={p.id} className="fav-param">
            <span className="overline">{p.label}</span>
            {p.kind === "choice" && (
              <div className="mode-choice">
                {p.options.map(([v, l]) => (
                  <label key={v}><input type="radio" checked={values[p.id] === v} onChange={() => setValues({ ...values, [p.id]: v })} /> {l}</label>
                ))}
              </div>
            )}
            {p.kind === "layer" && (
              <select value={values[p.id] as number} onChange={(e) => setValues({ ...values, [p.id]: Number(e.target.value) })}>
                {ctx.layers.map((l) => <option key={l.index} value={l.index}>{l.title}</option>)}
              </select>
            )}
            {p.kind === "keys" && (ctx.geometry.length > 0 ? (
              <>
                <MiniBoard geometry={ctx.geometry} labels={labels} selected={new Set(values[p.id] as number[])} onToggle={(i) => {
                  const cur = values[p.id] as number[];
                  const next = cur.includes(i) ? cur.filter((x) => x !== i) : [...cur, i].slice(-p.count);
                  setValues({ ...values, [p.id]: next });
                }} />
                <span className="muted small">Click {p.count === 1 ? "a key" : `${p.count} keys`}.</span>
              </>
            ) : <p className="warning small">The keyboard needs to be drawn first (“Draw my keyboard” above).</p>)}
          </div>
        ))}
        <div className="fav-preview">
          <span className="overline">What changes</span>
          {typeof result === "string" ? <p className="warning small">{result}</p> : (
            <ul>{result.changes.map((c, i) => <li key={i}>{c}</li>)}</ul>
          )}
        </div>
        {liveNote && <p className="muted small">This goes into your files; it reaches the keyboard with the next build and install.</p>}
        <div className="modal-foot">
          <span className="spacer" />
          <button onClick={onClose}>Cancel</button>
          <button className="primary" disabled={busy || typeof result === "string"} onClick={async () => {
            if (typeof result === "string") return;
            setBusy(true);
            await onApply(result.ops);
            setBusy(false);
          }}>Add to my keymap</button>
        </div>
      </div>
    </div>
  );
}

export type { KeyGeom };
