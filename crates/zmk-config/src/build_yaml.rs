//! `build.yaml` parsing — the list of firmware variants a zmk-config repo builds.
//!
//! ZMK's `build-user-config` workflow accepts two forms, which may be combined:
//!
//! ```yaml
//! board: [nice_nano@2.0.0]          # cross product of board × shield …
//! shield: [corne_left, corne_right]
//! include:                           # … plus explicit entries
//!   - board: nice_nano@2.0.0
//!     shield: corne_left nice_view_adapter nice_view
//!     snippet: studio-rpc-usb-uart
//!     cmake-args: -DCONFIG_ZMK_USB_LOGGING=y
//!     artifact-name: corne_left_view
//! ```
//!
//! `shield` is a space-separated list (keyboard shield + add-on shields such as displays).

use serde::{Deserialize, Serialize};

#[derive(Debug, thiserror::Error)]
pub enum BuildYamlError {
    #[error("build.yaml is not valid YAML: {0}")]
    Yaml(#[from] serde_yaml::Error),
}

/// A Zephyr board identifier, e.g. `nice_nano@2.0.0` or `nice_nano//zmk`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct BoardId {
    pub raw: String,
    /// Board name without revision/qualifiers, e.g. `nice_nano`.
    pub name: String,
    /// Hardware revision after `@`, e.g. `2.0.0`.
    pub revision: Option<String>,
    /// Qualifiers after the first `/`, e.g. `nrf52840/zmk` (empty SoC allowed: `/zmk`).
    pub qualifiers: Option<String>,
}

impl BoardId {
    pub fn parse(raw: &str) -> BoardId {
        let raw = raw.trim();
        let (base, revision) = match raw.split_once('@') {
            Some((b, r)) => {
                // Revision may itself be followed by qualifiers: `board@1.0.0/soc`.
                match r.split_once('/') {
                    Some((rev, q)) => (format!("{b}/{q}"), Some(rev.to_string())),
                    None => (b.to_string(), Some(r.to_string())),
                }
            }
            None => (raw.to_string(), None),
        };
        let (name, qualifiers) = match base.split_once('/') {
            Some((n, q)) => (n.to_string(), Some(q.trim_start_matches('/').to_string())),
            None => (base, None),
        };
        BoardId {
            raw: raw.to_string(),
            name,
            revision,
            qualifiers,
        }
    }
}

/// One firmware variant to build.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct BuildTarget {
    pub board: BoardId,
    /// Shields in order, e.g. `["corne_left", "nice_view_adapter", "nice_view"]`.
    pub shields: Vec<String>,
    pub snippets: Vec<String>,
    pub cmake_args: Option<String>,
    pub artifact_name: Option<String>,
}

/// Shields that are add-ons rather than keyboards.
const ADDON_SHIELDS: &[&str] = &["nice_view_adapter", "nice_view", "nice_view_gem"];
pub const SETTINGS_RESET_SHIELD: &str = "settings_reset";
pub const STUDIO_SNIPPET: &str = "studio-rpc-usb-uart";

impl BuildTarget {
    pub fn is_settings_reset(&self) -> bool {
        self.shields.iter().any(|s| s == SETTINGS_RESET_SHIELD)
    }

    /// Keyboard shields (e.g. `corne_left`), i.e. shields minus known add-ons.
    pub fn keyboard_shields(&self) -> impl Iterator<Item = &str> {
        self.shields
            .iter()
            .map(String::as_str)
            .filter(|s| !ADDON_SHIELDS.contains(s) && *s != SETTINGS_RESET_SHIELD)
    }

    pub fn has_shield(&self, shield: &str) -> bool {
        self.shields.iter().any(|s| s == shield)
    }

    pub fn has_nice_view(&self) -> bool {
        self.has_shield("nice_view")
    }

    pub fn has_studio(&self) -> bool {
        self.snippets.iter().any(|s| s == STUDIO_SNIPPET)
            || self
                .cmake_args
                .as_deref()
                .is_some_and(|a| a.contains("CONFIG_ZMK_STUDIO=y"))
    }

