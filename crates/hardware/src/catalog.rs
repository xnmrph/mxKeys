//! What mxKeys knows about hardware. v1 *supports* exactly one combination (Corne +
//! nice!nano v2 + nice!view); other entries exist only so detection and the quiz can
//! recognise them and say "not supported yet" instead of guessing.
//!
//! Identifiers marked `verified: false` come from memory/common knowledge, not from a
//! device on the desk — confirm them on real hardware (see docs/PROGRESS.md).

use serde::{Deserialize, Serialize};

/// A keyboard, named by its ZMK shield without the `_left`/`_right` suffix (`corne`,
/// `lily58`, `sofle`, `planck`). Any ZMK shield works; a few have nicer names below.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct KeyboardId(pub String);

impl KeyboardId {
    pub fn new(base: &str) -> Self {
        KeyboardId(base.to_string())
    }
    pub fn corne() -> Self {
        KeyboardId::new("corne")
    }
    pub fn as_str(&self) -> &str {
        &self.0
    }
    /// The shields to build for each half (or the one shield of a one-piece board).
    pub fn halves(&self, split: bool) -> Vec<HalfShield> {
        if split {
            vec![
                HalfShield {
                    side: Some(Side::Left),
                    shield: format!("{}_left", self.0),
                },
                HalfShield {
                    side: Some(Side::Right),
                    shield: format!("{}_right", self.0),
                },
            ]
        } else {
            vec![HalfShield {
                side: None,
                shield: self.0.clone(),
            }]
        }
    }
}

/// One firmware's keyboard shield: a half of a split keyboard, or the whole keyboard.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct HalfShield {
    pub side: Option<Side>,
    pub shield: String,
}

/// Plain name and facts for a keyboard id.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct KeyboardInfo {
    pub name: String,
    /// In the list below (nice name, layout known in advance).
    pub known: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ControllerId {
    NiceNanoV2,
    NiceNanoV1,
    XiaoBle,
    NrfMicro,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DisplayKind {
    None,
    NiceView,
    Oled128x32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Side {
    Left,
    Right,
}

/// One key of a physical layout, ZMK `zmk,physical-layout` units: position/size in 1/100
/// key units, rotation in 1/100 degrees around (rx, ry).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct KeyGeom {
    pub w: i32,
    pub h: i32,
    pub x: i32,
    pub y: i32,
    pub r: i32,
    pub rx: i32,
    pub ry: i32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct KeyboardDef {
    /// Shield name without `_left`/`_right` (the [`KeyboardId`]).
    pub base: &'static str,
    pub name: &'static str,
    /// Shield names that identify this keyboard in build.yaml.
    pub shields: &'static [&'static str],
    pub key_count: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ControllerDef {
    pub id: ControllerId,
    pub name: &'static str,
    /// Plain-language description for the "which controller do I have?" quiz.
    pub looks_like: &'static str,
    /// ZMK board id to build with: current HWMv2 naming *with the `zmk` variant*, which
    /// ZMK `main` (Zephyr 4.1) requires — the plain board is Zephyr's stock definition
    /// without ZMK's additions (e.g. the nice!view SPI bus).
    pub board: &'static str,
    /// Board names that mean this controller in build.yaml (`name`, `name@rev`, legacy).
    pub board_aliases: &'static [&'static str],
    /// Substrings of `Model:` in INFO_UF2.TXT (case-insensitive).
    pub uf2_models: &'static [&'static str],
    /// `Board-ID:` values in INFO_UF2.TXT.
    pub uf2_board_ids: &'static [&'static str],
    /// UF2 family id the firmware must carry (nRF52840 = 0xADA52840).
    pub uf2_family: u32,
    /// USB VID:PID of the UF2 bootloader.
    pub bootloader_usb: &'static [(u16, u16)],
    /// The compiler library variant ("multilib") this CPU uses. The managed toolchain
    /// keeps only these (nRF52840 = Cortex-M4F, hard float → ~1.1 GB → ~256 MB).
    pub multilib: &'static str,
    pub supported: bool,
    pub verified: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DisplayDef {
    pub kind: DisplayKind,
    pub name: &'static str,
    pub looks_like: &'static str,
    /// Extra shields to add to the build (after the keyboard shield).
    pub shields: &'static [&'static str],
    pub width: u32,
    pub height: u32,
    pub supported: bool,
}

pub const NRF52840_FAMILY: u32 = 0xADA5_2840;
/// `-mcpu=cortex-m4 -mfloat-abi=hard -mfpu=fpv4-sp-d16` (verified from a real build).
pub const NRF52840_MULTILIB: &str = "thumb/v7e-m+fp/hard";

