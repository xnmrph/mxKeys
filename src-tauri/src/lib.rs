//! Tauri shell. Keep this thin: commands validate input, call into `crates/*`, and map
//! errors to plain-language strings for the UI.

pub mod summary;

use std::path::PathBuf;

use zmk_config::keymap;
use zmk_config::repo::KeymapFile;

/// Open a zmk-config folder and summarise it.
#[tauri::command]
fn open_config_repo(path: String) -> Result<summary::RepoSummary, String> {
    summary::load_repo(&PathBuf::from(path)).map_err(|e| e.to_string())
}

/// Replace one key's binding and write the keymap file back (minimal, lossless edit).
#[tauri::command]
fn set_binding(
    keymap_path: String,
    layer: usize,
    position: usize,
    binding: String,
) -> Result<summary::KeymapSummary, String> {
    let path = PathBuf::from(keymap_path);
    let file = KeymapFile::load(&path).map_err(|e| e.to_string())?;
    let patch = file
        .keymap
        .set_binding(layer, position, &binding)
        .map_err(|e| e.to_string())?;
    let (document, keymap) = keymap::apply(&file.document, &patch).map_err(|e| e.to_string())?;
    std::fs::write(&path, document.source())
        .map_err(|e| format!("could not save {}: {e}", path.display()))?;
    Ok(summary::keymap_summary(&KeymapFile {
        path,
        document,
        keymap,
    }))
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .invoke_handler(tauri::generate_handler![open_config_repo, set_binding])
        .run(tauri::generate_context!())
        .expect("error while running mxKeys");
}
