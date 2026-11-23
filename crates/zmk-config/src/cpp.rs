//! Just enough of the C preprocessor to turn binding parameters into the numbers the
//! firmware sees (for live editing): object- and function-like `#define`s (incl. `##`),
//! `#include` of headers (`.h`) along search paths, and integer expression evaluation
//! with C precedence.
//!
//! Devicetree cells after preprocessing are integers or parenthesised expressions, so
//! `&kp LS(A)` → `(0x02 << 24 | ((0x07 << 16) | 0x04))` → one cell, and
//! `&bt BT_CLR` → `3 0`… → several cells.
//!
//! Simplifications (documented, conservative): `#if`/`#ifdef` blocks are not evaluated —
//! a name's first definition wins for `#ifndef`-guarded defaults, later definitions in
//! the keymap itself override; stringification (`#x`) isn't supported.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Tok {
    Ident(String),
    Num(String),
    Punct(String),
}

impl Tok {
    fn text(&self) -> &str {
        match self {
            Tok::Ident(s) | Tok::Num(s) | Tok::Punct(s) => s,
        }
    }
}

pub fn tokenize(s: &str) -> Vec<Tok> {
    let b = s.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < b.len() {
        let c = b[i];
        if c.is_ascii_whitespace() {
            i += 1;
        } else if c == b'/' && b.get(i + 1) == Some(&b'/') {
            while i < b.len() && b[i] != b'\n' {
                i += 1;
            }
        } else if c == b'/' && b.get(i + 1) == Some(&b'*') {
            i += 2;
            while i + 1 < b.len() && !(b[i] == b'*' && b[i + 1] == b'/') {
                i += 1;
            }
            i += 2;
        } else if c.is_ascii_alphabetic() || c == b'_' {
            let st = i;
            while i < b.len() && (b[i].is_ascii_alphanumeric() || b[i] == b'_') {
                i += 1;
            }
            out.push(Tok::Ident(s[st..i].to_string()));
        } else if c.is_ascii_digit() {
            let st = i;
            while i < b.len() && (b[i].is_ascii_alphanumeric() || b[i] == b'_') {
                i += 1;
            }
            out.push(Tok::Num(s[st..i].to_string()));
        } else {
            let two = s.get(i..i + 2).unwrap_or("");
            if ["<<", ">>", "<=", ">=", "==", "!=", "&&", "||", "##"].contains(&two) {
                out.push(Tok::Punct(two.to_string()));
                i += 2;
            } else {
                out.push(Tok::Punct((c as char).to_string()));
                i += 1;
            }
        }
    }
    out
}

#[derive(Debug, Clone)]
struct Def {
    params: Option<Vec<String>>,
    body: Vec<Tok>,
}

