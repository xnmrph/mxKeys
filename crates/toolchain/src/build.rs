//! Building one firmware target with `west build`.

use std::path::{Path, PathBuf};

use mxkeys_hardware::profile::{BuildTarget, TargetId};
use serde::Serialize;

use crate::install::Installed;
use crate::runner::{CommandRunner, RunError};
use crate::workspace::Workspace;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct BuildPlan {
    pub target: BuildTarget,
    /// The user's `config/` directory (keymap, .conf).
    pub config_dir: PathBuf,
    /// Extra Zephyr modules: the user's repo itself when it has `zephyr/module.yml`
    /// (custom shields/boards), like ZMK's GitHub workflow does.
    pub extra_modules: Vec<PathBuf>,
    pub build_dir: PathBuf,
    /// Start from scratch instead of an incremental build.
    pub pristine: bool,
    /// Only run the configure step (enough for `zephyr.dts`, e.g. the key layout).
    pub cmake_only: bool,
}

impl BuildPlan {
    /// Plan a target for the config repo at `repo`, building in `builds/<target>`.
    pub fn new(target: BuildTarget, repo: &Path, builds: &Path) -> BuildPlan {
        let extra_modules = if repo.join("zephyr").join("module.yml").is_file() {
            vec![repo.to_path_buf()]
        } else {
            Vec::new()
        };
        let name = match target.id {
            TargetId::Left => "left",
            TargetId::Right => "right",
            TargetId::Main => "main",
            TargetId::SettingsReset => "settings_reset",
        };
        BuildPlan {
            build_dir: builds.join(name),
            config_dir: repo.join("config"),
            extra_modules,
            target,
            pristine: false,
            cmake_only: false,
        }
    }

    /// The resolved devicetree of this build (written by the configure step).
    pub fn dts(&self) -> PathBuf {
        self.build_dir.join("zephyr").join("zephyr.dts")
    }

    pub fn uf2(&self) -> PathBuf {
        self.build_dir.join("zephyr").join("zmk.uf2")
    }
}

/// `west build` arguments (pure, so it's tested exactly).
pub fn west_build_args(plan: &BuildPlan, ws: &Workspace) -> Vec<String> {
    let mut a = vec![
        "build".to_string(),
        "-p".to_string(),
        if plan.pristine { "always" } else { "auto" }.to_string(),
        "-s".to_string(),
        ws.zmk_app().display().to_string(),
        "-d".to_string(),
        plan.build_dir.display().to_string(),
        "-b".to_string(),
        plan.target.board.clone(),
    ];
    for s in &plan.target.snippets {
        a.push("-S".into());
        a.push(s.clone());
    }
    if plan.cmake_only {
        a.push("--cmake-only".into());
    }
    a.push("--".into());
    // Found via an explicit path instead of `west zephyr-export`, which would write to the
    // user's global CMake package registry.
    a.push(format!("-DZephyr_DIR={}", ws.zephyr_cmake_dir().display()));
    if !plan.target.shields.is_empty() {
        a.push(format!("-DSHIELD={}", plan.target.shields.join(" ")));
    }
    a.push(format!("-DZMK_CONFIG={}", plan.config_dir.display()));
    if !plan.extra_modules.is_empty() {
        let mods: Vec<String> = plan
            .extra_modules
            .iter()
            .map(|m| m.display().to_string())
            .collect();
        a.push(format!("-DZMK_EXTRA_MODULES={}", mods.join(";")));
    }
    a
}

#[derive(Debug, thiserror::Error)]
pub enum BuildError {
    #[error(transparent)]
    Run(#[from] RunError),
    #[error("the build finished but no firmware file was produced at {0}")]
    NoOutput(PathBuf),
}

/// Run the build; every output line goes to `on_line`. Returns the `.uf2` path (or the
/// `zephyr.dts` path for a configure-only run).
pub fn build(
    plan: &BuildPlan,
    ws: &Workspace,
    tools: &Installed,
    runner: &dyn CommandRunner,
    on_line: &mut dyn FnMut(&str),
) -> Result<PathBuf, BuildError> {
    let cmd = tools
        .layout
        .west()
        .args(west_build_args(plan, ws))
        .cwd(&ws.dir)
        .envs(&tools.env());
    on_line(&format!("$ {}", cmd.display()));
    runner.run(&cmd, on_line)?;
    if plan.cmake_only {
        let dts = plan.dts();
        return if dts.is_file() {
            Ok(dts)
        } else {
            Err(BuildError::NoOutput(dts))
        };
    }
    let uf2 = plan.uf2();
    if uf2.is_file() {
        Ok(uf2)
    } else {
        Err(BuildError::NoOutput(uf2))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mxkeys_hardware::HardwareProfile;

    #[test]
    fn args_for_the_v1_profile() {
        let t = HardwareProfile::corne_nice_nano_v2_nice_view().build_targets();
        let ws = Workspace {
            dir: PathBuf::from("/ws"),
            manifest_text: String::new(),
        };
        let repo = Path::new("/repo");
        let left = BuildPlan::new(t[0].clone(), repo, Path::new("/builds"));
        let args = west_build_args(&left, &ws);
        // Separator-independent checks (paths use `\` on Windows).
        assert_eq!(args[..3], ["build", "-p", "auto"]);
        assert_eq!(
            args[7..12],
            ["-b", "nice_nano@2.0.0//zmk", "-S", "studio-rpc-usb-uart", "--"]
        );
        assert!(args.contains(&"-DSHIELD=corne_left nice_view_adapter nice_view".to_string()));
        assert!(
            !args.iter().any(|a| a.starts_with("-DZMK_EXTRA_MODULES")),
            "no zephyr/module.yml in /repo"
        );
        #[cfg(unix)]
        assert_eq!(
            args,
            [
                "build",
                "-p",
                "auto",
                "-s",
                "/ws/zmk/app",
                "-d",
                "/builds/left",
                "-b",
                "nice_nano@2.0.0//zmk",
                "-S",
                "studio-rpc-usb-uart",
                "--",
                "-DZephyr_DIR=/ws/zephyr/share/zephyr-package/cmake",
                "-DSHIELD=corne_left nice_view_adapter nice_view",
                "-DZMK_CONFIG=/repo/config",
            ]
        );
        assert_eq!(
            left.uf2(),
            Path::new("/builds/left").join("zephyr").join("zmk.uf2")
        );
        let reset = BuildPlan::new(t[2].clone(), repo, Path::new("/builds"));
        let a = west_build_args(&reset, &ws);
        assert!(a.contains(&"-DSHIELD=settings_reset".to_string()));
        assert!(!a.contains(&"-S".to_string()));
    }

    #[test]
    fn repo_with_module_yml_is_an_extra_module() {
        let repo = std::env::temp_dir().join(format!("mxkeys-plan-{}", std::process::id()));
        std::fs::create_dir_all(repo.join("zephyr")).unwrap();
        std::fs::write(repo.join("zephyr/module.yml"), "build: {}\n").unwrap();
        let t = HardwareProfile::corne_nice_nano_v2_nice_view().build_targets();
        let plan = BuildPlan::new(t[1].clone(), &repo, Path::new("/b"));
        assert_eq!(plan.extra_modules, std::slice::from_ref(&repo));
        let ws = Workspace {
            dir: PathBuf::from("/ws"),
            manifest_text: String::new(),
        };
        assert!(west_build_args(&plan, &ws)
            .last()
            .unwrap()
            .starts_with("-DZMK_EXTRA_MODULES="));
        std::fs::remove_dir_all(&repo).unwrap();
    }
}
