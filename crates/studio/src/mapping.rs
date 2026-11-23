//! Mapping between the keymap source and what the keyboard speaks: behaviors by
//! display name, parameters as numbers (via [`zmk_config::cpp`] and ZMK's headers from
//! the build workspace), layers by devicetree index.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use zmk_config::cpp::{tokenize, Macros, Tok};
use zmk_config::dts::{Document, Item, Node};

use crate::proto::keymap::BehaviorBinding;

/// A binding resolved to what the firmware contains.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Resolved {
    /// Behavior label without `&` (`kp`, `mo`, `hm`).
    pub label: String,
    pub params: Vec<u32>,
}

/// Where `#include`s are looked up: the config folder, ZMK's and Zephyr's include
/// folders, and every module's `include/` in the build workspace.
pub fn search_paths(workspace: &Path, config_dir: &Path) -> Vec<PathBuf> {
    let mut out = vec![
        config_dir.to_path_buf(),
        workspace.join("zmk/app/include"),
        workspace.join("zmk/app/dts"),
        workspace.join("zephyr/include"),
        workspace.join("zephyr/dts"),
    ];
    // Modules (zmk-helpers, zmk-unicode, …): any `<dir>/include` up to two levels down.
    let mut add = |d: PathBuf| {
        if d.join("include").is_dir() && !out.contains(&d.join("include")) {
            out.push(d.join("include"));
        }
    };
    if let Ok(rd) = std::fs::read_dir(workspace) {
        for e in rd.flatten() {
            let p = e.path();
            if !p.is_dir() || ["zmk", "zephyr", ".west"].contains(&e.file_name().to_string_lossy().as_ref()) {
                continue;
            }
            add(p.clone());
            if let Ok(sub) = std::fs::read_dir(&p) {
                for s in sub.flatten() {
                    if s.path().is_dir() {
                        add(s.path());
                        if let Ok(sub2) = std::fs::read_dir(s.path()) {
                            for s2 in sub2.flatten() {
                                if s2.path().is_dir() {
                                    add(s2.path());
                                }
                            }
                        }
                    }
                }
            }
        }
    }
    out
}

pub fn macros_for(keymap_text: &str, search: &[PathBuf]) -> Macros {
    let mut m = Macros::default();
    m.load_with_includes(keymap_text, search);
    m
}

/// Resolve a binding's source text (`&kp LS(A)`, `HRM(A, LGUI)`, `&bt BT_SEL 1`).
pub fn resolve(text: &str, macros: &Macros) -> Result<Resolved, String> {
    let toks = macros.expand(&tokenize(text));
    let (label, rest) = match toks.as_slice() {
        [Tok::Punct(a), Tok::Ident(l), rest @ ..] if a == "&" => (l.clone(), rest),
        _ => return Err(format!("`{text}` isn't a single behavior binding")),
    };
    // Another binding starts with a top-level `&label` (a `&` inside parentheses is
    // a bitwise AND, e.g. `MOVE_Y(v)` = `((v) & 0xFFFF)`).
    let mut depth = 0i32;
    for (i, t) in rest.iter().enumerate() {
        match t {
            Tok::Punct(p) if p == "(" => depth += 1,
            Tok::Punct(p) if p == ")" => depth -= 1,
            Tok::Punct(p) if p == "&" && depth == 0 && matches!(rest.get(i + 1), Some(Tok::Ident(_))) => {
                return Err(format!("`{text}` expands to more than one binding"));
            }
            _ => {}
        }
    }
    let rest_text = rest
        .iter()
        .map(|t| match t {
            Tok::Ident(s) | Tok::Num(s) | Tok::Punct(s) => s.as_str(),
        })
        .collect::<Vec<_>>()
        .join(" ");
    let params = macros.cells(&rest_text).map_err(|e| format!("`{text}`: {e}"))?;
    Ok(Resolved { label, params })
}

/// The name the firmware reports for a behavior node: `display-name`, else `label`,
/// else the node's full name (`DEVICE_DT_NAME`).
pub fn display_name(n: &Node) -> String {
    n.property("display-name")
        .and_then(|p| p.first_string())
        .or_else(|| n.property("label").and_then(|p| p.first_string()))
        .map(str::to_string)
        .unwrap_or_else(|| match &n.unit_address {
            Some(a) => format!("{}@{a}", n.name),
            None => n.name.clone(),
        })
}

fn add_behaviors(doc: &Document, out: &mut HashMap<String, String>) {
    for item in doc.walk() {
        if let Item::Node(n) = item {
            if n.compatible().is_some_and(|c| c.starts_with("zmk,behavior-")) {
                for l in &n.labels {
                    out.insert(l.name.clone(), display_name(n));
                }
            }
        }
    }
}

