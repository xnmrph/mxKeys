//! Evidence from the list of connected USB devices.

use serde::{Deserialize, Serialize};

use super::{Confidence, Evidence, Fact, Findings, Known, Note, Source};
use crate::catalog;

/// ZMK firmware's default USB ids (ZMK `app/Kconfig`: `USB_DEVICE_VID`/`PID`).
pub const ZMK_VID: u16 = 0x1D50;
pub const ZMK_PID: u16 = 0x615E;

/// What the OS tells us about one USB device. Produced by `mxkeys-probe` (or fixtures).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UsbDevice {
    pub vid: u16,
    pub pid: u16,
    #[serde(default)]
    pub manufacturer: Option<String>,
    #[serde(default)]
    pub product: Option<String>,
    #[serde(default)]
    pub serial: Option<String>,
    /// Exposes a CDC-ACM serial interface (ZMK Studio over USB uses one).
    #[serde(default)]
    pub has_serial_port: bool,
}

impl UsbDevice {
    pub fn is_zmk(&self) -> bool {
        (self.vid, self.pid) == (ZMK_VID, ZMK_PID)
            || self
                .manufacturer
                .as_deref()
                .is_some_and(|m| m.eq_ignore_ascii_case("ZMK Project"))
    }
}

pub fn from_usb(devices: &[UsbDevice]) -> Findings {
    let mut f = Findings::default();
    for d in devices {
        let name = d
            .product
            .clone()
            .unwrap_or_else(|| format!("{:04x}:{:04x}", d.vid, d.pid));
        // A controller in bootloader mode.
        if let Some(c) = catalog::CONTROLLERS
            .iter()
            .find(|c| c.bootloader_usb.contains(&(d.vid, d.pid)))
        {
            f.evidence.push(Evidence::new(
                Source::Usb,
                Fact::Controller(Known::Known(c.id)),
                Confidence::Strong,
                format!(
                    "a {} is connected in bootloader mode (USB {:04x}:{:04x})",
                    c.name, d.vid, d.pid
                ),
            ));
            continue;
        }
        if !d.is_zmk() {
            continue;
        }
        // A keyboard running ZMK. The product string is CONFIG_ZMK_KEYBOARD_NAME.
        f.notes.push(Note::info(format!(
            "A keyboard running ZMK is connected: “{name}”."
        )));
        let lower = name.to_ascii_lowercase();
        for k in catalog::KEYBOARDS {
            let already = f
                .evidence
                .iter()
                .any(|e| e.fact == Fact::Keyboard(Known::Known(k.id)));
            if !already && lower.contains(&k.name.to_ascii_lowercase()) {
                f.evidence.push(Evidence::new(
                    Source::Usb,
                    Fact::Keyboard(Known::Known(k.id)),
                    Confidence::Weak,
                    format!("a ZMK keyboard named “{name}” is connected"),
                ));
            }
        }
        if d.has_serial_port {
            f.evidence.push(Evidence::new(
                Source::Usb,
                Fact::Studio(true),
                Confidence::Medium,
                format!("“{name}” offers a live-editing connection over USB"),
            ));
        }
    }
    f
}
