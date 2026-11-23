//! Structural keymap edits as minimal patches: aligned binding changes and layer
//! add/remove/move with layer-reference renumbering.
//!
//! Layers are referenced by *index* (`&mo 1`, `&lt 2 TAB`, combo `layers = <0 1>`,
//! conditional layers) — directly or via a `#define NAV 1`. Moving or removing a layer
//! renumbers those references: numbers are patched in place, `#define`s once.

use std::collections::{BTreeMap, HashMap};

use crate::catalog::{builtin_behaviors, custom_behaviors, ParamKind};
use crate::dts::{CellKind, Directive, DirectiveKind, Document, Item, Node, Patch, Span, Value};

use super::{Keymap, KeymapError};

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum LayerNameError {
    #[error("Please give the layer a name.")]
    Empty,
    #[error("That name is too long (keep it under 32 characters).")]
    TooLong,
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum EditError {
    #[error(transparent)]
    Keymap(#[from] KeymapError),
    #[error(transparent)]
    Name(#[from] LayerNameError),
    #[error("The layer “{layer}” is still used by {by}. Change that first, then remove the layer.")]
    LayerInUse { layer: String, by: String },
    #[error("A keymap needs at least one layer.")]
    LastLayer,
    #[error("This layer's keys can't be copied automatically: {0}")]
    Unsupported(String),
    #[error(transparent)]
    Patch(#[from] crate::dts::PatchError),
}

/// How a layer is referenced at one place.
#[derive(Debug, Clone, PartialEq, Eq)]
enum LayerValue {
    Number(usize),
    Define(String),
}

#[derive(Debug, Clone)]
struct LayerUse {
    span: Span,
    value: LayerValue,
    /// Plain description for messages, e.g. "layer “Base”, key 38".
    place: String,
}

fn line_start(src: &str, pos: usize) -> usize {
    src[..pos].rfind('\n').map_or(0, |i| i + 1)
}

fn line_end_incl(src: &str, pos: usize) -> usize {
    src[pos..].find('\n').map_or(src.len(), |i| pos + i + 1)
}

fn indent_of(src: &str, pos: usize) -> String {
    let ls = line_start(src, pos);
    src[ls..]
        .chars()
        .take_while(|c| *c == ' ' || *c == '\t')
        .collect()
}

/// Behavior reference → indexes of its layer parameters.
fn layer_param_table(doc: &Document, km: &Keymap) -> HashMap<String, Vec<usize>> {
    builtin_behaviors()
        .into_iter()
        .chain(custom_behaviors(doc, km))
        .filter_map(|b| {
            let idx: Vec<usize> = b
                .params
                .iter()
                .enumerate()
                .filter(|(_, p)| matches!(p.kind, ParamKind::Layer))
                .map(|(i, _)| i)
                .collect();
            (!idx.is_empty()).then_some((b.reference, idx))
        })
        .collect()
}

/// `#define NAME <number>`: name → (number, span of the number).
fn numeric_defines(doc: &Document) -> HashMap<String, (usize, Span)> {
    let src = doc.source();
    let mut out = HashMap::new();
    for item in doc.walk() {
        if let Item::Directive(Directive {
            span,
            kind:
                DirectiveKind::Define {
                    name,
                    params: None,
                    body,
                },
        }) = item
        {
            if let Ok(n) = body.trim().parse::<usize>() {
                // Locate the number after the name inside the directive text.
                let text = &src[span.range()];
                if let Some(name_at) = text.find(name.as_str()) {
                    let after = name_at + name.len();
                    let rest = &text[after..];
                    let lead = rest.len() - rest.trim_start().len();
                    let digits = rest.trim_start().bytes().take_while(u8::is_ascii_digit).count();
                    if digits > 0 {
                        let start = span.start + after + lead;
                        out.insert(name.clone(), (n, Span::new(start, start + digits)));
                    }
                }
            }
        }
    }
    out
}

fn value_of(kind: &CellKind, text: &str) -> Option<LayerValue> {
    match kind {
        CellKind::Number(n) => n.parse().ok().map(LayerValue::Number),
        CellKind::Ident(name) => Some(LayerValue::Define(name.clone())),
        _ => text.parse().ok().map(LayerValue::Number),
    }
}

/// Every place a layer is referenced.
fn layer_uses(doc: &Document, km: &Keymap) -> Vec<LayerUse> {
    let table = layer_param_table(doc, km);
    let mut uses = Vec::new();
    // Key bindings on layers.
    for l in &km.layers {
        for list in [&l.bindings, &l.sensor_bindings].into_iter().flatten() {
            for (i, b) in list.bindings.iter().enumerate() {
                if let Some(idx) = table.get(&b.behavior) {
                    for &pi in idx {
                        if let (Some(text), Some(span)) = (b.params.get(pi), b.param_spans.get(pi)) {
                            let kind = if text.chars().all(|c| c.is_ascii_digit()) {
                                CellKind::Number(text.clone())
                            } else {
                                CellKind::Ident(text.clone())
                            };
                            if let Some(v) = value_of(&kind, text) {
                                uses.push(LayerUse {
                                    span: *span,
                                    value: v,
                                    place: format!("layer “{}”, key {}", l.title(), i + 1),
                                });
                            }
                        }
                    }
                }
            }
        }
    }
    // Bindings elsewhere (combos, macros, behaviors) and combo/conditional layer lists.
    let keymap_span = km.keymap_node_span;
    for item in doc.walk() {
        let Item::Node(n) = item else { continue };
        if keymap_span.contains(n.span) {
            continue;
        }
        for c in &n.children {
            let Item::Property(p) = c else { continue };
            let place = format!("“{}”", n.name);
            match p.name.as_str() {
                "bindings" => {
                    let cells: Vec<_> = p.cells().collect();
                    let mut i = 0;
                    while i < cells.len() {
                        if let CellKind::Ref(label) = &cells[i].kind {
                            if let Some(idx) = table.get(&format!("&{label}")) {
                                for &pi in idx {
                                    if let Some(cell) = cells.get(i + 1 + pi) {
                                        if let Some(v) = value_of(&cell.kind, doc.text(cell.span)) {
                                            uses.push(LayerUse {
                                                span: cell.span,
                                                value: v,
                                                place: place.clone(),
                                            });
                                        }
                                    }
                                }
                            }
                        }
                        i += 1;
                    }
                }
                "layers" | "if-layers" | "then-layer" => {
                    for cell in p.cells() {
                        if let Some(v) = value_of(&cell.kind, doc.text(cell.span)) {
                            uses.push(LayerUse {
                                span: cell.span,
                                value: v,
                                place: place.clone(),
                            });
                        }
                    }
                }
                _ => {}
            }
        }
    }
    uses
}

/// Patches that renumber every layer reference by `map` (old index → new index, or
/// `None` for a removed layer). References inside `skip` (the removed layer) are ignored.
fn renumber(
    doc: &Document,
    km: &Keymap,
    map: &dyn Fn(usize) -> Option<usize>,
    skip: Option<Span>,
) -> Result<Vec<Patch>, EditError> {
    let defines = numeric_defines(doc);
    let mut patches = Vec::new();
    let mut define_targets: BTreeMap<String, usize> = BTreeMap::new();
    for u in layer_uses(doc, km) {
        if skip.is_some_and(|s| s.contains(u.span)) {
            continue;
        }
        let old = match &u.value {
            LayerValue::Number(n) => *n,
            LayerValue::Define(name) => match defines.get(name) {
                Some((n, _)) => *n,
                None => continue, // not a numeric define (e.g. a keycode-like macro)
            },
        };
        let Some(new) = map(old) else {
            let layer = km
                .layers
                .get(old)
                .map(|l| l.title())
                .unwrap_or_else(|| old.to_string());
            return Err(EditError::LayerInUse { layer, by: u.place });
        };
        match &u.value {
            LayerValue::Number(_) if new != old => patches.push(Patch::replace(u.span, new.to_string())),
            LayerValue::Define(name) => {
                define_targets.insert(name.clone(), new);
            }
            _ => {}
        }
    }
    // Defines named after layer nodes (e.g. `#define NAV 1` + `NAV { … }`) move with them
    // even if nothing references them yet.
    for l in &km.layers {
        if let Some((n, _)) = defines.get(&l.node_name) {
            if *n == l.index {
                if let Some(new) = map(*n) {
                    define_targets.entry(l.node_name.clone()).or_insert(new);
                }
            }
        }
    }
    for (name, new) in define_targets {
        let (old, span) = defines[&name];
        if old != new {
            patches.push(Patch::replace(span, new.to_string()));
        }
    }
    Ok(patches)
}

/// Line-based extent of layer `i`'s node: (start of its first line, end incl. newline).
fn block(doc: &Document, km: &Keymap, i: usize) -> (usize, usize) {
    let src = doc.source();
    let n = &km.layers[i].node_span;
    (line_start(src, n.start), line_end_incl(src, n.end))
}

fn validate_name(name: &str) -> Result<&str, LayerNameError> {
    let n = name.trim();
    if n.is_empty() {
        return Err(LayerNameError::Empty);
    }
    if n.chars().count() > 32 {
        return Err(LayerNameError::TooLong);
    }
    Ok(n)
}

fn node_name_for(display: &str, km: &Keymap, doc: &Document) -> String {
    let mut base: String = display
        .to_ascii_lowercase()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
        .collect::<String>()
        .split('_')
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join("_");
    if base.is_empty() || base.starts_with(|c: char| c.is_ascii_digit()) {
        base = format!("layer_{base}").trim_end_matches('_').to_string();
    }
    if !base.ends_with("_layer") && !base.starts_with("layer") {
        base.push_str("_layer");
    }
    let taken = |n: &str| {
        km.layers.iter().any(|l| l.node_name == n)
            || doc
                .walk()
                .any(|i| matches!(i, Item::Node(Node { labels, .. }) if labels.iter().any(|l| l.name == n)))
    };
    let mut name = base.clone();
    let mut k = 2;
    while taken(&name) {
        name = format!("{base}_{k}");
        k += 1;
    }
    name
}

impl Keymap {
    /// Like [`Keymap::set_binding`], but keeps the following columns where they were by
    /// adjusting the spaces after the key (only the edited line changes).
    pub fn set_binding_aligned(
        &self,
        doc: &Document,
        layer: usize,
        pos: usize,
        text: &str,
    ) -> Result<Patch, KeymapError> {
        let plain = self.set_binding(layer, pos, text)?;
        let src = doc.source();
        let span = plain.span;
        let after = &src[span.end..];
        let gap = after.bytes().take_while(|b| *b == b' ').count();
        let next = after.as_bytes().get(gap).copied();
        // At the end of a line (or before `>`), there's nothing to keep aligned.
        if gap == 0 || matches!(next, None | Some(b'\n' | b'\r' | b'\t' | b'>')) {
            return Ok(plain);
        }
        let old_len = span.len() as isize;
        let new_len = plain.replacement.chars().count() as isize;
        let new_gap = (gap as isize - (new_len - old_len)).max(1) as usize;
        Ok(Patch::replace(
            Span::new(span.start, span.end + gap),
            format!("{}{}", plain.replacement, " ".repeat(new_gap)),
        ))
    }

    /// Patch adding a layer named `name` after the last one, with every key transparent,
    /// laid out like the first layer.
    pub fn add_layer(&self, doc: &Document, name: &str) -> Result<Patch, EditError> {
        let name = validate_name(name)?;
        let src = doc.source();
        let nl = doc.newline();
        let first = self.layers.first().ok_or(EditError::LastLayer)?;
        let last = self.layers.last().ok_or(EditError::LastLayer)?;
        let indent = indent_of(src, first.node_span.start);
        let unit = if indent.contains('\t') {
            "\t".to_string()
        } else {
            "    ".to_string()
        };
        let child = first
            .bindings
            .as_ref()
            .map(|b| indent_of(src, b.property_span.start))
            .unwrap_or_else(|| format!("{indent}{unit}"));

        // Copy the first layer's grid, every key replaced by `&trans` (padded to keep columns).
        let grid = match &first.bindings {
            Some(list) if list.opaque_reason.is_none() && !list.bindings.is_empty() => {
                let inner = doc
                    .walk()
                    .find_map(|i| match i {
                        Item::Property(p) if p.span == list.property_span => {
                            p.values.iter().find_map(|v| match v {
                                Value::Cells { inner, .. } => Some(*inner),
                                _ => None,
                            })
                        }
                        _ => None,
                    })
                    .ok_or_else(|| EditError::Unsupported("bindings aren't a single < … > list".into()))?;
                let patches: Vec<Patch> = list
                    .bindings
                    .iter()
                    .filter(|b| inner.contains(b.span))
                    .map(|b| {
                        let w = b.span.len().max(6);
                        Patch::replace(
                            Span::new(b.span.start - inner.start, b.span.end - inner.start),
                            format!("{:<w$}", "&trans"),
                        )
                    })
                    .collect();
                let text = crate::dts::apply_patches(&src[inner.range()], &patches)?;
                // Trailing spaces from padding at line ends are noise.
                text.split('\n')
                    .map(str::trim_end)
                    .collect::<Vec<_>>()
                    .join(nl)
                    .replace("\r\r", "\r")
            }
            _ => {
                return Err(EditError::Unsupported(
                    "the first layer is written with macros".into(),
                ))
            }
        };
        let node = node_name_for(name, self, doc);
        let quoted = crate::dts::escape_string(name);
        let new_block = format!(
            "{nl}{indent}{node} {{{nl}{child}display-name = \"{quoted}\";{nl}{child}bindings = <{grid}>;{nl}{indent}}};"
        );
        let insert_at = last.node_span.end;
        let sep = if self.layers.len() >= 2 {
            let (_, e0) = block(doc, self, 0);
            let (s1, _) = block(doc, self, 1);
            if src[e0..s1].trim().is_empty() && src[e0..s1].contains('\n') {
                nl
            } else {
                ""
            }
        } else {
            nl
        };
        Ok(Patch::insert(insert_at, format!("{sep}{new_block}")))
    }

    /// Patches removing layer `index` and renumbering references to later layers.
    pub fn remove_layer(&self, doc: &Document, index: usize) -> Result<Vec<Patch>, EditError> {
        let l = self.layer(index)?;
        if self.layers.len() == 1 {
            return Err(EditError::LastLayer);
        }
        let map = |i: usize| match i.cmp(&index) {
            std::cmp::Ordering::Less => Some(i),
            std::cmp::Ordering::Equal => None,
            std::cmp::Ordering::Greater => Some(i - 1),
        };
        let mut patches = renumber(doc, self, &map, Some(l.node_span))?;
        // Remove the block plus one neighbouring separator.
        let (s, e) = block(doc, self, index);
        let del = if index + 1 < self.layers.len() {
            Span::new(s, block(doc, self, index + 1).0)
        } else {
            Span::new(block(doc, self, index - 1).1, e)
        };
        patches.push(Patch::replace(del, ""));
        Ok(patches)
    }

    /// Patches moving layer `from` to position `to`, renumbering all layer references.
    pub fn move_layer(&self, doc: &Document, from: usize, to: usize) -> Result<Vec<Patch>, EditError> {
        self.layer(from)?;
        self.layer(to)?;
        if from == to {
            return Ok(Vec::new());
        }
        let n = self.layers.len();
        let mut order: Vec<usize> = (0..n).collect();
        let moved = order.remove(from);
        order.insert(to, moved);
        let new_index: Vec<usize> = (0..n)
            .map(|old| order.iter().position(|&o| o == old).unwrap())
            .collect();
        let mut patches = renumber(doc, self, &|i| new_index.get(i).copied(), None)?;

        let src = doc.source();
        let (s, e) = block(doc, self, from);
        // Renumbered references inside the moved layer are applied to its text, which is
        // cut and re-inserted; the rest stay as patches on the document.
        let (inside, outside): (Vec<Patch>, Vec<Patch>) = patches
            .into_iter()
            .partition(|p| p.span.start >= s && p.span.end <= e);
        patches = outside;
        let relative: Vec<Patch> = inside
            .into_iter()
            .map(|p| Patch::replace(Span::new(p.span.start - s, p.span.end - s), p.replacement))
            .collect();
        let core_text = crate::dts::apply_patches(&src[s..e], &relative)?;
        let core = core_text.as_str();
        let sep = {
            let (_, e0) = block(doc, self, 0);
            let (s1, _) = block(doc, self, 1);
            let gap = &src[e0..s1];
            if gap.trim().is_empty() {
                gap.to_string()
            } else {
                String::new()
            }
        };
        if to < from {
            let at = block(doc, self, to).0;
            patches.push(Patch::insert(at, format!("{core}{sep}")));
            patches.push(Patch::replace(Span::new(block(doc, self, from - 1).1, e), ""));
        } else {
            patches.push(Patch::replace(Span::new(s, block(doc, self, from + 1).0), ""));
            let at = block(doc, self, to).1;
            patches.push(Patch::insert(at, format!("{sep}{core}")));
        }
        Ok(patches)
    }
}
