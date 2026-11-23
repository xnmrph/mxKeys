// Parsing and describing ZMK bindings with the editor catalog.
import type { BehaviorInfo, EditorCatalog, KeycodeInfo } from "./types";

/** Split on whitespace outside parentheses: `&kp LC(LS(TAB))` → ["&kp", "LC(LS(TAB))"]. */
export function tokens(raw: string): string[] {
  const out: string[] = [];
  let cur = "";
  let depth = 0;
  for (const ch of raw.trim()) {
    if (ch === "(") depth++;
    if (ch === ")") depth--;
    if (/\s/.test(ch) && depth === 0) {
      if (cur) out.push(cur);
      cur = "";
    } else cur += ch;
  }
  if (cur) out.push(cur);
  return out;
}

export const MODIFIERS = [
  { fn: "LS", label: "Shift", symbol: "⇧" },
  { fn: "LC", label: "Ctrl", symbol: "⌃" },
  { fn: "LA", label: "Alt", symbol: "⌥" },
  { fn: "LG", label: "Cmd/Win", symbol: "⌘" },
  { fn: "RS", label: "Right Shift", symbol: "⇧" },
  { fn: "RC", label: "Right Ctrl", symbol: "⌃" },
  { fn: "RA", label: "AltGr", symbol: "⌥" },
  { fn: "RG", label: "Right Cmd/Win", symbol: "⌘" },
] as const;

export type ModFn = (typeof MODIFIERS)[number]["fn"];

/** `LC(LS(TAB))` → { mods: ["LC","LS"], key: "TAB" } */
export function splitMods(param: string): { mods: ModFn[]; key: string } {
  const mods: ModFn[] = [];
  let s = param.trim();
  for (;;) {
    const m = /^(L[SCAG]|R[SCAG])\((.*)\)$/.exec(s);
    if (!m) break;
    mods.push(m[1] as ModFn);
    s = m[2].trim();
  }
  return { mods, key: s };
}

export function joinMods(mods: ModFn[], key: string): string {
  return mods.reduceRight((inner, m) => `${m}(${inner})`, key);
}

export class Catalog {
  readonly byName = new Map<string, KeycodeInfo>();
  readonly behaviors = new Map<string, BehaviorInfo>();
  constructor(readonly data: EditorCatalog) {
    for (const k of data.keycodes) for (const n of [...k.names, ...k.deprecated]) if (!this.byName.has(n)) this.byName.set(n, k);
    for (const b of data.behaviors) this.behaviors.set(b.reference, b);
  }
  keyLabel(param: string): string {
    const { mods, key } = splitMods(param);
    const k = this.byName.get(key);
    const base = k ? k.label : key;
    const sym = mods.map((m) => MODIFIERS.find((x) => x.fn === m)!.symbol).join("");
    return sym + base;
  }
  layerTitle(param: string): string {
    const l = this.data.layers.find((x) => String(x.index) === param || x.define === param);
    return l ? l.title : `Layer ${param}`;
  }
}

export interface KeyLabel { tap: string; hold?: string; muted?: boolean }

/** Keycap text for a binding. */
export function describe(raw: string, cat: Catalog | null): KeyLabel {
  const [ref, ...params] = tokens(raw);
  if (!ref) return { tap: "" };
  if (!ref.startsWith("&")) return { tap: raw };
  if (ref === "&trans") return { tap: "▽", muted: true };
  if (ref === "&none") return { tap: "", muted: true };
  const b = cat?.behaviors.get(ref);
  if (!cat || !b) return { tap: ref.slice(1).replace(/_/g, " "), hold: params.join(" ") || undefined };
  switch (ref) {
    case "&kp": return { tap: cat.keyLabel(params[0] ?? "") };
    case "&mo": return { tap: cat.layerTitle(params[0] ?? ""), hold: "hold" };
    case "&to": return { tap: `→ ${cat.layerTitle(params[0] ?? "")}` };
    case "&tog": return { tap: `⇄ ${cat.layerTitle(params[0] ?? "")}` };
    case "&sl": return { tap: `1× ${cat.layerTitle(params[0] ?? "")}` };
    case "&lt": return { tap: cat.keyLabel(params[1] ?? ""), hold: cat.layerTitle(params[0] ?? "") };
    case "&mt": return { tap: cat.keyLabel(params[1] ?? ""), hold: cat.keyLabel(params[0] ?? "") };
    case "&sk": return { tap: `1× ${cat.keyLabel(params[0] ?? "")}` };
  }
  // Short keycap texts for connection keys (the picker shows the long labels).
  if (ref === "&out") {
    const short: Record<string, string> = { OUT_TOG: "USB ⇄ BT", OUT_USB: "USB", OUT_BLE: "Bluetooth" };
    if (short[params[0]]) return { tap: short[params[0]] };
  }
  if (ref === "&bt") {
    const n = params[1] !== undefined ? ` ${Number(params[1]) + 1}` : "";
    const short: Record<string, string> = { BT_SEL: `BT${n}`, BT_CLR: "BT forget", BT_CLR_ALL: "BT forget all", BT_NXT: "BT next", BT_PRV: "BT prev", BT_DISC: `BT off${n}` };
    if (short[params[0]]) return { tap: short[params[0]] };
  }
  // Command behaviors: show the command's label (with its number).
  const first = b.params[0];
  if (first?.kind.type === "command") {
    const opt = first.kind.options.find((o) => o.value === params[0]);
    if (opt) return { tap: opt.number ? `${opt.label} ${Number(params[1] ?? 0) + (opt.number[2] === "profile" ? 1 : 0)}` : opt.label };
  }
  if (b.category === "custom" && b.params.length === 2) {
    const tap = b.params[1].kind.type === "keycode" ? cat.keyLabel(params[1] ?? "") : params[1] ?? "";
    const hold = b.params[0].kind.type === "layer" ? cat.layerTitle(params[0] ?? "") : cat.keyLabel(params[0] ?? "");
    return { tap, hold };
  }
  return { tap: b.name };
}
