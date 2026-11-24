# mxKeys — Architecture

> Status: living document. Written in session 1 (2026-11-22) after researching ZMK `main`
> (commit `4de3133`, Zephyr 4.1), the ZMK Studio RPC protocol, `zmk-studio-api` 0.6.1,
> Nick Coutsos' keymap-editor and the nice!view shield sources. Decisions that are still
> open are marked **OPEN** with options and a recommendation.

## 1. Goal and scope

A cross-platform desktop app that lets a non-expert own a ZMK keyboard end to end:
edit the keymap visually, change common settings, build firmware **inside the app**,
flash it with a guided UF2 flow, manage the config repo with git, and (later) edit live
over ZMK Studio RPC — with the user's zmk-config git repo always the source of truth.

v1 hardware scope is exactly one profile:

| Part | Value |
|---|---|
| Keyboard | Corne, 3x6 + 3 thumbs (42 keys), split |
| Controller | nice!nano v2 — ZMK board id `nice_nano@2.0.0` (HWMv2 naming) |
| Display | nice!view — shields `nice_view_adapter nice_view` |
| Central half | left (`corne_left`), Studio enabled via snippet `studio-rpc-usb-uart` |

Everything is designed around a **hardware profile** (section 5) so other boards can be
added later, but only this one is implemented and tested.

## 2. Stack

- **Tauri v2**: Rust backend, TypeScript + React (Vite) frontend.
- Rust workspace:

