//! The settings page over one `.conf` file: catalog view, edits, undo/redo (sharing the
//! editor's per-file history) and raw "Advanced" edits of any Kconfig option.

use std::path::Path;

use serde::{Deserialize, Serialize};
use zmk_config::kconfig::{KconfigFile, KconfigValue};
use zmk_config::repo;
use zmk_config::settings::{self, Group, NewValue, SettingView};

use crate::editor::{self, Editor};
use crate::summary::{self, SettingSummary};

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum SettingsOp {
    /// A catalog setting by id.
    Set { id: String, value: NewValue },
    /// Any Kconfig option, value as written in the file (`y`, `42`, `"text"`).
    SetRaw { name: String, value: String },
    /// Comment out every line setting this option.
    RemoveRaw { name: String },
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GroupView {
    pub id: Group,
    pub title: &'static str,
    pub settings: Vec<SettingView>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SettingsPage {
    pub path: String,
    pub file_name: String,
    pub groups: Vec<GroupView>,
    /// Lines for options the catalog doesn't know (shown under Advanced).
    pub other: Vec<SettingSummary>,
    /// Every line of the file that sets something.
    pub all: Vec<SettingSummary>,
    pub duplicates: Vec<String>,
    pub version: String,
    pub can_undo: bool,
    pub can_redo: bool,
}

fn page(path: &Path, text: String, ed: &Editor) -> SettingsPage {
    let file = KconfigFile::parse(text.clone());
    let mut views = settings::view(&file);
    let groups = Group::ALL
        .iter()
        .map(|g| GroupView {
            id: *g,
            title: g.title(),
            settings: {
                let (mine, rest): (Vec<_>, Vec<_>) = views.drain(..).partition(|v| v.def.group == *g);
                views = rest;
                mine
            },
        })
        .collect();
    let conf = summary::conf_summary(&repo::ConfFile {
        path: path.to_path_buf(),
        file,
    });
    let known = settings::known_options();
    let (can_undo, can_redo) = ed.flags(path);
    SettingsPage {
        path: conf.path,
        file_name: conf.file_name,
        groups,
        other: conf
            .settings
            .iter()
            .filter(|s| !known.contains(&s.name.as_str()))
            .cloned()
            .collect(),
        all: conf.settings,
        duplicates: conf.duplicates,
        version: editor::version_of(&text),
        can_undo,
        can_redo,
    }
}

pub fn load(path: &Path, ed: &Editor) -> Result<SettingsPage, String> {
    let text = repo::read(path).map_err(|e| e.to_string())?;
    Ok(page(path, text, ed))
}

fn apply(file: &KconfigFile, op: &SettingsOp) -> Result<KconfigFile, String> {
    match op {
        SettingsOp::Set { id, value } => settings::set(file, id, value).map_err(|e| e.to_string()),
        SettingsOp::SetRaw { name, value } => {
            let name = name.trim();
            let name = if name.starts_with("CONFIG_") {
                name.to_string()
            } else {
                format!("CONFIG_{name}")
            };
            let value = value.trim();
            if value.is_empty() || value.contains(['\n', '\r']) {
                return Err("Please enter a value (for example y, n, a number or \"text\").".into());
            }
            let p = file
                .set(&name, &KconfigValue::parse(value))
                .map_err(|e| e.to_string())?;
            file.apply(&[p]).map_err(|e| e.to_string())
        }
        SettingsOp::RemoveRaw { name } => {
            // One line at a time, last first, until no line sets it.
            let mut f = file.clone();
            while let Some(p) = f.comment_out(name) {
                f = f.apply(&[p]).map_err(|e| e.to_string())?;
            }
            Ok(f)
        }
    }
}

pub fn edit(path: &Path, op: &SettingsOp, ed: &Editor) -> Result<SettingsPage, String> {
    let before = repo::read(path).map_err(|e| e.to_string())?;
    let after = apply(&KconfigFile::parse(before.clone()), op)?;
    let text = after.source().to_string();
    if text != before {
        editor::write(path, &text)?;
        ed.record(path, before);
    }
    Ok(page(path, text, ed))
}

pub fn step(path: &Path, undo: bool, ed: &Editor) -> Result<SettingsPage, String> {
    let text = ed.step_text(path, undo)?;
    Ok(page(path, text, ed))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn edit_undo_and_raw_options() {
        let fixtures =
            std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../crates/zmk-config/tests/fixtures");
        let dir = std::env::temp_dir().join(format!("mxkeys-settings-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("corne.conf");
        std::fs::copy(fixtures.join("xnmrph_corne.conf"), &path).unwrap();
        let original = std::fs::read_to_string(&path).unwrap();
        let ed = Editor::default();

        let p = load(&path, &ed).unwrap();
        assert_eq!(p.groups.len(), 6);
        assert!(p.other.is_empty(), "{:?}", p.other);
        assert!(!p.can_undo);

        let p = edit(
            &path,
            &SettingsOp::Set {
                id: "display".into(),
                value: NewValue::Bool(true),
            },
            &ed,
        )
        .unwrap();
        assert!(p.can_undo);
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            original.replace("CONFIG_ZMK_DISPLAY=n", "CONFIG_ZMK_DISPLAY=y")
        );

        let p = edit(
            &path,
            &SettingsOp::SetRaw {
                name: "ZMK_USB_LOGGING".into(),
                value: "y".into(),
            },
            &ed,
        )
        .unwrap();
        assert_eq!(p.other[0].name, "CONFIG_ZMK_USB_LOGGING");
        let p = edit(
            &path,
            &SettingsOp::RemoveRaw {
                name: "CONFIG_ZMK_USB_LOGGING".into(),
            },
            &ed,
        )
        .unwrap();
        assert_eq!(p.other[0].state, zmk_config::kconfig::SettingState::CommentedOut);
        assert!(edit(
            &path,
            &SettingsOp::SetRaw {
                name: "bad name".into(),
                value: "y".into()
            },
            &ed
        )
        .is_err());

        step(&path, true, &ed).unwrap();
        step(&path, true, &ed).unwrap();
        let p = step(&path, true, &ed).unwrap();
        assert!(!p.can_undo && p.can_redo);
        assert_eq!(std::fs::read_to_string(&path).unwrap(), original);
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
