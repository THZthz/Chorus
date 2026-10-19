//! The `explain <verb> [bindings…] [all]` view (§11): one verb's slots and
//! every recipe in evaluation order, each marked against a binding — §6.4's
//! first-match walk made visible. The marks come from the SAME evaluator
//! selection runs (`eval_pred`, strict-false, roll-less, `max` counts read
//! off the state), so a MATCH here is what the engine would decide; the
//! `when` AST and the §4.5 slot queries render back to authored-shaped text
//! (`pred_text`/`query_text`) because a recipe a player cannot read is a
//! recipe they cannot argue with.

use crate::content::{CompiledSlot, CompiledVerb};
use crate::core::predicate::{eval_pred, Pred, PredCtx, RoleBindings, ScalarCond, ValExpr};
use crate::core::query::{CmpOp, Query, QueryTerm, StateOp};
use crate::core::roles::ROLES;
use crate::core::types::{AttrValue, State};
use crate::error::Result;

use super::render::{display_name, pretty};

/// Detail entries per side without the `all` flag: the first [`DETAIL_LIMIT`]
/// deciding recipes (fires / refused / shadowed / spent) and the first
/// [`DETAIL_LIMIT`] no-MATCHes — a 500-recipe verb renders its counts and a
/// readable head, never a scroll.
pub const DETAIL_LIMIT: usize = 5;

/// The §4.5 query back as authored-shaped text: conjunction groups joined
/// ` | `, terms space-joined (`kind:text aspect:secret>=3 -name:X`).
#[must_use]
pub fn query_text(q: &Query) -> String {
    q.iter()
        .map(|g| {
            g.iter()
                .map(|t| {
                    let body = match t {
                        QueryTerm::Kind { kind, .. } => format!("kind:{kind}"),
                        QueryTerm::Aspect { path, op, level, .. } => match op {
                            Some(op) => format!("aspect:{path}{}{level}", op.as_str()),
                            None => format!("aspect:{path}"),
                        },
                        QueryTerm::State { key, op, value, .. } => match (op, value.as_ref()) {
                            (StateOp::Bang, _) => format!("state:!{key}"),
                            (StateOp::Cmp(CmpOp::Eq), Some(AttrValue::Bool(true))) => {
                                format!("state:{key}")
                            }
                            (StateOp::Cmp(op), Some(v)) => {
                                format!("state:{key}{}{}", op.as_str(), v.to_canonical_string())
                            }
                            (_, None) => format!("state:{key}"),
                        },
                        QueryTerm::Name { name, .. } => format!("name:{name}"),
                    };
                    if t.neg() {
                        format!("-{body}")
                    } else {
                        body
                    }
                })
                .collect::<Vec<_>>()
                .join(" ")
        })
        .collect::<Vec<_>>()
        .join(" | ")
}

/// The §6.3 predicate AST back as authored-shaped text (`a >= 1 AND NOT
/// any(actors, aspect:x >= 2)`, `rel(north, actors.container, object)`).
#[must_use]
pub fn pred_text(p: &Pred) -> String {
    match p {
        Pred::And(a, b) => format!("{} AND {}", compound(a), compound(b)),
        Pred::Or(a, b) => format!("{} OR {}", compound(a), compound(b)),
        Pred::Not(a) => format!("NOT {}", compound(a)),
        Pred::Cmp { l, op, r } => {
            format!("{} {} {}", val_expr_text(l), op.as_str(), val_expr_text(r))
        }
        Pred::Rel { rtype, role, role2 } => match role2 {
            Some(r2) => format!("rel({rtype}, {role}, {r2})"),
            None => format!("rel({rtype}, {role})"),
        },
        Pred::Every { role, cond } => format!("every({role}, {})", cond_text(cond)),
        Pred::Any { role, cond } => format!("any({role}, {})", cond_text(cond)),
        Pred::Exists { name } => format!("exists({name})"),
        Pred::ExistsQuery { query } => format!("exists({})", query_text(query)),
    }
}