```
Cargo.toml                 workspace
crates/
  zmk-config/              pure parsing/serialising of the config repo (no I/O beyond fs reads)
    dts/                   lossless devicetree (.keymap/.dtsi/.overlay) lexer + CST
    keymap/                typed keymap model on top of the CST (layers, bindings, behaviors…)
    build_yaml/            build.yaml parser + target model
    (later) west_yml, kconfig (.conf)
  hardware/      (ph. 2)   hardware profiles (Corne/nice!nano/nice!view) + pure detection logic
  probe/         (ph. 2)   OS-specific probing behind traits: USB enumeration, UF2 drives, serial
  toolchain/     (ph. 3)   managed toolchain: manifest, downloads, checksums, west/cmake runner
  flash/         (ph. 4)   UF2 flashing state machine (uses probe traits)
  studio/        (ph. 10)  Studio RPC client wrapper + keymap sync engine
src-tauri/                 Tauri shell: thin `#[tauri::command]` wrappers, app state, events
src/                       React UI (plain-language first; "Advanced" disclosure for ids)
toolchain/manifest.toml    (ph. 3) every pinned version + sha256 in one file
docs/                      ARCHITECTURE, ROADMAP, PROGRESS, build-toolchain, live-sync…
```

Rule: **all logic that can be pure is pure** and lives in `crates/*` with fixture tests.
`src-tauri` only wires commands, state and events. OS access (serial ports, USB, mounted
drives, process spawning, network) is behind traits so it can be mocked in CI (the cloud
dev environment has no keyboard attached).

## 3. Source-of-truth model

The user's **zmk-config git repository** is the only source of truth:

```
zmk-config/
  build.yaml            which firmware variants to build
  config/west.yml       ZMK revision + extra modules
  config/corne.keymap   keymap (devicetree + C preprocessor)
  config/corne.conf     Kconfig settings
  (later) boards/shields/…, config/*.c for custom nice!view art
```

Data flow:

```
          ┌──────────── git (status/diff/commit/push, any remote) ─────────────┐
          │                                                                    │
   repo files ──parse──▶ CST (lossless) ──▶ typed model ──▶ UI (React)         │
       ▲                    │  ▲                                │              │
       │                    │  └──── edit ops (typed) ◀─────────┘              │
       └──── span patches ◀─┘                                                  │
       │                                                                       │
       ├──▶ toolchain: west workspace + west build ──▶ .uf2 ──▶ guided flash   │
       └──▶ (ph. 10) live: Studio RPC ◀──▶ device; changes written back as patches
```

Key properties:

1. **Lossless editing.** Files are never regenerated from the model. The parser keeps a
   lossless token stream (every byte, including whitespace, comments and preprocessor
   lines). An edit is a minimal *span patch* (replace bytes `a..b` with new text) followed
   by a re-parse. Untouched bytes stay byte-identical, so `git diff` shows exactly what the
   user changed. This is what makes the repo a trustworthy source of truth.
2. **Write-through.** Every UI edit is written to disk immediately (or on explicit save —
   **OPEN**, recommendation: immediate write + undo stack, with git as the safety net).
3. **External edits win.** A file watcher re-parses on change; if the user edits the file
   in a text editor while the app has unsaved state, the app reloads and re-applies its
   pending ops only if they still apply cleanly, otherwise it asks.
4. **Unsupported constructs are preserved, not rejected.** Anything the typed model does
   not understand (macro-generated behaviors, `#ifdef` blocks, helper macros like
   `ZMK_HOLD_TAP(...)`) is kept as opaque CST and shown read-only ("edited by hand — open
   in text editor") instead of being lost.

## 4. The keymap parser (Phase 1 foundation)

`.keymap` files are devicetree source run through the C preprocessor. We do **not** run
the preprocessor: we parse the *unexpanded* text so we can write it back faithfully.

Layers:

1. **Lexer** — produces tokens that tile the input exactly (concatenating token texts ==
   input). Token kinds: whitespace, newline, line comment, block comment, preprocessor
   directive (whole logical line, including `\` continuations), identifier (DT names allow
   `,._+-` and `@`), number, string, char literal, punctuation (`{ } < > ( ) [ ] ; = , & /`
   …), and `/delete-node/`-style keywords.
2. **CST** — a tree of items with byte spans:
   - `Node` (labels, name, unit address, children), `RefNode` (`&label { … }`),
   - `Property` (name, values: cell arrays `<…>`, strings, byte strings `[…]`, `&ref`),
   - `Directive` (`#include`, `#define`, `#if…`), `MacroCall` (top-level/helper macro
     invocations such as `ZMK_COMBO(...)` kept opaque), `DeleteNode`, `Unknown` (error
     recovery: an unparseable region becomes opaque instead of failing the whole file).
   - Cell arrays keep cells as `Ref(&kp)`, `Number`, `Ident` (`LCTRL`, `N1`),
     `Call` (`LS(A)`, `RC(0,1)`), `Paren` (expressions) — each with its span.
3. **Keymap model** — interprets the CST: finds the `compatible = "zmk,keymap"` node,
   layers (node name, label, `display-name`, `bindings`, `sensor-bindings`), and groups
   binding cells into `Binding { behavior: "&kp", params: [...], span }`. Each `&ref`
   starts a new binding. Object-like `#define`s from the same file whose body starts with
   `&` (e.g. `#define XXX &none`) count as a binding. Bindings we cannot group reliably
   mark the layer as *partially opaque* (read-only in the UI).
4. **Edit ops** — `set_binding(layer, pos, text)`, `set_layer_display_name`, later
   add/remove/move layer, combos, macros… Each produces span patches. Re-alignment of the
   binding grid (keeping columns tidy) is a separate, optional formatting op applied only
   to the bindings block being edited.

Why hand-written and not tree-sitter? keymap-editor (MIT, © Nick Coutsos) uses
`tree-sitter-devicetree` via WASM on its `devicetree-parser` branch. A tree-sitter grammar
is a good option, but it adds a C build step on every platform and its grammar treats
preprocessor content loosely. A small hand-written lexer/parser gives exact control over
lossless spans and error recovery, and is easy to test. We reimplemented from the
devicetree spec + ZMK docs; **no keymap-editor code was copied**. (Its MIT license would
allow reuse with attribution; we chose not to need it.)

Fixtures: ZMK's stock `corne.keymap` (MIT, attributed in the fixture header), plus
original fixtures covering combos, macros, hold-taps, mod-morphs, conditional layers,
`#define`/`#ifdef`, helper macro invocations, CRLF line endings, unicode in comments,
`/delete-node/`, `&label { … }` overrides and string escapes.

## 5. Hardware profile and detection

### 5.1 Profile model (crate `hardware`)

```rust
struct HardwareProfile {
    keyboard:   KeyboardDef,     // id "corne", name, physical layout (42 keys, x/y/w/h/rot in 1/100 key units),
                                 // halves: [{ side: Left, shield: "corne_left", role: Central },
                                 //          { side: Right, shield: "corne_right", role: Peripheral }]
    controller: ControllerDef,   // board "nice_nano@2.0.0", display name "nice!nano v2",
                                 // uf2: { board_id: "nRF52840-nicenano", family_id: 0xADA52840 },
                                 // usb: bootloader VID:PID 239A:00B3 (Adafruit VID); see 5.3
    display:    DisplayDef,      // None | NiceView { shields: ["nice_view_adapter","nice_view"], 160x68 1-bit }
                                 //      | Oled128x32 { shield: none needed on corne (built in) }
    studio:     StudioDef,       // central half + snippet "studio-rpc-usb-uart"
}
```

The profile drives everything else: which build targets exist, which UF2 goes to which
half, what the keymap grid looks like, which image area the nice!view editor offers.

**Build targets derived from the profile** (only what's needed — the setup script's
"with and without display" matrix is reduced):

| Target | board | shield | snippet |
|---|---|---|---|
| left (central, Studio) | `nice_nano@2.0.0` | `corne_left nice_view_adapter nice_view` | `studio-rpc-usb-uart` |
| right | `nice_nano@2.0.0` | `corne_right nice_view_adapter nice_view` | — |
| settings reset | `nice_nano@2.0.0` | `settings_reset` | — |

The app builds these targets itself from the profile; it does **not** rewrite the user's
`build.yaml` unless asked (the GitHub Actions workflow in their repo may still use it).
A "tidy build.yaml" assistant can offer to remove unused variants.

### 5.2 Detection (pure functions, fixture-tested)

Each source produces `Evidence { source, claim, confidence }` where `claim` is one of
`Keyboard(id)`, `Controller(board)`, `Display(kind)`, `CentralHalf(side)`, `StudioEnabled`,
and `confidence ∈ {Weak, Medium, Strong}`. A combiner folds evidence into a
`DetectionResult` with, per field: chosen value, combined confidence, the sources that
support it, and any **conflicts** with a plain-language explanation.

| Source | What it tells us | Confidence |
|---|---|---|
| `build.yaml` | board ids, keyboard shields (`corne_left/right`), display shields (`nice_view`, OLED implied by corne), snippets (`studio-rpc-usb-uart` ⇒ that half is central/Studio) | Medium (it's what the user *builds*, not what they *have*) |
| `west.yml`, `config/*` | extra modules (e.g. custom shields), `.conf` hints (`CONFIG_ZMK_DISPLAY=y`) | Weak/Medium |
| UF2 drive `INFO_UF2.TXT` | `Model:` and `Board-ID:` of the bootloader ⇒ controller | Strong (for controller) |
| USB enumeration | bootloader VID:PID / product strings, ZMK firmware VID:PID `1D50:615E` (product = keyboard name, e.g. "Corne") | Medium/Strong |
| Studio RPC (ph. 10) | device name, physical layouts (key count + geometry ⇒ keyboard) | Strong |
| Quiz fallback | user answers ("does your screen look like A or B?") | Medium (user may be unsure) |

Combining rules (implemented in `crates/hardware/src/detect/combine.rs`):

- Per field, evidence is grouped by value; the winning value has the highest
  confidence, then the most trustworthy source (live connection > bootloader > USB >
  user's answers > build.yaml > settings), then the most distinct sources.
- Independent sources agreeing raise confidence one level, but **only a hardware source
  (bootloader, USB, live connection) can make it "sure"** — config files describe what is
  *built*, not what is on the desk (and the user may not know what's flashed).
- A disagreement lowers confidence only if it is as confident *and* from an equally or
  more trustworthy source (two halves' bootloaders disagreeing, say). build.yaml
  disagreeing with the user's answer does not downgrade the answer; it's explained.
- `CONFIG_ZMK_DISPLAY` is treated as "screen software on/off", not as hardware; if the
  chosen screen is real but the software is off, a warning says the screen will stay blank.

Conflict example: build.yaml has only `corne_left` (no display shield) but the user
answers "my screen looks like the nice!view picture" ⇒ *"Your config doesn't turn on the
screen yet. We'll add the nice!view to your build so the screen works."*

Summary line shown to the user: **"Corne · nice!nano v2 · nice!view — detected from
build.yaml + bootloader"**, with Confirm / Change, and an "Advanced" disclosure showing
board ids, shields and snippets.

### 5.3 Identifiers (researched; verify on hardware — see PROGRESS.md)

- UF2 bootloader (Adafruit nRF52 bootloader, used by nice!nano) mass-storage drive
  label is typically `NICENANO`; it contains `INFO_UF2.TXT`, `CURRENT.UF2`, `INDEX.HTM`.
  `INFO_UF2.TXT` is `Key: value` lines after a first free-form line, e.g.

  ```
  UF2 Bootloader 0.6.0 lib/nrfx (v2.0.0) lib/tinyusb (0.10.1-…) lib/uf2 (…)
  Model: nice!nano
  Board-ID: nRF52840-nicenano
  SoftDevice: S140 version 6.1.1
  Date: Jun 19 2021
  ```

  Parsing is tolerant: keys are case-insensitive, unknown keys kept, first line parsed for
  the bootloader version. The UF2 file format itself carries a **family id**
  (`0xADA52840` for nRF52840) in each 512-byte block; before flashing we check both the
  drive's `Board-ID` against the profile and the firmware's family id against the
  controller.
- USB IDs: nice!nano bootloader `239A:00B3` (Adafruit VID); ZMK firmware default
  `1D50:615E`, manufacturer "ZMK Project", product = `CONFIG_ZMK_KEYBOARD_NAME` (from ZMK
  `app/Kconfig`). With the Studio snippet, the firmware also exposes a CDC-ACM serial port.
  These go in a data table (not code) so they're easy to correct after hardware testing.

### 5.4 OS probing (crate `probe`, behind traits)

```rust
trait UsbEnumerator { fn devices(&self) -> Result<Vec<UsbDeviceInfo>>; }       // nusb / serialport
trait DriveWatcher  { fn uf2_drives(&self) -> Result<Vec<MountedDrive>>; }     // per-OS impl
trait SerialPorts   { fn ports(&self) -> Result<Vec<SerialPortInfo>>; }
```

Per OS: Linux — `/proc/self/mounts` + `/media/$USER`, `/run/media/$USER` (udisks
automount; NixOS users may need udisks2 enabled — document); macOS — `/Volumes/*`;
Windows — `GetLogicalDrives` + volume label. Mock implementations feed fixtures in tests.

## 6. Toolchain design (Phase 3)

Full write-up with numbers and trade-offs goes in `docs/build-toolchain.md` (Phase 3).
Summary of the plan:

- One pinned manifest `toolchain/manifest.toml`: Zephyr SDK version + per-host URLs +
  sha256 for **only** `toolchain_<host>_arm-zephyr-eabi` (+ hosttools where needed),
  python-build-standalone release + sha256, uv version, cmake, ninja, dtc, and the
  Python requirement lock (west + Zephyr's `requirements-base.txt` subset).
- Layout under the app-data dir:

  ```
  <appdata>/mxkeys/
    toolchain/<manifest-hash>/{zephyr-sdk, python, cmake, ninja, dtc, venv}
    workspaces/<zmk-revision-hash>/   west workspace (zephyr, modules, zmk), shared across projects
    builds/<project-id>/<target>/     build dirs (incremental)
    downloads/                        verified archives cache
  ```
- Steps: download → verify sha256 → extract → create venv (pip) → `west init -l` on a
  generated workspace manifest that imports the user's `config/west.yml` → `west update
  --narrow -o=--depth=1` → `west zephyr-export` is not needed (we pass
  `ZEPHYR_SDK_INSTALL_DIR` / `-DZephyr_DIR` explicitly) → `west build -s zmk/app -b
  <board> -- -DSHIELD="…" -DZMK_CONFIG=<repo>/config -S <snippet>` per target.
- Logs stream line-by-line as Tauri events; a log classifier turns common failures into
  plain language ("Unknown keycode `LCTL` on layer *Lower*, key 13 — did you mean
  `LCTRL`?"), mapping devicetree errors back to keymap positions via our CST spans.
- ZMK `main` pins Zephyr `v4.1.0+zmk-fixes` and already blocks many unneeded HALs in
  `app/west.yml`; ZMK CI uses the `zmk-build-arm:4.1` container, built with Zephyr SDK
  **0.16.9** (verified in `zmkfirmware/zmk-docker`).

**Size (measured in Phase 3, Linux x86-64 — details in `docs/build-toolchain.md`):**
≈ 500 MB download; ≈ 1.85 GB on disk with mxKeys' trimming (toolchain 256 MB after
multilib trim, Python 105 MB, venv 60 MB, CMake ~110 MB, workspace 1.2 GB with the
module allow-list, builds ~125 MB) vs ≈ 4.9 GB naive. Both trims were verified to
produce byte-identical firmware. The SDK must be **0.16.9** (what ZMK's build image
uses); 0.17.x does not compile Zephyr 4.1. Board ids need the `//zmk` variant.

**OPEN — toolchain size vs completeness.** Options:
(a) honour the user's `west.yml` as-is (most compatible, biggest);
(b) generated workspace manifest that imports the user's west.yml with a
    `name-allowlist` tuned for nRF52840 + nice!view (small, but a user module that needs
    another HAL breaks);
(c) (b) by default with automatic fallback to (a) if CMake reports a missing module.
**Recommendation: (c).** — *Implemented in Phase 3.*

## 7. ZMK version upgrades

- Each project's ZMK revision is pinned by its `config/west.yml` (`revision:` of the
  `zmk` project). The app reads it; if it is a branch (`main`), the app resolves and
  records the commit in an app-side lockfile (`<appdata>/projects/<id>/lock.json`) so
  rebuilds are reproducible, and suggests pinning to a commit/tag in the repo.
- West workspaces are keyed by resolved ZMK revision and shared between projects.
- The toolchain manifest records which Zephyr SDK goes with which Zephyr major (ZMK 0.3 →
  Zephyr 3.5 / SDK 0.16; ZMK main → Zephyr 4.1 / SDK 0.17).
- Later: an **upgrade assistant** builds the new revision in a scratch build dir, reports
  keymap/Kconfig breakages (renamed Kconfig, board-id changes like `nice_nano_v2` →
  `nice_nano@2.0.0`), proposes patches, and commits on success.

## 8. Flashing (Phase 4)

Implemented in `crates/flash` (pure logic) and `src-tauri/src/flashing.rs` (background
loop):

- `session::plan`: optional settings reset (left, right — ZMK's documented procedure),
  then left firmware, then right firmware.
- `Session` state machine: `WaitingForDrive → (Refused | TooManyDrives) → Copying →
  Restarting → next step … → Finished`, plus `Failed` with retry and a per-step skip.
  The app polls the drive scanner every 0.5 s and feeds it the drive list.
- Two checks before any byte is written: the drive's `INFO_UF2.TXT` must identify the
  profile's controller (Board-ID, falling back to Model because our Board-ID table is
  unverified), and each `.uf2` must carry the controller's UF2 family id
  (0xADA52840). Firmware files are checked when the session is created.
- Copy writes `mxkeys.uf2` in block-aligned chunks; an I/O error *after* all bytes were
  handed to the OS counts as success (the bootloader resets as soon as the last block
  lands).
- Both halves present identical drives, so the flow relies on the user plugging in the
  half it asks for and on the drive disappearing between steps.

### 8.1 Settings page (Phase 6)

- `zmk-config::kconfig` is the lossless `.conf` model (Phase 1); `zmk-config::settings`
  adds a **catalog**: id, group, plain label/help, kind (toggle, number with unit and
  range, text with a byte limit, Kconfig `choice`, int with named values), default, and
  an optional "only matters when X is on" dependency. Defaults and ranges were checked
  against ZMK `main` 4de3133 / Zephyr 4.1 and a real build's `.config`.
- Hardware-dependent defaults (the screen is on by default *because* the nice!view
  shield turns it on) are the ones for the v1 profile. When more profiles exist the
  catalog takes the profile as input.
- Write rules: set = replace the effective line, else uncomment the user's old line,
  else append. **Use default = comment out** every line setting the option (the old
  value stays visible). Choices write the chosen option and comment out the others.
  Some toggles switch companions on with them (RGB underglow → `WS2812_STRIP`;
  peripheral battery fetching → proxy); switching off leaves companions alone.
- Advanced: raw option names/line numbers per setting, the file's other options, and a
  free-form "set any option" (unvalidated — typos show up at build time).
- Undo/redo shares the keymap editor's per-file text history (`Editor::record` /
  `step_text`); the same 2 s content-hash poll reloads after outside edits.
- `firmware_state` compares the built `.uf2` times with `build.yaml` and `config/*`, so
  the Firmware panel says when a rebuild is needed after any edit.
- Settings are written to the `.conf` the repo already has (`config/corne.conf` applies
  to both halves). Per-half files (`corne_left.conf`) are shown as separate pages.

### 8.2 Behaviors, macros, combos (Phase 7)

- `zmk-config::behaviors` describes each definition type with a schema (property name,
  plain label/help, type: time, flag, choice, bindings (bare or with parameters, fixed
  or variable count), modifiers, key positions, layers, one layer, keycode list),
  taken from ZMK's devicetree bindings. The UI renders forms from it.
- Definitions are found by walking the CST: nodes with a `zmk,behavior-*` compatible,
  children of `zmk,combos` / `zmk,conditional-layers`, `&mt { … }` overrides of
  built-ins, and top-level zmk-helpers macro calls (parsed by turning the arguments back
  into a node body; read-only).
- Ids are stable across edits: `container/node-name`, `&label`, `helper:<n>`.
- Writes keep diffs minimal: one property's value span is replaced; a binding list with
  the same number of entries patches only changed entries (so `<&kp A &kp B>` groups
  survive); new properties go before the closing brace with the body's indentation;
  removed properties take their line (and a trailing comment) with them. New nodes use
  the indentation unit the file already uses (spaces or tabs).
- Layers are written with the file's `#define` names when they exist (as the key picker
  does); moving/removing layers already renumbers combo/conditional-layer references.

## 9. Live editing and sync-back (Phase 10)

### 9.1 Protocol (from ZMK docs, read respecting the clean-room policy)

ZMK's clean-room policy says contributors **must not reuse code from non-MIT projects or
study GPL code** when contributing to ZMK. mxKeys is not ZMK, but we follow the same
spirit: we only read ZMK's MIT/Apache docs and protobuf definitions
(`zmk-studio-messages`, MIT), never QMK/GPL sources, and keep mxKeys under a permissive
license (**MIT**, recommended) so code could flow back to ZMK if ever wanted.

- Framing: `SoF 0xAB`, `Esc 0xAC`, `EoF 0xAD`; special bytes in the payload are prefixed
  with `Esc`. Transports: USB CDC-ACM serial; BLE GATT service
  `00000000-0196-6107-c967-c5cfb1c2482a`, single characteristic
  `00000001-0196-6107-c967-c5cfb1c2482a` (write + indications).
- Messages (proto3): `zmk.studio.Request { request_id, oneof core|behaviors|keymap }`,
  `Response { oneof request_response | notification }`. Subsystems:
  - core: `get_device_info` (name, serial), `get_lock_state`, `lock`, `reset_settings`;
    notification `lock_state_changed`. Unlocking is physical (the `&studio_unlock`
    behavior); locked requests fail with `meta.UNLOCK_REQUIRED`.
  - behaviors: `list_all_behaviors` → ids; `get_behavior_details(id)` → display name +
    parameter metadata (nil / constant / range / HID usage / layer id).
  - keymap: `get_keymap` (layers with stable `id`, `name`, bindings of
    `{behavior_id, param1, param2}`), `set_layer_binding`, `check_unsaved_changes`,
    `save_changes`, `discard_changes`, `get_physical_layouts`,
    `set_active_physical_layout`, `move/add/remove/restore_layer`, `set_layer_props`;
    notification `unsaved_changes_status_changed`.

### 9.2 Client crate (built: own client)

Decision B: our own small client in `crates/studio` instead of `zmk-studio-api`.

```
framing.rs   SOF/ESC/EOF encode + streaming decoder
proto.rs     hand-written prost structs (field numbers tested against proto/*.proto,
             vendored from zmk-studio-messages, MIT) — no protoc at build time
client.rs    Transport trait (send, recv with timeout) → Client: requests by id,
             notifications queued, Locked / NotSupported / Refused errors
serial.rs    USB CDC-ACM via serialport (VID/PID 0x1D50:0x615E)
ble.rs       btleplug on a tokio thread; 20-byte writes, indications
sim.rs       simulated keyboard with ZMK's rules (CI + MXKEYS_SIM_KEYBOARD=1)
mapping.rs   source text ⇄ {behavior_id, param1, param2}
sync.rs      expected keymap from the files, comparison, decoding device bindings
live.rs      Live<T>: connect, poll lock state, set/restore key, rename layer,
             save, discard, clear saved
```

The app side (`src-tauri/src/live.rs`) holds one connection, runs each live edit as
“keyboard first, then the file through the normal editor (undo history); roll the
keyboard back if the file edit fails”, and remembers per keyboard serial when changes
were last saved and per repo when firmware was last installed (app data `live.json`)
to decide whether to offer clearing saved changes.

### 9.3 Sync-back design (as implemented; details in `docs/live-sync.md`)

The hard part: the device speaks *runtime* ids (`behavior_id`, HID usage numbers, layer
ids), the repo speaks *source* (`&kp LCTRL`, `&mo 1`, `#define`s).

1. **Behavior mapping.** At unlock, list behaviors and their display names. A behavior's
   name on the keyboard is `display-name` ?? `label` ?? node name, read from ZMK's
   `app/dts/behaviors/*.dtsi` in the build workspace and from the user's keymap; ids are
   per keyboard, so they are always looked up by name. Names reported twice ⇒ keys using
   them are read-only live.
2. **Parameter mapping.** No generated tables: a small C preprocessor
   (`zmk_config::cpp`) evaluates the binding's cells with ZMK's headers and the user's
   own includes from the workspace — exactly what the build does (checked against
   `zephyr.dts`). Decoding back (for “Keep keyboard's”) prefers the spelling already in
   the file, then the catalog's keycode names with modifier wrappers like `LS(…)`.
3. **Layer identity.** Studio layer ids are the devicetree layer indices; layers are
   matched by index, and a structure difference (layer count, keys per layer) turns
   live editing off until the firmware is rebuilt.
4. **Transaction per edit.** Live edit = `set_layer_binding` on the device, then the
   span patch on the `.keymap` through the editor; if the file edit fails, the old
   binding is put back on the keyboard. "Save on keyboard" = `save_changes`; the files
   are already written. Discard = `discard_changes` + the file as it was at the last
   save (an undo step). Nothing is ever only on the device.
5. **Conflict avoidance.** On connect, the app computes `normalize(device keymap)` and
   `normalize(compile(repo keymap))` and compares:
   - equal → in sync;
   - device differs (user used ZMK Studio elsewhere, or Studio-saved overrides survived a
     reflash — ZMK keeps Studio changes in settings storage, which **overrides** the
     keymap compiled into new firmware) → show a per-key diff with *"Keep keyboard's
     version (write into files)"* or *"Use files (reset keyboard's changes)"*;
   - repo changed since last sync (git pull, text editor) → offer "send to keyboard"
     (live, if only bindings changed) or "rebuild & flash" (structure changed).
   The last-synced snapshot per device serial number lives in app data, not in the repo.
6. **Things Studio cannot express** (new combos, macros, hold-tap tuning, Kconfig) are
   file-only edits that require rebuild + flash; the UI labels them "needs rebuild".

## 10. nice!view images (Phase 9)

From ZMK's `nice_view` shield: 160×68 Sharp memory display, LVGL with 1-bit colour.
On the Corne it is mounted turned: what LVGL draws at (x, y) in its 160×68 landscape
space is seen as a 68×160 portrait screen. ZMK's stock art (`balloon`, `mountain`,
140×68 `LV_COLOR_FORMAT_I1`, 8-byte palette + 18-byte rows) turned 90°
counter-clockwise is the upright picture — so mxKeys works upright and stores frames
turned 90° clockwise (`crates/niceview/src/lvgl.rs`, tested byte-for-byte against
`balloon`). On the right (peripheral) half, ZMK shows a 20-pixel status strip at the
top (battery, connection) and the art below; the left (central) half shows layer,
battery, output and WPM.

Pipeline (`crates/niceview`): decode → fit (fill/fit/stretch, transparent and bars
filled with the background) → brightness/contrast → dither (Atkinson,
Floyd–Steinberg, Bayer 4/8, threshold) → invert → frame limits (minimum frame time,
merge identical neighbours, sample evenly to the flash budget keeping the total
time) → `I1` encoding.

Generated module (config repo, see `module.rs`): `nice_view_art/` with
`CMakeLists.txt` (`target_sources(app …)` when `CONFIG_MXKEYS_NICE_VIEW_ART`), `Kconfig`
(default y with `ZMK_DISPLAY`), `src/art.c` (fixed code), `src/frames.c` (generated
frames for both halves, chosen by `ZMK_SPLIT_ROLE_CENTRAL`), the original pictures in
`source/` and settings in `mxkeys.json`. `zephyr/module.yml` gets `cmake:`/`kconfig:`
under `build:` (lossless edit; refused with instructions if the repo's module already
builds its own code). The app's builds — like ZMK's GitHub workflow — pass the repo
as an extra module when it has `zephyr/module.yml`.

Runtime: rather than forking ZMK's widget code (which tracks LVGL/ZMK changes), the
module waits until `zmk_display_is_initialized()` and then queues its work on
`zmk_display_work_q()` — behind ZMK's own `initialize_display` — to add an
`lv_image` on top of the active screen (top-left; 140×68 over the stock art or 160×68
over everything). Animations: an `lv_timer` steps frames with their delays and is
paused/resumed from `zmk_activity_state_changed` (via the same work queue). Palette:
a set bit is a light pixel, which on the nice!view is LVGL *black* (found on real
hardware: the first version used ZMK's art palette and every picture came out
inverted). Pictures ignore `CONFIG_NICE_VIEW_WIDGET_INVERTED` so they always match
the preview; each picture has its own Invert option.

Budget: program area 0xC6000 (792 KiB); measured Corne halves with screen ≈430 KiB /
≈315 KiB. Default 64 KiB per half (frame 1,368 B full / 1,232 B art + ~32 B overhead).

## 11. Git

Implemented in `crates/git/src/repo.rs` (Phase 8):

- **Reading** — status (branch, upstream, ahead/behind, changed files, identity, last
  commit), diff (HEAD → working tree incl. new files, renames), file-at-HEAD, log —
  always via libgit2: structured, no output parsing, works without git installed.
- **Writing** — commit, fetch, pull, push via **system git** when installed, so commit
  hooks/signing, credential helpers and SSH agents behave as in a terminal
  (`GIT_TERMINAL_PROMPT=0`: never hangs on a prompt). libgit2 fallback for HTTPS remotes
  (credential-helper callback); SSH remotes need system git.
- **Pull** = fetch, then fast-forward; if local commits exist, rebase on top with system
  git and `rebase --abort` on conflict (the folder is never left mid-rebase). libgit2:
  fast-forward only. Uncommitted edits to tracked files block pulling (plain message).
- **Commit a subset**: `git add -A -- paths` + `git commit -- paths` (renames include the
  old path); libgit2 updates only those index entries.
- **Identity**: if `user.name`/`user.email` are missing, the UI asks once and stores
  them in the repository's own `.git/config` (never the global config, never a
  committed file).
- Remote URLs are shown with embedded credentials removed (`redact_url`). The app
  never asks for or stores tokens.
- Errors are classified into plain-language messages (rejected push → “get latest
  first”, diverged, conflict, no permission (403), nothing to commit, no identity …).
- `zmk-config::changes` turns the difference between HEAD and the working copy of a
  keymap/`.conf` into plain sentences (“System: key 17 &trans → &out OUT_TOG”,
  “Screen: off → on”), shown per file and used for the suggested commit message.
- UI: “Changes and upload” panel (save version = commit, upload = push, get latest =
  pull, undo changes = discard with a native confirmation). Background fetch on open,
  every 3 minutes and on window focus. Recently opened folders are kept in app data.

## 12. Open questions (answered/recommended)

### App name — 3 options
1. **mxKeys** — matches the repo, short, neutral, no ZMK trademark use. *(recommended)*
2. **Keywright** — "a craftsperson of keys"; friendly, descriptive.
3. **Cornerstone** — nods to the Corne v1 target; may be too Corne-specific later.
(Check name collisions on crates.io/npm/app stores before 1.0. Avoid "ZMK" in the name
to not imply official status.)

### How big is the managed toolchain, and how to keep it small?
See section 6: ≈1–1.5 GB optimised vs 1.5–3 GB naive. Levers: arm-only SDK, shallow
`--narrow` west clones, allow-listed modules, shared workspace across projects, cached
downloads, and deleting the downloaded archives after extraction. Full write-up in
`docs/build-toolchain.md` (Phase 3).

### ZMK version upgrades
Section 7: pin per project via `west.yml` + app lockfile; upgrade assistant later.

### Live Studio edits vs file edits
Section 9.3: every live edit is simultaneously a file patch (transaction); on connect
the device is diffed against the repo and the user picks a side per difference; the repo
is never silently overwritten and the device is never left with changes the repo
doesn't have.
