//! Recursive-descent parser from the lossless token stream to the CST in [`super`].
//!
//! It works on the *significant* (non-trivia) tokens only; spans always refer back to the
//! original source, so trivia is preserved implicitly. Error recovery: when an item cannot
//! be parsed, tokens are skipped up to the next `;` (or a balanced `}`) at the current
//! nesting level and recorded as [`Item::Unknown`] with a diagnostic.

use super::*;

pub(super) fn parse(src: &str, tokens: &[Token]) -> (Vec<Item>, Vec<Diagnostic>) {
    let sig: Vec<Token> = tokens.iter().copied().filter(|t| !t.kind.is_trivia()).collect();
    let mut p = Parser {
        src,
        sig,
        pos: 0,
        diagnostics: Vec::new(),
    };
    let items = p.items(false);
    (items, p.diagnostics)
}

struct Parser<'a> {
    src: &'a str,
    sig: Vec<Token>,
    pos: usize,
    diagnostics: Vec<Diagnostic>,
}

impl<'a> Parser<'a> {
    // ----- token helpers -------------------------------------------------------------

    fn peek(&self) -> Option<Token> {
        self.sig.get(self.pos).copied()
    }
    fn peek_at(&self, n: usize) -> Option<Token> {
        self.sig.get(self.pos + n).copied()
    }
    fn text(&self, t: Token) -> &'a str {
        &self.src[t.span.range()]
    }
    fn is_punct(&self, t: Option<Token>, c: &str) -> bool {
        matches!(t, Some(t) if t.kind == TokenKind::Punct && self.text(t) == c)
    }
    fn at_punct(&self, c: &str) -> bool {
        self.is_punct(self.peek(), c)
    }
    fn bump(&mut self) -> Token {
        let t = self.sig[self.pos];
        self.pos += 1;
        t
    }
    fn eat_punct(&mut self, c: &str) -> Option<Token> {
        if self.at_punct(c) {
            Some(self.bump())
        } else {
            None
        }
    }
    fn prev_end(&self) -> usize {
        if self.pos == 0 {
            0
        } else {
            self.sig[self.pos - 1].span.end
        }
    }
    fn eof_span(&self) -> Span {
        Span::new(self.src.len(), self.src.len())
    }

    // ----- items ---------------------------------------------------------------------

    /// Parse items until EOF or (if `in_body`) a closing `}`.
    fn items(&mut self, in_body: bool) -> Vec<Item> {
        let mut items = Vec::new();
        while let Some(t) = self.peek() {
            if in_body && self.is_punct(Some(t), "}") {
                break;
            }
            // Stray `;` (e.g. after a macro call) — attach to nothing, just skip it.
            if self.is_punct(Some(t), ";") {
                self.bump();
                continue;
            }
            let start = self.pos;
            match self.item() {
                Some(item) => items.push(item),
                None => {
                    self.pos = start;
                    let span = self.recover();
                    self.diagnostics.push(Diagnostic {
                        span,
                        message: format!("could not parse `{}`", truncate(&self.src[span.range()])),
                    });
                    items.push(Item::Unknown(span));
                }
            }
        }
        items
    }

    /// Skip to the end of the current statement: the next `;` at depth 0, or a balanced
    /// `{ … }` block (plus optional `;`). Stops before a `}` that closes the parent.
    fn recover(&mut self) -> Span {
        let start = self.peek().map_or(self.src.len(), |t| t.span.start);
        let mut depth = 0i32;
        while let Some(t) = self.peek() {
            let s = if t.kind == TokenKind::Punct {
                self.text(t)
            } else {
                ""
            };
            match s {
                "{" | "(" | "[" => depth += 1,
                "}" | ")" | "]" => {
                    if depth == 0 {
                        break;
                    }
                    depth -= 1;
                    if depth == 0 && s == "}" {
                        self.bump();
                        self.eat_punct(";");
                        break;
                    }
                }
                ";" if depth == 0 => {
                    self.bump();
                    break;
                }
                _ => {}
            }
            self.bump();
        }
        if self.pos < self.sig.len() || start < self.src.len() {
            // Ensure progress even if we stopped immediately on a stray closer.
            if self.prev_end() <= start {
                if let Some(_t) = self.peek() {
                    self.bump();
                }
            }
        }
        Span::new(start, self.prev_end().max(start))
    }

    fn item(&mut self) -> Option<Item> {
        let t = self.peek()?;
        match t.kind {
            TokenKind::Directive => {
                self.bump();
                Some(Item::Directive(directive(self.src, t.span)))
            }
            TokenKind::Keyword => self.keyword_item(),
            TokenKind::Punct if self.text(t) == "/" => {
                // Root node `/ { … };`
                let name = self.bump();
                let open = self.eat_punct("{")?;
                Some(Item::Node(self.node_rest(
                    t.span.start,
                    Vec::new(),
                    "/".into(),
                    None,
                    name.span,
                    open,
                )))
            }
            TokenKind::Punct if self.text(t) == "&" => self.ref_node(Vec::new()),
            TokenKind::Ident => {
                let labels = self.labels();
                let t = self.peek()?;
                let start = labels.first().map_or(t.span.start, |l| l.span.start);
                if self.is_punct(Some(t), "&") {
                    return self.ref_node(labels);
                }
                if t.kind == TokenKind::Punct && self.text(t) == "/" {
                    // `label: / { … }` is unusual but legal-ish; treat like root.
                    let name = self.bump();
                    let open = self.eat_punct("{")?;
                    return Some(Item::Node(self.node_rest(
                        start,
                        labels,
                        "/".into(),
                        None,
                        name.span,
                        open,
                    )));
                }
                if t.kind != TokenKind::Ident {
                    return None;
                }
                let name_tok = self.bump();
                let full = self.text(name_tok);
                let next = self.peek();
                if self.is_punct(next, "(") && labels.is_empty() {
                    return self.macro_call(name_tok);
                }
                if self.is_punct(next, "{") {
                    let open = self.bump();
                    let (name, unit) = match full.split_once('@') {
                        Some((n, u)) => (n.to_string(), Some(u.to_string())),
                        None => (full.to_string(), None),
                    };
                    return Some(Item::Node(self.node_rest(
                        start,
                        labels,
                        name,
                        unit,
                        name_tok.span,
                        open,
                    )));
                }
                if self.is_punct(next, "=") {
                    self.bump();
                    let (values, values_span) = self.values()?;
                    let semi = self.eat_punct(";")?;
                    return Some(Item::Property(Property {
                        span: Span::new(start, semi.span.end),
                        labels,
                        name: full.to_string(),
                        name_span: name_tok.span,
                        values,
                        values_span,
                    }));
                }
                if self.is_punct(next, ";") {
                    let semi = self.bump();
                    return Some(Item::Property(Property {
                        span: Span::new(start, semi.span.end),
                        labels,
                        name: full.to_string(),
                        name_span: name_tok.span,
                        values: Vec::new(),
                        values_span: None,
                    }));
                }
                None
            }
            _ => None,
        }
    }

    fn labels(&mut self) -> Vec<Label> {
        let mut labels = Vec::new();
        while let (Some(t), next) = (self.peek(), self.peek_at(1)) {
            if t.kind == TokenKind::Ident && self.is_punct(next, ":") {
                self.bump();
                let colon = self.bump();
                labels.push(Label {
                    name: self.text(t).to_string(),
                    span: t.span.to(colon.span),
                });
            } else {
                break;
            }
        }
        labels
    }

    fn node_rest(
        &mut self,
        start: usize,
        labels: Vec<Label>,
        name: String,
        unit_address: Option<String>,
        name_span: Span,
        open_brace: Token,
    ) -> Node {
        let children = self.items(true);
        let close_brace = match self.eat_punct("}") {
            Some(t) => t.span,
            None => {
                self.diagnostics.push(Diagnostic {
                    span: open_brace.span,
                    message: format!("node `{name}` is missing its closing `}}`"),
                });
                self.eof_span()
            }
        };
        let end = self.eat_punct(";").map_or(close_brace.end, |t| t.span.end);
        Node {
            span: Span::new(start, end.max(close_brace.end)),
            labels,
            name,
            unit_address,
            name_span,
            open_brace: open_brace.span,
            close_brace,
            children,
        }
    }

    fn ref_node(&mut self, labels: Vec<Label>) -> Option<Item> {
        let amp = self.eat_punct("&")?;
        let start = labels.first().map_or(amp.span.start, |l| l.span.start);
        let (target, target_span) = if self.at_punct("{") {
            // `&{/path/to/node}`
            let open = self.bump();
            let mut end = open.span.end;
            while let Some(t) = self.peek() {
                self.bump();
                end = t.span.end;
                if self.is_punct(Some(t), "}") {
                    break;
                }
            }
            let path = self.src[open.span.end..end.saturating_sub(1).max(open.span.end)]
                .trim()
                .to_string();
            (path, Span::new(amp.span.start, end))
        } else {
            let t = self.peek()?;
            if t.kind != TokenKind::Ident {
                return None;
            }
            self.bump();
            (self.text(t).to_string(), amp.span.to(t.span))
        };
        let open = self.eat_punct("{")?;
        let children = self.items(true);
        let close_brace = match self.eat_punct("}") {
            Some(t) => t.span,
            None => {
                self.diagnostics.push(Diagnostic {
                    span: open.span,
                    message: format!("`&{target}` is missing its closing `}}`"),
                });
                self.eof_span()
            }
        };
        let end = self.eat_punct(";").map_or(close_brace.end, |t| t.span.end);
        Some(Item::RefNode(RefNode {
            span: Span::new(start, end.max(close_brace.end)),
            labels,
            target,
            target_span,
            open_brace: open.span,
            close_brace,
            children,
        }))
    }

    fn keyword_item(&mut self) -> Option<Item> {
        let kw = self.bump();
        let keyword = self.text(kw).to_string();
        if keyword == "/omit-if-no-ref/" {
            // Prefix for a node: keep it as its own item; the node follows.
            return Some(Item::Keyword(KeywordItem {
                span: kw.span,
                keyword,
            }));
        }
        // Consume up to and including `;`.
        let mut end = kw.span.end;
        while let Some(t) = self.peek() {
            if self.is_punct(Some(t), "}") {
                break;
            }
            self.bump();
            end = t.span.end;
            if self.is_punct(Some(t), ";") {
                break;
            }
        }
        Some(Item::Keyword(KeywordItem {
            span: Span::new(kw.span.start, end),
            keyword,
        }))
    }

    fn macro_call(&mut self, name_tok: Token) -> Option<Item> {
        let open = self.eat_punct("(")?;
        let close = self.balanced_close(open)?;
        let inner = &self.src[open.span.end..close.span.start];
        let args = split_top_level_commas(inner);
        Some(Item::MacroCall(MacroCall {
            span: name_tok.span.to(close.span),
            name: self.text(name_tok).to_string(),
            args,
        }))
    }

    /// After consuming an opening `(`/`[`/`{`, consume through its matching closer.
    fn balanced_close(&mut self, open: Token) -> Option<Token> {
        let mut depth = 1;
        while let Some(t) = self.peek() {
            self.bump();
            if t.kind == TokenKind::Punct {
                match self.text(t) {
                    "(" | "[" | "{" => depth += 1,
                    ")" | "]" | "}" => {
                        depth -= 1;
                        if depth == 0 {
                            return Some(t);
                        }
                    }
                    _ => {}
                }
            }
        }
        self.diagnostics.push(Diagnostic {
            span: open.span,
            message: "unbalanced bracket".into(),
        });
        None
    }

    // ----- property values -----------------------------------------------------------

    fn values(&mut self) -> Option<(Vec<Value>, Option<Span>)> {
        let mut values = Vec::new();
        loop {
            let t = self.peek()?;
            if self.is_punct(Some(t), ";") {
                break;
            }
            let v = match t.kind {
                TokenKind::Punct if self.text(t) == "<" => self.cells()?,
                TokenKind::String => {
                    self.bump();
                    Value::Str {
                        span: t.span,
                        value: unescape_string(self.text(t)),
                    }
                }
                TokenKind::Punct if self.text(t) == "[" => {
                    self.bump();
                    let close = self.balanced_close(t)?;
                    Value::Bytes {
                        span: t.span.to(close.span),
                    }
                }
                TokenKind::Punct if self.text(t) == "&" => {
                    self.bump();
                    let l = self.peek()?;
                    if l.kind != TokenKind::Ident {
                        return None;
                    }
                    self.bump();
                    Value::Ref {
                        span: t.span.to(l.span),
                        label: self.text(l).to_string(),
                    }
                }
                TokenKind::Punct if matches!(self.text(t), "}" | "{" | "=") => return None,
                _ => {
                    // Raw value (macro expanding to values, /bits/ prefix, …) up to `,`/`;`.
                    let start = t.span;
                    let mut end = t.span;
                    while let Some(n) = self.peek() {
                        if self.is_punct(Some(n), ",") || self.is_punct(Some(n), ";") {
                            break;
                        }
                        if n.kind == TokenKind::Punct && matches!(self.text(n), "(" | "<" | "[") {
                            self.bump();
                            let close_char = match self.text(n) {
                                "(" => ")",
                                "<" => ">",
                                _ => "]",
                            };
                            end = self.until_punct(close_char)?;
                            continue;
                        }
                        if n.kind == TokenKind::Punct && matches!(self.text(n), "}" | "{") {
                            return None;
                        }
                        self.bump();
                        end = n.span;
                    }
                    Value::Raw { span: start.to(end) }
                }
            };
            values.push(v);
            if self.eat_punct(",").is_none() {
                break;
            }
        }
        let span = match (values.first(), values.last()) {
            (Some(a), Some(b)) => Some(a.span().to(b.span())),
            _ => None,
        };
        Some((values, span))
    }

    fn until_punct(&mut self, c: &str) -> Option<Span> {
        while let Some(t) = self.peek() {
            self.bump();
            if self.is_punct(Some(t), c) {
                return Some(t.span);
            }
            if self.is_punct(Some(t), ";") {
                return None;
            }
        }
        None
    }

    fn cells(&mut self) -> Option<Value> {
        let open = self.bump(); // `<`
        let mut cells = Vec::new();
        loop {
            let t = self.peek()?;
            if t.kind == TokenKind::Punct {
                match self.text(t) {
                    ">" => break,
                    ";" | "{" | "}" => return None,
                    "&" => {
                        self.bump();
                        let n = self.peek()?;
                        if n.kind == TokenKind::Ident {
                            self.bump();
                            cells.push(Cell {
                                span: t.span.to(n.span),
                                kind: CellKind::Ref(self.text(n).to_string()),
                            });
                        } else if self.is_punct(Some(n), "{") {
                            let o = self.bump();
                            let c = self.balanced_close(o)?;
                            let path = self.src[o.span.end..c.span.start].trim().to_string();
                            cells.push(Cell {
                                span: t.span.to(c.span),
                                kind: CellKind::Ref(path),
                            });
                        } else {
                            cells.push(Cell {
                                span: t.span,
                                kind: CellKind::Other,
                            });
                        }
                        continue;
                    }
                    "(" => {
                        self.bump();
                        let c = self.balanced_close(t)?;
                        cells.push(Cell {
                            span: t.span.to(c.span),
                            kind: CellKind::Paren,
                        });
                        continue;
                    }
                    _ => {
                        self.bump();
                        cells.push(Cell {
                            span: t.span,
                            kind: CellKind::Other,
                        });
                        continue;
                    }
                }
            }
            self.bump();
            let kind = match t.kind {
                TokenKind::Number => CellKind::Number(self.text(t).to_string()),
                TokenKind::Ident if self.at_punct("(") => {
                    let o = self.bump();
                    let c = self.balanced_close(o)?;
                    let args = self.src[o.span.end..c.span.start].to_string();
                    cells.push(Cell {
                        span: t.span.to(c.span),
                        kind: CellKind::Call {
                            name: self.text(t).to_string(),
                            args,
                        },
                    });
                    continue;
                }
                TokenKind::Ident => CellKind::Ident(self.text(t).to_string()),
                _ => CellKind::Other,
            };
            cells.push(Cell { span: t.span, kind });
        }
        let close = self.bump(); // `>`
        Some(Value::Cells {
            span: open.span.to(close.span),
            inner: Span::new(open.span.end, close.span.start),
            cells,
        })
    }
}

