//! Lossless lexer for devicetree source with C-preprocessor content (ZMK `.keymap`,
//! `.dtsi`, `.overlay`).
//!
//! The defining property: the produced tokens **tile** the input exactly — every byte
//! belongs to exactly one token, in order. Concatenating all token texts reproduces the
//! input byte-for-byte. Whitespace, newlines and comments are tokens ("trivia") too.

use super::Span;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TokenKind {
    /// Spaces and tabs (and other non-newline whitespace).
    Whitespace,
    /// `\n` or `\r\n` (or a lone `\r`).
    Newline,
    /// `// …` up to (not including) the newline.
    LineComment,
    /// `/* … */` (may span lines). Unterminated comments run to end of input.
    BlockComment,
    /// A whole preprocessor logical line (`#include …`, `#define …` incl. `\` continuations),
    /// excluding the final newline.
    Directive,
    /// Identifier-like word: node/property names, labels, macro names, keycodes.
    /// Starts with `[A-Za-z_#]` and continues with `[A-Za-z0-9_,.+\-?#@]`.
    Ident,
    /// Number literal: starts with a digit, continues with `[A-Za-z0-9_.]`.
    Number,
    /// `"…"` string literal with backslash escapes.
    String,
    /// `'x'` character literal.
    CharLit,
    /// DTS keyword such as `/delete-node/`, `/dts-v1/`, `/include/`, `/bits/`.
    Keyword,
    /// Any single punctuation character (`{ } < > ( ) [ ] ; = , & : / - …`).
    Punct,
}

