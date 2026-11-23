//! Making the installed toolchain smaller without changing what it builds.
//!
//! - Remove files matching simple globs (docs, installers we don't run).
//! - Keep only the compiler library variants ("multilibs") the supported controllers use.
//!   Verified: trimming the arm-zephyr-eabi toolchain to `thumb/v7e-m+fp/hard` produced a
//!   byte-identical nice!nano firmware (1.1 GB → 256 MB).

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

/// `*` matches any run of characters within one path segment.
pub fn glob_match(pattern: &str, name: &str) -> bool {
    fn rec(p: &[u8], n: &[u8]) -> bool {
        match p.split_first() {
            None => n.is_empty(),
            Some((b'*', rest)) => (0..=n.len()).any(|i| rec(rest, &n[i..])),
            Some((c, rest)) => n.first() == Some(c) && rec(rest, &n[1..]),
        }
    }
    rec(pattern.as_bytes(), name.as_bytes())
}

/// Delete paths under `root` matching `patterns` (segment-wise globs like
/// `share/cmake-*/Help`). Returns bytes freed (approximate).
pub fn remove_globs(root: &Path, patterns: &[String]) -> io::Result<u64> {
    let mut freed = 0;
    for pat in patterns {
        let segs: Vec<&str> = pat.split('/').filter(|s| !s.is_empty()).collect();
        let mut current = vec![root.to_path_buf()];
        for (i, seg) in segs.iter().enumerate() {
            let mut next = Vec::new();
            for dir in &current {
                let Ok(rd) = fs::read_dir(dir) else { continue };
                for e in rd.flatten() {
                    if glob_match(seg, &e.file_name().to_string_lossy()) {
                        let p = e.path();
                        if i + 1 == segs.len() || p.is_dir() {
                            next.push(p);
                        }
                    }
                }
            }
            current = next;
        }
        for p in current {
            freed += size_of(&p);
            remove(&p)?;
        }
    }
    Ok(freed)
}

fn remove(p: &Path) -> io::Result<()> {
    let meta = fs::symlink_metadata(p)?;
    if meta.is_dir() {
        fs::remove_dir_all(p)
    } else {
        fs::remove_file(p)
    }
}

pub fn size_of(p: &Path) -> u64 {
    let Ok(meta) = fs::symlink_metadata(p) else {
        return 0;
    };
    if !meta.is_dir() {
        return meta.len();
    }
    fs::read_dir(p)
        .map(|rd| rd.flatten().map(|e| size_of(&e.path())).sum())
        .unwrap_or(0)
}

fn find_dirs_named(root: &Path, name: &str, out: &mut Vec<PathBuf>) {
    let Ok(rd) = fs::read_dir(root) else { return };
    for e in rd.flatten() {
        let p = e.path();
        if fs::symlink_metadata(&p).map(|m| m.is_dir()).unwrap_or(false) {
            if e.file_name() == name {
                out.push(p);
            } else {
                find_dirs_named(&p, name, out);
            }
        }
    }
}

/// Keep only `keep` multilibs (e.g. `thumb/v7e-m+fp/hard`) in a GCC toolchain tree.
/// Every `thumb` directory (newlib, picolibc, libgcc, libstdc++ headers) is pruned to the
/// kept sub-paths, and ARM-mode `arm` multilib directories next to them are removed.
pub fn keep_multilibs(toolchain: &Path, keep: &[&str]) -> io::Result<u64> {
    let mut freed = 0;
    let mut thumbs = Vec::new();
    find_dirs_named(toolchain, "thumb", &mut thumbs);
    for thumb in &thumbs {
        // Paths to keep, relative to this `thumb` dir: "v7e-m+fp/hard".
        let rels: Vec<Vec<&str>> = keep
            .iter()
            .filter_map(|k| k.strip_prefix("thumb/"))
            .map(|r| r.split('/').collect())
            .collect();
        freed += prune(thumb, &rels)?;
        // Sibling ARM-mode multilibs (`.../lib/arm`, `.../12.2.0/arm`).
        if let Some(parent) = thumb.parent() {
            let arm = parent.join("arm");
            if arm.is_dir() && !keep.iter().any(|k| k.starts_with("arm/")) {
                freed += size_of(&arm);
                fs::remove_dir_all(&arm)?;
            }
        }
    }
    Ok(freed)
}

