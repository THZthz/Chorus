//! Static `{token}` checking for prose (language.md §7): a token resolves to
//! a slot name, a unique-card name path, a roll binding, a spawn handle, a
//! legacy key, or the implicit `subject` inside card/expiry books. An unknown
//! token is [E205].

use crate::lower::{Compiler, Env};
use crate::span::Span;
use story_content::code;

pub(crate) fn check(c: &mut Compiler, text: &str, span: &Span, env: &Env) {
    let chars: Vec<char> = text.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == '{' {
            let mut j = i + 1;
            while j < chars.len() && chars[j] != '}' {
                j += 1;
            }
            if j >= chars.len() {
                c.err(span.error(code::E201, "unclosed `{` in prose token"));
                return;
            }
            let inner: String = chars[i + 1..j].iter().collect();
            check_token(c, &inner, span, env);
            i = j + 1;
        } else {
            i += 1;
        }
    }
}

fn check_token(c: &mut Compiler, tok: &str, span: &Span, env: &Env) {
    if let Some(key) = tok.strip_prefix("legacy.") {
        c.check_legacy_key(key, span);
        return;
    }
    if let Some((card, key)) = tok.split_once('.') {
        if env.subject && card == "subject" {
            c.check_aspect_key(key, span);
            return;
        }
        if c.cards.contains_key(card) {
            c.check_card_unique(card, span);
            c.check_aspect_key(key, span);
            return;
        }
        if env.handles.contains(card) {
            c.check_aspect_key(key, span);
            return;
        }
        c.err(span.error(
            code::E205,
            format!("prose token `{{{tok}}}` resolves to nothing known"),
        ));
    }
    if env.slots.iter().any(|s| s == tok)
        || env.rolls.contains(tok)
        || env.handles.contains(tok)
        || (env.subject && tok == "subject")
    {
        return;
    }
    c.err(span.error(
        code::E205,
        format!("prose token `{{{tok}}}` resolves to nothing known"),
    ));
}
