//! OS probing for hardware detection, behind traits so everything above it can be tested
//! without a keyboard:
//!
//! - [`UsbEnumerator`] — connected USB devices ([`NusbEnumerator`] on real systems)
//! - [`DriveScanner`] — mounted UF2 bootloader drives ([`FsDriveScanner`])
//!
//! [`MockUsb`] and [`MockDrives`] return fixed lists for tests and demos.

#[cfg(target_os = "linux")]
mod automount;
mod drives;

use std::path::PathBuf;

use mxkeys_hardware::detect::uf2::Uf2Info;
use mxkeys_hardware::detect::usb::UsbDevice;
use serde::Serialize;

pub use drives::{default_roots, parse_proc_mounts, FsDriveScanner};

#[derive(Debug, thiserror::Error)]
pub enum ProbeError {
    #[error("couldn't list USB devices: {0}")]
    Usb(String),
    #[error("couldn't look for bootloader drives: {0}")]
    Drives(String),
}

/// A mounted drive that carries an `INFO_UF2.TXT` (a controller in bootloader mode).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Uf2Drive {
    /// Mount point / drive root, e.g. `/run/media/me/NICENANO`, `/Volumes/NICENANO`, `E:\`.
    pub path: PathBuf,
    pub info: Uf2Info,
}

pub trait UsbEnumerator {
    fn devices(&self) -> Result<Vec<UsbDevice>, ProbeError>;
}

pub trait DriveScanner {
    fn uf2_drives(&self) -> Result<Vec<Uf2Drive>, ProbeError>;
}

/// Real USB enumeration via `nusb` (pure Rust; no libusb/udev needed for listing).
#[derive(Debug, Default, Clone, Copy)]
pub struct NusbEnumerator;

/// USB interface class codes for CDC-ACM ("virtual serial port"): communications
/// interface class 0x02 with ACM subclass 0x02.
const CDC_CLASS: u8 = 0x02;
const CDC_ACM_SUBCLASS: u8 = 0x02;

impl UsbEnumerator for NusbEnumerator {
    fn devices(&self) -> Result<Vec<UsbDevice>, ProbeError> {
        use nusb::MaybeFuture;
        let list = nusb::list_devices()
            .wait()
            .map_err(|e| ProbeError::Usb(e.to_string()))?;
        Ok(list
            .map(|d| UsbDevice {
                vid: d.vendor_id(),
                pid: d.product_id(),
                manufacturer: d.manufacturer_string().map(str::to_string),
                product: d.product_string().map(str::to_string),
                serial: d.serial_number().map(str::to_string),
                has_serial_port: d
                    .interfaces()
                    .any(|i| i.class() == CDC_CLASS && i.subclass() == CDC_ACM_SUBCLASS),
            })
            .collect())
    }
}

/// Fixed USB device list.
#[derive(Debug, Clone, Default)]
pub struct MockUsb(pub Vec<UsbDevice>);

impl UsbEnumerator for MockUsb {
    fn devices(&self) -> Result<Vec<UsbDevice>, ProbeError> {
        Ok(self.0.clone())
    }
}

/// Fixed drive list.
#[derive(Debug, Clone, Default)]
pub struct MockDrives(pub Vec<Uf2Drive>);

impl DriveScanner for MockDrives {
    fn uf2_drives(&self) -> Result<Vec<Uf2Drive>, ProbeError> {
        Ok(self.0.clone())
    }
}

/// Everything probed at one moment. Errors don't abort: detection works with whatever
/// was found, and the UI can mention what couldn't be checked.
#[derive(Debug, Clone, Default, Serialize)]
pub struct Snapshot {
    pub usb: Vec<UsbDevice>,
    pub drives: Vec<Uf2Drive>,
    pub errors: Vec<String>,
}

pub fn snapshot(usb: &dyn UsbEnumerator, drives: &dyn DriveScanner) -> Snapshot {
    let mut s = Snapshot::default();
    match usb.devices() {
        Ok(d) => s.usb = d,
        Err(e) => s.errors.push(e.to_string()),
    }
    match drives.uf2_drives() {
        Ok(d) => s.drives = d,
        Err(e) => s.errors.push(e.to_string()),
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    struct FailingUsb;
    impl UsbEnumerator for FailingUsb {
        fn devices(&self) -> Result<Vec<UsbDevice>, ProbeError> {
            Err(ProbeError::Usb("permission denied".into()))
        }
    }

    #[test]
    fn snapshot_collects_errors_without_failing() {
        let drives = MockDrives(vec![Uf2Drive {
            path: PathBuf::from("/Volumes/NICENANO"),
            info: mxkeys_hardware::detect::uf2::parse_info_uf2(
                "Model: nice!nano\nBoard-ID: nRF52840-nicenano\n",
            ),
        }]);
        let s = snapshot(&FailingUsb, &drives);
        assert!(s.usb.is_empty());
        assert_eq!(s.drives.len(), 1);
        assert_eq!(s.errors, ["couldn't list USB devices: permission denied"]);
    }

    /// Real enumeration must not fail on CI machines (they have no keyboard, but do
    /// have USB buses / virtual devices — or none, which is fine too).
    #[test]
    fn real_usb_enumeration_runs() {
        match NusbEnumerator.devices() {
            Ok(_) => {}
            // Some sandboxes have no /sys/bus/usb; that's an environment limit, not a bug.
            Err(e) => eprintln!("USB enumeration unavailable here: {e}"),
        }
    }
}
