//! The guided flash session: a background loop that polls for bootloader drives every
//! half second, drives `mxkeys_flash::session::Session`, performs the copies, and emits
//! `flash-event`s with the current view.

use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use mxkeys_flash::copy::copy_uf2;
use mxkeys_flash::session::{plan, Action, Firmware, Session, View};
use mxkeys_hardware::HardwareProfile;
use mxkeys_probe::DriveScanner;
use serde::Serialize;

#[derive(Default)]
pub struct Flashing {
    session: Arc<Mutex<Option<Session>>>,
    cancel: Arc<AtomicBool>,
    running: Arc<AtomicBool>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FlashEvent {
    pub view: View,
    /// Bytes copied / total during a copy.
    pub progress: Option<(u64, u64)>,
    /// The session ended (finished or cancelled).
    pub ended: bool,
}

pub fn firmware_for(data: &Path, repo: &Path) -> Firmware {
    Firmware::in_dir(&mxkeys_toolchain::pipeline::firmware_dir(data, repo))
}

/// Prepare a session; error in plain language if the firmware isn't built or is wrong.
pub fn new_session(fw: &Firmware, profile: &HardwareProfile, reset: bool) -> Result<Session, String> {
    for f in [&fw.left, &fw.right] {
        if !f.is_file() {
            return Err("Build the firmware first — there's nothing to install yet.".into());
        }
    }
    if reset && !fw.settings_reset.is_file() {
        return Err("The settings-reset firmware hasn't been built yet. Build the firmware first.".into());
    }
    Session::new(plan(fw, reset), profile.controller).map_err(|(path, m)| format!("{m} ({})", path.display()))
}

impl Flashing {
    pub fn start(
        &self,
        session: Session,
        scanner: impl DriveScanner + Send + 'static,
        emit: impl Fn(FlashEvent) + Send + 'static,
    ) -> Result<View, String> {
        if self.running.swap(true, Ordering::SeqCst) {
            return Err("Flashing is already in progress.".into());
        }
        let first = session.view();
        *self.session.lock().unwrap() = Some(session);
        self.cancel.store(false, Ordering::SeqCst);
        let (sess, cancel, running) = (self.session.clone(), self.cancel.clone(), self.running.clone());
        std::thread::spawn(move || {
            let mut last: Option<View> = None;
            loop {
                if cancel.load(Ordering::SeqCst) {
                    if let Some(s) = sess.lock().unwrap().as_ref() {
                        emit(FlashEvent {
                            view: s.view(),
                            progress: None,
                            ended: true,
                        });
                    }
                    break;
                }
                let drives = scanner.uf2_drives().unwrap_or_default();
                let action = sess.lock().unwrap().as_mut().and_then(|s| s.on_drives(&drives));
                if let Some(Action::Copy { drive, file }) = action {
                    if let Some(s) = sess.lock().unwrap().as_ref() {
                        emit(FlashEvent {
                            view: s.view(),
                            progress: Some((0, 1)),
                            ended: false,
                        });
                    }
                    let view_during = sess.lock().unwrap().as_ref().map(Session::view);
                    let res = copy_uf2(&file, &drive, |done, total| {
                        if let Some(v) = &view_during {
                            emit(FlashEvent {
                                view: v.clone(),
                                progress: Some((done, total)),
                                ended: false,
                            });
                        }
                    });
                    if let Some(s) = sess.lock().unwrap().as_mut() {
                        s.on_copy_done(res.map_err(|e| e.to_string()));
                    }
                }
                let (view, finished) = match sess.lock().unwrap().as_ref() {
                    Some(s) => (s.view(), s.is_finished()),
                    None => break,
                };
                if last.as_ref() != Some(&view) {
                    emit(FlashEvent {
                        view: view.clone(),
                        progress: None,
                        ended: finished,
                    });
                    last = Some(view);
                }
                if finished {
                    break;
                }
                std::thread::sleep(Duration::from_millis(500));
            }
            running.store(false, Ordering::SeqCst);
        });
        Ok(first)
    }

    /// `retry`, `skip` or `cancel`.
    pub fn control(&self, cmd: &str) -> Result<Option<View>, String> {
        if cmd == "cancel" {
            self.cancel.store(true, Ordering::SeqCst);
            return Ok(None);
        }
        let mut guard = self.session.lock().unwrap();
        let s = guard.as_mut().ok_or("No flashing in progress.")?;
        match cmd {
            "retry" => s.retry(),
            "skip" => s.skip(),
            other => return Err(format!("unknown command {other}")),
        }
        Ok(Some(s.view()))
    }

    pub fn running(&self) -> bool {
        self.running.load(Ordering::SeqCst)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mxkeys_flash::uf2::block;
    use mxkeys_probe::FsDriveScanner;
    use std::sync::mpsc;

    #[test]
    fn background_session_flashes_both_halves() {
        let base = std::env::temp_dir().join(format!("mxkeys-flashing-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let fwdir = base.join("fw");
        std::fs::create_dir_all(&fwdir).unwrap();
        for (n, blocks) in [("left", 8), ("right", 6), ("settings_reset", 2)] {
            let data: Vec<u8> = (0..blocks)
                .flat_map(|i| block(0x26000 + i * 256, 0xADA5_2840))
                .collect();
            std::fs::write(fwdir.join(format!("{n}.uf2")), data).unwrap();
        }
        let fw = Firmware::in_dir(&fwdir);
        let profile = HardwareProfile::CORNE_NICE_NANO_V2_NICE_VIEW;
        assert!(
            new_session(&Firmware::in_dir(&base.join("none")), &profile, false)
                .unwrap_err()
                .contains("Build the firmware first")
        );
        let session = new_session(&fw, &profile, false).unwrap();

        let drive = base.join("NICENANO");
        let scanner = FsDriveScanner::with_roots(vec![drive.clone()]);
        let flashing = Flashing::default();
        let (tx, rx) = mpsc::channel();
        flashing
            .start(session, scanner, move |e| {
                let _ = tx.send(e);
            })
            .unwrap();
        assert!(flashing
            .start(
                new_session(&fw, &profile, false).unwrap(),
                FsDriveScanner::with_roots(vec![]),
                |_| {}
            )
            .is_err());

        let wait_for = |pred: &dyn Fn(&FlashEvent) -> bool| {
            let start = std::time::Instant::now();
            loop {
                let e = rx.recv_timeout(Duration::from_secs(10)).expect("flash events");
                if pred(&e) {
                    return e;
                }
                assert!(start.elapsed() < Duration::from_secs(20), "timed out");
            }
        };
        for half in 0..2usize {
            // Double-tap: the drive appears; the session copies, then waits for a restart.
            std::fs::create_dir_all(&drive).unwrap();
            std::fs::write(
                drive.join("INFO_UF2.TXT"),
                "Model: nice!nano\nBoard-ID: nRF52840-nicenano\n",
            )
            .unwrap();
            wait_for(&|e| matches!(e.view.phase, mxkeys_flash::session::Phase::Restarting { .. }));
            assert!(drive.join(mxkeys_flash::copy::TARGET_NAME).is_file());
            // The bootloader restarts: the drive disappears and the next step begins.
            std::fs::remove_dir_all(&drive).unwrap();
            let e = wait_for(&|e| e.ended || e.view.current == Some(half + 1));
            if half == 1 {
                assert!(e.ended);
                assert_eq!(e.view.instruction, "All done!");
            }
        }
        std::fs::remove_dir_all(&base).unwrap();
    }
}