impl TokenKind {
    pub fn is_trivia(self) -> bool {
        matches!(
            self,
            TokenKind::Whitespace | TokenKind::Newline | TokenKind::LineComment | TokenKind::BlockComment
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Token {
    pub kind: TokenKind,
    pub span: Span,
}

const DIRECTIVES: &[&str] = &[
    "include",
    "define",
    "undef",
    "if",
    "ifdef",
    "ifndef",
    "elif",
    "elifdef",
    "elifndef",
    "else",
    "endif",
    "error",
    "warning",
    "pragma",
    "line",
    "import",
    "include_next",
];

fn is_ident_start(b: u8) -> bool {
    b.is_ascii_alphabetic() || b == b'_' || b == b'#'
}

fn is_ident_continue(b: u8) -> bool {
    b.is_ascii_alphanumeric() || matches!(b, b'_' | b',' | b'.' | b'+' | b'-' | b'?' | b'#' | b'@')
}

/// Tokenise `src`. Never fails: unexpected bytes become one-character `Punct` tokens.
pub fn lex(src: &str) -> Vec<Token> {
    let bytes = src.as_bytes();
    let mut tokens = Vec::new();
    let mut i = 0;
    // True when only whitespace has been seen since the last newline (or start of input).
    let mut at_line_start = true;

    while i < bytes.len() {
        let start = i;
        let b = bytes[i];
        let kind = match b {
            b'\n' => {
                i += 1;
                TokenKind::Newline
            }
            b'\r' => {
                i += 1;
                if bytes.get(i) == Some(&b'\n') {
                    i += 1;
                }
                TokenKind::Newline
            }
            b' ' | b'\t' | 0x0b | 0x0c => {
                while i < bytes.len() && matches!(bytes[i], b' ' | b'\t' | 0x0b | 0x0c) {
                    i += 1;
                }
                TokenKind::Whitespace
            }
            b'/' if bytes.get(i + 1) == Some(&b'/') => {
                i = line_end(bytes, i);
                TokenKind::LineComment
            }
            b'/' if bytes.get(i + 1) == Some(&b'*') => {
                i = block_comment_end(bytes, i);
                TokenKind::BlockComment
            }
            b'#' if at_line_start && is_directive(bytes, i) => {
                i = directive_end(bytes, i);
                TokenKind::Directive
            }
            b'"' => {
                i = quoted_end(bytes, i, b'"');
                TokenKind::String
            }
            b'\'' => {
                i = quoted_end(bytes, i, b'\'');
                TokenKind::CharLit
            }
            b'/' => {
                if let Some(end) = keyword_end(bytes, i) {
                    i = end;
                    TokenKind::Keyword
                } else {
                    i += 1;
                    TokenKind::Punct
                }
            }
            b if b.is_ascii_digit() => {
                while i < bytes.len() && (bytes[i].is_ascii_alphanumeric() || matches!(bytes[i], b'_' | b'.'))
                {
                    i += 1;
                }
                TokenKind::Number
            }
            b if is_ident_start(b) => {
                i += 1;
                while i < bytes.len() && is_ident_continue(bytes[i]) {
                    i += 1;
                }
                TokenKind::Ident
            }
            _ => {
                // One punctuation char; for non-ASCII, consume the whole UTF-8 sequence so
                // spans always sit on char boundaries.
                i += utf8_len(b);
                TokenKind::Punct
            }
        };
        let i_clamped = i.min(bytes.len());
        i = i_clamped;
        tokens.push(Token {
            kind,
            span: Span::new(start, i),
        });
        at_line_start = match kind {
            TokenKind::Newline => true,
            TokenKind::Whitespace => at_line_start,
            // A block comment at line start (`/* x */ #define`) is unusual; be conservative.
            _ => false,
        };
    }
    tokens
}

fn utf8_len(first: u8) -> usize {
    match first {
        0x00..=0x7f => 1,
        0xc0..=0xdf => 2,
        0xe0..=0xef => 3,
        0xf0..=0xf7 => 4,
        _ => 1,
    }
}

fn line_end(bytes: &[u8], mut i: usize) -> usize {
    while i < bytes.len() && bytes[i] != b'\n' && bytes[i] != b'\r' {
        i += 1;
    }
    i
}

fn block_comment_end(bytes: &[u8], start: usize) -> usize {
    let mut i = start + 2;
    while i + 1 < bytes.len() {
        if bytes[i] == b'*' && bytes[i + 1] == b'/' {
            return i + 2;
        }
        i += 1;
    }
    bytes.len()
}

fn quoted_end(bytes: &[u8], start: usize, quote: u8) -> usize {
    let mut i = start + 1;
    while i < bytes.len() {
        match bytes[i] {
            b'\\' => i += 2,
            b'\n' | b'\r' => return i, // unterminated: stop at end of line
            c if c == quote => return i + 1,
            _ => i += 1,
        }
    }
    bytes.len()
}

fn is_directive(bytes: &[u8], i: usize) -> bool {
    let mut j = i + 1;
    while j < bytes.len() && matches!(bytes[j], b' ' | b'\t') {
        j += 1;
    }
    // A lone `#` (null directive) or `# 12 "file"` line markers.
    if j >= bytes.len() || matches!(bytes[j], b'\n' | b'\r') || bytes[j].is_ascii_digit() {
        return true;
    }
    let word_start = j;
    while j < bytes.len() && (bytes[j].is_ascii_alphanumeric() || bytes[j] == b'_') {
        j += 1;
    }
    let word = &bytes[word_start..j];
    // `#binding-cells` / `#address-cells` are property names, not directives.
    if bytes.get(j) == Some(&b'-') {
        return false;
    }
    DIRECTIVES.iter().any(|d| d.as_bytes() == word)
}

/// End of a preprocessor logical line: handles `\`-newline continuations and block
/// comments that span lines inside the directive.
fn directive_end(bytes: &[u8], start: usize) -> usize {
    let mut i = start;
    while i < bytes.len() {
        match bytes[i] {
            b'\\' if matches!(bytes.get(i + 1), Some(b'\n')) => i += 2,
            b'\\' if matches!(bytes.get(i + 1), Some(b'\r')) => {
                i += 2;
                if bytes.get(i) == Some(&b'\n') {
                    i += 1;
                }
            }
            b'/' if bytes.get(i + 1) == Some(&b'*') => i = block_comment_end(bytes, i),
            b'/' if bytes.get(i + 1) == Some(&b'/') => return line_end(bytes, i),
            b'"' => i = quoted_end(bytes, i, b'"'),
            b'\n' | b'\r' => return i,
            _ => i += 1,
        }
    }
    bytes.len()
}

/// `/word-with-dashes/` keyword.
fn keyword_end(bytes: &[u8], start: usize) -> Option<usize> {
    let mut j = start + 1;
    while j < bytes.len() && (bytes[j].is_ascii_lowercase() || bytes[j] == b'-' || bytes[j].is_ascii_digit())
    {
        j += 1;
    }
    if j > start + 1 && bytes.get(j) == Some(&b'/') {
        Some(j + 1)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kinds(src: &str) -> Vec<(TokenKind, &str)> {
        lex(src)
            .into_iter()
            .map(|t| (t.kind, &src[t.span.range()]))
            .collect()
    }

    fn assert_tiles(src: &str) {
        let toks = lex(src);
        let mut pos = 0;
        for t in &toks {
            assert_eq!(t.span.start, pos, "gap/overlap at {pos}");
            assert!(t.span.end > t.span.start, "empty token at {pos}");
            pos = t.span.end;
        }
        assert_eq!(pos, src.len());
    }

    #[test]
    fn directive_vs_hash_property() {
        let src = "#include <x.h>\n  #binding-cells = <0>;\n#define A \\\n  1\n";
        assert_tiles(src);
        let k = kinds(src);
        assert_eq!(k[0], (TokenKind::Directive, "#include <x.h>"));
        assert!(k.contains(&(TokenKind::Ident, "#binding-cells")));
        assert!(k.contains(&(TokenKind::Directive, "#define A \\\n  1")));
    }

    #[test]
    fn comments_strings_keywords() {
        let src = "/delete-node/ &x; // hi\r\n/* a\n b */ s = \"q\\\"x\"; c = 'a';";
        assert_tiles(src);
        let k = kinds(src);
        assert_eq!(k[0], (TokenKind::Keyword, "/delete-node/"));
        assert!(k.contains(&(TokenKind::LineComment, "// hi")));
        assert!(k.contains(&(TokenKind::Newline, "\r\n")));
        assert!(k.contains(&(TokenKind::BlockComment, "/* a\n b */")));
        assert!(k.contains(&(TokenKind::String, "\"q\\\"x\"")));
        assert!(k.contains(&(TokenKind::CharLit, "'a'")));
    }

    #[test]
    fn idents_numbers_punct() {
        let src = "&kp LS(N1) <(-2400)> 0x1F node@0 hold-trigger-key-positions ü";
        assert_tiles(src);
        let k = kinds(src);
        assert!(k.contains(&(TokenKind::Ident, "kp")));
        assert!(k.contains(&(TokenKind::Ident, "LS")));
        assert!(k.contains(&(TokenKind::Ident, "N1")));
        assert!(k.contains(&(TokenKind::Number, "2400")));
        assert!(k.contains(&(TokenKind::Number, "0x1F")));
        assert!(k.contains(&(TokenKind::Ident, "node@0")));
        assert!(k.contains(&(TokenKind::Ident, "hold-trigger-key-positions")));
        assert!(k.contains(&(TokenKind::Punct, "ü")));
    }

    #[test]
    fn unterminated_things_still_tile() {
        for src in ["/* never ends", "\"open string\nnext", "#define X \\", "'", "/"] {
            assert_tiles(src);
        }
    }
}
