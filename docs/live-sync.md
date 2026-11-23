# Live editing (ZMK Studio RPC) — design

Status: **draft for review** (Phase 10, step 1). Nothing here is implemented yet; the
open decisions are listed at the end.

Goal: change keys on the connected keyboard *instantly* (no rebuild, no flashing) while
the config repo stays the single source of truth — every live change is also written
into the `.keymap` file, and nothing ever exists only on the keyboard.

All facts below were checked in ZMK `main` at 4de3133 (the revision the owner's config
builds) and in `zmk-studio-messages` (MIT). Only MIT/Apache sources were read.

---

## 1. What the keyboard offers

Studio RPC runs on the **central half** (the owner's left half, built with the
`studio-rpc-usb-uart` snippet, `CONFIG_ZMK_STUDIO=y`). Transports: USB serial
(CDC-ACM) and BLE GATT. Messages are protobuf, framed with `0xAB` (start), `0xAC`
(escape), `0xAD` (end).

| Subsystem | Requests used by mxKeys | Notes |
|---|---|---|
| core | `get_device_info` (name, serial number), `get_lock_state`, `reset_settings` | `reset_settings` needs unlock |
| behaviors | `list_all_behaviors`, `get_behavior_details(id)` | display name + parameter metadata |
| keymap | `get_keymap`, `set_layer_binding`, `check_unsaved_changes`, `save_changes`, `discard_changes`, `get_physical_layouts`, `set_layer_props` | all except reads need unlock |

Notifications: `lock_state_changed`, `unsaved_changes_status_changed`.

### Facts that shape the design

1. **Live changes are RAM-only until `save_changes`.** `save_changes` writes them to
   the settings partition (`keymap/l/<layer>/<pos>`, `keymap/l_n/<layer>`,
   `keymap/layer_order`).
2. **Saved changes override the compiled keymap — also after flashing new firmware.**
   Settings live in their own flash partition (`storage_partition @ 0xEC000`), which a
   UF2 update does not touch. A key once saved via Studio keeps the saved binding even
   if the `.keymap` later changes and new firmware is flashed. ⇒ stale overrides would
   silently hide file edits (see §5).
3. **`core.reset_settings` only clears the keymap overrides.** The only subsystem that
   registers a settings-reset hook is the keymap (`keymap_subsystem.c`); Bluetooth
   pairings are untouched. It needs the keyboard unlocked.
