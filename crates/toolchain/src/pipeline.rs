//! The whole "build my firmware" flow, used by the app and by `mxkeys-build` (CI).

use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

use mxkeys_hardware::detect::{detect, Inputs};
use mxkeys_hardware::profile::{BuildTarget, TargetId};
use mxkeys_hardware::HardwareProfile;
use serde::Serialize;
use sha2::{Digest, Sha256};
use zmk_config::build_yaml::BuildMatrix;
use zmk_config::kconfig::KconfigFile;
use zmk_config::repo::{self, RepoFiles};

use crate::build::{self, BuildPlan};
use crate::classify::{self, Problem};
use crate::install::{self, InstallError, Installed, Progress};
use crate::manifest::Manifest;
use crate::runner::CommandRunner;
use crate::workspace::{self, Workspace, WorkspaceError};

#[derive(Debug, Clone)]
pub struct Options {
    /// App data directory (toolchain, workspaces, builds).
    pub data: PathBuf,
    /// The user's zmk-config repository.
    pub repo: PathBuf,
    /// Confirmed hardware; detected from the repo if `None`.
    pub profile: Option<HardwareProfile>,
    /// Only these targets (all if empty).
    pub only: Vec<TargetId>,
    /// Fetch the newest commits for moving branches (e.g. ZMK `main`).
    pub update_workspace: bool,
    /// Skip the module allow-list.
    pub full_workspace: bool,
    /// Clean build directories first.
    pub pristine: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "event", rename_all = "snake_case")]
pub enum Event {
    /// A high-level stage in plain language ("Downloading build tools", …).
    Stage {
        text: String,
    },
    Install(Progress),
    /// A raw output line from west/cmake (for the log view).
    Line {
        target: Option<TargetId>,
        text: String,
    },
    TargetStarted {
        target: TargetId,
    },
    TargetFinished {
        target: TargetId,
        ok: bool,
        uf2: Option<PathBuf>,
        problems: Vec<Problem>,
    },
}

