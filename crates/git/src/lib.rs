//! Git for mxKeys. Two backends:
//!
//! - **System git** (preferred when installed): reuses the user's credential helpers,
//!   SSH keys/agent and config for any host — GitHub, Gitea, GitLab, self-hosted.
//! - **libgit2** (bundled): fallback so HTTPS clones work on machines without git
//!   (common on Windows). Credentials come from git's credential helpers if configured.
//!
//! The app never stores tokens.

pub mod repo;
mod url;

use std::io::{BufReader, Read};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use serde::Serialize;

pub use url::{parse_repo_url, RepoUrl, UrlError, UrlKind};

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Backend {
    SystemGit { path: PathBuf, version: String },
    Libgit2,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "stage", rename_all = "snake_case")]
pub enum CloneProgress {
    Connecting,
    Receiving { percent: u8 },
    Resolving { percent: u8 },
    CheckingOut { percent: u8 },
    Done,
}

#[derive(Debug, thiserror::Error)]
pub enum GitError {
    #[error(transparent)]
    Url(#[from] UrlError),
    #[error("The folder {0} already exists and isn't empty. Choose another place or remove it first.")]
    DestinationNotEmpty(PathBuf),
    #[error("The server asked for a login we don't have. Make sure you can clone this repository with git on this computer (or make the repository public), then try again.")]
    AuthenticationFailed,
    #[error(
        "Repository not found. Check the address — for private repositories you also need to be logged in."
    )]
    NotFound,
    #[error("Couldn't reach the server. Check your internet connection and the address.")]
    Network,
    #[error("SSH addresses (git@…) need git installed on this computer. Install git or use the https:// address instead.")]
    SshNeedsSystemGit,
    #[error("{0} isn't a git repository (no change history).")]
    NotARepository(PathBuf),
    #[error("The server has changes you don't have yet (for example edits made on GitHub). Get them first with “Get latest”, then upload again.")]
    Rejected,
    #[error("Your saved versions and the server's have both moved on. Install git so mxKeys can combine them, or combine them in a git tool.")]
    Diverged,
    #[error("Your changes and the server's changes touch the same lines, so they can't be combined automatically. Nothing was changed; combine them in a git tool (or undo your local version).")]
    Conflict,
    #[error("There's nothing new to save — no files changed since the last saved version.")]
    NothingToCommit,
    #[error("There's nothing to upload yet — save a version first.")]
    NothingToPush,
    #[error("Git needs your name and email for the version history. Enter them once and mxKeys will remember them for this folder.")]
    NoIdentity,
    #[error("You have unsaved changes. Save a version (or undo the changes) before getting the latest from the server.")]
    LocalChangesInTheWay,
    #[error("Your account isn't allowed to upload to this repository. If it belongs to someone else, fork it and use your copy.")]
    NoPushPermission,
    #[error("This folder isn't linked to a server (no git remote), so there's nowhere to upload to.")]
    NoRemote,
    #[error("git failed: {0}")]
    Other(String),
}

/// Pick the backend: system git if it is installed and usable, else libgit2.
pub fn detect_backend() -> Backend {
    find_system_git().unwrap_or(Backend::Libgit2)
}

fn find_system_git() -> Option<Backend> {
    // On macOS `/usr/bin/git` is a stub that pops an "install developer tools" dialog when
    // the Command Line Tools are missing. Only use it if they're installed.
    if cfg!(target_os = "macos") {
        let clt = Command::new("xcode-select")
            .arg("-p")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
        let has_clt = clt.is_ok_and(|s| s.success());
        let brew = ["/opt/homebrew/bin/git", "/usr/local/bin/git"]
            .into_iter()
            .find(|p| Path::new(p).exists());
        let path = match (brew, has_clt) {
            (Some(p), _) => PathBuf::from(p),
            (None, true) => PathBuf::from("git"),
            (None, false) => return None,
        };
        return git_version(&path).map(|version| Backend::SystemGit { path, version });
    }
    let path = PathBuf::from("git");
    git_version(&path).map(|version| Backend::SystemGit { path, version })
}

