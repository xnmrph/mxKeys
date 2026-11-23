import { useMemo, useState, type ReactElement } from "react";
import { confirmAction, tell } from "./dialogs";
import type { DefSchema, DefType, Definition, EditOp, KeyGeom, NewProp, PropSchema, PropValue } from "./types";
import { Catalog, describe, type KeyLabel } from "./bindings";
import { KeyPicker } from "./KeyPicker";

/** Tabs in display order, with plain names. */
const TABS: { type: DefType; name: string; newName: string }[] = [
  { type: "combo", name: "Combos", newName: "combo" },
  { type: "hold_tap", name: "Hold-taps", newName: "hold-tap" },
  { type: "mod_morph", name: "Shift-dependent keys", newName: "shift-dependent key" },
  { type: "tap_dance", name: "Tap dances", newName: "tap dance" },
  { type: "macro", name: "Macros", newName: "macro" },
  { type: "sticky_key", name: "Sticky keys", newName: "sticky key" },
  { type: "caps_word", name: "Caps word", newName: "" },
  { type: "conditional_layer", name: "Conditional layers", newName: "conditional layer" },
  { type: "other", name: "Other", newName: "" },
];

const MOD_NAMES: [string, string][] = [
  ["MOD_LSFT", "Left Shift"], ["MOD_RSFT", "Right Shift"], ["MOD_LCTL", "Left Ctrl"], ["MOD_RCTL", "Right Ctrl"],
  ["MOD_LALT", "Left Alt"], ["MOD_RALT", "Right Alt / AltGr"], ["MOD_LGUI", "Left GUI (⌘/❖)"], ["MOD_RGUI", "Right GUI"],
];

const TUNABLE = [
  { target: "mt", name: "&mt (modifier on hold)" },
  { target: "lt", name: "&lt (layer on hold)" },
  { target: "sk", name: "&sk (sticky modifier)" },
  { target: "sl", name: "&sl (sticky layer)" },
  { target: "caps_word", name: "&caps_word" },
];

const MINI = 34; // px per key unit on the small boards

export function MiniBoard({ geometry, labels, selected, onToggle }: {
  geometry: KeyGeom[]; labels: KeyLabel[]; selected: Set<number>; onToggle?: (i: number) => void;
}) {
  const width = Math.max(0, ...geometry.map((g) => g.x + g.w)) / 100;
  const height = Math.max(0, ...geometry.map((g) => g.y + g.h)) / 100 + 0.3;
  return (
    <div className="board mini" style={{ width: width * MINI, height: height * MINI }}>
      {geometry.map((g, i) => (
        <button
          key={i}
          className={`key${selected.has(i) ? " chosen" : ""}`}
          disabled={!onToggle}
          title={`key ${i}`}
          onClick={() => onToggle?.(i)}
          style={{
            left: (g.x / 100) * MINI, top: (g.y / 100) * MINI, width: (g.w / 100) * MINI - 3, height: (g.h / 100) * MINI - 3,
            transform: g.r ? `rotate(${g.r / 100}deg)` : undefined,
            transformOrigin: g.r ? `${((g.rx - g.x) / 100) * MINI}px ${((g.ry - g.y) / 100) * MINI}px` : undefined,
          }}
        >
          <span className="tap">{labels[i]?.tap ?? ""}</span>
        </button>
      ))}
    </div>
  );
}

function propOf(d: Definition, name: string): PropValue | null {
  return d.props.find((p) => p.name === name)?.value ?? null;
}

function summaryOf(d: Definition, cat: Catalog, baseLabels: KeyLabel[]): string {
  const b = propOf(d, "bindings");
  const first = b?.type === "bindings" ? b.value : [];
  switch (d.def_type) {
    case "combo": {
      const pos = propOf(d, "key-positions");
      const keys = pos?.type === "numbers" ? pos.value.map((p) => baseLabels[p]?.tap || `#${p}`).join(" + ") : "?";
      return `${keys} → ${first[0] ? describe(first[0], cat).tap || first[0] : "?"}`;
    }
    case "conditional_layer": {
      const ifl = propOf(d, "if-layers");
      const then = propOf(d, "then-layer");
      const name = (i: number) => cat.layerTitle(String(i));
      return `${ifl?.type === "layers" ? ifl.value.map(name).join(" + ") : "?"} → ${then?.type === "layers" ? name(then.value[0]) : "?"}`;
    }
    case "macro": return `${first.length} steps`;
    case "tap_dance": return first.map((x) => describe(x, cat).tap).join(" · ");
    case "mod_morph": return first.map((x) => describe(x, cat).tap).join(" / ⇧ ");
    default: return d.label ? `&${d.label}` : "";
  }
}

