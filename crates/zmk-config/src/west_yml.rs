//! `config/west.yml` — the west manifest that decides which ZMK revision and which extra
//! modules (e.g. `urob/zmk-helpers`) a config builds against.
//!
//! Read-only for now (Phase 3 generates its own workspace manifest that imports this
//! one; pinning a revision from the app is an upgrade-assistant feature).

use serde::{Deserialize, Serialize};

#[derive(Debug, thiserror::Error)]
pub enum WestError {
    #[error("west.yml is not valid YAML: {0}")]
    Yaml(#[from] serde_yaml::Error),
    #[error("west.yml has no `manifest:` section")]
    NoManifest,
}

#[derive(Debug, Clone, Deserialize)]
struct RawFile {
    manifest: Option<RawManifest>,
}

#[derive(Debug, Clone, Deserialize, Default)]
struct RawManifest {
    #[serde(default)]
    defaults: RawDefaults,
    #[serde(default)]
    remotes: Vec<RawRemote>,
    #[serde(default)]
    projects: Vec<RawProject>,
    #[serde(rename = "self")]
    self_: Option<RawSelf>,
}

#[derive(Debug, Clone, Deserialize, Default)]
struct RawDefaults {
    remote: Option<String>,
    revision: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct RawRemote {
    name: String,
    #[serde(rename = "url-base")]
    url_base: String,
}

#[derive(Debug, Clone, Deserialize)]
struct RawProject {
    name: String,
    remote: Option<String>,
    url: Option<String>,
    #[serde(rename = "repo-path")]
    repo_path: Option<String>,
    revision: Option<String>,
    path: Option<String>,
    import: Option<serde_yaml::Value>,
    #[serde(rename = "clone-depth")]
    clone_depth: Option<u32>,
}

#[derive(Debug, Clone, Deserialize)]
struct RawSelf {
    path: Option<String>,
}

/// How a project's `revision` behaves over time.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RevisionKind {
    /// A full or abbreviated commit hash — reproducible.
    Commit,
    /// Looks like a version (`v0.3`, `v0.3.0`) — a tag or release branch; stable-ish.
    Version,
    /// A moving branch (`main`, `master`, feature branches) — changes over time.
    Branch,
}

impl RevisionKind {
    pub fn classify(rev: &str) -> RevisionKind {
        let is_hex = rev.len() >= 7 && rev.len() <= 40 && rev.bytes().all(|b| b.is_ascii_hexdigit());
        if is_hex {
            return RevisionKind::Commit;
        }
        let v = rev.strip_prefix('v').unwrap_or("");
        if v.starts_with(|c: char| c.is_ascii_digit()) {
            return RevisionKind::Version;
        }
        RevisionKind::Branch
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Project {
    pub name: String,
    /// Clone URL, resolved from `url` or `remote` + `repo-path`.
    pub url: Option<String>,
    /// Effective revision (project's own, else `defaults.revision`, else west's `master`).
    pub revision: String,
    pub revision_kind: RevisionKind,
    /// Where west checks it out, relative to the workspace.
    pub path: String,
    /// The project imports manifests (ZMK imports `app/west.yml`: Zephyr + modules).
    pub imports: bool,
    pub clone_depth: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct WestManifest {
    pub projects: Vec<Project>,
    /// `self.path` (usually `config`).
    pub self_path: Option<String>,
}

/// West's built-in default when neither the project nor `defaults` sets a revision.
pub const WEST_DEFAULT_REVISION: &str = "master";

fn is_zmk_url(url: &str) -> bool {
    let u = url.trim_end_matches('/').trim_end_matches(".git");
    u.rsplit('/').next() == Some("zmk")
}

impl WestManifest {
    pub fn parse(yaml: &str) -> Result<WestManifest, WestError> {
        let raw: RawFile = serde_yaml::from_str(yaml)?;
        let m = raw.manifest.ok_or(WestError::NoManifest)?;
        let projects = m
            .projects
            .iter()
            .map(|p| {
                let remote = p.remote.as_ref().or(m.defaults.remote.as_ref());
                let url = p.url.clone().or_else(|| {
                    let r = m.remotes.iter().find(|r| Some(&r.name) == remote)?;
                    let repo = p.repo_path.as_deref().unwrap_or(&p.name);
                    Some(format!("{}/{}", r.url_base.trim_end_matches('/'), repo))
                });
                let revision = p
                    .revision
                    .clone()
                    .or_else(|| m.defaults.revision.clone())
                    .unwrap_or_else(|| WEST_DEFAULT_REVISION.to_string());
                let imports = match &p.import {
                    None => false,
                    Some(serde_yaml::Value::Bool(b)) => *b,
                    Some(_) => true,
                };
                Project {
                    name: p.name.clone(),
                    url,
                    revision_kind: RevisionKind::classify(&revision),
                    revision,
                    path: p.path.clone().unwrap_or_else(|| p.name.clone()),
                    imports,
                    clone_depth: p.clone_depth,
                }
            })
            .collect();
        Ok(WestManifest {
            projects,
            self_path: m.self_.and_then(|s| s.path),
        })
    }

    /// The ZMK firmware project (by name `zmk` or a URL ending in `/zmk`).
    pub fn zmk(&self) -> Option<&Project> {
        self.projects.iter().find(|p| p.name == "zmk").or_else(|| {
            self.projects
                .iter()
                .find(|p| p.url.as_deref().is_some_and(is_zmk_url))
        })
    }

    /// Every project other than ZMK: extra modules such as `zmk-helpers`.
    pub fn extra_modules(&self) -> Vec<&Project> {
        let zmk = self.zmk().map(|p| p.name.as_str());
        self.projects
            .iter()
            .filter(|p| Some(p.name.as_str()) != zmk)
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn revision_kinds() {
        assert_eq!(RevisionKind::classify("main"), RevisionKind::Branch);
        assert_eq!(RevisionKind::classify("v0.3"), RevisionKind::Version);
        assert_eq!(RevisionKind::classify("v0.3.0"), RevisionKind::Version);
        assert_eq!(
            RevisionKind::classify("4de313367288aad5aeaa1243890e40dc813f7008"),
            RevisionKind::Commit
        );
        assert_eq!(RevisionKind::classify("4de3133"), RevisionKind::Commit);
        assert_eq!(RevisionKind::classify("deadbeef-fix"), RevisionKind::Branch);
        assert_eq!(RevisionKind::classify("vendor"), RevisionKind::Branch);
    }

    #[test]
    fn url_and_default_resolution() {
        let m = WestManifest::parse(
            "manifest:\n  defaults: { remote: gh }\n  remotes: [{ name: gh, url-base: 'https://example.org/' }]\n  projects:\n    - name: fw\n      repo-path: zmk\n      import: { file: app/west.yml }\n    - name: other\n      url: https://git.example.com/x/other.git\n      import: false\n",
        )
        .unwrap();
        let zmk = m.zmk().unwrap();
        assert_eq!(zmk.name, "fw");
        assert_eq!(zmk.url.as_deref(), Some("https://example.org/zmk"));
        assert_eq!(zmk.revision, "master");
        assert!(zmk.imports);
        assert!(!m.projects[1].imports);
        assert_eq!(m.extra_modules().len(), 1);
    }

    #[test]
    fn errors() {
        assert!(matches!(
            WestManifest::parse("foo: 1"),
            Err(WestError::NoManifest)
        ));
        assert!(matches!(
            WestManifest::parse("manifest: ["),
            Err(WestError::Yaml(_))
        ));
    }
}
