//! Physical key layouts from ZMK's own definitions.
//!
//! Every ZMK keyboard describes where its keys are in devicetree
//! (`compatible = "zmk,physical-layout"`, `keys = <&key_physical_attrs w h x y r rx ry>, …`).
//! A build's resolved `zephyr.dts` has them with every macro expanded and the active one
//! named in `chosen { zmk,physical-layout = &label; }`, so reading it gives the exact
//! layout for any keyboard — no table of keyboards needed.

use serde::Serialize;

use crate::catalog::KeyGeom;

/// One `zmk,physical-layout` node.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PhysicalLayout {
    /// Node label (`foostan_corne_6col_layout`).
    pub label: String,
    /// `display-name`, e.g. "6 Column".
    pub name: Option<String>,
    pub keys: Vec<KeyGeom>,
}

/// All physical layouts in a resolved `zephyr.dts`, the chosen one first.
pub fn layouts_from_dts(dts: &str) -> Vec<PhysicalLayout> {
    let chosen = dts.lines().find_map(|l| {
        let l = l.trim();
        let rest = l
            .strip_prefix("zmk,physical-layout")?
            .trim_start()
            .strip_prefix('=')?;
        Some(
            rest.trim()
                .trim_start_matches('&')
                .trim_end_matches(';')
                .trim()
                .to_string(),
        )
    });
    let mut out = Vec::new();
    let mut rest = dts;
    while let Some(i) = rest.find("\"zmk,physical-layout\"") {
        // The node header is the last `{` line before the compatible.
        let before = &rest[..i];
        let header_start = before
            .rfind('{')
            .and_then(|b| before[..b].rfind('\n').map(|n| (n + 1, b)));
        let body_end = rest[i..].find("};").map_or(rest.len(), |e| i + e);
        let body = &rest[i..body_end];
        if let Some((hs, he)) = header_start {
            let header = rest[hs..he].trim();
            let label = header.split(':').next().unwrap_or(header).trim().to_string();
            if let Some(keys) = prop(body, "keys").and_then(parse_keys) {
                let name = prop(body, "display-name").map(|v| v.trim().trim_matches('"').to_string());
                out.push(PhysicalLayout { label, name, keys });
            }
        }
        rest = &rest[body_end..];
    }
    if let Some(c) = chosen {
        if let Some(pos) = out.iter().position(|l| l.label == c) {
            let l = out.remove(pos);
            out.insert(0, l);
        }
    }
    out
}

/// The value of `name = …;` inside a node body.
fn prop<'a>(body: &'a str, name: &str) -> Option<&'a str> {
    for line in body.lines() {
        let t = line.trim();
        if let Some(r) = t.strip_prefix(name) {
            if let Some(v) = r.trim_start().strip_prefix('=') {
                return Some(v.trim().trim_end_matches(';'));
            }
        }
    }
    None
}

/// `< &key_physical_attrs 0x64 0x64 0x0 0x25 0x0 0x0 0x0 >, …` → keys.
fn parse_keys(v: &str) -> Option<Vec<KeyGeom>> {
    let mut keys = Vec::new();
    for group in v.split('<').skip(1) {
        let inner = group.split('>').next()?;
        let mut cells = inner.split_whitespace().filter(|t| !t.starts_with('&'));
        let mut n = || -> Option<i32> {
            let t = cells.next()?;
            let u = if let Some(h) = t.strip_prefix("0x") {
                u32::from_str_radix(h, 16).ok()?
            } else {
                t.parse::<i64>().ok()? as u32
            };
            Some(u as i32) // two's complement: 0xfffff6a0 = -2400
        };
        let (w, h, x, y, r, rx, ry) = (n()?, n()?, n()?, n()?, n()?, n()?, n()?);
        keys.push(KeyGeom {
            w,
            h,
            x,
            y,
            r,
            rx,
            ry,
        });
    }
    (!keys.is_empty()).then_some(keys)
}

#[cfg(test)]
mod tests {
    use super::*;

    const DTS: &str = "/ {\n\tchosen {\n\t\tzmk,kscan = &kscan0;\n\t\tzmk,physical-layout = &six;\n\t};\n\tkey_physical_attrs: key_physical_attrs {\n\t\tcompatible = \"zmk,key-physical-attrs\";\n\t};\n\tfive: five {\n\t\tcompatible = \"zmk,physical-layout\";\n\t\tdisplay-name = \"5 Column\";\n\t\tkeys = < &key_physical_attrs 0x64 0x64 0x0 0x25 0x0 0x0 0x0 >;\n\t};\n\tsix: six {\n\t\tcompatible = \"zmk,physical-layout\";\n\t\tdisplay-name = \"6 Column\";\n\t\tkeys = < &key_physical_attrs 0x64 0x64 0x0 0x25 0x0 0x0 0x0 >, < &key_physical_attrs 0x64 0x96 0x28c 0x11b 0xfffff6a0 0x2f0 0x1b1 >;\n\t\ttransform = < &default_transform >;\n\t};\n};\n";

    #[test]
    fn reads_layouts_chosen_first() {
        let l = layouts_from_dts(DTS);
        assert_eq!(l.len(), 2);
        assert_eq!(l[0].label, "six");
        assert_eq!(l[0].name.as_deref(), Some("6 Column"));
        assert_eq!(
            l[0].keys[1],
            KeyGeom {
                w: 100,
                h: 150,
                x: 652,
                y: 283,
                r: -2400,
                rx: 752,
                ry: 433
            }
        );
        assert_eq!(l[1].label, "five");
        assert!(layouts_from_dts("/ { };").is_empty());
    }

    #[test]
    fn real_corne_build_matches_the_builtin_layout() {
        // Only when a real build output is available (MXKEYS_TEST_DTS=path/to/zephyr.dts).
        let Ok(p) = std::env::var("MXKEYS_TEST_DTS") else {
            return;
        };
        let l = layouts_from_dts(&std::fs::read_to_string(p).unwrap());
        assert_eq!(l[0].keys, crate::catalog::corne_layout());
    }
}
