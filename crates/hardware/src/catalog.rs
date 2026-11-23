//! What mxKeys knows about hardware. v1 *supports* exactly one combination (Corne +
//! nice!nano v2 + nice!view); other entries exist only so detection and the quiz can
//! recognise them and say "not supported yet" instead of guessing.
//!
//! Identifiers marked `verified: false` come from memory/common knowledge, not from a
//! device on the desk — confirm them on real hardware (see docs/PROGRESS.md).

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum KeyboardId {
    Corne,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct HalfDef {
    pub side: Side,
    pub shield: &'static str,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct KeyboardDef {
    pub id: KeyboardId,
    pub name: &'static str,
    pub halves: &'static [HalfDef],
    /// Shield names that identify this keyboard in build.yaml.
    pub shields: &'static [&'static str],
    pub key_count: usize,
    pub supported: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ControllerDef {
    pub id: ControllerId,
    pub name: &'static str,
    /// Plain-language description for the "which controller do I have?" quiz.
    pub looks_like: &'static str,
    /// ZMK board id to build with (current HWMv2 naming).
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

pub const CORNE: KeyboardDef = KeyboardDef {
    id: KeyboardId::Corne,
    name: "Corne",
    halves: &[
        HalfDef {
            side: Side::Left,
            shield: "corne_left",
        },
        HalfDef {
            side: Side::Right,
            shield: "corne_right",
        },
    ],
    shields: &["corne_left", "corne_right", "corne"],
    key_count: 42,
    supported: true,
};

pub const KEYBOARDS: &[KeyboardDef] = &[CORNE];

pub const CONTROLLERS: &[ControllerDef] = &[
    ControllerDef {
        id: ControllerId::NiceNanoV2,
        name: "nice!nano v2",
        looks_like: "A slim purple or black board with a USB-C port at one end and a small battery connector; \"nice!nano\" is printed on the back.",
        board: "nice_nano@2.0.0",
        board_aliases: &["nice_nano@2.0.0", "nice_nano@2", "nice_nano_v2", "nice_nano//zmk", "nice_nano"],
        uf2_models: &["nice!nano"],
        uf2_board_ids: &["nRF52840-nicenano"],
        uf2_family: NRF52840_FAMILY,
        bootloader_usb: &[(0x239A, 0x00B3)],
        supported: true,
        verified: false,
    },
    ControllerDef {
        id: ControllerId::NiceNanoV1,
        name: "nice!nano v1",
        looks_like: "Same shape as the nice!nano v2; \"v1\" or an older revision is printed on the back.",
        board: "nice_nano@1.0.0",
        board_aliases: &["nice_nano@1.0.0", "nice_nano@1", "nice_nano_v1"],
        uf2_models: &[],
        uf2_board_ids: &[],
        uf2_family: NRF52840_FAMILY,
        bootloader_usb: &[],
        supported: false,
        verified: false,
    },
    ControllerDef {
        id: ControllerId::XiaoBle,
        name: "Seeed XIAO nRF52840 (XIAO BLE)",
        looks_like: "A tiny square board, about the size of a thumbnail, with a metal shield can on top and \"XIAO\" printed on it.",
        board: "xiao_ble",
        board_aliases: &["xiao_ble", "seeeduino_xiao_ble"],
        uf2_models: &["xiao"],
        uf2_board_ids: &[],
        uf2_family: NRF52840_FAMILY,
        bootloader_usb: &[],
        supported: false,
        verified: false,
    },
    ControllerDef {
        id: ControllerId::NrfMicro,
        name: "nRFMicro",
        looks_like: "A Pro Micro-sized board like the nice!nano, often blue or green, with \"nRFMicro\" printed on it.",
        board: "nrfmicro",
        board_aliases: &["nrfmicro_13", "nrfmicro_13_52833", "nrfmicro_11", "nrfmicro", "nrfmicro@1.3.0"],
        uf2_models: &["nrfmicro"],
        uf2_board_ids: &[],
        uf2_family: NRF52840_FAMILY,
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

pub fn keyboard(id: KeyboardId) -> &'static KeyboardDef {
    KEYBOARDS
        .iter()
        .find(|k| k.id == id)
        .expect("every KeyboardId has a definition")
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
        assert_eq!(keyboard(KeyboardId::Corne).name, "Corne");
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
