//! What can go on a key: keycodes (generated from ZMK's `keys.h`) and behaviors with
//! their parameter types (built-ins plus the custom behaviors defined in a keymap).

use std::collections::HashMap;
use std::sync::OnceLock;

use serde::{Deserialize, Serialize};

use crate::dts::{Document, Item};
use crate::keymap::{DefinitionKind, Keymap};

// ----- keycodes -------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Keycode {
    /// Canonical name first, then aliases (e.g. `["LEFT_SHIFT", "LSHIFT", "LSHFT"]`).
    pub names: Vec<String>,
    /// Still accepted by ZMK but not offered.
    #[serde(default)]
    pub deprecated: Vec<String>,
    /// From keys.h, e.g. "Keyboard ; and : (Semicolon and Colon)".
    pub description: String,
    /// `key`, `consumer` (media) or `gd` (system).
    pub page: String,
    pub usage: String,
    /// Defined as Shift + another key (e.g. `!`).
    #[serde(default)]
    pub shifted: bool,
    pub category: String,
    /// Short label for a keycap, e.g. `;`, `Vol+`, `Shift`.
    pub label: String,
}

#[derive(Deserialize)]
struct KeycodeFile {
    source: String,
    keys: Vec<Keycode>,
}

const KEYCODES_JSON: &str = include_str!("../data/keycodes.json");

fn keycode_file() -> &'static KeycodeFile {
    static F: OnceLock<KeycodeFile> = OnceLock::new();
    F.get_or_init(|| serde_json::from_str(KEYCODES_JSON).expect("bundled keycodes.json is valid"))
}

/// All keycodes, in keys.h order.
pub fn keycodes() -> &'static [Keycode] {
    &keycode_file().keys
}

/// Where the keycode table came from (ZMK commit).
pub fn keycodes_source() -> &'static str {
    &keycode_file().source
}

/// Look up any name or alias (including deprecated ones).
pub fn keycode(name: &str) -> Option<&'static Keycode> {
    static IDX: OnceLock<HashMap<&'static str, usize>> = OnceLock::new();
    let idx = IDX.get_or_init(|| {
        let mut m = HashMap::new();
        for (i, k) in keycodes().iter().enumerate() {
            for n in k.names.iter().chain(&k.deprecated) {
                m.entry(n.as_str()).or_insert(i);
            }
        }
        m
    });
    idx.get(name).map(|&i| &keycodes()[i])
}

