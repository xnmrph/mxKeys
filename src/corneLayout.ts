// Corne 6-column physical layout (42 keys). Positions/sizes in 1/100 key units, rotation
// in 1/100 degrees around (rx, ry) — same convention as ZMK's `zmk,physical-layout`
// (`app/dts/layouts/foostan/corne/6column.dtsi`). Phase 2 moves this into the Rust
// hardware profile and serves it to the UI.
export interface KeyGeom { w: number; h: number; x: number; y: number; r: number; rx: number; ry: number }

const k = (w: number, h: number, x: number, y: number, r = 0, rx = 0, ry = 0): KeyGeom => ({ w, h, x, y, r, rx, ry });

const rowY = [37, 37, 12, 0, 12, 24, 24, 12, 0, 12, 37, 37];
const rowX = [0, 100, 200, 300, 400, 500, 800, 900, 1000, 1100, 1200, 1300];

export const CORNE_6COL: KeyGeom[] = [
  ...[0, 100, 200].flatMap((dy) => rowX.map((x, i) => k(100, 100, x, rowY[i] + dy))),
  k(100, 100, 350, 312),
  k(100, 100, 450, 312, 1200, 450, 412),
  k(100, 150, 548, 283, 2400, 548, 433),
  k(100, 150, 752, 283, -2400, 852, 433),
  k(100, 100, 850, 312, -1200, 950, 412),
  k(100, 100, 950, 312),
];
