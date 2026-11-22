use std::path::PathBuf;

use zmk_config::build_yaml::BuildMatrix;
use zmk_config::repo::{load_build_matrix, KeymapFile, RepoFiles};

fn fixtures() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
}

#[test]
fn users_build_yaml() {
    let m =
        BuildMatrix::parse(&std::fs::read_to_string(fixtures().join("user_build.yaml")).unwrap()).unwrap();
    assert_eq!(m.targets.len(), 5);
    assert_eq!(m.boards().len(), 1);
    let b = &m.boards()[0];
    assert_eq!(
        (b.name.as_str(), b.revision.as_deref()),
        ("nice_nano", Some("2.0.0"))
    );
    assert_eq!(m.keyboard_shields(), ["corne_left", "corne_right"]);

    let left_view = &m.targets[1];
    assert_eq!(
        left_view.shields,
        ["corne_left", "nice_view_adapter", "nice_view"]
    );
    assert!(left_view.has_nice_view() && left_view.has_studio());
    assert_eq!(left_view.keyboard_shields().collect::<Vec<_>>(), ["corne_left"]);

    let right_view = &m.targets[2];
    assert!(right_view.has_nice_view() && !right_view.has_studio());
    assert!(!m.targets[3].has_nice_view());

    // The variants the app actually needs: nice!view on both halves + settings_reset.
    let needed: Vec<_> = m
        .targets
        .iter()
        .filter(|t| t.has_nice_view() || t.is_settings_reset())
        .map(|t| t.shields.join(" "))
        .collect();
    assert_eq!(
        needed,
        [
            "corne_left nice_view_adapter nice_view",
            "corne_right nice_view_adapter nice_view",
            "settings_reset"
        ]
    );
    assert!(m.settings_reset().is_some());
}

#[test]
fn discover_and_load_a_repo() {
    let dir = std::env::temp_dir().join(format!("mxkeys-test-{}", std::process::id()));
    let config = dir.join("config");
    std::fs::create_dir_all(&config).unwrap();
    std::fs::copy(fixtures().join("user_build.yaml"), dir.join("build.yaml")).unwrap();
    std::fs::copy(fixtures().join("corne_stock.keymap"), config.join("corne.keymap")).unwrap();
    std::fs::write(config.join("corne.conf"), "CONFIG_ZMK_SLEEP=y\n").unwrap();
    std::fs::write(config.join("west.yml"), "manifest: {}\n").unwrap();

    let files = RepoFiles::discover(&dir).unwrap();
    assert_eq!(files.keymaps, [config.join("corne.keymap")]);
    assert_eq!(files.confs, [config.join("corne.conf")]);
    assert!(files.west_yml.is_some());
    assert_eq!(load_build_matrix(&files).unwrap().unwrap().targets.len(), 5);
    let km = KeymapFile::load(&files.keymaps[0]).unwrap();
    assert_eq!(km.keymap.layers.len(), 3);

    assert!(RepoFiles::discover(&dir.join("config/corne.keymap")).is_err());
    let empty = dir.join("empty");
    std::fs::create_dir_all(&empty).unwrap();
    assert!(RepoFiles::discover(&empty).is_err());
    std::fs::remove_dir_all(&dir).unwrap();
}
