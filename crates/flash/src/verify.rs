//! Refusing to flash the wrong thing.
//!
//! Two independent checks before any byte is copied:
//! 1. **The drive**: `INFO_UF2.TXT` must identify the controller from the confirmed
//!    hardware profile (by Board-ID, or by Model when the Board-ID isn't one we know).
//! 2. **The file**: the `.uf2` must be well-formed and carry the controller's UF2 family
//!    id (nRF52840 = 0xADA52840).

use mxkeys_hardware::catalog::{self, ControllerId};
use mxkeys_hardware::detect::uf2::Uf2Info;
use serde::Serialize;

use crate::uf2::Uf2Summary;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, thiserror::Error)]
pub enum Mismatch {
    #[error("This drive belongs to a different controller ({found}), not a {expected}. Nothing was copied.")]
    WrongController { found: String, expected: String },
    #[error("This drive doesn't say which controller it is, so we won't risk copying to it.")]
    UnknownController,
    #[error("This firmware file isn't for the {expected} (it's for a different chip). Nothing was copied.")]
    WrongFamily { expected: String, found: Option<u32> },
    #[error("This firmware file is damaged or isn't a UF2 file: {0}")]
    BadFile(String),
}

fn lower(s: &str) -> String {
    s.to_ascii_lowercase()
}

/// Is the bootloader drive's controller the one in the profile?
pub fn check_drive(info: &Uf2Info, expected: ControllerId) -> Result<(), Mismatch> {
    let want = catalog::controller(expected);
    let board_id = info.board_id.as_deref().unwrap_or("");
    let model = info.model.as_deref().unwrap_or("");
    if board_id.is_empty() && model.is_empty() {
        return Err(Mismatch::UnknownController);
    }
    let id_matches = want
        .uf2_board_ids
        .iter()
        .any(|b| b.eq_ignore_ascii_case(board_id));
    if id_matches {
        return Ok(());
    }
    // A Board-ID that belongs to another known controller is a definite mismatch.
    if let Some(other) = catalog::CONTROLLERS
        .iter()
        .find(|c| c.id != expected && c.uf2_board_ids.iter().any(|b| b.eq_ignore_ascii_case(board_id)))
    {
        return Err(Mismatch::WrongController {
            found: other.name.into(),
            expected: want.name.into(),
        });
    }
    // Board-ID unknown to us (our table is unverified): fall back to the model name.
    let model_matches = want.uf2_models.iter().any(|m| lower(model).contains(&lower(m)));
    if model_matches {
        return Ok(());
    }
    let found = if model.is_empty() {
        board_id.to_string()
    } else {
        model.to_string()
    };
    Err(Mismatch::WrongController {
        found,
        expected: want.name.into(),
    })
}

/// Is the firmware file for the profile's controller?
pub fn check_firmware(summary: &Uf2Summary, expected: ControllerId) -> Result<(), Mismatch> {
    let want = catalog::controller(expected);
    if summary.family == Some(want.uf2_family) {
        Ok(())
    } else {
        Err(Mismatch::WrongFamily {
            expected: want.name.into(),
            found: summary.family,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mxkeys_hardware::detect::uf2::parse_info_uf2;

    #[test]
    fn drive_checks() {
        let nano = parse_info_uf2("UF2 Bootloader 0.6.0\nModel: nice!nano\nBoard-ID: nRF52840-nicenano\n");
        assert_eq!(check_drive(&nano, ControllerId::NiceNanoV2), Ok(()));

        // Real Board-ID turns out different from our (unverified) table: model still matches.
        let renamed = parse_info_uf2("Model: nice!nano\nBoard-ID: nRF52840-nicenano-v2\n");
        assert_eq!(check_drive(&renamed, ControllerId::NiceNanoV2), Ok(()));

        let rp = parse_info_uf2("UF2 Bootloader v3.0\nModel: Raspberry Pi RP2\nBoard-ID: RPI-RP2\n");
        assert_eq!(
            check_drive(&rp, ControllerId::NiceNanoV2),
            Err(Mismatch::WrongController {
                found: "Raspberry Pi RP2".into(),
                expected: "nice!nano v2".into()
            })
        );
        assert_eq!(
            check_drive(&parse_info_uf2("hello"), ControllerId::NiceNanoV2),
            Err(Mismatch::UnknownController)
        );
        let msg = Mismatch::WrongController {
            found: "X".into(),
            expected: "nice!nano v2".into(),
        }
        .to_string();
        assert!(msg.contains("Nothing was copied"));
    }

    #[test]
    fn firmware_checks() {
        let ok = Uf2Summary {
            blocks: 1,
            family: Some(0xADA5_2840),
            start_address: 0x26000,
            payload: 256,
        };
        assert_eq!(check_firmware(&ok, ControllerId::NiceNanoV2), Ok(()));
        let rp2040 = Uf2Summary {
            family: Some(0xE48B_FF56),
            ..ok.clone()
        };
        assert!(matches!(
            check_firmware(&rp2040, ControllerId::NiceNanoV2),
            Err(Mismatch::WrongFamily { .. })
        ));
        let none = Uf2Summary { family: None, ..ok };
        assert!(check_firmware(&none, ControllerId::NiceNanoV2).is_err());
    }
}
