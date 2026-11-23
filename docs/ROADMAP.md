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
- [ ] Apply the same edit to the owner's real repo once they're happy (Phase 8 makes it a click; needs the owner's credentials)
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
- [x] Typed model (`zmk-config::behaviors`): hold-taps, mod-morphs, tap-dances, sticky keys, caps word, macros (incl. one/two-param), combos, conditional layers; property schemas from ZMK's devicetree bindings
- [x] Typed CST edits: set/insert/remove one property (minimal patch; same-count binding lists patch only changed entries; file's indent and multi-line style kept), add a definition (container created if missing), remove (refused while used)
- [x] Tune built-ins (`&mt`, `&lt`, `&sk`, `&sl`, `&caps_word`) via `&mt { … };`
- [x] Helper-macro read support (`ZMK_HOLD_TAP`, `ZMK_COMBO`, `ZMK_CONDITIONAL_LAYER`, …) — shown read-only
- [x] UI: tabs per type, forms from the schema, combo keys picked on a mini keyboard, macro steps list (press/tap/release modes, waits), same-keys combo warning, usage counts
- [x] Acceptance: new combo Z + X → Ctrl+Z created in the app on (a copy of) `xnmrph/corne` → 5-line diff → firmware builds, combo present in the compiled devicetree
- [ ] Editing helper-macro definitions (rewrite macro arguments)
- [ ] Renaming a behavior (label + every `&label` use)
- [ ] Leader keys, adaptive keys and other module behaviors (shown under "Other" with raw properties)

## Phase 8 — Git
- [x] Status (branch, ahead/behind, changed files), diff view with line numbers, commit (all or selected files)
- [x] Push / fetch / pull to any remote via system git (credential helpers, SSH agent, signing, hooks) with libgit2 fallback (HTTPS)
- [x] Pull: fast-forward, or rebase local versions on top (aborted cleanly on conflict); rejected pushes explained
- [x] Plain-language summary of each changed keymap/.conf + suggested commit message
- [x] Commit identity asked once, stored in the repo's `.git/config`; credentials never stored, URLs redacted
- [x] Undo uncommitted changes per file (native confirmation); start a history (`git init`) for plain folders
- [x] Clone/open polish: recently opened folders on the welcome screen
- [x] Acceptance: in the real app, on a clone of `xnmrph/corne` with a local test server — OUT_TOG key + screen on → save & upload (commit by the entered identity with the generated description) → server-side edit → “Get latest” fast-forwards and the files update
- [ ] Apply the owner's pending edits to the real `xnmrph/corne` (owner opens their clone in mxKeys and clicks Upload — needs their credentials)
- [ ] Branches (switch/create) and resolving conflicts in the app
- [ ] Commit history view with per-version diffs; revert a version

## Phase 9 — nice!view images/GIFs
- [x] Import PNG/GIF/JPEG/BMP/WebP (animated GIF frames + delays); fit (fill/fit/stretch) to the right half's picture area (68×140 under the battery strip) or the whole screen (68×160) on either half
- [x] Dithering: soft (Atkinson), fine (Floyd–Steinberg), patterns (Bayer 4/8), sharp threshold; brightness/contrast/mid point, invert, background; live animated preview at 3× and 1:1
- [x] LVGL 9 `I1` encoding in the display's orientation — verified byte-for-byte against ZMK's own `balloon` art
- [x] Generated ZMK module in the config repo (`nice_view_art/`, two lines in `zephyr/module.yml`): image placed over ZMK's stock screen on the display work queue, animation paused while idle, respects `NICE_VIEW_WIDGET_INVERTED`; originals + settings kept for re-editing; removable
- [x] Flash budget per half (default 64 KB, up to 192 KB) + minimum frame time; frames merged/sampled with the speed kept
- [x] Rebuild hint covers the pictures; one commit-message line for picture changes
- [x] Acceptance: owner's config (copy) — left full-screen still + right 6-frame GIF chosen in the app → built in the app → frames read back from the compiled `zmk.elf` decode to exactly the previews
- [ ] Manual hardware test: pictures visible and upright on real nice!views, animation pauses when idle (checklist in PROGRESS.md)
- [ ] Crop/zoom by dragging in the preview; per-frame editing
- [ ] Pictures for other displays (128×32 OLED)

## Phase 10 — Live editing (Studio RPC)
- [x] Write `docs/live-sync.md` (detailed design, reviewed) — owner chose: Save button, own client, offer to clear saved changes, layers stay file edits, USB **and** Bluetooth
- [x] Own client `crates/studio`: framing, hand-written prost messages checked against the vendored `.proto` files, request/response client with notifications
- [x] Simulator (`sim.rs`) with ZMK's rules (lock, per-keyboard behavior ids, RAM vs saved, reset) — sync engine tested against it on all three OSes
- [x] Transports: USB serial (found by VID/PID, permission hint) with a pty test; Bluetooth via btleplug (needs pairing)
- [x] Mapping source ⇄ device: mini C preprocessor over ZMK's headers and the user's includes, behavior display names; verified on the owner's keymap against a real `zephyr.dts` (252/252 bindings)
- [x] Transactional live edits: keyboard first, then the file patch; rollback of the keyboard if the file edit fails
- [x] Compare keyboard ↔ files on connect and on every file reload; per-key “Keep keyboard's” / “Send files'”, layer-name differences, unchecked keys explained
- [x] Save on keyboard / Discard (keyboard and file together, undoable); offer to clear saved live changes after a firmware install
- [x] App: connect panel (USB, Bluetooth scan, simulated keyboard in developer mode), unlock hint with the layer/key of `&studio_unlock`, live mode in the key editor
- [ ] Real-hardware checks (checklist in PROGRESS.md)
- [ ] RPC-based hardware detection (device info + physical layouts → Phase 2 evidence); the client already reads both
- [ ] Live layer add/remove/move (decision D: not in v1)
