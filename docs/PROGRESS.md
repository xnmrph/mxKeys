# Progress log

Newest first. Each session: what was done, what's next, open questions, and anything
that needs testing on real hardware.

## Session 3 (cont.) — 2026-11-23 — Phase 10 (design)

### Done
- `docs/live-sync.md`: design for live editing, checked against ZMK 4de3133 and
  `zmk-studio-messages`. Findings that shape it:
  - Studio changes are RAM-only until saved; **saved changes override the compiled
    keymap even after flashing new firmware** (separate settings partition).
  - `core.reset_settings` clears only keymap overrides (Bluetooth pairings stay).
  - Behavior ids are assigned per keyboard (settings table) → mapped by display name;
    display name = `display-name` ?? `label` ?? node name (the owner's macros show as
    “AE”, `&hm` as “homerow_mods”).
  - The build's `zephyr.dts` contains the fully resolved keymap (numbers, defines and
    helper macros expanded) → exact expected device keymap.

### Waiting for
- The owner's answers to decisions A–E at the end of `docs/live-sync.md` before coding.

## Session 3 (cont.) — 2026-11-23 — Phase 9

### Done
- `crates/niceview`: decode (PNG, GIF with frame delays, JPEG, BMP, WebP), fit, adjust,
  dither (Atkinson, Floyd–Steinberg, Bayer 4/8, threshold), frame limits (minimum
  frame time, identical-frame merging, even sampling to the flash budget with the
  total time kept), LVGL `I1` encoding, generated module, save/remove in a repo.
- Orientation established from ZMK's own art: the stored image turned 90°
  counter-clockwise is what you see on the Corne, so frames are stored turned
  clockwise; a fixture of ZMK's `balloon` round-trips byte-for-byte.
- Design: no fork of ZMK's nice!view code. A tiny module (`nice_view_art/src/art.c`)
  waits until ZMK's display is initialised, then adds an LVGL image on top of the
  stock screen from the display work queue (same thread as ZMK's widgets). Right half:
  over the stock 140×68 art (battery strip stays); either half: whole screen.
  Animations use an LVGL timer, paused on `zmk_activity_state_changed` idle.
- Flash: program area 792 KiB; Corne halves with the screen use ~430 KiB (left) /
  ~315 KiB (right) at ZMK 4de3133, so the default 64 KiB per half (≈47 full frames)
  is safe; up to 192 KiB allowed.
- The owner's repo already has `zephyr/module.yml` (`board_root: .`); mxKeys adds two
  lines (`cmake`, `kconfig`) and removes them again on “Remove pictures”.
- App: “Screen pictures” panel (shown for nice!view profiles).
- **Acceptance test** on a copy of `xnmrph/corne` (screen on): picked a gradient logo
  (left, whole screen) and a 12-frame bouncing-ball GIF (right, under the battery) in
  the app; saved; built in the app. The ELF has `mxk_left_0_map` (1,368 bytes) and
  `mxk_right_0…5_map` (1,232 bytes each); decoding them from `zmk.elf` gives exactly
  the app's previews.

### Hardware checklist (needs the real keyboard)
- [ ] Right half: picture under the battery strip, upright, animation runs; battery
      and connection icons still update.
- [ ] Left half whole screen: picture upright, covers the status.
- [ ] Animation stops after the idle timeout and resumes on a key press.
- [ ] `CONFIG_NICE_VIEW_WIDGET_INVERTED=y` inverts the pictures too.
- [ ] Remove pictures → rebuild → stock screens again.

## Session 3 (cont.) — 2026-11-23 — Phase 8

### Done
- `crates/git/src/repo.rs`: status, diff, file-at-HEAD, log, commit (all/selected),
  fetch, pull (fast-forward / rebase with clean abort), push (sets upstream on first
  push), discard, init, identity. Tested against local bare “servers” with **both**
  backends (libgit2 and system git): rejected push → pull → push, fast-forward, blocked
  pull with local edits, first push of a new repo.
- `zmk-config::changes`: plain-language change lists for keymaps (keys per layer, layer
  add/remove/rename/reorder, combos/macros/behaviors added/removed/changed) and `.conf`
  (settings by their plain names, other options raw).
- App: “Changes and upload” panel, background server checks, recent folders on the
  welcome screen.
- Found and fixed: `window.confirm` doesn't block in the Tauri webview (it returned at
  once, so “Undo changes” ran without asking). All confirmations now use the native
  dialog (`dialogs.ts`); verified Cancel keeps the change and confirming discards it.
