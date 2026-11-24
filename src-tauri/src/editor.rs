//! Keymap editing for the UI: one command for every edit, undo/redo history per file
//! (this session), and a content version so the UI can notice changes made outside
//! the app (text editor, `git pull`).

use std::collections::hash_map::DefaultHasher;
use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use serde::{Deserialize, Serialize};
use zmk_config::behaviors::{self, Schema};
use zmk_config::catalog::{self, Behavior, Keycode, LayerRef};
use zmk_config::dts::{Document, Patch};
use zmk_config::keymap::Keymap;
use zmk_config::repo::{self, KeymapFile};

use crate::summary::{self, KeymapSummary};

const MAX_HISTORY: usize = 200;

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum EditOp {
    SetBinding {
        layer: usize,
        position: usize,
        binding: String,
    },
    RenameLayer {
        layer: usize,
        name: String,
    },
    AddLayer {
        name: String,
    },
    RemoveLayer {
        layer: usize,
    },
    MoveLayer {
        from: usize,
        to: usize,
    },
    /// Set (or remove) one property of a hold-tap, macro, combo, … (by definition id).
    SetDefProp {
        id: String,
        prop: String,
        value: behaviors::NewProp,
    },
    /// Add a new definition.
    AddDef {
        def_type: behaviors::DefType,
        name: String,
        #[serde(default)]
        props: Vec<(String, behaviors::NewProp)>,
    },
    RemoveDef {
        id: String,
    },
    /// Add `&mt { };` (etc.) to tune a built-in behavior.
    TuneBuiltin {
        target: String,
    },
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EditResult {
    pub keymap: KeymapSummary,
    pub version: String,
    pub can_undo: bool,
    pub can_redo: bool,
    /// Set by live editing when the key went into the files only, because the
    /// keyboard doesn't have this behavior yet (it's new and needs an install).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub not_live: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EditorCatalog {
    pub keycodes: &'static [Keycode],
    pub keycodes_source: &'static str,
    pub behaviors: Vec<Behavior>,
    /// Steps only offered when editing a macro.
    pub macro_controls: Vec<Behavior>,
    pub layers: Vec<LayerRef>,
    /// Form descriptions for hold-taps, macros, combos, …
    pub schemas: Vec<Schema>,
}

#[derive(Default)]
struct History {
    undo: Vec<String>,
    redo: Vec<String>,
}

#[derive(Default)]
pub struct Editor {
    history: Mutex<HashMap<PathBuf, History>>,
}

/// Short content hash (changes whenever the file's text changes).
pub fn version_of(text: &str) -> String {
    let mut h = DefaultHasher::new();
    text.hash(&mut h);
    format!("{:016x}", h.finish())
}

pub fn version(path: &Path) -> Result<String, String> {
    repo::read(path)
        .map(|t| version_of(&t))
        .map_err(|e| e.to_string())
}

pub fn catalog_for(path: &Path) -> Result<EditorCatalog, String> {
    let file = KeymapFile::load(path).map_err(|e| e.to_string())?;
    let mut behaviors = catalog::builtin_behaviors();
    behaviors.extend(catalog::custom_behaviors(&file.document, &file.keymap));
    Ok(EditorCatalog {
        keycodes: catalog::keycodes(),
        keycodes_source: catalog::keycodes_source(),
        behaviors,
        macro_controls: catalog::macro_controls(),
        layers: catalog::layer_refs(&file.document, &file.keymap),
        schemas: [
            behaviors::DefType::HoldTap,
            behaviors::DefType::ModMorph,
            behaviors::DefType::TapDance,
            behaviors::DefType::StickyKey,
            behaviors::DefType::CapsWord,
            behaviors::DefType::Macro,
            behaviors::DefType::Combo,
            behaviors::DefType::ConditionalLayer,
            behaviors::DefType::Other,
        ]
        .map(behaviors::schema)
        .to_vec(),
    })
}

fn patches_for(doc: &Document, km: &Keymap, op: &EditOp) -> Result<Vec<Patch>, String> {
    let s = |e: &dyn std::fmt::Display| e.to_string();
    Ok(match op {
        EditOp::SetBinding {
            layer,
            position,
            binding,
        } => {
            vec![km
                .set_binding_aligned(doc, *layer, *position, binding)
                .map_err(|e| s(&e))?]
        }
        EditOp::RenameLayer { layer, name } => {
            if name.trim().is_empty() {
                return Err("Please give the layer a name.".into());
            }
            vec![km
                .set_layer_display_name(doc, *layer, name.trim())
                .map_err(|e| s(&e))?]
        }
        EditOp::AddLayer { name } => vec![km.add_layer(doc, name).map_err(|e| s(&e))?],
        EditOp::RemoveLayer { layer } => km.remove_layer(doc, *layer).map_err(|e| s(&e))?,
        EditOp::MoveLayer { from, to } => km.move_layer(doc, *from, *to).map_err(|e| s(&e))?,
        EditOp::SetDefProp { id, prop, value } => {
            behaviors::set_prop(doc, Some(km), id, prop, value).map_err(|e| s(&e))?
        }
        EditOp::AddDef {
            def_type,
            name,
            props,
        } => vec![behaviors::add(doc, Some(km), *def_type, name.trim(), props).map_err(|e| s(&e))?],
        EditOp::RemoveDef { id } => vec![behaviors::remove(doc, id).map_err(|e| s(&e))?],
        EditOp::TuneBuiltin { target } => behaviors::tune_builtin(doc, target.trim_start_matches('&'))
            .map_err(|e| s(&e))?
            .1
            .into_iter()
            .collect(),
    })
}

pub fn write(path: &Path, text: &str) -> Result<(), String> {
    std::fs::write(path, text).map_err(|e| format!("couldn't save {}: {e}", path.display()))
}

fn result(path: &Path, text: String, h: &History) -> Result<EditResult, String> {
    let document = Document::parse(text.clone());
    let keymap = Keymap::from_document(&document).map_err(|e| e.to_string())?;
    Ok(EditResult {
        keymap: summary::keymap_summary(&KeymapFile {
            path: path.to_path_buf(),
            document,
            keymap,
        }),
        version: version_of(&text),
        can_undo: !h.undo.is_empty(),
        can_redo: !h.redo.is_empty(),
        not_live: None,
    })
}

impl Editor {
    /// Apply an edit to the file as it is on disk *now* (so outside edits are respected).
    pub fn edit(&self, path: &Path, op: &EditOp) -> Result<EditResult, String> {
        let before = repo::read(path).map_err(|e| e.to_string())?;
        let doc = Document::parse(before.clone());
        let km = Keymap::from_document(&doc).map_err(|e| e.to_string())?;
        let patches = patches_for(&doc, &km, op)?;
        let after = doc.apply(&patches).map_err(|e| e.to_string())?;
        // Never write something we can't read back.
        if !after.diagnostics.is_empty() || Keymap::from_document(&after).is_err() {
            return Err(
                "That change would break the keymap file, so it wasn't saved. Please report this.".into(),
            );
        }
        let text = after.source().to_string();
        write(path, &text)?;
        self.record(path, before);
        let all = self.history.lock().unwrap();
        result(path, text, &all[path])
    }

    /// Apply several edits as one change (one undo step); nothing is written unless all
    /// of them apply.
    pub fn edit_many(&self, path: &Path, ops: &[EditOp]) -> Result<EditResult, String> {
        let before = repo::read(path).map_err(|e| e.to_string())?;
        let mut text = before.clone();
        for (i, op) in ops.iter().enumerate() {
            let doc = Document::parse(text);
            let km = Keymap::from_document(&doc).map_err(|e| e.to_string())?;
            let patches = patches_for(&doc, &km, op).map_err(|e| format!("step {}: {e}", i + 1))?;
            let after = doc.apply(&patches).map_err(|e| e.to_string())?;
            if !after.diagnostics.is_empty() || Keymap::from_document(&after).is_err() {
                return Err(
                    "That change would break the keymap file, so it wasn't saved. Please report this.".into(),
                );
            }
            text = after.source().to_string();
        }
        write(path, &text)?;
        self.record(path, before);
        let all = self.history.lock().unwrap();
        result(path, text, &all[path])
    }

    pub fn undo(&self, path: &Path) -> Result<EditResult, String> {
        self.step(path, true)
    }

    pub fn redo(&self, path: &Path) -> Result<EditResult, String> {
        self.step(path, false)
    }

    fn step(&self, path: &Path, undo: bool) -> Result<EditResult, String> {
        let text = self.step_text(path, undo)?;
        let all = self.history.lock().unwrap();
        result(path, text, &all[path])
    }

    /// Remember `before` as an undo step for `path` (after the new text was written).
    pub fn record(&self, path: &Path, before: String) {
        let mut all = self.history.lock().unwrap();
        let h = all.entry(path.to_path_buf()).or_default();
        h.undo.push(before);
        if h.undo.len() > MAX_HISTORY {
            h.undo.remove(0);
        }
        h.redo.clear();
    }

    /// `(can_undo, can_redo)` for `path`.
    pub fn flags(&self, path: &Path) -> (bool, bool) {
        let all = self.history.lock().unwrap();
        all.get(path)
            .map(|h| (!h.undo.is_empty(), !h.redo.is_empty()))
            .unwrap_or((false, false))
    }

    /// Undo/redo one step of any text file in the history; returns the text written.
    pub fn step_text(&self, path: &Path, undo: bool) -> Result<String, String> {
        let current = repo::read(path).map_err(|e| e.to_string())?;
        let mut all = self.history.lock().unwrap();
        let h = all.entry(path.to_path_buf()).or_default();
        let (from, to) = if undo {
            (&mut h.undo, &mut h.redo)
        } else {
            (&mut h.redo, &mut h.undo)
        };
        let text = from.pop().ok_or(if undo {
            "Nothing to undo."
        } else {
            "Nothing to redo."
        })?;
        to.push(current);
        write(path, &text)?;
        Ok(text)
    }

    /// Forget history (e.g. after the file changed outside the app).
    pub fn forget(&self, path: &Path) {
        self.history.lock().unwrap().remove(path);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_keymap() -> PathBuf {
        let fixtures = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../crates/zmk-config/tests/fixtures");
        let dir = std::env::temp_dir().join(format!("mxkeys-editor-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("corne.keymap");
        std::fs::copy(fixtures.join("xnmrph_corne.keymap"), &p).unwrap();
        p
    }

    #[test]
    fn edit_undo_redo_round_trip() {
        let path = temp_keymap();
        let original = std::fs::read_to_string(&path).unwrap();
        let ed = Editor::default();
        let r = ed
            .edit(
                &path,
                &EditOp::SetBinding {
                    layer: 3,
                    position: 10,
                    binding: "&out OUT_TOG".into(),
                },
            )
            .unwrap();
        assert_eq!(r.keymap.layers[3].bindings[10], "&out OUT_TOG");
        assert!(r.can_undo && !r.can_redo);
        assert_eq!(r.version, version(&path).unwrap());
        let r = ed
            .edit(
                &path,
                &EditOp::AddLayer {
                    name: "Gaming".into(),
                },
            )
            .unwrap();
        assert_eq!(r.keymap.layers.len(), 7);
        ed.undo(&path).unwrap();
        let r = ed.undo(&path).unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), original);
        assert!(!r.can_undo && r.can_redo);
        let r = ed.redo(&path).unwrap();
        assert_eq!(r.keymap.layers[3].bindings[10], "&out OUT_TOG");
        assert!(ed.undo(&path).is_ok() && ed.undo(&path).is_err());

        let err = ed.edit(&path, &EditOp::RemoveLayer { layer: 1 }).unwrap_err();
        assert!(err.contains("still used"), "{err}");
        let cat = catalog_for(&path).unwrap();
        assert!(cat.behaviors.iter().any(|b| b.reference == "&out"));
        assert_eq!(cat.layers[3].title, "System");
        std::fs::remove_file(&path).unwrap();
    }

    #[test]
    fn definition_ops_from_ui_json() {
        let fixtures = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../crates/zmk-config/tests/fixtures");
        let path = std::env::temp_dir().join(format!("mxkeys-defs-{}.keymap", std::process::id()));
        std::fs::copy(fixtures.join("xnmrph_corne.keymap"), &path).unwrap();
        let original = std::fs::read_to_string(&path).unwrap();
        let ed = Editor::default();
        let op = |j: &str| serde_json::from_str::<EditOp>(j).unwrap();
        let r = ed
            .edit(
                &path,
                &op(r#"{"op":"add_def","def_type":"combo","name":"undo_c","props":[["key-positions",{"type":"numbers","value":[25,26]}],["bindings",{"type":"bindings","value":["&kp LC(Z)"]}]]}"#),
            )
            .unwrap();
        assert!(r.keymap.definitions.iter().any(|d| d.id == "combos/undo_c"));
        ed.edit(
            &path,
            &op(r#"{"op":"set_def_prop","id":"combos/undo_c","prop":"timeout-ms","value":{"type":"int","value":40}}"#),
        )
        .unwrap();
        ed.edit(&path, &op(r#"{"op":"tune_builtin","target":"&mt"}"#))
            .unwrap();
        let r = ed
            .edit(
                &path,
                &op(r#"{"op":"set_def_prop","id":"&mt","prop":"quick-tap-ms","value":{"type":"int","value":150}}"#),
            )
            .unwrap();
        assert!(r.keymap.definitions.iter().any(|d| d.id == "&mt"));
        let err = ed
            .edit(&path, &op(r#"{"op":"remove_def","id":"macros/browsertab"}"#))
            .unwrap_err();
        assert!(err.contains("used"), "{err}");
        for _ in 0..4 {
            ed.undo(&path).unwrap();
        }
        assert_eq!(std::fs::read_to_string(&path).unwrap(), original);
        let cat = catalog_for(&path).unwrap();
        assert!(cat.macro_controls.iter().any(|b| b.reference == "&macro_press"));
        assert_eq!(cat.schemas.len(), 9);
        std::fs::remove_file(&path).unwrap();
    }
}

#[cfg(test)]
mod batch_tests {
    use super::*;

    #[test]
    fn many_edits_are_one_undo_step() {
        let dir = std::env::temp_dir().join(format!("mxkeys-batch-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("a.keymap");
        let src = "/ {\n    keymap {\n        compatible = \"zmk,keymap\";\n        base {\n            bindings = <&kp A &kp B>;\n        };\n    };\n};\n";
        std::fs::write(&p, src).unwrap();
        let ed = Editor::default();
        let ops = vec![
            EditOp::AddDef {
                def_type: behaviors::DefType::ModMorph,
                name: "bspc_del".into(),
                props: vec![
                    (
                        "bindings".into(),
                        behaviors::NewProp::Bindings(vec!["&kp BSPC".into(), "&kp DEL".into()]),
                    ),
                    (
                        "mods".into(),
                        behaviors::NewProp::Names(vec!["MOD_LSFT".into(), "MOD_RSFT".into()]),
                    ),
                ],
            },
            EditOp::SetBinding {
                layer: 0,
                position: 1,
                binding: "&bspc_del".into(),
            },
        ];
        let r = ed.edit_many(&p, &ops).unwrap();
        let text = std::fs::read_to_string(&p).unwrap();
        assert!(
            text.contains("bspc_del: bspc_del") && text.contains("&kp A &bspc_del"),
            "{text}"
        );
        assert!(r.can_undo);
        ed.undo(&p).unwrap();
        assert_eq!(std::fs::read_to_string(&p).unwrap(), src);
        // A failing step leaves the file alone.
        let bad = vec![
            EditOp::SetBinding {
                layer: 0,
                position: 0,
                binding: "&kp C".into(),
            },
            EditOp::RemoveLayer { layer: 7 },
        ];
        assert!(ed.edit_many(&p, &bad).is_err());
        assert_eq!(std::fs::read_to_string(&p).unwrap(), src);
        std::fs::remove_dir_all(&dir).ok();
    }
}