fn truncate(s: &str) -> String {
    let one_line: String = s.split_whitespace().collect::<Vec<_>>().join(" ");
    if one_line.chars().count() > 60 {
        format!("{}…", one_line.chars().take(60).collect::<String>())
    } else {
        one_line
    }
}

pub(crate) fn split_top_level_commas(s: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut depth = 0i32;
    let mut cur = String::new();
    let mut in_str = false;
    let mut escape = false;
    for c in s.chars() {
        if in_str {
            cur.push(c);
            if escape {
                escape = false;
            } else if c == '\\' {
                escape = true;
            } else if c == '"' {
                in_str = false;
            }
            continue;
        }
        match c {
            '"' => {
                in_str = true;
                cur.push(c);
            }
            '(' | '[' | '{' | '<' => {
                depth += 1;
                cur.push(c);
            }
            ')' | ']' | '}' | '>' => {
                depth -= 1;
                cur.push(c);
            }
            ',' if depth == 0 => out.push(std::mem::take(&mut cur).trim().to_string()),
            _ => cur.push(c),
        }
    }
    if !cur.trim().is_empty() || !out.is_empty() {
        out.push(cur.trim().to_string());
    }
    out
}

fn directive(src: &str, span: Span) -> Directive {
    let text = &src[span.range()];
    // Join continuation lines for interpretation (the span keeps the original text).
    let logical = text.replace("\\\r\n", " ").replace("\\\n", " ");
    let rest = logical.trim_start().trim_start_matches('#').trim_start();
    let name_len = rest
        .bytes()
        .take_while(|b| b.is_ascii_alphanumeric() || *b == b'_')
        .count();
    let name = &rest[..name_len];
    let after = rest[name_len..].trim();
    let kind = match name {
        "include" | "include_next" | "import" => DirectiveKind::Include {
            path: after
                .trim_matches(|c| c == '<' || c == '>' || c == '"')
                .to_string(),
        },
        "define" => {
            let after_name = rest[name_len..].trim_start();
            let id_len = after_name
                .bytes()
                .take_while(|b| b.is_ascii_alphanumeric() || *b == b'_')
                .count();
            let mname = after_name[..id_len].to_string();
            let tail = &after_name[id_len..];
            // Function-like only if `(` immediately follows the name.
            if let Some(stripped) = tail.strip_prefix('(') {
                let close = stripped.find(')').unwrap_or(stripped.len());
                let params = stripped[..close]
                    .split(',')
                    .map(|p| p.trim().to_string())
                    .filter(|p| !p.is_empty())
                    .collect();
                let body = stripped.get(close + 1..).unwrap_or("").trim().to_string();
                DirectiveKind::Define {
                    name: mname,
                    params: Some(params),
                    body: strip_comments(&body),
                }
            } else {
                DirectiveKind::Define {
                    name: mname,
                    params: None,
                    body: strip_comments(tail.trim()),
                }
            }
        }
        "undef" => DirectiveKind::Undef {
            name: after.to_string(),
        },
        "if" | "ifdef" | "ifndef" | "elif" | "elifdef" | "elifndef" | "else" | "endif" => {
            DirectiveKind::Conditional {
                name: name.to_string(),
                condition: strip_comments(after),
            }
        }
        _ => DirectiveKind::Other {
            name: name.to_string(),
        },
    };
    Directive { span, kind }
}

