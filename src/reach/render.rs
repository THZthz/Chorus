//! Text rendering: static predicate text for path-export guards and
//! proof/chain/reason display, plus the CLI's stdout shapes frozen byte for
//! byte: render_path's 4-space indent steps, `path i/n:` and
//! `verdict: X (existence: y)`.

use std::fmt::Write;

use crate::core::predicate::{Pred, ValExpr};
use crate::core::query::{Query, QueryTerm, StateOp};

use super::paths::{CardReport, Existence, ReachPath};

// ---------------------------------------------------------------------------
// rendering static predicates
// ---------------------------------------------------------------------------

/// Text of one query list: group terms space-joined, groups with ' | '
/// (§9.49). The design-time diagnostic renderer only — queries never
/// serialize into the log, so this is not the forbidden canonical printer.
pub(crate) fn query_text(q: &Query) -> String {
    q.iter()
        .map(|group| {
            group
                .iter()
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
                                        .map(|v| v.to_canonical_string())
                                        .unwrap_or_default()
                                ),
                            }
                        }
                    }
                })
                .collect::<Vec<String>>()
                .join(" ")
        })
        .collect::<Vec<String>>()
        .join(" | ")
}

/// Text of one static value expression.
pub(crate) fn expr_text(v: &ValExpr) -> String {
    match v {
        ValExpr::Int { v } => v.to_string(),
        ValExpr::CountQuery { query } => format!("count({})", query_text(query)),
        // non-static operand — never on a proven path
        _ => "?".to_string(),
    }
}

/// Text of one static predicate.
pub fn pred_text(p: &Pred) -> String {
    match p {
        Pred::And(a, b) => format!("({} && {})", pred_text(a), pred_text(b)),
        Pred::Or(a, b) => format!("({} || {})", pred_text(a), pred_text(b)),
        Pred::Not(a) => format!("!({})", pred_text(a)),
        Pred::Cmp { l, op, r } => format!("{} {} {}", expr_text(l), op.as_str(), expr_text(r)),
        Pred::Exists { name } => format!("exists({name})"),
        Pred::ExistsQuery { query } => format!("exists({})", query_text(query)),
        // the tag is the variant name (rel/every/any):
        // "<non-static rel>" and friends
        Pred::Rel { .. } => "<non-static rel>".to_string(),
        Pred::Every { .. } => "<non-static every>".to_string(),
        Pred::Any { .. } => "<non-static any>".to_string(),
    }
}

// ---------------------------------------------------------------------------
// the CLI stdout shapes
// ---------------------------------------------------------------------------

/// One path node with its
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
    // the literal text carries backticks and the
    // `${PROTO}_${OBJECT}` shape verbatim — those exact characters are emitted
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

/// One card's report block (the CLI calls this
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
        // an EMPTY demand is falsy (the frozen truthiness) and prints the bare card
        // name; otherwise the header is "matching '<demand>'"
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
