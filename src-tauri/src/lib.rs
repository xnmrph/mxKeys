//! Tauri shell. Keep this thin: commands validate input, call into `crates/*`, and map
//! errors to plain-language strings for the UI.

pub mod summary;

use std::path::PathBuf;

use tauri::{AppHandle, Emitter};
use zmk_config::keymap;
use zmk_config::repo::KeymapFile;

/// Open a zmk-config folder and summarise it.
#[tauri::command]
fn open_config_repo(path: String) -> Result<summary::RepoSummary, String> {
    summary::load_repo(&PathBuf::from(path)).map_err(|e| e.to_string())
}

/// A config folder passed on the command line (`mxkeys ~/zmk-config`), opened at startup.
#[tauri::command]
fn startup_path() -> Option<String> {
    std::env::args_os()
        .skip(1)
        .map(PathBuf::from)
        .find(|p| p.is_dir())
        .map(|p| p.display().to_string())
}

/// Which git the app will use (shown in the "Advanced" part of the clone screen).
#[tauri::command]
fn git_backend() -> mxkeys_git::Backend {
    mxkeys_git::detect_backend()
}

/// Check a pasted address and suggest the folder it would be cloned into.
#[tauri::command]
fn check_repo_url(url: String) -> Result<mxkeys_git::RepoUrl, String> {
    mxkeys_git::parse_repo_url(&url).map_err(|e| e.to_string())
}

/// Clone `url` into `<parent_dir>/<repo name>` and open it. Emits `clone-progress`
/// events (`CloneProgress`) while running.
#[tauri::command]
async fn clone_config_repo(
    app: AppHandle,
    url: String,
    parent_dir: String,
) -> Result<summary::RepoSummary, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let repo_url = mxkeys_git::parse_repo_url(&url).map_err(|e| e.to_string())?;
        let dest = PathBuf::from(parent_dir).join(&repo_url.dir_name);
        let backend = mxkeys_git::detect_backend();
        mxkeys_git::clone(&repo_url, &dest, &backend, |p| {
            let _ = app.emit("clone-progress", p);
        })
        .map_err(|e| e.to_string())?;
        summary::load_repo(&dest).map_err(|e| {
            format!(
                "Downloaded to {}, but it doesn't look like a ZMK config: {e}",
                dest.display()
            )
        })
    })
    .await
    .map_err(|e| e.to_string())?
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
        .invoke_handler(tauri::generate_handler![
            open_config_repo,
            startup_path,
            git_backend,
            check_repo_url,
            clone_config_repo,
            set_binding
        ])
        .run(tauri::generate_context!())
        .expect("error while running mxKeys");
}