#[derive(Debug, Clone, Default)]
pub struct Macros {
    defs: HashMap<String, Def>,
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum CppError {
    #[error("unknown name `{0}`")]
    Unknown(String),
    #[error("can't work out `{0}`")]
    Syntax(String),
    #[error("division by zero")]
    DivZero,
}

/// Logical lines of a source (backslash-continued lines joined).
fn logical_lines(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    for line in text.lines() {
        if let Some(stripped) = line.strip_suffix('\\') {
            cur.push_str(stripped);
            cur.push(' ');
        } else {
            cur.push_str(line);
            out.push(std::mem::take(&mut cur));
        }
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    out
}

fn parse_define(rest: &str) -> Option<(String, Def)> {
    let rest = rest.trim_start();
    let name_end = rest
        .find(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
        .unwrap_or(rest.len());
    let name = &rest[..name_end];
    if name.is_empty() {
        return None;
    }
    let after = &rest[name_end..];
    // Function-like only when '(' follows the name directly.
    if let Some(p) = after.strip_prefix('(') {
        let close = p.find(')')?;
        let params = p[..close]
            .split(',')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();
        Some((
            name.into(),
            Def {
                params: Some(params),
                body: tokenize(&p[close + 1..]),
            },
        ))
    } else {
        Some((
            name.into(),
            Def {
                params: None,
                body: tokenize(after),
            },
        ))
    }
}

impl Macros {
    /// `#define`s in `text`. `overwrite`: later definitions replace earlier ones (for the
    /// keymap itself); otherwise the first definition wins (header defaults).
    pub fn add_source(&mut self, text: &str, overwrite: bool) {
        for line in logical_lines(text) {
            let t = line.trim_start();
            let Some(rest) = t.strip_prefix('#') else { continue };
            let rest = rest.trim_start();
            if let Some(d) = rest.strip_prefix("define") {
                if let Some((name, def)) = parse_define(d) {
                    if overwrite || !self.defs.contains_key(&name) {
                        self.defs.insert(name, def);
                    }
                }
            } else if let Some(u) = rest.strip_prefix("undef") {
                if overwrite {
                    self.defs.remove(u.trim());
                }
            }
        }
    }

    pub fn define(&mut self, name: &str, body: &str) {
        self.defs.insert(
            name.into(),
            Def {
                params: None,
                body: tokenize(body),
            },
        );
    }

    pub fn is_defined(&self, name: &str) -> bool {
        self.defs.contains_key(name)
    }

    /// Load the headers `text` includes (recursively, `.h` only) from `search`, then
    /// `text`'s own defines (which win).
    pub fn load_with_includes(&mut self, text: &str, search: &[PathBuf]) {
        let mut seen = HashSet::new();
        self.load_includes(text, search, &mut seen, 0);
        self.add_source(text, true);
    }

    fn load_includes(&mut self, text: &str, search: &[PathBuf], seen: &mut HashSet<PathBuf>, depth: usize) {
        if depth > 16 {
            return;
        }
        for line in logical_lines(text) {
            let t = line.trim_start();
            let Some(rest) = t
                .strip_prefix('#')
                .map(str::trim_start)
                .and_then(|r| r.strip_prefix("include"))
            else {
                continue;
            };
            let rest = rest.trim();
            let name = rest.trim_matches(|c| c == '<' || c == '>' || c == '"');
            if !name.ends_with(".h") {
                continue;
            }
            if let Some(p) = search.iter().map(|d| d.join(name)).find(|p| p.is_file()) {
                let canon = p.canonicalize().unwrap_or(p.clone());
                if seen.insert(canon) {
                    if let Ok(inc) = std::fs::read_to_string(&p) {
                        // Includes inside a header resolve from its own folder first.
                        let mut s = vec![p.parent().map(Path::to_path_buf).unwrap_or_default()];
                        s.extend(search.iter().cloned());
                        self.load_includes(&inc, &s, seen, depth + 1);
                        self.add_source(&inc, false);
                    }
                }
            }
        }
    }

    /// Fully macro-expand `toks`.
    pub fn expand(&self, toks: &[Tok]) -> Vec<Tok> {
        self.expand_with(toks, &mut Vec::new(), 0)
    }

    fn expand_with(&self, toks: &[Tok], active: &mut Vec<String>, depth: usize) -> Vec<Tok> {
        if depth > 64 {
            return toks.to_vec();
        }
        let mut out = Vec::new();
        let mut i = 0;
        while i < toks.len() {
            let t = &toks[i];
            let Tok::Ident(name) = t else {
                out.push(t.clone());
                i += 1;
                continue;
            };
            let Some(def) = self.defs.get(name).filter(|_| !active.contains(name)) else {
                out.push(t.clone());
                i += 1;
                continue;
            };
            match &def.params {
                None => {
                    active.push(name.clone());
                    out.extend(self.expand_with(&paste(def.body.clone()), active, depth + 1));
                    active.pop();
                    i += 1;
                }
                Some(params) => {
                    if toks.get(i + 1) != Some(&Tok::Punct("(".into())) {
                        out.push(t.clone());
                        i += 1;
                        continue;
                    }
                    // Collect arguments.
                    let mut args: Vec<Vec<Tok>> = vec![vec![]];
                    let mut depth_p = 0;
                    let mut j = i + 2;
                    while j < toks.len() {
                        match toks[j].text() {
                            "(" => {
                                depth_p += 1;
                                args.last_mut().unwrap().push(toks[j].clone());
                            }
                            ")" if depth_p == 0 => break,
                            ")" => {
                                depth_p -= 1;
                                args.last_mut().unwrap().push(toks[j].clone());
                            }
                            "," if depth_p == 0 => args.push(vec![]),
                            _ => args.last_mut().unwrap().push(toks[j].clone()),
                        }
                        j += 1;
                    }
                    if params.is_empty() && args.len() == 1 && args[0].is_empty() {
                        args.clear();
                    }
                    // Substitute (arguments pre-expanded, except next to ##).
                    let mut body = Vec::new();
                    for (k, bt) in def.body.iter().enumerate() {
                        let near_paste = def.body.get(k + 1).is_some_and(|n| n.text() == "##")
                            || (k > 0 && def.body[k - 1].text() == "##");
                        match bt {
                            Tok::Ident(p) if params.contains(p) => {
                                let idx = params.iter().position(|x| x == p).unwrap();
                                let arg = args.get(idx).cloned().unwrap_or_default();
                                if near_paste {
                                    body.extend(arg);
                                } else {
                                    body.extend(self.expand_with(&arg, active, depth + 1));
                                }
                            }
                            _ => body.push(bt.clone()),
                        }
                    }
                    active.push(name.clone());
                    out.extend(self.expand_with(&paste(body), active, depth + 1));
                    active.pop();
                    i = j + 1;
                }
            }
        }
        out
    }

    /// Cells of a binding's parameter text (`LS(A)`, `BT_SEL 1`, `NAV`), as numbers.
    pub fn cells(&self, text: &str) -> Result<Vec<u32>, CppError> {
        let toks = self.expand(&tokenize(text));
        let mut out = Vec::new();
        let mut i = 0;
        while i < toks.len() {
            match &toks[i] {
                Tok::Num(_) => {
                    out.push(eval(&toks[i..i + 1])? as u32);
                    i += 1;
                }
                Tok::Punct(p) if p == "(" => {
                    let mut d = 0;
                    let mut j = i;
                    loop {
                        match toks.get(j).map(Tok::text) {
                            Some("(") => d += 1,
                            Some(")") => {
                                d -= 1;
                                if d == 0 {
                                    break;
                                }
                            }
                            None => return Err(CppError::Syntax(text.into())),
                            _ => {}
                        }
                        j += 1;
                    }
                    out.push(eval(&toks[i..=j])? as u32);
                    i = j + 1;
                }
                Tok::Ident(n) => return Err(CppError::Unknown(n.clone())),
                Tok::Punct(_) => return Err(CppError::Syntax(text.into())),
            }
        }
        Ok(out)
    }
}

/// Apply `##` (token pasting) in a substituted body.
fn paste(toks: Vec<Tok>) -> Vec<Tok> {
    if !toks.iter().any(|t| t.text() == "##") {
        return toks;
    }
    let mut out: Vec<Tok> = Vec::new();
    let mut i = 0;
    while i < toks.len() {
        if toks[i].text() == "##" {
            if let (Some(prev), Some(next)) = (out.pop(), toks.get(i + 1)) {
                let joined = format!("{}{}", prev.text(), next.text());
                out.extend(tokenize(&joined));
                i += 2;
                continue;
            }
            i += 1;
            continue;
        }
        out.push(toks[i].clone());
        i += 1;
    }
    out
}

fn parse_num(s: &str) -> Result<i64, CppError> {
    let s = s.trim_end_matches(['u', 'U', 'l', 'L']);
    let r = if let Some(h) = s.strip_prefix("0x").or_else(|| s.strip_prefix("0X")) {
        i64::from_str_radix(h, 16)
    } else if let Some(b) = s.strip_prefix("0b").or_else(|| s.strip_prefix("0B")) {
        i64::from_str_radix(b, 2)
    } else if s.len() > 1 && s.starts_with('0') {
        i64::from_str_radix(&s[1..], 8)
    } else {
        s.parse()
    };
    r.map_err(|_| CppError::Syntax(s.into()))
}

/// Evaluate an integer expression (C precedence; i64 arithmetic).
pub fn eval(toks: &[Tok]) -> Result<i64, CppError> {
    let mut p = Parser { toks, i: 0 };
    let v = p.expr(0)?;
    if p.i != toks.len() {
        return Err(CppError::Syntax(
            toks.iter().map(Tok::text).collect::<Vec<_>>().join(" "),
        ));
    }
    Ok(v)
}

struct Parser<'a> {
    toks: &'a [Tok],
    i: usize,
}

fn binop(op: &str) -> Option<u8> {
    Some(match op {
        "*" | "/" | "%" => 10,
        "+" | "-" => 9,
        "<<" | ">>" => 8,
        "<" | "<=" | ">" | ">=" => 7,
        "==" | "!=" => 6,
        "&" => 5,
        "^" => 4,
        "|" => 3,
        "&&" => 2,
        "||" => 1,
        _ => return None,
    })
}

impl Parser<'_> {
    fn peek(&self) -> Option<&str> {
        self.toks.get(self.i).map(Tok::text)
    }

    fn expr(&mut self, min: u8) -> Result<i64, CppError> {
        let mut lhs = self.unary()?;
        loop {
            let Some(op) = self.peek().map(str::to_string) else {
                break;
            };
            if op == "?" && min == 0 {
                self.i += 1;
                let a = self.expr(0)?;
                if self.peek() != Some(":") {
                    return Err(CppError::Syntax("?:".into()));
                }
                self.i += 1;
                let b = self.expr(0)?;
                lhs = if lhs != 0 { a } else { b };
                continue;
            }
            let Some(prec) = binop(&op) else { break };
            if prec < min.max(1) {
                break;
            }
            self.i += 1;
            let rhs = self.expr(prec + 1)?;
            lhs = match op.as_str() {
                "*" => lhs.wrapping_mul(rhs),
                "/" => lhs.checked_div(rhs).ok_or(CppError::DivZero)?,
                "%" => lhs.checked_rem(rhs).ok_or(CppError::DivZero)?,
                "+" => lhs.wrapping_add(rhs),
                "-" => lhs.wrapping_sub(rhs),
                "<<" => lhs.wrapping_shl(rhs as u32),
                ">>" => lhs.wrapping_shr(rhs as u32),
                "<" => (lhs < rhs) as i64,
                "<=" => (lhs <= rhs) as i64,
                ">" => (lhs > rhs) as i64,
                ">=" => (lhs >= rhs) as i64,
                "==" => (lhs == rhs) as i64,
                "!=" => (lhs != rhs) as i64,
                "&" => lhs & rhs,
                "^" => lhs ^ rhs,
                "|" => lhs | rhs,
                "&&" => (lhs != 0 && rhs != 0) as i64,
                "||" => (lhs != 0 || rhs != 0) as i64,
                _ => unreachable!(),
            };
        }
        Ok(lhs)
    }

    fn unary(&mut self) -> Result<i64, CppError> {
        let t = self
            .toks
            .get(self.i)
            .cloned()
            .ok_or_else(|| CppError::Syntax("missing value".into()))?;
        self.i += 1;
        match t {
            Tok::Num(n) => parse_num(&n),
            Tok::Ident(n) => Err(CppError::Unknown(n)),
            Tok::Punct(p) => match p.as_str() {
                "-" => Ok(self.unary()?.wrapping_neg()),
                "+" => self.unary(),
                "~" => Ok(!self.unary()?),
                "!" => Ok((self.unary()? == 0) as i64),
                "(" => {
                    let v = self.expr(0)?;
                    if self.peek() != Some(")") {
                        return Err(CppError::Syntax("missing )".into()));
                    }
                    self.i += 1;
                    Ok(v)
                }
                other => Err(CppError::Syntax(other.into())),
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const HEADERS: &str = r#"
#define ZMK_HID_USAGE(page, id) ((page << 16) | id)
#define HID_USAGE_KEY (0x07)
#define HID_USAGE_KEY_KEYBOARD_A 0x04
#define A (ZMK_HID_USAGE(HID_USAGE_KEY, HID_USAGE_KEY_KEYBOARD_A))
#define MOD_LSFT 0x02
#define APPLY_MODS(mods, keycode) (mods << 24 | keycode)
#define LS(keycode) APPLY_MODS(MOD_LSFT, keycode)
#define BT_CLR_CMD 0
#define BT_SEL_CMD 3
#define BT_CLR BT_CLR_CMD 0
#define BT_SEL BT_SEL_CMD
#ifndef ZMK_POINTING_DEFAULT_MOVE_VAL
#define ZMK_POINTING_DEFAULT_MOVE_VAL 600
#endif
#define MOVE_Y(vert) ((vert) & 0xFFFF)
#define MOVE_UP MOVE_Y(-ZMK_POINTING_DEFAULT_MOVE_VAL)
#define PASTE(a, b) a##b
#define LONG_ONE \
    (1 + \
     2)
"#;

    fn m() -> Macros {
        let mut m = Macros::default();
        m.add_source(HEADERS, false);
        m
    }

    #[test]
    fn evaluates_like_the_compiler() {
        let m = m();
        assert_eq!(m.cells("A").unwrap(), [0x070004]);
        assert_eq!(m.cells("LS(A)").unwrap(), [0x0207_0004]);
        assert_eq!(m.cells("BT_CLR").unwrap(), [0, 0]);
        assert_eq!(m.cells("BT_SEL 1").unwrap(), [3, 1]);
        assert_eq!(m.cells("MOVE_UP").unwrap(), [(-600i64 & 0xFFFF) as u32]);
        assert_eq!(m.cells("LONG_ONE").unwrap(), [3]);
        assert_eq!(m.cells("PASTE(BT_, SEL_CMD)").unwrap(), [3]);
        assert_eq!(m.cells("(1 ? 2 : 3) 0x10").unwrap(), [2, 16]);
        assert_eq!(m.cells("NOPE"), Err(CppError::Unknown("NOPE".into())));
        assert!(m.cells("(1 / 0)").is_err());
        assert_eq!(eval(&tokenize("2 + 3 * 4 << 1 | 1")).unwrap(), 29);
        assert_eq!(eval(&tokenize("-(1) & 0xF")).unwrap(), 15);
    }

    #[test]
    fn keymap_defines_win_over_header_defaults() {
        let mut m = m();
        m.add_source(
            "#define ZMK_POINTING_DEFAULT_MOVE_VAL 1000\n#define NAV 2\n#define HRM(k, mod) mod k",
            true,
        );
        assert_eq!(m.cells("MOVE_UP").unwrap(), [(-1000i64 & 0xFFFF) as u32]);
        assert_eq!(m.cells("NAV").unwrap(), [2]);
        // A header loaded later doesn't override (first definition wins for headers).
        m.add_source("#define NAV 5", false);
        assert_eq!(m.cells("NAV").unwrap(), [2]);
    }

    #[test]
    fn follows_includes() {
        let dir = std::env::temp_dir().join(format!("mxkeys-cpp-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("dt-bindings/zmk")).unwrap();
        std::fs::write(
            dir.join("dt-bindings/zmk/keys.h"),
            "#include \"usage.h\"\n#define B (0x07 << 16 | KB_B)\n",
        )
        .unwrap();
        std::fs::write(dir.join("dt-bindings/zmk/usage.h"), "#define KB_B 0x05\n").unwrap();
        let mut m = Macros::default();
        m.load_with_includes(
            "#include <dt-bindings/zmk/keys.h>\n#include <behaviors.dtsi>\n#define X B\n",
            &[dir.clone()],
        );
        assert_eq!(m.cells("X").unwrap(), [0x070005]);
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