fn strip_comments(s: &str) -> String {
    let mut out = String::new();
    let mut rest = s;
    loop {
        let line = rest.find("//");
        let block = rest.find("/*");
        match (line, block) {
            (Some(l), b) if b.is_none_or(|b| l < b) => {
                out.push_str(&rest[..l]);
                break;
            }
            (_, Some(b)) => {
                out.push_str(&rest[..b]);
                match rest[b + 2..].find("*/") {
                    Some(e) => {
                        out.push(' ');
                        rest = &rest[b + 2 + e + 2..];
                    }
                    None => break,
                }
            }
            _ => {
                out.push_str(rest);
                break;
            }
        }
    }
    out.trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_commas() {
        assert_eq!(
            split_top_level_commas("a, LS(B, C), \"x,y\", <1 2>"),
            vec!["a", "LS(B, C)", "\"x,y\"", "<1 2>"]
        );
        assert!(split_top_level_commas("").is_empty());
    }

    #[test]
    fn parses_defines() {
        let d = directive("#define XXX &none // nothing", Span::new(0, 28));
        assert_eq!(
            d.kind,
            DirectiveKind::Define {
                name: "XXX".into(),
                params: None,
                body: "&none".into()
            }
        );
        let src = "#define HRM(k, m) &hm m k";
        let d = directive(src, Span::new(0, src.len()));
        assert_eq!(
            d.kind,
            DirectiveKind::Define {
                name: "HRM".into(),
                params: Some(vec!["k".into(), "m".into()]),
                body: "&hm m k".into()
            }
        );
        let src = "#define DEF 0 /* base */";
        let d = directive(src, Span::new(0, src.len()));
        assert_eq!(
            d.kind,
            DirectiveKind::Define {
                name: "DEF".into(),
                params: None,
                body: "0".into()
            }
        );
    }
}
