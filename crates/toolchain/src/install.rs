//! Installing the toolchain into `<data>/toolchain/<manifest id>/`.
//!
//! Every component is installed independently and marked done with a `.done-<name>` file,
//! so an interrupted setup resumes where it stopped and a finished one costs nothing.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use serde::Serialize;

use crate::download::{self, DownloadError};
use crate::extract::{self, ExtractError};
use crate::host::Host;
use crate::manifest::{Artifact, Component, Manifest, ManifestError};
use crate::runner::{Cmd, CommandRunner, RunError};
use crate::trim;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Progress {
    /// A component step started (`step` = "download", "unpack", "trim", "setup").
    Step { component: String, step: String },
    Bytes {
        component: String,
        done: u64,
        total: Option<u64>,
    },
    /// A finished component (and bytes freed by trimming).
    Done { component: String, freed: u64 },
    /// Output from a tool (pip, west …).
    Line { text: String },
}

#[derive(Debug, thiserror::Error)]
pub enum InstallError {
    #[error("this computer ({os} {arch}) isn't supported yet")]
    UnsupportedHost { os: String, arch: String },
    #[error(transparent)]
    Manifest(#[from] ManifestError),
    #[error(transparent)]
    Download(#[from] DownloadError),
    #[error(transparent)]
    Extract(#[from] ExtractError),
    #[error(transparent)]
    Run(#[from] RunError),
    #[error("couldn't write {path}: {source}")]
    Io { path: PathBuf, source: std::io::Error },
    #[error("Building firmware needs git. On a Mac, install Apple's command line tools: open Terminal and run `xcode-select --install`, then try again.")]
    GitMissingMac,
    #[error("Building firmware needs git, which isn't installed. Install it from your package manager (for example `sudo apt install git`) and try again.")]
    GitMissingLinux,
}

fn io(path: &Path) -> impl FnOnce(std::io::Error) -> InstallError + '_ {
    move |source| InstallError::Io {
        path: path.to_path_buf(),
        source,
    }
}

/// Where everything lives.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Layout {
    pub data: PathBuf,
    pub root: PathBuf,
    pub host: Host,
}

impl Layout {
    pub fn new(data: &Path, manifest: &Manifest, host: Host) -> Layout {
        Layout {
            data: data.to_path_buf(),
            root: data.join("toolchain").join(&manifest.id),
            host,
        }
    }
    pub fn downloads(&self) -> PathBuf {
        self.data.join("downloads")
    }
    pub fn sdk(&self) -> PathBuf {
        self.root.join("zephyr-sdk")
    }
    pub fn python(&self) -> PathBuf {
        if self.host.is_windows() {
            self.root.join("python").join("python.exe")
        } else {
            self.root.join("python").join("bin").join("python3")
        }
    }
    pub fn venv(&self) -> PathBuf {
        self.root.join("venv")
    }
    pub fn venv_bin(&self) -> PathBuf {
        self.venv()
            .join(if self.host.is_windows() { "Scripts" } else { "bin" })
    }
    pub fn venv_python(&self) -> PathBuf {
        self.venv_bin().join(self.host.exe("python"))
    }
    /// `python -m west …` — independent of the `west` launcher script, whose absolute
    /// shebang/launcher breaks if the data directory moves.
    pub fn west(&self) -> Cmd {
        Cmd::new(self.venv_python()).args(["-m", "west"])
    }
    pub fn cmake_bin(&self) -> PathBuf {
        self.root.join("cmake").join("bin")
    }
    pub fn ninja_dir(&self) -> PathBuf {
        self.root.join("ninja")
    }
    pub fn bundled_git_dir(&self) -> PathBuf {
        self.root.join("git").join("cmd")
    }
    fn marker(&self, name: &str) -> PathBuf {
        self.root.join(format!(".done-{name}"))
    }
    pub fn is_done(&self, name: &str) -> bool {
        self.marker(name).is_file()
    }
}

/// An installed toolchain, ready to run tools.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Installed {
    pub layout: Layout,
    /// Directory holding a bundled git, if one was needed.
    pub git_dir: Option<PathBuf>,
}

impl Installed {
    /// Environment for python/west/cmake: our tools first on PATH, SDK location, and no
    /// writes into the user's global CMake package registry.
    pub fn env(&self) -> BTreeMap<String, String> {
        let l = &self.layout;
        let sep = if l.host.is_windows() { ";" } else { ":" };
        let mut path: Vec<String> = vec![
            l.venv_bin().display().to_string(),
            l.cmake_bin().display().to_string(),
            l.ninja_dir().display().to_string(),
        ];
        if let Some(g) = &self.git_dir {
            path.push(g.display().to_string());
        }
        if let Ok(p) = std::env::var("PATH") {
            path.push(p);
        }
        let mut env = BTreeMap::new();
        env.insert("PATH".into(), path.join(sep));
        env.insert("ZEPHYR_SDK_INSTALL_DIR".into(), l.sdk().display().to_string());
        env.insert("ZEPHYR_TOOLCHAIN_VARIANT".into(), "zephyr".into());
        env.insert("VIRTUAL_ENV".into(), l.venv().display().to_string());
        env.insert("PIP_DISABLE_PIP_VERSION_CHECK".into(), "1".into());
        env.insert("PYTHONUTF8".into(), "1".into());
        env.insert("PYTHONNOUSERSITE".into(), "1".into());
        env
    }
}

fn venv_healthy(l: &Layout) -> bool {
    Command::new(l.venv_python())
        .args(["-c", "import west"])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|s| s.success())
}

