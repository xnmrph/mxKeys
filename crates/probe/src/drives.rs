//! Finding mounted UF2 bootloader drives (the USB drive a nice!nano shows after a
//! double-tap on reset) by looking for `INFO_UF2.TXT` at the root of candidate drives.

use std::path::{Path, PathBuf};

use mxkeys_hardware::detect::uf2::parse_info_uf2;

use crate::{DriveScanner, ProbeError, Uf2Drive};

/// The info file is tiny; never read more than this.
const MAX_INFO_BYTES: u64 = 16 * 1024;
const INFO_NAMES: &[&str] = &["INFO_UF2.TXT", "info_uf2.txt", "Info_UF2.txt"];

/// Filesystem types UF2 bootloaders present (FAT12/16). `fuseblk`/`exfat` are cheap to
/// include and harmless.
const FAT_TYPES: &[&str] = &["vfat", "msdos", "fat", "exfat", "fuseblk"];

/// Parse `/proc/self/mounts` (or `/proc/mounts`): returns `(mount point, fs type)`.
/// Mount points are octal-escaped (`\040` = space).
pub fn parse_proc_mounts(text: &str) -> Vec<(PathBuf, String)> {
    text.lines()
        .filter_map(|line| {
            let mut parts = line.split_whitespace();
            let _device = parts.next()?;
            let mount = parts.next()?;
            let fstype = parts.next()?;
            Some((PathBuf::from(unescape_octal(mount)), fstype.to_string()))
        })
        .collect()
}

fn unescape_octal(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'\\' && i + 3 < b.len() && b[i + 1..i + 4].iter().all(|c| (b'0'..=b'7').contains(c)) {
            let v = (b[i + 1] - b'0') * 64 + (b[i + 2] - b'0') * 8 + (b[i + 3] - b'0');
            out.push(v);
            i += 4;
        } else {
            out.push(b[i]);
            i += 1;
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn subdirs(dir: &Path) -> Vec<PathBuf> {
    std::fs::read_dir(dir)
        .map(|rd| rd.flatten().map(|e| e.path()).filter(|p| p.is_dir()).collect())
        .unwrap_or_default()
}

/// Candidate drive roots for this OS.
pub fn default_roots() -> Vec<PathBuf> {
    let mut roots = Vec::new();
    if cfg!(target_os = "linux") {
        let mounts = std::fs::read_to_string("/proc/self/mounts")
            .or_else(|_| std::fs::read_to_string("/proc/mounts"))
            .unwrap_or_default();
        roots.extend(
            parse_proc_mounts(&mounts)
                .into_iter()
                .filter(|(_, t)| FAT_TYPES.contains(&t.as_str()))
                .map(|(p, _)| p),
        );
        // udisks automount locations, in case /proc isn't readable.
        if let Ok(user) = std::env::var("USER") {
            for base in [format!("/run/media/{user}"), format!("/media/{user}")] {
                roots.extend(subdirs(Path::new(&base)));
            }
        }
    } else if cfg!(target_os = "macos") {
        roots.extend(subdirs(Path::new("/Volumes")));
    } else if cfg!(windows) {
        // Skip A: and B: (legacy floppy letters can trigger "insert disk" prompts).
        roots.extend((b'C'..=b'Z').map(|l| PathBuf::from(format!("{}:\\", l as char))));
    }
    roots.sort();
    roots.dedup();
    roots
}

/// Scans a list of drive roots for `INFO_UF2.TXT`.
#[derive(Debug, Clone)]
pub struct FsDriveScanner {
    roots: Option<Vec<PathBuf>>,
}

impl FsDriveScanner {
    /// Scan the OS's usual places (see [`default_roots`]), re-evaluated on every scan.
    pub fn system() -> Self {
        FsDriveScanner { roots: None }
    }
    /// Scan only these roots (tests, or a user-picked folder).
    pub fn with_roots(roots: Vec<PathBuf>) -> Self {
        FsDriveScanner { roots: Some(roots) }
    }
}

fn read_info(root: &Path) -> Option<String> {
    for name in INFO_NAMES {
        let p = root.join(name);
        let Ok(meta) = std::fs::metadata(&p) else { continue };
        if !meta.is_file() || meta.len() > MAX_INFO_BYTES {
            continue;
        }
        if let Ok(bytes) = std::fs::read(&p) {
            return Some(String::from_utf8_lossy(&bytes).into_owned());
        }
    }
    None
}

impl DriveScanner for FsDriveScanner {
    fn uf2_drives(&self) -> Result<Vec<Uf2Drive>, ProbeError> {
        let roots = self.roots.clone().unwrap_or_else(default_roots);
        Ok(roots
            .into_iter()
            .filter_map(|root| {
                let info = parse_info_uf2(&read_info(&root)?);
                info.is_plausible().then_some(Uf2Drive { path: root, info })
            })
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn proc_mounts() {
        let m = parse_proc_mounts(
            "/dev/sda1 / ext4 rw 0 0\n/dev/sdb /run/media/me/NICE\\040NANO vfat rw,nosuid 0 0\nproc /proc proc rw 0 0\n",
        );
        assert_eq!(
            m[1],
            (PathBuf::from("/run/media/me/NICE NANO"), "vfat".to_string())
        );
        assert_eq!(m.len(), 3);
    }

    #[test]
    fn scans_roots_for_info_files() {
        let base = std::env::temp_dir().join(format!("mxkeys-drives-{}", std::process::id()));
        let nano = base.join("NICENANO");
        let usb_stick = base.join("STICK");
        let bogus = base.join("BOGUS");
        for d in [&nano, &usb_stick, &bogus] {
            std::fs::create_dir_all(d).unwrap();
        }
        std::fs::write(
            nano.join("INFO_UF2.TXT"),
            "UF2 Bootloader 0.6.0\nModel: nice!nano\nBoard-ID: nRF52840-nicenano\n",
        )
        .unwrap();
        std::fs::write(usb_stick.join("photo.jpg"), b"x").unwrap();
        std::fs::write(bogus.join("INFO_UF2.TXT"), "not really").unwrap();

        let found = FsDriveScanner::with_roots(vec![nano.clone(), usb_stick, bogus, base.join("missing")])
            .uf2_drives()
            .unwrap();
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].path, nano);
        assert_eq!(found[0].info.board_id.as_deref(), Some("nRF52840-nicenano"));
        std::fs::remove_dir_all(&base).unwrap();
    }

    #[test]
    fn system_scan_runs() {
        // No keyboard on CI: just make sure scanning the real system doesn't fail.
        assert!(FsDriveScanner::system().uf2_drives().is_ok());
    }
}