// ----- behaviors ------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CommandOption {
    pub value: &'static str,
    pub label: &'static str,
    /// Takes a number after the command (e.g. `BT_SEL 0`): (min, max, what it is).
    pub number: Option<(u32, u32, &'static str)>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ParamKind {
    /// Any keycode, optionally wrapped in modifier functions (`LS(A)`).
    Keycode,
    /// A modifier keycode (for hold-taps' hold side).
    Modifier,
    /// A layer number (or a `#define` for one).
    Layer,
    /// One of a fixed set of commands, possibly followed by a number.
    Command { options: Vec<CommandOption> },
    /// Free text (unknown parameter type).
    Free,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Param {
    pub name: String,
    pub kind: ParamKind,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Behavior {
    /// `&kp`
    pub reference: String,
    pub name: String,
    pub description: String,
    /// `keys`, `layers`, `hold_tap`, `connection`, `system`, `mouse`, `custom`.
    pub category: String,
    pub params: Vec<Param>,
}

fn p(name: &str, kind: ParamKind) -> Param {
    Param {
        name: name.into(),
        kind,
    }
}

fn cmd(value: &'static str, label: &'static str) -> CommandOption {
    CommandOption {
        value,
        label,
        number: None,
    }
}

fn b(reference: &str, name: &str, description: &str, category: &str, params: Vec<Param>) -> Behavior {
    Behavior {
        reference: reference.into(),
        name: name.into(),
        description: description.into(),
        category: category.into(),
        params,
    }
}

/// ZMK's built-in behaviors that make sense on a key.
pub fn builtin_behaviors() -> Vec<Behavior> {
    use ParamKind::*;
    vec![
        b(
            "&kp",
            "Key",
            "Types a key (with optional modifiers like Shift).",
            "keys",
            vec![p("key", Keycode)],
        ),
        b(
            "&trans",
            "Transparent",
            "Does whatever the layer below does.",
            "keys",
            vec![],
        ),
        b("&none", "Nothing", "Does nothing.", "keys", vec![]),
        b(
            "&mt",
            "Modifier on hold, key on tap",
            "Hold for a modifier, tap for a key.",
            "hold_tap",
            vec![p("hold", Modifier), p("tap", Keycode)],
        ),
        b(
            "&lt",
            "Layer on hold, key on tap",
            "Hold to use a layer, tap for a key.",
            "hold_tap",
            vec![p("layer", Layer), p("tap", Keycode)],
        ),
        b(
            "&mo",
            "Layer while held",
            "Use a layer while the key is held.",
            "layers",
            vec![p("layer", Layer)],
        ),
        b(
            "&to",
            "Switch to layer",
            "Switch to a layer (and stay there).",
            "layers",
            vec![p("layer", Layer)],
        ),
        b(
            "&tog",
            "Toggle layer",
            "Turn a layer on or off.",
            "layers",
            vec![p("layer", Layer)],
        ),
        b(
            "&sl",
            "Sticky layer",
            "Use a layer for the next key press only.",
            "layers",
            vec![p("layer", Layer)],
        ),
        b(
            "&sk",
            "Sticky key",
            "Hold a modifier/key for the next key press only.",
            "keys",
            vec![p("key", Keycode)],
        ),
        b(
            "&kt",
            "Key toggle",
            "Press and stay pressed until tapped again.",
            "keys",
            vec![p("key", Keycode)],
        ),
        b(
            "&caps_word",
            "Caps word",
            "Capitalise until the end of the word.",
            "keys",
            vec![],
        ),
        b(
            "&key_repeat",
            "Repeat key",
            "Repeat the last key.",
            "keys",
            vec![],
        ),
        b(
            "&gresc",
            "Grave/Escape",
            "Escape, or ` with Shift/GUI.",
            "keys",
            vec![],
        ),
        b(
            "&bt",
            "Bluetooth",
            "Choose, clear or cycle Bluetooth connections.",
            "connection",
            vec![p(
                "command",
                Command {
                    options: vec![
                        CommandOption {
                            value: "BT_SEL",
                            label: "Use profile",
                            number: Some((0, 4, "profile")),
                        },
                        cmd("BT_NXT", "Next profile"),
                        cmd("BT_PRV", "Previous profile"),
                        cmd("BT_CLR", "Forget current profile"),
                        cmd("BT_CLR_ALL", "Forget all profiles"),
                        CommandOption {
                            value: "BT_DISC",
                            label: "Disconnect profile",
                            number: Some((0, 4, "profile")),
                        },
                    ],
                },
            )],
        ),
        b(
            "&out",
            "USB / Bluetooth output",
            "Choose whether typing goes over USB or Bluetooth.",
            "connection",
            vec![p(
                "command",
                Command {
                    options: vec![
                        cmd("OUT_TOG", "Toggle USB ↔ Bluetooth"),
                        cmd("OUT_USB", "Use USB"),
                        cmd("OUT_BLE", "Use Bluetooth"),
                    ],
                },
            )],
        ),
        b(
            "&rgb_ug",
            "RGB underglow",
            "Control the RGB lights.",
            "system",
            vec![p(
                "command",
                Command {
                    options: vec![
                        cmd("RGB_TOG", "On/off"),
                        cmd("RGB_ON", "On"),
                        cmd("RGB_OFF", "Off"),
                        cmd("RGB_EFF", "Next effect"),
                        cmd("RGB_EFR", "Previous effect"),
                        cmd("RGB_BRI", "Brighter"),
                        cmd("RGB_BRD", "Dimmer"),
                        cmd("RGB_HUI", "Hue +"),
                        cmd("RGB_HUD", "Hue −"),
                        cmd("RGB_SAI", "Saturation +"),
                        cmd("RGB_SAD", "Saturation −"),
                        cmd("RGB_SPI", "Faster"),
                        cmd("RGB_SPD", "Slower"),
                    ],
                },
            )],
        ),
        b(
            "&ext_power",
            "External power",
            "Switch power to the screen/LEDs.",
            "system",
            vec![p(
                "command",
                Command {
                    options: vec![cmd("EP_TOG", "Toggle"), cmd("EP_ON", "On"), cmd("EP_OFF", "Off")],
                },
            )],
        ),
        b(
            "&mkp",
            "Mouse button",
            "Click a mouse button.",
            "mouse",
            vec![p(
                "button",
                Command {
                    options: vec![
                        cmd("LCLK", "Left click"),
                        cmd("RCLK", "Right click"),
                        cmd("MCLK", "Middle click"),
                        cmd("MB4", "Back button"),
                        cmd("MB5", "Forward button"),
                    ],
                },
            )],
        ),
        b(
            "&mmv",
            "Mouse move",
            "Move the mouse pointer.",
            "mouse",
            vec![p(
                "direction",
                Command {
                    options: vec![
                        cmd("MOVE_UP", "Up"),
                        cmd("MOVE_DOWN", "Down"),
                        cmd("MOVE_LEFT", "Left"),
                        cmd("MOVE_RIGHT", "Right"),
                    ],
                },
            )],
        ),
        b(
            "&msc",
            "Mouse scroll",
            "Scroll.",
            "mouse",
            vec![p(
                "direction",
                Command {
                    options: vec![
                        cmd("SCRL_UP", "Up"),
                        cmd("SCRL_DOWN", "Down"),
                        cmd("SCRL_LEFT", "Left"),
                        cmd("SCRL_RIGHT", "Right"),
                    ],
                },
            )],
        ),
        b(
            "&sys_reset",
            "Restart keyboard",
            "Restart this half.",
            "system",
            vec![],
        ),
        b(
            "&bootloader",
            "Update mode",
            "Enter bootloader mode (like double-tapping reset).",
            "system",
            vec![],
        ),
        b(
            "&soft_off",
            "Turn off",
            "Turn the keyboard off.",
            "system",
            vec![],
        ),
        b(
            "&studio_unlock",
            "Studio unlock",
            "Allow live editing over USB/Bluetooth.",
            "system",
            vec![],
        ),
    ]
}

fn binding_cells(props: &[(String, String)]) -> usize {
    props
        .iter()
        .find(|(k, _)| k == "#binding-cells")
        .and_then(|(_, v)| v.trim_matches(|c| c == '<' || c == '>' || c == ' ').parse().ok())
        .unwrap_or(0)
}

fn prettify(name: &str) -> String {
    let s = name.replace(['_', '-'], " ");
    let mut c = s.chars();
    match c.next() {
        Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
        None => s,
    }
}

/// Parameter kind for a hold-tap side, from its `bindings = <&kp>, <&mo>` entry.
fn kind_for(inner: &str, hold: bool) -> ParamKind {
    match inner.trim() {
        "&kp" if hold => ParamKind::Modifier,
        "&kp" => ParamKind::Keycode,
        "&mo" | "&to" | "&tog" | "&sl" => ParamKind::Layer,
        _ => ParamKind::Free,
    }
}

/// Custom behaviors defined in this keymap (hold-taps, macros, mod-morphs, …).
pub fn custom_behaviors(doc: &Document, km: &Keymap) -> Vec<Behavior> {
    let _ = doc;
    km.definitions
        .iter()
        .filter(|d| matches!(d.kind, DefinitionKind::Behavior | DefinitionKind::Macro))
        .filter_map(|d| {
            let label = d.labels.first()?;
            let compatible = d.compatible.as_deref().unwrap_or("");
            let cells = binding_cells(&d.properties);
            let display = d
                .properties
                .iter()
                .find(|(k, _)| k == "display-name" || k == "label")
                .map(|(_, v)| v.trim_matches('"').to_string())
                .unwrap_or_else(|| prettify(&d.node_name));
            let (kind_name, params) = if compatible == "zmk,behavior-hold-tap" {
                let inner: Vec<String> = d
                    .properties
                    .iter()
                    .find(|(k, _)| k == "bindings")
                    .map(|(_, v)| {
                        v.split(',')
                            .map(|s| s.trim().trim_matches(|c| c == '<' || c == '>').to_string())
                            .collect()
                    })
                    .unwrap_or_default();
                let hold = kind_for(inner.first().map(String::as_str).unwrap_or(""), true);
                let tap = kind_for(inner.get(1).map(String::as_str).unwrap_or(""), false);
                ("Hold-tap", vec![p("hold", hold), p("tap", tap)])
            } else {
                let kind = match compatible {
                    c if c.starts_with("zmk,behavior-macro") => "Macro",
                    "zmk,behavior-mod-morph" => "Shift-dependent key",
                    "zmk,behavior-tap-dance" => "Tap dance",
                    "zmk,behavior-sticky-key" => "Sticky key",
                    "zmk,behavior-caps-word" => "Caps word",
                    _ => "Custom",
                };
                let params = (0..cells)
                    .map(|i| p(&format!("parameter {}", i + 1), ParamKind::Free))
                    .collect();
                (kind, params)
            };
            Some(Behavior {
                reference: format!("&{label}"),
                name: display,
                description: format!("{kind_name} defined in your keymap."),
                category: "custom".into(),
                params,
            })
        })
        .collect()
}

/// A layer as a parameter value: its index, title, and a `#define` name if the keymap
/// has one for that number (e.g. `NAV` for 1).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct LayerRef {
    pub index: usize,
    pub title: String,
    pub define: Option<String>,
}

pub fn layer_refs(doc: &Document, km: &Keymap) -> Vec<LayerRef> {
    let mut by_value: HashMap<usize, String> = HashMap::new();
    for item in doc.walk() {
        if let Item::Directive(crate::dts::Directive {
            kind:
                crate::dts::DirectiveKind::Define {
                    name,
                    params: None,
                    body,
                },
            ..
        }) = item
        {
            if let Ok(n) = body.trim().parse::<usize>() {
                // Prefer a define whose name matches the layer node name.
                let entry = by_value.entry(n).or_insert_with(|| name.clone());
                if km.layers.get(n).is_some_and(|l| &l.node_name == name) {
                    *entry = name.clone();
                }
            }
        }
    }
    km.layers
        .iter()
        .map(|l| LayerRef {
            index: l.index,
            title: l.title(),
            define: by_value.get(&l.index).cloned(),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keycode_lookup() {
        assert!(keycodes().len() > 300);
        let shift = keycode("LSHFT").unwrap();
        assert_eq!(shift.names[0], "LEFT_SHIFT");
        assert_eq!(shift.label, "Shift");
        assert_eq!(keycode("EXCL").unwrap().label, "!");
        assert!(keycode("EXCL").unwrap().shifted);
        assert_eq!(keycode("N1").unwrap().label, "1");
        assert_eq!(keycode("C_VOL_UP").unwrap().category, "media");
        assert_eq!(
            keycode("LCUR").unwrap().names[0],
            "LEFT_BRACE",
            "deprecated alias still resolves"
        );
        assert!(keycode("NOT_A_KEY").is_none());
        assert!(keycodes_source().contains("4de3133"));
    }

    #[test]
    fn builtins_are_unique() {
        let b = builtin_behaviors();
        let mut refs: Vec<_> = b.iter().map(|x| x.reference.as_str()).collect();
        refs.sort();
        refs.dedup();
        assert_eq!(refs.len(), b.len());
        let out = b.iter().find(|x| x.reference == "&out").unwrap();
        assert!(
            matches!(&out.params[0].kind, ParamKind::Command { options } if options[0].value == "OUT_TOG")
        );
    }
}
