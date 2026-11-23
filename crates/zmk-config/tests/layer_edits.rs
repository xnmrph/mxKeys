//! Structural edits on the owner's real keymap: alignment, add/remove/move layers.
//! Invariant: after any move/remove, every layer reference still points at the same
//! layer *by name*.

use std::collections::BTreeMap;
use std::path::PathBuf;

use zmk_config::dts::{Document, Item, Patch};
use zmk_config::keymap::{EditError, Keymap};

fn fixture(name: &str) -> String {
    std::fs::read_to_string(
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures")
            .join(name),
    )
    .unwrap()
}

fn apply(src: &str, patches: &[Patch]) -> (Document, Keymap) {
    let doc = Document::parse(src).apply(patches).unwrap();
    assert!(doc.diagnostics.is_empty(), "{:?}", doc.diagnostics);
    let km = Keymap::from_document(&doc).unwrap();
    (doc, km)
}

/// Every layer reference → the *title* of the layer it points at, keyed by where it is.
fn resolved_refs(doc: &Document, km: &Keymap) -> BTreeMap<String, String> {
    let defines: BTreeMap<String, usize> = doc
        .defines()
        .filter_map(|(n, p, b)| {
            p.is_none()
                .then(|| b.trim().parse().ok().map(|v| (n.to_string(), v)))
                .flatten()
        })
        .collect();
    let resolve = |t: &str| t.parse::<usize>().ok().or_else(|| defines.get(t).copied());
    let title = |i: usize| {
        km.layers
            .get(i)
            .map(|l| l.title())
            .unwrap_or_else(|| format!("<missing {i}>"))
    };
    let mut out = BTreeMap::new();
    for l in &km.layers {
        for (k, b) in l.bindings.as_ref().unwrap().bindings.iter().enumerate() {
            if ["&mo", "&lt", "&to", "&tog", "&sl"].contains(&b.behavior.as_str()) {
                out.insert(
                    format!("{} key {}", l.title(), k + 1),
                    title(resolve(&b.params[0]).unwrap()),
                );
            }
        }
    }
    for item in doc.walk() {
        if let Item::Node(n) = item {
            for p in ["layers", "if-layers", "then-layer"] {
                if let Some(prop) = n.property(p) {
                    let v: Vec<String> = prop
                        .cells()
                        .map(|c| title(resolve(doc.text(c.span)).unwrap()))
                        .collect();
                    out.insert(format!("{} {p}", n.name), v.join(","));
                }
            }
        }
    }
    out
}

#[test]
fn aligned_binding_edit_keeps_columns_and_touches_one_line() {
    let src = fixture("xnmrph_corne.keymap");
    let doc = Document::parse(src.clone());
    let km = Keymap::from_document(&doc).unwrap();
    // The acceptance-test edit: USB/Bluetooth toggle on the System layer, top-right key.
    let system = km.layers.iter().position(|l| l.title() == "System").unwrap();
    assert_eq!(
        km.layers[system].bindings.as_ref().unwrap().bindings[11].raw,
        "&trans"
    );
    let p = km.set_binding_aligned(&doc, system, 10, "&out OUT_TOG").unwrap();
    let (doc2, km2) = apply(&src, &[p]);
    let b = &km2.layers[system].bindings.as_ref().unwrap().bindings;
    assert_eq!(b[10].raw, "&out OUT_TOG");
    assert_eq!(b.len(), 42);
    let (old_lines, new_lines): (Vec<_>, Vec<_>) = (src.lines().collect(), doc2.source().lines().collect());
    assert_eq!(old_lines.len(), new_lines.len());
    let changed: Vec<usize> = (0..old_lines.len())
        .filter(|&i| old_lines[i] != new_lines[i])
        .collect();
    assert_eq!(changed.len(), 1, "only the edited line changes");
    let line = new_lines[changed[0]];
    // The key after it (`&trans`, last on the row) — the row is short, so just check content.
    assert!(
        line.contains("&out OUT_TOG &trans") || line.contains("&out OUT_TOG  &trans"),
        "{line}"
    );

    // Shorter replacement in the middle of a row: the next column keeps its position.
    let p = km.set_binding_aligned(&doc, 0, 1, "&kp X").unwrap();
    let (doc3, _) = apply(&src, &[p]);
    let old = src.lines().find(|l| l.starts_with("&mt TAB ESCAPE")).unwrap();
    let new = doc3
        .source()
        .lines()
        .find(|l| l.starts_with("&mt TAB ESCAPE"))
        .unwrap();
    assert_eq!(old.find("&kp W"), new.find("&kp W"), "column kept:\n{old}\n{new}");
}