impl Layout {
    /// Identifies the current venv (changes whenever it's recreated).
    pub fn venv_token(&self) -> String {
        fs::read_to_string(self.marker("venv")).unwrap_or_default()
    }
}

fn file_name(url: &str) -> String {
    url.rsplit('/').next().unwrap_or("download").replace("%2B", "+")
}

fn has_system_git() -> bool {
    if std::env::var_os("MXKEYS_FORCE_BUNDLED_GIT").is_some() {
        return false;
    }
    Command::new("git")
        .arg("--version")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|s| s.success())
}

/// Install one artifact: download → verify → unpack to staging → move into place → trim.
fn install_artifact(
    l: &Layout,
    m: &Manifest,
    a: &Artifact,
    client: &reqwest::blocking::Client,
    progress: &mut dyn FnMut(Progress),
) -> Result<(), InstallError> {
    let name = serde_json::to_value(a.component)
        .ok()
        .and_then(|v| v.as_str().map(str::to_string))
        .unwrap_or_default();
    if l.is_done(&name) {
        return Ok(());
    }
    let label = a.component.label().to_string();
    let archive = l.downloads().join(file_name(&a.url));
    progress(Progress::Step {
        component: label.clone(),
        step: "download".into(),
    });
    download::download(client, &a.url, &archive, &a.sha256, |done, total| {
        progress(Progress::Bytes {
            component: label.clone(),
            done,
            total,
        })
    })?;

    progress(Progress::Step {
        component: label.clone(),
        step: "unpack".into(),
    });
    let staging = l.root.join(format!(".staging-{name}"));
    let _ = fs::remove_dir_all(&staging);
    extract::extract(&archive, a.format, &staging, a.strip)?;
    let dest = l.root.join(&a.dest);
    if dest.exists() {
        fs::remove_dir_all(&dest).map_err(io(&dest))?;
    }
    if let Some(p) = dest.parent() {
        fs::create_dir_all(p).map_err(io(p))?;
    }
    fs::rename(&staging, &dest).map_err(io(&dest))?;

    progress(Progress::Step {
        component: label.clone(),
        step: "trim".into(),
    });
    let freed = match a.component {
        Component::ZephyrSdk => trim::remove_globs(&dest, &m.trim.zephyr_sdk).map_err(io(&dest))?,
        Component::Cmake => trim::remove_globs(&dest, &m.trim.cmake).map_err(io(&dest))?,
        Component::ArmToolchain => {
            trim::keep_multilibs(&dest, &mxkeys_hardware::catalog::toolchain_multilibs())
                .map_err(io(&dest))?
        }
        _ => 0,
    };
    // The archive isn't needed any more; keeping it would double the disk use.
    let _ = fs::remove_file(&archive);
    fs::write(l.marker(&name), &a.sha256).map_err(io(&l.root))?;
    progress(Progress::Done {
        component: label,
        freed,
    });
    Ok(())
}