/// Behavior label → display name, from ZMK's (and modules') `dts/behaviors/*.dtsi` and
/// the keymap itself (which wins).
pub fn behavior_names(workspace: &Path, keymap: &Document) -> HashMap<String, String> {
    let mut out = HashMap::new();
    let mut dirs = vec![workspace.join("zmk/app/dts/behaviors")];
    if let Ok(rd) = std::fs::read_dir(workspace) {
        for e in rd.flatten() {
            for cand in [e.path().join("dts/behaviors"), e.path().join("app/dts/behaviors")] {
                if cand.is_dir() && !dirs.contains(&cand) {
                    dirs.push(cand);
                }
            }
            if let Ok(sub) = std::fs::read_dir(e.path()) {
                for s in sub.flatten() {
                    let cand = s.path().join("dts/behaviors");
                    if cand.is_dir() && !dirs.contains(&cand) {
                        dirs.push(cand);
                    }
                }
            }
        }
    }
    for d in dirs {
        if let Ok(rd) = std::fs::read_dir(&d) {
            for e in rd.flatten() {
                if e.path().extension().is_some_and(|x| x == "dtsi") {
                    if let Ok(t) = std::fs::read_to_string(e.path()) {
                        add_behaviors(&Document::parse(t), &mut out);
                    }
                }
            }
        }
    }
    add_behaviors(keymap, &mut out);
    out
}

/// The keyboard's behaviors: display name → id (ambiguous names are dropped).
#[derive(Debug, Clone, Default)]
pub struct DeviceBehaviors {
    by_name: HashMap<String, u32>,
    names: HashMap<u32, String>,
    pub ambiguous: Vec<String>,
}

impl DeviceBehaviors {
    pub fn new(list: impl IntoIterator<Item = (u32, String)>) -> Self {
        let mut d = DeviceBehaviors::default();
        for (id, name) in list {
            if d.by_name.contains_key(&name) {
                d.by_name.remove(&name);
                d.ambiguous.push(name.clone());
            } else if !d.ambiguous.contains(&name) {
                d.by_name.insert(name.clone(), id);
            }
            d.names.insert(id, name);
        }
        d
    }
    pub fn id(&self, display_name: &str) -> Option<u32> {
        self.by_name.get(display_name).copied()
    }
    pub fn name(&self, id: u32) -> Option<&str> {
        self.names.get(&id).map(String::as_str)
    }
}

/// Everything needed to translate between source and keyboard.
#[derive(Debug, Clone)]
pub struct Mapper {
    pub macros: Macros,
    /// label → display name
    pub names: HashMap<String, String>,
    pub device: DeviceBehaviors,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Unmappable {
    /// Couldn't work out the numbers (unknown macro …).
    Source(String),
    /// The keyboard has no behavior with this name (different firmware?).
    NotOnKeyboard(String),
}

impl std::fmt::Display for Unmappable {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Unmappable::Source(s) => write!(f, "{s}"),
            Unmappable::NotOnKeyboard(n) => {
                write!(f, "the keyboard has no “{n}” behavior — build and install first")
            }
        }
    }
}

