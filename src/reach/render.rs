//! Text rendering: static predicate text for path-export guards, the
//! proof/chain/reason display strings that ride inside the report, and the
//! CLI's stdout shapes frozen byte for byte: render_path's 4-space indent
//! steps, `path i/n:` and `verdict: X (existence: y)`. Beside the frozen
//! faces sit the two diagnostic reports: `render_why` (the `--why` section)
//! and `render_verb` (the `--verb` report) — their bytes are pinned by the
//! tests/reach suite.

use std::fmt::Write;

use indexmap::IndexMap;

use crate::core::predicate::{Pred, ValExpr};
use crate::core::query::{Query, QueryTerm, StateOp};
use crate::core::types::Entity;

use super::census::{draw_deck, RawSite, SiteKind, MANY};
use super::fixpoint::{SiteBlocker, SiteOp};
use super::paths::{CardReport, Existence, ReachPath, SiteVerdict, VerbReport};

// ---------------------------------------------------------------------------
// rendering static predicates
// ---------------------------------------------------------------------------

/// Text of one query list: group terms space-joined, groups with ' | '
/// (§4.5). The design-time diagnostic renderer only — queries never
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
                        QueryTerm::Aspect { path, op, level, .. } => format!(
                            "{neg}aspect:{path}{}",
                            match op {
                                Some(op) => format!("{}{level}", op.as_str()),
                                None => String::new(),
                            }
                        ),
                        QueryTerm::State { neg, key, op, value } => {
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
        ValExpr::TotalTable { path } => format!("total(table, aspect:{path})"),
        ValExpr::TotalBound { path } => format!("total(bound, aspect:{path})"),
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
// the report's display strings (chain lines, candidate reasons)
// ---------------------------------------------------------------------------

/// The initial-entity provenance line.
pub(crate) fn initial_decl_line(name: &str, e: &Entity) -> String {
    let aspects = if e.aspects.is_empty() {
        String::new()
    } else {
        format!(", aspects {}", crate::core::jsonw::aspects_to_json(&e.aspects))
    };
    format!("{name} is declared in the initial entities ({}{aspects})", e.kind)
}

/// The display chain of one proven site.
pub(crate) fn site_chain(site: &RawSite, target: &str, supports: &[String]) -> Vec<String> {
    let mut lines: Vec<String> = Vec::new();
    if site.kind == SiteKind::Draw {
        let deck = draw_deck(site);
        let pos = deck.order.iter().position(|c| c.name == target);
        let where_ = match pos {
            Some(p) => format!(
                "deck {} serves '{}' at order position {} (the permutation is seed-fixed at load — pure in seed × deck id)",
                deck.id, target, p
            ),
            None => format!(
                "deck {} serves '{}' as defaultcard{}",
                deck.id,
                target,
                if deck.reset { " (reset — periodic)" } else { "" }
            ),
        };
        lines.push(where_);
        if site.cap != MANY {
            lines.push(format!(
                "  finite draw capacity: {} once-trigger draw(s) — only order[0..{}] are ever served",
                site.cap,
                site.cap - 1
            ));
        }
        lines.push(format!("  ← {}", site.where_));
    } else {
        lines.push(format!(
            "{} mints '{}' — {}",
            if site.kind == SiteKind::Replace { "REPLACE" } else { "SPAWN" },
            target,
            site.where_
        ));
        if let Some(ko) = &site.kind_override {
            lines.push(format!(
                "  minted kind overridden to '{ko}' (kind: syntax overrides kind, it does not look up by kind)"
            ));
        }
    }
    for s in supports {
        lines.push(format!("  · {s}"));
    }
    lines
}

/// Why a guarded site stays a candidate.
pub(crate) fn site_reason(site: &RawSite, init_names: &IndexMap<String, &Entity>) -> String {
    let mut bits: Vec<String> = Vec::new();
    if site.fallback {
        bits.push("ELSE fallback body".to_string());
    }
    if site.killed_in_list {
        bits.push(
            "the mint is destroyed/Replaced later in the same outcome list — it never survives a step"
                .to_string(),
        );
    }
    if site.dup_risk {
        bits.push(
            "this list mints a name that can collide with a living card (duplicate mint or an initial copy) — the refusal would discard the draft"
                .to_string(),
        );
    }
    if site.throw_ops > 0 {
        bits.push(
            "a throwing op in this list may discard the whole draft at completion".to_string(),
        );
    }
    if site.shadow_risk {
        bits.push("an earlier recipe of the same verb may shadow it (first-match)".to_string());
    }
    // QUIRK(Q17): reachable for zero-entity scripts (the language
    // allows a script with no entities:) — this branch must stay
    if site.verb.is_some() && init_names.is_empty() {
        bits.push("verb committability unknown".to_string());
    }
    if !site.path.is_empty() {
        bits.push(format!("{} IF condition(s) on the path", site.path.len()));
    }
    if site.verb.is_some() && site.when.is_some() {
        bits.push("recipe `when` guard".to_string());
    }
    if bits.is_empty() {
        "unknown guard".to_string()
    } else {
        bits.join(", ")
    }
}

/// The demand chain head line.
pub(crate) fn demand_chain_head(
    shown: &str,
    kind: &str,
    aspects: &crate::core::types::Aspects,
    demand: Option<&str>,
) -> String {
    let aspects_part = if aspects.is_empty() {
        String::new()
    } else {
        format!(", aspects {}", crate::core::jsonw::aspects_to_json(aspects))
    };
    format!("{shown} ({kind}{aspects_part}) satisfies the demand '{}'", demand.unwrap_or(""))
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
    let derived = if p.derived_name { " (derived-mint name — `${PROTO}_${OBJECT}`)" } else { "" };
    let _ = writeln!(out, "{pad}· {} '{}'{derived}{deck} — via {}", p.op.as_str(), p.mints, p.via);
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

/// The `--why` section: the target's live unproven serving sites, each with
/// its owner and the ladder's first blocker. Appended AFTER the per-card
/// report when the CLI carries `--why` (without it, nothing here renders —
/// the frozen output is byte-identical). A proven card (or one whose every
/// live serving site is proven) renders the one-line empty face.
pub fn render_why(out: &mut String, file: &str, card: &str, blockers: &[SiteBlocker]) {
    let _ = writeln!(out, "{file} · card {card} — why");
    if blockers.is_empty() {
        let _ = writeln!(out, "  (no blocked candidate sites)");
        return;
    }
    for b in blockers {
        let site = match b.op {
            SiteOp::Draw => format!("{} deck {}", b.op.as_str(), b.name),
            SiteOp::Spawn | SiteOp::Replace => format!("{} '{}'", b.op.as_str(), b.name),
        };
        let _ = writeln!(out, "  · {site} — {}: {}", b.owner, b.blocker.as_str());
    }
}

/// The `--verb` report: one title line, then duration, slots, the
/// committability verdict with its proof lines (`slot_proof_lines`' own
/// text), and one line per recipe — id, `when`, the shadow_risk flag and
/// the mint-site verdicts (`; `-joined). All data is the settled facts
/// re-rendered.
pub fn render_verb(out: &mut String, file: &str, r: &VerbReport) {
    let _ = writeln!(out, "{file} · verb {}", r.verb);
    let _ = writeln!(out, "  duration: {} min", r.duration);
    if !r.slots.is_empty() {
        let _ = writeln!(out, "  slots:");
        for s in &r.slots {
            let query = match &s.query {
                Some(q) => format!(" — {q}"),
                None => " — no query — binds anything".to_string(),
            };
            let _ = writeln!(out, "  · {} ×{}{query}", s.role, s.min);
        }
    }
    if r.committable {
        let _ = writeln!(out, "  committable: yes");
        for p in &r.proofs {
            let _ = writeln!(out, "  · {p}");
        }
    } else if r.dead {
        let _ = writeln!(out, "  committable: no — dead (a min>0 slot's query can never match)");
    } else {
        let _ = writeln!(out, "  committable: no");
    }
    if !r.recipes.is_empty() {
        let _ = writeln!(out, "  recipes:");
        for rec in &r.recipes {
            let when = match &rec.when {
                Some(w) => format!("when: {w}"),
                None => "when: none".to_string(),
            };
            let shadow = if rec.shadow_risk { ", shadow_risk" } else { "" };
            // §6.4: a recipe `dur` override rides its row; no override —
            // nothing (the verb default line above is the length)
            let dur = match rec.dur {
                Some(d) => format!(", dur {d}m"),
                None => String::new(),
            };
            let sites = if rec.sites.is_empty() {
                "no live mint site".to_string()
            } else {
                rec.sites
                    .iter()
                    .map(|s| {
                        let site = match s.op {
                            SiteOp::Draw => format!("{} deck {}", s.op.as_str(), s.name),
                            SiteOp::Spawn | SiteOp::Replace => {
                                format!("{} '{}'", s.op.as_str(), s.name)
                            }
                        };
                        match &s.verdict {
                            SiteVerdict::Proven => format!("{site}: proven"),
                            SiteVerdict::Unproven(b) => {
                                format!("{site}: unproven — {}", b.as_str())
                            }
                        }
                    })
                    .collect::<Vec<String>>()
                    .join("; ")
            };
            let _ = writeln!(out, "  · {} ({when}{shadow}{dur}) — {sites}", rec.id);
        }
    }
    out.push('\n');
}

/// One card's report block (the CLI calls this
/// per card; a trailing blank line separates reports). The frozen shape —
/// exactly [`render_report_by`] with no `--by` annotation.
pub fn render_report(
    out: &mut String,
    file: &str,
    card: &str,
    demand: Option<&str>,
    usable: bool,
    r: &CardReport,
) {
    render_report_by(out, file, card, demand, usable, None, r);
}

/// The same block with the `--by` face's one header annotation
/// (`  [by minute N]`, the `[usable semantics]` bracket style). `None`
/// renders byte-identical to [`render_report`].
pub fn render_report_by(
    out: &mut String,
    file: &str,
    card: &str,
    demand: Option<&str>,
    usable: bool,
    by: Option<i64>,
    r: &CardReport,
) {
    let head = match demand {
        // an EMPTY demand is falsy (the frozen truthiness) and prints the bare card
        // name; otherwise the header is "matching '<demand>'"
        Some(d) if !d.is_empty() => format!("matching '{d}'"),
        _ => card.to_string(),
    };
    let by_tag = match by {
        Some(m) => format!("  [by minute {m}]"),
        None => String::new(),
    };
    let _ = writeln!(
        out,
        "{file} · card {head}{}{by_tag}",
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
