//! nice!view pictures for the UI: preview with the real 1-bit result, save into the
//! config repo, remove again.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use base64::Engine;
use mxkeys_niceview::module::{self, ArtConfig, HalfArt, Mode};
use mxkeys_niceview::{lvgl, render, repo, Options, Source};
use serde::{Deserialize, Serialize};

/// Decoded pictures, so dragging a slider doesn't decode a GIF again.
#[derive(Default)]
pub struct Pictures {
    cache: Mutex<HashMap<PathBuf, Arc<Source>>>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum PictureRef {
    /// A file the user picked.
    File { path: String },
    /// A picture already saved in the repo (`nice_view_art/source/<name>`).
    Repo { name: String },
}

impl PictureRef {
    fn path(&self, repo_root: &Path) -> PathBuf {
        match self {
            PictureRef::File { path } => PathBuf::from(path),
            PictureRef::Repo { name } => repo::source_path(repo_root, name),
        }
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Preview {
    /// PNG data URLs at 1:1 (68 wide), upright as on the keyboard.
    pub frames: Vec<String>,
    pub delays_ms: Vec<u32>,
    /// Frames in the original picture.
    pub source_frames: usize,
    pub source_size: (u32, u32),
    /// Most frames that fit the flash budget.
    pub max_frames: usize,
    /// Flash used by these frames.
    pub bytes: usize,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PicturesState {
    pub config: ArtConfig,
    /// Saved in the repo already (vs. defaults).
    pub saved: bool,
    /// Why mxKeys can't add its module (another module already uses `cmake:`).
    pub module_problem: Option<String>,
    pub default_budget_kib: u32,
    pub max_budget_kib: u32,
}

pub fn state(repo_root: &Path) -> PicturesState {
    let saved = repo::load_config(repo_root);
    let yml = std::fs::read_to_string(repo_root.join(module::MODULE_YML)).ok();
    PicturesState {
        saved: saved.is_some(),
        config: saved.unwrap_or_default(),
        module_problem: module::add_to_module_yml(yml.as_deref())
            .err()
            .map(|e| e.to_string()),
        default_budget_kib: module::DEFAULT_BUDGET_KIB,
        max_budget_kib: module::MAX_BUDGET_KIB,
    }
}

#[derive(Debug, Serialize)]
pub struct SourceImage {
    pub url: String,
    pub width: u32,
    pub height: u32,
}

impl Pictures {
    fn load(&self, path: &Path) -> Result<Arc<Source>, String> {
        if let Some(s) = self.cache.lock().unwrap().get(path) {
            return Ok(s.clone());
        }
        let meta = std::fs::metadata(path).map_err(|e| format!("couldn't open {}: {e}", path.display()))?;
        if meta.len() > 25 * 1024 * 1024 {
            return Err("That file is over 25 MB — please pick a smaller picture.".into());
        }
        let bytes = std::fs::read(path).map_err(|e| format!("couldn't read {}: {e}", path.display()))?;
        let src = Arc::new(render::load(&bytes).map_err(|e| e.to_string())?);
        let mut cache = self.cache.lock().unwrap();
        if cache.len() > 8 {
            cache.clear();
        }
        cache.insert(path.to_path_buf(), src.clone());
        Ok(src)
    }

    /// The original picture (first frame) for the position editor, as a PNG data URL,
    /// at the same resolution the renderer works from.
    pub fn source_image(&self, repo_root: &Path, picture: &PictureRef) -> Result<SourceImage, String> {
        let src = self.load(&picture.path(repo_root))?;
        let (img, _) = src.frames.first().ok_or("the picture is empty")?;
        let png = render::rgba_png(img);
        let b64 = base64::engine::general_purpose::STANDARD;
        Ok(SourceImage {
            url: format!("data:image/png;base64,{}", b64.encode(png)),
            width: img.width(),
            height: img.height(),
        })
    }

    pub fn preview(
        &self,
        repo_root: &Path,
        picture: &PictureRef,
        mode: Mode,
        options: &Options,
        budget_kib: u32,
    ) -> Result<Preview, String> {
        let area = mode.area().ok_or("Choose where the picture goes first.")?;
        let src = self.load(&picture.path(repo_root))?;
        let max_frames = module::max_frames(area, budget_kib);
        let frames = render::render(&src, area, options, max_frames);
        let b64 = base64::engine::general_purpose::STANDARD;
        Ok(Preview {
            frames: frames
                .iter()
                .map(|f| format!("data:image/png;base64,{}", b64.encode(render::preview_png(f, 1))))
                .collect(),
            delays_ms: frames.iter().map(|f| f.delay_ms).collect(),
            source_frames: src.frames.len(),
            source_size: (src.width, src.height),
            max_frames,
            bytes: frames.iter().map(lvgl::frame_bytes).sum(),
        })
    }
}

/// Save: copy newly picked pictures in as `left.<ext>` / `right.<ext>` and write the
/// module.
pub fn save(
    repo_root: &Path,
    mut config: ArtConfig,
    left: Option<PictureRef>,
    right: Option<PictureRef>,
) -> Result<repo::Saved, String> {
    let mut new_sources = Vec::new();
    for (half, pic, name) in [
        (&mut config.left, left, "left"),
        (&mut config.right, right, "right"),
    ] {
        if half.mode == Mode::Off {
            *half = HalfArt {
                mode: Mode::Off,
                source: None,
                options: half.options,
            };
            continue;
        }
        match pic {
            Some(PictureRef::File { path }) => {
                let p = PathBuf::from(&path);
                let ext = p
                    .extension()
                    .and_then(|e| e.to_str())
                    .unwrap_or("png")
                    .to_ascii_lowercase();
                let bytes = std::fs::read(&p).map_err(|e| format!("couldn't read {path}: {e}"))?;
                let file = format!("{name}.{ext}");
                new_sources.push((file.clone(), bytes));
                half.source = Some(file);
            }
            Some(PictureRef::Repo { name: n }) => half.source = Some(n),
            None => {}
        }
    }
    repo::save(repo_root, &config, &new_sources).map_err(|e| e.to_string())
}

pub fn remove(repo_root: &Path) -> Result<(), String> {
    repo::remove(repo_root).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preview_then_save() {
        let dir = std::env::temp_dir().join(format!("mxkeys-pictures-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let pic = dir.join("cat.png");
        image::RgbaImage::from_pixel(40, 80, image::Rgba([200, 200, 200, 255]))
            .save(&pic)
            .unwrap();
        let p = Pictures::default();
        let r = PictureRef::File {
            path: pic.display().to_string(),
        };
        let pv = p.preview(&dir, &r, Mode::Full, &Options::default(), 64).unwrap();
        assert_eq!(pv.frames.len(), 1);
        assert!(pv.frames[0].starts_with("data:image/png;base64,"));
        assert_eq!((pv.source_frames, pv.source_size, pv.bytes), (1, (40, 80), 1368));
        assert!(p.preview(&dir, &r, Mode::Off, &Options::default(), 64).is_err());

        let cfg = ArtConfig {
            right: HalfArt {
                mode: Mode::Art,
                source: None,
                options: Options::default(),
            },
            ..ArtConfig::default()
        };
        let saved = save(&dir, cfg, None, Some(r)).unwrap();
        assert_eq!(saved.right_frames, 1);
        let st = state(&dir);
        assert!(st.saved);
        assert_eq!(st.config.right.source.as_deref(), Some("right.png"));
        // Re-preview from the saved copy.
        let again = p
            .preview(
                &dir,
                &PictureRef::Repo {
                    name: "right.png".into(),
                },
                Mode::Art,
                &Options::default(),
                64,
            )
            .unwrap();
        assert_eq!(again.bytes, 1232);
        remove(&dir).unwrap();
        assert!(!state(&dir).saved);
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
