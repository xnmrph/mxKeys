//! Tauri shell. Keep this thin: commands validate input, call into `crates/*`, and map
//! errors to plain-language strings for the UI.

pub mod building;
pub mod conf_settings;
pub mod detection;
pub mod editor;
pub mod flashing;
pub mod history;
pub mod profiles;
pub mod summary;

use std::path::PathBuf;

use mxkeys_hardware::detect::quiz::{self, QuizAnswers};
use mxkeys_hardware::HardwareProfile;
use tauri::{AppHandle, Emitter, Manager};

/// Open a zmk-config folder and summarise it.
#[tauri::command]
fn open_config_repo(app: AppHandle, path: String) -> Result<summary::RepoSummary, String> {
    let dir = PathBuf::from(path);
    let s = summary::load_repo(&dir).map_err(|e| e.to_string())?;
    if let Ok(data) = data_dir(&app) {
        history::remember(&data, &dir);
    }
    Ok(s)
}

/// Config folders opened before (newest first; missing folders are left out).
#[tauri::command]
fn recent_folders(app: AppHandle) -> Result<Vec<String>, String> {
    Ok(history::recent(&data_dir(&app)?))
}

/// Git status of the config folder with plain-language summaries per changed file.
#[tauri::command]
async fn git_history(repo_path: String) -> Result<history::History, String> {
    tauri::async_runtime::spawn_blocking(move || history::history(&PathBuf::from(repo_path)))
        .await
        .map_err(|e| e.to_string())?
}

/// Line-by-line changes since the last saved version (optionally one file).
#[tauri::command]
async fn git_diff(
    repo_path: String,
    file: Option<String>,
) -> Result<Vec<mxkeys_git::repo::FileDiff>, String> {
    tauri::async_runtime::spawn_blocking(move || history::diff(&PathBuf::from(repo_path), file.as_deref()))
        .await
        .map_err(|e| e.to_string())?
}

/// Save a version, upload, get latest, … (may use the network and credential helpers).
#[tauri::command]
async fn git_run(repo_path: String, op: history::GitOp) -> Result<history::GitOpResult, String> {
    tauri::async_runtime::spawn_blocking(move || history::run(&PathBuf::from(repo_path), &op))
        .await
        .map_err(|e| e.to_string())?
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
        if let Ok(data) = data_dir(&app) {
            history::remember(&data, &dest);
        }
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

/// Start the guided flashing flow for the built firmware (`flash-event`s follow).
#[tauri::command]
fn start_flash(
    app: AppHandle,
    flashing: tauri::State<'_, flashing::Flashing>,
    repo_path: String,
    reset: bool,
) -> Result<mxkeys_flash::session::View, String> {
    let data = data_dir(&app)?;
    let repo = PathBuf::from(&repo_path);
    let profile = profiles::load(&data, &repo).ok_or("Confirm your keyboard's hardware first.")?;
    let session = flashing::new_session(&flashing::firmware_for(&data, &repo), &profile, reset)?;
    let handle = app.clone();
    flashing.start(session, mxkeys_probe::FsDriveScanner::system(), move |e| {
        let _ = handle.emit("flash-event", e);
    })
}

/// Whether firmware for this config has been built (so it can be installed).
#[tauri::command]
fn firmware_ready(app: AppHandle, repo_path: String) -> Result<bool, String> {
    let fw = flashing::firmware_for(&data_dir(&app)?, &PathBuf::from(repo_path));
    Ok(fw.left.is_file() && fw.right.is_file())
}

/// `retry`, `skip` or `cancel` the current flashing step.
#[tauri::command]
fn flash_control(
    flashing: tauri::State<'_, flashing::Flashing>,
    command: String,
) -> Result<Option<mxkeys_flash::session::View>, String> {
    flashing.control(&command)
}

/// Keycodes, behaviors (incl. the keymap's own) and layers for the key editor.
#[tauri::command]
fn editor_catalog(keymap_path: String) -> Result<editor::EditorCatalog, String> {
    editor::catalog_for(&PathBuf::from(keymap_path))
}

/// Apply one edit (set a key, rename/add/remove/move a layer) to the keymap file.
#[tauri::command]
fn edit_keymap(
    editor: tauri::State<'_, editor::Editor>,
    keymap_path: String,
    op: editor::EditOp,
) -> Result<editor::EditResult, String> {
    editor.edit(&PathBuf::from(keymap_path), &op)
}

#[tauri::command]
fn undo_keymap(
    editor: tauri::State<'_, editor::Editor>,
    keymap_path: String,
) -> Result<editor::EditResult, String> {
    editor.undo(&PathBuf::from(keymap_path))
}

#[tauri::command]
fn redo_keymap(
    editor: tauri::State<'_, editor::Editor>,
    keymap_path: String,
) -> Result<editor::EditResult, String> {
    editor.redo(&PathBuf::from(keymap_path))
}

/// Content version of a file, to notice changes made outside the app.
#[tauri::command]
fn file_version(path: String) -> Result<String, String> {
    editor::version(&PathBuf::from(path))
}

/// The file changed outside the app: drop its undo history.
#[tauri::command]
fn forget_history(editor: tauri::State<'_, editor::Editor>, keymap_path: String) {
    editor.forget(&PathBuf::from(keymap_path));
}

/// Whether the built firmware is missing, current, or older than the config files.
#[tauri::command]
fn firmware_state(app: AppHandle, repo_path: String) -> Result<flashing::FirmwareState, String> {
    let repo = PathBuf::from(repo_path);
    Ok(flashing::firmware_state(
        &flashing::firmware_for(&data_dir(&app)?, &repo),
        &repo,
    ))
}

/// The settings page for one `.conf` file.
#[tauri::command]
fn settings_page(
    editor: tauri::State<'_, editor::Editor>,
    conf_path: String,
) -> Result<conf_settings::SettingsPage, String> {
    conf_settings::load(&PathBuf::from(conf_path), &editor)
}

/// Change one setting (or a raw Kconfig option) in the `.conf` file.
#[tauri::command]
fn edit_settings(
    editor: tauri::State<'_, editor::Editor>,
    conf_path: String,
    op: conf_settings::SettingsOp,
) -> Result<conf_settings::SettingsPage, String> {
    conf_settings::edit(&PathBuf::from(conf_path), &op, &editor)
}

#[tauri::command]
fn undo_settings(
    editor: tauri::State<'_, editor::Editor>,
    conf_path: String,
) -> Result<conf_settings::SettingsPage, String> {
    conf_settings::step(&PathBuf::from(conf_path), true, &editor)
}

#[tauri::command]
fn redo_settings(
    editor: tauri::State<'_, editor::Editor>,
    conf_path: String,
) -> Result<conf_settings::SettingsPage, String> {
    conf_settings::step(&PathBuf::from(conf_path), false, &editor)
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .manage(building::Jobs::default())
        .manage(flashing::Flashing::default())
        .manage(editor::Editor::default())
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
            start_flash,
            flash_control,
            firmware_ready,
            editor_catalog,
            edit_keymap,
            undo_keymap,
            redo_keymap,
            file_version,
            forget_history,
            firmware_state,
            settings_page,
            edit_settings,
            undo_settings,
            redo_settings,
            recent_folders,
            git_history,
            git_diff,
            git_run
        ])
        .run(tauri::generate_context!())
        .expect("error while running mxKeys");
}
