//! The verb-block parsers: slot spec lines (`role: count query`) and recipe
//! lines (`[id] [hidden|shown] [hint "..."] [max N] [dur D] [aspects NAME, ...]
//! [when COND] -> OUTCOMES`).

use std::sync::OnceLock;

use regex::Regex;

use crate::content::{RecipeDef, SlotDef};
use crate::core::clock::parse_duration;
use crate::core::num_parse::parse_authored_int;
use crate::core::regex_util::{grp_opt, grp_text, static_regex};
use crate::core::roles::{role_names, Role};
use crate::core::text::split_dsl_ws;
use crate::core::text::{dsl_trim, is_dsl_ws_char, DSL_NON_WS_CLASS, DSL_WS_CLASS};
use crate::error::{ChorusError, Result};

/// `/^(\S+)(?:\s+(.*))?$/`
fn slot_line_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    static_regex(&RE, || Regex::new(&format!(r#"^({DSL_NON_WS_CLASS}+)(?:{DSL_WS_CLASS}+(.*))?$"#)))
}

/// `/^(\d+)(?:\.\.(\d+)?)?$/`
fn count_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    static_regex(&RE, || Regex::new(r"^([0-9]+)(?:\.\.([0-9]+)?)?$"))
}
/// `/^default\b/`
fn default_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    static_regex(&RE, || Regex::new(r"^default(?-u:\b)"))
}

/// `/^[a-z0-9_.]+$/` (candidate recipe ids)
fn recipe_id_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    static_regex(&RE, || Regex::new(r"^[a-z0-9_.]+$"))
}

