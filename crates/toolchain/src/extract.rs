//! Archive extraction (tar.xz, tar.gz, zip, 7z) with leading-path stripping.
//!
//! Safety: entries whose path (after stripping) is absolute or contains `..` are refused,
//! so a malicious archive can't write outside the destination. Unix permissions and
//! symlinks are preserved (the compiler and Python need both).

use std::fs::{self, File};
use std::io::{self, BufReader, Read};
use std::path::{Component, Path, PathBuf};

use crate::manifest::Format;

#[derive(Debug, thiserror::Error)]
pub enum ExtractError {
    #[error("couldn't unpack {archive}: {message}")]
    Archive { archive: PathBuf, message: String },
    #[error("{archive} contains an unsafe path: {entry}")]
    UnsafePath { archive: PathBuf, entry: String },
    #[error("couldn't write {path}: {source}")]
    Io { path: PathBuf, source: io::Error },
}

/// Strip `n` leading components from a relative archive path; `None` if nothing remains.
/// Errors on absolute paths and `..`.
pub fn strip_path(entry: &Path, n: usize) -> Result<Option<PathBuf>, ()> {
    let mut parts = Vec::new();
    for c in entry.components() {
        match c {
            Component::Normal(p) => parts.push(p),
            Component::CurDir => {}
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => return Err(()),
        }
    }
    if parts.len() <= n {
        return Ok(None);
    }
    Ok(Some(parts[n..].iter().collect()))
}

pub fn extract(archive: &Path, format: Format, dest: &Path, strip: usize) -> Result<(), ExtractError> {
    fs::create_dir_all(dest).map_err(|source| ExtractError::Io {
        path: dest.to_path_buf(),
        source,
    })?;
    let open = || {
        File::open(archive)
            .map(BufReader::new)
            .map_err(|source| ExtractError::Io {
                path: archive.to_path_buf(),
                source,
            })
    };
    match format {
        Format::TarXz => extract_tar(archive, liblzma::read::XzDecoder::new(open()?), dest, strip),
        Format::TarGz => extract_tar(archive, flate2::read::GzDecoder::new(open()?), dest, strip),
        Format::Zip => extract_zip(archive, dest, strip),
        Format::SevenZ => extract_7z(archive, dest, strip),
    }
}

fn archive_err(archive: &Path, e: impl std::fmt::Display) -> ExtractError {
    ExtractError::Archive {
        archive: archive.to_path_buf(),
        message: e.to_string(),
    }
}

fn target(archive: &Path, dest: &Path, entry: &Path, strip: usize) -> Result<Option<PathBuf>, ExtractError> {
    match strip_path(entry, strip) {
        Ok(Some(rel)) => Ok(Some(dest.join(rel))),
        Ok(None) => Ok(None),
        Err(()) => Err(ExtractError::UnsafePath {
            archive: archive.to_path_buf(),
            entry: entry.display().to_string(),
        }),
    }
}

