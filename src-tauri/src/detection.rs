//! Hardware detection for the UI: config repo + probe snapshot (+ quiz answers) →
//! a report. Pure apart from reading the repo files, so it's tested with mocks.

use std::path::Path;

use mxkeys_hardware::detect::quiz::QuizAnswers;
use mxkeys_hardware::detect::{detect, Detection, Inputs};
use mxkeys_hardware::profile::BuildTarget;
use mxkeys_probe::{Snapshot, Uf2Drive};
use serde::Serialize;
use zmk_config::build_yaml::BuildMatrix;
use zmk_config::kconfig::KconfigFile;
use zmk_config::repo::{self, RepoFiles};

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DetectionReport {
    pub detection: Detection,
    /// Bootloader drives found right now.
    pub drives: Vec<Uf2Drive>,
    /// Things that couldn't be checked (e.g. USB listing not permitted).
    pub probe_errors: Vec<String>,
    /// Firmware the app would build for the detected profile.
    pub targets: Vec<BuildTarget>,
    /// Parts of the detected profile the app can't handle yet.
    pub unsupported: Vec<String>,
}

pub struct RepoInputs {
    pub build: Option<BuildMatrix>,
    pub confs: Vec<KconfigFile>,
}

/// Read build.yaml and `.conf` files; unreadable files are skipped (the repo summary
/// already reports them).
pub fn repo_inputs(root: &Path) -> RepoInputs {
    let Ok(files) = RepoFiles::discover(root) else {
        return RepoInputs {
            build: None,
            confs: Vec::new(),
        };
    };
    RepoInputs {
        build: repo::load_build_matrix(&files).ok().flatten(),
        confs: files
            .confs
            .iter()
            .filter_map(|p| repo::read(p).ok())
            .map(KconfigFile::parse)
            .collect(),
    }
}

pub fn report(repo: &RepoInputs, snap: Snapshot, quiz: Option<&QuizAnswers>) -> DetectionReport {
    let infos: Vec<_> = snap.drives.iter().map(|d| d.info.clone()).collect();
    let detection = detect(&Inputs {
        build: repo.build.as_ref(),
        confs: &repo.confs,
        bootloader_drives: &infos,
        usb: &snap.usb,
        quiz,
    });
    let (targets, unsupported) = match &detection.profile {
        Some(p) => (p.build_targets(), p.unsupported_parts()),
        None => (Vec::new(), Vec::new()),
    };
    DetectionReport {
        detection,
        drives: snap.drives,
        probe_errors: snap.errors,
        targets,
        unsupported,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mxkeys_hardware::detect::uf2::parse_info_uf2;
    use mxkeys_probe::{snapshot, MockDrives, MockUsb};
    use std::path::PathBuf;

    #[test]
    fn owner_repo_plus_bootloader_drive() {
        let fixtures = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../crates/zmk-config/tests/fixtures");
        let dir = std::env::temp_dir().join(format!("mxkeys-detect-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("config")).unwrap();
        std::fs::copy(fixtures.join("user_build.yaml"), dir.join("build.yaml")).unwrap();
        std::fs::copy(fixtures.join("xnmrph_corne.conf"), dir.join("config/corne.conf")).unwrap();

        let drives = MockDrives(vec![Uf2Drive {
            path: PathBuf::from("/run/media/me/NICENANO"),
            info: parse_info_uf2("UF2 Bootloader 0.6.0\nModel: nice!nano\nBoard-ID: nRF52840-nicenano\n"),
        }]);
        let r = report(&repo_inputs(&dir), snapshot(&MockUsb::default(), &drives), None);
        assert_eq!(
            r.detection.summary,
            "Corne · nice!nano v2 · nice!view — detected from build.yaml + bootloader"
        );
        assert_eq!(r.targets.len(), 3);
        assert!(r.unsupported.is_empty());
        assert_eq!(r.drives.len(), 1);
        let json = serde_json::to_value(&r).unwrap();
        assert!(json["detection"]["notes"]
            .as_array()
            .unwrap()
            .iter()
            .any(|n| n["level"] == "warning"));
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
