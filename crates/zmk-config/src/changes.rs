//! Plain-language summaries of what changed between two versions of a keymap or
//! `.conf` file — shown before saving a version and used for the suggested commit
//! message (“System: key 11 &none → &out OUT_TOG”, “Screen: off → on”).

use std::collections::BTreeSet;

use crate::behaviors::{self, Definition, PropValue};
use crate::dts::Document;
use crate::kconfig::KconfigFile;
use crate::keymap::Keymap;
use crate::settings::{self, Kind, Value};

/// More individual key changes than this on one layer are summarised as a count.
const MAX_KEYS_PER_LAYER: usize = 4;

fn norm(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn parse(src: &str) -> Option<(Document, Keymap)> {
    let doc = Document::parse(src);
    let km = Keymap::from_document(&doc).ok()?;
    Some((doc, km))
}

fn kind_name(d: &Definition) -> &'static str {
    match d.def_type {
        behaviors::DefType::HoldTap => "hold-tap",
        behaviors::DefType::ModMorph => "shift-dependent key",
        behaviors::DefType::TapDance => "tap dance",
        behaviors::DefType::StickyKey => "sticky key",
        behaviors::DefType::CapsWord => "caps word",
        behaviors::DefType::Macro => "macro",
        behaviors::DefType::Combo => "combo",
        behaviors::DefType::ConditionalLayer => "conditional layer",
        behaviors::DefType::Other => "behavior",
    }
}

fn def_name(d: &Definition) -> String {
    match d.source {
        behaviors::Source::Override => format!("built-in &{}", d.node_name),
        _ => format!("{} “{}”", kind_name(d), d.node_name),
    }
}

fn show(v: &Option<PropValue>) -> String {
    match v {
        None => "default".into(),
        Some(PropValue::Int(i)) => i.to_string(),
        Some(PropValue::Flag(b)) => if *b { "on" } else { "off" }.into(),
        Some(PropValue::Text(t)) => t.clone(),
        Some(PropValue::Bindings(b)) => b.join(", "),
        Some(PropValue::Numbers(n)) => n.iter().map(i64::to_string).collect::<Vec<_>>().join(" "),
        Some(PropValue::Layers(n)) => n.iter().map(usize::to_string).collect::<Vec<_>>().join(" "),
        Some(PropValue::Names(n)) => n.join(" "),
        Some(PropValue::Raw(r)) => r.clone(),
    }
}

