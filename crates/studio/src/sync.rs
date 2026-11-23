//! Comparing the keyboard with the files, and turning the keyboard's numbers back into
//! source text (for "keep the keyboard's keys").

use std::collections::HashMap;

use serde::Serialize;
use zmk_config::keymap::Keymap;

use crate::mapping::{Mapper, Unmappable};
use crate::proto::keymap::{BehaviorBinding, Keymap as DeviceKeymap};

#[derive(Debug, Clone)]
pub struct ExpectedKey {
    pub text: String,
    pub binding: Result<BehaviorBinding, Unmappable>,
}

#[derive(Debug, Clone)]
pub struct ExpectedLayer {
    /// Devicetree index = the keyboard's layer id.
    pub index: usize,
    pub title: String,
    /// The name the keyboard reports: `display-name`, else `label`, else empty.
    pub device_name: String,
    pub keys: Vec<ExpectedKey>,
}

/// What the files say the keyboard should contain.
pub fn expected(km: &Keymap, mapper: &Mapper) -> Vec<ExpectedLayer> {
    km.layers
        .iter()
        .map(|l| ExpectedLayer {
            index: l.index,
            title: l.title(),
            device_name: l.display_name.clone().unwrap_or_default(),
            keys: l
                .bindings
                .as_ref()
                .map(|b| {
                    b.bindings
                        .iter()
                        .map(|k| ExpectedKey {
                            text: k.raw.split_whitespace().collect::<Vec<_>>().join(" "),
                            binding: mapper.encode(&k.raw),
                        })
                        .collect()
                })
                .unwrap_or_default(),
        })
        .collect()
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct KeyDiff {
    pub layer: usize,
    pub layer_title: String,
    pub position: usize,
    pub file_text: String,
    /// The keyboard's binding as source text (best effort).
    pub device_text: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Unchecked {
    pub layer: usize,
    pub position: usize,
    pub file_text: String,
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Comparison {
    /// Layers/key counts don't match: the keyboard runs different firmware.
    pub structure: Option<String>,
    pub diffs: Vec<KeyDiff>,
    /// Layer renames: (layer, files' name, keyboard's name).
    pub names: Vec<(usize, String, String)>,
    pub unchecked: Vec<Unchecked>,
}

impl Comparison {
    pub fn in_sync(&self) -> bool {
        self.structure.is_none() && self.diffs.is_empty() && self.names.is_empty()
    }
}

pub fn compare(
    device: &DeviceKeymap,
    expected: &[ExpectedLayer],
    mapper: &Mapper,
    layer_tokens: &[String],
) -> Comparison {
    let mut c = Comparison {
        structure: None,
        diffs: vec![],
        names: vec![],
        unchecked: vec![],
    };
    if device.layers.len() != expected.len() {
        c.structure = Some(format!(
            "the keyboard has {} layers, your files have {}",
            device.layers.len(),
            expected.len()
        ));
        return c;
    }
    for e in expected {
        let Some(d) = device.layers.iter().find(|l| l.id as usize == e.index) else {
            c.structure = Some(format!("the keyboard has no layer {}", e.index));
            return c;
        };
        if d.bindings.len() != e.keys.len() {
            c.structure = Some(format!(
                "layer “{}” has {} keys on the keyboard, {} in your files",
                e.title,
                d.bindings.len(),
                e.keys.len()
            ));
            return c;
        }
        if d.name != e.device_name {
            c.names.push((e.index, e.device_name.clone(), d.name.clone()));
        }
        for (pos, (k, db)) in e.keys.iter().zip(&d.bindings).enumerate() {
            match &k.binding {
                Ok(b) if b == db => {}
                Ok(_) => c.diffs.push(KeyDiff {
                    layer: e.index,
                    layer_title: e.title.clone(),
                    position: pos,
                    file_text: k.text.clone(),
                    device_text: mapper.decode(db, Some(&k.text), layer_tokens),
                }),
                Err(why) => c.unchecked.push(Unchecked {
                    layer: e.index,
                    position: pos,
                    file_text: k.text.clone(),
                    reason: why.to_string(),
                }),
            }
        }
    }
    c
}

/// Modifier wrappers in the top byte of a HID usage, ZMK's bit order.
const MOD_FNS: [(u32, &str); 8] = [
    (0x01, "LC"),
    (0x02, "LS"),
    (0x04, "LA"),
    (0x08, "LG"),
    (0x10, "RC"),
    (0x20, "RS"),
    (0x40, "RA"),
    (0x80, "RG"),
];

/// Header whose names a behavior's parameters come from (for decoding commands).
fn header_for(label: &str) -> Option<&'static str> {
    Some(match label {
        "bt" => "bt.h",
        "out" => "outputs.h",
        "rgb_ug" => "rgb.h",
        "ext_power" => "ext_power.h",
        "mmv" | "msc" | "mkp" => "pointing.h",
        "bl" => "backlight.h",
        _ => return None,
    })
}

/// Behaviors whose parameters are keys (HID usages).
fn key_params(label: &str) -> (bool, bool) {
    match label {
        "kp" | "kt" | "sk" => (true, false),
        "mt" => (true, true),
        "lt" => (false, true),
        _ => (false, false),
    }
}

/// Behaviors whose first parameter is a layer.
fn layer_param(label: &str) -> bool {
    matches!(label, "mo" | "to" | "tog" | "sl" | "lt")
}

impl Mapper {
    /// Label (without `&`) for a keyboard behavior id: prefer the label `hint` uses.
    fn label_for(&self, id: u32, hint: Option<&str>) -> Option<String> {
        let name = self.device.name(id)?;
        let hint_label = hint
            .and_then(|h| h.trim().strip_prefix('&'))
            .and_then(|h| h.split_whitespace().next());
        let mut labels: Vec<&String> = self
            .names
            .iter()
            .filter(|(_, n)| n.as_str() == name)
            .map(|(l, _)| l)
            .collect();
        labels.sort_by_key(|l| (Some(l.as_str()) != hint_label, l.len(), l.to_string()));
        labels.first().map(|l| l.to_string())
    }

    fn key_name(&self, v: u32, table: &HashMap<u32, String>) -> String {
        let mods = v >> 24;
        let base = v & 0x00FF_FFFF;
        let mut s = table.get(&base).cloned().unwrap_or_else(|| format!("0x{base:x}"));
        for (bit, f) in MOD_FNS.iter().rev() {
            if mods & bit != 0 {
                s = format!("{f}({s})");
            }
        }
        if mods & !0xFF != 0 {
            return format!("0x{v:x}");
        }
        s
    }

    /// The keyboard's binding as source text. Keeps `hint` (the file's text) when it
    /// means the same thing.
    pub fn decode(&self, b: &BehaviorBinding, hint: Option<&str>, layer_tokens: &[String]) -> String {
        if let Some(h) = hint {
            if self.encode(h).as_ref() == Ok(b) {
                return h.to_string();
            }
        }
        let Some(label) = self.label_for(b.behavior_id as u32, hint) else {
            let name = self.device.name(b.behavior_id as u32).unwrap_or("?");
            return format!("“{name}” {} {}", b.param1, b.param2);
        };
        let keys: HashMap<u32, String> = zmk_config::catalog::keycodes()
            .iter()
            .rev()
            .filter_map(|k| {
                let n = k.names.first()?;
                let v = self.macros.cells(n).ok()?;
                (v.len() == 1).then(|| (v[0], n.clone()))
            })
            .collect();
        let layer = |v: u32| {
            layer_tokens
                .get(v as usize)
                .cloned()
                .unwrap_or_else(|| v.to_string())
        };
        let (k1, k2) = key_params(&label);
        let p = |v: u32, is_key: bool| {
            if is_key {
                self.key_name(v, &keys)
            } else {
                v.to_string()
            }
        };
        let params: Vec<String> = if layer_param(&label) {
            if label == "lt" {
                vec![layer(b.param1), p(b.param2, true)]
            } else {
                vec![layer(b.param1)]
            }
        } else if k1 || k2 {
            if label == "mt" {
                vec![p(b.param1, true), p(b.param2, true)]
            } else {
                vec![p(b.param1, true)]
            }
        } else if let Some(h) = header_for(&label) {
            let names = self.macros.names_from(h);
            if let Some((n, _)) = names.iter().find(|(_, v)| v.as_slice() == [b.param1, b.param2]) {
                vec![n.clone()]
            } else if let Some((n, _)) = names.iter().find(|(_, v)| v.as_slice() == [b.param1]) {
                if b.param2 != 0 {
                    vec![n.clone(), b.param2.to_string()]
                } else {
                    vec![n.clone()]
                }
            } else {
                vec![b.param1.to_string(), b.param2.to_string()]
            }
        } else if b.param2 != 0 {
            vec![b.param1.to_string(), b.param2.to_string()]
        } else if b.param1 != 0 {
            vec![b.param1.to_string()]
        } else {
            vec![]
        };
        std::iter::once(format!("&{label}"))
            .chain(params)
            .collect::<Vec<_>>()
            .join(" ")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mapping::DeviceBehaviors;
    use zmk_config::cpp::Macros;
    use zmk_config::dts::Document;

    fn mapper() -> Mapper {
        let mut macros = Macros::default();
        // A tiny stand-in for ZMK's headers.
        macros.add_source(
            "#define A 0x70004\n#define B 0x70005\n#define ESCAPE 0x70029\n#define LEFT_SHIFT 0x700e1\n\
             #define LS(k) (0x02 << 24 | k)\n#define LC(k) (0x01 << 24 | k)\n#define NAV 1\n",
            true,
        );
        let names: HashMap<String, String> = [
            ("kp", "Key Press"),
            ("mo", "Momentary Layer"),
            ("mt", "Mod-Tap"),
            ("bt", "Bluetooth"),
        ]
        .map(|(a, b)| (a.to_string(), b.to_string()))
        .into();
        let device = DeviceBehaviors::new([
            (10, "Key Press".into()),
            (11, "Momentary Layer".into()),
            (12, "Mod-Tap".into()),
            (13, "Bluetooth".into()),
        ]);
        Mapper {
            macros,
            names,
            device,
        }
    }

    #[test]
    fn compare_and_decode() {
        let doc = Document::parse(
            "/ { keymap { compatible = \"zmk,keymap\";\n\
             base { display-name = \"Base\"; bindings = <&kp A &kp LS(B) &mo NAV &kp NOPE>; };\n\
             nav { bindings = <&kp ESCAPE &kp B &kp A &kp A>; }; }; };",
        );
        let km = Keymap::from_document(&doc).unwrap();
        let m = mapper();
        let exp = expected(&km, &m);
        let kp = |p: u32| BehaviorBinding {
            behavior_id: 10,
            param1: p,
            param2: 0,
        };
        let mut device = DeviceKeymap {
            layers: vec![
                crate::proto::keymap::Layer {
                    id: 0,
                    name: "Base".into(),
                    bindings: vec![
                        kp(0x70004),
                        kp(0x0207_0005),
                        BehaviorBinding {
                            behavior_id: 11,
                            param1: 1,
                            param2: 0,
                        },
                        kp(4),
                    ],
                },
                crate::proto::keymap::Layer {
                    id: 1,
                    name: String::new(),
                    bindings: vec![kp(0x70029), kp(0x70005), kp(0x70004), kp(0x70004)],
                },
            ],
            available_layers: 0,
            max_layer_name_length: 20,
        };
        let tokens = vec!["0".to_string(), "NAV".to_string()];
        let c = compare(&device, &exp, &m, &tokens);
        assert!(c.in_sync(), "{c:?}");
        assert_eq!(c.unchecked.len(), 1, "&kp NOPE can't be resolved");

        // Somebody changed two keys and renamed a layer with ZMK Studio.
        device.layers[0].bindings[0] = kp(0x0107_0005); // LC(B)
        device.layers[1].bindings[1] = BehaviorBinding {
            behavior_id: 11,
            param1: 1,
            param2: 0,
        };
        device.layers[1].name = "Arrows".into();
        let c = compare(&device, &exp, &m, &tokens);
        assert!(!c.in_sync());
        assert_eq!(c.names, [(1, String::new(), "Arrows".into())]);
        let texts: Vec<_> = c
            .diffs
            .iter()
            .map(|d| (d.file_text.as_str(), d.device_text.as_str()))
            .collect();
        assert_eq!(texts, [("&kp A", "&kp LC(B)"), ("&kp B", "&mo NAV")]);

        // Structure mismatch = different firmware.
        device.layers.pop();
        assert!(compare(&device, &exp, &m, &tokens).structure.is_some());
    }

    #[test]
    fn decodes_mod_tap_and_unknown_behaviors() {
        let m = mapper();
        let mt = BehaviorBinding {
            behavior_id: 12,
            param1: 0x700e1,
            param2: 0x70029,
        };
        assert_eq!(m.decode(&mt, None, &[]), "&mt LEFT_SHIFT ESCAPE");
        assert_eq!(
            m.decode(&mt, Some("&mt LEFT_SHIFT ESCAPE"), &[]),
            "&mt LEFT_SHIFT ESCAPE"
        );
        let unknown = BehaviorBinding {
            behavior_id: 99,
            param1: 1,
            param2: 2,
        };
        assert_eq!(m.decode(&unknown, None, &[]), "“?” 1 2");
    }
}