- **Acceptance test** in the real app with a clone of `xnmrph/corne` and a local bare
  repository as the server: set `&out OUT_TOG` above “BT forget” and turned the screen
  on → the panel listed “System: key 17 &trans → &out OUT_TOG” and “Screen: off → on”
  → asked for a name/email (empty HOME, as on a fresh machine) → “Save version and
  upload” → the server has the commit “Update keymap and settings” with both bullets.
  Then a commit pushed from another clone (“Rename keyboard on the website”) was found
  by “Check the server” and applied with “Get latest”.
- 151 tests.

### For the owner
- To apply the pending edits to the real repo: open your local clone of `xnmrph/corne`
  in mxKeys, make the edits (or keep the ones already made), and click **Save version
  and upload**. Pushing uses your normal git login (credential helper / SSH key); mxKeys
  never sees a token.

### Next
1. Phase 9: nice!view images.

## Session 3 (cont.) — 2026-11-23 — Phase 7

### Done
- `zmk-config::behaviors`: schema-driven typed view of every definition (types, labels,
  defaults and help per property; checked against ZMK's `dts/bindings` YAML at 4de3133 —
  e.g. hold-tap `tapping-term-ms` is required, flavor defaults to hold-preferred, combo
  timeout 50 ms, macro wait/tap 15/30 ms). Reads layer `#define`s, numeric defines,
  `<&a &b>` groups in macros, `(MOD_LSFT|MOD_RSFT)`, and how often each `&label` is used.
- Edits are property-level patches; adding creates the node (and the `combos` /
  `behaviors` / `macros` container if needed) in the file's own indentation; removing
  is refused while keys use the behavior.
- zmk-helpers macros (`ZMK_HOLD_TAP(…)`, `ZMK_COMBO(…)`, …) are parsed and shown
  read-only.
- App: “Combos, macros and special keys” panel under the keymap; macro steps in the key
  picker only when editing a macro; same-keys combo warning (layer-aware).
- Keypad keys now show as “Num 0”, “Num −” (generator rule; only labels changed).
- **Acceptance test** in the real app on a copy of `xnmrph/corne`: created combo
  “Undo” (Z + X → Ctrl+Z) with the mini keyboard and key picker → 5-line diff → built
  in the app; `zephyr.dts` contains `key-positions = <0x19 0x1a>` and `&kp 0x107001d`.
- Owner's keymap read correctly: 19 combos, 13 macros, the hold-tap `&hm` (currently
  unused by any key), 2 mod-morphs, tap dance, caps word, conditional layer.
- 141 tests.

### Next
1. Phase 8: git (status, diff, commit, push) — also how the owner's pending edits
   (OUT_TOG key, screen on) reach their real repo.

## Session 3 (cont.) — 2026-11-23 — Phase 6

### Done
- `zmk-config::settings`: catalog of 21 settings in six groups (battery and sleep,
  Bluetooth, screen, lights, typing, live editing). Defaults and ranges from ZMK `main`
  4de3133 Kconfig files and a real build's `.config` (e.g. sleep is off by default,
  idle 30 s, deep sleep 15 min, TX power 0 dBm, HKRO, nice!view screen on).
- Edit semantics: replace / uncomment / append; "Use default" comments out; choices
  comment out the other options; RGB also switches on the LED-strip driver.
- App: settings page replaces the read-only list — toggles, numbers in seconds/minutes,
  name with a 16-byte counter, dropdowns, greyed-out dependent settings, undo/redo,
  outside-change reload, Advanced (raw names, other options, set any option).
- Firmware panel says "rebuild needed" when `config/*` or `build.yaml` is newer than the
  built firmware.