/// Install (or finish installing) everything for this computer.
pub fn install(
    data: &Path,
    manifest: &Manifest,
    runner: &dyn CommandRunner,
    progress: &mut dyn FnMut(Progress),
) -> Result<Installed, InstallError> {
    let host = Host::current().ok_or_else(|| InstallError::UnsupportedHost {
        os: std::env::consts::OS.into(),
        arch: std::env::consts::ARCH.into(),
    })?;
    let l = Layout::new(data, manifest, host);
    fs::create_dir_all(&l.root).map_err(io(&l.root))?;
    let client = download::client();
    for a in manifest.artifacts_for(host)? {
        install_artifact(&l, manifest, a, &client, progress)?;
    }

    let git_dir = if has_system_git() {
        None
    } else if let Some(a) = manifest.optional_for(host, Component::Git) {
        install_artifact(&l, manifest, a, &client, progress)?;
        Some(l.bundled_git_dir())
    } else if cfg!(target_os = "macos") {
        return Err(InstallError::GitMissingMac);
    } else {
        return Err(InstallError::GitMissingLinux);
    };
    let installed = Installed {
        layout: l.clone(),
        git_dir,
    };

    // A venv breaks if the data folder moved or the base Python changed: rebuild it then.
    if l.is_done("venv") && !venv_healthy(&l) {
        let _ = fs::remove_file(l.marker("venv"));
    }
    if !l.is_done("venv") {
        progress(Progress::Step {
            component: "Python packages".into(),
            step: "setup".into(),
        });
        let env = installed.env();
        let mut line = |t: &str| progress(Progress::Line { text: t.to_string() });
        let _ = fs::remove_dir_all(l.venv());
        runner.run(
            &Cmd::new(l.python())
                .args(["-m", "venv"])
                .arg(l.venv().display().to_string())
                .envs(&env),
            &mut line,
        )?;
        runner.run(
            &Cmd::new(l.venv_python())
                .args(["-m", "pip", "install", "--quiet"])
                .args(manifest.python.bootstrap.clone())
                .envs(&env),
            &mut line,
        )?;
        // A fresh token: workspaces reinstall their Python requirements into a new venv.
        let token = format!("{:?}", std::time::SystemTime::now());
        fs::write(l.marker("venv"), token).map_err(io(&l.root))?;
        progress(Progress::Done {
            component: "Python packages".into(),
            freed: 0,
        });
    }
    Ok(installed)
}

/// What's installed, for the UI ("Build tools: ready / 3 of 6 parts installed").
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Status {
    pub host: Option<Host>,
    pub root: PathBuf,
    pub done: Vec<String>,
    pub missing: Vec<String>,
    pub ready: bool,
    /// Bytes on disk under the toolchain root.
    pub size: u64,
}

pub fn status(data: &Path, manifest: &Manifest) -> Status {
    let host = Host::current();
    let root = data.join("toolchain").join(&manifest.id);
    let mut done = Vec::new();
    let mut missing = Vec::new();
    if let Some(h) = host {
        let l = Layout::new(data, manifest, h);
        let mut names: Vec<(String, String)> = Component::ALL
            .iter()
            .map(|c| {
                (
                    serde_json::to_value(c)
                        .ok()
                        .and_then(|v| v.as_str().map(str::to_string))
                        .unwrap_or_default(),
                    c.label().to_string(),
                )
            })
            .collect();
        names.push(("venv".into(), "Python packages".into()));
        for (key, label) in names {
            if l.is_done(&key) {
                done.push(label);
            } else {
                missing.push(label);
            }
        }
    }
    let ready = host.is_some() && missing.is_empty();
    let size = if root.exists() { trim::size_of(&root) } else { 0 };
    Status {
        host,
        root,
        done,
        missing,
        ready,
        size,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn layout_paths_per_os() {
        let m = Manifest::embedded();
        let lw = Layout::new(Path::new("/data"), &m, Host::WindowsX86_64);
        assert!(lw.venv_python().ends_with("venv/Scripts/python.exe"));
        assert!(lw.python().ends_with("python/python.exe"));
        let ll = Layout::new(Path::new("/data"), &m, Host::LinuxX86_64);
        assert!(ll.west().display().ends_with("venv/bin/python -m west"));
        assert_eq!(ll.root, Path::new("/data/toolchain").join(&m.id));
    }

    #[test]
    fn env_puts_our_tools_first() {
        let m = Manifest::embedded();
        let i = Installed {
            layout: Layout::new(Path::new("/d"), &m, Host::LinuxX86_64),
            git_dir: None,
        };
        let env = i.env();
        assert!(env["PATH"].starts_with(&format!("/d/toolchain/{}/venv/bin:", m.id)));
        assert!(env["ZEPHYR_SDK_INSTALL_DIR"].ends_with("zephyr-sdk"));
    }

    #[test]
    fn status_of_empty_dir() {
        let s = status(Path::new("/definitely/not/here"), &Manifest::embedded());
        assert!(!s.ready);
        assert_eq!(s.missing.len(), 6);
        assert_eq!(s.size, 0);
    }

    #[test]
    fn file_names() {
        assert_eq!(
            file_name("https://x/releases/download/20261110/cpython-3.12.16%2B20261110-x86_64-unknown-linux-gnu-install_only_stripped.tar.gz"),
            "cpython-3.12.16+20261110-x86_64-unknown-linux-gnu-install_only_stripped.tar.gz"
        );
    }
}