pub const CORNE: KeyboardDef = KeyboardDef {
    base: "corne",
    name: "Corne",
    shields: &["corne_left", "corne_right", "corne"],
    key_count: 42,
};

pub const KEYBOARDS: &[KeyboardDef] = &[CORNE];

pub const CONTROLLERS: &[ControllerDef] = &[
    ControllerDef {
        id: ControllerId::NiceNanoV2,
        name: "nice!nano v2",
        looks_like: "A slim purple or black board with a USB-C port at one end and a small battery connector; \"nice!nano\" is printed on the back.",
        board: "nice_nano@2.0.0//zmk",
        board_aliases: &[
            "nice_nano@2.0.0//zmk",
            "nice_nano@2//zmk",
            "nice_nano//zmk",
            "nice_nano/nrf52840/zmk",
            "nice_nano@2.0.0/nrf52840/zmk",
            "nice_nano@2.0.0",
            "nice_nano@2",
            "nice_nano_v2",
            "nice_nano",
        ],
        uf2_models: &["nice!nano"],
        uf2_board_ids: &["nRF52840-nicenano"],
        uf2_family: NRF52840_FAMILY,
        multilib: NRF52840_MULTILIB,
        bootloader_usb: &[(0x239A, 0x00B3)],
        supported: true,
        verified: false,
    },
    ControllerDef {
        id: ControllerId::NiceNanoV1,
        name: "nice!nano v1",
        looks_like: "Same shape as the nice!nano v2; \"v1\" or an older revision is printed on the back.",
        board: "nice_nano@1.0.0//zmk",
        board_aliases: &["nice_nano@1.0.0//zmk", "nice_nano@1//zmk", "nice_nano@1.0.0", "nice_nano@1", "nice_nano_v1"],
        uf2_models: &[],
        uf2_board_ids: &[],
        uf2_family: NRF52840_FAMILY,
        multilib: NRF52840_MULTILIB,
        bootloader_usb: &[],
        supported: false,
        verified: false,
    },
    ControllerDef {
        id: ControllerId::XiaoBle,
        name: "Seeed XIAO nRF52840 (XIAO BLE)",
        looks_like: "A tiny square board, about the size of a thumbnail, with a metal shield can on top and \"XIAO\" printed on it.",
        board: "xiao_ble//zmk",
        board_aliases: &["xiao_ble//zmk", "xiao_ble", "seeeduino_xiao_ble"],
        uf2_models: &["xiao"],
        uf2_board_ids: &[],
        uf2_family: NRF52840_FAMILY,
        multilib: NRF52840_MULTILIB,
        bootloader_usb: &[],
        supported: false,
        verified: false,
    },
    ControllerDef {
        id: ControllerId::NrfMicro,
        name: "nRFMicro",
        looks_like: "A Pro Micro-sized board like the nice!nano, often blue or green, with \"nRFMicro\" printed on it.",
        board: "nrfmicro/nrf52840/zmk",
        board_aliases: &[
            "nrfmicro/nrf52840/zmk",
            "nrfmicro@1.3.0/nrf52840/zmk",
            "nrfmicro_13",
            "nrfmicro_13_52833",
            "nrfmicro_11",
            "nrfmicro",
        ],
        uf2_models: &["nrfmicro"],
        uf2_board_ids: &[],
        uf2_family: NRF52840_FAMILY,
        multilib: NRF52840_MULTILIB,
        bootloader_usb: &[],
        supported: false,
        verified: false,
    },
];

pub const DISPLAYS: &[DisplayDef] = &[
    DisplayDef {
        kind: DisplayKind::NiceView,
        name: "nice!view",
        looks_like: "A wide, low screen that looks like grey paper with black pixels (like an e-reader). It's readable in daylight without a backlight.",
        shields: &["nice_view_adapter", "nice_view"],
        width: 160,
        height: 68,
        supported: true,
    },
    DisplayDef {
        kind: DisplayKind::Oled128x32,
        name: "128×32 OLED",
        looks_like: "A small black glass screen with bright white (or blue) glowing pixels on a dark background.",
        shields: &[],
        width: 128,
        height: 32,
        supported: false,
    },
    DisplayDef {
        kind: DisplayKind::None,
        name: "No screen",
        looks_like: "Nothing above the controller — or just a blank cover plate.",
        shields: &[],
        width: 0,
        height: 0,
        supported: true,
    },
];

/// Multilibs the managed toolchain must keep: those of every supported controller.
pub fn toolchain_multilibs() -> Vec<&'static str> {
    let mut v: Vec<&str> = CONTROLLERS
        .iter()
        .filter(|c| c.supported)
        .map(|c| c.multilib)
        .collect();
    v.sort();
    v.dedup();
    v
}