#[test]
fn add_layer_copies_the_grid() {
    let src = fixture("xnmrph_corne.keymap");
    let doc = Document::parse(src.clone());
    let km = Keymap::from_document(&doc).unwrap();
    let (doc2, km2) = apply(&src, &[km.add_layer(&doc, "Gaming").unwrap()]);
    assert_eq!(km2.layers.len(), km.layers.len() + 1);
    let g = km2.layers.last().unwrap();
    assert_eq!(
        (g.title().as_str(), g.node_name.as_str()),
        ("Gaming", "gaming_layer")
    );
    let b = &g.bindings.as_ref().unwrap().bindings;
    assert_eq!(b.len(), 42);
    assert!(b.iter().all(|x| x.raw == "&trans"));
    assert!(
        doc2.source()
            .starts_with(&src[..km.layers.last().unwrap().node_span.end]),
        "earlier text untouched"
    );
    // Same name again → unique node name.
    let (_, km3) = apply(doc2.source(), &[km2.add_layer(&doc2, "Gaming").unwrap()]);
    assert_eq!(km3.layers.last().unwrap().node_name, "gaming_layer_2");
    assert!(km.add_layer(&doc, "   ").is_err());
}

#[test]
fn moving_layers_keeps_every_reference_pointing_at_the_same_layer() {
    let src = fixture("xnmrph_corne.keymap");
    let doc = Document::parse(src.clone());
    let km = Keymap::from_document(&doc).unwrap();
    let before = resolved_refs(&doc, &km);
    assert!(before.len() > 15, "{before:?}");
    let n = km.layers.len();
    for (from, to) in [(4, 1), (1, 4), (0, n - 1), (n - 1, 0), (2, 3)] {
        let (doc2, km2) = apply(&src, &km.move_layer(&doc, from, to).unwrap());
        let titles: Vec<String> = km2.layers.iter().map(|l| l.title()).collect();
        let mut expected: Vec<String> = km.layers.iter().map(|l| l.title()).collect();
        let t = expected.remove(from);
        expected.insert(to, t);
        assert_eq!(titles, expected, "move {from}->{to}");
        // References are keyed by "<layer title> key N", so moving keeps keys stable.
        assert_eq!(resolved_refs(&doc2, &km2), before, "move {from}->{to}");
    }
}

#[test]
fn removing_layers() {
    let src = fixture("xnmrph_corne.keymap");
    let doc = Document::parse(src.clone());
    let km = Keymap::from_document(&doc).unwrap();
    // NavNum (1) is used by `&mo 1` on Base.
    match km.remove_layer(&doc, 1) {
        Err(EditError::LayerInUse { layer, by }) => {
            assert_eq!(layer, "NavNum");
            assert!(by.contains("Base"), "{by}");
        }
        other => panic!("{other:?}"),
    }
    // NOR (5) is the conditional layer's target → in use.
    assert!(matches!(
        km.remove_layer(&doc, 5),
        Err(EditError::LayerInUse { .. })
    ));
    // Add a layer, then remove it again → byte-identical to the original.
    let (doc2, km2) = apply(&src, &[km.add_layer(&doc, "Temp").unwrap()]);
    let (doc3, _) = apply(
        doc2.source(),
        &km2.remove_layer(&doc2, km2.layers.len() - 1).unwrap(),
    );
    assert_eq!(doc3.source(), src);
}

#[test]
fn removing_a_layer_renumbers_later_references() {
    // advanced.keymap: DEF 0, NAV 1, SYM 2, ADJ 3 via #defines; tri-layer NAV+SYM→ADJ.
    let src = fixture("advanced.keymap");
    let doc = Document::parse(src.clone());
    let km = Keymap::from_document(&doc).unwrap();
    // Insert an unused layer at the front by adding + moving, then remove it again.
    let (d1, k1) = apply(&src, &[km.add_layer(&doc, "Scratch").unwrap()]);
    let last = k1.layers.len() - 1;
    let (d2, k2) = apply(d1.source(), &k1.move_layer(&d1, last, 0).unwrap());
    assert_eq!(k2.layers[0].title(), "Scratch");
    assert!(d2.source().contains("#define NAV 2"), "defines renumbered");
    let before = resolved_refs(&d2, &k2);
    let (d3, k3) = apply(d2.source(), &k2.remove_layer(&d2, 0).unwrap());
    assert_eq!(resolved_refs(&d3, &k3), before);
    assert!(
        d3.source().contains("#define NAV 1   // navigation"),
        "comment kept"
    );
}
