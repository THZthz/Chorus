//! Canonical serializer (spec §5.1, §4.3, §9.2).
//! Format(parse(x)) is the canonical form: actors sorted,
//! pre-verbal AT, bare-name refs (re-quoted when unsafe/reserved), concrete
//! outcomes.

use crate::core::clock::{format_clock, format_duration};
use crate::core::dict::Dictionary;
use crate::core::r#ref::{format_ref, is_safe_name, quote_string, sort_refs, Ref};
use crate::core::sentence::{
    signed_int_spelling, Arg, Clause, IntoTarget, Outcome, Sentence, SentenceKind,
};
use crate::error::{ChorusError, Result};

/// The canonical spelling of one parsed sentence (spec §5.1).
///
/// The dictionary stays an `Option` at THIS boundary on purpose — the
/// acceptance harness calls the formatter both with a loaded dictionary and
/// without (a name formats bare unless unsafe) — but it is resolved here and
/// never threaded below: every helper takes `&Dictionary`.
pub fn format_sentence(s: &Sentence, dict: &Dictionary) -> Result<String> {
    let mut parts: Vec<String> = Vec::new();
    if let Some(clock) = s.clock {
        parts.push(format!("{} @", format_clock(clock)));
    }
    match s.kind {
        SentenceKind::Event => {
            parts.push("EVENT".to_string());
            parts.push(format_trigger_id(s.trigger.as_deref().unwrap_or("?")));
        }
        SentenceKind::Complete => {
            // v0.5: `1:42 @ COMPLETE READ -> <outcomes>` — the verb prints
            // bare (UPPER_SNAKE always re-parses as a name token)
            parts.push("COMPLETE".to_string());
            parts.push(s.trigger.clone().unwrap_or_else(|| "?".to_string()));
        }
        SentenceKind::Wait => {
            // v0.5: `0:42 @ WAIT 1h` — the pure-advance clock row
            let span = format_duration(s.span_min.unwrap_or(0))?;
            parts.push("WAIT".to_string());
            parts.push(span);
        }
        SentenceKind::Action => {
            let clauses =
                s.clauses.iter().map(|c| format_clause(c, dict)).collect::<Result<Vec<_>>>()?;
            parts.push(clauses.join("; "));
        }
    }
    if !s.outcomes.is_empty() {
        let outs =
            s.outcomes.iter().map(|o| format_outcome(o, dict)).collect::<Result<Vec<_>>>()?;
        parts.push("->".to_string());
        parts.push(outs.join(", "));
    }
    Ok(parts.join(" "))
}

/// One clause in canonical order: actors (sorted), pre-verbal AT, verb,
/// TO, objects (sorted), WITH (sorted), ABOUT.
fn format_clause(c: &Clause, dict: &Dictionary) -> Result<String> {
    let mut parts: Vec<String> = Vec::new();
    let actors =
        sort_refs(&c.actors).iter().map(|r| format_ref(r, dict)).collect::<Result<Vec<_>>>()?;
    parts.push(actors.join(", "));
    if let Some(at) = &c.at {
        parts.push("AT".to_string());
        parts.push(format_ref(at, dict)?);
    }
    parts.push(c.verb.clone());
    if let Some(to) = &c.to {
        parts.push("TO".to_string());
        parts.push(format_ref(to, dict)?);
    }
    if !c.objects.is_empty() {
        let objects = sort_refs(&c.objects)
            .iter()
            .map(|r| format_ref(r, dict))
            .collect::<Result<Vec<_>>>()?;
        parts.push(objects.join(", "));
    }
    if !c.with.is_empty() {
        let with =
            sort_refs(&c.with).iter().map(|r| format_ref(r, dict)).collect::<Result<Vec<_>>>()?;
        parts.push("WITH".to_string());
        parts.push(with.join(", "));
    }
    if let Some(about) = &c.about {
        parts.push("ABOUT".to_string());
        parts.push(format_ref(about, dict)?);
    }
    Ok(parts.join(" "))
}

