//! `.conf` and `west.yml` on the owner's real config repos.

use std::path::PathBuf;

use zmk_config::kconfig::{KconfigFile, KconfigValue, SettingState};
use zmk_config::west_yml::{RevisionKind, WestManifest};

fn fixture(name: &str) -> String {
    std::fs::read_to_string(
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures")
            .join(name),
    )
    .unwrap()
}

#[test]
fn owners_conf_files_round_trip_and_read() {
    for name in ["xnmrph_corne.conf", "xnmrph_corne_macos.conf"] {
        let src = fixture(name);
        let f = KconfigFile::parse(src.clone());
        assert_eq!(f.source(), src);
        assert_eq!(
            f.get("CONFIG_ZMK_SLEEP"),
            Some(&KconfigValue::Bool(true)),
            "{name}"
        );
        assert_eq!(
            f.get("CONFIG_ZMK_STUDIO"),
            Some(&KconfigValue::Bool(true)),
            "{name}"
        );
        assert_eq!(
            f.get("CONFIG_ZMK_POINTING"),
            Some(&KconfigValue::Bool(true)),
            "{name}"
        );
        assert!(f.duplicates().is_empty(), "{name}");
    }

    let corne = KconfigFile::parse(fixture("xnmrph_corne.conf"));
    assert_eq!(
        corne.get("CONFIG_ZMK_KEYBOARD_NAME"),
        Some(&KconfigValue::Str("Corne xnmrph".into()))
    );
    // Screen explicitly turned off even though build.yaml builds nice!view variants —
    // a disagreement Phase 2 detection must explain.
    assert_eq!(corne.get("CONFIG_ZMK_DISPLAY"), Some(&KconfigValue::Bool(false)));
    let underglow = corne
        .settings
        .iter()
        .find(|s| s.name == "CONFIG_ZMK_RGB_UNDERGLOW")
        .unwrap();
    assert_eq!(underglow.state, SettingState::CommentedOut);

    let macos = KconfigFile::parse(fixture("xnmrph_corne_macos.conf"));
    assert_eq!(
        macos.get("CONFIG_ZMK_DISPLAY"),
        None,
        "commented out → Kconfig default"
    );
    assert_eq!(
        macos.get("CONFIG_ZMK_KSCAN_DEBOUNCE_PRESS_MS"),
        Some(&KconfigValue::Int(1))
    );
    // `CONFIG_ZMK_POINTING=y   ` — trailing spaces are kept outside the value span.
    let p = macos
        .set("CONFIG_ZMK_POINTING", &KconfigValue::Bool(false))
        .unwrap();
    let out = macos.apply(&[p]).unwrap();
    assert!(out.source().contains("CONFIG_ZMK_POINTING=n   \n"));
}

#[test]
fn first_settings_page_edits_are_minimal() {
    let src = fixture("xnmrph_corne.conf");
    let f = KconfigFile::parse(src.clone());
    let patches = [
        f.set("CONFIG_ZMK_IDLE_TIMEOUT", &KconfigValue::Int(60000))
            .unwrap(),
        f.set("CONFIG_ZMK_IDLE_SLEEP_TIMEOUT", &KconfigValue::Int(1800000))
            .unwrap(),
    ];
    // Both append at the same offset; apply one after the other.
    let g = f.apply(&patches[..1]).unwrap();
    let g = g
        .apply(&[g
            .set("CONFIG_ZMK_IDLE_SLEEP_TIMEOUT", &KconfigValue::Int(1800000))
            .unwrap()])
        .unwrap();
    assert!(g.source().starts_with(&src));
    assert_eq!(
        &g.source()[src.len()..],
        "CONFIG_ZMK_IDLE_TIMEOUT=60000\nCONFIG_ZMK_IDLE_SLEEP_TIMEOUT=1800000\n"
    );
    // Re-enabling underglow uncomments the existing line instead of appending a duplicate.
    let h = f
        .apply(&[f
            .set("CONFIG_ZMK_RGB_UNDERGLOW", &KconfigValue::Bool(true))
            .unwrap()])
        .unwrap();
    assert_eq!(
        h.source(),
        src.replacen("#CONFIG_ZMK_RGB_UNDERGLOW=y", "CONFIG_ZMK_RGB_UNDERGLOW=y", 1)
    );
}

#[test]
fn owners_west_manifests() {
    let corne = WestManifest::parse(&fixture("xnmrph_corne_west.yml")).unwrap();
    let zmk = corne.zmk().unwrap();
    assert_eq!(zmk.url.as_deref(), Some("https://github.com/zmkfirmware/zmk"));
    assert_eq!(
        (zmk.revision.as_str(), zmk.revision_kind),
        ("main", RevisionKind::Branch)
    );
    assert!(zmk.imports);
    let extras: Vec<_> = corne
        .extra_modules()
        .iter()
        .map(|p| (p.name.as_str(), p.url.as_deref().unwrap()))
        .collect();
    assert_eq!(
        extras,
        [
            ("zmk-helpers", "https://github.com/urob/zmk-helpers"),
            ("zmk-unicode", "https://github.com/urob/zmk-unicode")
        ]
    );
    assert_eq!(corne.self_path.as_deref(), Some("config"));

    let macos = WestManifest::parse(&fixture("xnmrph_corne_macos_west.yml")).unwrap();
    let zmk = macos.zmk().unwrap();
    assert_eq!(
        (zmk.revision.as_str(), zmk.revision_kind),
        ("v0.3", RevisionKind::Version),
        "from defaults"
    );
    assert!(macos.extra_modules().is_empty());
}
