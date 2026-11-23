//! Folding evidence into one decision per field, with confidence and explanations.

use serde::Serialize;

use super::{Confidence, Evidence, Fact, Known, Note, Source};
use crate::catalog::{self, ControllerId, DisplayKind, KeyboardId, Side};
use crate::profile::HardwareProfile;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct FieldResult<T> {
    pub value: Option<T>,
    pub confidence: Option<Confidence>,
    /// Sources that support the chosen value.
    pub sources: Vec<Source>,
    /// All evidence about this field (for the "Advanced" view).
    pub evidence: Vec<Evidence>,
    /// Plain-language explanations of disagreements.
    pub conflicts: Vec<String>,
}

impl<T> FieldResult<T> {
    fn empty() -> Self {
        FieldResult {
            value: None,
            confidence: None,
            sources: Vec::new(),
            evidence: Vec::new(),
            conflicts: Vec::new(),
        }
    }
    /// Known with at least medium confidence.
    pub fn is_settled(&self) -> bool {
        self.value.is_some() && self.confidence >= Some(Confidence::Medium)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Detection {
    pub keyboard: FieldResult<Known<KeyboardId>>,
    pub controller: FieldResult<Known<ControllerId>>,
    pub display: FieldResult<DisplayKind>,
    pub central: FieldResult<Side>,
    pub studio: FieldResult<bool>,
    pub notes: Vec<Note>,
    /// "Corne · nice!nano v2 · nice!view — detected from build.yaml + bootloader"
    pub summary: String,
    /// Complete profile if keyboard, controller and display are all known.
    pub profile: Option<HardwareProfile>,
    /// Which questions the guided quiz should ask (unknown or weakly known fields).
    pub ask: Vec<Question>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Question {
    Keyboard,
    Controller,
    Display,
}

fn join_sources(sources: &[Source]) -> String {
    let labels: Vec<&str> = sources.iter().map(|s| s.phrase()).collect();
    match labels.len() {
        0 => String::new(),
        1 => labels[0].to_string(),
        _ => format!(
            "{} and {}",
            labels[..labels.len() - 1].join(", "),
            labels[labels.len() - 1]
        ),
    }
}

fn distinct_sources(ev: &[&Evidence]) -> Vec<Source> {
    let mut s: Vec<Source> = ev.iter().map(|e| e.source).collect();
    s.sort();
    s.dedup();
    s
}

/// Custom wording for a disagreement: (winner, winner sources, loser, loser sources).
type SpecialWording<T> = dyn Fn(&T, &[Source], &T, &[Source]) -> Option<String>;

/// Context for wording a disagreement.
struct Wording<'a, T> {
    describe: &'a dyn Fn(&T) -> String,
    special: &'a SpecialWording<T>,
}

fn decide<T: Clone + PartialEq>(items: Vec<(T, &Evidence)>, wording: Wording<'_, T>) -> FieldResult<T> {
    let mut out = FieldResult::empty();
    out.evidence = items.iter().map(|(_, e)| (*e).clone()).collect();
    let mut groups: Vec<(T, Vec<&Evidence>)> = Vec::new();
    for (v, e) in items {
        match groups.iter_mut().find(|(g, _)| *g == v) {
            Some((_, es)) => es.push(e),
            None => groups.push((v, vec![e])),
        }
    }
    let key = |es: &Vec<&Evidence>| {
        (
            es.iter().map(|e| e.confidence).max(),
            es.iter().map(|e| e.source.trust()).max(),
            distinct_sources(es).len(),
        )
    };
    groups.sort_by_key(|g| std::cmp::Reverse(key(&g.1)));
    let Some((winner, wes)) = groups.first() else {
        return out;
    };
    let sources = distinct_sources(wes);
    let mut conf = wes.iter().map(|e| e.confidence).max().expect("non-empty group");
    // Independent sources agreeing raise confidence — but only something the keyboard
    // itself reported can make it "strong"; config files describe what you *build*.
    if sources.len() >= 2 {
        let hardware = sources.iter().any(|s| s.is_hardware());
        conf = conf.bump().min(if hardware {
            Confidence::Strong
        } else {
            Confidence::Medium
        });
    }
    for (loser, les) in &groups[1..] {
        let lsrc = distinct_sources(les);
        let text = (wording.special)(winner, &sources, loser, &lsrc).unwrap_or_else(|| {
            let best = sources
                .iter()
                .max_by_key(|s| s.trust())
                .copied()
                .unwrap_or(Source::BuildYaml);
            let reason = match best {
                Source::Bootloader | Source::Usb | Source::LiveConnection => {
                    "The keyboard itself reports this, so we trust it."
                }
                Source::Quiz => "We're going with what you see on your keyboard.",
                Source::BuildYaml | Source::Settings => "We're going with your config.",
            };
            format!(
                "According to {}, {}, but according to {}, {}. {reason}",
                join_sources(&lsrc),
                (wording.describe)(loser),
                join_sources(&sources),
                (wording.describe)(winner)
            )
        });
        out.conflicts.push(text);
        // A disagreement that is just as confident *and* from an equally (or more)
        // trustworthy source lowers our certainty. build.yaml disagreeing with what the
        // user sees doesn't: it only says what's built, not what's on the desk.
        let winner_trust = sources.iter().map(|s| s.trust()).max();
        let loser_trust = lsrc.iter().map(|s| s.trust()).max();
        if les.iter().map(|e| e.confidence).max() >= Some(conf)
            && loser_trust >= winner_trust
            && conf > Confidence::Weak
        {
            conf = match conf {
                Confidence::Strong => Confidence::Medium,
                _ => Confidence::Weak,
            };
        }
    }
    out.value = Some(winner.clone());
    out.confidence = Some(conf);
    out.sources = sources;
    out
}

fn no_special<T>(_: &T, _: &[Source], _: &T, _: &[Source]) -> Option<String> {
    None
}

fn describe_keyboard(k: &Known<KeyboardId>) -> String {
    match k {
        Known::Known(id) => format!("it's a {}", catalog::keyboard(*id).name),
        Known::Other(name) => format!("it's a keyboard called “{name}”"),
    }
}

fn describe_controller(c: &Known<ControllerId>) -> String {
    match c {
        Known::Known(id) => format!("the controller is a {}", catalog::controller(*id).name),
        Known::Other(name) => format!("the controller is a “{name}”"),
    }
}

fn describe_display(d: &DisplayKind) -> String {
    match d {
        DisplayKind::None => "there's no screen".into(),
        d => format!("there's a {}", catalog::display(*d).name),
    }
}

fn screen_name(d: &DisplayKind) -> String {
    match d {
        DisplayKind::None => "no screen".into(),
        d => format!("a {}", catalog::display(*d).name),
    }
}

fn display_special(w: &DisplayKind, ws: &[Source], l: &DisplayKind, ls: &[Source]) -> Option<String> {
    let build_only = ls
        .iter()
        .all(|s| matches!(s, Source::BuildYaml | Source::Settings));
    if !build_only {
        return None;
    }
    Some(match (w, l) {
        (DisplayKind::None, _) => format!(
            "Your config builds firmware for {}, but according to {}, there's no screen. The app will build without one, which saves a little battery.",
            screen_name(l),
            join_sources(ws)
        ),
        (_, DisplayKind::None) => format!(
            "Your config doesn't build firmware for a screen yet, but according to {}, you have {}. The app will build with the {} so the screen works.",
            join_sources(ws),
            screen_name(w),
            catalog::display(*w).name
        ),
        _ => format!(
            "Your config builds firmware for {}, but according to {}, you have {}. The app will build for the {}.",
            screen_name(l),
            join_sources(ws),
            screen_name(w),
            catalog::display(*w).name
        ),
    })
}

fn describe_side(s: &Side) -> String {
    match s {
        Side::Left => "the left half talks to the computer".into(),
        Side::Right => "the right half talks to the computer".into(),
    }
}

fn describe_bool(b: &bool) -> String {
    if *b {
        "live editing is on".into()
    } else {
        "live editing is off".into()
    }
}

/// Combine all evidence and detector notes into a [`Detection`].
pub fn combine(evidence: &[Evidence], mut notes: Vec<Note>) -> Detection {
    macro_rules! pick {
        ($pat:path) => {
            evidence
                .iter()
                .filter_map(|e| match &e.fact {
                    $pat(v) => Some((v.clone(), e)),
                    _ => None,
                })
                .collect::<Vec<_>>()
        };
    }
    let keyboard = decide(
        pick!(Fact::Keyboard),
        Wording {
            describe: &describe_keyboard,
            special: &no_special,
        },
    );
    let controller = decide(
        pick!(Fact::Controller),
        Wording {
            describe: &describe_controller,
            special: &no_special,
        },
    );
    let display = decide(
        pick!(Fact::Display),
        Wording {
            describe: &describe_display,
            special: &display_special,
        },
    );
    let central = decide(
        pick!(Fact::Central),
        Wording {
            describe: &describe_side,
            special: &no_special,
        },
    );
    let studio = decide(
        pick!(Fact::Studio),
        Wording {
            describe: &describe_bool,
            special: &no_special,
        },
    );

    // Screen software vs. screen hardware.
    let screen_sw = evidence.iter().rev().find_map(|e| match e.fact {
        Fact::ScreenSoftware(on) => Some(on),
        _ => None,
    });
    match (display.value, screen_sw) {
        (Some(d), Some(false)) if d != DisplayKind::None => notes.push(Note::warning(format!(
            "Your settings switch the screen off (CONFIG_ZMK_DISPLAY=n), so the {} will stay blank. This can be turned back on in the settings.",
            catalog::display(d).name
        ))),
        (Some(DisplayKind::None), Some(true)) => notes.push(Note::info(
            "Your settings switch screen support on, but no screen was found. That's harmless.",
        )),
        _ => {}
    }

    let profile = match (&keyboard.value, &controller.value, display.value) {
        (Some(Known::Known(k)), Some(Known::Known(c)), Some(d)) => Some(HardwareProfile {
            keyboard: *k,
            controller: *c,
            display: d,
            central: central.value.unwrap_or(Side::Left),
            studio: studio.value.unwrap_or(false),
        }),
        _ => None,
    };

    let mut ask = Vec::new();
    if !matches!(keyboard.value, Some(Known::Known(_))) {
        ask.push(Question::Keyboard);
    }
    if !controller.is_settled() || !matches!(controller.value, Some(Known::Known(_))) {
        ask.push(Question::Controller);
    }
    if !display.is_settled() {
        ask.push(Question::Display);
    }

    let summary = {
        let kb = match &keyboard.value {
            Some(Known::Known(k)) => catalog::keyboard(*k).name.to_string(),
            Some(Known::Other(n)) => n.clone(),
            None => "Keyboard unknown".into(),
        };
        let ctrl = match &controller.value {
            Some(Known::Known(c)) => catalog::controller(*c).name.to_string(),
            Some(Known::Other(n)) => n.clone(),
            None => "controller unknown".into(),
        };
        let disp = match display.value {
            Some(d) => catalog::display(d).name.to_string(),
            None => "screen unknown".into(),
        };
        let mut srcs: Vec<Source> = keyboard
            .sources
            .iter()
            .chain(&controller.sources)
            .chain(&display.sources)
            .copied()
            .collect();
        srcs.sort_by_key(|s| s.trust());
        srcs.dedup();
        let from: Vec<&str> = srcs.iter().map(|s| s.label()).collect();
        if from.is_empty() {
            format!("{kb} · {ctrl} · {disp}")
        } else {
            format!("{kb} · {ctrl} · {disp} — detected from {}", from.join(" + "))
        }
    };

    Detection {
        keyboard,
        controller,
        display,
        central,
        studio,
        notes,
        summary,
        profile,
        ask,
    }
}
