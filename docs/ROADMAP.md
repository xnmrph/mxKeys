# mxKeys — Roadmap

Phases are done in order, one at a time. Tick boxes as work lands (keep this file and
`docs/PROGRESS.md` current at the end of every session).

## Phase 0 — Research & design
- [x] ZMK Studio RPC protocol docs (read from ZMK repo docs; clean-room policy noted)
- [x] `zmk-studio-messages` protobufs (MIT) reviewed
- [x] `zmk-studio-api` crate evaluated (0.6.1, Apache-2.0)
- [x] UF2 `INFO_UF2.TXT` format + nice!nano / ZMK USB identifiers
- [x] nice!view shield + widget/image implementation in ZMK
- [x] keymap-editor parsing approach + license (MIT; reimplemented, nothing copied)
- [x] `docs/ARCHITECTURE.md`
- [x] `docs/ROADMAP.md`, `docs/PROGRESS.md`

## Phase 1 — Scaffold + parsing
- [x] Cargo workspace, `crates/zmk-config`
- [x] Tauri v2 app skeleton (`src-tauri`) + React/TS/Vite frontend
- [x] Lossless devicetree lexer (tokens tile input exactly)
- [x] CST parser: nodes, ref-nodes, properties, cell arrays, strings, directives, macro calls, delete-node, error recovery
- [x] Keymap model: layers, display-name, bindings grouped per key, sensor-bindings
- [x] Behavior/combo/macro nodes listed generically (typed in Phase 7)
- [x] Span-patch edit engine (`set_binding`, `set_layer_display_name`) with re-parse
- [x] Round-trip tests on real-world-style keymaps (comments, preprocessor, CRLF, unicode)
- [x] `build.yaml` parser (include list + board/shield matrix form), target model, user's build.yaml fixture
- [x] Tauri command: open a config folder → summary (layers, targets)
- [x] CI workflow: fmt, clippy, tests (Linux/macOS/Windows), frontend typecheck/build
- [ ] Parse `config/west.yml` (ZMK revision, extra modules)
- [ ] Parse `.conf` (Kconfig lines, lossless)
- [ ] Clone a config repo from a URL (git2) in the first-run flow
- [ ] More real-world keymap fixtures (miryoku-style, urob-style helper macros) — ask user for their keymap
- [ ] Binding-grid re-alignment formatter (optional op)

## Phase 2 — Hardware detection + first-run setup
- [ ] `crates/hardware`: `HardwareProfile` types + Corne/nice!nano v2/nice!view profile data (physical layout from ZMK `foostan/corne/6column.dtsi`)
- [ ] Derive needed build targets from profile (left+nice!view+studio snippet, right+nice!view, settings_reset)
- [ ] Evidence/confidence model + combiner with conflict explanations
- [ ] Detector: build.yaml (boards, shields, displays, snippets → central/Studio half)
- [ ] Detector: west.yml / config files (extra modules, `.conf` display hints)
- [ ] `INFO_UF2.TXT` parser + detector (fixtures for nice!nano, XIAO BLE, nRFMicro)
- [ ] USB descriptor table (VID/PID/product) + detector (fixtures)
- [ ] `crates/probe`: traits `UsbEnumerator`, `DriveWatcher`, `SerialPorts` + mocks
- [ ] OS impls: Linux (mounts, udisks paths), macOS (`/Volumes`), Windows (drive letters)
- [ ] Guided quiz data (controllers: nice!nano, XIAO BLE, nRFMicro; displays: nice!view, 128×32 OLED, none) + illustrations
- [ ] First-run UI: open/clone repo → detect → summary "Corne · nice!nano v2 · nice!view — detected from …" → confirm/correct → start toolchain download in background
- [ ] Advanced view showing board ids/shields/snippets
- [ ] Manual hardware test checklist in PROGRESS.md

## Phase 3 — Toolchain + build
- [ ] `toolchain/manifest.toml` (all pinned versions + sha256 per host)
- [ ] Downloader with resume, sha256 verification, extraction (tar.xz, zip, 7z where needed)
- [ ] Zephyr SDK (arm-zephyr-eabi only) per host OS/arch
- [ ] Standalone Python (python-build-standalone) + uv-managed venv with west + Zephyr requirements
- [ ] cmake, ninja, dtc per platform (verify whether dtc is optional on Windows)
- [ ] West workspace: generated manifest importing user's west.yml; `--narrow`, depth 1; cache keyed by ZMK revision
- [ ] Module allow-list strategy + fallback (see ARCHITECTURE §6 OPEN)
- [ ] `west build` runner per target; streamed logs as Tauri events
- [ ] Log classifier → plain-language errors mapped to keymap positions
- [ ] Measure sizes; write `docs/build-toolchain.md` with trade-offs
- [ ] CI job building the Corne firmware on Linux, macOS and Windows runners

## Phase 4 — Flash
- [ ] UF2 file reader (family id, block count) + verification against profile
- [ ] Flash state machine (pure) + tests with mock drives
- [ ] Guided UI: left → right, optional settings_reset step, friendly copy
- [ ] Refuse mismatched firmware (Board-ID / family id)
- [ ] Manual hardware test on Linux/macOS/Windows

## Phase 5 — Keymap editor UI
- [ ] Visual Corne layout from physical layout data
- [ ] Layer tabs, add/remove/rename/reorder layers (file patches)
- [ ] Key binding editor with behavior picker + keycode search (plain names)
- [ ] Undo/redo, file watcher reload
- [ ] Read-only display for opaque/macro-generated bindings

## Phase 6 — Settings page
- [ ] Lossless `.conf` editor model
- [ ] Forms: `CONFIG_ZMK_IDLE_TIMEOUT`, `CONFIG_ZMK_SLEEP`, `CONFIG_ZMK_IDLE_SLEEP_TIMEOUT`
- [ ] BLE power (`CONFIG_BT_CTLR_TX_PWR_*`), RGB, display options
- [ ] "Advanced" raw Kconfig names

## Phase 7 — Advanced behaviors
- [ ] Macros, combos, hold-taps, mod-morphs, conditional layers, tap-dance, sticky keys
- [ ] Custom behavior nodes editing (typed CST edits)
- [ ] Helper-macro (e.g. `ZMK_HOLD_TAP(...)`) read support

## Phase 8 — Git
- [ ] Status, diff view, commit
- [ ] Push to any remote via system git (credential helper) with libgit2 fallback
- [ ] Clone/open flow polish

## Phase 9 — nice!view images/GIFs
- [ ] Import PNG/GIF, crop/resize to stock art area or full 160×68
- [ ] Dithering algorithms + threshold, live 1:1 preview
- [ ] Generate LVGL 9 `I1` C arrays + custom widget into the config repo
- [ ] Flash budget + frame-count/timing limits; rebuild

## Phase 10 — Live editing (Studio RPC)
- [ ] Write `docs/live-sync.md` (detailed design, review before coding)
- [ ] `StudioSession` trait; adapter over `zmk-studio-api` (serial first, BLE later)
- [ ] Behavior/keycode mapping tables generated from pinned ZMK revision
- [ ] Transactional live edits (device + file patch)
- [ ] On-connect diff device ↔ repo, conflict UI
- [ ] RPC-based hardware detection (device info + physical layouts)