fn git_version(path: &Path) -> Option<String> {
    let out = Command::new(path)
        .arg("--version")
        .stdin(Stdio::null())
        .output()
        .ok()?;
    out.status
        .success()
        .then(|| String::from_utf8_lossy(&out.stdout).trim().to_string())
}

fn check_destination(dest: &Path) -> Result<(), GitError> {
    if dest.exists() {
        let non_empty = std::fs::read_dir(dest)
            .map(|mut d| d.next().is_some())
            .unwrap_or(true);
        if non_empty {
            return Err(GitError::DestinationNotEmpty(dest.to_path_buf()));
        }
    }
    Ok(())
}

/// Clone `url` into `dest` (which must not exist or be empty).
pub fn clone(
    url: &RepoUrl,
    dest: &Path,
    backend: &Backend,
    mut progress: impl FnMut(CloneProgress),
) -> Result<(), GitError> {
    check_destination(dest)?;
    progress(CloneProgress::Connecting);
    let result = match backend {
        Backend::SystemGit { path, .. } => clone_system(path, url, dest, &mut progress),
        Backend::Libgit2 if url.kind.needs_system_git() => Err(GitError::SshNeedsSystemGit),
        Backend::Libgit2 => clone_libgit2(url, dest, &mut progress),
    };
    if result.is_ok() {
        progress(CloneProgress::Done);
    }
    result
}

// ----- system git ------------------------------------------------------------------------

/// Parse a git progress line like `Receiving objects:  45% (9/20)`.
pub fn parse_progress_line(line: &str) -> Option<CloneProgress> {
    let line = line.trim().trim_start_matches("remote:").trim();
    let percent = || -> Option<u8> {
        let pct = line.find('%')?;
        let start = line[..pct]
            .rfind(|c: char| !c.is_ascii_digit())
            .map_or(0, |i| i + 1);
        line[start..pct].parse().ok()
    };
    if line.starts_with("Receiving objects") {
        Some(CloneProgress::Receiving { percent: percent()? })
    } else if line.starts_with("Resolving deltas") {
        Some(CloneProgress::Resolving { percent: percent()? })
    } else if line.starts_with("Updating files") {
        Some(CloneProgress::CheckingOut { percent: percent()? })
    } else {
        None
    }
}

/// Map git's stderr to a friendly error.
pub fn classify_git_error(stderr: &str) -> GitError {
    let s = stderr.to_ascii_lowercase();
    if s.contains("authentication failed")
        || s.contains("could not read username")
        || s.contains("permission denied (publickey")
        || s.contains("terminal prompts disabled")
    {
        GitError::AuthenticationFailed
    } else if s.contains("not found")
        || s.contains("does not exist")
        || s.contains("does not appear to be a git repository")
    {
        GitError::NotFound
    } else if s.contains("could not resolve host")
        || s.contains("failed to connect")
        || s.contains("connection timed out")
        || s.contains("network is unreachable")
    {
        GitError::Network
    } else {
        let last = stderr
            .lines()
            .rev()
            .find(|l| !l.trim().is_empty())
            .unwrap_or("unknown error");
        GitError::Other(last.trim().to_string())
    }
}

fn clone_system(
    git: &Path,
    url: &RepoUrl,
    dest: &Path,
    progress: &mut impl FnMut(CloneProgress),
) -> Result<(), GitError> {
    let mut child = Command::new(git)
        .args(["clone", "--progress", "--", &url.url])
        .arg(dest)
        // Never block on a terminal prompt (there is no terminal); GUI credential
        // helpers / askpass still work.
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("LC_ALL", "C")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| GitError::Other(e.to_string()))?;
    let mut stderr_text = String::new();
    if let Some(stderr) = child.stderr.take() {
        // Progress lines end in `\r`; split on both `\r` and `\n`.
        let mut reader = BufReader::new(stderr);
        let mut buf = Vec::new();
        let mut byte = [0u8; 1];
        while reader
            .read(&mut byte)
            .map_err(|e| GitError::Other(e.to_string()))?
            == 1
        {
            if byte[0] == b'\r' || byte[0] == b'\n' {
                let line = String::from_utf8_lossy(&buf).into_owned();
                if let Some(p) = parse_progress_line(&line) {
                    progress(p);
                } else if !line.trim().is_empty() {
                    stderr_text.push_str(&line);
                    stderr_text.push('\n');
                }
                buf.clear();
            } else {
                buf.push(byte[0]);
            }
        }
    }
    let status = child.wait().map_err(|e| GitError::Other(e.to_string()))?;
    if status.success() {
        Ok(())
    } else {
        Err(classify_git_error(&stderr_text))
    }
}