/// What changed between two keymap texts (`old` empty for a new file).
pub fn keymap_changes(old: &str, new: &str) -> Vec<String> {
    if old.is_empty() {
        return vec!["New keymap file".into()];
    }
    let (Some((od, ok)), Some((nd, nk))) = (parse(old), parse(new)) else {
        return if norm(old) == norm(new) {
            vec![]
        } else {
            vec!["Keymap edited".into()]
        };
    };
    let mut out = Vec::new();

    // Layers, matched by node name.
    for l in &nk.layers {
        match ok.layers.iter().find(|o| o.node_name == l.node_name) {
            None => out.push(format!("New layer “{}”", l.title())),
            Some(o) => {
                if o.title() != l.title() {
                    out.push(format!("Layer “{}” renamed to “{}”", o.title(), l.title()));
                }
                let (Some(ob), Some(nb)) = (&o.bindings, &l.bindings) else {
                    continue;
                };
                let changed: Vec<(usize, &str, &str)> = ob
                    .bindings
                    .iter()
                    .zip(&nb.bindings)
                    .enumerate()
                    .filter(|(_, (a, b))| norm(&a.raw) != norm(&b.raw))
                    .map(|(i, (a, b))| (i, a.raw.as_str(), b.raw.as_str()))
                    .collect();
                if changed.len() > MAX_KEYS_PER_LAYER {
                    out.push(format!("{}: {} keys changed", l.title(), changed.len()));
                } else {
                    for (i, a, b) in changed {
                        out.push(format!("{}: key {} {} → {}", l.title(), i + 1, norm(a), norm(b)));
                    }
                }
                if ob.bindings.len() != nb.bindings.len() {
                    out.push(format!(
                        "{}: {} keys instead of {}",
                        l.title(),
                        nb.bindings.len(),
                        ob.bindings.len()
                    ));
                }
            }
        }
    }
    for o in &ok.layers {
        if !nk.layers.iter().any(|l| l.node_name == o.node_name) {
            out.push(format!("Layer “{}” removed", o.title()));
        }
    }
    let order = |k: &Keymap, keep: &BTreeSet<&str>| -> Vec<String> {
        k.layers
            .iter()
            .filter(|l| keep.contains(l.node_name.as_str()))
            .map(|l| l.node_name.clone())
            .collect()
    };
    let common: BTreeSet<&str> = ok
        .layers
        .iter()
        .map(|l| l.node_name.as_str())
        .filter(|n| nk.layers.iter().any(|l| l.node_name == *n))
        .collect();
    if order(&ok, &common) != order(&nk, &common) {
        out.push("Layers reordered".into());
    }

    // Definitions (combos, macros, hold-taps …), matched by id.
    let odefs = behaviors::list(&od, Some(&ok));
    let ndefs = behaviors::list(&nd, Some(&nk));
    for d in &ndefs {
        match odefs.iter().find(|o| o.id == d.id) {
            None => {
                let extra = match d.def_type {
                    behaviors::DefType::Combo => {
                        let p = |n: &str| d.props.iter().find(|p| p.name == n).map(|p| show(&p.value));
                        format!(
                            " (keys {} → {})",
                            p("key-positions").unwrap_or_default(),
                            p("bindings").unwrap_or_default()
                        )
                    }
                    _ => String::new(),
                };
                out.push(format!("Add {}{extra}", def_name(d)));
            }
            Some(o) => {
                let mut diffs = Vec::new();
                for p in &d.props {
                    let before = o
                        .props
                        .iter()
                        .find(|q| q.name == p.name)
                        .and_then(|q| q.value.clone());
                    if before != p.value {
                        diffs.push(format!("{} {} → {}", p.name, show(&before), show(&p.value)));
                    }
                }
                if o.other_props != d.other_props {
                    diffs.push("other settings".into());
                }
                if !diffs.is_empty() {
                    let name = def_name(d);
                    let mut c = name[..1].to_uppercase();
                    c.push_str(&name[1..]);
                    out.push(format!("{c}: {}", diffs.join("; ")));
                }
            }
        }
    }
    for o in &odefs {
        if !ndefs.iter().any(|d| d.id == o.id) {
            out.push(format!("Remove {}", def_name(o)));
        }
    }
    if out.is_empty() && norm(old) != norm(new) {
        out.push("Keymap formatting or comments changed".into());
    }
    out
}

fn show_setting(def: &settings::SettingDef, v: &Value) -> String {
    match (&def.kind, v) {
        (_, Value::Bool(b)) => if *b { "on" } else { "off" }.into(),
        (Kind::Number { unit, .. }, Value::Int(i)) => settings::show_unit(*unit, *i),
        (_, Value::Int(i)) => i.to_string(),
        (_, Value::Text(t)) => format!("“{t}”"),
        (Kind::Choice { options, .. } | Kind::Pick { options, .. }, Value::Choice(i)) => options
            .get(*i)
            .map(|o| o.label.split(" — ").next().unwrap_or(o.label).to_string())
            .unwrap_or_default(),
        (_, Value::Choice(i)) => i.to_string(),
    }
}

/// What changed between two `.conf` texts.
pub fn conf_changes(old: &str, new: &str) -> Vec<String> {
    let (of, nf) = (KconfigFile::parse(old), KconfigFile::parse(new));
    let (ov, nv) = (settings::view(&of), settings::view(&nf));
    let mut out = Vec::new();
    for (o, n) in ov.iter().zip(&nv) {
        if o.value != n.value {
            out.push(format!(
                "{}: {} → {}",
                n.def.label,
                show_setting(&n.def, &o.value),
                show_setting(&n.def, &n.value)
            ));
        } else if o.is_default != n.is_default {
            out.push(format!(
                "{}: {}",
                n.def.label,
                if n.is_default {
                    "back to the default"
                } else {
                    "set explicitly"
                }
            ));
        }
    }
    let known = settings::known_options();
    let names: BTreeSet<&str> = of
        .settings
        .iter()
        .chain(&nf.settings)
        .map(|s| s.name.as_str())
        .filter(|n| !known.contains(n))
        .collect();
    for name in names {
        let (a, b) = (
            of.get(name).map(|v| v.to_conf()),
            nf.get(name).map(|v| v.to_conf()),
        );
        match (a, b) {
            (a, b) if a == b => {}
            (None, Some(b)) => out.push(format!("{name}={b}")),
            (Some(_), None) => out.push(format!("{name} removed")),
            (Some(a), Some(b)) => out.push(format!("{name}: {a} → {b}")),
            (None, None) => {}
        }
    }
    if out.is_empty() && norm(old) != norm(new) {
        out.push("Comments or formatting changed".into());
    }
    out
}

