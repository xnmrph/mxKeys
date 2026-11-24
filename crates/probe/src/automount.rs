//! Linux: mount a UF2 bootloader drive that nothing mounted automatically (e.g. NixOS
//! or window managers without a file manager), through udisks — the same service
//! desktops use, so no root and no extra setup.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// Volume labels of the UF2 bootloaders on supported controllers (plus any label
/// ending in `BOOT`, the Adafruit bootloader's convention).
const LABELS: &[&str] = &["NICENANO", "XIAO-SENSE", "NRF52BOOT", "FTHR840BOOT", "NRFMICRO"];

/// Don't hammer udisks: one attempt per device every few seconds.
const RETRY: Duration = Duration::from_secs(5);

static ATTEMPTS: Mutex<Option<HashMap<String, Instant>>> = Mutex::new(None);

/// `/dev/disk/by-label` escapes special characters as `\xNN`.
fn unescape_label(s: &str) -> String {
    let mut out = String::new();
    let mut rest = s;
    while let Some(i) = rest.find("\\x") {
        out.push_str(&rest[..i]);
        match u8::from_str_radix(rest.get(i + 2..i + 4).unwrap_or(""), 16) {
            Ok(b) => {
                out.push(b as char);
                rest = &rest[i + 4..];
            }
            Err(_) => {
                out.push_str("\\x");
                rest = &rest[i + 2..];
            }
        }
    }
    out.push_str(rest);
    out
}

pub fn is_bootloader_label(label: &str) -> bool {
    let l = label.to_ascii_uppercase();
    LABELS.contains(&l.as_str()) || l.ends_with("BOOT")
}

/// Unmounted bootloader drives: `(device name like "sdb", label)`.
fn unmounted_candidates(mounts: &str) -> Vec<(String, String)> {
    let Ok(rd) = std::fs::read_dir("/dev/disk/by-label") else {
        return Vec::new();
    };
    let mounted: Vec<&str> = mounts
        .lines()
        .filter_map(|l| l.split_whitespace().next())
        .collect();
    rd.flatten()
        .filter_map(|e| {
            let label = unescape_label(&e.file_name().to_string_lossy());
            if !is_bootloader_label(&label) {
                return None;
            }
            let dev = std::fs::canonicalize(e.path()).ok()?;
            let dev_str = dev.display().to_string();
            if mounted.contains(&dev_str.as_str()) {
                return None;
            }
            Some((dev.file_name()?.to_string_lossy().into_owned(), label))
        })
        .collect()
}

fn udisks_mount(device: &str) -> Result<PathBuf, String> {
    use dbus::arg::{PropMap, RefArg, Variant};
    use dbus::blocking::Connection;

    let conn = Connection::new_system().map_err(|e| e.to_string())?;
    // udisks object paths escape non-alphanumerics; kernel block names are plain.
    let path = format!("/org/freedesktop/UDisks2/block_devices/{device}");
    let proxy = conn.with_proxy("org.freedesktop.UDisks2", path, Duration::from_secs(10));
    let mut opts: PropMap = HashMap::new();
    opts.insert(
        "auth.no_user_interaction".into(),
        Variant(Box::new(true) as Box<dyn RefArg>),
    );
    let (mount_path,): (String,) = proxy
        .method_call("org.freedesktop.UDisks2.Filesystem", "Mount", (opts,))
        .map_err(|e| e.to_string())?;
    Ok(PathBuf::from(mount_path))
}

/// Try to mount unmounted bootloader drives; returns the new mount points.
pub fn mount_bootloader_drives(mounts: &str) -> Vec<PathBuf> {
    let mut out = Vec::new();
    for (device, label) in unmounted_candidates(mounts) {
        {
            let mut guard = ATTEMPTS.lock().unwrap();
            let attempts = guard.get_or_insert_with(HashMap::new);
            if attempts.get(&device).is_some_and(|t| t.elapsed() < RETRY) {
                continue;
            }
            attempts.insert(device.clone(), Instant::now());
        }
        match udisks_mount(&device) {
            Ok(p) => out.push(p),
            Err(e) => eprintln!("mxkeys: couldn't mount {label} (/dev/{device}) via udisks: {e}"),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn labels() {
        assert!(is_bootloader_label("NICENANO"));
        assert!(is_bootloader_label("nicenano"));
        assert!(is_bootloader_label("FTHR840BOOT"));
        assert!(!is_bootloader_label("grippers"));
        assert!(!is_bootloader_label("BIOS"));
        assert_eq!(unescape_label("MY\\x20DRIVE"), "MY DRIVE");
        assert_eq!(unescape_label("A\\xZZ"), "A\\xZZ");
    }
}
