//! "Changes & upload": git status with a plain-language summary of each changed file,
//! diffs, saving a version (commit), getting the latest (pull) and uploading (push).
//! Also remembers recently opened config folders.

use std::path::{Path, PathBuf};

use mxkeys_git::repo::{self, CommitInfo, FileChange, FileDiff, PullOutcome, RepoStatus};
use mxkeys_git::Backend;
use serde::{Deserialize, Serialize};
use zmk_config::changes;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChangedFile {
    #[serde(flatten)]
    pub change: FileChange,
    /// What changed, in plain words (keymaps and `.conf` files).
    pub summary: Vec<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct History {
    /// `false` when the folder has no git history (offer to start one).
    pub is_repo: bool,
    pub status: Option<RepoStatus>,
    pub files: Vec<ChangedFile>,
    /// Suggested commit message (title, blank line, bullet list).
    pub suggested_message: String,
    pub backend: Backend,
    pub recent: Vec<CommitInfo>,
}

fn kind(path: &str) -> &'static str {
    if path.ends_with(".keymap") {
        "keymap"
    } else if path.ends_with(".conf") {
        "conf"
    } else {
        "other"
    }
}

fn summarize(root: &Path, c: &FileChange) -> Vec<String> {
    let old = repo::file_at_head(root, c.old_path.as_deref().unwrap_or(&c.path))
        .ok()
        .flatten()
        .unwrap_or_default();
    let new = std::fs::read_to_string(root.join(&c.path)).unwrap_or_default();
    match (kind(&c.path), c.kind) {
        (_, repo::ChangeKind::Deleted) => vec![],
        ("keymap", _) => changes::keymap_changes(&old, &new),
        ("conf", _) => changes::conf_changes(&old, &new),
        _ => vec![],
    }
}

fn message(files: &[ChangedFile]) -> String {
    let pick = |k: &str| -> Vec<String> {
        files
            .iter()
            .filter(|f| kind(&f.change.path) == k && f.change.kind != repo::ChangeKind::Deleted)
            .flat_map(|f| f.summary.clone())
            .collect()
    };
    let (keymap, conf) = (pick("keymap"), pick("conf"));
    let art_dir = format!("{}/", mxkeys_niceview::module::DIR);
    let is_art = |f: &&ChangedFile| f.change.path.starts_with(&art_dir);
    let mut others: Vec<String> = Vec::new();
    if files.iter().any(|f| is_art(&f)) {
        let all_new = files
            .iter()
            .filter(is_art)
            .all(|f| f.change.kind == repo::ChangeKind::Added);
        let all_gone = files
            .iter()
            .filter(is_art)
            .all(|f| f.change.kind == repo::ChangeKind::Deleted);
        others.push(
            if all_new {
                "Add nice!view pictures"
            } else if all_gone {
                "Remove nice!view pictures"
            } else {
                "Update nice!view pictures"
            }
            .to_string(),
        );
    }
    others.extend(
        files
            .iter()
            .filter(|f| !is_art(f) && f.change.path != mxkeys_niceview::module::MODULE_YML)
            .filter(|f| kind(&f.change.path) == "other" || f.change.kind == repo::ChangeKind::Deleted)
            .map(|f| {
                let verb = match f.change.kind {
                    repo::ChangeKind::Added => "Add",
                    repo::ChangeKind::Deleted => "Remove",
                    repo::ChangeKind::Renamed => "Rename",
                    _ => "Update",
                };
                format!("{verb} {}", f.change.path)
            }),
    );
    let title = changes::title_for(&keymap, &conf, &others);
    let bullets: Vec<String> = keymap.iter().chain(&conf).chain(&others).cloned().collect();
    if bullets.len() <= 1 && bullets.first().is_none_or(|b| *b == title) {
        title
    } else {
        format!(
            "{title}\n\n{}",
            bullets
                .iter()
                .map(|b| format!("- {b}"))
                .collect::<Vec<_>>()
                .join("\n")
        )
    }
}

pub fn history(dir: &Path) -> Result<History, String> {
    let backend = mxkeys_git::detect_backend();
    if !repo::is_repository(dir) {
        return Ok(History {
            is_repo: false,
            status: None,
            files: vec![],
            suggested_message: String::new(),
            backend,
            recent: vec![],
        });
    }
    let status = repo::status(dir).map_err(|e| e.to_string())?;
    let files: Vec<ChangedFile> = status
        .files
        .iter()
        .map(|c| ChangedFile {
            summary: summarize(&status.root, c),
            change: c.clone(),
        })
        .collect();
    Ok(History {
        is_repo: true,
        suggested_message: message(&files),
        recent: repo::log(dir, 5).unwrap_or_default(),
        status: Some(status),
        files,
        backend,
    })
}

pub fn diff(dir: &Path, file: Option<&str>) -> Result<Vec<FileDiff>, String> {
    repo::diff(dir, file).map_err(|e| e.to_string())
}

/// What to do (all run in the background; they may touch the network).
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum GitOp {
    Commit {
        message: String,
        files: Option<Vec<String>>,
    },
    Push,
    Pull,
    Fetch,
    Discard {
        files: Vec<String>,
    },
    SetIdentity {
        name: String,
        email: String,
    },
    Init,
}