export function BehaviorsPanel({ definitions, cat, geometry, baseLabels, edit }: {
  definitions: Definition[];
  cat: Catalog;
  geometry: KeyGeom[];
  baseLabels: KeyLabel[];
  edit: (op: EditOp) => Promise<boolean>;
}) {
  const counts = useMemo(() => {
    const m = new Map<DefType, number>();
    for (const d of definitions) m.set(d.def_type, (m.get(d.def_type) ?? 0) + 1);
    return m;
  }, [definitions]);
  const [tab, setTab] = useState<DefType>(() => TABS.find((t) => counts.get(t.type))?.type ?? "combo");
  const [selected, setSelected] = useState<string | null>(null);
  const [creating, setCreating] = useState<string | null>(null);
  const schemas = useMemo(() => new Map(cat.data.schemas.map((s) => [s.def_type, s])), [cat]);
  const shown = definitions.filter((d) => d.def_type === tab);
  const current = definitions.find((d) => d.id === selected && d.def_type === tab) ?? null;
  const tabInfo = TABS.find((t) => t.type === tab)!;
  const schema = schemas.get(tab)!;
  const nameTaken = (n: string) => definitions.some((d) => d.node_name === n || d.label === n);
  // Combos on exactly the same keys: only one of them can ever fire.
  const sameKeys = useMemo(() => {
    const byKeys = new Map<string, Definition[]>();
    for (const d of definitions) {
      const p = d.def_type === "combo" ? propOf(d, "key-positions") : null;
      if (p?.type === "numbers") {
        const k = [...p.value].sort((a, b) => a - b).join(" ");
        byKeys.set(k, [...(byKeys.get(k) ?? []), d]);
      }
    }
    const layersOf = (d: Definition) => { const l = propOf(d, "layers"); return l?.type === "layers" ? l.value : null; };
    const overlap = (a: Definition, b: Definition) => {
      const [la, lb] = [layersOf(a), layersOf(b)];
      return la === null || lb === null || la.some((x) => lb.includes(x));
    };
    const out = new Map<string, string>();
    for (const ds of byKeys.values()) for (const d of ds) {
      const others = ds.filter((x) => x !== d && overlap(d, x));
      if (others.length) out.set(d.id, others.map((x) => x.title).join(", "));
    }
    return out;
  }, [definitions]);

  function suggest(): string {
    const base = tab === "combo" ? "combo" : tab === "conditional_layer" ? "auto_layer" : tab.replace("_", "");
    for (let i = 1; ; i++) if (!nameTaken(`${base}_${i}`)) return `${base}_${i}`;
  }

  async function create(name: string) {
    const props: [string, NewProp][] = [];
    if (tab === "conditional_layer") {
      const n = cat.data.layers.length;
      if (n < 3) return void tell("A conditional layer needs at least three layers.");
      props.push(["if-layers", { type: "layers", value: [n - 3 + 1, n - 3 + 2].filter((x) => x < n) }], ["then-layer", { type: "layers", value: [n - 1] }]);
    }
    if (await edit({ op: "add_def", def_type: tab, name, props })) {
      setCreating(null);
      const parent = tab === "macro" ? "macros" : tab === "combo" ? "combos" : tab === "conditional_layer" ? "conditional_layers" : "behaviors";
      setSelected(`${parent}/${name}`);
    }
  }

  return (
    <section className="behaviors">
      <h2>Combos, macros and special keys</h2>
      <nav className="tabs">
        {TABS.filter((t) => counts.get(t.type) || t.newName).map((t) => (
          <button key={t.type} className={t.type === tab ? "active" : ""} onClick={() => { setTab(t.type); setSelected(null); setCreating(null); }}>
            {t.name}{counts.get(t.type) ? ` (${counts.get(t.type)})` : ""}
          </button>
        ))}
      </nav>
      <p className="muted">{schema.help}</p>
      <div className="def-layout">
        <ul className="def-list" key={tab}>
          {shown.map((d) => (
            <li key={d.id}>
              <button className={d.id === selected ? "active" : ""} onClick={() => setSelected(d.id)}>
                <span className="def-title">{sameKeys.has(d.id) && <span title="Another combo uses the same keys">⚠ </span>}{d.title}</span>
                <span className="muted def-summary">{summaryOf(d, cat, baseLabels)}</span>
              </button>
            </li>
          ))}
          {shown.length === 0 && <li className="muted">None yet.</li>}
          {tabInfo.newName && (creating === null ? (
            <li><button className="add" onClick={() => setCreating(suggest())}>＋ New {tabInfo.newName}</button></li>
          ) : (
            <li>
              <form className="inline" onSubmit={(e) => { e.preventDefault(); void create(creating.trim()); }}>
                <input autoFocus value={creating} onChange={(e) => setCreating(e.target.value)} placeholder="name, e.g. copy_paste" />
                <button type="submit">Create</button>
                <button type="button" onClick={() => setCreating(null)}>Cancel</button>
              </form>
              {tab !== "combo" && tab !== "conditional_layer" && <p className="muted small">Keys use it as <code>&amp;{creating || "name"}</code>.</p>}
            </li>
          ))}
          {(tab === "hold_tap" || tab === "sticky_key" || tab === "caps_word") && (
            <li className="tune">
              <span className="muted small">Adjust a built-in:</span>
              {TUNABLE.filter((t) => schemas.get(tab) && (tab === "hold_tap" ? ["mt", "lt"] : tab === "sticky_key" ? ["sk", "sl"] : ["caps_word"]).includes(t.target)).map((t) => (
                <button key={t.target} className="link" onClick={async () => { if (await edit({ op: "tune_builtin", target: t.target })) setSelected(`&${t.target}`); }}>{t.name}</button>
              ))}
            </li>
          )}
        </ul>
        <div className="def-editor">
          {current ? (
            <DefinitionEditor key={current.id} d={current} conflict={sameKeys.get(current.id)} schema={schema} cat={cat} geometry={geometry} baseLabels={baseLabels} edit={edit} onRemoved={() => setSelected(null)} />
          ) : <p className="muted">Pick one on the left{tabInfo.newName ? ", or create a new one" : ""}.</p>}
        </div>
      </div>
    </section>
  );
}

