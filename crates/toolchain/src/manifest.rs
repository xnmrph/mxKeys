//! `toolchain/manifest.toml`: every pinned version, URL and checksum.

use serde::Deserialize;
use sha2::{Digest, Sha256};

use crate::host::Host;

/// The manifest compiled into the app.
pub const EMBEDDED: &str = include_str!("../../../toolchain/manifest.toml");

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Manifest {
    pub schema: u32,
    pub zephyr_sdk: SdkSection,
    pub python: PythonSection,
    pub workspace: WorkspaceSection,
    #[serde(default)]
    pub trim: TrimSection,
    #[serde(rename = "artifact")]
    pub artifacts: Vec<Artifact>,
    /// sha256 of the manifest text (first 12 hex chars) — names the install directory.
    #[serde(skip)]
    pub id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct SdkSection {
    pub version: String,
    pub toolchain: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct PythonSection {
    pub version: String,
    pub bootstrap: Vec<String>,
    pub workspace_requirements: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct WorkspaceSection {
    pub allowlist: Vec<String>,
    pub clone_depth: u32,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
pub struct TrimSection {
    #[serde(default)]
    pub cmake: Vec<String>,
    #[serde(default)]
    pub zephyr_sdk: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Deserialize, serde::Serialize, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum Component {
    ZephyrSdk,
    ArmToolchain,
    Python,
    Cmake,
    Ninja,
}

impl Component {
    pub const ALL: [Component; 5] = [
        Component::ZephyrSdk,
        Component::ArmToolchain,
        Component::Python,
        Component::Cmake,
        Component::Ninja,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Component::ZephyrSdk => "Zephyr SDK",
            Component::ArmToolchain => "ARM compiler",
            Component::Python => "Python",
            Component::Cmake => "CMake",
            Component::Ninja => "Ninja",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
pub enum Format {
    #[serde(rename = "tar.xz")]
    TarXz,
    #[serde(rename = "tar.gz")]
    TarGz,
    #[serde(rename = "zip")]
    Zip,
    #[serde(rename = "7z")]
    SevenZ,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Artifact {
    pub component: Component,
    pub host: Host,
    pub url: String,
    pub sha256: String,
    pub format: Format,
    pub strip: usize,
    pub dest: String,
}

#[derive(Debug, thiserror::Error)]
pub enum ManifestError {
    #[error("toolchain manifest is invalid: {0}")]
    Toml(#[from] toml::de::Error),
    #[error("toolchain manifest: {0}")]
    Invalid(String),
}

impl Manifest {
    pub fn parse(text: &str) -> Result<Manifest, ManifestError> {
        let mut m: Manifest = toml::from_str(text)?;
        if m.schema != 1 {
            return Err(ManifestError::Invalid(format!("unsupported schema {}", m.schema)));
        }
        for a in &m.artifacts {
            if a.sha256.len() != 64 || !a.sha256.bytes().all(|b| b.is_ascii_hexdigit()) {
                return Err(ManifestError::Invalid(format!("bad sha256 for {}", a.url)));
            }
            if !a.url.starts_with("https://") {
                return Err(ManifestError::Invalid(format!(
                    "artifact URL must be https: {}",
                    a.url
                )));
            }
        }
        m.id = hex::encode(Sha256::digest(text.as_bytes()))[..12].to_string();
        Ok(m)
    }

    pub fn embedded() -> Manifest {
        Manifest::parse(EMBEDDED).expect("embedded toolchain manifest is valid (checked by tests)")
    }

    /// Artifacts for `host`, in install order; error if a component is missing.
    pub fn artifacts_for(&self, host: Host) -> Result<Vec<&Artifact>, ManifestError> {
        Component::ALL
            .iter()
            .map(|c| {
                self.artifacts
                    .iter()
                    .find(|a| a.component == *c && a.host == host)
                    .ok_or_else(|| ManifestError::Invalid(format!("no {} for {}", c.label(), host.label())))
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn embedded_manifest_is_complete_for_every_host() {
        let m = Manifest::embedded();
        assert_eq!(m.zephyr_sdk.version, "0.16.9");
        assert_eq!(m.id.len(), 12);
        for h in Host::ALL {
            let a = m.artifacts_for(h).unwrap();
            assert_eq!(a.len(), 5, "{h:?}");
            // SDK minimal and toolchain go to the same SDK dir.
            assert!(a[1].dest.starts_with(&a[0].dest));
        }
        assert!(m.workspace.allowlist.contains(&"hal_nordic".to_string()));
    }

    #[test]
    fn rejects_bad_checksums_and_plain_http() {
        let bad = EMBEDDED.replacen("sha256 = \"b433", "sha256 = \"zz33", 1);
        assert!(Manifest::parse(&bad).is_err());
        let http = EMBEDDED.replacen("url = \"https://", "url = \"http://", 1);
        assert!(Manifest::parse(&http).is_err());
    }
}