- **Acceptance test** in the real app (Xvfb) on a copy of `xnmrph/corne`: Screen → On
  gives a one-line diff (`CONFIG_ZMK_DISPLAY=n` → `=y`); building from the app gives
  `CONFIG_ZMK_DISPLAY=y`, `CONFIG_LVGL=y`, `CONFIG_NICE_VIEW_WIDGET_STATUS=y` on both
  halves (left UF2 550 KB → 880 KB). Toggling another option afterwards shows the
  rebuild notice.
- 133 tests.

### Notes
- Appending to a file that ends in a blank line puts the new option after that blank
  line (harmless; kept simple).
- The hardware panel's "your settings switch the screen off" hint only refreshes when
  the folder is reopened.

### Next
1. Phase 7: advanced behaviors (macros, combos, hold-taps, …).
2. Still open with the owner: apply the OUT_TOG and screen edits to the real repo
   (needs push access or Phase 8), and fix `build.yaml`'s board id.

## Session 3 (cont.) — 2026-11-23 — Phase 5

### Done
- Keycode catalog generated from ZMK `keys.h` @ 4de3133 (373 keys, 583 names incl.
  aliases; shifted symbols; deprecated names accepted but not offered). Generator:
  `tools/gen_keycodes.py`. Every key name in the owner's keymaps resolves.
- Behavior catalog: built-ins with typed parameters (keys, hold-tap, layers, Bluetooth,
  USB/BT output, RGB, external power, mouse, system) + the keymap's own behaviors.
- Edit ops in `zmk-config`: aligned key edits; add/remove/move/rename layers; layer
  references renumbered on move/remove (numbers and `#define`s incl. combos and
  conditional layers; refs inside the moved layer too). Invariant test on the owner's
  keymap: after five different moves, every reference still points at the same layer.
- App: key picker, layer tools, undo/redo, outside-change reload (2 s polling of a content
  hash), keycaps show layer names and short Bluetooth/output labels.
- **Acceptance test passed** in the real app on a copy of `xnmrph/corne`: `&out OUT_TOG`
  (“Toggle USB ↔ Bluetooth”) above “BT forget” on the System layer; one-line diff.
  Also verified: undo/redo restore the file exactly; an outside `sed` edit is noticed
  and reloaded; adding a “Gaming” layer copies the grid.
- Found a WebKitGTK quirk: `&` in `<optgroup>` labels renders wrongly in the native
  select popup — avoid `&` in option/group labels.

