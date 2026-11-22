//! Round-trip and editing tests on real-world-style keymaps.

use std::path::PathBuf;

use zmk_config::dts::{Document, Item, Patch, TokenKind};
use zmk_config::keymap::{self, DefinitionKind, Keymap, KeymapError};

fn fixture(name: &str) -> String {
    let p = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name);
    std::fs::read_to_string(&p).unwrap_or_else(|e| panic!("{}: {e}", p.display()))
}

const ALL: &[&str] = &[
    "corne_stock.keymap",
    "advanced.keymap",
    "helper_macros.keymap",
    "malformed.keymap",
    "xnmrph_corne.keymap",
    "xnmrph_corne_macos.keymap",
];

/// Every byte must belong to exactly one token, and rebuilding from tokens is identical.
fn assert_lossless(src: &str) {
    let doc = Document::parse(src);
    assert_eq!(doc.to_source(), src);
    let mut pos = 0;
    for t in doc.tokens() {
        assert_eq!(t.span.start, pos);
        pos = t.span.end;
    }
    assert_eq!(pos, src.len());
    // Items are in order, non-overlapping, and never cover only trivia.
    let mut last_end = 0;
    for item in &doc.items {
        let s = item.span();
        assert!(s.start >= last_end, "overlapping items at {s:?}");
        last_end = s.end;
    }
}

#[test]
fn all_fixtures_round_trip() {
    for name in ALL {
        let src = fixture(name);
        assert_lossless(&src);
        // Also with CRLF line endings and with a BOM-less trailing-newline variation.
        assert_lossless(&src.replace('\n', "\r\n"));
        assert_lossless(src.trim_end());
    }
}

#[test]
fn well_formed_fixtures_have_no_diagnostics() {
    for name in [
        "corne_stock.keymap",
        "advanced.keymap",
        "helper_macros.keymap",
        "xnmrph_corne.keymap",
        "xnmrph_corne_macos.keymap",
    ] {
        let doc = Document::parse(fixture(name));
        assert!(doc.diagnostics.is_empty(), "{name}: {:?}", doc.diagnostics);
        assert!(!doc.walk().any(|i| matches!(i, Item::Unknown(_))), "{name}");
    }
}

#[test]
fn stock_corne_keymap_model() {
    let doc = Document::parse(fixture("corne_stock.keymap"));
    let km = Keymap::from_document(&doc).unwrap();
    assert_eq!(km.layers.len(), 3);
    let names: Vec<_> = km.layers.iter().map(|l| l.title()).collect();
    assert_eq!(names, ["Default Layer", "Lower Layer", "Raise Layer"]);
    for l in &km.layers {
        let b = l.bindings.as_ref().unwrap();
        assert!(b.opaque_reason.is_none());
        assert_eq!(b.bindings.len(), 42, "layer {}", l.node_name);
    }
    let base = &km.layers[0].bindings.as_ref().unwrap().bindings;
    assert_eq!(base[0].behavior, "&kp");
    assert_eq!(base[0].params, ["TAB"]);
    assert_eq!(base[37].raw, "&mo 1");
    let lower = &km.layers[1].bindings.as_ref().unwrap().bindings;
    assert_eq!(lower[13].raw, "&bt BT_SEL 0");
    assert_eq!(lower[13].params, ["BT_SEL", "0"]);
    assert_eq!(
        km.includes,
        ["behaviors.dtsi", "dt-bindings/zmk/keys.h", "dt-bindings/zmk/bt.h"]
    );
}

