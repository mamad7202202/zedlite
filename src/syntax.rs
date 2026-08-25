use std::collections::HashSet;
use std::sync::OnceLock;

use gpui::rgb;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TokenKind {
    Plain,
    Comment,
    Str,
    Number,
    Keyword,
    Type,
    Punct,
}

impl TokenKind {
    pub fn color(self) -> u32 {
        match self {
            TokenKind::Plain => 0xd7dae0,
            TokenKind::Comment => 0x5c6370,
            TokenKind::Str => 0x98c379,
            TokenKind::Number => 0xd19a66,
            TokenKind::Keyword => 0xc678dd,
            TokenKind::Type => 0xe5c07b,
            TokenKind::Punct => 0x828997,
        }
    }

    pub fn rgb(self) -> gpui::Rgb {
        rgb(self.color())
    }
}

fn keywords() -> &'static HashSet<&'static str> {
    static KEYWORDS: OnceLock<HashSet<&'static str>> = OnceLock::new();
    KEYWORDS.get_or_init(|| {
        [
            "as", "async", "await", "break", "case", "catch", "class", "const", "continue",
            "crate", "def", "default", "do", "dyn", "elif", "else", "enum", "except", "export",
            "extends", "extern", "false", "finally", "fn", "for", "from", "func", "function",
            "go", "if", "impl", "import", "in", "interface", "is", "lambda", "let", "loop",
            "match", "mod", "move", "mut", "new", "nil", "None", "not", "null", "or", "package",
            "pass", "print", "pub", "raise", "ref", "return", "self", "Self", "static", "struct",
            "super", "switch", "this", "throw", "trait", "true", "try", "type", "typeof", "union",
            "unsafe", "use", "var", "void", "where", "while", "with", "yield", "and", "del",
            "global", "nonlocal", "assert", "yield", "final", "synchronized", "volatile",
            "transient", "instanceof", "defer", "chan", "map", "range", "select", "fallthrough",
        ]
        .into_iter()
        .collect()
    })
}

pub fn is_code_file(path: &std::path::Path) -> bool {
    matches!(
        path.extension()
            .and_then(|ext| ext.to_str())
            .unwrap_or_default(),
        "rs" | "js" | "jsx" | "ts" | "tsx" | "py" | "rb" | "go" | "java" | "kt" | "swift"
            | "c" | "h" | "cpp" | "cc" | "hpp" | "cs" | "php" | "dart" | "scala" | "zig"
            | "json" | "toml" | "yaml" | "yml" | "sh" | "bash" | "zsh"
    )
}

#[derive(Debug)]
pub struct Token {
    pub start: usize,
    pub end: usize,
    pub kind: TokenKind,
}

/// A tiny single-pass tokenizer good enough for colorful code display.
/// It is not a real parser: block comments and nested strings are out of scope.
pub fn tokenize(line: &str) -> Vec<Token> {
    let chars: Vec<char> = line.chars().collect();
    let mut tokens = Vec::new();
    let byte_index_of = |char_ix: usize| -> usize { chars[..char_ix].iter().map(char::len_utf8).sum() };

    let mut i = 0usize;
    while i < chars.len() {
        let c = chars[i];

        if c.is_whitespace() {
            i += 1;
            continue;
        }

        if c == '/' && chars.get(i + 1) == Some(&'/') {
            tokens.push(Token {
                start: byte_index_of(i),
                end: byte_index_of(chars.len()),
                kind: TokenKind::Comment,
            });
            break;
        }

        if c == '#' && looks_like_hash_comment(&chars, i) {
            tokens.push(Token {
                start: byte_index_of(i),
                end: byte_index_of(chars.len()),
                kind: TokenKind::Comment,
            });
            break;
        }

        if c == '"' || c == '\'' || c == '`' {
            let quote = c;
            let start_char = i;
            let mut j = i + 1;
            while j < chars.len() && chars[j] != quote {
                if chars[j] == '\\' {
                    j += 1;
                }
                j += 1;
            }
            let end_char = (j + 1).min(chars.len());
            tokens.push(Token {
                start: byte_index_of(start_char),
                end: byte_index_of(end_char),
                kind: TokenKind::Str,
            });
            i = end_char;
            continue;
        }

        if c.is_ascii_digit() {
            let start_char = i;
            while i < chars.len()
                && (chars[i].is_alphanumeric() || chars[i] == '_' || chars[i] == '.')
            {
                i += 1;
            }
            tokens.push(Token {
                start: byte_index_of(start_char),
                end: byte_index_of(i),
                kind: TokenKind::Number,
            });
            continue;
        }

        if c.is_alphabetic() || c == '_' {
            let start_char = i;
            while i < chars.len() && (chars[i].is_alphanumeric() || chars[i] == '_') {
                i += 1;
            }
            let word: String = chars[start_char..i].iter().collect();
            let kind = if keywords().contains(word.as_str()) {
                TokenKind::Keyword
            } else if word.chars().next().is_some_and(|ch| ch.is_uppercase()) {
                TokenKind::Type
            } else {
                TokenKind::Plain
            };
            tokens.push(Token {
                start: byte_index_of(start_char),
                end: byte_index_of(i),
                kind,
            });
            continue;
        }

        // punctuation run
        let start_char = i;
        while i < chars.len()
            && !chars[i].is_alphanumeric()
            && !chars[i].is_whitespace()
            && !"\"'#".contains(chars[i])
        {
            i += 1;
        }
        if i == start_char {
            i += 1;
        }
        tokens.push(Token {
            start: byte_index_of(start_char),
            end: byte_index_of(i),
            kind: TokenKind::Punct,
        });
    }

    tokens
}

fn looks_like_hash_comment(chars: &[char], i: usize) -> bool {
    // `#` starts a comment in Python/shell-ish files; avoid eating Rust attributes.
    chars.get(i + 1).is_none_or(|next| *next != '[')
}
