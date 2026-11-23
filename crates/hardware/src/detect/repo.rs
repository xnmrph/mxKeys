//! Evidence from the config repository: build.yaml and `.conf` files.

use zmk_config::build_yaml::{BuildMatrix, BuildTarget};
use zmk_config::kconfig::{KconfigFile, KconfigValue};

use super::{Confidence, Evidence, Fact, Findings, Known, Note, Source};
use crate::catalog::{self, ControllerId, DisplayKind, Side};

fn side_of(shield: &str) -> Option<Side> {
    if shield.ends_with("_left") {
        Some(Side::Left)
    } else if shield.ends_with("_right") {
        Some(Side::Right)
    } else {
        None
    }
}

fn base_name(shield: &str) -> &str {
    shield.trim_end_matches("_left").trim_end_matches("_right")
}

fn keyboard_targets(m: &BuildMatrix) -> impl Iterator<Item = &BuildTarget> {
    m.targets
        .iter()
        .filter(|t| !t.is_settings_reset() && t.keyboard_shields().next().is_some())
}

/// Match a build.yaml board id to a controller. Returns (controller, exact?).
pub fn controller_for_board(raw: &str) -> Option<(ControllerId, bool)> {
    let raw = raw.trim();
    // Exact aliases first (e.g. `nice_nano@2.0.0`, `nice_nano_v2`).
    for c in catalog::CONTROLLERS {
        // Bare `nice_nano` is handled below (it depends on ZMK's default revision).
        if raw != "nice_nano" && c.board_aliases.contains(&raw) {
            return Some((c.id, true));
        }
    }
    let b = zmk_config::build_yaml::BoardId::parse(raw);
    // `nice_nano` without a revision: ZMK main defaults to revision 2.0.0.
    if b.name == "nice_nano" {
        return match b.revision.as_deref() {
            Some(r) if r.starts_with('1') => Some((ControllerId::NiceNanoV1, true)),
            Some(_) => Some((ControllerId::NiceNanoV2, true)),
            None => Some((ControllerId::NiceNanoV2, false)),
        };
    }
    catalog::CONTROLLERS
        .iter()
        .find(|c| {
            c.board_aliases
                .iter()
                .any(|a| zmk_config::build_yaml::BoardId::parse(a).name == b.name)
        })
        .map(|c| (c.id, true))
}