fn extract_tar<R: Read>(archive: &Path, reader: R, dest: &Path, strip: usize) -> Result<(), ExtractError> {
    let mut ar = tar::Archive::new(reader);
    ar.set_preserve_permissions(true);
    ar.set_overwrite(true);
    for entry in ar.entries().map_err(|e| archive_err(archive, e))? {
        let mut entry = entry.map_err(|e| archive_err(archive, e))?;
        let path = entry.path().map_err(|e| archive_err(archive, e))?.into_owned();
        let Some(out) = target(archive, dest, &path, strip)? else {
            continue;
        };
        // Hard links point at paths *inside the archive* — strip those too.
        if entry.header().entry_type() == tar::EntryType::Link {
            if let Some(link) = entry.link_name().map_err(|e| archive_err(archive, e))? {
                if let Some(src) = target(archive, dest, &link, strip)? {
                    if let Some(p) = out.parent() {
                        fs::create_dir_all(p).map_err(|source| ExtractError::Io {
                            path: p.to_path_buf(),
                            source,
                        })?;
                    }
                    let _ = fs::remove_file(&out);
                    fs::hard_link(&src, &out)
                        .or_else(|_| fs::copy(&src, &out).map(|_| ()))
                        .map_err(|source| ExtractError::Io {
                            path: out.clone(),
                            source,
                        })?;
                }
            }
            continue;
        }
        if let Some(p) = out.parent() {
            fs::create_dir_all(p).map_err(|source| ExtractError::Io {
                path: p.to_path_buf(),
                source,
            })?;
        }
        // Symlink targets are relative to the link's directory; refuse absolute ones.
        if entry.header().entry_type() == tar::EntryType::Symlink {
            if let Some(link) = entry.link_name().map_err(|e| archive_err(archive, e))? {
                if link.is_absolute() {
                    return Err(ExtractError::UnsafePath {
                        archive: archive.to_path_buf(),
                        entry: format!("{} -> {}", path.display(), link.display()),
                    });
                }
            }
        }
        entry.unpack(&out).map_err(|e| archive_err(archive, e))?;
    }
    Ok(())
}

fn extract_zip(archive: &Path, dest: &Path, strip: usize) -> Result<(), ExtractError> {
    let file = File::open(archive).map_err(|source| ExtractError::Io {
        path: archive.to_path_buf(),
        source,
    })?;
    let mut zip = zip::ZipArchive::new(file).map_err(|e| archive_err(archive, e))?;
    for i in 0..zip.len() {
        let mut f = zip.by_index(i).map_err(|e| archive_err(archive, e))?;
        let Some(name) = f.enclosed_name() else {
            return Err(ExtractError::UnsafePath {
                archive: archive.to_path_buf(),
                entry: f.name().to_string(),
            });
        };
        let Some(out) = target(archive, dest, &name, strip)? else {
            continue;
        };
        if f.is_dir() {
            fs::create_dir_all(&out).map_err(|source| ExtractError::Io {
                path: out.clone(),
                source,
            })?;
            continue;
        }
        if let Some(p) = out.parent() {
            fs::create_dir_all(p).map_err(|source| ExtractError::Io {
                path: p.to_path_buf(),
                source,
            })?;
        }
        let mut w = File::create(&out).map_err(|source| ExtractError::Io {
            path: out.clone(),
            source,
        })?;
        io::copy(&mut f, &mut w).map_err(|source| ExtractError::Io {
            path: out.clone(),
            source,
        })?;
        #[cfg(unix)]
        if let Some(mode) = f.unix_mode() {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&out, fs::Permissions::from_mode(mode & 0o7777)).map_err(|source| {
                ExtractError::Io {
                    path: out.clone(),
                    source,
                }
            })?;
        }
    }
    Ok(())
}

