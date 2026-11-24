// Community favourites: well-known ZMK tricks as ready-made recipes. Each one is our
// own description of a widely shared idea (sources credited), turned into the same
// edits the keymap editor makes — added as one change, undone with one Undo.

import type { Definition, EditOp, KeyGeom, LayerSummary } from "./types";

export interface FavCtx {
  layers: LayerSummary[];
  definitions: Definition[];
  geometry: KeyGeom[];
  /** The layer shown in the editor (where single keys get placed). */
  layer: number;
}

export type Param =
  | { kind: "keys"; id: string; label: string; count: number; suggest: (c: FavCtx) => number[] }
  | { kind: "choice"; id: string; label: string; options: [string, string][]; initial: string }
  | { kind: "layer"; id: string; label: string; initial: (c: FavCtx) => number };

export type Values = Record<string, number[] | string | number>;

export interface Favourite {
  id: string;
  title: string;
  /** One line on the card. */
  summary: string;
  /** What it does, how to use it. */
  details: string;
  credit: string;
  category: "Typing" | "Shortcuts" | "Layers";
  params: Param[];
  added: (c: FavCtx) => boolean;
  /** The edits, or a plain-language reason it can't be added here. */
  build: (c: FavCtx, v: Values) => { ops: EditOp[]; changes: string[] } | string;
}

// ---- helpers ----

const KP = /^&kp\s+(\S+)$/;
const kpName = (b: string | undefined) => (b ? KP.exec(b.trim())?.[1] ?? null : null);

/** Positions on `layer` whose binding is `&kp <one of names>`. */
function findKeys(c: FavCtx, layer: number, names: string[]): number[] {
  const l = c.layers.find((x) => x.index === layer);
  if (!l) return [];
  return l.bindings.flatMap((b, i) => (names.includes(kpName(b) ?? "") ? [i] : []));
}
const first = (c: FavCtx, names: string[]) => findKeys(c, 0, names)[0];
/** "Z" for a key that types Z on the base layer, else "key 12". */
function keyText(c: FavCtx, p: number): string {
  const k = kpName(c.layers.find((l) => l.index === 0)?.bindings[p]);
  return k ? KEY_NAMES[k] ?? k : `key ${p + 1}`;
}
const keysText = (c: FavCtx, ps: number[]) => ps.map((p) => keyText(c, p)).join(" + ");

function nameTaken(c: FavCtx, n: string) {
  return c.definitions.some((d) => d.node_name === n || d.label === n);
}
function unique(c: FavCtx, base: string) {
  if (!nameTaken(c, base)) return base;
  for (let i = 2; ; i++) if (!nameTaken(c, `${base}_${i}`)) return `${base}_${i}`;
}
const usesBehavior = (c: FavCtx, label: string) => c.layers.some((l) => l.bindings.some((b) => b.trim().startsWith(`&${label} `) || b.trim() === `&${label}`));

/** Left/right halves of the layout, split at the widest gap between key columns. */
function halves(g: KeyGeom[]): { left: Set<number>; right: Set<number> } | null {
  const centers = g.map((k, i) => ({ i, x: k.x + k.w / 2 })).sort((a, b) => a.x - b.x);
  let gap = 0, cut = 0;
  for (let j = 1; j < centers.length; j++) {
    const d = centers[j].x - centers[j - 1].x;
    if (d > gap) { gap = d; cut = (centers[j].x + centers[j - 1].x) / 2; }
  }
  if (gap < 150) return null;
  const left = new Set<number>(), right = new Set<number>();
  g.forEach((k, i) => (k.x + k.w / 2 < cut ? left : right).add(i));
  return { left, right };
}
const cy = (k: KeyGeom) => k.y + k.h / 2;
const cx = (k: KeyGeom) => k.x + k.w / 2;

/** The key and the `n - 1` keys towards the middle of the keyboard on its row. */
function rowFrom(c: FavCtx, start: number, n: number, inward: 1 | -1): number[] {
  const s = c.geometry[start];
  return c.geometry
    .map((k, i) => ({ k, i }))
    .filter(({ k }) => Math.abs(cy(k) - cy(s)) < 50 && (inward > 0 ? cx(k) >= cx(s) : cx(k) <= cx(s)))
    .sort((a, b) => inward * (cx(a.k) - cx(b.k)))
    .slice(0, n)
    .map(({ i }) => i);
}

const MOD_NAMES: Record<string, string> = { GUI: "Gui (⌘/❖)", ALT: "Alt", CTRL: "Ctrl", SHFT: "Shift" };
const KEY_NAMES: Record<string, string> = { SEMI: ";", SEMICOLON: ";" };

