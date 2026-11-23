//! Lossless model of Kconfig fragment files (`config/*.conf`).
//!
//! A `.conf` file is line-based: `CONFIG_NAME=value`, comments (`# …`), the special
//! `# CONFIG_NAME is not set`, and blank lines. Users often keep *commented-out*
//! settings (`#CONFIG_ZMK_RGB_UNDERGLOW=y`) as switches to flip later; we recognise those
//! too so the settings page can "uncomment" instead of appending a duplicate.
//!
//! Like the keymap model, this is a view with spans into the original text; edits are
//! [`Patch`]es, so everything the app doesn't touch stays byte-identical.

use serde::Serialize;

use crate::dts::{apply_patches, Patch, PatchError, Span};

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "type", content = "value", rename_all = "snake_case")]
pub enum KconfigValue {
    Bool(bool),
    Int(i64),
    /// Hex literal, kept as written (e.g. `0x1D50`).
    Hex(String),
    /// Unescaped string content.
    Str(String),
    /// Anything else, kept raw.
    Raw(String),
}

impl KconfigValue {
    pub fn parse(raw: &str) -> KconfigValue {
        let raw = raw.trim();
        match raw {
            "y" => return KconfigValue::Bool(true),
            "n" => return KconfigValue::Bool(false),
            _ => {}
        }
        if raw.len() >= 2 && raw.starts_with('"') && raw.ends_with('"') {
            return KconfigValue::Str(crate::dts::unescape_string(raw));
        }
        if let Some(hex) = raw.strip_prefix("0x").or_else(|| raw.strip_prefix("0X")) {
            if !hex.is_empty() && hex.chars().all(|c| c.is_ascii_hexdigit()) {
                return KconfigValue::Hex(raw.to_string());
            }
        }
        if let Ok(i) = raw.parse::<i64>() {
            return KconfigValue::Int(i);
        }
        KconfigValue::Raw(raw.to_string())
    }