#[test]
fn advanced_keymap_model() {
    let doc = Document::parse(fixture("advanced.keymap"));
    let km = Keymap::from_document(&doc).unwrap();
    let titles: Vec<_> = km.layers.iter().map(|l| l.title()).collect();
    assert_eq!(titles, ["Base", "Nav", "Sym", "Adjust \"⚙\""]);
    for l in &km.layers {
        let b = l.bindings.as_ref().unwrap();
        assert_eq!(b.opaque_reason, None, "layer {}", l.node_name);
        assert_eq!(b.bindings.len(), 42, "layer {}", l.node_name);
    }

    let base = &km.layers[0].bindings.as_ref().unwrap().bindings;
    assert_eq!(base[13].raw, "HRM(A, LGUI)");
    assert_eq!(base[13].behavior, "&hm");
    assert_eq!(base[13].via_define.as_deref(), Some("HRM"));
    assert_eq!(base[37].raw, "&lt NAV SPACE");
    assert_eq!(base[38].params, ["LCTRL", "RET"]);
    let sensors = km.layers[0].sensor_bindings.as_ref().unwrap();
    assert_eq!(sensors.bindings[0].params, ["C_VOL_UP", "C_VOL_DN"]);

    let nav = &km.layers[1].bindings.as_ref().unwrap().bindings;
    assert_eq!(nav[0].behavior, "&trans");
    assert_eq!(nav[0].via_define.as_deref(), Some("___"));
    assert_eq!(nav[12].raw, "BT_STUFF");
    assert_eq!(nav[13].raw, "&bt BT_SEL 0");

    let sym = &km.layers[2].bindings.as_ref().unwrap().bindings;
    assert_eq!(sym[14].raw, "&kp LC(LS(TAB))");
    // The inline block comment ends up *between* bindings, not inside one.
    assert_eq!(sym[15].raw, "&kp LA(F4)");
    assert_eq!(sym[16].raw, "&email");

    let kinds = |k: DefinitionKind| km.definitions.iter().filter(|d| d.kind == k).count();
    assert_eq!(kinds(DefinitionKind::Behavior), 3);
    assert_eq!(kinds(DefinitionKind::Macro), 1);
    assert_eq!(kinds(DefinitionKind::BehaviorOverride), 2);
    let hm = km.definitions.iter().find(|d| d.labels == ["hm"]).unwrap();
    assert_eq!(hm.compatible.as_deref(), Some("zmk,behavior-hold-tap"));
    assert!(hm
        .properties
        .iter()
        .any(|(k, v)| k == "flavor" && v == "\"balanced\""));
    assert!(hm
        .properties
        .iter()
        .any(|(k, v)| k == "hold-trigger-on-release" && v.is_empty()));

    let combos = keymap::combos(&doc);
    assert_eq!(
        combos.iter().map(|c| c.node_name.as_str()).collect::<Vec<_>>(),
        ["combo_esc", "combo_tab"]
    );
    let cl = keymap::conditional_layers(&doc);
    assert_eq!(
        cl[0].properties,
        [
            ("if-layers".into(), "<NAV SYM>".into()),
            ("then-layer".into(), "<ADJ>".into())
        ]
    );

    // Preprocessor content is visible to the model.
    let defines: Vec<_> = doc.defines().map(|(n, _, _)| n).collect();
    assert!(defines.starts_with(&["DEF", "NAV", "SYM", "ADJ", "XXX", "___", "HRM", "BT_STUFF", "BT_STUFF"]));
}

#[test]
fn helper_macro_keymap_is_preserved_and_flagged() {
    let doc = Document::parse(fixture("helper_macros.keymap"));
    let macros: Vec<_> = doc
        .items
        .iter()
        .filter_map(|i| match i {
            Item::MacroCall(m) => Some((m.name.as_str(), m.args.first().cloned().unwrap_or_default())),
            _ => None,
        })
        .collect();
    assert_eq!(
        macros,
        [
            ("ZMK_HOLD_TAP", "hml".into()),
            ("ZMK_COMBO", "esc".into()),
            ("ZMK_COMBO", "tab".into()),
            ("ZMK_LAYER", "base".into())
        ]
    );
    let km = Keymap::from_document(&doc).unwrap();
    assert_eq!(
        km.definitions
            .iter()
            .filter(|d| d.kind == DefinitionKind::HelperMacro)
            .count(),
        4
    );
    let opaque = km.layers[0].bindings.as_ref().unwrap();
    assert_eq!(
        opaque.opaque_reason.as_deref(),
        Some("`MULTI` expands to 2 bindings")
    );
    let macro_valued = km.layers[1].bindings.as_ref().unwrap();
    assert_eq!(
        macro_valued.opaque_reason.as_deref(),
        Some("bindings are produced by a macro")
    );
    assert_eq!(
        km.set_binding(0, 0, "&kp X"),
        Err(KeymapError::Opaque {
            layer: 0,
            reason: "`MULTI` expands to 2 bindings".into()
        })
    );
}