### Next
1. Phase 6: settings page (Kconfig forms: idle/sleep timers first; screen on/off —
   the owner's `CONFIG_ZMK_DISPLAY=n` is the first real use).
2. Ask the owner whether to apply the OUT_TOG edit to the real repo now.

## Session 3 (cont.) — 2026-11-23 — Phase 4

### Done
- `crates/flash`: UF2 parsing (moved from toolchain), drive/firmware verification,
  resilient copy, guided state machine; 8 tests incl. a full reset+flash run against
  temp-dir "drives" found by the real scanner.
- App: "Install on the keyboard" wizard under Firmware — works for firmware built
  earlier too; optional "Reset settings first"; step list, half illustration with a
  pulsing reset button, copy progress, refusal/failed states with Try again, Skip, Stop.
- Verified in the real app (Xvfb) with simulated drives at `/run/media/<user>/NICENANO`:
  wrong device refused (nothing written), left/right got byte-identical firmware,
  restart detection, "All done!".

### Manual hardware test checklist — flashing (needs the real keyboard)
1. Build in the app, then Install → Start. Left half: plug in, double-tap reset.
   Expect: copy progress, "restarting…", then the right-half step. Note how long the
   drive takes to disappear and whether the OS shows an "eject"/"not ejected properly"
   warning (macOS does — we may want to tell users that's expected).
2. Right half, same. Then unplug/replug both: do they connect to each other?
3. With "Reset settings first": four steps; after flashing, re-pair Bluetooth hosts.
4. Plug in some other USB stick during the flow: must be ignored (no INFO_UF2.TXT).
5. Record the real `INFO_UF2.TXT` (copy it off the drive) → fixtures.
6. Windows: drive letter detection; macOS: `/Volumes/NICENANO`; NixOS: needs an
   automounter (udisks2) or the drive is never mounted.

- Firmware CI is green on all three OSes (run 3): the managed toolchain builds the
  pinned Corne test config from scratch on Linux (3 min), macOS (6.5 min) and Windows
  (8 min, bundled MinGit forced). Two CI-only bugs were fixed on the way (empty env var
  forcing bundled git; a Windows path separator in a test).

### Next
1. Phase 5: keymap editor UI (visual binding editor, layer management, undo, file
   watcher), including the acceptance test: add `&out OUT_TOG` on the System layer.

## Session 3 — 2026-11-23 — Phase 3

### Done
- Manual end-to-end build first (to learn, not guess): your `xnmrph/corne` builds for
  left (nice!view + Studio), right and settings reset with a fully app-managed
  toolchain. Findings that changed the plan:
  - **SDK must be 0.16.9** (ZMK's build image); 0.17.4 fails on Zephyr 4.1's picolibc.
  - **Board id must be `nice_nano@2.0.0//zmk`** on ZMK main. The owner's build.yaml
    (`nice_nano@2.0.0`) fails for the nice!view targets — likely why their GitHub
    builds with the screen fail. Detection now warns about it.
  - dtc not needed; `west zephyr-export` avoided via `-DZephyr_DIR`; nanopb needs
    `grpcio-tools` (bundles protoc); `west packages pip` is overkill (285 MB venv).
  - Your built firmware confirms the screen is off (`# CONFIG_ZMK_DISPLAY is not set`)
    and the USB name is "Corne xnmrph".
- Size work, all verified byte-identical: ARM toolchain multilib trim 1.1 GB → 256 MB;
  workspace allow-list 3.0 GB → 1.2 GB; stripped Python; total ≈ 1.85 GB vs 4.9 GB.
  `docs/build-toolchain.md` has the numbers and trade-offs.
- `crates/toolchain`: manifest, resumable verified downloads, extraction (tar.xz/gz,
  zip, 7z) with traversal protection, trimming, idempotent install with markers,
  venv health check, workspace generation + lock, `west build` (as `python -m west`),
  UF2 check, log classifier with layer/key, pipeline + `mxkeys-build` CLI. 31 tests.
- App: Firmware panel after hardware confirmation — background tool download,
  Build button, per-target status, plain-language problems, log, exact versions.
  Verified in the real app (Xvfb): Build → three `.uf2` files.
- CI: `firmware.yml` builds a pinned test config on Linux/macOS/Windows (Windows forces
  the bundled MinGit). First run found a bug (empty env var forced bundled git) — fixed.

### Open questions / decisions for the owner
- Fix your `build.yaml` board id (`nice_nano@2.0.0//zmk`)? The app builds correctly
  regardless; this only matters for your GitHub Actions builds.
- Git on macOS still needs Apple's Command Line Tools (see build-toolchain.md "Open: git").

### Next
1. Watch the firmware CI on macOS/Windows; fix what it finds.
2. Phase 4: guided UF2 flashing (UF2 parsing already exists in `crates/toolchain/src/uf2.rs`;
   move it into a `flash` crate).

## Session 2 (cont.) — 2026-11-23 — Phase 2

### Done
- `crates/hardware`: catalog (Corne, nice!nano v2, nice!view supported; nice!nano v1,
  XIAO BLE, nRFMicro, 128×32 OLED recognised only), `HardwareProfile` → needed build
  targets, pure detectors (build.yaml, .conf, INFO_UF2.TXT, USB lists, quiz), combiner
  with confidence/sources/plain-language conflicts and notes. 18 tests incl. the owner's
  two real repos.
- `crates/probe`: `UsbEnumerator` (nusb; CDC-ACM detection) and `DriveScanner`
  (INFO_UF2.TXT at FAT mount points / `/Volumes` / drive letters) + mocks.
- App: "Your keyboard" panel — summary line, per-field value + confidence + sources,
  conflicts and notes (e.g. the owner's screen-off setting), live re-check every 2.5 s
  while the panel is open (double-tap reset → controller confirmed by the bootloader),
  quiz with original SVG illustrations, Advanced evidence table + build targets,
  "Looks right" saves the profile per config folder in app data; afterwards a compact
  chip with "Change". The keyboard drawing now comes from the Rust catalog.
- Verified in the real Tauri app under Xvfb (with a simulated bootloader drive folder).

### Design decisions made (see ARCHITECTURE §5.2)
- Only hardware sources can make a field "sure"; config + answers max out at "fairly sure".
- The user's answer isn't downgraded by build.yaml disagreeing — it's explained instead.

### Manual hardware test checklist (needs the real keyboard)
1. Linux (NixOS): double-tap reset on the left half → within ~3 s the panel shows
   "Found …/NICENANO" and Controller becomes "sure · build.yaml + bootloader".
   Note the real `INFO_UF2.TXT` contents and mount path → replace the synthesised
   fixture in `crates/hardware/tests/fixtures/uf2/`.
   (NixOS: needs udisks2 automount, e.g. `services.udisks2.enable = true` + a desktop
   automounter; otherwise the drive isn't mounted and can't be seen.)
2. Same on the right half.
3. With the keyboard running normally over USB: Advanced shows a USB note
   "A keyboard running ZMK is connected: “Corne xnmrph”" (or the firmware's actual
   name — tells us which firmware is flashed!) and Live editing gets USB evidence if
   Studio is in the firmware. Record `lsusb -v` for the keyboard and the bootloader
   (VID/PID, product strings) → replace `usb/*.json` fixtures.
4. macOS and Windows: repeat 1 (drive under `/Volumes/…` / a drive letter).

### Next
1. Phase 3: toolchain manifest + downloader, west workspace, building the Corne firmware
   in the app, CI job building it on all three OSes.

## Session 2 — 2026-11-23

### Done — Phase 1 complete
- `zmk-config::kconfig`: lossless `.conf` model (active / commented-out / `is not set`
  lines, last-wins effective value, duplicate detection). Edits: set (replace value,
  uncomment an existing commented-out line, or append), comment-out. Owner's two
  `.conf` files as fixtures.
- `zmk-config::west_yml`: projects with resolved URLs + effective revision, ZMK project,
  extra modules, revision kind (commit / version / moving branch). Owner's two
  `west.yml` files as fixtures.
- `crates/git` (`mxkeys-git`): clone from any git URL. System git preferred (credential
  helpers, SSH, never prompts on a terminal; macOS git stub avoided without Command Line
  Tools); bundled libgit2 for HTTPS when git is missing. Plain-language errors, progress
  events. Local clone tests for both backends; opt-in network test
  (`cargo test -p mxkeys-git -- --ignored`) passes against `xnmrph/corne`.
- App: welcome screen clone form (address check, folder picker, progress bar,
  "Advanced" shows which git is used); repo view shows ZMK version + add-ons and the
  `.conf` settings in plain language (raw table under "Advanced"); warns when the config
  follows a moving branch.
- Parser: bare object-like macros as items (`&mt { MT_CORE };`) parsed as opaque macro
  uses; keymaps generated by `ZMK_LAYER`/included from another file get a clear
  "generated by macros, can't edit yet" message. Checked against urob/zmk-config locally
  (no license file → not vendored).
- Verified the UI in Chromium against the real Rust summary of `xnmrph/corne`.

### Findings
- Owner's `corne.conf` has `CONFIG_ZMK_DISPLAY=n` although build.yaml builds nice!view
  variants → the screen is off. Phase 2 detection must surface this ("your config turns
  the screen off").

### Next
1. Phase 2: `crates/hardware` (profile + Corne/nice!nano v2/nice!view data, needed
   targets), evidence/confidence detection from build.yaml + .conf + west.yml first.
2. `crates/probe` traits + mocks; `INFO_UF2.TXT` parser with fixtures.

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
1. ~~Check the first CI run~~ — green on all three OSes (run 5).
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
- Owner chose to **wait for the app** to add the output toggle (Phase 5 acceptance test in
  ROADMAP) — don't edit `xnmrph/corne` by hand.

### Needs manual testing (no hardware / display in the cloud environment)
- `npm run tauri dev` on NixOS (try `nix-shell` with the provided `shell.nix`, untested).
- Native folder picker on Linux/macOS/Windows.
- Later phases: everything touching USB, UF2 drives and serial.
