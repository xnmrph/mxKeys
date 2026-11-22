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

### Next (in order)
1. Check the first CI run on GitHub (macOS/Windows never built locally).
2. Finish Phase 1: `west.yml` parser, lossless `.conf` model, clone-from-URL.
3. Ask the user for their real `.keymap` as an extra round-trip fixture.
4. Phase 2: `crates/hardware` profile + detection, `crates/probe` traits + mocks.

### Open questions for the user
- App name: mxKeys / Keywright / Cornerstone (recommendation: mxKeys) — ARCHITECTURE §12.
- Edits write to disk immediately (with undo + git as safety net) vs explicit Save?
  Current prototype writes immediately.
- Toolchain module strategy (ARCHITECTURE §6): recommend allow-list with automatic
  fallback to the full manifest.
- Can you share your actual `config/corne.keymap` and `corne.conf`?

### Needs manual testing (no hardware / display in the cloud environment)
- `npm run tauri dev` on NixOS (try `nix-shell` with the provided `shell.nix`, untested).
- Native folder picker on Linux/macOS/Windows.
- Later phases: everything touching USB, UF2 drives and serial.