    /// Text as written to a `.conf` file.
    pub fn to_conf(&self) -> String {
        match self {
            KconfigValue::Bool(true) => "y".into(),
            KconfigValue::Bool(false) => "n".into(),
            KconfigValue::Int(i) => i.to_string(),
            KconfigValue::Hex(h) => h.clone(),
            KconfigValue::Str(s) => format!("\"{}\"", crate::dts::escape_string(s)),
            KconfigValue::Raw(r) => r.clone(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SettingState {
    /// `CONFIG_X=…` — in effect.
    Active,
    /// `#CONFIG_X=…` / `# CONFIG_X=…` — a switched-off line the user kept around.
    CommentedOut,
    /// `# CONFIG_X is not set` — explicitly `n` (Kconfig syntax).
    NotSet,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Setting {
    /// Full name including the `CONFIG_` prefix.
    pub name: String,
    pub value: KconfigValue,
    pub state: SettingState,
    /// The whole line, excluding its newline.
    pub line_span: Span,
    /// Just the value text (for `NotSet`, an empty span at the end of the name).
    pub value_span: Span,
    /// 1-based line number.
    pub line: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct KconfigFile {
    source: String,
    pub settings: Vec<Setting>,
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum KconfigError {
    #[error("`{0}` is not a valid Kconfig option name (expected CONFIG_ followed by A–Z, 0–9, _)")]
    InvalidName(String),
    #[error(transparent)]
    Patch(#[from] PatchError),
}

fn is_valid_name(name: &str) -> bool {
    name.strip_prefix("CONFIG_")
        .is_some_and(|rest| !rest.is_empty() && rest.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_'))
}

/// Split `src` into `(line_span, line_text)` without newlines.
fn lines(src: &str) -> Vec<Span> {
    let mut out = Vec::new();
    let mut start = 0;
    let b = src.as_bytes();
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'\n' || b[i] == b'\r' {
            out.push(Span::new(start, i));
            if b[i] == b'\r' && b.get(i + 1) == Some(&b'\n') {
                i += 1;
            }
            start = i + 1;
        }
        i += 1;
    }
    if start < b.len() {
        out.push(Span::new(start, b.len()));
    }
    out
}

/// Parse `NAME=value` at `text` (relative offsets). Returns (name, value_start, value_end).
fn parse_assignment(text: &str) -> Option<(&str, usize, usize)> {
    let eq = text.find('=')?;
    let name = text[..eq].trim_end();
    if !is_valid_name(name) {
        return None;
    }
    let after = &text[eq + 1..];
    let lead = after.len() - after.trim_start().len();
    let value = after.trim();
    let start = eq + 1 + lead;
    Some((name, start, start + value.len()))
}

impl KconfigFile {
    pub fn parse(source: impl Into<String>) -> KconfigFile {
        let source = source.into();
        let mut settings = Vec::new();
        for (idx, span) in lines(&source).into_iter().enumerate() {
            let text = &source[span.range()];
            let indent = text.len() - text.trim_start().len();
            let body = &text[indent..];
            let line = idx + 1;
            if let Some(comment) = body.strip_prefix('#') {
                let c_lead = comment.len() - comment.trim_start().len();
                let inner = &comment[c_lead..];
                let inner_off = indent + 1 + c_lead;
                if let Some(name) = inner.strip_suffix("is not set").map(str::trim_end) {
                    if is_valid_name(name) {
                        let end = span.start + inner_off + name.len();
                        settings.push(Setting {
                            name: name.to_string(),
                            value: KconfigValue::Bool(false),
                            state: SettingState::NotSet,
                            line_span: span,
                            value_span: Span::new(end, end),
                            line,
                        });
                        continue;
                    }
                }
                if let Some((name, vs, ve)) = parse_assignment(inner) {
                    settings.push(Setting {
                        name: name.to_string(),
                        value: KconfigValue::parse(&inner[vs..ve]),
                        state: SettingState::CommentedOut,
                        line_span: span,
                        value_span: Span::new(span.start + inner_off + vs, span.start + inner_off + ve),
                        line,
                    });
                }
                continue;
            }
            if let Some((name, vs, ve)) = parse_assignment(body) {
                settings.push(Setting {
                    name: name.to_string(),
                    value: KconfigValue::parse(&body[vs..ve]),
                    state: SettingState::Active,
                    line_span: span,
                    value_span: Span::new(span.start + indent + vs, span.start + indent + ve),
                    line,
                });
            }
        }
        KconfigFile { source, settings }
    }

    pub fn source(&self) -> &str {
        &self.source
    }

    /// The setting in effect for `name`: the *last* active or not-set line (later lines
    /// override earlier ones in Kconfig fragments).
    pub fn effective(&self, name: &str) -> Option<&Setting> {
        self.settings
            .iter()
            .rev()
            .find(|s| s.name == name && s.state != SettingState::CommentedOut)
    }

    /// Effective value, if the file sets it.
    pub fn get(&self, name: &str) -> Option<&KconfigValue> {
        self.effective(name).map(|s| &s.value)
    }

    /// Names that are set (actively) more than once — worth a warning in the UI.
    pub fn duplicates(&self) -> Vec<&str> {
        let mut seen: Vec<&str> = Vec::new();
        let mut dups: Vec<&str> = Vec::new();
        for s in self
            .settings
            .iter()
            .filter(|s| s.state != SettingState::CommentedOut)
        {
            if seen.contains(&s.name.as_str()) {
                if !dups.contains(&s.name.as_str()) {
                    dups.push(&s.name);
                }
            } else {
                seen.push(&s.name);
            }
        }
        dups
    }

    fn newline(&self) -> &'static str {
        match self.source.find('\n') {
            Some(i) if i > 0 && self.source.as_bytes()[i - 1] == b'\r' => "\r\n",
            _ => "\n",
        }
    }

    /// Patch that makes `name` = `value`:
    /// 1. an effective line exists → replace its value (or rewrite a `is not set` line);
    /// 2. else a commented-out line exists → uncomment it with the new value (last one);
    /// 3. else append `NAME=value` at the end of the file.
    pub fn set(&self, name: &str, value: &KconfigValue) -> Result<Patch, KconfigError> {
        if !is_valid_name(name) {
            return Err(KconfigError::InvalidName(name.to_string()));
        }
        let text = value.to_conf();
        if let Some(s) = self.effective(name) {
            return Ok(match s.state {
                SettingState::NotSet => Patch::replace(s.line_span, format!("{name}={text}")),
                _ => Patch::replace(s.value_span, text),
            });
        }
        if let Some(s) = self.settings.iter().rev().find(|s| s.name == name) {
            let indent = {
                let line = &self.source[s.line_span.range()];
                &line[..line.len() - line.trim_start().len()]
            };
            return Ok(Patch::replace(s.line_span, format!("{indent}{name}={text}")));
        }
        let nl = self.newline();
        let needs_nl =
            !self.source.is_empty() && !self.source.ends_with('\n') && !self.source.ends_with('\r');
        Ok(Patch::insert(
            self.source.len(),
            format!("{}{name}={text}{nl}", if needs_nl { nl } else { "" }),
        ))
    }

    /// Patch that turns the effective line for `name` into a commented-out line, so the
    /// Kconfig default applies again but the user's value is kept for later. `None` if
    /// the file doesn't set it.
    pub fn comment_out(&self, name: &str) -> Option<Patch> {
        let s = self.effective(name)?;
        Some(match s.state {
            SettingState::NotSet => Patch::replace(s.line_span, format!("# {name}=n")),
            _ => Patch::insert(s.line_span.start + self.indent_len(s), "# "),
        })
    }

    fn indent_len(&self, s: &Setting) -> usize {
        let line = &self.source[s.line_span.range()];
        line.len() - line.trim_start().len()
    }

    pub fn apply(&self, patches: &[Patch]) -> Result<KconfigFile, KconfigError> {
        Ok(KconfigFile::parse(apply_patches(&self.source, patches)?))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn values() {
        assert_eq!(KconfigValue::parse("y"), KconfigValue::Bool(true));
        assert_eq!(KconfigValue::parse("n"), KconfigValue::Bool(false));
        assert_eq!(KconfigValue::parse("300000"), KconfigValue::Int(300000));
        assert_eq!(KconfigValue::parse("-4"), KconfigValue::Int(-4));
        assert_eq!(KconfigValue::parse("0x1D50"), KconfigValue::Hex("0x1D50".into()));
        assert_eq!(
            KconfigValue::parse("\"Corne \\\"x\\\"\""),
            KconfigValue::Str("Corne \"x\"".into())
        );
        assert_eq!(KconfigValue::Str("a\"b".into()).to_conf(), "\"a\\\"b\"");
        assert_eq!(KconfigValue::parse("m"), KconfigValue::Raw("m".into()));
    }

    #[test]
    fn states_and_effective_value() {
        let f = KconfigFile::parse(
            "CONFIG_A=y\n#CONFIG_B=y\n# CONFIG_C is not set\n  CONFIG_A=n  \n# just a comment = x\nCONFIG_lower=1\n",
        );
        let names: Vec<_> = f.settings.iter().map(|s| (s.name.as_str(), s.state)).collect();
        assert_eq!(
            names,
            [
                ("CONFIG_A", SettingState::Active),
                ("CONFIG_B", SettingState::CommentedOut),
                ("CONFIG_C", SettingState::NotSet),
                ("CONFIG_A", SettingState::Active),
                ("CONFIG_lower", SettingState::Active),
            ]
        );
        assert_eq!(f.get("CONFIG_A"), Some(&KconfigValue::Bool(false)));
        assert_eq!(f.get("CONFIG_B"), None);
        assert_eq!(f.get("CONFIG_C"), Some(&KconfigValue::Bool(false)));
        assert_eq!(f.duplicates(), ["CONFIG_A"]);
        assert_eq!(f.effective("CONFIG_A").unwrap().line, 4);
    }

    #[test]
    fn set_replaces_uncomments_or_appends() {
        let src = "CONFIG_A=y\n#CONFIG_B=y\n# CONFIG_C is not set\nCONFIG_D=1";
        let f = KconfigFile::parse(src);
        let out = |p: Patch| f.apply(&[p]).unwrap().source().to_string();
        assert_eq!(
            out(f.set("CONFIG_A", &KconfigValue::Bool(false)).unwrap()),
            src.replace("CONFIG_A=y", "CONFIG_A=n")
        );
        assert_eq!(
            out(f.set("CONFIG_B", &KconfigValue::Bool(true)).unwrap()),
            src.replace("#CONFIG_B=y", "CONFIG_B=y")
        );
        assert_eq!(
            out(f.set("CONFIG_C", &KconfigValue::Bool(true)).unwrap()),
            src.replace("# CONFIG_C is not set", "CONFIG_C=y")
        );
        assert_eq!(
            out(f.set("CONFIG_E", &KconfigValue::Int(5)).unwrap()),
            format!("{src}\nCONFIG_E=5\n")
        );
        assert_eq!(
            f.set("ZMK_SLEEP", &KconfigValue::Bool(true)),
            Err(KconfigError::InvalidName("ZMK_SLEEP".into()))
        );
        let crlf = KconfigFile::parse("CONFIG_A=y\r\n");
        let p = crlf.set("CONFIG_B", &KconfigValue::Bool(true)).unwrap();
        assert_eq!(crlf.apply(&[p]).unwrap().source(), "CONFIG_A=y\r\nCONFIG_B=y\r\n");
        let empty = KconfigFile::parse("");
        let p = empty.set("CONFIG_A", &KconfigValue::Bool(true)).unwrap();
        assert_eq!(empty.apply(&[p]).unwrap().source(), "CONFIG_A=y\n");
    }

    #[test]
    fn comment_out_keeps_the_value() {
        let f = KconfigFile::parse("  CONFIG_A=y\n# CONFIG_C is not set\n");
        let g = f.apply(&[f.comment_out("CONFIG_A").unwrap()]).unwrap();
        assert_eq!(g.source(), "  # CONFIG_A=y\n# CONFIG_C is not set\n");
        assert_eq!(g.get("CONFIG_A"), None);
        let h = f.apply(&[f.comment_out("CONFIG_C").unwrap()]).unwrap();
        assert_eq!(h.source(), "  CONFIG_A=y\n# CONFIG_C=n\n");
        assert!(f.comment_out("CONFIG_X").is_none());
    }
}
