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

use mxkeys_hardware::catalog::ControllerId;
use mxkeys_probe::Uf2Drive;
use serde::Serialize;

use crate::copy::CopyOutcome;
use crate::uf2;
use crate::verify::{self, Mismatch};

/// What gets flashed: a half of a split keyboard, or a one-piece keyboard.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Part {
    Left,
    Right,
    Whole,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", content = "side", rename_all = "snake_case")]
pub enum Job {
    /// Flash `settings_reset.uf2` (clears pairings and saved settings).
    Reset(Part),
    /// Flash the real firmware.
    Firmware(Part),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Step {
    pub job: Job,
    pub file: PathBuf,
}

/// Firmware files from a build.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Firmware {
    /// Left then right for a split keyboard, or the one firmware of a one-piece board.
    pub parts: Vec<(Part, PathBuf)>,
    pub settings_reset: PathBuf,
}

impl Firmware {
    /// The files `mxkeys-toolchain` writes: `<dir>/{left,right,main,settings_reset}.uf2`.
    pub fn in_dir(dir: &Path, split: bool) -> Firmware {
        let parts = if split {
            vec![
                (Part::Left, dir.join("left.uf2")),
                (Part::Right, dir.join("right.uf2")),
            ]
        } else {
            vec![(Part::Whole, dir.join("main.uf2"))]
        };
        Firmware {
            parts,
            settings_reset: dir.join("settings_reset.uf2"),
        }
    }
}

/// The steps for a run. With `reset`, every part gets the settings-reset firmware first
/// (ZMK's documented procedure), then the real firmware.
pub fn plan(fw: &Firmware, reset: bool) -> Vec<Step> {
    let mut steps = Vec::new();
    if reset {
        for (p, _) in &fw.parts {
            steps.push(Step {
                job: Job::Reset(*p),
                file: fw.settings_reset.clone(),
            });
        }
    }
    for (p, f) in &fw.parts {
        steps.push(Step {
            job: Job::Firmware(*p),
            file: f.clone(),
        });
    }
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

/// "the left half", "the keyboard".
fn part_name(p: Part) -> &'static str {
    match p {
        Part::Left => "the left half",
        Part::Right => "the right half",
        Part::Whole => "the keyboard",
    }
}

fn capitalize(s: &str) -> String {
    let mut c = s.chars();
    c.next().map_or(String::new(), |f| {
        f.to_uppercase().collect::<String>() + c.as_str()
    })
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
            Job::Reset(p) => format!("Reset {}", part_name(p)),
            Job::Firmware(p) => format!("Install firmware on {}", part_name(p)),
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
        let part = match job {
            Some(Job::Reset(p) | Job::Firmware(p)) => Some(p),
            None => None,
        };
        let side = part.map_or("", part_name);
        let unplug_other = match part {
            Some(Part::Left) => " (unplug the right half)",
            Some(Part::Right) => " (unplug the left half)",
            _ => "",
        };
        let split = self
            .steps
            .iter()
            .any(|s| matches!(s.job, Job::Firmware(Part::Left | Part::Right)));
        let (instruction, detail) = match (&self.phase, job) {
            (Phase::Finished, _) => (
                "All done!".to_string(),
                Some(if split {
                    "Unplug and replug the halves (or switch them off and on). They should find each other within a few seconds.".to_string()
                } else {
                    "Unplug and replug the keyboard (or switch it off and on).".to_string()
                }),
            ),
            (Phase::WaitingForDrive, Some(j)) => (
                format!(
                    "Plug {side} into this computer with USB{unplug_other}, then quickly press its reset button twice."
                ),
                Some(match j {
                    Job::Reset(_) => format!("This clears saved settings and Bluetooth pairings so {} start fresh. You'll pair your computers again afterwards.", if split { "both halves" } else { "the keyboard can" }),
                    Job::Firmware(_) => "A drive called something like NICENANO appears for a moment — we'll notice it and copy the firmware automatically.".to_string(),
                }),
            ),
            (Phase::TooManyDrives, _) => (
                "More than one keyboard is in update mode.".to_string(),
                Some(format!("Unplug everything except {side}.")),
            ),
            (Phase::Refused { reason, .. }, _) => (
                "This isn't the right device.".to_string(),
                Some(format!("{reason} Unplug it, then plug in {side} and press its reset button twice.")),
            ),
            (Phase::Copying { .. }, _) => (format!("Copying firmware to {side}…"), Some("Don't unplug it.".to_string())),
            (Phase::Restarting { polls, .. }, _) => (
                format!("{} is restarting…", capitalize(side)),
                (*polls >= STUCK_AFTER).then(|| {
                    "The drive is still there. If this doesn't change, unplug it, plug it in again and double-tap reset to retry.".to_string()
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
