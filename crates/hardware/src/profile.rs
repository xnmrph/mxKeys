//! The confirmed hardware profile and everything derived from it (build targets).

use serde::{Deserialize, Serialize};

use crate::catalog::{self, ControllerId, DisplayKind, KeyboardId, Side};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct HardwareProfile {
    pub keyboard: KeyboardId,
    pub controller: ControllerId,
    pub display: DisplayKind,
    /// The half that talks to the computer (and runs ZMK Studio).
    pub central: Side,
    /// Live editing (ZMK Studio) enabled on the central half.
    pub studio: bool,
}

impl HardwareProfile {
    /// The v1 target hardware.
    pub const CORNE_NICE_NANO_V2_NICE_VIEW: HardwareProfile = HardwareProfile {
        keyboard: KeyboardId::Corne,
        controller: ControllerId::NiceNanoV2,
        display: DisplayKind::NiceView,
        central: Side::Left,
        studio: true,
    };

    /// Plain-language reasons the app can't fully support this profile yet.
    pub fn unsupported_parts(&self) -> Vec<String> {
        let mut out = Vec::new();
        if !catalog::keyboard(self.keyboard).supported {
            out.push(format!("{} keyboards", catalog::keyboard(self.keyboard).name));
        }
        let c = catalog::controller(self.controller);
        if !c.supported {
            out.push(format!("the {} controller", c.name));
        }
        let d = catalog::display(self.display);
        if !d.supported {
            out.push(format!("the {} screen", d.name));
        }
        if self.central != Side::Left {
            out.push("a right-hand central half".to_string());
        }
        out
    }

    /// "Corne · nice!nano v2 · nice!view"
    pub fn title(&self) -> String {
        format!(
            "{} · {} · {}",
            catalog::keyboard(self.keyboard).name,
            catalog::controller(self.controller).name,
            catalog::display(self.display).name
        )
    }

    /// The firmware variants the app builds for this profile — only what's needed
    /// (each half with its display, plus settings reset).
    pub fn build_targets(&self) -> Vec<BuildTarget> {
        let kb = catalog::keyboard(self.keyboard);
        let ctrl = catalog::controller(self.controller);
        let disp = catalog::display(self.display);
        let mut out: Vec<BuildTarget> = kb
            .halves
            .iter()
            .map(|h| {
                let mut shields = vec![h.shield.to_string()];
                shields.extend(disp.shields.iter().map(|s| s.to_string()));
                let central = h.side == self.central;
                BuildTarget {
                    id: match h.side {
                        Side::Left => TargetId::Left,
                        Side::Right => TargetId::Right,
                    },
                    board: ctrl.board.to_string(),
                    shields,
                    snippets: if central && self.studio {
                        vec![zmk_config::build_yaml::STUDIO_SNIPPET.to_string()]
                    } else {
                        Vec::new()
                    },
                }
            })
            .collect();
        out.push(BuildTarget {
            id: TargetId::SettingsReset,
            board: ctrl.board.to_string(),
            shields: vec![zmk_config::build_yaml::SETTINGS_RESET_SHIELD.to_string()],
            snippets: Vec::new(),
        });
        out
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TargetId {
    Left,
    Right,
    SettingsReset,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct BuildTarget {
    pub id: TargetId,
    pub board: String,
    pub shields: Vec<String>,
    pub snippets: Vec<String>,
}

impl BuildTarget {
    /// Whether a build.yaml entry describes the same firmware.
    pub fn matches(&self, t: &zmk_config::build_yaml::BuildTarget) -> bool {
        let same_board = catalog::CONTROLLERS
            .iter()
            .find(|c| c.board == self.board)
            .map_or(t.board.raw == self.board, |c| {
                c.board_aliases.contains(&t.board.raw.as_str())
            });
        same_board && t.shields == self.shields && t.snippets == self.snippets
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use zmk_config::build_yaml::BuildMatrix;

    #[test]
    fn v1_profile_targets_match_the_brief() {
        let p = HardwareProfile::CORNE_NICE_NANO_V2_NICE_VIEW;
        assert_eq!(p.title(), "Corne · nice!nano v2 · nice!view");
        assert!(p.unsupported_parts().is_empty());
        let t = p.build_targets();
        let flat: Vec<_> = t
            .iter()
            .map(|t| (t.board.as_str(), t.shields.join(" "), t.snippets.join(" ")))
            .collect();
        assert_eq!(
            flat,
            [
                (
                    "nice_nano@2.0.0",
                    "corne_left nice_view_adapter nice_view".to_string(),
                    "studio-rpc-usb-uart".to_string()
                ),
                (
                    "nice_nano@2.0.0",
                    "corne_right nice_view_adapter nice_view".to_string(),
                    String::new()
                ),
                ("nice_nano@2.0.0", "settings_reset".to_string(), String::new()),
            ]
        );
    }

    #[test]
    fn targets_match_owners_build_yaml_entries() {
        let m = BuildMatrix::parse(
            "include:\n  - board: nice_nano@2.0.0\n    shield: corne_left\n    snippet: studio-rpc-usb-uart\n  - board: nice_nano_v2\n    shield: corne_left nice_view_adapter nice_view\n    snippet: studio-rpc-usb-uart\n  - board: nice_nano@2.0.0\n    shield: settings_reset\n",
        )
        .unwrap();
        let t = HardwareProfile::CORNE_NICE_NANO_V2_NICE_VIEW.build_targets();
        assert!(!t[0].matches(&m.targets[0]), "no screen variant isn't needed");
        assert!(
            t[0].matches(&m.targets[1]),
            "legacy board name is the same controller"
        );
        assert!(t[2].matches(&m.targets[2]));
    }

    #[test]
    fn unsupported_parts_are_named() {
        let p = HardwareProfile {
            controller: ControllerId::XiaoBle,
            display: DisplayKind::Oled128x32,
            ..HardwareProfile::CORNE_NICE_NANO_V2_NICE_VIEW
        };
        assert_eq!(
            p.unsupported_parts(),
            [
                "the Seeed XIAO nRF52840 (XIAO BLE) controller",
                "the 128×32 OLED screen"
            ]
        );
        let no_screen = HardwareProfile {
            display: DisplayKind::None,
            ..HardwareProfile::CORNE_NICE_NANO_V2_NICE_VIEW
        };
        assert_eq!(no_screen.build_targets()[1].shields, ["corne_right"]);
    }
}