/// One outcome. Authored-only ops (IF, FN, ROLL, DRAW) are errors here; the
/// concrete rows ROLLED/DRAWN ride the generic paren'd fallback; SPAWN,
/// NARRATE, REPLACE and ENDING have dedicated canonical spellings.
fn format_outcome(o: &Outcome, dict: &Dictionary) -> Result<String> {
    if o.op == "IF" {
        return Err(ChorusError::new(
            "an IF outcome is authored form only — the log records the taken branch (§6.8)",
        ));
    }
    if o.op == "FN" {
        return Err(ChorusError::new(
            "a function call is authored form only — calls expand at load (§10)",
        ));
    }
    if o.op == "ROLL" {
        return Err(ChorusError::new(
            "a ROLL is authored form only — the log records the engine-written ROLLED result (§4.4)",
        ));
    }
    if o.op == "DRAW" {
        return Err(ChorusError::new(
            "a DRAW is authored form only — the log records the engine-written DRAWN+SPAWN rows (§4.4)",
        ));
    }
    // ROLLED (§4.4) and DRAWN (§4.4) ride the generic paren'd fallback on
    // purpose: their args format and re-parse — `ROLLED(14, 3)` is two plain
    // ints, `DRAWN(RUMOURS, HEARSAY)` two plain name refs
    if o.op == "RUN" {
        // §4.4: the ids render RE-QUOTED in the paren'd shell — recipe ids
        // are dotted (`study.recipe0`), and a bare dotted spelling would
        // re-parse as a SET key (the generic name path), never back into a
        // RUN candidate. RUN is zero-concrete (the follow-up's BEGIN row is
        // the record), so the log never carries one; the arm exists so the
        // §5.2 round-trip holds for the authored form wherever it formats.
        let parts = o
            .args
            .iter()
            .map(|a| match a {
                Arg::Ref { r#ref: Ref::Name { name, .. } } => Ok(quote(name)),
                _ => Err(ChorusError::new("a RUN candidate is a quoted-string recipe id (§4.4)")),
            })
            .collect::<Result<Vec<_>>>()?;
        return Ok(format!("RUN({})", parts.join(", ")));
    }
    if o.op == "SPAWN" {
        // pretty form: SPAWN X [INTO Y] [AS #id] [expires <duration>]
        let mut parts = vec!["SPAWN".to_string()];
        for a in &o.args {
            parts.push(format_arg(a, dict)?);
        }
        return Ok(parts.join(" "));
    }
    if o.op == "NARRATE" {
        return Ok(match o.args.first() {
            Some(Arg::Str { v }) => format!("NARRATE {}", quote(v)),
            _ => format!("NARRATE {}", format_args(&o.args, dict)?),
        });
    }
    if o.op == "REPLACE" {
        // canonical paren-less authored form: REPLACE ref WITH prototype (§4.4)
        let target = match o.args.first() {
            Some(a) => format_arg(a, dict)?,
            None => "?".to_string(),
        };
        let proto = match o.args.get(1) {
            Some(a) => format_arg(a, dict)?,
            None => "?".to_string(),
        };
        return Ok(format!("REPLACE {target} WITH {proto}"));
    }
    if o.op == "ENDING" {
        // canonical paren-less form: ENDING id (§4.4) — the id round-trips
        // as a name ref
        let id = match o.args.first() {
            Some(a) => format_arg(a, dict)?,
            None => "?".to_string(),
        };
        return Ok(format!("ENDING {id}"));
    }
    let args = o.args.iter().map(|a| format_arg(a, dict)).collect::<Result<Vec<_>>>()?;
    Ok(format!("{}({})", o.op, args.join(", ")))
}

/// Comma-joined args.
fn format_args(args: &[Arg], dict: &Dictionary) -> Result<String> {
    let parts = args.iter().map(|a| format_arg(a, dict)).collect::<Result<Vec<_>>>()?;
    Ok(parts.join(", "))
}

/// The canonical arg spelling. The str/pair spellings here
/// QUOTE — `fn_expand`'s `arg_spelling` deliberately does not; the two spelling
/// layers are frozen separately, only delta/int/dur/bool are shared.
fn format_arg(a: &Arg, dict: &Dictionary) -> Result<String> {
    match a {
        Arg::Ref { r#ref } => format_ref(r#ref, dict),
        Arg::Role { role } => Ok(role.as_str().to_uppercase()),
        Arg::Int { v } => Ok(v.to_string()),
        // signed args print with their sign (§4.4); -0 → +0 (Q16: the
        // sign branch is explicit on the integer domain)
        Arg::Delta { v } => Ok(signed_int_spelling(*v)),
        Arg::Str { v } => Ok(quote(v)),
        Arg::Bool { v } => Ok(v.to_string()),
        Arg::Dur { v } => format_duration(*v),
        Arg::Key { v } => Ok(v.clone()), // plain, dotted and aspect:-prefixed keys print bare (§4.4)
        Arg::Into { r#ref } => Ok(match r#ref {
            IntoTarget::Role { role } => format!("INTO {}", role.as_str().to_uppercase()),
            IntoTarget::Ref(r#ref) => format!("INTO {}", format_ref(r#ref, dict)?),
        }),
        Arg::AsId { id } => Ok(format!("AS #{id}")),
        Arg::Pair { k, v } => Ok(match v {
            crate::core::types::AttrValue::Str(s) => format!("{k}={}", quote(s)),
            other => format!("{k}={}", other.to_canonical_string()),
        }),
        Arg::Expires { v } => Ok(match v {
            // the SPAWN mint modifier (§4.4, §4.2): keyword restored, the
            // duration canonicalized per §5.1 so the rendering re-parses
            crate::core::types::AttrValue::Str(s) => format!("expires {}", quote(s)),
            crate::core::types::AttrValue::Num(mins) => {
                format!("expires {}", format_duration(*mins)?)
            }
            crate::core::types::AttrValue::Bool(b) => format!("expires {b}"),
        }),
        Arg::Vexpr { .. } => Err(ChorusError::new(
            "a ROLL value expression is authored form only — the log records the computed ROLLED ints (§4.4)",
        )),
        Arg::Param { .. } => Err(ChorusError::new(
            "a function parameter is authored form only — calls expand at load (§10)",
        )),
    }
}

/// Quote a string: `\` → `\\` first, then `"` → `\"` (backslash must be
/// escaped first or the quote escapes would double-encode). The discipline
/// itself lives once, in [`crate::core::r#ref::quote_string`] — names and
/// prose quote by the same rule.
#[must_use]
fn quote(s: &str) -> String {
    quote_string(s)
}

/// Colon-joined trigger ids (`expire:<NAME>`,
/// §6.6; authored ids may carry colons too): each segment prints bare when it
/// re-parses as a name token and quoted when it does not, so every logged
/// event row stays lossless (§1, §7) whatever the expiring entity was named.
#[must_use]
fn format_trigger_id(id: &str) -> String {
    id.split(':')
        .map(|seg| if is_safe_name(seg) { seg.to_string() } else { quote(seg) })
        .collect::<Vec<_>>()
        .join(":")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::dict::core_dictionary;
    use crate::core::parse::parse_sentence;

    fn dict() -> Dictionary {
        core_dictionary()
    }

    #[test]
    fn canonical_round_trips_the_worked_shapes() {
        let d = dict();
        // spec §5.2 worked-example shape: actors sorted, pre-verbal AT
        let s = parse_sentence("THING, ANOTHER READ TO book AT shelf", &d).unwrap();
        let out = format_sentence(&s, &d).unwrap();
        assert_eq!(out, "ANOTHER, THING AT shelf READ TO book");
        // post-verbal AT canonicalizes pre-verbal (§5.1)
        let s2 = parse_sentence("ANA READ book AT shelf", &d).unwrap();
        assert_eq!(format_sentence(&s2, &d).unwrap(), "ANA AT shelf READ book");
        // re-parse stability: format(parse(format(parse(x)))) == format(parse(x))
        let s3 = parse_sentence(&out, &d).unwrap();
        assert_eq!(format_sentence(&s3, &d).unwrap(), out);
    }

    #[test]
    fn outcome_spellings_are_byte_exact() {
        let d = dict();
        let s = parse_sentence(
            "ANA READ book -> SPAWN RUMOUR, DESTROY OBJECT, SET(level, +1), NARRATE \"a \\\"b\\\"\"",
            &d,
        )
        .unwrap();
        let out = format_sentence(&s, &d).unwrap();
        // DESTROY has no format special-case — the generic paren'd
        // fallback prints DESTROY(OBJECT), which re-parses through the
        // generic arg grammar (arg() reads OBJECT as a role ref)
        assert_eq!(
            out,
            "ANA READ book -> SPAWN RUMOUR, DESTROY(OBJECT), SET(level, +1), NARRATE \"a \\\"b\\\"\""
        );
        // ROLLED/DRAWN ride the generic paren'd fallback on purpose — their
        // args format and re-parse (the fold's counter/cursor rows)
        let s2 =
            parse_sentence("ANA READ book -> ROLLED(14, 3), DRAWN(RUMOURS, HEARSAY)", &d).unwrap();
        assert_eq!(
            format_sentence(&s2, &d).unwrap(),
            "ANA READ book -> ROLLED(14, 3), DRAWN(RUMOURS, HEARSAY)"
        );
    }

    #[test]
    fn wait_and_event_rows_print_their_rows() {
        let d = dict();
        // 90 minutes has no exact hour fit — canonical is 3halfhour — and the
        // rendering re-parses to the same span
        let s = parse_sentence("0:42 @ WAIT 1h30m", &d).unwrap();
        let out = format_sentence(&s, &d).unwrap();
        assert_eq!(out, "0:42 @ WAIT 3halfhour");
        let s2 = parse_sentence(&out, &d).unwrap();
        assert_eq!(s2.span_min, s.span_min);
        // an unsafe trigger segment is authored quoted and re-quotes per
        // colon-joined segment
        let s = parse_sentence("1:2 @ EVENT expire:\"THE DOOR\" -> NARRATE \"x\"", &d).unwrap();
        assert_eq!(
            format_sentence(&s, &d).unwrap(),
            "1:2 @ EVENT expire:\"THE DOOR\" -> NARRATE \"x\""
        );
    }

    #[test]
    fn quote_escapes_backslash_then_quote() {
        assert_eq!(quote("a\\b\"c"), "\"a\\\\b\\\"c\"");
    }
}
