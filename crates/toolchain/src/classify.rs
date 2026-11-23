//! Turning build logs into plain-language problems, pointing at the layer and key in the
//! keymap where possible. Patterns come from real ZMK/Zephyr 4.1 build output (captured
//! by breaking a copy of the owner's config on purpose — see the tests).

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use serde::Serialize;
use zmk_config::dts::Document;
use zmk_config::keymap::Keymap;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Location {
    pub file: PathBuf,
    pub line: usize,
    pub layer: Option<String>,
    /// 1-based key number on the layer.
    pub key: Option<usize>,
    pub binding: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Problem {
    /// Plain language, e.g. "Unknown key name “WWW”".
    pub title: String,
    /// What to do about it.
    pub hint: Option<String>,
    pub location: Option<Location>,
    /// The original log line (for the "Advanced" view).
    pub raw: String,
}

/// What the classifier may use to be more specific.
#[derive(Debug, Default, Clone)]
pub struct Context<'a> {
    /// The user's keymap file and its text.
    pub keymap: Option<(&'a Path, &'a str)>,
    /// Key names ZMK knows (from `dt-bindings/zmk/keys.h`, see [`names_defined_in`]).
    pub known_names: Option<&'a HashSet<String>>,
}

/// Every `#define NAME` in a header (keycode names, layer names…).
pub fn names_defined_in(header: &str) -> HashSet<String> {
    header
        .lines()
        .filter_map(|l| l.trim_start().strip_prefix("#define"))
        .filter_map(|rest| {
            let name: String = rest
                .trim_start()
                .chars()
                .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
                .collect();
            (!name.is_empty()).then_some(name)
        })
        .collect()
}

fn same_file(a: &Path, b: &str) -> bool {
    let b = Path::new(b);
    a == b || a.file_name().is_some() && a.file_name() == b.file_name() && b.ends_with(a.file_name().unwrap())
}

/// (layer title, 1-based key, binding text) for bindings on `line` (1-based).
fn bindings_on_line(src: &str, line: usize) -> Vec<(String, usize, String)> {
    let doc = Document::parse(src);
    let Ok(km) = Keymap::from_document(&doc) else {
        return Vec::new();
    };
    let line_start = src
        .split_inclusive('\n')
        .take(line.saturating_sub(1))
        .map(str::len)
        .sum::<usize>();
    let line_end = src[line_start..].find('\n').map_or(src.len(), |i| line_start + i);
    let mut out = Vec::new();
    for l in &km.layers {
        if let Some(b) = &l.bindings {
            for (i, binding) in b.bindings.iter().enumerate() {
                if binding.span.start < line_end && binding.span.end > line_start {
                    out.push((l.title(), i + 1, binding.raw.clone()));
                }
            }
        }
    }
    out
}

fn find_behavior(src: &str, label: &str) -> Option<(String, usize, String)> {
    let doc = Document::parse(src);
    let km = Keymap::from_document(&doc).ok()?;
    let wanted = format!("&{label}");
    km.layers.iter().find_map(|l| {
        let b = l.bindings.as_ref()?;
        b.bindings
            .iter()
            .enumerate()
            .find(|(_, x)| x.behavior == wanted)
            .map(|(i, x)| (l.title(), i + 1, x.raw.clone()))
    })
}

fn line_of(src: &str, offset: usize) -> usize {
    src[..offset.min(src.len())].matches('\n').count() + 1
}

