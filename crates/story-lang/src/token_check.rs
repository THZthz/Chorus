//! Static `{token}` checking for prose (language.md §7): a token resolves to
//! a slot name, a unique-card name path, a roll binding, a spawn handle, a
//! legacy key, the implicit `subject` inside card/expiry books, or a
//! `related:<kind> (of|to) <card-ref>` enumeration. An unknown token is
//! [E205].

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

fn unknown(c: &mut Compiler, tok: &str, span: &Span) {
    c.err(span.error(
        code::E205,
        format!("prose token `{{{tok}}}` resolves to nothing known"),
    ));
}

fn check_token(c: &mut Compiler, tok: &str, span: &Span, env: &Env) {
    if let Some(key) = tok.strip_prefix("legacy.") {
        c.check_legacy_key(key, span);
        return;
    }
    if let Some(rest) = tok.strip_prefix("related:") {
        check_related_token(c, rest, tok, span, env);
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
        unknown(c, tok, span);
    }
    if env.slots.iter().any(|s| s == tok)
        || env.rolls.contains(tok)
        || env.handles.contains(tok)
        || (env.subject && tok == "subject")
    {
        return;
    }
    unknown(c, tok, span);
}

/// `{related:<kind> (of|to) <card-ref>}`: the kind must be declared and the
/// card-ref resolvable — a unique name, `subject` in a subject context, or
/// `first(<scope> [<terms>])` over the checkable term forms. Every
/// unresolvable spelling is [E205].
fn check_related_token(c: &mut Compiler, rest: &str, tok: &str, span: &Span, env: &Env) {
    let Some((kind, after)) = rest.split_once(char::is_whitespace) else {
        unknown(c, tok, span);
        return;
    };
    let after = after.trim_start();
    let Some(ref_text) = strip_direction_word(after) else {
        unknown(c, tok, span);
        return;
    };
    let ref_text = ref_text.trim();
    if !c.relation_kinds.contains_key(kind) || ref_text.is_empty() {
        unknown(c, tok, span);
        return;
    }
    check_card_ref_text(c, ref_text, tok, span, env, 0);
}

/// `of` / `to` after the kind, word-bounded; returns the card-ref text.
fn strip_direction_word(after: &str) -> Option<&str> {
    for word in ["of", "to"] {
        if let Some(t) = after.strip_prefix(word) {
            if t.is_empty() || t.starts_with(char::is_whitespace) {
                return Some(t);
            }
        }
    }
    None
}

/// The card-ref spelling inside a `related:` token.
fn check_card_ref_text(
    c: &mut Compiler,
    text: &str,
    tok: &str,
    span: &Span,
    env: &Env,
    depth: u32,
) {
    if depth > 8 {
        unknown(c, tok, span);
        return;
    }
    let text = text.trim();
    if text == "subject" {
        if !env.subject {
            unknown(c, tok, span);
        }
        return;
    }
    if let Some(inner) = text.strip_prefix("first(") {
        let Some(body) = inner.strip_suffix(')') else {
            unknown(c, tok, span);
            return;
        };
        let body = body.trim();
        let (scope_word, term_part) = match body.split_once(char::is_whitespace) {
            Some((w, t)) => (w, t.trim()),
            None => (body, ""),
        };
        check_scope_text(c, scope_word, tok, span, env);
        check_terms_text(c, term_part, tok, span);
        return;
    }
    // A bare unique-card name; a declared card follows the name-path
    // precedent ([E239] when not unique), anything else resolves to nothing.
    if c.cards.contains_key(text) {
        c.check_card_unique(text, span);
        return;
    }
    unknown(c, tok, span);
}

/// The scope spelling inside a token's `first(...)`.
fn check_scope_text(c: &mut Compiler, scope: &str, tok: &str, span: &Span, env: &Env) {
    if let Some(z) = scope.strip_prefix("zone:") {
        if !c.zones.contains_key(z) {
            unknown(c, tok, span);
        }
        return;
    }
    if let Some(rest) = scope.strip_prefix("related:") {
        check_related_token(c, rest, tok, span, env);
        return;
    }
    if matches!(scope, "world" | "interior" | "table" | "anywhere" | "bound")
        || scope.starts_with("slot:")
    {
        return;
    }
    unknown(c, tok, span);
}

/// The query terms inside a token's `first(...)`: the checkable forms
/// `aspect:<k>`, `kind:<k>`, `name:<c>`, and negation; anything else
/// resolves to nothing (the engine's runtime reader knows these forms only).
fn check_terms_text(c: &mut Compiler, terms: &str, tok: &str, span: &Span) {
    if terms.is_empty() {
        return;
    }
    for term in terms.split_whitespace() {
        if let Some(rest) = term.strip_prefix('-') {
            check_terms_text(c, rest, tok, span);
            continue;
        }
        if let Some(key) = term.strip_prefix("aspect:") {
            let key: String = key
                .chars()
                .take_while(|ch| !matches!(ch, '>' | '<' | '=' | '!'))
                .collect();
            c.check_aspect_key(&key, span);
            continue;
        }
        if let Some(k) = term.strip_prefix("kind:") {
            if !c.kinds.contains(k) {
                c.err(
                    span.error(
                        code::E203,
                        format!("unknown card kind `{k}` (the kind registry is collected from card `kind` lines)"),
                    ),
                );
            }
            continue;
        }
        if let Some(n) = term.strip_prefix("name:") {
            if !c.cards.contains_key(n) {
                unknown(c, tok, span);
            }
            continue;
        }
        unknown(c, tok, span);
        return;
    }
}