impl Mapper {
    pub fn encode(&self, text: &str) -> Result<BehaviorBinding, Unmappable> {
        let r = resolve(text, &self.macros).map_err(Unmappable::Source)?;
        let name = self
            .names
            .get(&r.label)
            .cloned()
            .unwrap_or_else(|| r.label.clone());
        let id = self.device.id(&name).ok_or(Unmappable::NotOnKeyboard(name))?;
        if r.params.len() > 2 {
            return Err(Unmappable::Source(format!(
                "`{text}` has more than two parameters"
            )));
        }
        Ok(BehaviorBinding {
            behavior_id: id as i32,
            param1: r.params.first().copied().unwrap_or(0),
            param2: r.params.get(1).copied().unwrap_or(0),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_names_follow_the_firmware_rule() {
        let doc = Document::parse(
            r#"/ { behaviors {
                hm: homerow_mods { compatible = "zmk,behavior-hold-tap"; };
                ae: ae { compatible = "zmk,behavior-macro"; label = "AE"; };
                kp: key_press { compatible = "zmk,behavior-key-press"; display-name = "Key Press"; };
            }; };"#,
        );
        let mut names = HashMap::new();
        add_behaviors(&doc, &mut names);
        assert_eq!(names["hm"], "homerow_mods");
        assert_eq!(names["ae"], "AE");
        assert_eq!(names["kp"], "Key Press");
    }

    #[test]
    fn device_behaviors_drop_ambiguous_names() {
        let d = DeviceBehaviors::new([(1, "Key Press".into()), (2, "Twin".into()), (3, "Twin".into())]);
        assert_eq!(d.id("Key Press"), Some(1));
        assert_eq!(d.id("Twin"), None);
        assert_eq!(d.ambiguous, ["Twin"]);
        assert_eq!(d.name(3), Some("Twin"));
    }

    #[test]
    fn encodes_with_defines() {
        let mut macros = Macros::default();
        macros.add_source("#define A 0x70004\n#define LS(k) (0x02 << 24 | k)\n#define HRM(k, m) &hm m k\n#define LSHIFT 0x700e1\n", true);
        let names: HashMap<String, String> = [("kp", "Key Press"), ("hm", "homerow_mods")]
            .map(|(a, b)| (a.to_string(), b.to_string()))
            .into();
        let m = Mapper {
            macros,
            names,
            device: DeviceBehaviors::new([(5, "Key Press".into()), (9, "homerow_mods".into())]),
        };
        assert_eq!(
            m.encode("&kp LS(A)").unwrap(),
            BehaviorBinding {
                behavior_id: 5,
                param1: 0x0207_0004,
                param2: 0
            }
        );
        assert_eq!(
            m.encode("HRM(A, LSHIFT)").unwrap(),
            BehaviorBinding {
                behavior_id: 9,
                param1: 0x700e1,
                param2: 0x70004
            }
        );
        assert_eq!(m.encode("&mo 1"), Err(Unmappable::NotOnKeyboard("mo".into())));
        assert!(matches!(m.encode("&kp NOPE"), Err(Unmappable::Source(_))));
    }

    /// With a real build workspace and build output (set MXKEYS_TEST_WORKSPACE,
    /// MXKEYS_TEST_DTS, MXKEYS_TEST_KEYMAP): every binding of the keymap resolves to
    /// exactly what the compiler put into zephyr.dts.
    #[test]
    fn matches_a_real_build_when_available() {
        let (Ok(ws), Ok(dts), Ok(keymap)) = (
            std::env::var("MXKEYS_TEST_WORKSPACE"),
            std::env::var("MXKEYS_TEST_DTS"),
            std::env::var("MXKEYS_TEST_KEYMAP"),
        ) else {
            return;
        };
        let km_text = std::fs::read_to_string(&keymap).unwrap();
        let km_doc = Document::parse(km_text.clone());
        let km = zmk_config::keymap::Keymap::from_document(&km_doc).unwrap();
        let macros = macros_for(
            &km_text,
            &search_paths(Path::new(&ws), Path::new(&keymap).parent().unwrap()),
        );
        let built = Document::parse(std::fs::read_to_string(&dts).unwrap());
        let keymap_node = built
            .walk()
            .find_map(|i| match i {
                Item::Node(n) if n.compatible() == Some("zmk,keymap") => Some(n),
                _ => None,
            })
            .unwrap();
        let mut checked = 0;
        for layer in &km.layers {
            // zephyr.dts doesn't keep the file's layer order; match by node name.
            // (Node names are preprocessed too: the owner's `DEF` layer becomes `0`.)
            let built_name: String = macros
                .expand(&tokenize(&layer.node_name))
                .iter()
                .map(|t| match t {
                    Tok::Ident(x) | Tok::Num(x) | Tok::Punct(x) => x.clone(),
                })
                .collect();
            let node = keymap_node
                .child_nodes()
                .find(|n| n.name == built_name)
                .unwrap_or_else(|| {
                    panic!(
                        "{built_name:?} not in {:?}",
                        keymap_node.child_nodes().map(|n| &n.name).collect::<Vec<_>>()
                    )
                });
            let cells: Vec<String> = node
                .property("bindings")
                .unwrap()
                .cells()
                .map(|c| built.text(c.span).to_string())
                .collect();
            // Group the built cells into bindings (&label n n …).
            let mut built_bindings: Vec<(String, Vec<u32>)> = Vec::new();
            for c in cells {
                if let Some(l) = c.strip_prefix('&') {
                    built_bindings.push((l.to_string(), vec![]));
                } else {
                    let v = zmk_config::cpp::eval(&tokenize(&c)).unwrap() as u32;
                    built_bindings.last_mut().unwrap().1.push(v);
                }
            }
            let ours = layer.bindings.as_ref().unwrap();
            assert_eq!(ours.bindings.len(), built_bindings.len());
            for (b, (label, params)) in ours.bindings.iter().zip(&built_bindings) {
                let r = resolve(&b.raw, &macros).unwrap_or_else(|e| panic!("{}: {e}", b.raw));
                assert_eq!((&r.label, &r.params), (label, params), "{}", b.raw);
                checked += 1;
            }
        }
        assert!(checked > 100, "checked {checked}");
        // Display names: ours (from source + ZMK's dtsi) equal the built nodes'.
        let ours = behavior_names(Path::new(&ws), &km_doc);
        let mut built_names = HashMap::new();
        add_behaviors(&built, &mut built_names);
        let mut compared = 0;
        for (label, name) in &built_names {
            if let Some(o) = ours.get(label) {
                assert_eq!(o, name, "&{label}");
                compared += 1;
            }
        }
        assert!(compared > 20, "compared {compared}");
        eprintln!("compared {compared} behavior display names");
        eprintln!("checked {checked} bindings against the build");
    }
}