/// Classify a whole log. Problems are de-duplicated and in log order.
pub fn classify(log: &[String], ctx: &Context<'_>) -> Vec<Problem> {
    let mut out: Vec<Problem> = Vec::new();
    let mut push = |p: Problem| {
        if !out.iter().any(|q| q.title == p.title) {
            out.push(p);
        }
    };
    for raw in log {
        let line = raw.trim();
        // devicetree error: /path/corne.keymap:368 (column 97): parse error: ...
        if let Some(rest) = line.strip_prefix("devicetree error: ") {
            if let Some(label) = rest
                .split("undefined node label '")
                .nth(1)
                .and_then(|r| r.split('\'').next())
            {
                let location = ctx.keymap.and_then(|(path, src)| {
                    find_behavior(src, label).map(|(layer, key, binding)| {
                        let off = src.find(&format!("&{label}")).unwrap_or(0);
                        Location {
                            file: path.to_path_buf(),
                            line: line_of(src, off),
                            layer: Some(layer),
                            key: Some(key),
                            binding: Some(binding),
                        }
                    })
                });
                push(Problem {
                    title: format!("“&{label}” isn't something ZMK knows"),
                    hint: Some("Check the spelling. If it comes from an add-on module, make sure that module is listed in config/west.yml.".into()),
                    location,
                    raw: raw.clone(),
                });
                continue;
            }
            // `<file>:<line> (column N): parse error: …`
            let (file_part, msg) = rest.split_once(": parse error: ").unwrap_or((rest, ""));
            let (file, lineno) = match file_part.split_once(" (column ") {
                Some((fl, _)) => fl
                    .rsplit_once(':')
                    .map(|(f, l)| (f, l.parse::<usize>().ok()))
                    .unwrap_or((fl, None)),
                None => (file_part, None),
            };
            let mut location = None;
            let mut title = format!("The keymap has a mistake: {msg}");
            if let (Some((path, src)), Some(n)) = (ctx.keymap, lineno) {
                if same_file(path, file) {
                    let on_line = bindings_on_line(src, n);
                    // Find the parameter that isn't a known key name or a #define.
                    let local = names_defined_in(src);
                    let unknown = on_line.iter().find_map(|(layer, key, b)| {
                        b.split_whitespace().skip(1).find_map(|p| {
                            let word: String = p
                                .chars()
                                .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
                                .collect();
                            let looks_like_name = word.chars().next().is_some_and(|c| c.is_ascii_uppercase());
                            let known =
                                ctx.known_names.is_some_and(|k| k.contains(&word)) || local.contains(&word);
                            (looks_like_name && ctx.known_names.is_some() && !known)
                                .then(|| (word, layer.clone(), *key, b.clone()))
                        })
                    });
                    match unknown {
                        Some((word, layer, key, b)) => {
                            title = format!("Unknown key name “{word}”");
                            location = Some(Location {
                                file: path.to_path_buf(),
                                line: n,
                                layer: Some(layer),
                                key: Some(key),
                                binding: Some(b),
                            });
                        }
                        None => {
                            title = format!("The keymap has a mistake on line {n}");
                            location = Some(Location {
                                file: path.to_path_buf(),
                                line: n,
                                layer: on_line.first().map(|x| x.0.clone()),
                                key: None,
                                binding: None,
                            });
                        }
                    }
                }
            }
            push(Problem {
                title,
                hint: Some("Fix that key and build again.".into()),
                location,
                raw: raw.clone(),
            });
            continue;
        }
        // keymap.keymap:1:10: fatal error: keys_xx.h: No such file or directory
        if let Some((pre, post)) = line.split_once(": fatal error: ") {
            if let Some(header) = post.strip_suffix(": No such file or directory") {
                let location = pre
                    .rsplitn(3, ':')
                    .nth(2)
                    .zip(pre.rsplit(':').nth(1).and_then(|l| l.parse().ok()))
                    .map(|(file, line)| Location {
                        file: PathBuf::from(file),
                        line,
                        layer: None,
                        key: None,
                        binding: None,
                    });
                push(Problem {
                    title: format!("The keymap includes “{header}”, which wasn't found"),
                    hint: Some("If that file comes from an add-on module (like zmk-helpers), make sure the module is listed in config/west.yml. Otherwise check the file name.".into()),
                    location,
                    raw: raw.clone(),
                });
                continue;
            }
        }
        // corne.conf:19: warning: attempt to assign the value 'y' to the undefined symbol ZMK_SLEEPY
        if let Some((pre, sym)) = line.split_once("to the undefined symbol ") {
            let location = pre
                .split(": warning:")
                .next()
                .and_then(|fl| fl.rsplit_once(':'))
                .and_then(|(f, l)| {
                    l.parse().ok().map(|line| Location {
                        file: PathBuf::from(f),
                        line,
                        layer: None,
                        key: None,
                        binding: None,
                    })
                });
            push(Problem {
                title: format!("Your settings use CONFIG_{sym}, which this ZMK version doesn't know"),
                hint: Some("Check the spelling — or the option may have been renamed or removed in this ZMK version.".into()),
                location,
                raw: raw.clone(),
            });
            continue;
        }
        if line.contains("region `FLASH' overflowed by") || line.contains("region 'FLASH' overflowed by") {
            let bytes = line
                .split("overflowed by ")
                .nth(1)
                .and_then(|r| r.split_whitespace().next())
                .unwrap_or("?");
            push(Problem {
                title: format!("The firmware is too big for the controller (by {bytes} bytes)"),
                hint: Some("Remove some images, macros or features (for example the screen or RGB), then build again.".into()),
                location: None,
                raw: raw.clone(),
            });
            continue;
        }
        if line.contains("No board named")
            || line.contains("Invalid BOARD")
            || line.contains("Board qualifiers")
        {
            push(Problem {
                title: "The controller board name isn't valid for this ZMK version".into(),
                hint: Some(
                    "Current ZMK needs names like “nice_nano@2.0.0//zmk”. Check your hardware choice.".into(),
                ),
                location: None,
                raw: raw.clone(),
            });
            continue;
        }
        if line.contains("Could not resolve host")
            || line.contains("unable to access")
            || line.contains("Failed to connect")
        {
            push(Problem {
                title: "Couldn't download ZMK's source code".into(),
                hint: Some("Check your internet connection (and proxy), then try again.".into()),
                location: None,
                raw: raw.clone(),
            });
            continue;
        }
    }
    if out.is_empty() {
        if let Some(raw) = log.iter().rev().find(|l| {
            let l = l.to_ascii_lowercase();
            (l.contains("error") || l.contains("fatal")) && !l.starts_with("-- ")
        }) {
            out.push(Problem {
                title: "The build failed".into(),
                hint: Some("See the full log for details.".into()),
                location: None,
                raw: raw.clone(),
            });
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const KEYMAP: &str = "#include <behaviors.dtsi>\n#define NAV 1\n/ {\n  keymap {\n    compatible = \"zmk,keymap\";\n    base {\n      display-name = \"Base\";\n      bindings = <\n&mt TAB ESCAPE  &kp Q  &kp WWW  &kpp E &mo NAV\n      >;\n    };\n  };\n};\n";

    fn known() -> HashSet<String> {
        names_defined_in("#define Q 1\n#define TAB 2\n#define ESCAPE 3\n#define E 4\n  #define  W (5)\n")
    }

    fn run(lines: &[&str]) -> Vec<Problem> {
        let k = known();
        let ctx = Context {
            keymap: Some((Path::new("/cfg/config/corne.keymap"), KEYMAP)),
            known_names: Some(&k),
        };
        classify(&lines.iter().map(|s| s.to_string()).collect::<Vec<_>>(), &ctx)
    }

    #[test]
    fn unknown_key_name_points_at_layer_and_key() {
        // Real message shape (Zephyr 4.1 dtlib), line 9 of KEYMAP.
        let p = run(&["devicetree error: /cfg/config/corne.keymap:9 (column 97): parse error: expected number or parenthesized expression"]);
        assert_eq!(p[0].title, "Unknown key name “WWW”");
        let l = p[0].location.as_ref().unwrap();
        assert_eq!(
            (l.layer.as_deref(), l.key, l.binding.as_deref()),
            (Some("Base"), Some(3), Some("&kp WWW"))
        );
    }

    #[test]
    fn unknown_behavior() {
        let p = run(&["devicetree error: /keymap/0: undefined node label 'kpp'"]);
        assert_eq!(p[0].title, "“&kpp” isn't something ZMK knows");
        let l = p[0].location.as_ref().unwrap();
        assert_eq!((l.layer.as_deref(), l.key, l.line), (Some("Base"), Some(4), 9));
    }

    #[test]
    fn missing_include_and_kconfig_and_others() {
        let p = run(&[
            "/cfg/config/corne.keymap:1:10: fatal error: keys_xx.h: No such file or directory",
            "/cfg/config/corne.conf:19: warning: attempt to assign the value 'y' to the undefined symbol ZMK_SLEEPY",
            "/cfg/config/corne.conf:19: warning: attempt to assign the value 'y' to the undefined symbol ZMK_SLEEPY",
            "error: Aborting due to Kconfig warnings",
            "zephyr/zmk.elf section `text' will not fit in region `FLASH'",
            "region `FLASH' overflowed by 1234 bytes",
            "fatal: unable to access 'https://github.com/zmkfirmware/zmk/': Could not resolve host: github.com",
        ]);
        let titles: Vec<&str> = p.iter().map(|x| x.title.as_str()).collect();
        assert_eq!(
            titles,
            [
                "The keymap includes “keys_xx.h”, which wasn't found",
                "Your settings use CONFIG_ZMK_SLEEPY, which this ZMK version doesn't know",
                "The firmware is too big for the controller (by 1234 bytes)",
                "Couldn't download ZMK's source code",
            ]
        );
        assert_eq!(p[0].location.as_ref().unwrap().line, 1);
        assert_eq!(p[1].location.as_ref().unwrap().line, 19);
    }

    #[test]
    fn generic_fallback_and_names() {
        let p = run(&[
            "-- Zephyr version: 4.1.0",
            "ninja: build stopped: subcommand failed.",
            "FATAL ERROR: command exited with status 1",
        ]);
        assert_eq!(p[0].title, "The build failed");
        assert!(run(&["all good"]).is_empty());
        assert!(known().contains("W"));
    }
}
