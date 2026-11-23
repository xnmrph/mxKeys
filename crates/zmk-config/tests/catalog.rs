//! The editor catalog must understand everything in the owner's real keymap.

use std::path::PathBuf;

use zmk_config::catalog::{builtin_behaviors, custom_behaviors, keycode, layer_refs, ParamKind};
use zmk_config::dts::Document;
use zmk_config::keymap::Keymap;

fn fixture(name: &str) -> String {
    std::fs::read_to_string(
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures")
            .join(name),
    )
    .unwrap()
}

/// Strip modifier functions: `LC(LS(TAB))` → `TAB`.
fn base_key(p: &str) -> &str {
    let mut s = p;
    while let Some(i) = s.find('(') {
        s = &s[i + 1..];
    }
    s.trim_end_matches(')')
}

#[test]
fn owners_keymap_is_fully_understood() {
    for name in ["xnmrph_corne.keymap", "xnmrph_corne_macos.keymap"] {
        let doc = Document::parse(fixture(name));
        let km = Keymap::from_document(&doc).unwrap();
        let mut known: Vec<String> = builtin_behaviors().into_iter().map(|b| b.reference).collect();
        let custom = custom_behaviors(&doc, &km);
        known.extend(custom.iter().map(|b| b.reference.clone()));
        for l in &km.layers {
            for b in &l.bindings.as_ref().unwrap().bindings {
                assert!(
                    known.contains(&b.behavior),
                    "{name}: unknown behavior {} on layer {}",
                    b.behavior,
                    l.title()
                );
                if b.behavior == "&kp" {
                    let k = base_key(&b.params[0]);
                    assert!(keycode(k).is_some(), "{name}: unknown key {k} ({})", b.raw);
                }
            }
        }
        // Hold-tap parameters are typed from its `bindings = <&kp>, <&kp>`.
        let hm = custom
            .iter()
            .find(|b| b.name.to_lowercase().contains("homerow"))
            .expect("homerow mods");
        assert_eq!(hm.params.len(), 2);
        assert!(matches!(hm.params[0].kind, ParamKind::Modifier));
        assert!(matches!(hm.params[1].kind, ParamKind::Keycode));

        let refs = layer_refs(&doc, &km);
        assert_eq!(refs[0].define.as_deref(), Some("DEF"), "{name}");
        assert_eq!(refs[1].define.as_deref(), Some("NAV"));
        assert_eq!(refs[1].title, "NavNum");
    }
}
