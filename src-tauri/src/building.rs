//! Toolchain setup and firmware builds, run on background threads with events streamed
//! to the UI (`toolchain-event`, `build-event`).

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use mxkeys_hardware::profile::TargetId;
use mxkeys_hardware::HardwareProfile;
use mxkeys_toolchain::install::{self, Progress, Status};
use mxkeys_toolchain::manifest::Manifest;
use mxkeys_toolchain::pipeline::{self, Event, Options, Outcome};
use mxkeys_toolchain::runner::ProcessRunner;
use serde::Serialize;

#[derive(Default)]
pub struct Jobs {
    pub toolchain: Arc<AtomicBool>,
    pub build: Arc<AtomicBool>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolchainInfo {
    pub status: Status,
    pub sdk_version: String,
    pub running: bool,
}

pub fn info(data: &std::path::Path, jobs: &Jobs) -> ToolchainInfo {
    let m = Manifest::embedded();
    ToolchainInfo {
        status: install::status(data, &m),
        sdk_version: m.zephyr_sdk.version.clone(),
        running: jobs.toolchain.load(Ordering::SeqCst),
    }
}

/// Messages for the frontend. `Finished` closes a run.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ToolchainEvent {
    Progress(Progress),
    Finished { ok: bool, error: Option<String> },
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum BuildEvent {
    Event(Event),
    Finished {
        outcome: Option<Outcome>,
        error: Option<String>,
    },
}

/// Clears the flag when the job ends, even on panic.
struct Running(Arc<AtomicBool>);
impl Drop for Running {
    fn drop(&mut self) {
        self.0.store(false, Ordering::SeqCst);
    }
}

fn claim(flag: &Arc<AtomicBool>) -> Option<Running> {
    flag.compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
        .ok()
        .map(|_| Running(flag.clone()))
}

/// Start installing the toolchain in the background. Returns false if already running.
pub fn start_setup(data: PathBuf, jobs: &Jobs, emit: impl Fn(ToolchainEvent) + Send + 'static) -> bool {
    let Some(guard) = claim(&jobs.toolchain) else {
        return false;
    };
    std::thread::spawn(move || {
        let _guard = guard;
        let m = Manifest::embedded();
        let res = install::install(&data, &m, &ProcessRunner, &mut |p| {
            emit(ToolchainEvent::Progress(p))
        });
        emit(ToolchainEvent::Finished {
            ok: res.is_ok(),
            error: res.err().map(|e| e.to_string()),
        });
    });
    true
}

pub struct BuildRequest {
    pub data: PathBuf,
    pub repo: PathBuf,
    pub profile: Option<HardwareProfile>,
    pub only: Vec<TargetId>,
    pub update: bool,
    pub pristine: bool,
}

/// Start a build in the background. Returns false if a build is already running.
pub fn start_build(req: BuildRequest, jobs: &Jobs, emit: impl Fn(BuildEvent) + Send + 'static) -> bool {
    let Some(guard) = claim(&jobs.build) else {
        return false;
    };
    std::thread::spawn(move || {
        let _guard = guard;
        let m = Manifest::embedded();
        let opts = Options {
            data: req.data,
            repo: req.repo,
            profile: req.profile,
            only: req.only,
            update_workspace: req.update,
            full_workspace: false,
            pristine: req.pristine,
        };
        let res = pipeline::run(&opts, &m, &ProcessRunner, &mut |e| emit(BuildEvent::Event(e)));
        match res {
            Ok(o) => emit(BuildEvent::Finished {
                outcome: Some(o),
                error: None,
            }),
            Err(e) => emit(BuildEvent::Finished {
                outcome: None,
                error: Some(e.to_string()),
            }),
        }
    });
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_one_job_at_a_time() {
        let jobs = Jobs::default();
        let a = claim(&jobs.build);
        assert!(a.is_some());
        assert!(claim(&jobs.build).is_none());
        drop(a);
        assert!(claim(&jobs.build).is_some());
    }

    #[test]
    fn info_for_fresh_data_dir() {
        let i = info(std::path::Path::new("/nonexistent/mxkeys"), &Jobs::default());
        assert!(!i.status.ready && !i.running);
        assert_eq!(i.sdk_version, "0.16.9");
    }
}