#[derive(Debug, Serialize)]
#[serde(tag = "result", rename_all = "snake_case")]
pub enum GitOpResult {
    Committed { commit: CommitInfo },
    Pushed,
    Pulled { outcome: PullOutcome },
    Fetched,
    Done,
}

pub fn run(dir: &Path, op: &GitOp) -> Result<GitOpResult, String> {
    let backend = mxkeys_git::detect_backend();
    let s = |e: mxkeys_git::GitError| e.to_string();
    Ok(match op {
        GitOp::Commit { message, files } => GitOpResult::Committed {
            commit: repo::commit(dir, message, files.as_deref(), &backend).map_err(s)?,
        },
        GitOp::Push => {
            repo::push(dir, &backend).map_err(s)?;
            GitOpResult::Pushed
        }
        GitOp::Pull => GitOpResult::Pulled {
            outcome: repo::pull(dir, &backend).map_err(s)?,
        },
        GitOp::Fetch => {
            repo::fetch(dir, &backend).map_err(s)?;
            GitOpResult::Fetched
        }
        GitOp::Discard { files } => {
            repo::discard(dir, files).map_err(s)?;
            GitOpResult::Done
        }
        GitOp::SetIdentity { name, email } => {
            repo::set_identity(dir, name, email).map_err(s)?;
            GitOpResult::Done
        }
        GitOp::Init => {
            repo::init(dir).map_err(s)?;
            GitOpResult::Done
        }
    })
}

// ---- recently opened folders (app data, never the repo) ----

const MAX_RECENT: usize = 8;

fn recent_file(data: &Path) -> PathBuf {
    data.join("recent.json")
}

pub fn recent(data: &Path) -> Vec<String> {
    std::fs::read_to_string(recent_file(data))
        .ok()
        .and_then(|t| serde_json::from_str::<Vec<String>>(&t).ok())
        .unwrap_or_default()
        .into_iter()
        .filter(|p| Path::new(p).is_dir())
        .collect()
}

pub fn remember(data: &Path, folder: &Path) {
    let folder = folder.display().to_string();
    let mut list = recent(data);
    list.retain(|p| *p != folder);
    list.insert(0, folder);
    list.truncate(MAX_RECENT);
    let _ = std::fs::create_dir_all(data);
    if let Ok(text) = serde_json::to_string_pretty(&list) {
        let _ = std::fs::write(recent_file(data), text);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn summary_and_message_for_a_config_repo() {
        let fixtures = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../crates/zmk-config/tests/fixtures");
        let dir = std::env::temp_dir().join(format!("mxkeys-history-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("config")).unwrap();
        std::fs::copy(
            fixtures.join("xnmrph_corne.keymap"),
            dir.join("config/corne.keymap"),
        )
        .unwrap();
        std::fs::copy(fixtures.join("xnmrph_corne.conf"), dir.join("config/corne.conf")).unwrap();

        let h = history(&dir).unwrap();
        assert!(!h.is_repo);
        run(&dir, &GitOp::Init).unwrap();
        run(
            &dir,
            &GitOp::SetIdentity {
                name: "T".into(),
                email: "t@example.com".into(),
            },
        )
        .unwrap();
        run(
            &dir,
            &GitOp::Commit {
                message: "Start".into(),
                files: None,
            },
        )
        .unwrap();

        let conf = std::fs::read_to_string(dir.join("config/corne.conf")).unwrap();
        std::fs::write(
            dir.join("config/corne.conf"),
            conf.replace("CONFIG_ZMK_DISPLAY=n", "CONFIG_ZMK_DISPLAY=y"),
        )
        .unwrap();
        let h = history(&dir).unwrap();
        assert_eq!(h.files.len(), 1);
        assert_eq!(h.files[0].summary, ["Screen: off → on"]);
        assert_eq!(h.suggested_message, "Screen: off → on");

        std::fs::write(dir.join("build.yaml"), "include: []\n").unwrap();
        let h = history(&dir).unwrap();
        assert_eq!(
            h.suggested_message,
            "Update settings and other files\n\n- Screen: off → on\n- Add build.yaml"
        );
        let d = diff(&dir, Some("config/corne.conf")).unwrap();
        assert_eq!((d[0].added, d[0].removed), (1, 1));
        assert_eq!(h.recent[0].summary, "Start");

        // nice!view pictures (and the module.yml line) are one entry.
        std::fs::create_dir_all(dir.join("nice_view_art/src")).unwrap();
        std::fs::write(dir.join("nice_view_art/src/frames.c"), "x").unwrap();
        std::fs::write(dir.join("nice_view_art/mxkeys.json"), "{}").unwrap();
        std::fs::create_dir_all(dir.join("zephyr")).unwrap();
        std::fs::write(dir.join("zephyr/module.yml"), "build:\n").unwrap();
        let h = history(&dir).unwrap();
        assert_eq!(
            h.suggested_message,
            "Update settings and other files\n\n- Screen: off → on\n- Add nice!view pictures\n- Add build.yaml"
        );

        let data = dir.join("appdata");
        remember(&data, &dir);
        remember(&data, &dir.join("config"));
        remember(&data, &dir);
        assert_eq!(
            recent(&data),
            [
                dir.display().to_string(),
                dir.join("config").display().to_string()
            ]
        );
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