// ----- libgit2 ---------------------------------------------------------------------------

fn clone_libgit2(
    url: &RepoUrl,
    dest: &Path,
    progress: &mut impl FnMut(CloneProgress),
) -> Result<(), GitError> {
    let config = git2::Config::open_default().ok();
    let mut tried_helper = false;
    let mut callbacks = git2::RemoteCallbacks::new();
    callbacks.credentials(move |remote_url, username, allowed| {
        if allowed.contains(git2::CredentialType::USER_PASS_PLAINTEXT) && !tried_helper {
            tried_helper = true;
            if let Some(cfg) = &config {
                if let Ok(c) = git2::Cred::credential_helper(cfg, remote_url, username) {
                    return Ok(c);
                }
            }
        }
        if allowed.contains(git2::CredentialType::DEFAULT) {
            return git2::Cred::default();
        }
        Err(git2::Error::from_str("no credentials available"))
    });
    // Progress is reported through a RefCell since both callbacks need it.
    let progress_cell = std::cell::RefCell::new(progress);
    let mut last = (255u8, 255u8);
    callbacks.transfer_progress(|stats| {
        let total = stats.total_objects().max(1);
        let recv = (stats.received_objects() * 100 / total) as u8;
        let res = if stats.total_deltas() > 0 {
            (stats.indexed_deltas() * 100 / stats.total_deltas()) as u8
        } else {
            0
        };
        if (recv, res) != last {
            last = (recv, res);
            let mut p = progress_cell.borrow_mut();
            if recv < 100 || stats.total_deltas() == 0 {
                (p)(CloneProgress::Receiving { percent: recv });
            } else {
                (p)(CloneProgress::Resolving { percent: res });
            }
        }
        true
    });
    let mut fetch = git2::FetchOptions::new();
    fetch.remote_callbacks(callbacks);
    let result = git2::build::RepoBuilder::new()
        .fetch_options(fetch)
        .clone(&url.url, dest);
    match result {
        Ok(_) => Ok(()),
        Err(e) if e.code() == git2::ErrorCode::Auth => Err(GitError::AuthenticationFailed),
        Err(e) if e.code() == git2::ErrorCode::NotFound => Err(GitError::NotFound),
        Err(e) => Err(classify_git_error(e.message())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn progress_lines() {
        assert_eq!(
            parse_progress_line("Receiving objects:  45% (9/20)"),
            Some(CloneProgress::Receiving { percent: 45 })
        );
        assert_eq!(
            parse_progress_line("Receiving objects: 100% (20/20), 5.1 KiB | 5.1 MiB/s, done."),
            Some(CloneProgress::Receiving { percent: 100 })
        );
        assert_eq!(
            parse_progress_line("Resolving deltas:   3% (1/30)"),
            Some(CloneProgress::Resolving { percent: 3 })
        );
        assert_eq!(parse_progress_line("remote: Counting objects:  50% (1/2)"), None);
        assert_eq!(parse_progress_line("Cloning into 'x'..."), None);
    }

    #[test]
    fn error_classification() {
        assert!(matches!(
            classify_git_error(
                "fatal: could not read Username for 'https://github.com': terminal prompts disabled"
            ),
            GitError::AuthenticationFailed
        ));
        assert!(matches!(
            classify_git_error("git@host: Permission denied (publickey)."),
            GitError::AuthenticationFailed
        ));
        assert!(matches!(
            classify_git_error("remote: Repository not found.\nfatal: repository 'https://x/y/' not found"),
            GitError::NotFound
        ));
        assert!(matches!(
            classify_git_error("fatal: unable to access: Could not resolve host: x"),
            GitError::Network
        ));
        match classify_git_error("weird\nfatal: something odd\n") {
            GitError::Other(m) => assert_eq!(m, "fatal: something odd"),
            e => panic!("{e:?}"),
        }
    }
}
