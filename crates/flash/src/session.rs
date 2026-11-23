//! The guided flashing flow as a pure state machine.
//!
//! The UI (or a test) feeds it the current list of bootloader drives every half second
//! ([`Session::on_drives`]) and the result of copies it asked for
//! ([`Session::on_copy_done`]); it answers with at most one [`Action`] and exposes a
//! [`View`] with plain-language instructions. No I/O happens here.
//!
//! Order: optional settings reset (left, right) → left firmware → right firmware. Both
//! halves present the same kind of drive, so we can't tell them apart; the flow relies
//! on the user plugging in the half it asks for, and on the drive disappearing between
//! steps (the bootloader resets after a successful copy).

use std::path::{Path, PathBuf};

use mxkeys_hardware::catalog::{ControllerId, Side};
use mxkeys_probe::Uf2Drive;
use serde::Serialize;

use crate::copy::CopyOutcome;
use crate::uf2;
use crate::verify::{self, Mismatch};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", content = "side", rename_all = "snake_case")]
pub enum Job {
    /// Flash `settings_reset.uf2` (clears pairings and saved settings).
    Reset(Side),
    /// Flash the real firmware.
    Firmware(Side),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Step {
    pub job: Job,
    pub file: PathBuf,
}

/// Firmware files from a build.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Firmware {
    pub left: PathBuf,
    pub right: PathBuf,
    pub settings_reset: PathBuf,
}

impl Firmware {
    /// The files `mxkeys-toolchain` writes: `<dir>/{left,right,settings_reset}.uf2`.
    pub fn in_dir(dir: &Path) -> Firmware {
        Firmware {
            left: dir.join("left.uf2"),
            right: dir.join("right.uf2"),
            settings_reset: dir.join("settings_reset.uf2"),
        }
    }
}

