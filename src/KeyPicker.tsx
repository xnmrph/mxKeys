import { useMemo, useState } from "react";
import type { BehaviorInfo, BehaviorParam, CommandOption } from "./types";
import { Catalog, MODIFIERS, joinMods, splitMods, tokens, type ModFn } from "./bindings";

const CATEGORY_NAMES: Record<string, string> = {
  keys: "Keys",
  hold_tap: "Hold and tap",
  layers: "Layers",
  connection: "Bluetooth and USB",
  mouse: "Mouse",
  system: "System",
  custom: "Your behaviors",
  macro: "Macro steps",
};

const PARAM_NAMES: Record<string, string> = {
  key: "Key",
  tap: "Key when tapped",
  hold: "When held",
  layer: "Layer",
  command: "Action",
  button: "Button",
  direction: "Direction",
};

const KEY_CATEGORIES = ["letters", "numbers", "symbols", "editing", "navigation", "modifiers", "function", "media", "keypad", "international", "system", "other"];

/** Split a binding's parameter tokens into one value per declared parameter. */
function slotsFor(b: BehaviorInfo, params: string[]): string[] {
  const out: string[] = [];
  let i = 0;
  for (const p of b.params) {
    if (p.kind.type === "command") {
      const opt = p.kind.options.find((o) => o.value === params[i]);
      if (opt?.number) {
        out.push(`${params[i] ?? ""} ${params[i + 1] ?? opt.number[0]}`);
        i += 2;
        continue;
      }
    }
    out.push(params[i] ?? "");
    i++;
  }
  return out;
}

function defaultFor(p: BehaviorParam, cat: Catalog): string {
  switch (p.kind.type) {
    case "keycode": return "A";
    case "modifier": return "LSHIFT";
    case "layer": return String(cat.data.layers[1]?.index ?? 0);
    case "command": {
      const o = p.kind.options[0];
      return o.number ? `${o.value} ${o.number[0]}` : o.value;
    }
    case "free": return "";
  }
}

function KeycodeField({ value, onChange, cat, modifiersOnly }: { value: string; onChange: (v: string) => void; cat: Catalog; modifiersOnly?: boolean }) {
  const { mods, key } = splitMods(value);
  const [query, setQuery] = useState("");
  const [category, setCategory] = useState<string>(modifiersOnly ? "modifiers" : (cat.byName.get(key)?.category ?? "letters"));
  const current = cat.byName.get(key);
  const results = useMemo(() => {
    const q = query.trim().toLowerCase();
    const list = cat.data.keycodes.filter((k) => (modifiersOnly ? k.category === "modifiers" : true));
    if (!q) return list.filter((k) => k.category === category).slice(0, 60);
    return list
      .filter((k) => k.label.toLowerCase() === q || k.names.some((n) => n.toLowerCase().includes(q)) || k.description.toLowerCase().includes(q))
      .sort((a, b) => Number(b.label.toLowerCase() === q) - Number(a.label.toLowerCase() === q))
      .slice(0, 60);
  }, [query, category, cat, modifiersOnly]);
  const toggleMod = (m: ModFn) => onChange(joinMods(mods.includes(m) ? mods.filter((x) => x !== m) : [...mods, m], key));
  return (
    <div className="keycode-field">
      <div className="current">
        <span className="keycap">{cat.keyLabel(value) || "?"}</span>
        <span className="muted">{current ? current.description : key ? `“${key}” (not a known key name)` : "Choose a key"}</span>
      </div>
      {!modifiersOnly && (
        <div className="mods">
          {MODIFIERS.slice(0, 4).map((m) => (
            <label key={m.fn}><input type="checkbox" checked={mods.includes(m.fn)} onChange={() => toggleMod(m.fn)} /> {m.label}</label>
          ))}
          {mods.filter((m) => m.startsWith("R")).map((m) => (
            <label key={m}><input type="checkbox" checked onChange={() => toggleMod(m)} /> {MODIFIERS.find((x) => x.fn === m)!.label}</label>
          ))}
        </div>
      )}
      <input className="search" placeholder="Search keys (e.g. “enter”, “volume”, “ø”)…" value={query} onChange={(e) => setQuery(e.target.value)} />
      {!query && !modifiersOnly && (
        <div className="chips">
          {KEY_CATEGORIES.map((c) => (
            <button key={c} className={c === category ? "chip active" : "chip"} onClick={() => setCategory(c)}>{c}</button>
          ))}
        </div>
      )}
      <div className="key-grid">
        {results.map((k) => (
          <button
            key={k.names[0]}
            className={k.names.includes(key) ? "keychoice chosen" : "keychoice"}
            title={`${k.description}\n${k.names.join(", ")}`}
            onClick={() => onChange(joinMods(mods, k.names[k.names.length > 1 && k.names[1].length < k.names[0].length ? 1 : 0]))}
          >
            {k.label}
          </button>
        ))}
        {results.length === 0 && <span className="muted">No key matches “{query}”.</span>}
      </div>
    </div>
  );
}