pub fn from_build_yaml(m: &BuildMatrix) -> Findings {
    let mut f = Findings::default();
    let src = Source::BuildYaml;

    // Keyboard.
    let mut kb_shields: Vec<&str> = Vec::new();
    for t in keyboard_targets(m) {
        for s in t.keyboard_shields() {
            if !kb_shields.contains(&s) {
                kb_shields.push(s);
            }
        }
    }
    let mut named: Vec<&str> = Vec::new();
    for s in &kb_shields {
        match catalog::KEYBOARDS.iter().find(|k| k.shields.contains(s)) {
            Some(k) => {
                if !named.contains(&k.name) {
                    named.push(k.name);
                    f.evidence.push(Evidence::new(
                        src,
                        Fact::Keyboard(Known::Known(k.id)),
                        Confidence::Medium,
                        format!("build.yaml builds {}", kb_shields.join(" and ")),
                    ));
                }
            }
            None => {
                let base = base_name(s);
                if !named.contains(&base) {
                    named.push(base);
                    f.evidence.push(Evidence::new(
                        src,
                        Fact::Keyboard(Known::Other(base.to_string())),
                        Confidence::Medium,
                        format!("build.yaml builds the “{s}” shield"),
                    ));
                }
            }
        }
    }

    // Controller (all targets, including settings_reset, use the controller's board).
    let mut seen: Vec<&str> = Vec::new();
    for t in &m.targets {
        let raw = t.board.raw.as_str();
        if seen.contains(&raw) {
            continue;
        }
        seen.push(raw);
        match controller_for_board(raw) {
            Some((id, exact)) => {
                let because = if exact {
                    format!("build.yaml builds for the board “{raw}”")
                } else {
                    format!(
                        "build.yaml builds for “{raw}” without a version; current ZMK picks v2 by default"
                    )
                };
                let conf = if exact {
                    Confidence::Medium
                } else {
                    Confidence::Weak
                };
                f.evidence.push(Evidence::new(
                    src,
                    Fact::Controller(Known::Known(id)),
                    conf,
                    because,
                ));
                let b = zmk_config::build_yaml::BoardId::parse(raw);
                let hwmv2 = b.revision.is_some() || b.qualifiers.is_some() || !raw.contains("_v");
                let has_zmk_variant = b
                    .qualifiers
                    .as_deref()
                    .is_some_and(|q| q.split('/').any(|p| p.ends_with("zmk")));
                if hwmv2 && !has_zmk_variant {
                    let fixed = catalog::controller(id).board;
                    f.notes.push(Note::warning(format!(
                        "build.yaml builds for “{raw}”. Current ZMK needs the “zmk” variant (“{fixed}”); without it the screen and other ZMK features fail to build. The app uses “{fixed}” when it builds.",
                    )));
                }
                if raw == "nice_nano_v2" {
                    f.notes.push(Note::info(
                        "build.yaml uses the old board name “nice_nano_v2”. Current ZMK calls it “nice_nano@2.0.0”; the app uses the new name when it builds.",
                    ));
                }
            }
            None => f.evidence.push(Evidence::new(
                src,
                Fact::Controller(Known::Other(raw.to_string())),
                Confidence::Medium,
                format!("build.yaml builds for the board “{raw}”"),
            )),
        }
    }

    // Display.
    let kts: Vec<&BuildTarget> = keyboard_targets(m).collect();
    if !kts.is_empty() {
        let with_view = kts.iter().filter(|t| t.has_nice_view()).count();
        if with_view > 0 {
            f.evidence.push(Evidence::new(
                src,
                Fact::Display(DisplayKind::NiceView),
                Confidence::Medium,
                "build.yaml adds the nice!view screen (nice_view_adapter nice_view)",
            ));
            if with_view < kts.len() {
                f.notes.push(Note::info(
                    "build.yaml builds every half both with and without the screen. The app only builds the versions your hardware needs.",
                ));
            }
        } else if !kts
            .iter()
            .any(|t| t.shields.iter().any(|s| catalog::is_display_shield(s)))
        {
            f.evidence.push(Evidence::new(
                src,
                Fact::Display(DisplayKind::None),
                Confidence::Weak,
                "build.yaml doesn't add a screen",
            ));
        }
    }

    // Central half / live editing.
    let studio_sides: Vec<Side> = kts
        .iter()
        .filter(|t| t.has_studio())
        .filter_map(|t| t.keyboard_shields().find_map(side_of))
        .collect();
    if let Some(side) = studio_sides.first() {
        f.evidence.push(Evidence::new(
            src,
            Fact::Central(*side),
            Confidence::Medium,
            "build.yaml turns on live editing (studio-rpc-usb-uart) for this half, which only works on the central half",
        ));
        f.evidence.push(Evidence::new(
            src,
            Fact::Studio(true),
            Confidence::Medium,
            "build.yaml adds the studio-rpc-usb-uart snippet",
        ));
    } else if !kts.is_empty() {
        f.evidence.push(Evidence::new(
            src,
            Fact::Studio(false),
            Confidence::Weak,
            "build.yaml doesn't add the live-editing snippet",
        ));
    }
    if kts
        .iter()
        .any(|t| t.keyboard_shields().any(|s| s == "corne_left"))
    {
        f.evidence.push(Evidence::new(
            src,
            Fact::Central(Side::Left),
            Confidence::Weak,
            "on a Corne the left half is the central half by default",
        ));
    }
    if m.settings_reset().is_none() && !m.targets.is_empty() {
        f.notes.push(Note::info(
            "build.yaml doesn't build the “settings reset” firmware. The app builds it anyway — it's only needed if the halves stop talking to each other.",
        ));
    }
    f
}

/// Evidence from a `.conf` file. `nice_view_in_build` tells whether build.yaml adds a
/// nice!view (to interpret `CONFIG_ZMK_DISPLAY=y` without a display shield).
pub fn from_conf(conf: &KconfigFile, nice_view_in_build: bool) -> Findings {
    let mut f = Findings::default();
    let src = Source::Settings;
    let yn = |on: bool| if on { "y" } else { "n" };
    if let Some(KconfigValue::Bool(on)) = conf.get("CONFIG_ZMK_STUDIO") {
        f.evidence.push(Evidence::new(
            src,
            Fact::Studio(*on),
            Confidence::Weak,
            format!("your settings have CONFIG_ZMK_STUDIO={}", yn(*on)),
        ));
    }
    if let Some(KconfigValue::Bool(on)) = conf.get("CONFIG_ZMK_DISPLAY") {
        f.evidence.push(Evidence::new(
            src,
            Fact::ScreenSoftware(*on),
            Confidence::Medium,
            format!("your settings have CONFIG_ZMK_DISPLAY={}", yn(*on)),
        ));
        if *on && !nice_view_in_build {
            f.evidence.push(Evidence::new(
                src,
                Fact::Display(DisplayKind::Oled128x32),
                Confidence::Weak,
                "your settings turn a screen on without the nice!view add-on, which on a Corne means the built-in 128×32 OLED support",
            ));
        }
    }
    f
}