const OS: [string, string][] = [["ctrl", "Ctrl (Linux, Windows)"], ["cmd", "Cmd (macOS)"]];
const shortcut = (os: string, key: string) => (os === "cmd" ? `&kp LG(${key})` : `&kp LC(${key})`);

// ---- the favourites ----

export const FAVOURITES: Favourite[] = [
  {
    id: "bspc_del",
    title: "Shift + Backspace = Delete",
    summary: "Backspace deletes forwards when Shift is held.",
    details: "Frees a key: hold Shift and press Backspace to delete the character after the cursor. Every Backspace key in your keymap gets it.",
    credit: "ZMK documentation (mod-morph example); in many community configs",
    category: "Typing",
    params: [],
    added: (c) => c.definitions.some((d) => d.def_type === "mod_morph" && JSON.stringify(d.props).includes("DEL")),
    build: (c) => {
      const name = unique(c, "bspc_del");
      const ops: EditOp[] = [{
        op: "add_def", def_type: "mod_morph", name,
        props: [["bindings", { type: "bindings", value: ["&kp BSPC", "&kp DEL"] }], ["mods", { type: "names", value: ["MOD_LSFT", "MOD_RSFT"] }]],
      }];
      const changes = [`Adds “${name}” (Backspace, Delete with Shift)`];
      for (const l of c.layers) {
        for (const p of findKeys(c, l.index, ["BSPC", "BACKSPACE"])) {
          ops.push({ op: "set_binding", layer: l.index, position: p, binding: `&${name}` });
          changes.push(`${l.title}, key ${p + 1}: Backspace → Backspace/Delete`);
        }
      }
      if (ops.length === 1) changes.push("No Backspace key found — pick it on a key with the key editor (“Your behaviors”).");
      return { ops, changes };
    },
  },
  {
    id: "smart_shift",
    title: "Smart Shift",
    summary: "Tap: Shift for the next letter. Double-tap: caps word. Hold: Shift.",
    details: "One key for all your capitals. Tap it and the next key comes out shifted — no holding. Tap twice to TYPE A WHOLE WORD in capitals (ends at a space). Hold it like a normal Shift.",
    credit: "Popular pattern combining ZMK's sticky keys and caps word (e.g. in urob's and Miryoku-style configs)",
    category: "Typing",
    params: [{ kind: "keys", id: "key", label: "Which key becomes Smart Shift", count: 1, suggest: (c) => { const p = first(c, ["LSHFT", "LSHIFT", "LEFT_SHIFT"]); return p === undefined ? [] : [p]; } }],
    added: (c) => nameTaken(c, "smart_shift"),
    build: (c, v) => {
      const key = (v.key as number[])[0];
      if (key === undefined) return "Pick the key on the board.";
      const name = unique(c, "smart_shift");
      return {
        ops: [
          { op: "add_def", def_type: "tap_dance", name, props: [["bindings", { type: "bindings", value: ["&sk LSHFT", "&caps_word"] }], ["tapping-term-ms", { type: "int", value: 200 }]] },
          { op: "set_binding", layer: 0, position: key, binding: `&${name}` },
        ],
        changes: [`Adds “${name}” (tap: one-shot Shift, double-tap: caps word)`, `Base layer: ${keyText(c, key)} → Smart Shift`],
      };
    },
  },
  {
    id: "homerow_mods",
    title: "Home-row mods (timeless)",
    summary: "Hold A S D F / J K L ; for Gui, Alt, Ctrl, Shift — without typing mistakes.",
    details: "Your home row doubles as modifiers: hold a key and it's a modifier, tap it and it types. Tuned so fast typing never triggers a modifier: a modifier only counts with a key on the other hand (or a thumb), and never right after another key.",
    credit: "urob's “timeless home-row mods” (github.com/urob/zmk-config)",
    category: "Typing",
    params: [{ kind: "choice", id: "order", label: "Order from pinky to index", options: [["gacs", "Gui · Alt · Ctrl · Shift (most common)"], ["cags", "Ctrl · Alt · Gui · Shift"]], initial: "gacs" }],
    added: (c) => usesBehavior(c, "hml") || nameTaken(c, "hml"),
    build: (c, v) => {
      if (c.geometry.length === 0) return "The keyboard needs to be drawn first (Keymap → “Draw my keyboard”).";
      const h = halves(c.geometry);
      if (!h) return "This works on split keyboards (the two hands need to be told apart).";
      const a = first(c, ["A"]), j = first(c, ["J"]);
      if (a === undefined || j === undefined) return "Couldn't find A and J on the base layer.";
      const left = rowFrom(c, a, 4, 1), right = rowFrom(c, j, 4, 1);
      if (left.length < 4 || right.length < 4) return "Couldn't find four home-row keys on each hand.";
      const order = v.order === "cags" ? ["CTRL", "ALT", "GUI", "SHFT"] : ["GUI", "ALT", "CTRL", "SHFT"];
      const aY = cy(c.geometry[a]);
      // Thumb keys: below the bottom letter row (rows are ~100 apart; staggers stay under ~60).
      const thumbs = (side: Set<number>) => [...side].filter((i) => cy(c.geometry[i]) > aY + 140);
      const hml = unique(c, "hml"), hmr = unique(c, "hmr");
      const ht = (name: string, trigger: number[]): EditOp => ({
        op: "add_def", def_type: "hold_tap", name,
        props: [
          ["bindings", { type: "bindings", value: ["&kp", "&kp"] }],
          ["flavor", { type: "text", value: "balanced" }],
          ["tapping-term-ms", { type: "int", value: 280 }],
          ["quick-tap-ms", { type: "int", value: 175 }],
          ["require-prior-idle-ms", { type: "int", value: 150 }],
          ["hold-trigger-key-positions", { type: "numbers", value: trigger.sort((x, y) => x - y) }],
          ["hold-trigger-on-release", { type: "flag", value: true }],
        ],
      });
      const ops: EditOp[] = [ht(hml, [...h.right, ...thumbs(h.left)]), ht(hmr, [...h.left, ...thumbs(h.right)])];
      const changes = [`Adds “${hml}” and “${hmr}” (tuned hold-taps for each hand)`];
      const base = c.layers.find((l) => l.index === 0)!;
      const place = (keys: number[], mods: string[], side: "L" | "R", ht: string) => keys.forEach((p, n) => {
        const k = kpName(base.bindings[p]);
        if (!k) { changes.push(`Key ${p + 1} isn't a plain key — left as it is`); return; }
        const mod = `${side}${mods[n]}`;
        ops.push({ op: "set_binding", layer: 0, position: p, binding: `&${ht} ${mod} ${k}` });
        changes.push(`${KEY_NAMES[k] ?? k}: hold for ${MOD_NAMES[mods[n]]}`);
      });
      place(left, order, "L", hml);
      // Right hand: index finger first, so the order runs the other way.
      place(right, [...order].reverse(), "R", hmr);
      return { ops, changes };
    },
  },
  {
    id: "clipboard_combos",
    title: "Copy, paste and undo combos",
    summary: "Z+X undo, X+C copy, C+V paste — two keys pressed together.",
    details: "Press two neighbouring bottom-row keys at the same time for the shortcut. Only on the base layer, and ignored while you're typing fast.",
    credit: "Common in ZMK and QMK community keymaps",
    category: "Shortcuts",
    params: [{ kind: "choice", id: "os", label: "Shortcuts for", options: OS, initial: "ctrl" }],
    added: (c) => ["undo_combo", "copy_combo", "paste_combo"].every((n) => nameTaken(c, n)),
    build: (c, v) => {
      const pos = (k: string) => first(c, [k]);
      const [z, x, cc, vv] = ["Z", "X", "C", "V"].map(pos);
      if ([z, x, cc, vv].some((p) => p === undefined)) return "Couldn't find Z, X, C and V on the base layer.";
      const existing = new Set(c.definitions.filter((d) => d.def_type === "combo").map((d) => {
        const kp = d.props.find((p) => p.name === "key-positions")?.value;
        return kp?.type === "numbers" ? [...kp.value].sort((a, b) => a - b).join(",") : "";
      }));
      const ops: EditOp[] = [], changes: string[] = [];
      for (const [name, keys, result, what] of [
        ["undo_combo", [z!, x!], "Z", "undo"], ["copy_combo", [x!, cc!], "C", "copy"], ["paste_combo", [cc!, vv!], "V", "paste"],
      ] as [string, number[], string, string][]) {
        if (existing.has([...keys].sort((a, b) => a - b).join(","))) { changes.push(`Skipped ${what}: you already have a combo on those keys`); continue; }
        const n = unique(c, name);
        ops.push({
          op: "add_def", def_type: "combo", name: n,
          props: [
            ["key-positions", { type: "numbers", value: keys }],
            ["bindings", { type: "bindings", value: [shortcut(v.os as string, result)] }],
            ["layers", { type: "layers", value: [0] }],
            ["require-prior-idle-ms", { type: "int", value: 150 }],
          ],
        });
        changes.push(`Combo ${what}: ${keysText(c, keys)}`);
      }
      if (ops.length === 0) return "You already have combos on all of those keys.";
      return { ops, changes };
    },
  },
  {
    id: "esc_combo",
    title: "Escape combo",
    summary: "Two keys together give Escape — no reaching for the corner.",
    details: "Pick two neighbouring keys (Q+W is common). Works on every layer and is ignored while you're typing fast.",
    credit: "Common in ZMK and QMK community keymaps",
    category: "Shortcuts",
    params: [{ kind: "keys", id: "keys", label: "The two keys", count: 2, suggest: (c) => [first(c, ["Q"]), first(c, ["W"])].filter((p): p is number => p !== undefined) }],
    added: (c) => nameTaken(c, "esc_combo"),
    build: (c, v) => {
      const keys = v.keys as number[];
      if (keys.length !== 2) return "Pick two keys on the board.";
      const n = unique(c, "esc_combo");
      return {
        ops: [{ op: "add_def", def_type: "combo", name: n, props: [["key-positions", { type: "numbers", value: keys }], ["bindings", { type: "bindings", value: ["&kp ESC"] }], ["require-prior-idle-ms", { type: "int", value: 150 }]] }],
        changes: [`Combo Escape: ${keysText(c, keys)}`],
      };
    },
  },
  {
    id: "tri_layer",
    title: "Third layer from two thumbs",
    summary: "Hold both layer keys together to reach a third layer.",
    details: "With a lower and a raise layer, holding both turns on a third one (often for settings, Bluetooth or function keys) — no extra key needed.",
    credit: "ZMK conditional layers (the “tri-layer” idea from QMK)",
    category: "Layers",
    params: [
      { kind: "layer", id: "a", label: "When this layer…", initial: (c) => Math.min(1, c.layers.length - 1) },
      { kind: "layer", id: "b", label: "…and this one are on", initial: (c) => Math.min(2, c.layers.length - 1) },
      { kind: "layer", id: "then", label: "turn on", initial: (c) => Math.min(3, c.layers.length - 1) },
    ],
    added: () => false,
    build: (c, v) => {
      const [a, b, t] = [v.a as number, v.b as number, v.then as number];
      if (c.layers.length < 3) return "This needs at least three layers.";
      if (new Set([a, b, t]).size < 3) return "Pick three different layers.";
      const n = unique(c, "tri_layer");
      const title = (i: number) => c.layers.find((l) => l.index === i)?.title ?? `layer ${i}`;
      return {
        ops: [{ op: "add_def", def_type: "conditional_layer", name: n, props: [["if-layers", { type: "layers", value: [a, b] }], ["then-layer", { type: "layers", value: [t] }]] }],
        changes: [`${title(a)} + ${title(b)} → ${title(t)}`],
      };
    },
  },
  {
    id: "repeat_key",
    title: "Repeat key",
    summary: "One key that types the last key again.",
    details: "Great for double letters (“ll”, “ee”) and repeating shortcuts. Uses ZMK's built-in key repeat. Placed on the layer shown above.",
    credit: "ZMK key repeat behavior; popular with “magic/repeat key” layouts",
    category: "Typing",
    params: [{ kind: "keys", id: "key", label: "Which key", count: 1, suggest: () => [] }],
    added: (c) => usesBehavior(c, "key_repeat"),
    build: (c, v) => {
      const key = (v.key as number[])[0];
      if (key === undefined) return "Pick the key on the board.";
      const title = c.layers.find((l) => l.index === c.layer)?.title ?? "";
      return { ops: [{ op: "set_binding", layer: c.layer, position: key, binding: "&key_repeat" }], changes: [`${title}, key ${key + 1} → Repeat`] };
    },
  },
  {
    id: "select_word",
    title: "Select word",
    summary: "One key selects the word under the cursor.",
    details: "Jumps to the start of the word and selects to its end — then type to replace it, or copy it. Placed on the layer shown above.",
    credit: "Popular text-editing macro in community keymaps",
    category: "Shortcuts",
    params: [
      { kind: "choice", id: "os", label: "Shortcuts for", options: OS, initial: "ctrl" },
      { kind: "keys", id: "key", label: "Which key", count: 1, suggest: () => [] },
    ],
    added: (c) => nameTaken(c, "select_word"),
    build: (c, v) => {
      const key = (v.key as number[])[0];
      if (key === undefined) return "Pick the key on the board.";
      const word = v.os === "cmd" ? "LA" : "LC";
      const n = unique(c, "select_word");
      const title = c.layers.find((l) => l.index === c.layer)?.title ?? "";
      return {
        ops: [
          { op: "add_def", def_type: "macro", name: n, props: [["bindings", { type: "bindings", value: ["&macro_tap", `&kp ${word}(RIGHT)`, `&kp ${word}(LEFT)`, `&kp ${word}(LS(RIGHT))`] }]] },
          { op: "set_binding", layer: c.layer, position: key, binding: `&${n}` },
        ],
        changes: [`Adds macro “${n}”`, `${title}, key ${key + 1} → Select word`],
      };
    },
  },
];