4. **Behavior ids are per-device.** With `ZMK_BEHAVIOR_LOCAL_ID_TYPE_SETTINGS_TABLE`
   (the default; the owner's build has it), ids are assigned on the keyboard and kept in
   its settings. They can't be predicted from the source; mxKeys maps them by **display
   name** each time it connects.
5. **Display name = `display-name`, else `label`, else the node name**
   (`DEVICE_DT_NAME`). ZMK's built-ins have display names ("Key Press", "Momentary
   Layer", "Mod-Tap" …). The owner's behaviors: `&hm` → "homerow_mods", macros with
   `label = "AE"` → "AE". Two behaviors with the same name are ambiguous → read-only.
6. **Parameters are numbers**: keys are 32-bit HID usages with implicit modifiers in the
   top byte (`&kp LS(A)` = `0x02070004`), layers are layer **ids** (the devicetree index,
   stable when layers are reordered on the device), commands are their `#define` values.
7. **Unlocking is physical**: a key bound to `&studio_unlock` (the owner has one on the
   System layer). Locked requests fail with `UNLOCK_REQUIRED`; `lock` re-locks;
   the keyboard locks itself after an idle timeout.

## 2. Architecture

```
crates/studio/                 (new)
  framing.rs     encode/decode frames (escape bytes), resync on garbage
  proto.rs       prost message structs, hand-written from zmk-studio-messages
                 (no protoc at build time; a test checks field numbers against the .proto)
  transport.rs   trait Transport { send(&[u8]); recv(timeout) }  — serial impl, mock impl
  client.rs      StudioClient: request ids, timeouts, notifications channel, typed calls
  session.rs     connect → device info, lock state, behaviors (+details), keymap, layouts
  mapping.rs     device ⇄ source: behaviors by display name, HID usages ⇄ keycode names,
                 layer ids ⇄ layer nodes/#defines, commands ⇄ #defines
  sync.rs        compare device vs files, live-edit transactions, reset-after-flash
  sim.rs         a simulated keyboard (in-memory keymap + lock + settings) for tests/CI
```

The app talks to it through a `StudioSession` trait, so a BLE transport (later) or the
third-party `zmk-studio-api` crate could slot in without touching the UI.

**Serial port**: find the ZMK device by USB VID/PID (`0x1D50:0x615E`) via the existing
`probe` crate, then open its CDC-ACM port with the `serialport` crate (without libudev).
Linux note: `/dev/ttyACM*` needs the user in `dialout` (Debian/Ubuntu/Fedora) or `uucp`
(Arch); the app explains this when opening fails with "permission denied".

## 3. Mapping device ⇄ files

### 3.1 The resolved keymap from the build

The text in `.keymap` uses `#define`s, helper macros, `LS(A)` … Resolving that the way
the compiler does is error-prone. Instead mxKeys uses the **build's own output**:
`build/<half>/zephyr/zephyr.dts` contains the keymap after the preprocessor, with every
binding as `&label <numbers>` (e.g. `&kp 0x407001e &mo 0x3`). The parser from Phase 1
reads it. Together with the behavior nodes in the same file (their display
names/labels/node names) this gives, per key, the **exact binding the firmware
contains** — the "expected device keymap".

When the current files are newer than the last build, mxKeys can still compare the keys
it can resolve itself (`&kp KEY`, `&mo LAYER`, `&lt`, `&mt`, custom hold-taps with plain
keys …) and marks the rest "can't check until the next build".

### 3.2 Behaviors

At connect: list behaviors, get details for each → `display_name → device id`. From the
resolved keymap: `&label → display name`. Join on the name. Missing or ambiguous names
make the affected keys read-only in live mode (still editable in files + rebuild).

### 3.3 Parameters

- **HID usages ⇄ key names**: `tools/gen_keycodes.py` is extended to store each key's
  numeric usage (page << 16 | id, from Zephyr's/ZMK's HID headers at the pinned
  revision). Modifier wrappers (`LS`, `RC`, …) map to the top byte.
  When writing a live change into the file, the **existing spelling is kept** if the
  value is unchanged (`LEFT_CONTROL` stays `LEFT_CONTROL`, not `LCTRL`); new values use
  the name the key picker shows.
- **Layers**: device layer id → devicetree index → the layer's `#define` if the file
  uses one (`NAV`), else the number — the same rule the key picker already uses.
- **Commands** (`&bt BT_SEL 1`, `&out OUT_TOG`): numeric values from ZMK's
  `dt-bindings/zmk/*.h`, generated alongside the keycodes.

## 4. A live edit (one transaction)

In live mode, changing a key does all of this as one step:

1. Map the new binding to `{behavior_id, param1, param2}`. Not mappable → the key
   picker says "needs rebuild" and the edit becomes a normal file edit.
2. `set_layer_binding(layer_id, position, binding)` on the keyboard.
   Fails (locked / invalid) → nothing is written to the file; the UI explains
   (e.g. "Press the unlock key — System layer, key 17", shown on the keyboard drawing).
3. Patch the `.keymap` with the existing aligned-edit code (Phase 5).
   Fails → `set_layer_binding` back to the old value (the keyboard keeps it in RAM only,
   so `discard_changes` is the fallback).
4. Undo/redo work as today and also update the keyboard.

**Saving on the keyboard** (so the change survives a restart): see decision A.

Layer renames: `set_layer_props` + the existing rename patch. Adding, removing and moving
layers, combos, macros, hold-tap timing, settings and pictures are **file edits that
need a rebuild** (they're compiled into the firmware or need new behavior nodes); the
UI keeps labelling them "takes effect after you build and install".

## 5. Keeping keyboard and files in agreement

On connect mxKeys compares the keyboard's keymap with the expected one (§3.1):

| Situation | What mxKeys shows |
|---|---|
| Same | "In sync" — live editing ready. |
| Keyboard differs on some keys (ZMK Studio used elsewhere, or saved overrides from earlier) | A per-key list: *keyboard has X, files have Y*, with **"Keep the keyboard's keys"** (write them into the files — they're already live) or **"Use the files"** (`reset_settings` + the keyboard reverts to its firmware). Choose per key or for all. |
| Files changed since the last build (git pull, text editor, offline edits) | Keys that can be sent live: **"Send to keyboard"**. Others: "build and install". |
| Behavior/layer structure differs (different firmware) | "The keyboard runs different firmware than these files — build and install first." Live editing stays off. |

**Stale overrides after flashing** (fact 2): every live edit is already in the files,
so after new firmware is installed the overrides are redundant — and harmful later.
The flash wizard therefore remembers "this keyboard has saved live changes" (per device
serial number, in app data). On the next connection after an install, mxKeys offers
**"Clear the keyboard's saved live changes"** (needs one press of the unlock key). If the
keyboard's keys match the files it's purely housekeeping; if not, §5's table applies.

The last-synced snapshot per keyboard (serial number → layer ids, behavior map, keymap
hash) lives in app data, never in the repo.

## 6. Hardware detection

Studio adds strong evidence to Phase 2's detector: device name, serial number, and the
physical layouts (key count and geometry identify the Corne 6-column layout). Shown as
"sure · connected keyboard".

## 7. Testing

- Framing, proto encoding, mapping: unit tests (including the owner's real keymap and
  a real `zephyr.dts` from a build as fixtures).
- `sim.rs`: a simulated keyboard implementing the RPC (lock, behaviors with ids from a
  settings table, keymap RAM vs saved, reset) — the sync engine is tested end to end
  against it in CI on all three OSes, including the "stale override after flash" case.
- Real hardware: checklist in PROGRESS.md (connect, unlock prompt, live key change takes
  effect without flashing, survives a restart after saving, reset clears overrides, BT
  pairings intact).

## 8. Decisions for review

**A. Saving on the keyboard.** ZMK Studio keeps changes in RAM until you press "Save".
  - *Recommended:* save automatically ~2 s after the last live edit. Then the keyboard
    always matches what you see, also after a restart. Flash wear is negligible (the
    settings storage wear-levels; one small write per burst of edits).
  - Alternative: a "Save on keyboard" button like ZMK Studio (unsaved changes are lost
    when the keyboard restarts — they're still in the files and in the next build).

**B. Client library.**
  - *Recommended:* our own small client (`crates/studio`: framing + prost structs +
    `serialport`). The protocol is ~5 small `.proto` files; we need tight control for
    transactions, a simulator for CI, and no `protoc` at build time.
  - Alternative: `zmk-studio-api` (Apache-2.0, single maintainer, young) behind the
    `StudioSession` trait — less code now, less control.

**C. Clearing saved overrides after installing new firmware** (one unlock-key press,
  only when the keyboard has saved live changes). *Recommended: yes* — otherwise an
  older live change can silently win over a newer file edit.

**D. Layer add/remove/move live.** *Recommended: not in v1* — files + rebuild as today.
  Studio's layer ids and reordering add a lot of mapping risk for little gain.

**E. Bluetooth transport.** *Recommended: later* (USB first; BLE needs a native BLE
  stack per OS). With the software-KVM setup the left half is usually on USB anyway when
  editing.
