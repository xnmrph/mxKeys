//! `INFO_UF2.TXT` — the info file on a UF2 bootloader's USB drive (the drive that appears
//! after double-tapping reset).
//!
//! Format (Adafruit nRF52 bootloader, used by nice!nano):
//!
//! ```text
//! UF2 Bootloader 0.6.0 lib/nrfx (v2.0.0) lib/tinyusb (0.10.1-293-gaf8e5a90) lib/uf2 (…)
//! Model: nice!nano
//! Board-ID: nRF52840-nicenano
//! SoftDevice: S140 version 6.1.1
//! Date: Jun 19 2021
//! ```
//!
//! Parsing is tolerant: keys are case-insensitive, unknown keys are kept, BOM/CRLF are fine.

use serde::Serialize;

use super::{Confidence, Evidence, Fact, Findings, Known, Note, Source};
use crate::catalog;

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct Uf2Info {
    /// The free-form first line, e.g. `UF2 Bootloader 0.6.0 lib/nrfx …`.
    pub bootloader: Option<String>,
    /// Version number from the first line, e.g. `0.6.0`.
    pub bootloader_version: Option<String>,
    pub model: Option<String>,
    pub board_id: Option<String>,
    pub softdevice: Option<String>,
    pub date: Option<String>,
    /// Every `Key: value` line, in order.
    pub fields: Vec<(String, String)>,
}

pub fn parse_info_uf2(text: &str) -> Uf2Info {
    let text = text.trim_start_matches('\u{feff}');
    let mut info = Uf2Info::default();
    for (i, line) in text.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        // `Key: value` with a one-word key; the free-form first line never matches.
        let kv = line.split_once(':').filter(|(k, _)| {
            let k = k.trim();
            !k.is_empty() && !k.contains(char::is_whitespace)
        });
        match kv {
            Some((k, v)) => {
                let (k, v) = (k.trim(), v.trim().to_string());
                match k.to_ascii_lowercase().as_str() {
                    "model" => info.model = Some(v.clone()),
                    "board-id" => info.board_id = Some(v.clone()),
                    "softdevice" => info.softdevice = Some(v.clone()),
                    "date" => info.date = Some(v.clone()),
                    _ => {}
                }
                info.fields.push((k.to_string(), v));
            }
            None if i == 0 || info.bootloader.is_none() && info.fields.is_empty() => {
                info.bootloader = Some(line.to_string());
                info.bootloader_version = line
                    .split_whitespace()
                    .find(|w| w.chars().next().is_some_and(|c| c.is_ascii_digit()) && w.contains('.'))
                    .map(str::to_string);
            }
            None => {}
        }
    }
    info
}

impl Uf2Info {
    /// Looks like a real INFO_UF2.TXT (has at least a model or board id).
    pub fn is_plausible(&self) -> bool {
        self.model.is_some() || self.board_id.is_some()
    }
}

pub fn from_uf2(info: &Uf2Info) -> Findings {
    let mut f = Findings::default();
    if !info.is_plausible() {
        return f;
    }
    let model = info.model.as_deref().unwrap_or("");
    let board_id = info.board_id.as_deref().unwrap_or("");
    let by_id = catalog::CONTROLLERS
        .iter()
        .find(|c| c.uf2_board_ids.iter().any(|b| b.eq_ignore_ascii_case(board_id)));
    let by_model = || {
        catalog::CONTROLLERS.iter().find(|c| {
            c.uf2_models
                .iter()
                .any(|m| model.to_ascii_lowercase().contains(&m.to_ascii_lowercase()))
        })
    };
    let because = format!(
        "the bootloader drive says Model: {} / Board-ID: {}",
        if model.is_empty() { "?" } else { model },
        if board_id.is_empty() { "?" } else { board_id }
    );
    match by_id.or_else(by_model) {
        Some(c) => f.evidence.push(Evidence::new(
            Source::Bootloader,
            Fact::Controller(Known::Known(c.id)),
            Confidence::Strong,
            because,
        )),
        None => f.evidence.push(Evidence::new(
            Source::Bootloader,
            Fact::Controller(Known::Other(if model.is_empty() {
                board_id.to_string()
            } else {
                model.to_string()
            })),
            Confidence::Strong,
            because,
        )),
    }
    if let Some(v) = &info.bootloader_version {
        f.notes.push(Note::info(format!("Bootloader version {v}.")));
    }
    f
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tolerant_parsing() {
        let i = parse_info_uf2("\u{feff}UF2 Bootloader 0.6.0 lib/nrfx (v2.0.0)\r\nmodel:  nice!nano \r\nBoard-ID: nRF52840-nicenano\r\nExtra: thing\r\n");
        assert_eq!(i.bootloader_version.as_deref(), Some("0.6.0"));
        assert_eq!(i.model.as_deref(), Some("nice!nano"));
        assert_eq!(i.board_id.as_deref(), Some("nRF52840-nicenano"));
        assert_eq!(i.fields.len(), 3);
        assert!(i.is_plausible());
        assert!(!parse_info_uf2("hello\nworld").is_plausible());
        assert!(!parse_info_uf2("").is_plausible());
    }
}
