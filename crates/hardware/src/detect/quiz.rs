//! The guided fallback: a few visual questions when automatic checks are inconclusive.

use serde::{Deserialize, Serialize};

use super::{Confidence, Evidence, Fact, Findings, Known, Source};
use crate::catalog::{self, ControllerId, DisplayKind, KeyboardId};

/// Answers from the UI. `None` = not asked / "I'm not sure".
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct QuizAnswers {
    pub keyboard: Option<KeyboardId>,
    pub controller: Option<ControllerId>,
    pub display: Option<DisplayKind>,
}

pub fn from_quiz(a: &QuizAnswers) -> Findings {
    let mut f = Findings::default();
    if let Some(k) = &a.keyboard {
        f.evidence.push(Evidence::new(
            Source::Quiz,
            Fact::Keyboard(Known::Known(k.clone())),
            Confidence::Medium,
            format!("you said it's a {}", catalog::keyboard(k).name),
        ));
    }
    if let Some(c) = a.controller {
        f.evidence.push(Evidence::new(
            Source::Quiz,
            Fact::Controller(Known::Known(c)),
            Confidence::Medium,
            format!(
                "you said your controller looks like a {}",
                catalog::controller(c).name
            ),
        ));
    }
    if let Some(d) = a.display {
        f.evidence.push(Evidence::new(
            Source::Quiz,
            Fact::Display(d),
            Confidence::Medium,
            match d {
                DisplayKind::None => "you said there's no screen".to_string(),
                d => format!("you said your screen looks like a {}", catalog::display(d).name),
            },
        ));
    }
    f
}

/// One question for the UI, with options drawn from the catalog.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct QuizQuestion {
    pub id: &'static str,
    pub prompt: &'static str,
    pub options: Vec<QuizOption>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct QuizOption {
    /// Serialised id (matches the `QuizAnswers` field's enum values).
    pub value: String,
    pub label: &'static str,
    pub looks_like: &'static str,
    pub supported: bool,
}

/// The snake_case name of a unit enum variant (e.g. `nice_nano_v2`).
fn id<T: Serialize>(v: &T) -> String {
    match serde_json::to_value(v) {
        Ok(serde_json::Value::String(s)) => s,
        other => panic!("quiz ids must be unit enum variants, got {other:?}"),
    }
}

pub fn questions() -> Vec<QuizQuestion> {
    vec![
        QuizQuestion {
            id: "controller",
            prompt: "Which of these looks like the small board on the back of your keyboard (where the USB cable plugs in)?",
            options: catalog::CONTROLLERS
                .iter()
                .map(|c| QuizOption { value: id(&c.id), label: c.name, looks_like: c.looks_like, supported: c.supported })
                .collect(),
        },
        QuizQuestion {
            id: "display",
            prompt: "Does your keyboard have a screen? Which one looks like yours?",
            options: catalog::DISPLAYS
                .iter()
                .map(|d| QuizOption { value: id(&d.kind), label: d.name, looks_like: d.looks_like, supported: d.supported })
                .collect(),
        },
    ]
}