/// A compound child keeps its parentheses (`(a OR b) AND c`); a leaf prints
/// bare (`NOT any(actors, …)`).
fn compound(p: &Pred) -> String {
    match p {
        Pred::And(..) | Pred::Or(..) => format!("({})", pred_text(p)),
        _ => pred_text(p),
    }
}

/// `aspect:secret >= 2` — a scalar condition's authored spelling.
fn cond_text(c: &ScalarCond) -> String {
    format!("{}:{} {} {}", c.path.field.as_str(), c.path.key, c.op.as_str(), c.value)
}

/// One `Cmp` operand: numbers, role paths, the count/total reductions, the
/// dice binding (an IF-condition operand — never in a `when:`).
fn val_expr_text(x: &ValExpr) -> String {
    match x {
        ValExpr::Int { v } => v.to_string(),
        ValExpr::Count { role } => format!("count({role})"),
        ValExpr::CountQuery { query } => format!("count({})", query_text(query)),
        ValExpr::Total { role, path } => format!("total({role}, aspect:{path})"),
        ValExpr::Path { role, field, key } => format!("{role}.{}.{}", field.as_str(), key),
        ValExpr::Param { name } => name.clone(),
        ValExpr::Roll => "roll".to_string(),
    }
}

/// The leftmost clause that read false, rendered — the walk mirrors
/// `eval_pred`'s short-circuit (`&&` stops at the first false arm), so the
/// named clause is the one that decided a no-MATCH. `None` = the predicate
/// held.
fn pred_failure(p: &Pred, ctx: &PredCtx) -> Result<Option<String>> {
    match p {
        Pred::And(a, b) => {
            let a = pred_failure(a, ctx)?;
            match a {
                Some(_) => Ok(a),
                None => pred_failure(b, ctx),
            }
        }
        Pred::Or(a, b) => {
            let a = pred_failure(a, ctx)?;
            match a {
                Some(_) => Ok(a),
                None => pred_failure(b, ctx),
            }
        }
        Pred::Not(a) => {
            if eval_pred(p, ctx)? {
                Ok(None)
            } else {
                Ok(Some(format!("NOT {} holds", compound(a))))
            }
        }
        _ => {
            if eval_pred(p, ctx)? {
                Ok(None)
            } else {
                Ok(Some(pred_text(p)))
            }
        }
    }
}

/// One slot: `actors 0..1 kind:character` — role, cardinality, query text.
fn slot_text(s: &CompiledSlot) -> String {
    let range = match s.max {
        Some(m) => format!("{}..{m}", s.min),
        None => format!("{}..", s.min),
    };
    match &s.query {
        Some(q) => format!("{} {} {}", s.role, range, query_text(q)),
        None => format!("{} {range}", s.role),
    }
}