/// `/hint\s+"([^"]*)"/`
fn hint_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    static_regex(&RE, || Regex::new(&format!(r#"hint{DSL_WS_CLASS}+"([^"]*)""#)))
}

/// `/(?:^|\s)when\s+(.+)$/` (single `\s`)
fn when_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    static_regex(&RE, || Regex::new(&format!(r"(?:^|{DSL_WS_CLASS})when{DSL_WS_CLASS}+(.+)$")))
}

/// `ln` is the 1-BASED line number.
pub(super) fn parse_slot_line(role: &str, rest: &str, ln: usize) -> Result<SlotDef> {
    // §4.3's six-role list + §10's "failing loudly at load time (… bad
    // grammar)": a slot key outside the role vocabulary is a load error —
    // formerly QUIRK(Q4), the role was cast unvalidated and a typo'd slot
    // compiled only to silently never match.
    if Role::from_lower(role).is_none() {
        return Err(ChorusError::new(format!(
            "line {ln}: bad slot role '{role}' (expected one of {})",
            role_names().join(", ")
        )));
    };
    let trimmed = dsl_trim(rest);
    let Some(m) = slot_line_re().captures(trimmed) else {
        return Err(ChorusError::new(format!("line {ln}: bad slot '{role}: {rest}'")));
    };
    let count_spec = grp_text(&m, 1);
    let Some(cm) = count_re().captures(count_spec) else {
        return Err(ChorusError::new(format!(
            "line {ln}: bad count '{count_spec}' (expected 1, 1..2, 2.., 0..1)"
        )));
    };
    let min = parse_authored_int(grp_text(&cm, 1));
    // max: the upper-bound group when present; a trailing `..` means
    // unbounded (no upper bound); no `..` pins max = min.
    let max = match grp_opt(&cm, 2) {
        Some(g) => Some(parse_authored_int(g)),
        None if count_spec.ends_with("..") => None, // unbounded
        None => Some(min),
    };
    // §6.1: the slot flags — `greedy` / `consuming` sit between the count and
    // the query, in any order and composition. They are held-only (the
    // compile rejects them on any other role, §6.1); the query is whatever
    // follows the flags, sliced from the original text so its bytes ride
    // untouched.
    let rest_all = grp_opt(&m, 2).unwrap_or_default();
    let mut greedy = false;
    let mut consuming = false;
    let mut pos = 0usize;
    let query_start = loop {
        // skip whitespace, then test one token for a flag keyword
        while let Some(ch) = rest_all[pos..].chars().next() {
            if is_dsl_ws_char(ch) {
                pos += ch.len_utf8();
            } else {
                break;
            }
        }
        let tok_end = rest_all[pos..].find(is_dsl_ws_char).map_or(rest_all.len(), |end| pos + end);
        match &rest_all[pos..tok_end] {
            "greedy" if !greedy => {
                greedy = true;
                pos = tok_end;
            }
            "consuming" if !consuming => {
                consuming = true;
                pos = tok_end;
            }
            _ => break pos,
        }
    };
    let query =
        rest_all.get(query_start..).map(str::trim).filter(|q| !q.is_empty()).map(str::to_string);
    Ok(SlotDef {
        // validated above via Role::from_lower — every slot role binds
        role: role.to_string(),
        min,
        max,
        query,
        shown: None,
        greedy,
        consuming,
    })
}

/// `ln` is the 1-BASED line number.
pub(super) fn parse_recipe_line(t: &str, ln: usize) -> Result<RecipeDef> {
    // the ASCII `->` needle aligns byte and
    // UTF-16 unit offsets, so the byte slices carry the same text
    let Some(arrow) = t.find("->") else {
        return Err(ChorusError::new(format!(
            "line {ln}: recipes expect  [id] [hidden|shown] [hint \"...\"] [max N] [dur D] [aspects NAME, ...] [when COND] -> OUTCOMES"
        )));
    };
    let lhs = dsl_trim(&t[..arrow]);
    let rhs = dsl_trim(&t[arrow + 2..]);
    let mut def = RecipeDef { out: vec![rhs.to_string()], ..RecipeDef::default() };
    if default_re().is_match(lhs) {
        // §6.4: `default` is the whole shape — a trailing token is a load
        // error, never a silently dropped modifier
        let rest = dsl_trim(&lhs["default".len()..]);
        if !rest.is_empty() {
            return Err(ChorusError::new(format!(
                "line {ln}: `default` takes no modifiers — got '{rest}' (a default recipe is `default -> OUTCOMES`; a conditional one is `[id] [when COND] -> OUTCOMES`, §6.4)"
            )));
        }
        return Ok(def);
    }
    let tokens = split_dsl_ws(lhs);
    let mut i = 0usize;
    if let Some(tok0) = tokens.first() {
        // §6.4/§10: `max`, `aspects` and `dur` are keywords — a leading
        // `max N`, `aspects NAME, …` or `dur D` must never parse as a recipe id
        if recipe_id_re().is_match(tok0)
            && *tok0 != "when"
            && *tok0 != "hidden"
            && *tok0 != "shown"
            && *tok0 != "max"
            && *tok0 != "aspects"
            && *tok0 != "dur"
        {
            def.id = Some((*tok0).to_string());
            i += 1;
        }
    }
    // §6.4: the positional walk — `hidden|shown` and `max N` compose in any
    // order around the regex-extracted clauses (N ≥ 1: a `max 0` or negative
    // is a load error — such a recipe could never fire, so it fails loudly
    // instead). §10 adds the `aspects NAME, …` presence list to the same
    // walk — both orders around `max` parse — and the `dur D` window
    // override (the same §5.1 duration literals the verb's `duration:`
    // FIELD takes; the lowercase token is distinct from the field).
    // `hint "…"` and `when COND` are
    // matched on the lhs BELOW, so their tokens stay on the stream: the walk
    // jumps the hint's span by byte offset, and a token left at the cursor
    // that neither clause owns is a load error — positionals are never
    // silently dropped.
    let hint_cap = hint_re().captures(lhs);
    let hint_end = hint_cap.as_ref().and_then(|c| c.get(0)).map(|m| m.end());
    // byte offset of each lhs token's start — the hint jump below skips the
    // hint's WHOLE quoted span (its text may contain whitespace), not tokens
    let token_starts: Vec<usize> = {
        let mut pos = 0usize;
        let mut starts: Vec<usize> = Vec::with_capacity(tokens.len());
        for t in &tokens {
            while let Some(ch) = lhs[pos..].chars().next() {
                if is_dsl_ws_char(ch) {
                    pos += ch.len_utf8();
                } else {
                    break;
                }
            }
            starts.push(pos);
            pos += t.len();
        }
        starts
    };
    loop {
        match tokens.get(i) {
            Some(tok) if *tok == "hidden" || *tok == "shown" => {
                def.shown = Some(*tok == "shown");
                i += 1;
            }
            Some(tok) if *tok == "max" => {
                i += 1;
                let Some(n_tok) = tokens.get(i) else {
                    return Err(ChorusError::new(format!(
                        "line {ln}: `max` needs an integer N ≥ 1"
                    )));
                };
                let n = parse_authored_int(n_tok);
                if n < 1 {
                    return Err(ChorusError::new(format!(
                        "line {ln}: `max {n_tok}` — N must be an integer ≥ 1"
                    )));
                }
                def.max = Some(n);
                i += 1;
            }
            Some(tok) if *tok == "aspects" => {
                // §10: `aspects <name> [, <name>]*` — the presence list an
                // `on complete <aspect>` gate reads. The whitespace split
                // leaves each separator comma riding its member token; every
                // member but the last must end in one, so `aspects a max 1`
                // never swallows the positional `max` (the keyword set is
                // excluded from members entirely).
                i += 1;
                let mut names: Vec<String> = Vec::new();
                while let Some(tok) = tokens.get(i) {
                    if !is_aspects_member(tok) {
                        break;
                    }
                    let ended = !tok.ends_with(',');
                    names.push(tok.trim_end_matches(',').to_string());
                    i += 1;
                    if ended {
                        break;
                    }
                }
                if names.is_empty() {
                    return Err(ChorusError::new(format!(
                        "line {ln}: `aspects` needs at least one name — aspects <name> [, <name>]*"
                    )));
                }
                def.aspects.extend(names);
            }
            Some(tok) if *tok == "dur" => {
                // §6.4: the per-recipe window override — the same §5.1
                // duration grammar the verb's `duration:` FIELD takes
                // (`30m`, `2h`, `1h30m`, …); `0m` is legal exactly like a
                // zero verb duration (the window completes at its begin
                // minute). Parse errors ride parse_duration's own message,
                // like the field arm.
                i += 1;
                let Some(d_tok) = tokens.get(i) else {
                    return Err(ChorusError::new(format!(
                        "line {ln}: `dur` needs a duration (e.g. 30m, 2h, 90m, 1d, 1h30m)"
                    )));
                };
                def.dur = Some(parse_duration(d_tok)?);
                i += 1;
            }
            // the quoted hint text may contain whitespace — skip its whole
            // span by offset. A span that ends before the cursor (a second
            // `hint`) leaves i unmoved: the trailing check below errors.
            Some(tok) if *tok == "hint" && hint_end.is_some() => {
                let end = hint_end.unwrap_or(0);
                let after = token_starts.partition_point(|&s| s < end);
                if after > i {
                    i = after;
                } else {
                    break;
                }
            }
            _ => break,
        }
    }
    if let Some(tok) = tokens.get(i) {
        if *tok != "when" || !when_re().is_match(lhs) {
            return Err(ChorusError::new(format!(
                "line {ln}: unexpected '{tok}' before '->' — positionals are [id] [hidden|shown] [hint \"...\"] [max N] [dur D] [aspects NAME, ...], then [when COND]"
            )));
        }
    }
    if let Some(hint) = &hint_cap {
        def.hint = Some(grp_text(hint, 1).to_string());
    }
    if let Some(wm) = when_re().captures(lhs) {
        def.when = Some(grp_text(&wm, 1).to_string());
    }
    Ok(def)
}

/// An `aspects` list member after the whitespace split (§10): a lowercase
/// name with an optional trailing separator comma. The positional keywords
/// are excluded, so a list never swallows `max`/`when`/…, and a dotted
/// sub-aspect (`language.latin`) is one member — the exact class the
/// `on complete <aspect>` head reads (its trailing-dot case errors there).
fn is_aspects_member(tok: &str) -> bool {
    let t = tok.strip_suffix(',').unwrap_or(tok);
    !t.is_empty()
        && t.starts_with(|c: char| c.is_ascii_lowercase())
        && t.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_' || c == '.')
        && !matches!(
            t,
            "max" | "when" | "hidden" | "shown" | "hint" | "default" | "aspects" | "dur"
        )
}
