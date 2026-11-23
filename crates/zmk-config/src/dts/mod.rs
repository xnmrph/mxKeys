//! Lossless devicetree source model.
//!
//! [`Document::parse`] never fails: it always yields a tree of [`Item`]s whose spans point
//! into the original text, plus diagnostics for regions it could not understand (those
//! become [`Item::Unknown`] and are preserved verbatim). Serialising a document
//! ([`Document::to_source`]) reproduces the input byte-for-byte; edits are expressed as
//! [`Patch`]es (byte-range replacements) followed by a re-parse, so untouched bytes never
//! change.

pub mod lexer;
mod parser;

use std::ops::Range;

pub use lexer::{Token, TokenKind};

/// Half-open byte range `start..end` into the document source.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Default, serde::Serialize)]
pub struct Span {
    pub start: usize,
    pub end: usize,
}

impl Span {
    pub const fn new(start: usize, end: usize) -> Self {
        Span { start, end }
    }
    pub fn range(self) -> Range<usize> {
        self.start..self.end
    }
    pub fn len(self) -> usize {
        self.end - self.start
    }
    pub fn is_empty(self) -> bool {
        self.start == self.end
    }
    pub fn to(self, other: Span) -> Span {
        Span::new(self.start, other.end)
    }
    pub fn contains(self, other: Span) -> bool {
        self.start <= other.start && other.end <= self.end
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Label {
    pub name: String,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Item {
    Node(Node),
    /// `&label { … };` — amends a node defined elsewhere (e.g. `&mt { tapping-term-ms = <200>; };`).
    RefNode(RefNode),
    Property(Property),
    Directive(Directive),
    /// Opaque macro invocation at item position, e.g. `ZMK_COMBO(esc, &kp ESC, 0 1)`.
    MacroCall(MacroCall),
    /// `/delete-node/ name;`, `/delete-property/ name;`, `/dts-v1/;`, `/omit-if-no-ref/` …
    Keyword(KeywordItem),
    /// A region the parser could not understand, kept verbatim.
    Unknown(Span),
}

impl Item {
    pub fn span(&self) -> Span {
        match self {
            Item::Node(n) => n.span,
            Item::RefNode(n) => n.span,
            Item::Property(p) => p.span,
            Item::Directive(d) => d.span,
            Item::MacroCall(m) => m.span,
            Item::Keyword(k) => k.span,
            Item::Unknown(s) => *s,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Node {
    /// From the first label (or name) to the closing `;` (or `}` if `;` is missing).
    pub span: Span,
    pub labels: Vec<Label>,
    /// Node name without unit address (`/` for the root node).
    pub name: String,
    pub unit_address: Option<String>,
    pub name_span: Span,
    /// Span of the `{` token.
    pub open_brace: Span,
    /// Span of the `}` token (empty span at EOF if missing).
    pub close_brace: Span,
    pub children: Vec<Item>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RefNode {
    pub span: Span,
    pub labels: Vec<Label>,
    /// Referenced label without `&`, or the path for `&{/path}`.
    pub target: String,
    pub target_span: Span,
    pub open_brace: Span,
    pub close_brace: Span,
    pub children: Vec<Item>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Property {
    /// From the property name (or label) to the `;`.
    pub span: Span,
    pub labels: Vec<Label>,
    pub name: String,
    pub name_span: Span,
    /// Empty for boolean properties (`foo;`).
    pub values: Vec<Value>,
    /// Span covering all values (between `=` and `;`), if any.
    pub values_span: Option<Span>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Value {
    /// `< … >` cell array; `inner` excludes the angle brackets.
    Cells {
        span: Span,
        inner: Span,
        cells: Vec<Cell>,
    },
    /// `"…"`; `value` is the unescaped content.
    Str { span: Span, value: String },
    /// `[ … ]` byte string.
    Bytes { span: Span },
    /// `&label` outside cells.
    Ref { span: Span, label: String },
    /// Anything else (e.g. a macro name expanding to a value list), kept raw.
    Raw { span: Span },
}

impl Value {
    pub fn span(&self) -> Span {
        match self {
            Value::Cells { span, .. }
            | Value::Str { span, .. }
            | Value::Bytes { span }
            | Value::Ref { span, .. }
            | Value::Raw { span } => *span,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Cell {
    pub span: Span,
    pub kind: CellKind,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CellKind {
    /// `&label` (phandle reference) — label without `&`.
    Ref(String),
    Number(String),
    /// Bare identifier: keycode/macro (`LCTRL`, `N1`, `XXX`).
    Ident(String),
    /// Function-like macro use: `LS(A)`, `RC(0,1)`. `args` is the raw text between parens.
    Call {
        name: String,
        args: String,
    },
    /// Parenthesised expression `( … )`.
    Paren,
    /// Any other token (operators etc.).
    Other,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Directive {
    pub span: Span,
    pub kind: DirectiveKind,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DirectiveKind {
    Include {
        path: String,
    },
    Define {
        name: String,
        params: Option<Vec<String>>,
        body: String,
    },
    Undef {
        name: String,
    },
    /// `#if`, `#ifdef`, `#ifndef`, `#elif`, `#else`, `#endif`, … — `name` is the word after `#`.
    Conditional {
        name: String,
        condition: String,
    },
    Other {
        name: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MacroCall {
    pub span: Span,
    pub name: String,
    /// Raw argument texts, split on top-level commas and trimmed.
    pub args: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeywordItem {
    pub span: Span,
    /// e.g. `/delete-node/`.
    pub keyword: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Diagnostic {
    pub span: Span,
    pub message: String,
}

/// A byte-range replacement.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Patch {
    pub span: Span,
    pub replacement: String,
}

impl Patch {
    pub fn replace(span: Span, replacement: impl Into<String>) -> Self {
        Patch {
            span,
            replacement: replacement.into(),
        }
    }
    pub fn insert(at: usize, text: impl Into<String>) -> Self {
        Patch {
            span: Span::new(at, at),
            replacement: text.into(),
        }
    }
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum PatchError {
    #[error("patch {0:?} is out of bounds or not on a character boundary")]
    OutOfBounds(Span),
    #[error("patches {0:?} and {1:?} overlap")]
    Overlap(Span, Span),
}

#[derive(Debug, Clone)]
pub struct Document {
    source: String,
    tokens: Vec<Token>,
    pub items: Vec<Item>,
    pub diagnostics: Vec<Diagnostic>,
}

impl Document {
    pub fn parse(source: impl Into<String>) -> Document {
        let source = source.into();
        let tokens = lexer::lex(&source);
        let (items, diagnostics) = parser::parse(&source, &tokens);
        Document {
            source,
            tokens,
            items,
            diagnostics,
        }
    }

    pub fn source(&self) -> &str {
        &self.source
    }

    pub fn tokens(&self) -> &[Token] {
        &self.tokens
    }

    pub fn text(&self, span: Span) -> &str {
        &self.source[span.range()]
    }

    /// Rebuild the source from the token stream. Always equal to the parsed input; this
    /// is the lossless guarantee exercised by the round-trip tests.
    pub fn to_source(&self) -> String {
        let mut out = String::with_capacity(self.source.len());
        for t in &self.tokens {
            out.push_str(&self.source[t.span.range()]);
        }
        out
    }

    /// Apply non-overlapping patches and re-parse.
    pub fn apply(&self, patches: &[Patch]) -> Result<Document, PatchError> {
        Ok(Document::parse(apply_patches(&self.source, patches)?))
    }

    /// Depth-first iteration over all items (including nested children).
    pub fn walk(&self) -> impl Iterator<Item = &Item> {
        let mut stack: Vec<&Item> = self.items.iter().rev().collect();
        std::iter::from_fn(move || {
            let item = stack.pop()?;
            let children = match item {
                Item::Node(n) => Some(&n.children),
                Item::RefNode(n) => Some(&n.children),
                _ => None,
            };
            if let Some(children) = children {
                stack.extend(children.iter().rev());
            }
            Some(item)
        })
    }

    /// All `#define`s in the document, in order.
    pub fn defines(&self) -> impl Iterator<Item = (&str, Option<&[String]>, &str)> {
        self.walk().filter_map(|i| match i {
            Item::Directive(Directive {
                kind: DirectiveKind::Define { name, params, body },
                ..
            }) => Some((name.as_str(), params.as_deref(), body.as_str())),
            _ => None,
        })
    }

    /// Byte offset of the start of the line containing `pos`.
    pub fn line_start(&self, pos: usize) -> usize {
        self.source[..pos].rfind(['\n', '\r']).map_or(0, |i| i + 1)
    }

    /// The whitespace indentation of the line containing `pos`.
    pub fn indent_at(&self, pos: usize) -> &str {
        let start = self.line_start(pos);
        let line = &self.source[start..];
        let n = line.bytes().take_while(|b| *b == b' ' || *b == b'\t').count();
        &line[..n]
    }

    /// The newline style used by the document (`\r\n` if the first newline is CRLF).
    pub fn newline(&self) -> &'static str {
        match self.source.find('\n') {
            Some(i) if i > 0 && self.source.as_bytes()[i - 1] == b'\r' => "\r\n",
            _ => "\n",
        }
    }
}

impl Node {
    pub fn property(&self, name: &str) -> Option<&Property> {
        self.children.iter().find_map(|c| match c {
            Item::Property(p) if p.name == name => Some(p),
            _ => None,
        })
    }
    pub fn child_nodes(&self) -> impl Iterator<Item = &Node> {
        self.children.iter().filter_map(|c| match c {
            Item::Node(n) => Some(n),
            _ => None,
        })
    }
    pub fn compatible(&self) -> Option<&str> {
        self.property("compatible").and_then(Property::first_string)
    }
}

impl RefNode {
    pub fn property(&self, name: &str) -> Option<&Property> {
        self.children.iter().find_map(|c| match c {
            Item::Property(p) if p.name == name => Some(p),
            _ => None,
        })
    }
}

impl Property {
    pub fn first_string(&self) -> Option<&str> {
        self.values.iter().find_map(|v| match v {
            Value::Str { value, .. } => Some(value.as_str()),
            _ => None,
        })
    }
    /// All cells of all `< … >` values, in order.
    pub fn cells(&self) -> impl Iterator<Item = &Cell> {
        self.values.iter().flat_map(|v| match v {
            Value::Cells { cells, .. } => cells.as_slice(),
            _ => &[],
        })
    }
}

pub fn apply_patches(source: &str, patches: &[Patch]) -> Result<String, PatchError> {
    let mut sorted: Vec<&Patch> = patches.iter().collect();
    sorted.sort_by_key(|p| (p.span.start, p.span.end));
    for p in &sorted {
        if p.span.end > source.len()
            || p.span.start > p.span.end
            || !source.is_char_boundary(p.span.start)
            || !source.is_char_boundary(p.span.end)
        {
            return Err(PatchError::OutOfBounds(p.span));
        }
    }
    for w in sorted.windows(2) {
        if w[0].span.end > w[1].span.start {
            return Err(PatchError::Overlap(w[0].span, w[1].span));
        }
    }
    let mut out = String::with_capacity(source.len());
    let mut pos = 0;
    for p in sorted {
        out.push_str(&source[pos..p.span.start]);
        out.push_str(&p.replacement);
        pos = p.span.end;
    }
    out.push_str(&source[pos..]);
    Ok(out)
}

/// Escape a string for use inside a devicetree `"…"` literal.
pub fn escape_string(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\t' => out.push_str("\\t"),
            c => out.push(c),
        }
    }
    out
}

pub(crate) fn unescape_string(raw: &str) -> String {
    let inner = raw.strip_prefix('"').unwrap_or(raw);
    let inner = inner.strip_suffix('"').unwrap_or(inner);
    let mut out = String::with_capacity(inner.len());
    let mut chars = inner.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('n') => out.push('\n'),
            Some('t') => out.push('\t'),
            Some('r') => out.push('\r'),
            Some('0') => out.push('\0'),
            Some(other) => out.push(other),
            None => out.push('\\'),
        }
    }
    out
}

#[cfg(test)]
mod numeric_node_tests {
    use super::*;

    #[test]
    fn node_names_may_start_with_a_digit() {
        let d = Document::parse("/ { keymap { 0 { bindings = <&kp 0x4>; }; NOR { }; }; };");
        assert!(d.diagnostics.is_empty(), "{:?}", d.diagnostics);
        let names: Vec<_> = d
            .walk()
            .filter_map(|i| match i {
                Item::Node(n) => Some(n.name.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(names, ["/", "keymap", "0", "NOR"]);
        assert_eq!(
            d.to_source(),
            "/ { keymap { 0 { bindings = <&kp 0x4>; }; NOR { }; }; };"
        );
    }
}
