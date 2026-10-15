//! Text rendering — ts: src/reach/reach.ts:531-575 (static predicate text for
//! path-export guards) and :1917-1968 (proof/chain/reason display), plus the
//! reach-cli's stdout shapes (tools/reach-cli.ts:44-79) frozen byte for byte:
//! renderPath's 4-space indent steps, `path i/n:` and
//! `verdict: X (existence: y)`.

use std::fmt::Write;

use crate::core::predicate::{Pred, ValExpr};
use crate::core::query::{QueryTerm, StateOp};

use super::paths::{CardReport, Existence, ReachPath};

// ---------------------------------------------------------------------------
// rendering static predicates — ts: reach.ts:538-564
// ---------------------------------------------------------------------------

/// ts: reach.ts:538-546
pub(crate) fn query_text(q: &[QueryTerm]) -> String {
    q.iter()
        .map(|t| {
            let neg = if t.neg() { "!" } else { "" };
            match t {
                QueryTerm::Kind { kind, .. } => format!("{neg}kind:{kind}"),
                QueryTerm::Name { name, .. } => format!("{neg}name:{name}"),
                QueryTerm::Aspect {
                    path, op, level, ..
                } => format!(
                    "{neg}aspect:{path}{}",
                    match op {
                        Some(op) => format!("{}{level}", op.as_str()),
                        None => String::new(),
                    }
                ),
                QueryTerm::State {
                    neg, key, op, value,
                } => {
                    let neg = if *neg { "!" } else { "" };
                    match op {
                        StateOp::Bang => format!("{neg}state:!{key}"),
                        StateOp::Cmp(op) => format!(
                            "{neg}state.{key}{}{}",
                            op.as_str(),
                            value
                                .as_ref()
                                .map(|v| v.to_js_string())
                                .unwrap_or_default()
                        ),
                    }
                }
            }
        })
        .collect::<Vec<String>>()
        .join(" ")
}

/// ts: reach.ts:548-552
pub(crate) fn expr_text(v: &ValExpr) -> String {
    match v {
        ValExpr::Int { v } => v.to_string(),
        ValExpr::CountQuery { query } => format!("count({})", query_text(query)),
        // non-static operand — never on a proven path
        _ => "?".to_string(),
    }
}

/// ts: reach.ts:554-564 — exported in TS (`export function predText`).
pub fn pred_text(p: &Pred) -> String {
    match p {
        Pred::And(a, b) => format!("({} && {})", pred_text(a), pred_text(b)),
        Pred::Or(a, b) => format!("({} || {})", pred_text(a), pred_text(b)),
        Pred::Not(a) => format!("!({})", pred_text(a)),
        Pred::Cmp { l, op, r } => format!("{} {} {}", expr_text(l), op.as_str(), expr_text(r)),
        Pred::Exists { name } => format!("exists({name})"),
        Pred::ExistsQuery { query } => format!("exists({})", query_text(query)),
        // ts: reach.ts:562 — `<non-static ${p.t}>`; the TS tags are the
        // variant discriminants 'rel' / 'every' / 'any'
        Pred::Rel { .. } => "<non-static rel>".to_string(),
        Pred::Every { .. } => "<non-static every>".to_string(),
        Pred::Any { .. } => "<non-static any>".to_string(),
    }
}

// ---------------------------------------------------------------------------
// the reach-cli stdout shapes — ts: tools/reach-cli.ts:44-79
// ---------------------------------------------------------------------------

/// ts: tools/reach-cli.ts:44-66 — `renderPath`, one path node with its
/// recursive preconditions; indent steps are 4 spaces per depth level.
pub fn render_path(out: &mut String, p: &ReachPath, depth: usize) {
    let pad = "    ".repeat(depth + 1);
    let deck = match &p.deck {
        Some(d) => format!(
            " — deck {}{}{}",
            d.id,
            match d.position {
                Some(pos) => format!(" order[{pos}]"),
                None => " defaultcard".to_string(),
            },
            match d.capacity {
                Some(c) => format!(" (draw capacity {c})"),
                None => String::new(),
            }
        ),
        None => String::new(),
    };
    // ts: tools/reach-cli.ts:48 — the literal text carries backticks and the
    // ${PROTO}_${OBJECT} shape verbatim
    let derived = if p.derived_name {
        " (derived-mint name — `${PROTO}_${OBJECT}`)"
    } else {
        ""
    };
    let _ = writeln!(
        out,
        "{pad}· {} '{}'{derived}{deck} — via {}",
        p.op.as_str(),
        p.mints,
        p.via
    );
    for g in &p.guards {
        let _ = writeln!(out, "{pad}    guard: {g}");
    }
    for s in &p.supports {
        let _ = writeln!(out, "{pad}    · {s}");
    }
    for r in &p.requires {
        let _ = writeln!(out, "{pad}    requires {} — {}", r.name, r.how);
        if r.paths.is_empty() {
            let _ = writeln!(
                out,
                "{pad}      (no proven source on this branch — cycle guard / depth cap)"
            );
        }
        for sub in &r.paths {
            render_path(out, sub, depth + 2);
        }
    }
}

/// ts: tools/reach-cli.ts:68-79 — one card's report block (the CLI calls this
/// per card; a trailing blank line separates reports).
pub fn render_report(
    out: &mut String,
    file: &str,
    card: &str,
    demand: Option<&str>,
    usable: bool,
    r: &CardReport,
) {
    let head = match demand {
        // ts: tools/reach-cli.ts:56 — `demand ? \`matching '${demand}'\` : name`
        // — JS truthiness: the EMPTY string is falsy and prints the card name
        // (wave-5 must-fix-1)
        Some(d) if !d.is_empty() => format!("matching '{d}'"),
        _ => card.to_string(),
    };
    let _ = writeln!(
        out,
        "{file} · card {head}{}",
        if usable { "  [usable semantics]" } else { "" }
    );
    let _ = writeln!(
        out,
        "  verdict: {}  (existence: {})",
        r.verdict.as_str().to_uppercase(),
        r.existence.as_str()
    );
    if r.paths.is_empty() && r.existence == Existence::Proven {
        let _ = writeln!(out, "  (no path exported — unexpected)");
    }
    for (i, p) in r.paths.iter().enumerate() {
        let _ = writeln!(out, "  path {}/{}:", i + 1, r.paths.len());
        render_path(out, p, 0);
    }
    for c in &r.candidates {
        let _ = writeln!(out, "  candidate source: {} — guarded ({})", c.via, c.reason);
    }
    for u in &r.usable_via {
        let _ = writeln!(out, "  bindable via {u}");
    }
    for n in &r.notes {
        let _ = writeln!(out, "  note: {n}");
    }
    out.push('\n');
}
