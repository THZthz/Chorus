//! The `on complete [<aspect>] -> ( outcomes )` clause machinery (§10): the
//! string- and paren-aware clause locator, the clause-sequence parser, and
//! the engine-minted completion-trigger ids.

use indexmap::IndexMap;

use crate::content::{CompleteTriggerDef, TriggerDef};
use crate::core::text::{dsl_trim, is_dsl_ws_char};
use crate::error::{ChorusError, Result};

/// One parsed `on complete [<aspect>] -> ( body )` clause (§10). The body
/// is the RAW outcome text inside the parens — compiled like every trigger
/// body (function expansion, the load guards) in `compile_content`.
#[derive(Debug, Clone)]
pub(crate) struct CompleteClause {
    pub aspect: Option<String>,
    pub body: String,
}

/// The clause keywords an aspect name may not collide with (§10): the
/// entity/prototype-line clause heads and the trigger's own keywords —
/// `with` is the load-bearing one (the with-clause scanner works the same
/// line), the rest keep the vocabulary closed.
const COMPLETE_ASPECT_BANNED: [&str; 7] =
    ["with", "at", "in", "on", "complete", "deriving", "stackable"];

/// Push one parsed clause as a completion trigger under `home` (§10).
pub(super) fn push_complete(
    triggers: &mut Vec<TriggerDef>,
    home: &str,
    clause: CompleteClause,
    ordinals: &mut IndexMap<String, usize>,
) {
    triggers.push(TriggerDef::Complete(CompleteTriggerDef {
        id: mint_complete_id(home, ordinals),
        home: home.to_string(),
        aspect: clause.aspect,
        out: vec![clause.body],
    }));
}

/// Byte offset of the first TOP-LEVEL `on complete` in `text`, or `None`.
/// Top-level means paren-depth 0 and outside a string literal — an `on
/// complete` inside a clause body (e.g. in a NARRATE string) never splits the
/// line. Runs on split('\n') + trimmed SINGLE-LINE text only (the module's
/// frozen regex domain). A token start is the text start or a position after
/// a DSL-whitespace CHARACTER (the frozen class — U+3000 and U+FEFF
/// included), so a name like `only_complete` never matches and a wide-space
/// boundary splits exactly like an ASCII one; the walk is char_indices, so
/// no multi-byte char is ever read as its first byte.
pub(super) fn find_on_complete(text: &str) -> Option<usize> {
    let mut depth: usize = 0;
    let mut in_str = false;
    let mut prev: Option<char> = None;
    for (i, c) in text.char_indices() {
        if in_str {
            if c == '"' {
                in_str = false;
            }
            prev = Some(c);
            continue;
        }
        match c {
            '"' => in_str = true,
            '(' => depth += 1,
            ')' => depth = depth.saturating_sub(1),
            _ => {
                let at_token_start = prev.is_none_or(is_dsl_ws_char);
                if depth == 0 && at_token_start && c == 'o' && text[i..].starts_with("on") {
                    // the boundary after `on`, then the `complete` token, then
                    // its own boundary (an aspect token or the arrow follows)
                    let after_on = &text[i + "on".len()..];
                    let ws_run = after_on.len() - after_on.trim_start_matches(is_dsl_ws_char).len();
                    if let Some(after) = after_on[ws_run..].strip_prefix("complete") {
                        let bounded =
                            after.chars().next().is_none_or(|n| is_dsl_ws_char(n) || n == '-');
                        if bounded {
                            return Some(i);
                        }
                    }
                }
            }
        }
        prev = Some(c);
    }
    None
}

/// Offset of the ')' matching the '(' at offset 0 of `s` — string-literal
/// aware (the string grammar has no escapes: the hint form `[^"]*` froze
/// that). `None` when unbalanced.
fn matching_paren(s: &str) -> Option<usize> {
    let mut depth: usize = 0;
    let mut in_str = false;
    for (i, b) in s.bytes().enumerate() {
        if in_str {
            if b == b'"' {
                in_str = false;
            }
            continue;
        }
        match b {
            b'"' => in_str = true,
            b'(' => depth += 1,
            b')' => {
                depth = depth.saturating_sub(1);
                if depth == 0 {
                    return Some(i);
                }
            }
            _ => {}
        }
    }
    None
}