/// Remove everything under `dir` except the given relative paths (as segment lists).
fn prune(dir: &Path, keep: &[Vec<&str>]) -> io::Result<u64> {
    let mut freed = 0;
    for e in fs::read_dir(dir)?.flatten() {
        let name = e.file_name().to_string_lossy().into_owned();
        let p = e.path();
        if !p.is_dir() {
            continue; // files directly in `thumb/` are shared; keep them
        }
        let deeper: Vec<Vec<&str>> = keep
            .iter()
            .filter(|k| k.first() == Some(&name.as_str()))
            .map(|k| k[1..].to_vec())
            .collect();
        if deeper.is_empty() {
            freed += size_of(&p);
            fs::remove_dir_all(&p)?;
        } else if deeper.iter().all(|d| !d.is_empty()) {
            freed += prune(&p, &deeper)?;
        }
    }
    Ok(freed)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("mxkeys-trim-{}-{name}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(&d).unwrap();
        d
    }

    fn touch(p: &Path) {
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        fs::write(p, b"x").unwrap();
    }

    #[test]
    fn globs() {
        assert!(glob_match(
            "zephyr-sdk-*-hosttools-standalone-*.sh",
            "zephyr-sdk-x86_64-hosttools-standalone-0.9.sh"
        ));
        assert!(glob_match("doc", "doc"));
        assert!(!glob_match("doc", "docs"));
        assert!(glob_match("*", ""));
    }

    #[test]
    fn remove_globs_handles_nested_patterns() {
        let d = tmp("globs");
        touch(&d.join("doc/a.html"));
        touch(&d.join("share/cmake-3.31/Help/x.rst"));
        touch(&d.join("share/cmake-3.31/Modules/keep.cmake"));
        touch(&d.join("zephyr-sdk-x86_64-hosttools-standalone-0.9.sh"));
        let freed = remove_globs(
            &d,
            &[
                "doc".into(),
                "share/cmake-*/Help".into(),
                "zephyr-sdk-*-hosttools-standalone-*.sh".into(),
            ],
        )
        .unwrap();
        assert_eq!(freed, 3);
        assert!(!d.join("doc").exists() && !d.join("share/cmake-3.31/Help").exists());
        assert!(d.join("share/cmake-3.31/Modules/keep.cmake").exists());
    }

    #[test]
    fn multilib_pruning_keeps_only_the_needed_variant() {
        let d = tmp("multilib");
        for base in [
            "arm-zephyr-eabi/lib",
            "picolibc/arm-zephyr-eabi/lib",
            "lib/gcc/arm-zephyr-eabi/12.2.0",
        ] {
            for ml in ["v6-m/nofp", "v7e-m+fp/hard", "v7e-m+fp/softfp", "v8-m.main/nofp"] {
                touch(&d.join(base).join("thumb").join(ml).join("libc.a"));
            }
            touch(&d.join(base).join("arm/v5te/hard/libc.a"));
            touch(&d.join(base).join("libc.a"));
        }
        keep_multilibs(&d, &["thumb/v7e-m+fp/hard"]).unwrap();
        for base in [
            "arm-zephyr-eabi/lib",
            "picolibc/arm-zephyr-eabi/lib",
            "lib/gcc/arm-zephyr-eabi/12.2.0",
        ] {
            let b = d.join(base);
            assert!(b.join("thumb/v7e-m+fp/hard/libc.a").exists(), "{base}");
            assert!(!b.join("thumb/v7e-m+fp/softfp").exists());
            assert!(!b.join("thumb/v6-m").exists());
            assert!(!b.join("arm").exists());
            assert!(b.join("libc.a").exists(), "default libs stay");
        }
    }
}