/// The steps for a run. With `reset`, both halves get the settings-reset firmware first
/// (ZMK's documented procedure), then the real firmware.
pub fn plan(fw: &Firmware, reset: bool) -> Vec<Step> {
    let mut steps = Vec::new();
    if reset {
        steps.push(Step {
            job: Job::Reset(Side::Left),
            file: fw.settings_reset.clone(),
        });
        steps.push(Step {
            job: Job::Reset(Side::Right),
            file: fw.settings_reset.clone(),
        });
    }
    steps.push(Step {
        job: Job::Firmware(Side::Left),
        file: fw.left.clone(),
    });
    steps.push(Step {
        job: Job::Firmware(Side::Right),
        file: fw.right.clone(),
    });
    steps
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "phase", rename_all = "snake_case")]
pub enum Phase {
    /// Waiting for the half to show up in bootloader mode.
    WaitingForDrive,
    /// More than one bootloader drive is connected.
    TooManyDrives,
    /// The connected drive failed verification; waiting for it to go away.
    Refused {
        reason: String,
        drive: PathBuf,
    },
    Copying {
        drive: PathBuf,
    },
    /// Copied; waiting for the bootloader to restart (drive disappears).
    Restarting {
        drive: PathBuf,
        polls: u32,
    },
    /// Copy failed; user can retry.
    Failed {
        reason: String,
    },
    Finished,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum Action {
    Copy { drive: PathBuf, file: PathBuf },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum StepState {
    Pending,
    Current,
    Done,
    Skipped,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct StepView {
    pub title: String,
    pub state: StepState,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct View {
    pub steps: Vec<StepView>,
    pub current: Option<usize>,
    pub phase: Phase,
    /// What the user should do now.
    pub instruction: String,
    /// Extra explanation (why, what to expect, or what went wrong).
    pub detail: Option<String>,
}

#[derive(Debug, Clone)]
pub struct Session {
    steps: Vec<Step>,
    states: Vec<StepState>,
    index: usize,
    phase: Phase,
    controller: ControllerId,
}

/// Polls (≈0.5 s each) after which a drive that hasn't restarted earns a hint.
const STUCK_AFTER: u32 = 20;

fn side_name(s: Side) -> &'static str {
    match s {
        Side::Left => "left",
        Side::Right => "right",
    }
}

impl Session {
    /// Validate every firmware file up front (exists, is UF2, right chip family).
    pub fn new(steps: Vec<Step>, controller: ControllerId) -> Result<Session, (PathBuf, Mismatch)> {
        for s in &steps {
            let data =
                std::fs::read(&s.file).map_err(|e| (s.file.clone(), Mismatch::BadFile(e.to_string())))?;
            let summary =
                uf2::inspect(&data).map_err(|e| (s.file.clone(), Mismatch::BadFile(e.to_string())))?;
            verify::check_firmware(&summary, controller).map_err(|m| (s.file.clone(), m))?;
        }
        let mut states = vec![StepState::Pending; steps.len()];
        if let Some(first) = states.first_mut() {
            *first = StepState::Current;
        }
        let phase = if steps.is_empty() {
            Phase::Finished
        } else {
            Phase::WaitingForDrive
        };
        Ok(Session {
            steps,
            states,
            index: 0,
            phase,
            controller,
        })
    }

    pub fn phase(&self) -> &Phase {
        &self.phase
    }

    pub fn is_finished(&self) -> bool {
        self.phase == Phase::Finished
    }

    fn advance(&mut self, how: StepState) {
        self.states[self.index] = how;
        self.index += 1;
        if self.index >= self.steps.len() {
            self.phase = Phase::Finished;
        } else {
            self.states[self.index] = StepState::Current;
            self.phase = Phase::WaitingForDrive;
        }
    }

    /// Feed the currently mounted bootloader drives. Returns an action to perform.
    pub fn on_drives(&mut self, drives: &[Uf2Drive]) -> Option<Action> {
        match self.phase.clone() {
            Phase::WaitingForDrive | Phase::TooManyDrives => match drives {
                [] => {
                    self.phase = Phase::WaitingForDrive;
                    None
                }
                [d] => match verify::check_drive(&d.info, self.controller) {
                    Ok(()) => {
                        self.phase = Phase::Copying {
                            drive: d.path.clone(),
                        };
                        Some(Action::Copy {
                            drive: d.path.clone(),
                            file: self.steps[self.index].file.clone(),
                        })
                    }
                    Err(m) => {
                        self.phase = Phase::Refused {
                            reason: m.to_string(),
                            drive: d.path.clone(),
                        };
                        None
                    }
                },
                _ => {
                    self.phase = Phase::TooManyDrives;
                    None
                }
            },
            Phase::Refused { drive, .. } => {
                if !drives.iter().any(|d| d.path == drive) {
                    self.phase = Phase::WaitingForDrive;
                }
                None
            }
            Phase::Restarting { drive, polls } => {
                if drives.iter().any(|d| d.path == drive) {
                    self.phase = Phase::Restarting {
                        drive,
                        polls: polls + 1,
                    };
                } else {
                    self.advance(StepState::Done);
                }
                None
            }
            Phase::Copying { .. } | Phase::Failed { .. } | Phase::Finished => None,
        }
    }

    /// Report the result of an [`Action::Copy`].
    pub fn on_copy_done(&mut self, result: Result<CopyOutcome, String>) {
        let Phase::Copying { drive } = self.phase.clone() else {
            return;
        };
        self.phase = match result {
            Ok(_) => Phase::Restarting { drive, polls: 0 },
            Err(reason) => Phase::Failed { reason },
        };
    }

    /// Try the current step again (after a failure or a refused drive).
    pub fn retry(&mut self) {
        if matches!(self.phase, Phase::Failed { .. } | Phase::Refused { .. }) {
            self.phase = Phase::WaitingForDrive;
        }
    }

    /// Skip the current step (e.g. the user already flashed that half).
    pub fn skip(&mut self) {
        if !self.is_finished() && !matches!(self.phase, Phase::Copying { .. }) {
            self.advance(StepState::Skipped);
        }
    }

    fn title(job: Job) -> String {
        match job {
            Job::Reset(s) => format!("Reset the {} half", side_name(s)),
            Job::Firmware(s) => format!("Install firmware on the {} half", side_name(s)),
        }
    }

    pub fn view(&self) -> View {
        let steps = self
            .steps
            .iter()
            .zip(&self.states)
            .map(|(s, st)| StepView {
                title: Session::title(s.job),
                state: *st,
            })
            .collect();
        let job = self.steps.get(self.index).map(|s| s.job);
        let side = match job {
            Some(Job::Reset(s) | Job::Firmware(s)) => side_name(s),
            None => "",
        };
        let other = if side == "left" { "right" } else { "left" };
        let (instruction, detail) = match (&self.phase, job) {
            (Phase::Finished, _) => (
                "All done!".to_string(),
                Some("Unplug and replug the halves (or switch them off and on). They should find each other within a few seconds.".to_string()),
            ),
            (Phase::WaitingForDrive, Some(j)) => (
                format!(
                    "Plug the {side} half into this computer with USB (unplug the {other} half), then quickly press its reset button twice."
                ),
                Some(match j {
                    Job::Reset(_) => "This clears saved settings and Bluetooth pairings so both halves start fresh. You'll pair your computers again afterwards.".to_string(),
                    Job::Firmware(_) => "A drive called something like NICENANO appears for a moment — we'll notice it and copy the firmware automatically.".to_string(),
                }),
            ),
            (Phase::TooManyDrives, _) => (
                "More than one keyboard half is in update mode.".to_string(),
                Some(format!("Unplug everything except the {side} half.")),
            ),
            (Phase::Refused { reason, .. }, _) => (
                "This isn't the right device.".to_string(),
                Some(format!("{reason} Unplug it, then plug in the {side} half and press its reset button twice.")),
            ),
            (Phase::Copying { .. }, _) => (format!("Copying firmware to the {side} half…"), Some("Don't unplug it.".to_string())),
            (Phase::Restarting { polls, .. }, _) => (
                format!("The {side} half is restarting…"),
                (*polls >= STUCK_AFTER).then(|| {
                    "The drive is still there. If this doesn't change, unplug the half, plug it in again and double-tap reset to retry.".to_string()
                }),
            ),
            (Phase::Failed { reason }, _) => ("Copying didn't finish.".to_string(), Some(reason.clone())),
            (_, None) => (String::new(), None),
        };
        View {
            steps,
            current: (!self.is_finished()).then_some(self.index),
            phase: self.phase.clone(),
            instruction,
            detail,
        }
    }
}