function LayerField({ value, onChange, cat }: { value: string; onChange: (v: string) => void; cat: Catalog }) {
  const useDefines = value !== "" && !/^\d+$/.test(value);
  return (
    <select value={value} onChange={(e) => onChange(e.target.value)}>
      {cat.data.layers.map((l) => {
        const v = useDefines && l.define ? l.define : String(l.index);
        return <option key={l.index} value={v}>{l.title}</option>;
      })}
      {!cat.data.layers.some((l) => String(l.index) === value || l.define === value) && value && <option value={value}>{value}</option>}
    </select>
  );
}

function CommandField({ value, onChange, options }: { value: string; onChange: (v: string) => void; options: CommandOption[] }) {
  const [cmd, num] = value.split(/\s+/);
  const opt = options.find((o) => o.value === cmd);
  return (
    <span className="command-field">
      <select value={cmd} onChange={(e) => {
        const o = options.find((x) => x.value === e.target.value)!;
        onChange(o.number ? `${o.value} ${o.number[0]}` : o.value);
      }}>
        {options.map((o) => <option key={o.value} value={o.value}>{o.label}</option>)}
        {!opt && cmd && <option value={cmd}>{cmd}</option>}
      </select>
      {opt?.number && (
        <select value={num} onChange={(e) => onChange(`${cmd} ${e.target.value}`)}>
          {Array.from({ length: opt.number[1] - opt.number[0] + 1 }, (_, i) => opt.number![0] + i).map((n) => (
            <option key={n} value={n}>{opt.number![2]} {opt.number![2] === "profile" ? n + 1 : n}</option>
          ))}
        </select>
      )}
    </span>
  );
}

export function KeyPicker({ raw, cat, title, onSave, onCancel }: {
  raw: string; cat: Catalog; title: string; onSave: (binding: string) => void; onCancel: () => void;
}) {
  const [ref0, ...params0] = tokens(raw);
  const known = cat.behaviors.get(ref0);
  const [ref, setRef] = useState(known ? ref0 : "");
  const [slots, setSlots] = useState<string[]>(known ? slotsFor(known, params0) : []);
  const [rawDraft, setRawDraft] = useState(raw);
  const behavior = cat.behaviors.get(ref);

  const binding = behavior ? [ref, ...slots.flatMap((s) => s.split(/\s+/).filter(Boolean))].join(" ") : rawDraft;
  const groups = useMemo(() => {
    const g = new Map<string, typeof cat.data.behaviors>();
    for (const b of cat.data.behaviors) g.set(b.category, [...(g.get(b.category) ?? []), b]);
    return [...g.entries()];
  }, [cat]);

  function chooseBehavior(r: string) {
    setRef(r);
    const b = cat.behaviors.get(r);
    if (!b) return;
    // Keep compatible values (e.g. the key when switching &kp → &mt's tap side).
    const prevKey = slots.find((s, i) => behavior?.params[i]?.kind.type === "keycode" && s);
    setSlots(b.params.map((p) => (p.kind.type === "keycode" && prevKey ? prevKey : defaultFor(p, cat))));
  }

  return (
    <div className="key-picker">
      <div className="picker-head">
        <strong>{title}</strong>
        <span className="muted">currently <code>{raw}</code></span>
      </div>
      <label className="field">
        <span>What should this key do?</span>
        <select value={ref} onChange={(e) => chooseBehavior(e.target.value)}>
          {!behavior && <option value="">Custom text (Advanced)</option>}
          {groups.map(([c, bs]) => (
            <optgroup key={c} label={CATEGORY_NAMES[c] ?? c}>
              {bs.map((b) => <option key={b.reference} value={b.reference}>{b.name}</option>)}
            </optgroup>
          ))}
        </select>
      </label>
      {behavior && <p className="muted">{behavior.description}</p>}
      {behavior?.params.map((p, i) => (
        <div key={`${ref}-${i}`} className="field">
          <span className="param-name">{PARAM_NAMES[p.name] ?? p.name.charAt(0).toUpperCase() + p.name.slice(1)}</span>
          {(() => {
            const set = (v: string) => setSlots((s) => s.map((x, j) => (j === i ? v : x)));
            switch (p.kind.type) {
              case "keycode": return <KeycodeField value={slots[i] ?? ""} onChange={set} cat={cat} />;
              case "modifier": return <KeycodeField value={slots[i] ?? ""} onChange={set} cat={cat} modifiersOnly />;
              case "layer": return <LayerField value={slots[i] ?? ""} onChange={set} cat={cat} />;
              case "command": return <CommandField value={slots[i] ?? ""} onChange={set} options={p.kind.options} />;
              default: return <input value={slots[i] ?? ""} onChange={(e) => set(e.target.value)} spellCheck={false} />;
            }
          })()}
        </div>
      ))}
      <div className="row">
        <button className="primary" onClick={() => onSave(binding)}>Save</button>
        <button onClick={onCancel}>Cancel</button>
        <code className="muted">{binding}</code>
      </div>
      <details>
        <summary>Advanced: edit the ZMK text directly</summary>
        <div className="row">
          <input value={rawDraft} onChange={(e) => setRawDraft(e.target.value)} spellCheck={false} />
          <button onClick={() => onSave(rawDraft)}>Save this text</button>
        </div>
      </details>
    </div>
  );
}
