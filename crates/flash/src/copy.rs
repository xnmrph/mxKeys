//! Copying a `.uf2` onto the bootloader drive.
//!
//! The bootloader flashes as blocks arrive and **resets as soon as the last block is
//! written** — often before the OS has finished closing the file. So an I/O error *after*
//! every byte was handed to the OS (or the drive vanishing then) is the normal "done"
//! signal, not a failure. An error before that is a real failure.

use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

use serde::Serialize;

/// Name used on the drive. Not `CURRENT.UF2`, which is the bootloader's read-only image
/// of what's already flashed.
pub const TARGET_NAME: &str = "mxkeys.uf2";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CopyOutcome {
    /// Written and closed normally.
    Written,
    /// All bytes written; the drive went away while closing (bootloader reset) — expected.
    WrittenThenDisconnected,
}

#[derive(Debug, thiserror::Error)]
pub enum CopyError {
    #[error("couldn't read the firmware file {path}: {source}")]
    Read { path: PathBuf, source: std::io::Error },
    #[error("copying stopped after {written} of {total} bytes ({source}). Double-tap reset and try again.")]
    Interrupted {
        written: u64,
        total: u64,
        source: std::io::Error,
    },
}

pub fn copy_uf2(
    src: &Path,
    drive_root: &Path,
    mut progress: impl FnMut(u64, u64),
) -> Result<CopyOutcome, CopyError> {
    let data = std::fs::read(src).map_err(|source| CopyError::Read {
        path: src.to_path_buf(),
        source,
    })?;
    let total = data.len() as u64;
    let dest = drive_root.join(TARGET_NAME);
    let interrupted = |written: u64, source| CopyError::Interrupted {
        written,
        total,
        source,
    };

    let mut f: File = OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .open(&dest)
        .map_err(|e| interrupted(0, e))?;
    let mut written = 0u64;
    // UF2 blocks are 512 bytes; write in block-aligned chunks so progress is meaningful.
    for chunk in data.chunks(64 * 512) {
        f.write_all(chunk).map_err(|e| interrupted(written, e))?;
        written += chunk.len() as u64;
        progress(written, total);
    }
    // Everything was handed to the OS. Flushing/closing may now fail because the
    // bootloader already reset — that's success.
    let finished = f.flush().and_then(|_| f.sync_all());
    drop(f);
    Ok(match finished {
        Ok(()) => CopyOutcome::Written,
        Err(_) => CopyOutcome::WrittenThenDisconnected,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn copies_and_reports_progress() {
        let dir = std::env::temp_dir().join(format!("mxkeys-copy-{}", std::process::id()));
        let drive = dir.join("NICENANO");
        std::fs::create_dir_all(&drive).unwrap();
        let src = dir.join("left.uf2");
        let mut data = Vec::new();
        for i in 0..100u32 {
            data.extend(crate::uf2::block(0x26000 + i * 256, 0xADA5_2840));
        }
        std::fs::write(&src, &data).unwrap();
        let mut last = (0, 0);
        assert_eq!(
            copy_uf2(&src, &drive, |a, b| last = (a, b)).unwrap(),
            CopyOutcome::Written
        );
        assert_eq!(last, (51_200, 51_200));
        assert_eq!(std::fs::read(drive.join(TARGET_NAME)).unwrap(), data);

        // A drive that's gone before we start is a real failure.
        let err = copy_uf2(&src, &dir.join("GONE"), |_, _| {}).unwrap_err();
        assert!(matches!(err, CopyError::Interrupted { written: 0, .. }));
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
