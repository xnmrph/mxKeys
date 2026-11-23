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
- [x] Tauri command: open a config folder → summary (layers, targets); `set_binding` writes the file
- [x] Minimal UI: layer tabs on a visual Corne layout, edit one key's binding, build targets list
- [x] CI workflow: fmt, clippy, tests (Linux/macOS/Windows), frontend typecheck/build — green on Linux, macOS, Windows (run 5, 2026-11-22)
- [x] Parse `config/west.yml` (ZMK revision + kind, extra modules, resolved URLs)
- [x] Parse `.conf` (Kconfig lines, lossless; set / uncomment / append / comment-out edits)
- [x] Clone a config repo from any git URL (system git preferred, bundled libgit2 fallback) with progress, in the welcome screen
- [x] Owner's real keymaps as fixtures (`xnmrph/corne`, `xnmrph/corne-macOS-Optimised`)
- [x] Macro-generated keymaps (zmk-helpers `ZMK_LAYER`, keymap in another file) explained instead of failing; bare macros as node bodies parsed (checked locally against urob/zmk-config, which has no license so isn't vendored)
- [~] Binding-grid re-alignment formatter — moved to Phase 5 (needed with the editor UI)

## Phase 2 — Hardware detection + first-run setup
- [x] `crates/hardware`: `HardwareProfile` types + Corne/nice!nano v2/nice!view profile data (physical layout from ZMK `foostan/corne/6column.dtsi`)
- [x] Derive needed build targets from profile (left+nice!view+studio snippet, right+nice!view, settings_reset)
- [x] Evidence/confidence model + combiner with conflict explanations
- [x] Detector: build.yaml (boards, shields, displays, snippets → central/Studio half)
- [x] Detector: `.conf` (Studio, screen switched on/off); west.yml extra modules shown in the repo view (no hardware facts yet)
- [x] `INFO_UF2.TXT` parser + detector (fixtures for nice!nano, XIAO BLE, nRFMicro, RP2040 — synthesised, verify on hardware)
- [x] USB descriptor table (VID/PID/product) + detector (fixtures)
- [x] `crates/probe`: traits `UsbEnumerator`, `DriveScanner` + mocks (serial ports come with Phase 10)
- [x] OS impls: Linux (`/proc/self/mounts` FAT mounts + udisks paths), macOS (`/Volumes`), Windows (drive letters) — Linux exercised; macOS/Windows only via CI smoke test
- [x] Guided quiz data (controllers: nice!nano, XIAO BLE, nRFMicro; displays: nice!view, 128×32 OLED, none) + original SVG illustrations
- [x] First-run UI: detect → summary "Corne · nice!nano v2 · nice!view — detected from …" → confirm/correct; live re-check while a half is in bootloader mode; profile remembered per config folder (app data, not the repo)
- [x] Advanced view showing evidence, board ids/shields/snippets
- [x] Manual hardware test checklist in PROGRESS.md
- [ ] Start the toolchain download in the background after confirming (needs Phase 3)
- [ ] Keyboard question in the quiz (only matters once a second keyboard is supported)
- [ ] Replace synthesised INFO_UF2.TXT / USB fixtures with real captures

## Phase 3 — Toolchain + build
- [x] `toolchain/manifest.toml` (all pinned versions + sha256 per host) — SDK 0.16.9 (matches ZMK's build image), stripped Python 3.12, CMake 3.31, ninja; MinGit for Windows
- [x] Downloader with resume, sha256 verification, extraction (tar.xz, tar.gz, zip, 7z)
- [x] Zephyr SDK (arm-zephyr-eabi only) per host OS/arch, trimmed to the needed multilib (1.1 GB → 256 MB, byte-identical firmware)
- [x] Standalone Python + venv with west + Zephyr base / nanopb requirements (no uv needed; pip is enough)
- [x] cmake, ninja per platform; dtc verified unnecessary (Zephyr 4.1 only uses it for extra warnings)
- [x] West workspace: generated manifest from the user's west.yml; `--narrow`, depth 1; shared per manifest; lock of resolved commits
- [x] Module allow-list strategy + automatic fallback to the full manifest (ARCHITECTURE §6 option c)
- [x] `west build` runner per target; streamed logs as Tauri events
- [x] Log classifier → plain-language errors mapped to layer/key (from real error output)
- [x] Measure sizes; write `docs/build-toolchain.md` with trade-offs
- [x] App: Firmware panel (auto background tool download after hardware confirm, Build button, per-target status, problems, log, versions)
- [x] CI job building the Corne firmware on Linux, macOS and Windows runners — green (Firmware run 3: Linux 3 min, macOS 6.5 min, Windows 8 min with bundled MinGit, all cold)
- [ ] Hash-locked Python constraints per manifest
- [ ] git-free builds (own manifest resolver + libgit2 + direct CMake) — see build-toolchain.md "Open: git"
- [ ] Skip the SDK "minimal" download by shipping its CMake package files
- [ ] Cancel button for running builds/downloads; disk-usage view + "remove old toolchains"

## Phase 4 — Flash
- [x] UF2 file reader (magic, family id, blocks) + verification against the profile (`crates/flash`)
- [x] Flash state machine (pure) + tests with temp-dir and mock drives
- [x] Guided UI: optional settings reset (both halves) → left → right, illustrations, plain-language steps, retry/skip/stop
- [x] Refuse mismatched firmware (drive Board-ID/Model, UF2 family id) — nothing is written
- [x] Copy tolerates the bootloader resetting while the file closes
- [x] Flash existing firmware without rebuilding
- [ ] Manual hardware test on Linux/macOS/Windows (checklist in PROGRESS.md)
- [ ] Detect a fast replug during "restarting" (fresh mount lacks our file) instead of the "still there" hint
- [ ] After flashing: offer to check the keyboard over USB (ZMK device appears with the new name)

## Phase 5 — Keymap editor UI
- [x] Visual Corne layout from physical layout data (served by the Rust hardware catalog)
- [x] Layer tabs: add, delete (refused while referenced), rename, move — all minimal file patches
- [x] Moving/removing layers renumbers every layer reference (numbers, `#define`s, combos, conditional layers)
- [x] Key picker: behavior by plain name → typed parameters (searchable keycodes with categories and modifier toggles, layer names, command choices), Advanced raw text
- [x] Keycode catalog generated from ZMK `keys.h` (`tools/gen_keycodes.py`), keycap labels using layer names
- [x] Custom behaviors from the keymap in the picker (hold-tap sides typed from their bindings)
- [x] Aligned edits: the gap after a key absorbs length changes (only the edited line changes)
- [x] Undo/redo (buttons + Ctrl/⌘+Z), file-change detection with automatic reload
- [x] Read-only display for opaque/macro-generated bindings
- [x] Acceptance test: `&out OUT_TOG` added next to the BT keys on the *System* layer of (a copy of) `xnmrph/corne` using only the app — `git diff` shows one changed line
- [ ] Apply the same edit to the owner's real repo once they're happy (needs push access / Phase 8)
- [ ] Drag & drop keys between positions; copy/paste keys
- [ ] Sensor bindings (encoders) editing; non-Corne layouts
- [ ] Re-align a whole bindings block on request ("tidy up")

## Phase 6 — Settings page
- [x] Lossless `.conf` editor model (Phase 1) + settings catalog with plain names, units, ranges, defaults (`zmk-config::settings`)
- [x] Forms: `CONFIG_ZMK_IDLE_TIMEOUT`, `CONFIG_ZMK_SLEEP`, `CONFIG_ZMK_IDLE_SLEEP_TIMEOUT`
- [x] BLE power (`CONFIG_BT_CTLR_TX_PWR_*` choice), keyboard name, RGB, display options, typing (HKRO/NKRO, mouse keys, debounce), Studio
- [x] "Use default" (comments the line out), choices, companion options (RGB → LED driver)
- [x] "Advanced" raw Kconfig names, other options in the file, set any option
- [x] Undo/redo + outside-change reload; "rebuild needed" notice when config files are newer than the firmware
- [x] Acceptance: screen switched on in (a copy of) `xnmrph/corne` → one-line diff → build has `CONFIG_ZMK_DISPLAY=y` + LVGL + nice!view widget
- [ ] Refresh the hardware panel's hints (e.g. "screen switched off") after a settings edit
- [ ] Profile-dependent catalog (defaults/options per board and shield) once more hardware is supported
- [ ] Per-half settings (`corne_left.conf` vs `corne_right.conf`) explained in the UI

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
