//! The lexer (language.md §1): `#` line comments, single-line double-quoted
//! strings with `\n` `\"` `\\` escapes, `lower_snake` identifiers, duration
//! literals `<n>m` / `<n>h` / `<n>h<m>m` (minute counts), a tab is a lex
//! error. Output is line-grouped: nesting is decided by the column of each
//! line's first token.

use crate::span::Span;
use std::sync::Arc;
use story_content::{code, Diagnostic};

#[derive(Clone, PartialEq, Eq, Debug)]
pub enum P {
    Colon,
    Comma,
    Dot,
    DotDot,
    LParen,
    RParen,
    LBracket,
    RBracket,
    Plus,
    Minus,
    Star,
    Slash,
    Eq,
    PlusEq,
    MinusEq,
    Ge,
    Le,
    Gt,
    Lt,
    EqEq,
    Ne,
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Tok {
    Ident(String),
    Int(i64),
    /// Duration literal, in minutes.
    Dur(i64),
    Str(String),
    P(P),
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Line {
    pub no: u32,
    /// Column (1-based) of the line's first token.
    pub indent: u32,
    pub toks: Vec<(Span, Tok)>,
}

impl Line {
    pub fn first(&self) -> Option<&(Span, Tok)> {
        self.toks.first()
    }

    pub fn first_ident(&self) -> Option<&str> {
        match self.toks.first() {
            Some((_, Tok::Ident(s))) => Some(s.as_str()),
            _ => None,
        }
    }
}

pub fn lex(file: &Arc<str>, src: &str, diags: &mut Vec<Diagnostic>) -> Vec<Line> {
    let mut lines = Vec::new();
    for (idx, raw) in src.lines().enumerate() {
        let no = idx as u32 + 1;
        if let Some(line) = lex_line(file, no, raw, diags) {
            lines.push(line);
        }
    }
    lines
}

fn lex_line(file: &Arc<str>, no: u32, raw: &str, diags: &mut Vec<Diagnostic>) -> Option<Line> {
    let chars: Vec<(usize, char)> = raw.char_indices().collect();
    let mut i = 0;
    let mut col = 1usize;
    let mut toks: Vec<(Span, Tok)> = Vec::new();

    let sp = |c: usize| Span::new(file.clone(), no, c as u32);
    let err = |c: usize, msg: &str, diags: &mut Vec<Diagnostic>| {
        diags.push(Diagnostic::error(code::E202, file, no, c as u32, msg));
    };

    while i < chars.len() {
        let (_, ch) = chars[i];
        match ch {
            ' ' => {
                i += 1;
                col += 1;
            }
            '\t' => {
                err(col, "a tab is a lex error; indent with spaces", diags);
                i += 1;
                col += 1;
            }
            '#' => break,
            '\r' => {
                i += 1;
            }
            '"' => {
                let start_col = col;
                i += 1;
                col += 1;
                let mut s = String::new();
                let mut closed = false;
                while i < chars.len() {
                    let (_, c) = chars[i];
                    if c == '"' {
                        closed = true;
                        i += 1;
                        col += 1;
                        break;
                    }
                    if c == '\\' {
                        let next = chars.get(i + 1).map(|(_, c)| *c);
                        match next {
                            Some('n') => s.push('\n'),
                            Some('"') => s.push('"'),
                            Some('\\') => s.push('\\'),
                            Some(other) => {
                                err(
                                    col,
                                    &format!("unknown escape `\\{other}` (only \\n, \\\", \\\\)",),
                                    diags,
                                );
                                s.push(other);
                            }
                            None => err(col, "unterminated string", diags),
                        }
                        i += 2;
                        col += 2;
                        continue;
                    }
                    s.push(c);
                    i += 1;
                    col += 1;
                }
                if !closed {
                    err(start_col, "unterminated string", diags);
                }
                toks.push((sp(start_col), Tok::Str(s)));
            }
            '0'..='9' => {
                let start_col = col;
                let mut digits = String::new();
                while i < chars.len() {
                    let (_, c) = chars[i];
                    if c.is_ascii_digit() {
                        digits.push(c);
                        i += 1;
                        col += 1;
                    } else {
                        break;
                    }
                }
                // Duration literal: <n>m | <n>h | <n>h<m>m.
                let unit = chars.get(i).map(|(_, c)| *c);
                let after_digits = |i: usize| chars.get(i).map(|(_, c)| *c);
                let out_of_range = |col: usize, diags: &mut Vec<Diagnostic>| {
                    diags.push(Diagnostic::error(
                        code::E246,
                        file,
                        no,
                        col as u32,
                        "duration literal out of range",
                    ));
                };
                match unit {
                    Some('m') => {
                        i += 1;
                        col += 1;
                        match digits.parse::<i64>() {
                            Ok(v) => toks.push((sp(start_col), Tok::Dur(v))),
                            Err(_) => {
                                out_of_range(start_col, diags);
                                toks.push((sp(start_col), Tok::Dur(0)));
                            }
                        }
                    }
                    Some('h') => {
                        i += 1;
                        col += 1;
                        let mut total = 0i64;
                        let mut fits = true;
                        match digits.parse::<i64>() {
                            Ok(h) => {
                                if let Some(v) = h.checked_mul(60) {
                                    total = v;
                                } else {
                                    fits = false;
                                }
                            }
                            Err(_) => fits = false,
                        }
                        // Well-spelled until proven otherwise: `<n>h<m>m`.
                        let mut spelled = true;
                        if after_digits(i).is_some_and(|c| c.is_ascii_digit()) {
                            let mut m = String::new();
                            while i < chars.len() {
                                let (_, c) = chars[i];
                                if c.is_ascii_digit() {
                                    m.push(c);
                                    i += 1;
                                    col += 1;
                                } else {
                                    break;
                                }
                            }
                            if after_digits(i) == Some('m') {
                                i += 1;
                                col += 1;
                                match m.parse::<i64>() {
                                    Ok(m) => {
                                        if let Some(v) = total.checked_add(m) {
                                            total = v;
                                        } else {
                                            fits = false;
                                        }
                                    }
                                    Err(_) => fits = false,
                                }
                            } else {
                                spelled = false;
                            }
                        }
                        if spelled {
                            if fits {
                                toks.push((sp(start_col), Tok::Dur(total)));
                            } else {
                                out_of_range(start_col, diags);
                                toks.push((sp(start_col), Tok::Dur(0)));
                            }
                        } else {
                            err(
                                start_col,
                                "bad duration literal; spell it <n>m, <n>h, or <n>h<m>m",
                                diags,
                            );
                        }
                    }
                    _ => {
                        let v: i64 = digits.parse().unwrap_or(i64::MAX);
                        toks.push((sp(start_col), Tok::Int(v)));
                    }
                }
            }
            c if c.is_ascii_lowercase() || c == '_' => {
                let start_col = col;
                let mut s = String::new();
                while i < chars.len() {
                    let (_, c) = chars[i];
                    if c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_' {
                        s.push(c);
                        i += 1;
                        col += 1;
                    } else {
                        break;
                    }
                }
                toks.push((sp(start_col), Tok::Ident(s)));
            }
            c if c.is_ascii_uppercase() => {
                err(
                    col,
                    "identifiers are lower_snake; display names are quoted strings",
                    diags,
                );
                // Skip the whole identifier-shaped run.
                while i < chars.len() {
                    let (_, c) = chars[i];
                    if c.is_ascii_alphanumeric() || c == '_' {
                        i += 1;
                        col += 1;
                    } else {
                        break;
                    }
                }
            }
            _ => {
                let start_col = col;
                let two = chars.get(i + 1).map(|(_, c)| *c);
                let (len, p) = match (ch, two) {
                    ('+', Some('=')) => (2, P::PlusEq),
                    ('-', Some('=')) => (2, P::MinusEq),
                    ('>', Some('=')) => (2, P::Ge),
                    ('<', Some('=')) => (2, P::Le),
                    ('=', Some('=')) => (2, P::EqEq),
                    ('!', Some('=')) => (2, P::Ne),
                    ('.', Some('.')) => (2, P::DotDot),
                    _ => match ch {
                        ':' => (1, P::Colon),
                        ',' => (1, P::Comma),
                        '.' => (1, P::Dot),
                        '(' => (1, P::LParen),
                        ')' => (1, P::RParen),
                        '[' => (1, P::LBracket),
                        ']' => (1, P::RBracket),
                        '+' => (1, P::Plus),
                        '-' => (1, P::Minus),
                        '*' => (1, P::Star),
                        '/' => (1, P::Slash),
                        '=' => (1, P::Eq),
                        '>' => (1, P::Gt),
                        '<' => (1, P::Lt),
                        other => {
                            err(col, &format!("unexpected character `{other}`"), diags);
                            i += 1;
                            col += 1;
                            continue;
                        }
                    },
                };
                i += len;
                col += len;
                toks.push((sp(start_col), Tok::P(p)));
            }
        }
    }

    if toks.is_empty() {
        None
    } else {
        Some(Line {
            no,
            indent: match &toks[0] {
                (s, _) => s.col,
            },
            toks,
        })
    }
}
