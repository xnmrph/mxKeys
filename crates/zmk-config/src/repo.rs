//! Locating and loading the files of a zmk-config checkout.

use std::fs;
use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::build_yaml::{BuildMatrix, BuildYamlError};
use crate::dts::Document;
use crate::keymap::{Keymap, KeymapError};

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RepoFiles {
    pub root: PathBuf,
    pub build_yaml: Option<PathBuf>,
    pub west_yml: Option<PathBuf>,
    pub keymaps: Vec<PathBuf>,
    pub confs: Vec<PathBuf>,
}

#[derive(Debug, thiserror::Error)]
pub enum RepoError {
    #[error("{0} is not a folder")]
    NotADirectory(PathBuf),
    #[error("this folder doesn't look like a ZMK config: no `config/` folder and no `build.yaml`")]
    NotAZmkConfig,
    #[error("could not read {path}: {source}")]
    Io { path: PathBuf, source: std::io::Error },
    #[error(transparent)]
    BuildYaml(#[from] BuildYamlError),
    #[error("{path}: {source}")]
    Keymap { path: PathBuf, source: KeymapError },
    #[error(transparent)]
    West(WestLoadError),
}

impl RepoFiles {
    /// Find the relevant files under `root`. Keymaps/confs are looked up in `config/`
    /// (the zmk-config convention), sorted by name.
    pub fn discover(root: &Path) -> Result<RepoFiles, RepoError> {
        if !root.is_dir() {
            return Err(RepoError::NotADirectory(root.to_path_buf()));
        }
        let config = root.join("config");
        let build_yaml = Some(root.join("build.yaml")).filter(|p| p.is_file());
        if !config.is_dir() && build_yaml.is_none() {
            return Err(RepoError::NotAZmkConfig);
        }
        let mut keymaps = Vec::new();
        let mut confs = Vec::new();
        if config.is_dir() {
            let entries = fs::read_dir(&config).map_err(|source| RepoError::Io {
                path: config.clone(),
                source,
            })?;
            for e in entries.flatten() {
                let p = e.path();
                match p.extension().and_then(|e| e.to_str()) {
                    Some("keymap") => keymaps.push(p),
                    Some("conf") => confs.push(p),
                    _ => {}
                }
            }
        }
        keymaps.sort();
        confs.sort();
        Ok(RepoFiles {
            root: root.to_path_buf(),
            build_yaml,
            west_yml: Some(config.join("west.yml")).filter(|p| p.is_file()),
            keymaps,
            confs,
        })
    }
}

pub fn read(path: &Path) -> Result<String, RepoError> {
    fs::read_to_string(path).map_err(|source| RepoError::Io {
        path: path.to_path_buf(),
        source,
    })
}

/// A loaded keymap file: lossless document + typed model.
#[derive(Debug, Clone)]
pub struct KeymapFile {
    pub path: PathBuf,
    pub document: Document,
    pub keymap: Keymap,
}

impl KeymapFile {
    pub fn load(path: &Path) -> Result<KeymapFile, RepoError> {
        let document = Document::parse(read(path)?);
        let keymap = Keymap::from_document(&document).map_err(|source| RepoError::Keymap {
            path: path.to_path_buf(),
            source,
        })?;
        Ok(KeymapFile {
            path: path.to_path_buf(),
            document,
            keymap,
        })
    }
}

pub fn load_build_matrix(files: &RepoFiles) -> Result<Option<BuildMatrix>, RepoError> {
    match &files.build_yaml {
        Some(p) => Ok(Some(BuildMatrix::parse(&read(p)?)?)),
        None => Ok(None),
    }
}

#[derive(Debug, thiserror::Error)]
#[error("{path}: {source}")]
pub struct WestLoadError {
    pub path: PathBuf,
    pub source: crate::west_yml::WestError,
}

/// Parse `config/west.yml` if present.
pub fn load_west(files: &RepoFiles) -> Result<Option<crate::west_yml::WestManifest>, RepoError> {
    match &files.west_yml {
        Some(p) => crate::west_yml::WestManifest::parse(&read(p)?)
            .map(Some)
            .map_err(|source| {
                RepoError::West(WestLoadError {
                    path: p.clone(),
                    source,
                })
            }),
        None => Ok(None),
    }
}

/// A loaded `.conf` file.
#[derive(Debug, Clone)]
pub struct ConfFile {
    pub path: PathBuf,
    pub file: crate::kconfig::KconfigFile,
}

impl ConfFile {
    pub fn load(path: &Path) -> Result<ConfFile, RepoError> {
        Ok(ConfFile {
            path: path.to_path_buf(),
            file: crate::kconfig::KconfigFile::parse(read(path)?),
        })
    }
}
