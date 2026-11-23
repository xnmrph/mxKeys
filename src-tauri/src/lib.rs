//! Tauri shell. Keep this thin: commands validate input, call into `crates/*`, and map
//! errors to plain-language strings for the UI.

pub mod building;
pub mod detection;
pub mod profiles;
pub mod summary;

use std::path::PathBuf;

use mxkeys_hardware::detect::quiz::{self, QuizAnswers};
use mxkeys_hardware::HardwareProfile;
use tauri::{AppHandle, Emitter, Manager};
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

/// Detect the hardware from the config repo, connected USB devices and mounted
/// bootloader drives, plus quiz answers if the user gave any.
#[tauri::command]
async fn detect_hardware(
    repo_path: String,
    quiz: Option<QuizAnswers>,
) -> Result<detection::DetectionReport, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let inputs = detection::repo_inputs(&PathBuf::from(repo_path));
        let snap = mxkeys_probe::snapshot(
            &mxkeys_probe::NusbEnumerator,
            &mxkeys_probe::FsDriveScanner::system(),
        );
        detection::report(&inputs, snap, quiz.as_ref())
    })
    .await
    .map_err(|e| e.to_string())
}

#[tauri::command]
fn quiz_questions() -> Vec<quiz::QuizQuestion> {
    quiz::questions()
}

/// Physical key positions for drawing the keyboard.
#[tauri::command]
fn physical_layout() -> Vec<mxkeys_hardware::catalog::KeyGeom> {
    mxkeys_hardware::catalog::corne_layout()
}

fn data_dir(app: &AppHandle) -> Result<PathBuf, String> {
    app.path().app_data_dir().map_err(|e| e.to_string())
}

#[tauri::command]
fn load_profile(app: AppHandle, repo_path: String) -> Result<Option<HardwareProfile>, String> {
    Ok(profiles::load(&data_dir(&app)?, &PathBuf::from(repo_path)))
}

#[tauri::command]
fn save_profile(app: AppHandle, repo_path: String, profile: HardwareProfile) -> Result<(), String> {
    profiles::save(&data_dir(&app)?, &PathBuf::from(repo_path), profile)
        .map_err(|e| format!("couldn't save your hardware choice: {e}"))
}

#[tauri::command]
fn toolchain_info(
    app: AppHandle,
    jobs: tauri::State<'_, building::Jobs>,
) -> Result<building::ToolchainInfo, String> {
    Ok(building::info(&data_dir(&app)?, &jobs))
}

/// Download/finish installing the build tools in the background (`toolchain-event`s).
#[tauri::command]
fn start_toolchain_setup(app: AppHandle, jobs: tauri::State<'_, building::Jobs>) -> Result<bool, String> {
    let data = data_dir(&app)?;
    let handle = app.clone();
    Ok(building::start_setup(data, &jobs, move |e| {
        let _ = handle.emit("toolchain-event", e);
    }))
}

/// Build firmware for the config repo in the background (`build-event`s).
#[tauri::command]
fn start_build(
    app: AppHandle,
    jobs: tauri::State<'_, building::Jobs>,
    repo_path: String,
    targets: Option<Vec<mxkeys_hardware::profile::TargetId>>,
    update: Option<bool>,
    pristine: Option<bool>,
) -> Result<bool, String> {
    let data = data_dir(&app)?;
    let repo = PathBuf::from(&repo_path);
    let profile = profiles::load(&data, &repo);
    let handle = app.clone();
    let req = building::BuildRequest {
        data,
        repo,
        profile,
        only: targets.unwrap_or_default(),
        update: update.unwrap_or(false),
        pristine: pristine.unwrap_or(false),
    };
    Ok(building::start_build(req, &jobs, move |e| {
        let _ = handle.emit("build-event", e);
    }))
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
        .manage(building::Jobs::default())
        .invoke_handler(tauri::generate_handler![
            open_config_repo,
            startup_path,
            git_backend,
            check_repo_url,
            clone_config_repo,
            detect_hardware,
            quiz_questions,
            physical_layout,
            load_profile,
            save_profile,
            toolchain_info,
            start_toolchain_setup,
            start_build,
            set_binding
        ])
        .run(tauri::generate_context!())
        .expect("error while running mxKeys");
}