fn extract_7z(archive: &Path, dest: &Path, strip: usize) -> Result<(), ExtractError> {
    let mut failure: Option<ExtractError> = None;
    sevenz_rust2::decompress_file_with_extract_fn(archive, dest, |entry, reader, _default_dest| {
        let name = PathBuf::from(entry.name().replace('\\', "/"));
        let out = match target(archive, dest, &name, strip) {
            Ok(Some(o)) => o,
            Ok(None) => return Ok(true),
            Err(e) => {
                failure = Some(e);
                return Ok(false);
            }
        };
        if entry.is_directory() {
            fs::create_dir_all(&out)?;
            return Ok(true);
        }
        if let Some(p) = out.parent() {
            fs::create_dir_all(p)?;
        }
        let mut w = File::create(&out)?;
        io::copy(reader, &mut w)?;
        Ok(true)
    })
    .map_err(|e| archive_err(archive, e))?;
    match failure {
        Some(e) => Err(e),
        None => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn tmp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("mxkeys-ex-{}-{name}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn stripping_and_safety() {
        assert_eq!(
            strip_path(Path::new("a/b/c.txt"), 1),
            Ok(Some(PathBuf::from("b/c.txt")))
        );
        assert_eq!(strip_path(Path::new("./a/b"), 1), Ok(Some(PathBuf::from("b"))));
        assert_eq!(strip_path(Path::new("a/"), 1), Ok(None));
        assert_eq!(strip_path(Path::new("a/../../etc/passwd"), 1), Err(()));
        assert_eq!(strip_path(Path::new("/etc/passwd"), 0), Err(()));
    }

    fn tar_bytes() -> Vec<u8> {
        let mut b = tar::Builder::new(Vec::new());
        let mut h = tar::Header::new_gnu();
        let data = b"#!/bin/sh\necho hi\n";
        h.set_size(data.len() as u64);
        h.set_mode(0o755);
        h.set_cksum();
        b.append_data(&mut h, "top/bin/tool", &data[..]).unwrap();
        let mut s = tar::Header::new_gnu();
        s.set_entry_type(tar::EntryType::Symlink);
        s.set_size(0);
        s.set_cksum();
        b.append_link(&mut s, "top/bin/tool-link", "tool").unwrap();
        b.into_inner().unwrap()
    }

    #[test]
    fn tar_gz_and_xz_with_strip_and_symlinks() {
        let dir = tmp("tar");
        let gz = dir.join("a.tar.gz");
        let mut enc = flate2::write::GzEncoder::new(File::create(&gz).unwrap(), flate2::Compression::fast());
        enc.write_all(&tar_bytes()).unwrap();
        enc.finish().unwrap();
        extract(&gz, Format::TarGz, &dir.join("out-gz"), 1).unwrap();
        assert!(dir.join("out-gz/bin/tool").is_file());

        let xz = dir.join("a.tar.xz");
        let mut enc = liblzma::write::XzEncoder::new(File::create(&xz).unwrap(), 1);
        enc.write_all(&tar_bytes()).unwrap();
        enc.finish().unwrap();
        extract(&xz, Format::TarXz, &dir.join("out-xz"), 1).unwrap();
        let tool = dir.join("out-xz/bin/tool");
        assert!(tool.is_file());
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(fs::metadata(&tool).unwrap().permissions().mode() & 0o777, 0o755);
            assert_eq!(
                fs::read_link(dir.join("out-xz/bin/tool-link")).unwrap(),
                PathBuf::from("tool")
            );
        }
    }

    #[test]
    fn zip_with_strip_and_traversal_refused() {
        let dir = tmp("zip");
        let p = dir.join("a.zip");
        {
            let mut z = zip::ZipWriter::new(File::create(&p).unwrap());
            let opts = zip::write::SimpleFileOptions::default().unix_permissions(0o755);
            z.start_file("cmake-x/bin/cmake", opts).unwrap();
            z.write_all(b"binary").unwrap();
            z.finish().unwrap();
        }
        extract(&p, Format::Zip, &dir.join("out"), 1).unwrap();
        assert_eq!(fs::read(dir.join("out/bin/cmake")).unwrap(), b"binary");

        let evil = dir.join("evil.zip");
        {
            let mut z = zip::ZipWriter::new(File::create(&evil).unwrap());
            z.start_file("../../escape.txt", zip::write::SimpleFileOptions::default())
                .unwrap();
            z.write_all(b"x").unwrap();
            z.finish().unwrap();
        }
        assert!(extract(&evil, Format::Zip, &dir.join("out2"), 0).is_err());
        assert!(!dir.join("escape.txt").exists());
    }

    #[test]
    fn seven_z_with_strip() {
        let dir = tmp("7z");
        let src = dir.join("src/top/sub");
        fs::create_dir_all(&src).unwrap();
        fs::write(src.join("f.txt"), b"seven").unwrap();
        let p = dir.join("a.7z");
        sevenz_rust2::compress_to_path(dir.join("src"), &p).unwrap();
        extract(&p, Format::SevenZ, &dir.join("out"), 1).unwrap();
        assert_eq!(fs::read(dir.join("out/sub/f.txt")).unwrap(), b"seven");
    }
}
