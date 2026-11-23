//! Reading and writing the nice!view pictures in a config repo.

use std::path::{Path, PathBuf};

use crate::module::{self, ArtConfig, HalfArt, DIR, MODULE_YML};
use crate::render::{self, Frame, RenderError};

#[derive(Debug, thiserror::Error)]
pub enum SaveError {
    #[error(transparent)]
    Render(#[from] RenderError),
    #[error(transparent)]
    ModuleYml(#[from] module::ModuleYmlError),
    #[error("couldn't write {0}: {1}")]
    Io(PathBuf, std::io::Error),
    #[error("the {0} half has no picture chosen")]
    NoPicture(&'static str),
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct Saved {
    pub left_frames: usize,
    pub right_frames: usize,
    /// Flash used by the pictures, per half.
    pub left_bytes: usize,
    pub right_bytes: usize,
    pub written: Vec<String>,
}

pub fn config_path(repo: &Path) -> PathBuf {
    repo.join(DIR).join("mxkeys.json")
}

pub fn source_path(repo: &Path, name: &str) -> PathBuf {
    repo.join(DIR).join("source").join(name)
}

/// The saved settings, if the repo has pictures from mxKeys.
pub fn load_config(repo: &Path) -> Option<ArtConfig> {
    let text = std::fs::read_to_string(config_path(repo)).ok()?;
    serde_json::from_str(&text).ok()
}

fn write(repo: &Path, rel: &str, bytes: &[u8]) -> Result<(), SaveError> {
    let p = repo.join(rel);
    if let Some(dir) = p.parent() {
        std::fs::create_dir_all(dir).map_err(|e| SaveError::Io(dir.to_path_buf(), e))?;
    }
    // Leave unchanged files alone (keeps git status and build caches quiet).
    if std::fs::read(&p).ok().as_deref() == Some(bytes) {
        return Ok(());
    }
    std::fs::write(&p, bytes).map_err(|e| SaveError::Io(p, e))
}

/// Render one half from its saved source picture.
pub fn render_half(
    repo: &Path,
    half: &HalfArt,
    budget_kib: u32,
    which: &'static str,
) -> Result<Vec<Frame>, SaveError> {
    let Some(area) = half.mode.area() else {
        return Ok(vec![]);
    };
    let name = half.source.as_deref().ok_or(SaveError::NoPicture(which))?;
    let p = source_path(repo, name);
    let bytes = std::fs::read(&p).map_err(|e| SaveError::Io(p, e))?;
    let src = render::load(&bytes)?;
    Ok(render::render(
        &src,
        area,
        &half.options,
        module::max_frames(area, budget_kib),
    ))
}

/// Save `cfg` into the repo. `new_sources` are pictures to copy into `source/` first
/// (file name → bytes). Sources no longer used are deleted.
pub fn save(repo: &Path, cfg: &ArtConfig, new_sources: &[(String, Vec<u8>)]) -> Result<Saved, SaveError> {
    let mut written = Vec::new();
    for (name, bytes) in new_sources {
        let rel = format!("{DIR}/source/{name}");
        write(repo, &rel, bytes)?;
        written.push(rel);
    }
    let left = render_half(repo, &cfg.left, cfg.budget_kib, "left")?;
    let right = render_half(repo, &cfg.right, cfg.budget_kib, "right")?;
    let yml_path = repo.join(MODULE_YML);
    let yml = std::fs::read_to_string(&yml_path).ok();
    let new_yml = module::add_to_module_yml(yml.as_deref())?;
    for (rel, bytes) in module::generated_files(cfg, &left, &right) {
        write(repo, &rel, &bytes)?;
        written.push(rel);
    }
    write(repo, MODULE_YML, new_yml.as_bytes())?;
    written.push(MODULE_YML.into());
    // Drop source pictures nothing refers to any more.
    let used: Vec<&str> = [&cfg.left, &cfg.right]
        .iter()
        .filter_map(|h| h.source.as_deref())
        .collect();
    if let Ok(dir) = std::fs::read_dir(repo.join(DIR).join("source")) {
        for e in dir.flatten() {
            let n = e.file_name().to_string_lossy().into_owned();
            if !used.contains(&n.as_str()) {
                let _ = std::fs::remove_file(e.path());
            }
        }
    }
    let bytes = |f: &[Frame]| f.iter().map(crate::lvgl::frame_bytes).sum();
    Ok(Saved {
        left_frames: left.len(),
        right_frames: right.len(),
        left_bytes: bytes(&left),
        right_bytes: bytes(&right),
        written,
    })
}

/// Remove the pictures: delete the folder and mxKeys' lines in `zephyr/module.yml`
/// (the file itself if mxKeys created it).
pub fn remove(repo: &Path) -> Result<(), SaveError> {
    let dir = repo.join(DIR);
    if dir.exists() {
        std::fs::remove_dir_all(&dir).map_err(|e| SaveError::Io(dir, e))?;
    }
    let yml_path = repo.join(MODULE_YML);
    if let Ok(text) = std::fs::read_to_string(&yml_path) {
        match module::remove_from_module_yml(&text) {
            Some(t) if t != text => write(repo, MODULE_YML, t.as_bytes())?,
            Some(_) => {}
            None => {
                std::fs::remove_file(&yml_path).map_err(|e| SaveError::Io(yml_path.clone(), e))?;
                let _ = std::fs::remove_dir(repo.join("zephyr"));
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::module::Mode;
    use crate::render::{Dither, Options};

    fn png() -> Vec<u8> {
        let img = image::RgbaImage::from_fn(30, 30, |x, y| {
            image::Rgba(if (x / 5 + y / 5) % 2 == 0 {
                [0, 0, 0, 255]
            } else {
                [255; 4]
            })
        });
        let mut out = Vec::new();
        image::DynamicImage::ImageRgba8(img)
            .write_to(&mut std::io::Cursor::new(&mut out), image::ImageFormat::Png)
            .unwrap();
        out
    }

    #[test]
    fn save_and_remove_in_a_repo_with_module_yml() {
        let repo = std::env::temp_dir().join(format!("mxkeys-nv-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&repo);
        std::fs::create_dir_all(repo.join("zephyr")).unwrap();
        let owner_yml = "build:\n  settings:\n    board_root: .\n";
        std::fs::write(repo.join(MODULE_YML), owner_yml).unwrap();

        let cfg = ArtConfig {
            right: HalfArt {
                mode: Mode::Art,
                source: Some("right.png".into()),
                options: Options {
                    dither: Dither::Threshold,
                    ..Options::default()
                },
            },
            ..ArtConfig::default()
        };
        let s = save(&repo, &cfg, &[("right.png".into(), png())]).unwrap();
        assert_eq!((s.left_frames, s.right_frames, s.right_bytes), (0, 1, 1232));
        assert_eq!(load_config(&repo), Some(cfg.clone()));
        let yml = std::fs::read_to_string(repo.join(MODULE_YML)).unwrap();
        assert!(yml.contains("cmake: nice_view_art") && yml.contains("board_root: ."));
        let frames = std::fs::read_to_string(repo.join(DIR).join("src/frames.c")).unwrap();
        assert!(frames.contains("/* Right half: 1 frame(s) */") && frames.contains(".header.w = 140,"));

        // Switching the right half off drops its now unused source picture.
        let off = ArtConfig {
            right: HalfArt {
                mode: Mode::Off,
                source: None,
                ..cfg.right.clone()
            },
            ..cfg
        };
        save(&repo, &off, &[]).unwrap();
        assert!(!source_path(&repo, "right.png").exists());

        remove(&repo).unwrap();
        assert!(!repo.join(DIR).exists());
        assert_eq!(std::fs::read_to_string(repo.join(MODULE_YML)).unwrap(), owner_yml);
        std::fs::remove_dir_all(&repo).unwrap();
    }

    #[test]
    fn module_yml_created_by_us_is_removed_again() {
        let repo = std::env::temp_dir().join(format!("mxkeys-nv2-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&repo);
        std::fs::create_dir_all(&repo).unwrap();
        let cfg = ArtConfig {
            left: HalfArt {
                mode: Mode::Full,
                source: Some("l.png".into()),
                options: Options::default(),
            },
            ..ArtConfig::default()
        };
        let s = save(&repo, &cfg, &[("l.png".into(), png())]).unwrap();
        assert_eq!((s.left_frames, s.left_bytes), (1, 1368));
        assert!(repo.join(MODULE_YML).exists());
        remove(&repo).unwrap();
        assert!(!repo.join("zephyr").exists());
        assert!(matches!(
            save(
                &repo,
                &ArtConfig {
                    left: HalfArt {
                        source: None,
                        ..cfg.left.clone()
                    },
                    ..cfg
                },
                &[]
            ),
            Err(SaveError::NoPicture("left"))
        ));
        std::fs::remove_dir_all(&repo).unwrap();
    }
}