#[test]
fn malformed_input_recovers() {
    let doc = Document::parse(fixture("malformed.keymap"));
    assert!(!doc.diagnostics.is_empty());
    let km = Keymap::from_document(&doc).unwrap();
    assert_eq!(km.layers.len(), 2);
    assert!(km.layers[0].bindings.is_none(), "broken property became Unknown");
    assert_eq!(km.layers[1].title(), "Fine");
    assert_eq!(km.layers[1].bindings.as_ref().unwrap().bindings[0].raw, "&kp C");
}

/// Returns (prefix, suffix) lengths that are byte-identical between a and b.
fn common_affixes(a: &str, b: &str) -> (usize, usize) {
    let p = a.bytes().zip(b.bytes()).take_while(|(x, y)| x == y).count();
    let s = a
        .bytes()
        .rev()
        .zip(b.bytes().rev())
        .take_while(|(x, y)| x == y)
        .count();
    (p, s)
}

#[test]
fn set_binding_is_a_minimal_edit() {
    let src = fixture("corne_stock.keymap");
    let doc = Document::parse(src.clone());
    let km = Keymap::from_document(&doc).unwrap();
    let patch = km.set_binding(0, 0, "&kp ESC").unwrap();
    let (doc2, km2) = keymap::apply(&doc, &patch).unwrap();
    let out = doc2.source();
    assert_eq!(out, src.replacen("&kp TAB   &kp Q", "&kp ESC   &kp Q", 1));
    let (p, s) = common_affixes(&src, out);
    assert!(p + s >= src.len() - "&kp TAB".len(), "only the binding changed");
    assert_eq!(
        km2.layers[0].bindings.as_ref().unwrap().bindings[0].raw,
        "&kp ESC"
    );
    // Everything else in the model is unchanged.
    for (a, b) in km.layers.iter().zip(&km2.layers).skip(1) {
        let ra: Vec<_> = a
            .bindings
            .as_ref()
            .unwrap()
            .bindings
            .iter()
            .map(|b| &b.raw)
            .collect();
        let rb: Vec<_> = b
            .bindings
            .as_ref()
            .unwrap()
            .bindings
            .iter()
            .map(|b| &b.raw)
            .collect();
        assert_eq!(ra, rb);
    }
}

#[test]
fn set_binding_in_crlf_and_macro_keymap() {
    let src = fixture("advanced.keymap").replace('\n', "\r\n");
    let doc = Document::parse(src.clone());
    let km = Keymap::from_document(&doc).unwrap();
    // Replace a macro-call binding and a define binding in one go.
    let p1 = km.set_binding(0, 13, "&hm LGUI A").unwrap();
    let p2 = km.set_binding(1, 0, "&kp GRAVE").unwrap();
    let doc2 = doc.apply(&[p1, p2]).unwrap();
    assert!(doc2.source().contains("&td_caps &hm LGUI A HRM(S, LALT)"));
    assert!(doc2.source().contains("&kp GRAVE &kp N1"));
    assert!(!doc2.source().contains("\r\r"));
    assert_eq!(doc2.source().matches("\r\n").count(), src.matches("\r\n").count());
    let km2 = Keymap::from_document(&doc2).unwrap();
    assert_eq!(km2.layers[0].bindings.as_ref().unwrap().bindings.len(), 42);
}

#[test]
fn set_binding_errors() {
    let doc = Document::parse(fixture("corne_stock.keymap"));
    let km = Keymap::from_document(&doc).unwrap();
    assert_eq!(km.set_binding(9, 0, "&kp A"), Err(KeymapError::NoSuchLayer(9)));
    assert_eq!(
        km.set_binding(0, 42, "&kp A"),
        Err(KeymapError::NoSuchKey { layer: 0, pos: 42 })
    );
    assert_eq!(
        km.set_binding(0, 0, "kp A"),
        Err(KeymapError::InvalidBinding("kp A".into()))
    );
}