    /// The artifact name ZMK's workflow would produce: explicit `artifact-name`, else
    /// `<shields joined by '-'>-<board with '/' and '@' made filename-safe>-zmk`.
    pub fn default_artifact_name(&self) -> String {
        if let Some(a) = &self.artifact_name {
            return a.clone();
        }
        let board = self.board.raw.replace(['/', '@'], "_");
        if self.shields.is_empty() {
            format!("{board}-zmk")
        } else {
            format!("{}-{board}-zmk", self.shields.join("-"))
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct BuildMatrix {
    pub targets: Vec<BuildTarget>,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum OneOrMany {
    One(String),
    Many(Vec<String>),
}

impl OneOrMany {
    fn into_vec(self) -> Vec<String> {
        match self {
            OneOrMany::One(s) => vec![s],
            OneOrMany::Many(v) => v,
        }
    }
}

#[derive(Deserialize, Default)]
struct RawFile {
    board: Option<OneOrMany>,
    shield: Option<OneOrMany>,
    #[serde(default)]
    include: Vec<RawEntry>,
}

#[derive(Deserialize)]
struct RawEntry {
    board: String,
    shield: Option<String>,
    snippet: Option<String>,
    #[serde(rename = "cmake-args")]
    cmake_args: Option<String>,
    #[serde(rename = "artifact-name")]
    artifact_name: Option<String>,
}

fn split_words(s: Option<&str>) -> Vec<String> {
    s.map(|s| s.split_whitespace().map(str::to_string).collect())
        .unwrap_or_default()
}

impl BuildMatrix {
    pub fn parse(yaml: &str) -> Result<BuildMatrix, BuildYamlError> {
        // An empty or comment-only file is a valid (empty) matrix.
        let raw: Option<RawFile> = serde_yaml::from_str(yaml)?;
        let raw = raw.unwrap_or_default();
        let mut targets = Vec::new();

        let boards = raw.board.map(OneOrMany::into_vec).unwrap_or_default();
        let shields = raw.shield.map(OneOrMany::into_vec);
        for b in &boards {
            match &shields {
                Some(shields) => {
                    for s in shields {
                        targets.push(BuildTarget {
                            board: BoardId::parse(b),
                            shields: split_words(Some(s)),
                            snippets: Vec::new(),
                            cmake_args: None,
                            artifact_name: None,
                        });
                    }
                }
                None => targets.push(BuildTarget {
                    board: BoardId::parse(b),
                    shields: Vec::new(),
                    snippets: Vec::new(),
                    cmake_args: None,
                    artifact_name: None,
                }),
            }
        }

        for e in raw.include {
            targets.push(BuildTarget {
                board: BoardId::parse(&e.board),
                shields: split_words(e.shield.as_deref()),
                snippets: split_words(e.snippet.as_deref()),
                cmake_args: e.cmake_args.filter(|s| !s.trim().is_empty()),
                artifact_name: e.artifact_name,
            });
        }
        Ok(BuildMatrix { targets })
    }

    /// Distinct keyboard shields across all targets (e.g. `corne_left`, `corne_right`).
    pub fn keyboard_shields(&self) -> Vec<&str> {
        let mut out: Vec<&str> = Vec::new();
        for t in &self.targets {
            for s in t.keyboard_shields() {
                if !out.contains(&s) {
                    out.push(s);
                }
            }
        }
        out
    }

    /// Distinct boards across all targets.
    pub fn boards(&self) -> Vec<&BoardId> {
        let mut out: Vec<&BoardId> = Vec::new();
        for t in &self.targets {
            if !out.iter().any(|b| b.raw == t.board.raw) {
                out.push(&t.board);
            }
        }
        out
    }

    pub fn settings_reset(&self) -> Option<&BuildTarget> {
        self.targets.iter().find(|t| t.is_settings_reset())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn board_ids() {
        let b = BoardId::parse("nice_nano@2.0.0");
        assert_eq!(
            (b.name.as_str(), b.revision.as_deref(), b.qualifiers.as_deref()),
            ("nice_nano", Some("2.0.0"), None)
        );
        let b = BoardId::parse("nice_nano//zmk");
        assert_eq!(
            (b.name.as_str(), b.revision, b.qualifiers.as_deref()),
            ("nice_nano", None, Some("zmk"))
        );
        let b = BoardId::parse("nice_nano@2.0.0/nrf52840/zmk");
        assert_eq!(
            (b.name.as_str(), b.revision.as_deref(), b.qualifiers.as_deref()),
            ("nice_nano", Some("2.0.0"), Some("nrf52840/zmk"))
        );
        let b = BoardId::parse("nice_nano_v2");
        assert_eq!(
            (b.name.as_str(), b.revision, b.qualifiers),
            ("nice_nano_v2", None, None)
        );
    }

    #[test]
    fn matrix_form() {
        let m =
            BuildMatrix::parse("board: [nice_nano_v2]\nshield:\n  - corne_left\n  - corne_right\n").unwrap();
        assert_eq!(m.targets.len(), 2);
        assert_eq!(m.keyboard_shields(), vec!["corne_left", "corne_right"]);
        let m = BuildMatrix::parse("board: nice_nano_v2\n").unwrap();
        assert_eq!(m.targets.len(), 1);
        assert!(m.targets[0].shields.is_empty());
    }

    #[test]
    fn empty_and_comment_only() {
        assert!(BuildMatrix::parse("").unwrap().targets.is_empty());
        assert!(BuildMatrix::parse("# nothing\n---\n").unwrap().targets.is_empty());
    }

    #[test]
    fn invalid_yaml_is_an_error() {
        assert!(BuildMatrix::parse("include: [ {board: }").is_err());
    }

    #[test]
    fn artifact_names() {
        let m = BuildMatrix::parse(
            "include:\n  - board: nice_nano@2.0.0\n    shield: corne_left nice_view_adapter nice_view\n  - board: nice_nano@2.0.0\n    shield: settings_reset\n    artifact-name: reset\n",
        )
        .unwrap();
        assert_eq!(
            m.targets[0].default_artifact_name(),
            "corne_left-nice_view_adapter-nice_view-nice_nano_2.0.0-zmk"
        );
        assert_eq!(m.targets[1].default_artifact_name(), "reset");
    }
}
