//! Working with an existing config repository: status, diff, commit, fetch, pull, push.
//!
//! Reading (status, diff, identity) always uses libgit2 — structured results, no output
//! parsing, works without git installed. Writing that may involve the user's setup
//! (commit hooks and signing, credentials for push/pull) goes through **system git**
//! when it is installed, so credential helpers, SSH keys/agents and signing keys work
//! exactly as in a terminal; libgit2 is the fallback for HTTPS remotes.
//!
//! The app never asks for, stores or logs tokens; remote URLs are shown with any
//! embedded credentials removed.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use serde::Serialize;

use crate::{classify_git_error, Backend, GitError};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ChangeKind {
    Added,
    Modified,
    Deleted,
    Renamed,
    /// Both sides changed it (merge/rebase in progress).
    Conflicted,
    TypeChange,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct FileChange {
    /// Path relative to the repository root, with `/` separators.
    pub path: String,
    pub old_path: Option<String>,
    pub kind: ChangeKind,
    /// Not tracked by git yet.
    pub untracked: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RepoStatus {
    pub root: PathBuf,
    /// Current branch (`None` when detached).
    pub branch: Option<String>,
    /// e.g. `origin/main`.
    pub upstream: Option<String>,
    pub remote_name: Option<String>,
    /// Remote URL with any credentials removed.
    pub remote_url: Option<String>,
    /// Commits here that the server doesn't have.
    pub ahead: usize,
    /// Commits on the server we don't have (as of the last fetch).
    pub behind: usize,
    /// No commits yet.
    pub unborn: bool,
    /// A merge, rebase, cherry-pick … is in progress.
    pub operation: Option<String>,
    pub files: Vec<FileChange>,
    /// Name and email commits will use, if configured.
    pub identity: Option<(String, String)>,
    pub last_commit: Option<CommitInfo>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CommitInfo {
    pub id: String,
    pub short_id: String,
    pub summary: String,
    pub author: String,
    /// Seconds since the Unix epoch.
    pub time: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum LineKind {
    Context,
    Added,
    Removed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DiffLine {
    pub kind: LineKind,
    pub old_line: Option<u32>,
    pub new_line: Option<u32>,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Hunk {
    pub header: String,
    pub lines: Vec<DiffLine>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct FileDiff {
    pub path: String,
    pub old_path: Option<String>,
    pub kind: ChangeKind,
    pub binary: bool,
    pub added: usize,
    pub removed: usize,
    pub hunks: Vec<Hunk>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "result", rename_all = "snake_case")]
pub enum PullOutcome {
    UpToDate,
    /// Fast-forwarded to the server's version.
    Updated {
        commits: usize,
    },
    /// Local commits were replayed on top of the server's (system git only).
    Rebased {
        commits: usize,
    },
}

fn open(dir: &Path) -> Result<git2::Repository, GitError> {
    git2::Repository::discover(dir).map_err(|_| GitError::NotARepository(dir.to_path_buf()))
}

fn g(e: git2::Error) -> GitError {
    match e.code() {
        git2::ErrorCode::Auth => GitError::AuthenticationFailed,
        git2::ErrorCode::NotFound => GitError::NotFound,
        _ => classify_git_error(e.message()),
    }
}

/// `https://user:token@host/x` → `https://host/x`.
pub fn redact_url(url: &str) -> String {
    if let Some(scheme_end) = url.find("://") {
        let rest = &url[scheme_end + 3..];
        let host_end = rest.find('/').unwrap_or(rest.len());
        if let Some(at) = rest[..host_end].rfind('@') {
            return format!("{}{}", &url[..scheme_end + 3], &rest[at + 1..]);
        }
    }
    url.to_string()
}

fn commit_info(c: &git2::Commit) -> CommitInfo {
    let id = c.id().to_string();
    CommitInfo {
        short_id: id[..7].to_string(),
        id,
        summary: c.summary().ok().flatten().unwrap_or("").to_string(),
        author: c.author().name().unwrap_or("").to_string(),
        time: c.time().seconds(),
    }
}

fn kind_of(s: git2::Status) -> Option<(ChangeKind, bool)> {
    use git2::Status as S;
    if s.contains(S::CONFLICTED) {
        return Some((ChangeKind::Conflicted, false));
    }
    if s.contains(S::WT_NEW) && !s.intersects(S::INDEX_NEW | S::INDEX_MODIFIED) {
        return Some((ChangeKind::Added, true));
    }
    if s.intersects(S::INDEX_RENAMED | S::WT_RENAMED) {
        return Some((ChangeKind::Renamed, false));
    }
    if s.intersects(S::INDEX_NEW) {
        return Some((ChangeKind::Added, false));
    }
    if s.intersects(S::INDEX_DELETED | S::WT_DELETED) {
        return Some((ChangeKind::Deleted, false));
    }
    if s.intersects(S::INDEX_TYPECHANGE | S::WT_TYPECHANGE) {
        return Some((ChangeKind::TypeChange, false));
    }
    if s.intersects(S::INDEX_MODIFIED | S::WT_MODIFIED) {
        return Some((ChangeKind::Modified, false));
    }
    None
}

fn operation_name(state: git2::RepositoryState) -> Option<String> {
    use git2::RepositoryState as R;
    Some(
        match state {
            R::Clean => return None,
            R::Merge => "merge",
            R::Revert | R::RevertSequence => "revert",
            R::CherryPick | R::CherryPickSequence => "cherry-pick",
            R::Bisect => "bisect",
            R::Rebase | R::RebaseInteractive | R::RebaseMerge => "rebase",
            R::ApplyMailbox | R::ApplyMailboxOrRebase => "am",
        }
        .to_string(),
    )
}

/// Whether `dir` is inside a git repository.
pub fn is_repository(dir: &Path) -> bool {
    git2::Repository::discover(dir).is_ok()
}

/// Turn a plain folder into a git repository (first branch `main`).
pub fn init(dir: &Path) -> Result<(), GitError> {
    let mut opts = git2::RepositoryInitOptions::new();
    opts.initial_head("main");
    git2::Repository::init_opts(dir, &opts).map_err(g)?;
    Ok(())
}

pub fn status(dir: &Path) -> Result<RepoStatus, GitError> {
    let repo = open(dir)?;
    let root = repo
        .workdir()
        .ok_or_else(|| GitError::Other("this is a bare repository".into()))?
        .to_path_buf();
    let head = repo.head().ok();
    let unborn = head.is_none();
    let branch = match &head {
        Some(h) if h.is_branch() => h.shorthand().ok().map(str::to_string),
        Some(_) => None,
        // Unborn: HEAD points at a branch that has no commits yet.
        None => repo.find_reference("HEAD").ok().and_then(|r| {
            r.symbolic_target()
                .ok()
                .flatten()
                .map(|t| t.trim_start_matches("refs/heads/").to_string())
        }),
    };
    let mut upstream = None;
    let mut remote_name = None;
    let (mut ahead, mut behind) = (0, 0);
    if let Some(name) = &branch {
        if let Ok(b) = repo.find_branch(name, git2::BranchType::Local) {
            if let Ok(up) = b.upstream() {
                upstream = up.name().ok().flatten().map(str::to_string);
                if let (Some(local), Some(remote)) = (b.get().target(), up.get().target()) {
                    if let Ok((a, bh)) = repo.graph_ahead_behind(local, remote) {
                        ahead = a;
                        behind = bh;
                    }
                }
            }
            let key = format!("branch.{name}.remote");
            remote_name = repo.config().ok().and_then(|c| c.get_string(&key).ok());
        }
    }
    if remote_name.is_none() {
        let remotes = repo.remotes().map_err(g)?;
        let names: Vec<&str> = remotes.iter().flatten().flatten().collect();
        remote_name = names
            .iter()
            .find(|n| **n == "origin")
            .or(names.first())
            .map(|s| s.to_string());
    }
    let remote_url = remote_name
        .as_deref()
        .and_then(|n| repo.find_remote(n).ok())
        .and_then(|r| r.url().ok().map(redact_url));
    // An unpushed branch without upstream: count every commit as "not on the server".
    if upstream.is_none() && !unborn && remote_name.is_some() {
        if let Some(oid) = head.as_ref().and_then(|h| h.target()) {
            let mut walk = repo.revwalk().map_err(g)?;
            walk.push(oid).map_err(g)?;
            // Hide everything any remote-tracking branch already has.
            for r in repo.references_glob("refs/remotes/*").map_err(g)?.flatten() {
                if let Some(t) = r.target() {
                    let _ = walk.hide(t);
                }
            }
            ahead = walk.count();
        }
    }

    let mut opts = git2::StatusOptions::new();
    opts.include_untracked(true)
        .recurse_untracked_dirs(true)
        .include_ignored(false)
        .renames_head_to_index(true)
        .renames_index_to_workdir(true);
    let statuses = repo.statuses(Some(&mut opts)).map_err(g)?;
    let mut files = Vec::new();
    for e in statuses.iter() {
        let Some((kind, untracked)) = kind_of(e.status()) else {
            continue;
        };
        let delta = e.head_to_index().or(e.index_to_workdir());
        let old_path = delta
            .as_ref()
            .and_then(|d| d.old_file().path())
            .map(|p| p.to_string_lossy().replace('\\', "/"));
        let path = e.path().unwrap_or("").to_string();
        files.push(FileChange {
            old_path: old_path.filter(|o| *o != path && kind == ChangeKind::Renamed),
            path,
            kind,
            untracked,
        });
    }
    files.sort_by(|a, b| a.path.cmp(&b.path));
    let identity = identity_of(&repo);
    let last_commit = head
        .and_then(|h| h.peel_to_commit().ok())
        .map(|c| commit_info(&c));
    Ok(RepoStatus {
        root,
        branch,
        upstream,
        remote_name,
        remote_url,
        ahead,
        behind,
        unborn,
        operation: operation_name(repo.state()),
        files,
        identity,
        last_commit,
    })
}

fn identity_of(repo: &git2::Repository) -> Option<(String, String)> {
    let cfg = repo.config().ok()?.snapshot().ok()?;
    let name = cfg
        .get_string("user.name")
        .ok()
        .filter(|s| !s.trim().is_empty())?;
    let email = cfg
        .get_string("user.email")
        .ok()
        .filter(|s| !s.trim().is_empty())?;
    Some((name, email))
}

/// Save the name and email for commits in *this repository's* `.git/config` (not the
/// user's global git config, and never in a committed file).
pub fn set_identity(dir: &Path, name: &str, email: &str) -> Result<(), GitError> {
    let (name, email) = (name.trim(), email.trim());
    if name.is_empty() || !email.contains('@') || name.contains('\n') || email.contains('\n') {
        return Err(GitError::Other("please enter a name and an email address".into()));
    }
    let repo = open(dir)?;
    let mut cfg = repo
        .config()
        .map_err(g)?
        .open_level(git2::ConfigLevel::Local)
        .map_err(g)?;
    cfg.set_str("user.name", name).map_err(g)?;
    cfg.set_str("user.email", email).map_err(g)?;
    Ok(())
}

/// Changes since the last commit (staged and unstaged, plus new files), optionally for
/// one file only.
pub fn diff(dir: &Path, only: Option<&str>) -> Result<Vec<FileDiff>, GitError> {
    let repo = open(dir)?;
    let head_tree = repo.head().ok().and_then(|h| h.peel_to_tree().ok());
    let mut opts = git2::DiffOptions::new();
    opts.include_untracked(true)
        .recurse_untracked_dirs(true)
        .show_untracked_content(true)
        .context_lines(3);
    if let Some(p) = only {
        opts.pathspec(p).disable_pathspec_match(true);
    }
    let mut diff = repo
        .diff_tree_to_workdir_with_index(head_tree.as_ref(), Some(&mut opts))
        .map_err(g)?;
    let mut find = git2::DiffFindOptions::new();
    find.renames(true).for_untracked(true);
    let _ = diff.find_similar(Some(&mut find));

    let mut out: Vec<FileDiff> = Vec::new();
    for (i, d) in diff.deltas().enumerate() {
        let path = d
            .new_file()
            .path()
            .or(d.old_file().path())
            .map(|p| p.to_string_lossy().replace('\\', "/"))
            .unwrap_or_default();
        let old = d
            .old_file()
            .path()
            .map(|p| p.to_string_lossy().replace('\\', "/"));
        let kind = match d.status() {
            git2::Delta::Added | git2::Delta::Untracked | git2::Delta::Copied => ChangeKind::Added,
            git2::Delta::Deleted => ChangeKind::Deleted,
            git2::Delta::Renamed => ChangeKind::Renamed,
            git2::Delta::Conflicted => ChangeKind::Conflicted,
            git2::Delta::Typechange => ChangeKind::TypeChange,
            _ => ChangeKind::Modified,
        };
        let mut fd = FileDiff {
            old_path: old.filter(|o| *o != path),
            path,
            kind,
            binary: d.flags().is_binary(),
            added: 0,
            removed: 0,
            hunks: Vec::new(),
        };
        if let Ok(Some(patch)) = git2::Patch::from_diff(&diff, i) {
            fd.binary |= patch.delta().flags().is_binary();
            for h in 0..patch.num_hunks() {
                let (hunk, n) = patch.hunk(h).map_err(g)?;
                let mut lines = Vec::with_capacity(n);
                for l in 0..n {
                    let line = patch.line_in_hunk(h, l).map_err(g)?;
                    let kind = match line.origin() {
                        '+' => LineKind::Added,
                        '-' => LineKind::Removed,
                        ' ' => LineKind::Context,
                        _ => continue, // "\ No newline at end of file" markers
                    };
                    match kind {
                        LineKind::Added => fd.added += 1,
                        LineKind::Removed => fd.removed += 1,
                        LineKind::Context => {}
                    }
                    let text = String::from_utf8_lossy(line.content());
                    lines.push(DiffLine {
                        kind,
                        old_line: line.old_lineno(),
                        new_line: line.new_lineno(),
                        text: text.trim_end_matches(['\n', '\r']).to_string(),
                    });
                }
                fd.hunks.push(Hunk {
                    header: String::from_utf8_lossy(hunk.header()).trim_end().to_string(),
                    lines,
                });
            }
        }
        out.push(fd);
    }
    Ok(out)
}

/// Run system git in `dir`, never prompting on a terminal (there is none). Returns
/// stdout; on failure, a classified error from stderr.
fn run_git(git: &Path, dir: &Path, args: &[&str]) -> Result<String, GitError> {
    let out = Command::new(git)
        .args(args)
        .current_dir(dir)
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("LC_ALL", "C")
        .stdin(Stdio::null())
        .output()
        .map_err(|e| GitError::Other(e.to_string()))?;
    if out.status.success() {
        Ok(String::from_utf8_lossy(&out.stdout).into_owned())
    } else {
        let stderr = String::from_utf8_lossy(&out.stderr).into_owned();
        let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
        Err(classify_write_error(&format!("{stderr}\n{stdout}")))
    }
}

/// Errors that only happen when writing (push rejected, nothing to commit …), else the
/// general classification.
pub fn classify_write_error(text: &str) -> GitError {
    let s = text.to_ascii_lowercase();
    if s.contains("[rejected]")
        || s.contains("non-fast-forward")
        || s.contains("fetch first")
        || s.contains("contains commits that are not present locally")
    {
        GitError::Rejected
    } else if s.contains("not possible to fast-forward")
        || s.contains("diverging branches")
        || s.contains("have diverged")
    {
        GitError::Diverged
    } else if s.contains("nothing to commit") || s.contains("no changes added to commit") {
        GitError::NothingToCommit
    } else if s.contains("please tell me who you are")
        || s.contains("author identity unknown")
        || s.contains("empty ident name")
    {
        GitError::NoIdentity
    } else if s.contains("would be overwritten") {
        GitError::LocalChangesInTheWay
    } else if s.contains("conflict") {
        GitError::Conflict
    } else if s.contains("protected branch") || s.contains("gh006") {
        GitError::Other("the server doesn't allow pushing to this branch directly (it is protected)".into())
    } else if s.contains("permission to") && s.contains("denied") || s.contains("403") {
        GitError::NoPushPermission
    } else {
        classify_git_error(text)
    }
}

/// Commit `files` (paths relative to the repository root; all changes when `None`).
pub fn commit(
    dir: &Path,
    message: &str,
    files: Option<&[String]>,
    backend: &Backend,
) -> Result<CommitInfo, GitError> {
    let message = message.trim();
    if message.is_empty() {
        return Err(GitError::Other("please describe what you changed".into()));
    }
    let repo = open(dir)?;
    let root = repo
        .workdir()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| dir.to_path_buf());
    if identity_of(&repo).is_none() {
        return Err(GitError::NoIdentity);
    }
    if let Some(op) = operation_name(repo.state()) {
        return Err(GitError::Other(format!(
            "a git {op} is in progress in this folder; finish or cancel it in a git tool first"
        )));
    }
    let st = status(&root)?;
    if st.files.is_empty() || files.is_some_and(|f| f.is_empty()) {
        return Err(GitError::NothingToCommit);
    }
    if let Some(fs) = files {
        for f in fs {
            if !st
                .files
                .iter()
                .any(|c| &c.path == f || c.old_path.as_ref() == Some(f))
            {
                return Err(GitError::Other(format!("{f} has no changes")));
            }
        }
    }
    match backend {
        Backend::SystemGit { path, .. } => {
            let mut add = vec!["add", "-A", "--"];
            let mut commit_args = vec!["commit", "--quiet", "-m", message];
            match files {
                Some(fs) => {
                    // Renames: include the old path so the deletion is committed too.
                    let mut paths: Vec<&str> = fs.iter().map(String::as_str).collect();
                    for c in &st.files {
                        if let Some(old) = &c.old_path {
                            if fs.contains(&c.path) {
                                paths.push(old);
                            }
                        }
                    }
                    add.extend(paths.iter().copied());
                    commit_args.push("--");
                    commit_args.extend(paths.iter().copied());
                }
                None => add.push("."),
            }
            run_git(path, &root, &add)?;
            run_git(path, &root, &commit_args)?;
        }
        Backend::Libgit2 => commit_libgit2(&repo, &st, message, files)?,
    }
    let repo = open(&root)?;
    let head = repo.head().map_err(g)?.peel_to_commit().map_err(g)?;
    Ok(commit_info(&head))
}

fn commit_libgit2(
    repo: &git2::Repository,
    st: &RepoStatus,
    message: &str,
    files: Option<&[String]>,
) -> Result<(), GitError> {
    let mut index = repo.index().map_err(g)?;
    let root = repo.workdir().unwrap_or(Path::new("."));
    let selected = |p: &str| files.is_none_or(|fs| fs.iter().any(|f| f == p));
    for c in st.files.iter().filter(|c| selected(&c.path)) {
        if let Some(old) = &c.old_path {
            let _ = index.remove_path(Path::new(old));
        }
        if root.join(&c.path).exists() {
            index.add_path(Path::new(&c.path)).map_err(g)?;
        } else {
            index.remove_path(Path::new(&c.path)).map_err(g)?;
        }
    }
    index.write().map_err(g)?;
    let tree = repo.find_tree(index.write_tree().map_err(g)?).map_err(g)?;
    let sig = repo.signature().map_err(|_| GitError::NoIdentity)?;
    let parent = repo.head().ok().and_then(|h| h.peel_to_commit().ok());
    if parent.as_ref().is_some_and(|p| p.tree_id() == tree.id()) {
        return Err(GitError::NothingToCommit);
    }
    let parents: Vec<&git2::Commit> = parent.iter().collect();
    repo.commit(Some("HEAD"), &sig, &sig, message, &tree, &parents)
        .map_err(g)?;
    Ok(())
}

fn callbacks<'a>() -> git2::RemoteCallbacks<'a> {
    let config = git2::Config::open_default().ok();
    let mut tried_helper = false;
    let mut cb = git2::RemoteCallbacks::new();
    cb.credentials(move |url, username, allowed| {
        if allowed.contains(git2::CredentialType::USER_PASS_PLAINTEXT) && !tried_helper {
            tried_helper = true;
            if let Some(cfg) = &config {
                if let Ok(c) = git2::Cred::credential_helper(cfg, url, username) {
                    return Ok(c);
                }
            }
        }
        if allowed.contains(git2::CredentialType::DEFAULT) {
            return git2::Cred::default();
        }
        Err(git2::Error::from_str(
            "authentication failed: no credentials available",
        ))
    });
    cb
}

fn remote_for(st: &RepoStatus) -> Result<String, GitError> {
    st.remote_name.clone().ok_or(GitError::NoRemote)
}

fn check_remote_kind(repo: &git2::Repository, remote: &str, backend: &Backend) -> Result<(), GitError> {
    if matches!(backend, Backend::Libgit2) {
        let url = repo
            .find_remote(remote)
            .map_err(g)?
            .url()
            .unwrap_or("")
            .to_string();
        if url.starts_with("git@") || url.starts_with("ssh://") {
            return Err(GitError::SshNeedsSystemGit);
        }
    }
    Ok(())
}

/// Download what's new on the server (doesn't change any files).
pub fn fetch(dir: &Path, backend: &Backend) -> Result<(), GitError> {
    let st = status(dir)?;
    let remote = remote_for(&st)?;
    match backend {
        Backend::SystemGit { path, .. } => {
            run_git(path, &st.root, &["fetch", "--quiet", &remote]).map(|_| ())
        }
        Backend::Libgit2 => {
            let repo = open(dir)?;
            check_remote_kind(&repo, &remote, backend)?;
            let mut r = repo.find_remote(&remote).map_err(g)?;
            let mut fo = git2::FetchOptions::new();
            fo.remote_callbacks(callbacks());
            let refspecs: [&str; 0] = [];
            r.fetch(&refspecs, Some(&mut fo), None).map_err(g)
        }
    }
}

/// Bring in the server's changes. Fast-forward when possible; with system git, local
/// commits are rebased on top (aborted cleanly on conflict). Uncommitted changes must be
/// committed first.
pub fn pull(dir: &Path, backend: &Backend) -> Result<PullOutcome, GitError> {
    fetch(dir, backend)?;
    let st = status(dir)?;
    if st.upstream.is_none() || st.behind == 0 {
        return Ok(PullOutcome::UpToDate);
    }
    if !st.files.iter().all(|f| f.untracked) {
        return Err(GitError::LocalChangesInTheWay);
    }
    let behind = st.behind;
    if st.ahead == 0 {
        match backend {
            Backend::SystemGit { path, .. } => {
                run_git(path, &st.root, &["merge", "--ff-only", "--quiet", "@{upstream}"])?;
            }
            Backend::Libgit2 => fast_forward_libgit2(dir, &st)?,
        }
        return Ok(PullOutcome::Updated { commits: behind });
    }
    match backend {
        Backend::SystemGit { path, .. } => {
            match run_git(path, &st.root, &["rebase", "--quiet", "@{upstream}"]) {
                Ok(_) => Ok(PullOutcome::Rebased { commits: behind }),
                Err(e) => {
                    let _ = run_git(path, &st.root, &["rebase", "--abort"]);
                    Err(match e {
                        GitError::Conflict | GitError::Other(_) => GitError::Conflict,
                        other => other,
                    })
                }
            }
        }
        Backend::Libgit2 => Err(GitError::Diverged),
    }
}

fn fast_forward_libgit2(dir: &Path, st: &RepoStatus) -> Result<(), GitError> {
    let repo = open(dir)?;
    let up = repo
        .find_branch(st.upstream.as_deref().unwrap_or(""), git2::BranchType::Remote)
        .map_err(g)?;
    let target = up.get().target().ok_or(GitError::NotFound)?;
    let commit = repo.find_commit(target).map_err(g)?;
    repo.checkout_tree(
        commit.as_object(),
        Some(git2::build::CheckoutBuilder::new().safe()),
    )
    .map_err(g)?;
    let mut head = repo.head().map_err(g)?;
    head.set_target(target, "mxkeys: fast-forward").map_err(g)?;
    Ok(())
}

/// Upload local commits. Sets the upstream on the first push of a branch.
pub fn push(dir: &Path, backend: &Backend) -> Result<(), GitError> {
    let st = status(dir)?;
    let remote = remote_for(&st)?;
    let branch = st.branch.clone().ok_or_else(|| {
        GitError::Other(
            "you're not on a branch (detached HEAD); switch to a branch in a git tool first".into(),
        )
    })?;
    if st.unborn {
        return Err(GitError::NothingToPush);
    }
    match backend {
        Backend::SystemGit { path, .. } => {
            let args: Vec<&str> = if st.upstream.is_some() {
                vec!["push", "--porcelain"]
            } else {
                vec!["push", "--porcelain", "-u", &remote, "HEAD"]
            };
            run_git(path, &st.root, &args).map(|_| ())
        }
        Backend::Libgit2 => {
            let repo = open(dir)?;
            check_remote_kind(&repo, &remote, backend)?;
            let (dest_remote, dest_branch) = match st.upstream.as_deref().and_then(|u| u.split_once('/')) {
                Some((r, b)) => (r.to_string(), b.to_string()),
                None => (remote.clone(), branch.clone()),
            };
            let mut r = repo.find_remote(&dest_remote).map_err(g)?;
            let rejected = std::cell::RefCell::new(None::<String>);
            let mut cb = callbacks();
            cb.push_update_reference(|_, status| {
                if let Some(msg) = status {
                    *rejected.borrow_mut() = Some(msg.to_string());
                }
                Ok(())
            });
            let mut po = git2::PushOptions::new();
            po.remote_callbacks(cb);
            let spec = format!("refs/heads/{branch}:refs/heads/{dest_branch}");
            r.push(&[spec.as_str()], Some(&mut po))
                .map_err(|e| classify_write_error(e.message()))?;
            let rejected = rejected.borrow().clone();
            if let Some(msg) = rejected {
                return Err(classify_write_error(&format!("[rejected] {msg}")));
            }
            if st.upstream.is_none() {
                // Record the remote-tracking ref and the upstream like `git push -u`.
                if let Ok(local) = repo.find_branch(&branch, git2::BranchType::Local) {
                    if let Some(oid) = local.get().target() {
                        let _ = repo.reference(
                            &format!("refs/remotes/{dest_remote}/{dest_branch}"),
                            oid,
                            true,
                            "mxkeys: push",
                        );
                    }
                }
                let mut local = repo.find_branch(&branch, git2::BranchType::Local).map_err(g)?;
                local
                    .set_upstream(Some(&format!("{dest_remote}/{dest_branch}")))
                    .map_err(g)?;
            } else if let Some(oid) = repo.head().ok().and_then(|h| h.target()) {
                let _ = repo.reference(
                    &format!("refs/remotes/{dest_remote}/{dest_branch}"),
                    oid,
                    true,
                    "mxkeys: push",
                );
            }
            Ok(())
        }
    }
}

/// Throw away uncommitted changes to `files` (back to the last commit; new files are
/// deleted).
pub fn discard(dir: &Path, files: &[String]) -> Result<(), GitError> {
    let repo = open(dir)?;
    let st = status(dir)?;
    let root = st.root.clone();
    let mut checkout_paths: Vec<&str> = Vec::new();
    for f in files {
        let c = st
            .files
            .iter()
            .find(|c| &c.path == f)
            .ok_or_else(|| GitError::Other(format!("{f} has no changes")))?;
        if c.untracked
            || (c.kind == ChangeKind::Added
                && repo
                    .head()
                    .ok()
                    .and_then(|h| h.peel_to_tree().ok())
                    .and_then(|t| t.get_path(Path::new(f)).ok())
                    .is_none())
        {
            let p = root.join(f);
            if p.is_file() {
                std::fs::remove_file(&p).map_err(|e| GitError::Other(e.to_string()))?;
            }
            let mut index = repo.index().map_err(g)?;
            let _ = index.remove_path(Path::new(f));
            index.write().map_err(g)?;
        } else {
            checkout_paths.push(f);
            if let Some(old) = &c.old_path {
                checkout_paths.push(old);
            }
        }
    }
    if !checkout_paths.is_empty() {
        let head = repo.head().map_err(g)?.peel_to_tree().map_err(g)?;
        let mut co = git2::build::CheckoutBuilder::new();
        co.force().update_index(true);
        for p in &checkout_paths {
            co.path(p);
        }
        // Reset the index entries first so staged changes go too.
        let mut index = repo.index().map_err(g)?;
        for p in &checkout_paths {
            match head.get_path(Path::new(p)) {
                Ok(_) => {}
                Err(_) => {
                    let _ = index.remove_path(Path::new(p));
                }
            }
        }
        index.write().map_err(g)?;
        repo.checkout_tree(head.as_object(), Some(&mut co)).map_err(g)?;
        let mut index = repo.index().map_err(g)?;
        index.read(true).map_err(g)?;
        let obj = repo
            .head()
            .map_err(g)?
            .peel(git2::ObjectType::Commit)
            .map_err(g)?;
        repo.reset_default(Some(&obj), checkout_paths.iter().copied())
            .map_err(g)?;
    }
    Ok(())
}

/// A file's text as of the last commit (`None` if it didn't exist or isn't text).
pub fn file_at_head(dir: &Path, path: &str) -> Result<Option<String>, GitError> {
    let repo = open(dir)?;
    let Ok(head) = repo.head() else { return Ok(None) };
    let tree = head.peel_to_tree().map_err(g)?;
    let Ok(entry) = tree.get_path(Path::new(path)) else {
        return Ok(None);
    };
    let blob = repo.find_blob(entry.id()).map_err(g)?;
    Ok(std::str::from_utf8(blob.content()).ok().map(str::to_string))
}

/// Recent commits on the current branch (newest first).
pub fn log(dir: &Path, limit: usize) -> Result<Vec<CommitInfo>, GitError> {
    let repo = open(dir)?;
    let Ok(head) = repo.head() else { return Ok(vec![]) };
    let mut walk = repo.revwalk().map_err(g)?;
    walk.push(head.target().ok_or(GitError::NotFound)?).map_err(g)?;
    walk.take(limit)
        .map(|oid| {
            let c = repo.find_commit(oid.map_err(g)?).map_err(g)?;
            Ok(commit_info(&c))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A bare "server" repo plus a clone of it with one commit, identity set locally.
    struct Fixture {
        base: PathBuf,
        server: PathBuf,
        work: PathBuf,
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.base);
        }
    }

    fn backends() -> Vec<Backend> {
        let mut b = vec![Backend::Libgit2];
        if let Backend::SystemGit { path, version } = crate::detect_backend() {
            b.push(Backend::SystemGit { path, version });
        }
        b
    }

    fn fixture(name: &str) -> Fixture {
        let base = std::env::temp_dir().join(format!("mxkeys-git-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(&base).unwrap();
        let server = base.join("server.git");
        let mut init = git2::RepositoryInitOptions::new();
        init.bare(true).initial_head("main");
        git2::Repository::init_opts(&server, &init).unwrap();
        let seed = base.join("seed");
        init_with_commit(
            &seed,
            &[
                ("config/corne.keymap", "a\nb\nc\n"),
                ("config/corne.conf", "CONFIG_X=y\n"),
            ],
        );
        let repo = git2::Repository::open(&seed).unwrap();
        repo.remote("origin", server.to_str().unwrap()).unwrap();
        let mut r = repo.find_remote("origin").unwrap();
        r.push(&["refs/heads/main:refs/heads/main"], None).unwrap();
        let work = base.join("work");
        git2::Repository::clone(server.to_str().unwrap(), &work).unwrap();
        set_identity(&work, "Test User", "test@example.com").unwrap();
        Fixture { base, server, work }
    }

    fn init_with_commit(dir: &Path, files: &[(&str, &str)]) {
        init(dir).unwrap();
        set_identity(dir, "Seed", "seed@example.com").unwrap();
        for (p, t) in files {
            std::fs::create_dir_all(dir.join(p).parent().unwrap()).unwrap();
            std::fs::write(dir.join(p), t).unwrap();
        }
        commit(dir, "first", None, &Backend::Libgit2).unwrap();
    }

    fn write(dir: &Path, p: &str, t: &str) {
        std::fs::write(dir.join(p), t).unwrap();
    }

    #[test]
    fn redacts_credentials() {
        assert_eq!(
            redact_url("https://user:tok@github.com/a/b.git"),
            "https://github.com/a/b.git"
        );
        assert_eq!(redact_url("https://github.com/a/b"), "https://github.com/a/b");
        assert_eq!(redact_url("git@github.com:a/b.git"), "git@github.com:a/b.git");
    }

    #[test]
    fn status_and_diff() {
        let f = fixture("status");
        let st = status(&f.work).unwrap();
        assert_eq!(st.branch.as_deref(), Some("main"));
        assert_eq!(st.upstream.as_deref(), Some("origin/main"));
        assert_eq!((st.ahead, st.behind), (0, 0));
        assert!(st.files.is_empty());
        assert_eq!(st.identity, Some(("Test User".into(), "test@example.com".into())));
        assert_eq!(st.last_commit.unwrap().summary, "first");

        write(&f.work, "config/corne.keymap", "a\nB\nc\n");
        write(&f.work, "config/new.dtsi", "x\n");
        std::fs::remove_file(f.work.join("config/corne.conf")).unwrap();
        let st = status(&f.work.join("config")).unwrap(); // discovered from a subfolder
        let kinds: Vec<_> = st
            .files
            .iter()
            .map(|c| (c.path.as_str(), c.kind, c.untracked))
            .collect();
        assert_eq!(
            kinds,
            [
                ("config/corne.conf", ChangeKind::Deleted, false),
                ("config/corne.keymap", ChangeKind::Modified, false),
                ("config/new.dtsi", ChangeKind::Added, true),
            ]
        );
        let d = diff(&f.work, None).unwrap();
        let km = d.iter().find(|d| d.path == "config/corne.keymap").unwrap();
        assert_eq!((km.added, km.removed), (1, 1));
        let lines: Vec<_> = km.hunks[0]
            .lines
            .iter()
            .map(|l| (l.kind, l.text.as_str()))
            .collect();
        assert_eq!(
            lines,
            [
                (LineKind::Context, "a"),
                (LineKind::Removed, "b"),
                (LineKind::Added, "B"),
                (LineKind::Context, "c")
            ]
        );
        assert_eq!(
            d.iter().find(|d| d.path == "config/new.dtsi").unwrap().kind,
            ChangeKind::Added
        );
        let only = diff(&f.work, Some("config/corne.keymap")).unwrap();
        assert_eq!(only.len(), 1);
        assert_eq!(
            file_at_head(&f.work, "config/corne.keymap").unwrap().as_deref(),
            Some("a\nb\nc\n")
        );
        assert_eq!(file_at_head(&f.work, "config/new.dtsi").unwrap(), None);
    }

    #[test]
    fn commit_push_pull_with_each_backend() {
        for (i, b) in backends().into_iter().enumerate() {
            let f = fixture(&format!("roundtrip{i}"));
            // Commit only the keymap; the conf change stays uncommitted.
            write(&f.work, "config/corne.keymap", "a\nb\nc\nd\n");
            write(&f.work, "config/corne.conf", "CONFIG_X=n\n");
            let c = commit(&f.work, "Add d", Some(&["config/corne.keymap".to_string()]), &b).unwrap();
            assert_eq!(c.summary, "Add d");
            let st = status(&f.work).unwrap();
            assert_eq!(st.files.len(), 1, "{b:?}");
            assert_eq!(st.files[0].path, "config/corne.conf");
            assert_eq!(st.ahead, 1);
            push(&f.work, &b).unwrap();
            let st = status(&f.work).unwrap();
            assert_eq!(st.ahead, 0, "{b:?}");
            let server = git2::Repository::open_bare(&f.server).unwrap();
            let tip = server
                .find_reference("refs/heads/main")
                .unwrap()
                .peel_to_commit()
                .unwrap();
            assert_eq!(tip.summary().ok().flatten(), Some("Add d"));

            // Someone else pushes; our push is rejected until we pull.
            let other = f.base.join("other");
            git2::Repository::clone(f.server.to_str().unwrap(), &other).unwrap();
            set_identity(&other, "Other", "o@example.com").unwrap();
            write(&other, "README.md", "hi\n");
            commit(&other, "Add readme", None, &Backend::Libgit2).unwrap();
            push(&other, &Backend::Libgit2).unwrap();
            commit(&f.work, "Conf", None, &b).unwrap();
            let r = push(&f.work, &b);
            assert!(matches!(r, Err(GitError::Rejected)), "{b:?} {r:?}");
            match b {
                Backend::SystemGit { .. } => {
                    assert_eq!(pull(&f.work, &b).unwrap(), PullOutcome::Rebased { commits: 1 });
                    push(&f.work, &b).unwrap();
                    assert!(f.work.join("README.md").exists());
                    let st = status(&f.work).unwrap();
                    assert_eq!((st.ahead, st.behind), (0, 0));
                }
                Backend::Libgit2 => assert!(matches!(pull(&f.work, &b), Err(GitError::Diverged))),
            }
        }
    }

    #[test]
    fn fast_forward_pull_and_errors() {
        for (i, b) in backends().into_iter().enumerate() {
            let f = fixture(&format!("ff{i}"));
            let other = f.base.join("other");
            git2::Repository::clone(f.server.to_str().unwrap(), &other).unwrap();
            set_identity(&other, "Other", "o@example.com").unwrap();
            write(&other, "config/corne.conf", "CONFIG_X=n\n");
            commit(&other, "Server change", None, &Backend::Libgit2).unwrap();
            push(&other, &Backend::Libgit2).unwrap();
            // Uncommitted edits to tracked files block pulling.
            write(&f.work, "config/corne.keymap", "z\n");
            assert!(
                matches!(pull(&f.work, &b), Err(GitError::LocalChangesInTheWay)),
                "{b:?}"
            );
            discard(&f.work, &["config/corne.keymap".to_string()]).unwrap();
            assert_eq!(
                std::fs::read_to_string(f.work.join("config/corne.keymap")).unwrap(),
                "a\nb\nc\n"
            );
            assert_eq!(
                pull(&f.work, &b).unwrap(),
                PullOutcome::Updated { commits: 1 },
                "{b:?}"
            );
            assert_eq!(
                std::fs::read_to_string(f.work.join("config/corne.conf")).unwrap(),
                "CONFIG_X=n\n"
            );
            assert!(matches!(
                commit(&f.work, "x", None, &b),
                Err(GitError::NothingToCommit)
            ));
            assert!(matches!(commit(&f.work, "  ", None, &b), Err(GitError::Other(_))));
            assert_eq!(log(&f.work, 5).unwrap().len(), 2);
        }
    }

    #[test]
    fn new_repository_first_push_and_identity() {
        let f = fixture("fresh");
        let dir = f.base.join("fresh");
        std::fs::create_dir_all(&dir).unwrap();
        assert!(!is_repository(&dir) || dir.starts_with(std::env::temp_dir()));
        init(&dir).unwrap();
        write(&dir, "a.txt", "1\n");
        let st = status(&dir).unwrap();
        assert!(st.unborn);
        assert_eq!(st.branch.as_deref(), Some("main"));
        // No identity in this repo; the global config might have one, so only check the
        // error when there is none.
        if st.identity.is_none() {
            assert!(matches!(
                commit(&dir, "x", None, &Backend::Libgit2),
                Err(GitError::NoIdentity)
            ));
        }
        set_identity(&dir, "Me", "me@example.com").unwrap();
        assert!(set_identity(&dir, "Me", "not-an-email").is_err());
        commit(&dir, "Start", None, &Backend::Libgit2).unwrap();
        assert!(matches!(push(&dir, &Backend::Libgit2), Err(GitError::NoRemote)));
        // First push of a branch sets its upstream.
        let server2 = f.base.join("server2.git");
        let mut init2 = git2::RepositoryInitOptions::new();
        init2.bare(true);
        git2::Repository::init_opts(&server2, &init2).unwrap();
        git2::Repository::open(&dir)
            .unwrap()
            .remote("origin", server2.to_str().unwrap())
            .unwrap();
        assert_eq!(status(&dir).unwrap().ahead, 1);
        for b in backends().into_iter().rev().take(1) {
            push(&dir, &b).unwrap();
        }
        let st = status(&dir).unwrap();
        assert_eq!(st.upstream.as_deref(), Some("origin/main"));
        assert_eq!(st.ahead, 0);
    }

    #[test]
    fn write_errors_are_classified() {
        assert!(matches!(
            classify_write_error(
                " ! [rejected]        main -> main (fetch first)\nerror: failed to push some refs"
            ),
            GitError::Rejected
        ));
        assert!(matches!(
            classify_write_error("fatal: Not possible to fast-forward, aborting."),
            GitError::Diverged
        ));
        assert!(matches!(
            classify_write_error("remote: Permission to xnmrph/corne.git denied to someone.\nfatal: unable to access: The requested URL returned error: 403"),
            GitError::NoPushPermission
        ));
        assert!(matches!(
            classify_write_error("Author identity unknown\n*** Please tell me who you are."),
            GitError::NoIdentity
        ));
        assert!(matches!(
            classify_write_error("CONFLICT (content): Merge conflict in config/corne.keymap"),
            GitError::Conflict
        ));
    }
}
