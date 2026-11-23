//! Developer helper: put pictures into a config repo without the app.
//!
//! cargo run -p mxkeys-niceview --example save -- <repo> <left-picture|-> <right-picture|->
//! (left is shown full screen, right in the stock picture area)

use mxkeys_niceview::module::{ArtConfig, HalfArt, Mode};
use mxkeys_niceview::repo;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let [repo_dir, left, right] = args.as_slice() else {
        eprintln!("usage: save <repo> <left-picture|-> <right-picture|->");
        std::process::exit(2);
    };
    let mut sources = Vec::new();
    let mut half = |path: &str, mode: Mode, name: &str| -> HalfArt {
        if path == "-" {
            return HalfArt::default();
        }
        let ext = std::path::Path::new(path)
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("png");
        let file = format!("{name}.{ext}");
        sources.push((file.clone(), std::fs::read(path).expect("read picture")));
        HalfArt {
            mode,
            source: Some(file),
            options: Default::default(),
        }
    };
    let cfg = ArtConfig {
        left: half(left, Mode::Full, "left"),
        right: half(right, Mode::Art, "right"),
        ..ArtConfig::default()
    };
    let saved = repo::save(std::path::Path::new(repo_dir), &cfg, &sources).expect("save");
    println!("{saved:#?}");
}