#[test]
fn layer_display_name_replace_and_insert() {
    let src = fixture("advanced.keymap");
    let doc = Document::parse(src.clone());
    let km = Keymap::from_document(&doc).unwrap();

    // Replace existing, with escaping.
    let p = km.set_layer_display_name(&doc, 0, "Home \"row\"").unwrap();
    let (doc2, km2) = keymap::apply(&doc, &p).unwrap();
    assert_eq!(km2.layers[0].display_name.as_deref(), Some("Home \"row\""));
    assert!(doc2.source().contains(r#"display-name = "Home \"row\"";"#));

    // Insert where missing (sym_layer), matching indentation.
    let p = km.set_layer_display_name(&doc, 2, "Symbols").unwrap();
    let (doc3, km3) = keymap::apply(&doc, &p).unwrap();
    assert_eq!(km3.layers[2].display_name.as_deref(), Some("Symbols"));
    assert!(doc3
        .source()
        .contains("        sym_layer {\n            display-name = \"Symbols\";\n            bindings = <"));
    assert!(doc3.diagnostics.is_empty());
}

#[test]
fn overlapping_patches_are_rejected() {
    let doc = Document::parse("/ { a = <1>; };");
    let r = doc.apply(&[
        Patch::replace(zmk_config::dts::Span::new(4, 8), "x"),
        Patch::replace(zmk_config::dts::Span::new(6, 9), "y"),
    ]);
    assert!(r.is_err());
}

#[test]
fn trivia_kinds_cover_comments_and_directives() {
    let doc = Document::parse(fixture("advanced.keymap"));
    let count = |k: TokenKind| doc.tokens().iter().filter(|t| t.kind == k).count();
    // Header + `/* inline */`; `/* symbols */` belongs to its `#define` directive token.
    assert_eq!(count(TokenKind::BlockComment), 2);
    assert_eq!(count(TokenKind::LineComment), 1);
    // 4 includes, 7 defines, `#ifdef` + 2 defines + `#else` + `#endif`.
    assert_eq!(count(TokenKind::Directive), 4 + 7 + 5);
}

/// The project owner's real keymaps (`xnmrph/corne`, `xnmrph/corne-macOS-Optimised`).
#[test]
fn owners_real_keymaps() {
    for (name, titles, macros) in [
        (
            "xnmrph_corne.keymap",
            &["Base", "NavNum", "Symbols", "System", "Extra", "NOR"][..],
            13,
        ),
        (
            "xnmrph_corne_macos.keymap",
            &["Base", "NavNum", "Symbols", "System", "Extra"][..],
            7,
        ),
    ] {
        let src = fixture(name);
        let doc = Document::parse(src.clone());
        let km = Keymap::from_document(&doc).unwrap();
        assert_eq!(
            km.layers.iter().map(|l| l.title()).collect::<Vec<_>>(),
            titles,
            "{name}"
        );
        for l in &km.layers {
            let b = l.bindings.as_ref().unwrap();
            assert_eq!(b.opaque_reason, None, "{name} {}", l.node_name);
            assert_eq!(b.bindings.len(), 42, "{name} {}", l.node_name);
        }
        let count = |k: DefinitionKind| km.definitions.iter().filter(|d| d.kind == k).count();
        assert_eq!(count(DefinitionKind::Behavior), 5, "{name}");
        assert_eq!(count(DefinitionKind::Macro), macros, "{name}");
        assert_eq!(keymap::combos(&doc).len(), 19, "{name}");

        // Layer nodes are named after `#define`d layer numbers (`DEF`, `NAV`, …).
        assert_eq!(km.layers[0].node_name, "DEF");
        let base = &km.layers[0].bindings.as_ref().unwrap().bindings;
        assert!(base[0].raw.starts_with("&mt TAB ESC"), "{name}");

        // A one-key edit changes exactly those bytes.
        let patch = km.set_binding(0, 1, "&kp ESC").unwrap();
        let (doc2, km2) = keymap::apply(&doc, &patch).unwrap();
        assert_eq!(
            doc2.source().len(),
            src.len() + "&kp ESC".len() - base[1].raw.len()
        );
        assert_eq!(&doc2.source()[..base[1].span.start], &src[..base[1].span.start]);
        assert_eq!(&doc2.source()[base[1].span.start + 7..], &src[base[1].span.end..]);
        assert_eq!(
            km2.layers[0].bindings.as_ref().unwrap().bindings[1].raw,
            "&kp ESC"
        );
    }
}
