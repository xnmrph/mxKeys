//! Hardware detection: pure functions from inputs (config files, INFO_UF2.TXT contents,
//! USB descriptor lists, quiz answers) to [`Evidence`], and a combiner that turns
//! evidence into a [`Detection`] with confidence, sources and plain-language conflicts.
//!
//! No I/O happens here — OS probing lives in `mxkeys-probe` and feeds these functions.

mod combine;
pub mod quiz;
pub mod repo;
pub mod uf2;
pub mod usb;

use serde::Serialize;

use crate::catalog::{ControllerId, DisplayKind, KeyboardId, Side};

pub use combine::{combine, Detection, FieldResult, Question};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Source {
    /// The config files say so (`config/*.conf`).
    Settings,
    /// `build.yaml` builds firmware for it.
    BuildYaml,
    /// The user answered the quiz.
    Quiz,
    /// USB device list.
    Usb,
    /// INFO_UF2.TXT on the bootloader drive.
    Bootloader,
    /// ZMK Studio RPC (Phase 10).
    LiveConnection,
}

impl Source {
    /// How much this source knows about the *physical* hardware (higher = more).
    /// What the keyboard reports beats what the user saw beats what the config builds.
    pub fn trust(self) -> u8 {
        match self {
            Source::Settings => 0,
            Source::BuildYaml => 1,
            Source::Quiz => 2,
            Source::Usb => 3,
            Source::Bootloader => 4,
            Source::LiveConnection => 5,
        }
    }

    /// Reported by the keyboard itself (not config or the user).
    pub fn is_hardware(self) -> bool {
        matches!(self, Source::Usb | Source::Bootloader | Source::LiveConnection)
    }

    /// For sentences: "according to {phrase}".
    pub fn phrase(self) -> &'static str {
        match self {
            Source::Settings => "your settings",
            Source::BuildYaml => "your build.yaml",
            Source::Quiz => "your answers",
            Source::Usb => "the connected USB devices",
            Source::Bootloader => "the bootloader drive",
            Source::LiveConnection => "the live connection",
        }
    }

    /// Short label for summaries: "detected from build.yaml + bootloader".
    pub fn label(self) -> &'static str {
        match self {
            Source::Settings => "your settings",
            Source::BuildYaml => "build.yaml",
            Source::Quiz => "your answers",
            Source::Usb => "USB",
            Source::Bootloader => "bootloader",
            Source::LiveConnection => "live connection",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Confidence {
    Weak,
    Medium,
    Strong,
}

impl Confidence {
    pub fn bump(self) -> Confidence {
        match self {
            Confidence::Weak => Confidence::Medium,
            _ => Confidence::Strong,
        }
    }
}

/// A keyboard or controller we either know, or saw by name but don't know.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize)]
#[serde(tag = "type", content = "id", rename_all = "snake_case")]
pub enum Known<T> {
    Known(T),
    Other(String),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "field", content = "value", rename_all = "snake_case")]
pub enum Fact {
    Keyboard(Known<KeyboardId>),
    Controller(Known<ControllerId>),
    Display(DisplayKind),
    Central(Side),
    Studio(bool),
    /// `CONFIG_ZMK_DISPLAY` — whether the screen software is switched on (not a hardware
    /// fact; used for warnings).
    ScreenSoftware(bool),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Evidence {
    pub source: Source,
    pub fact: Fact,
    pub confidence: Confidence,
    /// Plain-language reason, e.g. "build.yaml builds corne_left and corne_right".
    pub because: String,
}

impl Evidence {
    pub fn new(source: Source, fact: Fact, confidence: Confidence, because: impl Into<String>) -> Self {
        Evidence {
            source,
            fact,
            confidence,
            because: because.into(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum NoteLevel {
    Info,
    Warning,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Note {
    pub level: NoteLevel,
    pub text: String,
    /// A repair the app can make for this note (e.g. `"build_yaml_zmk_variant"`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fix: Option<&'static str>,
}

impl Note {
    pub fn info(text: impl Into<String>) -> Self {
        Note {
            level: NoteLevel::Info,
            text: text.into(),
            fix: None,
        }
    }
    pub fn warning(text: impl Into<String>) -> Self {
        Note {
            level: NoteLevel::Warning,
            text: text.into(),
            fix: None,
        }
    }
    pub fn with_fix(mut self, fix: &'static str) -> Self {
        self.fix = Some(fix);
        self
    }
}

/// What a detector returns.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct Findings {
    pub evidence: Vec<Evidence>,
    pub notes: Vec<Note>,
}

impl Findings {
    pub fn extend(&mut self, other: Findings) {
        self.evidence.extend(other.evidence);
        self.notes.extend(other.notes);
    }
}

/// Everything detection can look at. Any part may be missing.
#[derive(Debug, Clone, Default)]
pub struct Inputs<'a> {
    pub build: Option<&'a zmk_config::build_yaml::BuildMatrix>,
    pub confs: &'a [zmk_config::kconfig::KconfigFile],
    /// INFO_UF2.TXT of every bootloader drive currently mounted.
    pub bootloader_drives: &'a [uf2::Uf2Info],
    pub usb: &'a [usb::UsbDevice],
    pub quiz: Option<&'a quiz::QuizAnswers>,
}

/// Run every detector and combine the results.
pub fn detect(inputs: &Inputs<'_>) -> Detection {
    let mut all = Findings::default();
    if let Some(b) = inputs.build {
        all.extend(repo::from_build_yaml(b));
    }
    let nice_view_in_build = inputs
        .build
        .is_some_and(|b| b.targets.iter().any(|t| t.has_nice_view()));
    for c in inputs.confs {
        all.extend(repo::from_conf(c, nice_view_in_build));
    }
    for d in inputs.bootloader_drives {
        all.extend(uf2::from_uf2(d));
    }
    all.extend(usb::from_usb(inputs.usb));
    if let Some(q) = inputs.quiz {
        all.extend(quiz::from_quiz(q));
    }
    combine(&all.evidence, all.notes)
}