/// A one-line commit title for a list of changes in a file kind.
pub fn title_for(keymap: &[String], conf: &[String], other_files: &[String]) -> String {
    let all: Vec<&String> = keymap.iter().chain(conf).collect();
    if all.len() == 1 && other_files.is_empty() && all[0].chars().count() <= 72 {
        return all[0].clone();
    }
    let mut parts = Vec::new();
    if !keymap.is_empty() {
        parts.push("keymap");
    }
    if !conf.is_empty() {
        parts.push("settings");
    }
    if !other_files.is_empty() {
        parts.push("other files");
    }
    match parts.as_slice() {
        [] => "Update config".into(),
        [one] => format!("Update {one}"),
        [a, b] => format!("Update {a} and {b}"),
        [a, b, c] => format!("Update {a}, {b} and {c}"),
        _ => "Update config".into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const OWNER: &str = include_str!("../tests/fixtures/xnmrph_corne.keymap");
    const CONF: &str = include_str!("../tests/fixtures/xnmrph_corne.conf");

    #[test]
    fn keymap_key_and_combo_changes() {
        let doc = Document::parse(OWNER);
        let km = Keymap::from_document(&doc).unwrap();
        let p = km.set_binding_aligned(&doc, 3, 10, "&out OUT_TOG").unwrap();
        let new = doc.apply(&[p]).unwrap();
        let km2 = Keymap::from_document(&new).unwrap();
        let p = behaviors::add(
            &new,
            Some(&km2),
            behaviors::DefType::Combo,
            "undo",
            &[
                ("key-positions".into(), behaviors::NewProp::Numbers(vec![25, 26])),
                (
                    "bindings".into(),
                    behaviors::NewProp::Bindings(vec!["&kp LC(Z)".into()]),
                ),
            ],
        )
        .unwrap();
        let new = new.apply(&[p]).unwrap();
        let c = keymap_changes(OWNER, new.source());
        let old_key = km.layers[3].bindings.as_ref().unwrap().bindings[10].raw.clone();
        assert_eq!(
            c,
            [
                format!("System: key 11 {old_key} → &out OUT_TOG"),
                "Add combo “undo” (keys 25 26 → &kp LC(Z))".to_string()
            ]
        );
        assert_eq!(title_for(&c, &[], &[]), "Update keymap");
        assert_eq!(title_for(&c[..1], &[], &[]), c[0]);
        assert!(keymap_changes(OWNER, OWNER).is_empty());
        assert_eq!(
            keymap_changes(OWNER, &OWNER.replacen("tune to taste", "tune me", 1)),
            ["Keymap formatting or comments changed"]
        );
    }

    #[test]
    fn definition_and_layer_changes() {
        let new = OWNER
            .replacen("tapping-term-ms = <200>;", "tapping-term-ms = <230>;", 1)
            .replacen("display-name = \"System\"", "display-name = \"Sys\"", 1);
        let c = keymap_changes(OWNER, &new);
        assert!(
            c.contains(&"Layer “System” renamed to “Sys”".to_string()),
            "{c:?}"
        );
        assert!(
            c.contains(&"Hold-tap “homerow_mods”: tapping-term-ms 200 → 230".to_string()),
            "{c:?}"
        );
    }

    #[test]
    fn conf_changes_in_plain_words() {
        let new = CONF
            .replace("CONFIG_ZMK_DISPLAY=n", "CONFIG_ZMK_DISPLAY=y")
            .replace(
                "CONFIG_BT_CTLR_TX_PWR_PLUS_8=y",
                "# CONFIG_BT_CTLR_TX_PWR_PLUS_8=y",
            )
            + "CONFIG_ZMK_IDLE_SLEEP_TIMEOUT=1800000\nCONFIG_ZMK_USB_LOGGING=y\n";
        let c = conf_changes(CONF, &new);
        assert_eq!(
            c,
            [
                "Deep sleep after: 15 min → 30 min",
                "Signal strength: +8 dBm → 0 dBm",
                "Screen: off → on",
                "CONFIG_ZMK_USB_LOGGING=y",
            ]
        );
        assert_eq!(title_for(&[], &c[2..3], &[]), "Screen: off → on");
        assert_eq!(
            title_for(&["x".into()], &c, &["build.yaml".into()]),
            "Update keymap, settings and other files"
        );
    }
}
