//! Understanding what the user pasted as a repository address. Host-agnostic: GitHub,
//! Gitea, GitLab, self-hosted, SSH or a local folder are all fine.

use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum UrlKind {
    /// `https://…` or `http://…`
    Http,
    /// `ssh://…` or scp-like `user@host:path`
    Ssh,
    /// `git://…`
    Git,
    /// `file://…` or a local path
    Local,
}

impl UrlKind {
    /// SSH needs keys/agents that only the system git handles well.
    pub fn needs_system_git(self) -> bool {
        matches!(self, UrlKind::Ssh)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RepoUrl {
    /// Normalised address passed to git.
    pub url: String,
    pub kind: UrlKind,
    /// Suggested folder name (last path segment without `.git`).
    pub dir_name: String,
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum UrlError {
    #[error("Please paste the address of your config repository.")]
    Empty,
    #[error("That doesn't look like a git address. It usually starts with https:// or git@ (for example https://github.com/you/zmk-config).")]
    Unrecognised,
}

fn dir_name_from(path: &str) -> String {
    let trimmed = path.trim_end_matches(['/', '\\']);
    let last = trimmed.rsplit(['/', '\\', ':']).next().unwrap_or(trimmed);
    let name = last.strip_suffix(".git").unwrap_or(last);
    if name.is_empty() {
        "zmk-config".into()
    } else {
        name.to_string()
    }
}

fn is_scp_like(s: &str) -> bool {
    // user@host:path (no scheme, colon before any slash, not a Windows drive `C:\`).
    match s.find(':') {
        Some(colon) => {
            let before = &s[..colon];
            !before.is_empty()
                && !before.contains('/')
                && before.contains('@')
                && !s[colon + 1..].starts_with("//")
        }
        None => false,
    }
}

fn is_local_path(s: &str) -> bool {
    let b = s.as_bytes();
    s.starts_with('/')
        || s.starts_with("./")
        || s.starts_with("../")
        || s.starts_with('~')
        || s.starts_with("\\\\")
        || (b.len() >= 3 && b[0].is_ascii_alphabetic() && b[1] == b':' && (b[2] == b'\\' || b[2] == b'/'))
}

pub fn parse_repo_url(input: &str) -> Result<RepoUrl, UrlError> {
    let s = input.trim();
    if s.is_empty() {
        return Err(UrlError::Empty);
    }
    let lower = s.to_ascii_lowercase();
    let (kind, path_part) = if lower.starts_with("https://") || lower.starts_with("http://") {
        let rest = &s[s.find("://").unwrap() + 3..];
        // Needs a host and at least one path segment.
        let (host, path) = rest.split_once('/').ok_or(UrlError::Unrecognised)?;
        if host.is_empty() || path.trim_matches('/').is_empty() {
            return Err(UrlError::Unrecognised);
        }
        (UrlKind::Http, path)
    } else if lower.starts_with("ssh://") {
        (UrlKind::Ssh, &s[6..])
    } else if lower.starts_with("git://") {
        (UrlKind::Git, &s[6..])
    } else if lower.starts_with("file://") {
        (UrlKind::Local, &s[7..])
    } else if is_scp_like(s) {
        (UrlKind::Ssh, &s[s.find(':').unwrap() + 1..])
    } else if is_local_path(s) {
        (UrlKind::Local, s)
    } else {
        return Err(UrlError::Unrecognised);
    };
    // Drop a trailing web-UI suffix people copy by accident (…/tree/main).
    let url = match (kind, s.find("/tree/").or_else(|| s.find("/-/tree/"))) {
        (UrlKind::Http, Some(i)) => s[..i].trim_end_matches("/-").to_string(),
        _ => s.trim_end_matches('/').to_string(),
    };
    if path_part.trim_matches(['/', '\\']).is_empty() {
        return Err(UrlError::Unrecognised);
    }
    Ok(RepoUrl {
        dir_name: dir_name_from(&url),
        url,
        kind,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ok(s: &str) -> (String, UrlKind, String) {
        let u = parse_repo_url(s).unwrap();
        (u.url, u.kind, u.dir_name)
    }

    #[test]
    fn accepts_common_forms() {
        assert_eq!(
            ok(" https://github.com/xnmrph/corne "),
            (
                "https://github.com/xnmrph/corne".into(),
                UrlKind::Http,
                "corne".into()
            )
        );
        assert_eq!(
            ok("https://gitea.example.org/me/zmk-config.git"),
            (
                "https://gitea.example.org/me/zmk-config.git".into(),
                UrlKind::Http,
                "zmk-config".into()
            )
        );
        assert_eq!(
            ok("git@github.com:xnmrph/corne.git"),
            (
                "git@github.com:xnmrph/corne.git".into(),
                UrlKind::Ssh,
                "corne".into()
            )
        );
        assert_eq!(ok("ssh://git@host:2222/a/b.git").1, UrlKind::Ssh);
        assert_eq!(ok("ssh://git@host:2222/a/b.git").2, "b");
        assert_eq!(ok("/home/me/zmk-config").1, UrlKind::Local);
        assert_eq!(ok("C:\\Users\\me\\zmk-config\\").2, "zmk-config");
        assert_eq!(ok("file:///srv/git/kb.git").2, "kb");
    }

    #[test]
    fn strips_web_ui_paths() {
        assert_eq!(
            ok("https://github.com/xnmrph/corne/tree/main/config").0,
            "https://github.com/xnmrph/corne"
        );
        assert_eq!(
            ok("https://gitlab.com/me/kb/-/tree/main").0,
            "https://gitlab.com/me/kb"
        );
        assert_eq!(ok("https://gitlab.com/me/kb/-/tree/main").2, "kb");
    }

    #[test]
    fn rejects_garbage() {
        assert_eq!(parse_repo_url("   "), Err(UrlError::Empty));
        assert_eq!(parse_repo_url("corne"), Err(UrlError::Unrecognised));
        assert_eq!(parse_repo_url("xnmrph/corne"), Err(UrlError::Unrecognised));
        assert_eq!(parse_repo_url("https://github.com"), Err(UrlError::Unrecognised));
        assert_eq!(parse_repo_url("https://github.com/"), Err(UrlError::Unrecognised));
    }
}