pub fn keyboard(id: &KeyboardId) -> KeyboardInfo {
    match KEYBOARDS.iter().find(|k| k.base == id.as_str()) {
        Some(k) => KeyboardInfo {
            name: k.name.to_string(),
            known: true,
        },
        None => KeyboardInfo {
            name: pretty_name(id.as_str()),
            known: false,
        },
    }
}

/// `lily58` → "Lily58", `kyria_rev3` → "Kyria rev3", `cradio` → "Cradio".
pub fn pretty_name(base: &str) -> String {
    let spaced = base.replace(['_', '-'], " ");
    let mut c = spaced.chars();
    match c.next() {
        Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
        None => String::new(),
    }
}

/// The keyboard a shield name belongs to (`lily58_left` → `lily58`), and whether it
/// names a half.
pub fn keyboard_of_shield(shield: &str) -> (KeyboardId, bool) {
    if let Some(k) = KEYBOARDS.iter().find(|k| k.shields.contains(&shield)) {
        let half = shield.ends_with("_left") || shield.ends_with("_right");
        return (KeyboardId::new(k.base), half);
    }
    for suffix in ["_left", "_right"] {
        if let Some(b) = shield.strip_suffix(suffix) {
            return (KeyboardId::new(b), true);
        }
    }
    (KeyboardId::new(shield), false)
}

pub fn controller(id: ControllerId) -> &'static ControllerDef {
    CONTROLLERS
        .iter()
        .find(|c| c.id == id)
        .expect("every ControllerId has a definition")
}

pub fn display(kind: DisplayKind) -> &'static DisplayDef {
    DISPLAYS
        .iter()
        .find(|d| d.kind == kind)
        .expect("every DisplayKind has a definition")
}

/// Shields that are display add-ons (any display).
pub fn is_display_shield(shield: &str) -> bool {
    DISPLAYS.iter().any(|d| d.shields.contains(&shield))
}

/// Corne 6-column physical layout (42 keys) — from ZMK
/// `app/dts/layouts/foostan/corne/6column.dtsi` (MIT).
pub fn corne_layout() -> Vec<KeyGeom> {
    const ROW_Y: [i32; 12] = [37, 37, 12, 0, 12, 24, 24, 12, 0, 12, 37, 37];
    const ROW_X: [i32; 12] = [0, 100, 200, 300, 400, 500, 800, 900, 1000, 1100, 1200, 1300];
    let k = |w, h, x, y, r, rx, ry| KeyGeom {
        w,
        h,
        x,
        y,
        r,
        rx,
        ry,
    };
    let mut keys = Vec::with_capacity(42);
    for dy in [0, 100, 200] {
        for i in 0..12 {
            keys.push(k(100, 100, ROW_X[i], ROW_Y[i] + dy, 0, 0, 0));
        }
    }
    keys.extend([
        k(100, 100, 350, 312, 0, 0, 0),
        k(100, 100, 450, 312, 1200, 450, 412),
        k(100, 150, 548, 283, 2400, 548, 433),
        k(100, 150, 752, 283, -2400, 852, 433),
        k(100, 100, 850, 312, -1200, 950, 412),
        k(100, 100, 950, 312, 0, 0, 0),
    ]);
    keys
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_id_has_a_definition() {
        for c in [
            ControllerId::NiceNanoV2,
            ControllerId::NiceNanoV1,
            ControllerId::XiaoBle,
            ControllerId::NrfMicro,
        ] {
            assert_eq!(controller(c).id, c);
        }
        for d in [DisplayKind::None, DisplayKind::NiceView, DisplayKind::Oled128x32] {
            assert_eq!(display(d).kind, d);
        }
        assert_eq!(keyboard(&KeyboardId::corne()).name, "Corne");
        assert_eq!(keyboard(&KeyboardId::new("lily58")).name, "Lily58");
        assert!(!keyboard(&KeyboardId::new("kyria_rev3")).known);
        assert_eq!(pretty_name("kyria_rev3"), "Kyria rev3");
        assert_eq!(
            keyboard_of_shield("lily58_right"),
            (KeyboardId::new("lily58"), true)
        );
        assert_eq!(keyboard_of_shield("planck"), (KeyboardId::new("planck"), false));
        assert_eq!(keyboard_of_shield("corne_left"), (KeyboardId::corne(), true));
    }

    #[test]
    fn corne_layout_matches_key_count() {
        assert_eq!(corne_layout().len(), CORNE.key_count);
    }

    #[test]
    fn display_shields() {
        assert!(is_display_shield("nice_view") && is_display_shield("nice_view_adapter"));
        assert!(!is_display_shield("corne_left"));
    }
}