function DefinitionEditor({ d, conflict, schema, cat, geometry, baseLabels, edit, onRemoved }: {
  d: Definition; conflict?: string; schema: DefSchema; cat: Catalog; geometry: KeyGeom[]; baseLabels: KeyLabel[];
  edit: (op: EditOp) => Promise<boolean>; onRemoved: () => void;
}) {
  const set = (prop: string, value: NewProp) => edit({ op: "set_def_prop", id: d.id, prop, value });
  const readOnly = !!d.read_only;
  return (
    <div className="def-card">
      <div className="def-head">
        <div>
          <strong>{d.title}</strong>{" "}
          {d.label && d.source !== "override" && <code className="muted">&amp;{d.label}</code>}
          <div className="muted small">
            {d.source === "override" ? `Changes the built-in &${d.label} everywhere it is used (${d.uses}×).`
              : d.label && d.def_type !== "combo" && d.def_type !== "conditional_layer"
                ? (d.uses ? `Used ${d.uses}× in your keymap.` : "Not used on any key yet — pick it in the key editor under “Your behaviors”.")
                : null}
          </div>
        </div>
        {!readOnly && d.source === "node" && (
          <button className="danger" disabled={d.uses > 0} title={d.uses > 0 ? "Remove it from your keys first" : undefined}
            onClick={async () => { if (await confirmAction(`Delete “${d.title}”? (You can undo this.)`, "Delete") && await edit({ op: "remove_def", id: d.id })) onRemoved(); }}>
            Delete
          </button>
        )}
      </div>
      {conflict && <p className="warning small">“{conflict}” uses exactly the same keys, so only one of them can work. Give one of them different keys or limit them to different layers.</p>}
      {readOnly && <p className="warning">This is {d.read_only}; mxKeys shows it but can't change it yet. You can still edit it in a text editor.</p>}
      {schema.props.filter((s) => !(d.source === "override" && s.name === "bindings")).map((s) => {
        const pv = d.props.find((p) => p.name === s.name);
        return (
          <PropField key={s.name} s={s} value={pv?.value ?? null} raw={pv?.raw ?? null} cat={cat} geometry={geometry} baseLabels={baseLabels}
            macro={d.def_type === "macro"} readOnly={readOnly} optional={d.source === "override"} onSet={(v) => set(s.name, v)} />
        );
      })}
      <details>
        <summary>Advanced</summary>
        <p className="muted small">
          {d.source === "helper" ? "Helper macro" : "Node"} <code>{d.node_name}</code> at line {d.line}
          {d.compatible && <> · <code>{d.compatible}</code></>}
        </p>
        {d.props.filter((p) => p.raw !== null).map((p) => <div key={p.name} className="small"><code>{p.name} = {p.raw};</code> <span className="muted">line {p.line}</span></div>)}
        {d.other_props.map(([n, v]) => <div key={n} className="small"><code>{n}{v ? ` = ${v}` : ""};</code> <span className="muted">(not in the form, kept as is)</span></div>)}
      </details>
    </div>
  );
}

function PropField({ s: schemaProp, value, raw, cat, geometry, baseLabels, macro, readOnly, optional, onSet }: {
  s: PropSchema; value: PropValue | null; raw: string | null; cat: Catalog; geometry: KeyGeom[]; baseLabels: KeyLabel[];
  macro: boolean; readOnly: boolean; optional: boolean; onSet: (v: NewProp) => Promise<boolean>;
}) {
  // Tuning a built-in: nothing is required, defaults are the built-in's own.
  const s = optional ? { ...schemaProp, required: false, default: schemaProp.default === "none" ? "built-in value" : schemaProp.default } : schemaProp;
  const unset = value === null;
  const isRaw = value?.type === "raw";
  let control: ReactElement | null = null;
  switch (s.type) {
    case "ms":
      control = <MsInput key={JSON.stringify(value)} value={value?.type === "int" ? value.value : null} placeholder={s.default ?? ""} disabled={readOnly} onSave={(n) => onSet({ type: "int", value: n })} />;
      break;
    case "flag":
      control = <label className="switch"><input type="checkbox" disabled={readOnly} checked={value?.type === "flag" && value.value} onChange={(e) => onSet({ type: "flag", value: e.target.checked })} />{value?.type === "flag" && value.value ? "On" : "Off"}</label>;
      break;
    case "choice":
      control = (
        <select disabled={readOnly} value={value?.type === "text" ? value.value : ""} onChange={(e) => onSet(e.target.value ? { type: "text", value: e.target.value } : { type: "remove" })}>
          {(unset || isRaw) && <option value="">Default ({s.default})</option>}
          {s.options.map(([v, label]) => <option key={v} value={v}>{label}</option>)}
        </select>
      );
      break;
    case "mods": {
      const cur = new Set(value?.type === "names" ? value.value : []);
      control = (
        <div className="mods">
          {MOD_NAMES.map(([m, label]) => (
            <label key={m} className="check">
              <input type="checkbox" disabled={readOnly} checked={cur.has(m)} onChange={(e) => {
                const next = MOD_NAMES.map(([x]) => x).filter((x) => (x === m ? e.target.checked : cur.has(x)));
                void onSet(next.length ? { type: "names", value: next } : { type: "remove" });
              }} />{label}
            </label>
          ))}
        </div>
      );
      break;
    }
    case "layers":
    case "layer": {
      const cur = value?.type === "layers" ? value.value : [];
      control = s.type === "layer" ? (
        <select disabled={readOnly} value={cur[0] ?? ""} onChange={(e) => onSet({ type: "layers", value: [Number(e.target.value)] })}>
          {cur.length === 0 && <option value="">Choose…</option>}
          {cat.data.layers.map((l) => <option key={l.index} value={l.index}>{l.title}</option>)}
        </select>
      ) : (
        <div className="mods">
          {cat.data.layers.map((l) => (
            <label key={l.index} className="check">
              <input type="checkbox" disabled={readOnly} checked={cur.includes(l.index)} onChange={(e) => {
                const next = cat.data.layers.map((x) => x.index).filter((x) => (x === l.index ? e.target.checked : cur.includes(x)));
                void onSet(next.length ? { type: "layers", value: next } : { type: "remove" });
              }} />{l.title}
            </label>
          ))}
        </div>
      );
      break;
    }
    case "positions":
      control = <PositionsField key={JSON.stringify(value)} value={value?.type === "numbers" ? value.value : []} geometry={geometry} labels={baseLabels} readOnly={readOnly} required={s.required} onSave={(ns) => onSet(ns.length ? { type: "numbers", value: ns } : { type: "remove" })} />;
      break;
    case "keys":
      control = <KeysInput key={JSON.stringify(value)} value={value?.type === "names" ? value.value : []} disabled={readOnly} onSave={(ns) => onSet(ns.length ? { type: "names", value: ns } : { type: "remove" })} />;
      break;
    case "bindings":
      control = <BindingsField value={value?.type === "bindings" ? value.value : []} s={s} cat={cat} macro={macro} readOnly={readOnly} onSave={(list) => onSet({ type: "bindings", value: list })} />;
      break;
  }
  const canDefault = !s.required && !unset && !readOnly && s.type !== "flag";
  return (
    <div className="setting def-prop">
      <div className="setting-main">
        <div className="setting-label">{s.label}{s.required && <span className="muted"> *</span>}</div>
        {unset && s.default && s.type !== "choice" && s.type !== "flag" && <span className="badge">default: {s.default}</span>}
        {canDefault && <button className="link" onClick={() => onSet({ type: "remove" })} title="Remove this line so ZMK's default applies">Use default</button>}
      </div>
      {s.help && <div className="setting-help muted">{s.help}</div>}
      {isRaw && <div className="warning small">Written as <code>{raw}</code>, which the form can't show (e.g. a #define). Changing it here replaces that text.</div>}
      <div className="def-control">{control}</div>
    </div>
  );
}

function MsInput({ value, placeholder, disabled, onSave }: { value: number | null; placeholder: string; disabled: boolean; onSave: (n: number) => Promise<boolean> }) {
  const [text, setText] = useState(value === null ? "" : String(value));
  const save = () => {
    if (text.trim() === "" || Number(text) === value) return;
    const n = Number(text);
    if (!Number.isInteger(n) || n < 0) { setText(value === null ? "" : String(value)); return; }
    void onSave(n);
  };
  return (
    <span className="number">
      <input inputMode="numeric" disabled={disabled} value={text} placeholder={placeholder} onChange={(e) => setText(e.target.value)} onBlur={save} onKeyDown={(e) => { if (e.key === "Enter") save(); }} /> ms
    </span>
  );
}

function KeysInput({ value, disabled, onSave }: { value: string[]; disabled: boolean; onSave: (v: string[]) => Promise<boolean> }) {
  const [text, setText] = useState(value.join(" "));
  const save = () => { const v = text.trim().split(/\s+/).filter(Boolean); if (v.join(" ") !== value.join(" ")) void onSave(v); };
  return <input className="wide" disabled={disabled} value={text} placeholder="e.g. MINUS UNDERSCORE BACKSPACE" onChange={(e) => setText(e.target.value)} onBlur={save} onKeyDown={(e) => { if (e.key === "Enter") save(); }} spellCheck={false} />;
}

function PositionsField({ value, geometry, labels, readOnly, required, onSave }: {
  value: number[]; geometry: KeyGeom[]; labels: KeyLabel[]; readOnly: boolean; required: boolean; onSave: (v: number[]) => Promise<boolean>;
}) {
  const [draft, setDraft] = useState<number[]>(value);
  const changed = draft.join(" ") !== value.join(" ");
  if (geometry.length === 0) return <code>{value.join(" ")}</code>;
  return (
    <div>
      <MiniBoard geometry={geometry} labels={labels} selected={new Set(draft)} onToggle={readOnly ? undefined : (i) => setDraft((d) => (d.includes(i) ? d.filter((x) => x !== i) : [...d, i].sort((a, b) => a - b)))} />
      {!readOnly && (
        <div className="row">
          <span className="muted small">{draft.length ? `Keys ${draft.join(", ")}` : "Click keys to choose them."}</span>
          {changed && <>
            <button className="primary" disabled={required && draft.length < 2} onClick={() => onSave(draft)}>Save keys</button>
            <button onClick={() => setDraft(value)}>Reset</button>
          </>}
        </div>
      )}
    </div>
  );
}

function BindingsField({ value, s, cat, macro, readOnly, onSave }: {
  value: string[]; s: PropSchema & { type: "bindings" }; cat: Catalog; macro: boolean; readOnly: boolean; onSave: (v: string[]) => Promise<boolean>;
}) {
  const [editing, setEditing] = useState<number | null>(null);
  const pickerCat = useMemo(() => (macro ? new Catalog({ ...cat.data, behaviors: [...cat.data.macroControls, ...cat.data.behaviors] }) : cat), [cat, macro]);
  const fixed = s.count !== null;
  const save = (list: string[]) => onSave(list);
  if (s.bare) {
    // Behaviors without parameters (hold-tap sides, sticky key).
    const names = s.count === 2 ? ["When held", "When tapped"] : ["Behavior"];
    return (
      <div className="bare-bindings">
        {(value.length ? value : Array(s.count ?? 1).fill("")).map((b, i) => (
          <label key={i} className="field">
            {names[i] ?? `Entry ${i + 1}`}
            <select disabled={readOnly} value={b} onChange={(e) => save(value.map((x, j) => (j === i ? e.target.value : x)))}>
              {!cat.behaviors.has(b) && <option value={b}>{b || "Choose…"}</option>}
              {cat.data.behaviors.filter((x) => x.params.length > 0 || x.reference === b).map((x) => <option key={x.reference} value={x.reference}>{x.name} ({x.reference})</option>)}
            </select>
          </label>
        ))}
      </div>
    );
  }
  const stepLabel = (b: string) => {
    if (macro) {
      const mc = cat.data.macroControls.find((m) => b.split(" ")[0] === m.reference);
      if (mc) return <span className="step-control">{mc.name}{b.split(" ")[1] ? ` ${b.split(" ")[1]} ms` : ""}</span>;
    }
    const l = describe(b, cat);
    return <span>{l.tap}{l.hold ? <span className="muted"> / {l.hold}</span> : null}</span>;
  };
  const tdNames = (i: number) => (s.name === "bindings" && s.count === 2 ? ["Normally", "With modifier"][i] : macro ? `${i + 1}.` : s.count === null ? `${i + 1}×` : "");
  return (
    <div className="binding-steps">
      <ol>
        {value.map((b, i) => (
          <li key={i}>
            <span className="step-name muted">{tdNames(i)}</span>
            {stepLabel(b)} <code className="muted small">{b}</code>
            {!readOnly && <span className="step-tools">
              <button className="link" onClick={() => setEditing(i)}>Change</button>
              {!fixed && <>
                <button className="link" disabled={i === 0} onClick={() => save(value.map((x, j) => (j === i - 1 ? value[i] : j === i ? value[i - 1] : x)))} title="Move up">↑</button>
                <button className="link" disabled={i === value.length - 1} onClick={() => save(value.map((x, j) => (j === i + 1 ? value[i] : j === i ? value[i + 1] : x)))} title="Move down">↓</button>
                <button className="link" disabled={value.length === 1} onClick={() => save(value.filter((_, j) => j !== i))} title="Remove">✕</button>
              </>}
            </span>}
          </li>
        ))}
      </ol>
      {!readOnly && !fixed && <button className="add" onClick={() => setEditing(value.length)}>＋ Add {macro ? "step" : "entry"}</button>}
      {editing !== null && (
        <KeyPicker
          key={editing}
          raw={value[editing] ?? "&kp A"}
          cat={pickerCat}
          title={editing < value.length ? `Change ${macro ? `step ${editing + 1}` : "entry"}` : `New ${macro ? "step" : "entry"}`}
          onCancel={() => setEditing(null)}
          onSave={async (b) => {
            const next = editing < value.length ? value.map((x, j) => (j === editing ? b : x)) : [...value, b];
            if (await save(next)) setEditing(null);
          }}
        />
      )}
    </div>
  );
}
