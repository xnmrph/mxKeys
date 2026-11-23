//! Build ZMK firmware for a config repo with the managed toolchain — the same code path
//! the app uses. Used by CI and handy for development.
//!
//! ```text
//! mxkeys-build <config-repo> [--data DIR] [--out DIR] [--target left|right|settings_reset]...
//!              [--update] [--full-workspace] [--pristine]
//! ```

use std::path::PathBuf;
use std::process::ExitCode;

use mxkeys_hardware::profile::TargetId;
use mxkeys_toolchain::install::Progress;
use mxkeys_toolchain::manifest::Manifest;
use mxkeys_toolchain::pipeline::{self, Event, Options};
use mxkeys_toolchain::runner::ProcessRunner;

fn usage() -> ExitCode {
    eprintln!("usage: mxkeys-build <config-repo> [--data DIR] [--out DIR] [--target left|right|settings_reset]... [--update] [--full-workspace] [--pristine] [--verbose]");
    ExitCode::from(2)
}

fn default_data() -> PathBuf {
    std::env::var_os("MXKEYS_DATA")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            let home = std::env::var_os("HOME")
                .or_else(|| std::env::var_os("USERPROFILE"))
                .unwrap_or_default();
            PathBuf::from(home).join(".mxkeys-build")
        })
}

fn main() -> ExitCode {
    let mut args = std::env::args().skip(1);
    let mut repo = None;
    let mut data = default_data();
    let mut out = None;
    let mut only = Vec::new();
    let (mut update, mut full, mut pristine, mut verbose) = (false, false, false, false);
    while let Some(a) = args.next() {
        match a.as_str() {
            "--data" => data = args.next().map(PathBuf::from).unwrap_or(data),
            "--out" => out = args.next().map(PathBuf::from),
            "--target" => match args.next().as_deref() {
                Some("left") => only.push(TargetId::Left),
                Some("right") => only.push(TargetId::Right),
                Some("settings_reset") => only.push(TargetId::SettingsReset),
                _ => return usage(),
            },
            "--update" => update = true,
            "--full-workspace" => full = true,
            "--pristine" => pristine = true,
            "--verbose" | "-v" => verbose = true,
            "-h" | "--help" => return usage(),
            s if !s.starts_with('-') && repo.is_none() => repo = Some(PathBuf::from(s)),
            _ => return usage(),
        }
    }
    let Some(repo) = repo else { return usage() };
    let opts = Options {
        data,
        repo,
        profile: None,
        only,
        update_workspace: update,
        full_workspace: full,
        pristine,
    };
    let manifest = Manifest::embedded();
    let mut last_pct: Option<(String, u64)> = None;
    let result = pipeline::run(&opts, &manifest, &ProcessRunner, &mut |e| match e {
        Event::Stage { text } => eprintln!("==> {text}"),
        Event::Install(Progress::Step { component, step }) => eprintln!("    {component}: {step}"),
        Event::Install(Progress::Bytes {
            component,
            done,
            total: Some(total),
        }) => {
            let pct = done * 100 / total.max(1);
            if last_pct.as_ref() != Some(&(component.clone(), pct / 10)) {
                eprintln!("    {component}: {pct}% of {} MB", total / 1_048_576);
                last_pct = Some((component, pct / 10));
            }
        }
        Event::Install(Progress::Done { component, freed }) if freed > 0 => {
            eprintln!("    {component}: done (trimmed {} MB)", freed / 1_048_576)
        }
        Event::Install(Progress::Line { text }) | Event::Line { text, .. } if verbose => {
            eprintln!("    {text}")
        }
        Event::Line { text, .. } if text.contains("FLASH:") || text.starts_with("Wrote ") => {
            eprintln!("    {}", text.trim())
        }
        Event::TargetFinished {
            target, ok, problems, ..
        } => {
            eprintln!(
                "    {}: {}",
                pipeline::target_name(target),
                if ok { "OK" } else { "FAILED" }
            );
            for p in problems {
                eprintln!("      ✗ {}", p.title);
                if let Some(l) = p.location {
                    eprintln!(
                        "        at {}:{}{}",
                        l.file.display(),
                        l.line,
                        match (l.layer, l.key) {
                            (Some(layer), Some(k)) => format!(" (layer “{layer}”, key {k})"),
                            (Some(layer), None) => format!(" (layer “{layer}”)"),
                            _ => String::new(),
                        }
                    );
                }
                if let Some(h) = p.hint {
                    eprintln!("        {h}");
                }
                eprintln!("        log: {}", p.raw);
            }
        }
        _ => {}
    });
    match result {
        Ok(outcome) => {
            if let Some(out) = out {
                let _ = std::fs::create_dir_all(&out);
                for r in &outcome.results {
                    if let Some(u) = &r.uf2 {
                        let _ = std::fs::copy(u, out.join(u.file_name().unwrap()));
                    }
                }
                eprintln!("==> Firmware copied to {}", out.display());
            } else {
                eprintln!("==> Firmware in {}", outcome.firmware_dir.display());
            }
            if outcome.ok() {
                ExitCode::SUCCESS
            } else {
                ExitCode::FAILURE
            }
        }
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
}