/// The verb explained: the counts and the outcome first, the slots and the
/// bindings the marks were computed against, then the recipes in evaluation
/// order — every recipe counted, hidden ones marked, each with its `when`
/// text and (for a no-MATCH) the leftmost failing clause. Without `all`,
/// details cap at the first [`DETAIL_LIMIT`] deciding and [`DETAIL_LIMIT`]
/// no-MATCH entries.
pub fn explain_verb(
    verb: &CompiledVerb,
    bindings: &RoleBindings,
    state: &State,
    show_all: bool,
) -> Result<Vec<String>> {
    let ctx = PredCtx { b: bindings, state, strict: false, roll: None };
    let mut fires = 0usize;
    let mut refused = 0usize;
    let mut shadowed = 0usize;
    let mut spent = 0usize;
    let mut no_match = 0usize;
    let mut hidden = 0usize;
    // the §6.4 walk: the first non-spent recipe whose `when` holds decides —
    // a REFUSE among them is the wall, everything matching behind it is
    // shadowed, everything spent is passed over unread
    let mut winner: Option<String> = None;
    let mut deciding_shown = 0usize;
    let mut failing_shown = 0usize;
    let mut body: Vec<String> = Vec::new();
    for (i, r) in verb.recipes.iter().enumerate() {
        if !r.shown {
            hidden += 1;
        }
        let flags = format!(
            "{}{}{}",
            if r.shown { "" } else { " · hidden" },
            if r.out.iter().any(|o| o.op == "REFUSE") { " · REFUSE" } else { "" },
            r.max.map(|m| format!(" · max {m}")).unwrap_or_default(),
        );
        let when_line = match &r.when {
            Some(w) => format!("     when {}", pred_text(w)),
            None => "     when — the default (no when:)".to_string(),
        };
        let is_spent = r.max.is_some_and(|m| state.executed.get(&r.id).is_some_and(|&n| n >= m));
        let (mark, extra) = if is_spent {
            spent += 1;
            ("spent", None)
        } else {
            let holds = match &r.when {
                None => true,
                Some(w) => eval_pred(w, &ctx)?,
            };
            match holds {
                false => {
                    no_match += 1;
                    let when = match &r.when {
                        Some(w) => w,
                        None => unreachable!("a false `holds` implies a `when`"),
                    };
                    ("no-match", pred_failure(when, &ctx)?.map(|f| format!("     fails: {f}")))
                }
                true if winner.is_none() => {
                    winner = Some(r.id.clone());
                    if r.out.iter().any(|o| o.op == "REFUSE") {
                        refused += 1;
                        ("refused", None)
                    } else {
                        fires += 1;
                        ("fires", None)
                    }
                }
                true => {
                    shadowed += 1;
                    let w = winner.clone().unwrap_or_default();
                    ("shadowed", Some(format!("     would match, but {w} decides first")))
                }
            }
        };
        let deciding = mark != "no-match";
        let shown =
            deciding && deciding_shown < DETAIL_LIMIT || !deciding && failing_shown < DETAIL_LIMIT;
        if show_all || shown {
            if deciding {
                deciding_shown += 1;
            } else {
                failing_shown += 1;
            }
            body.push(format!("{:>3}. {:<9}{}{}", i + 1, mark, r.id, flags));
            body.push(when_line);
            if let Some(extra) = extra {
                body.push(extra);
            }
        }
    }
    let mut lines = vec![
        format!(
            "{}: {} recipes · {fires} fires · {refused} refused · {shadowed} shadowed · \
             {spent} spent · {no_match} no-match{}",
            display_name(&verb.name),
            verb.recipes.len(),
            if hidden > 0 { format!(" ({hidden} hidden)") } else { String::new() },
        ),
        match &winner {
            Some(id) if refused > 0 => format!(
                "outcome: REFUSED at the table by {id} — the attempt journals, no window opens"
            ),
            Some(id) => format!("outcome: {id} fires"),
            None => "outcome: no recipe matched — the engine rejects the step".to_string(),
        },
    ];
    let slots = verb.slots.iter().map(slot_text).collect::<Vec<_>>().join(" · ");
    lines.push(if slots.is_empty() {
        "slots: none".to_string()
    } else {
        format!("slots: {slots}")
    });
    let bound = bindings_line(bindings, state);
    lines.push(if bound.is_empty() {
        "bindings: none".to_string()
    } else {
        format!("bindings: {bound}")
    });
    lines.extend(body);
    let shown_total = deciding_shown + failing_shown;
    if !show_all && shown_total < verb.recipes.len() {
        lines.push(format!(
            "({shown_total} of {} recipes shown — the first {DETAIL_LIMIT} deciding + \
             {DETAIL_LIMIT} no-match; `explain {} all` lists every recipe)",
            verb.recipes.len(),
            display_name(&verb.name),
        ));
    }
    Ok(lines)
}

/// The roles the marks were computed against, display names per id — the
/// same `role NAMES` reading the try row's phrases speak.
fn bindings_line(bindings: &RoleBindings, state: &State) -> String {
    ROLES
        .iter()
        .filter_map(|role| {
            let ids = bindings.get(role).filter(|v| !v.is_empty())?;
            let names = ids
                .iter()
                .map(|id| match state.entities.get(id) {
                    Some(e) => pretty(&e.name),
                    None => format!("#{id}"),
                })
                .collect::<Vec<_>>()
                .join(", ");
            Some(format!("{} {names}", role.as_str()))
        })
        .collect::<Vec<_>>()
        .join(" · ")
}