/// Parse the clause SEQUENCE
/// `on complete [<aspect>] -> ( body ) [on complete …]` from `src` — the
/// caller locates the first clause with [`find_on_complete`], so `src` starts
/// there. The paren'd body may itself carry nested parens (IF arms) and
/// strings; the depth scan is string-aware for both. `ln` is 1-BASED.
/// (No dictionary needed: the aspect class is syntactic, and its collision
/// set is the local clause-keyword list.)
pub(super) fn parse_complete_clauses(src: &str, ln: usize) -> Result<Vec<CompleteClause>> {
    const SYNTAX: &str = "on complete [<aspect>] -> ( outcomes )";
    let mut rest = dsl_trim(src);
    let mut clauses = Vec::new();
    loop {
        if rest.is_empty() {
            return Ok(clauses);
        }
        let Some(after_on) = rest.strip_prefix("on").and_then(|r| {
            let r = r.trim_start_matches(is_dsl_ws_char);
            r.strip_prefix("complete")
        }) else {
            return Err(ChorusError::new(format!("line {ln}: expected `{SYNTAX}`, got '{rest}'")));
        };
        // the keyword ends at a boundary — an aspect token or the arrow
        if !after_on.chars().next().is_none_or(|c| is_dsl_ws_char(c) || c == '-') {
            return Err(ChorusError::new(format!("line {ln}: expected `{SYNTAX}`, got '{rest}'")));
        }
        let mut cur = after_on.trim_start_matches(is_dsl_ws_char);
        // the optional aspect: one bare lowercase name token — DOTTED keys
        // included, the exact class the recipe `aspects` list declares (the
        // gate matches the full dotted key, §10)
        let mut aspect: Option<String> = None;
        let name_end = cur
            .chars()
            .take_while(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || *c == '_' || *c == '.')
            .count();
        if name_end > 0 && cur.starts_with(|c: char| c.is_ascii_lowercase()) {
            let tok = &cur[..name_end];
            if COMPLETE_ASPECT_BANNED.contains(&tok) {
                return Err(ChorusError::new(format!(
                    "line {ln}: aspect '{tok}' is a reserved clause keyword (§10)"
                )));
            }
            if tok.ends_with('.') {
                return Err(ChorusError::new(format!(
                    "line {ln}: bad aspect '{tok}' — a dotted aspect name cannot end at its dot (§10)"
                )));
            }
            aspect = Some(tok.to_string());
            cur = cur[name_end..].trim_start_matches(is_dsl_ws_char);
        }
        let Some(after_arrow) = cur.strip_prefix("->") else {
            return Err(ChorusError::new(format!(
                "line {ln}: `on complete` needs `-> ( outcomes )`{}",
                aspect.as_ref().map(|a| format!(" after aspect '{a}'")).unwrap_or_default()
            )));
        };
        let body_src = after_arrow.trim_start_matches(is_dsl_ws_char);
        let Some(open_rel) = body_src.find('(') else {
            return Err(ChorusError::new(format!(
                "line {ln}: `on complete` needs a paren'd outcome list: {SYNTAX}"
            )));
        };
        if !dsl_trim(&body_src[..open_rel]).is_empty() {
            return Err(ChorusError::new(format!(
                "line {ln}: `on complete` needs a paren'd outcome list: {SYNTAX}"
            )));
        }
        let Some(close_rel) = matching_paren(&body_src[open_rel..]) else {
            return Err(ChorusError::new(format!(
                "line {ln}: `on complete` outcome list is missing its closing ')'"
            )));
        };
        let open = open_rel + 1;
        let close = open_rel + close_rel;
        let body = dsl_trim(&body_src[open..close]);
        if body.is_empty() {
            return Err(ChorusError::new(format!(
                "line {ln}: `on complete` needs an outcome list: {SYNTAX}"
            )));
        }
        clauses.push(CompleteClause { aspect, body: body.to_string() });
        rest = dsl_trim(&body_src[close + 1..]);
    }
}

/// The engine-minted completion-trigger id (§6.7): `complete:{home}` for the
/// first clause authored on a home, `complete:{home}#n` from the second on —
/// deterministic and unique, shaped like the engine-written `expire:{name}`
/// id. The id surfaces in load errors only: completion triggers log no EVENT
/// rows and never touch `state.fired`.
fn mint_complete_id(home: &str, ordinals: &mut IndexMap<String, usize>) -> String {
    let n = ordinals.entry(home.to_string()).or_insert(0);
    *n += 1;
    if *n == 1 {
        format!("complete:{home}")
    } else {
        format!("complete:{home}#{}", n)
    }
}
