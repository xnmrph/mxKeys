# Progress log

Newest first. Each session: what was done, what's next, open questions, and anything
that needs testing on real hardware.

## Session 1 — 2026-11-22

### Done
- **Research** (sources read from cloned repos, because `zmk.dev` was not reachable
  from the cloud environment — the docs are the same Markdown in `zmk/docs/`):
  - ZMK Studio RPC protocol + clean-room policy; `zmk-studio-messages` protos (MIT).
  - `zmk-studio-api` crate 0.6.1 (Apache-2.0, serial + optional BLE) — looks usable
    behind our own trait; see ARCHITECTURE §9.2.
  - nice!view shield/widget sources (LVGL 9, 1-bit, 160×68; peripheral art images are
    140×68 `LV_COLOR_FORMAT_I1`, 68×68 status canvas). Note: the brief said "~68×68
    picture area"; in current ZMK `main` the stock art is 140×68 under a 68×68 canvas —
    to verify on the real screen in Phase 9.
  - keymap-editor (MIT): its devicetree parsing uses tree-sitter-devicetree (WASM) on the
    `devicetree-parser` branch. We wrote our own parser; nothing copied.
  - ZMK `main` = Zephyr `v4.1.0+zmk-fixes`, CI image `zmk-build-arm:4.1`; latest tag
    `v0.3.0` is on older Zephyr. `nice_nano@2.0.0` naming = ZMK `main` (HWMv2).
  - USB ids: ZMK firmware default `1D50:615E` (from ZMK `app/Kconfig`). nice!nano
    bootloader `239A:00B3` and the `INFO_UF2.TXT` sample are from memory/common
    knowledge — **verify on hardware**.
- `docs/ARCHITECTURE.md`, `docs/ROADMAP.md`, this file.
- **Phase 1** (most of it):
  - Cargo workspace; `crates/zmk-config` with lossless lexer, error-recovering CST parser,
    typed keymap model, span-patch edits (`set_binding`, `set_layer_display_name`),
    `build.yaml` parser, repo discovery.
  - Tests: round-trip on 4 fixtures (+ CRLF variants), model tests, minimal-diff edit
    tests, 600-case mutation robustness test, user's `build.yaml` fixture.
  - Tauri v2 app (`src-tauri`) with `open_config_repo` and `set_binding` commands;
    React UI: open folder → layer tabs on a visual Corne layout → click key → edit binding
    (writes the file) → plain-language build target list with "Advanced" table.
    Verified in Chromium with a mocked Tauri bridge fed by the real Rust summary JSON.
  - CI: frontend build; fmt + clippy + tests on Ubuntu, macOS, Windows.

### Owner's real config repos (found after GitHub access was granted)
- `xnmrph/corne` — **the active config** (matches the brief's build.yaml): ZMK `main`
  (unpinned branch), extra west modules `urob/zmk-helpers` and `urob/zmk-unicode`,
  Norwegian keys via `config/keys_nb.h`, 6 layers, 5 custom behaviors, 13 macros,
  19 combos. Layer nodes are named after `#define`d layer numbers (`DEF`, `NAV`, …).
- `xnmrph/corne-macOS-Optimised` — older variant on ZMK `v0.3`, legacy board id
  `nice_nano_v2`, already builds only the needed variants.
- Both keymaps parse losslessly with zero diagnostics, all layers editable; added as
  fixtures (`keys_nb.h` not copied — Unicode license) with model + minimal-edit tests.
- Implications: the toolchain must fetch extra modules from the user's `west.yml`
  (affects the allow-list idea in ARCHITECTURE §6); `revision: main` needs the app-side
  lockfile (§7); the two repos span two Zephyr/SDK generations, a good upgrade-assistant
  test case.

### Next (in order)
1. Check the first CI run on GitHub (macOS/Windows never built locally).
2. Finish Phase 1: `west.yml` parser, lossless `.conf` model, clone-from-URL.
3. Parse `west.yml` using the owner's two repos as fixtures (modules, `defaults.revision`).
4. Phase 2: `crates/hardware` profile + detection, `crates/probe` traits + mocks.

### Open questions for the user
- App name: mxKeys / Keywright / Cornerstone (recommendation: mxKeys) — ARCHITECTURE §12.
- Edits write to disk immediately (with undo + git as safety net) vs explicit Save?
  Current prototype writes immediately.
- Toolchain module strategy (ARCHITECTURE §6): recommend allow-list with automatic
  fallback to the full manifest.
- ~~Which repo is canonical?~~ **Answered:** `xnmrph/corne` (ZMK `main`). Owner doesn't know
  which firmware is currently flashed.
- Host setup: **mainly Linux**; macOS reached via a **software KVM** (Deskflow-style) or
  by switching to **another Bluetooth profile** on the keyboard. No hardware KVM.

### Notes from the owner's answers
- "Which firmware am I on?" can't be fully answered from the device: ZMK doesn't report
  its version over USB or Studio RPC (device info = name + serial only). What detection
  *can* tell: bootloader/controller (`INFO_UF2.TXT`), whether firmware is ZMK (USB
  `1D50:615E`, product = keyboard name), whether Studio is enabled (CDC serial port,
  RPC answers). Plan for Phase 2/4: say this plainly, and offer "flash from your repo
  (+ settings reset)" so the firmware is known from then on; record the flashed repo
  commit per half in app data.
- Linux-first: Linux is the primary dev *and* user platform; UF2 drive detection on
  Linux (udisks automount, NixOS) gets tested first. Studio-over-serial only works on
  the machine the keyboard is plugged into, which may not be the macOS side of a KVM.
- The keymap has both Windows- and macOS-style æ/ø/å macros; on a Norwegian Linux
  layout the native `keys_nb.h` keys are what's needed. A future "which computer do you
  type on" setting could pick the right variant (Phase 7 idea).

- Owner's keymap already has `&bt BT_SEL 0/1/2`, `BT_CLR`, `BT_CLR_ALL` on the *System*
  layer, but **no `&out` key**. ZMK sends over USB while USB is connected, so with the
  keyboard cabled to Linux, picking the Mac's Bluetooth profile won't reach the Mac until
  output is switched (`&out OUT_BLE` / `OUT_TOG`). Suggest adding an output toggle next to
  the BT keys (owner's decision; first real edit candidate once Phase 5 lands).
- Implications for the app: a "computers" concept (profile 1 = Linux, 2 = Mac) in the UI
  to label BT keys; Studio over BLE (Phase 10, `zmk-studio-api` `ble` feature) becomes
  more useful; settings reset clears pairings — the flash flow must warn and walk through
  re-pairing both computers.

### Needs manual testing (no hardware / display in the cloud environment)
- `npm run tauri dev` on NixOS (try `nix-shell` with the provided `shell.nix`, untested).
- Native folder picker on Linux/macOS/Windows.
- Later phases: everything touching USB, UF2 drives and serial.