#[derive(Debug, thiserror::Error)]
pub enum PipelineError {
    #[error(transparent)]
    Install(#[from] InstallError),
    #[error(transparent)]
    Workspace(#[from] WorkspaceError),
    #[error("{0}")]
    Repo(String),
    #[error("couldn't write {path}: {source}")]
    Io { path: PathBuf, source: std::io::Error },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TargetResult {
    pub target: TargetId,
    pub uf2: Option<PathBuf>,
    pub problems: Vec<Problem>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Outcome {
    pub profile: HardwareProfile,
    pub results: Vec<TargetResult>,
    /// Where the finished `.uf2` files were copied.
    pub firmware_dir: PathBuf,
    /// `<project> <commit>` for every workspace project actually built against.
    pub lock: String,
}

impl Outcome {
    pub fn ok(&self) -> bool {
        self.results.iter().all(|r| r.uf2.is_some())
    }
}

/// Where the finished `.uf2` files for a config repo are kept.
pub fn firmware_dir(data: &Path, repo: &Path) -> PathBuf {
    data.join("firmware").join(repo_key(repo))
}

fn repo_key(repo: &Path) -> String {
    let canon = fs::canonicalize(repo).unwrap_or_else(|_| repo.to_path_buf());
    hex::encode(Sha256::digest(canon.display().to_string().as_bytes()))[..12].to_string()
}

pub fn target_name(t: TargetId) -> &'static str {
    match t {
        TargetId::Left => "left",
        TargetId::Right => "right",
        TargetId::SettingsReset => "settings_reset",
    }
}

/// Hardware profile from the repo's build.yaml/.conf when none was confirmed.
fn detect_profile(files: &RepoFiles) -> Option<HardwareProfile> {
    let build = repo::load_build_matrix(files)
        .ok()
        .flatten()
        .unwrap_or(BuildMatrix { targets: Vec::new() });
    let confs: Vec<KconfigFile> = files
        .confs
        .iter()
        .filter_map(|p| repo::read(p).ok())
        .map(KconfigFile::parse)
        .collect();
    detect(&Inputs {
        build: Some(&build),
        confs: &confs,
        ..Default::default()
    })
    .profile
}

pub fn run(
    opts: &Options,
    manifest: &Manifest,
    runner: &dyn CommandRunner,
    emit: &mut dyn FnMut(Event),
) -> Result<Outcome, PipelineError> {
    let files = RepoFiles::discover(&opts.repo).map_err(|e| PipelineError::Repo(e.to_string()))?;
    let west_yml = files.west_yml.as_ref().ok_or_else(|| {
        PipelineError::Repo("config/west.yml is missing, so we don't know which ZMK to build".into())
    })?;
    let west_text = repo::read(west_yml).map_err(|e| PipelineError::Repo(e.to_string()))?;
    let profile = opts.profile.or_else(|| detect_profile(&files)).ok_or_else(|| {
        PipelineError::Repo(
            "couldn't tell which keyboard this config is for — confirm your hardware first".into(),
        )
    })?;

    emit(Event::Stage {
        text: "Getting the build tools ready".into(),
    });
    let tools: Installed = install::install(&opts.data, manifest, runner, &mut |p| emit(Event::Install(p)))?;

    let mut full = opts.full_workspace;
    loop {
        let allow = (!full).then_some(manifest.workspace.allowlist.as_slice());
        let ws = Workspace::for_manifest(&opts.data, &workspace::generate_manifest(&west_text, allow)?);
        emit(Event::Stage {
            text: "Getting ZMK's source code".into(),
        });
        workspace::prepare(&ws, &tools, manifest, runner, opts.update_workspace, &mut |p| {
            emit(Event::Install(p))
        })?;

        let outcome = build_targets(opts, &files, &ws, &tools, profile, runner, emit)?;
        let unexplained = outcome
            .results
            .iter()
            .any(|r| r.uf2.is_none() && r.problems.iter().all(|p| p.title == "The build failed"));
        if !outcome.ok() && unexplained && !full {
            emit(Event::Stage {
                text: "The build failed for an unclear reason — trying again with all of ZMK's modules"
                    .into(),
            });
            full = true;
            continue;
        }
        return Ok(outcome);
    }
}

fn build_targets(
    opts: &Options,
    files: &RepoFiles,
    ws: &Workspace,
    tools: &Installed,
    profile: HardwareProfile,
    runner: &dyn CommandRunner,
    emit: &mut dyn FnMut(Event),
) -> Result<Outcome, PipelineError> {
    let key = repo_key(&opts.repo);
    let builds = opts.data.join("builds").join(&key);
    let firmware_dir = firmware_dir(&opts.data, &opts.repo);
    fs::create_dir_all(&firmware_dir).map_err(|source| PipelineError::Io {
        path: firmware_dir.clone(),
        source,
    })?;

    let keymap = files
        .keymaps
        .first()
        .and_then(|p| repo::read(p).ok().map(|t| (p.clone(), t)));
    let keys_h = ws.dir.join("zmk/app/include/dt-bindings/zmk/keys.h");
    let mut known: HashSet<String> = fs::read_to_string(&keys_h)
        .map(|t| classify::names_defined_in(&t))
        .unwrap_or_default();
    for h in [
        "bt.h",
        "outputs.h",
        "rgb.h",
        "backlight.h",
        "ext_power.h",
        "modifiers.h",
        "pointing.h",
        "mouse.h",
    ] {
        if let Ok(t) = fs::read_to_string(ws.dir.join("zmk/app/include/dt-bindings/zmk").join(h)) {
            known.extend(classify::names_defined_in(&t));
        }
    }

    let targets: Vec<BuildTarget> = profile
        .build_targets()
        .into_iter()
        .filter(|t| opts.only.is_empty() || opts.only.contains(&t.id))
        .collect();
    let mut results = Vec::new();
    for t in targets {
        let id = t.id;
        emit(Event::TargetStarted { target: id });
        emit(Event::Stage {
            text: format!("Building the {} firmware", plain_target(id)),
        });
        let mut plan = BuildPlan::new(t, &opts.repo, &builds);
        plan.pristine = opts.pristine;
        let mut log: Vec<String> = Vec::new();
        let res = build::build(&plan, ws, tools, runner, &mut |l: &str| {
            log.push(l.to_string());
            emit(Event::Line {
                target: Some(id),
                text: l.to_string(),
            });
        });
        let (uf2, problems) = match res {
            Ok(path) => {
                // Sanity-check the output: a UF2 for the controller's chip family.
                let expected = mxkeys_hardware::catalog::controller(profile.controller).uf2_family;
                let check = fs::read(&path)
                    .map_err(|e| e.to_string())
                    .and_then(|b| mxkeys_flash::uf2::inspect(&b).map_err(|e| e.to_string()));
                match check {
                    Ok(s) if s.family == Some(expected) => {
                        let dest = firmware_dir.join(format!("{}.uf2", target_name(id)));
                        fs::copy(&path, &dest).map_err(|source| PipelineError::Io { path: dest.clone(), source })?;
                        emit(Event::Line {
                            target: Some(id),
                            text: format!("Checked {}: {} blocks, family 0x{:08X}", dest.display(), s.blocks, expected),
                        });
                        (Some(dest), Vec::new())
                    }
                    other => (
                        None,
                        vec![Problem {
                            title: "The build produced a firmware file that doesn't look right for this controller".into(),
                            hint: Some("Please report this — it shouldn't happen.".into()),
                            location: None,
                            raw: format!("{other:?}"),
                        }],
                    ),
                }
            }
            Err(_) => {
                let ctx = classify::Context {
                    keymap: keymap.as_ref().map(|(p, t)| (p.as_path(), t.as_str())),
                    known_names: (!known.is_empty()).then_some(&known),
                };
                (None, classify::classify(&log, &ctx))
            }
        };
        emit(Event::TargetFinished {
            target: id,
            ok: uf2.is_some(),
            uf2: uf2.clone(),
            problems: problems.clone(),
        });
        results.push(TargetResult {
            target: id,
            uf2,
            problems,
        });
    }
    let lock = fs::read_to_string(ws.lock_file()).unwrap_or_default();
    Ok(Outcome {
        profile,
        results,
        firmware_dir,
        lock,
    })
}

pub fn plain_target(t: TargetId) -> &'static str {
    match t {
        TargetId::Left => "left half",
        TargetId::Right => "right half",
        TargetId::SettingsReset => "settings reset",
    }
}
