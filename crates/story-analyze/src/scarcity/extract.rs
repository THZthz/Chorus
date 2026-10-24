//! Goal and clause extraction: compiles goal predicates into scarcity clauses.

use story_content::ir::{
    AspectTerm, CmpOp, EffectNode, Expr, LogicOp, QueryTerm, RecipeDecl, VerbMode,
};
use story_content::Content;
use story_core::{CardId, EndingId};

use super::helpers::{is_unique_card, visit_effects, visit_non_recipe_books};
use super::model::{Bound, Clause};

// ---------------------------------------------------------------------------
// Goal & clause extraction
// ---------------------------------------------------------------------------

pub(super) fn extract_goal_clauses(
    goal: &crate::query::GoalPredicate,
    content: &Content,
) -> Vec<Clause> {
    use crate::query::GoalPredicate;
    match goal {
        GoalPredicate::Expr(e) => expr_clauses(e, false, content),
        GoalPredicate::Ending(Some(id)) => ending_clauses(id, content),
        GoalPredicate::CardMatch {
            prototype: Some(card),
            terms,
            min_count,
            ..
        } if *min_count >= 1 && is_unique_card(content, card) => {
            let mut bounds = Vec::new();
            for t in terms {
                if let QueryTerm::Aspect(a) = t {
                    push_aspect_term_bounds(card, a, &mut bounds);
                }
            }
            if bounds.is_empty() {
                Vec::new()
            } else {
                vec![Clause {
                    complete_bounds: bounds,
                    ..Clause::default()
                }]
            }
        }
        GoalPredicate::PartialState(ps) => {
            let mut bounds = Vec::new();
            let mut legacy_gates = Vec::new();
            for (card, key, val) in &ps.aspects {
                if is_unique_card(content, card) {
                    bounds.push(Bound::Low {
                        card: card.clone(),
                        key: key.clone(),
                        n: *val,
                    });
                    bounds.push(Bound::High {
                        card: card.clone(),
                        key: key.clone(),
                        n: *val,
                    });
                }
            }
            for (_, proto, terms) in &ps.cards {
                if let Some(card) = proto {
                    if is_unique_card(content, card) {
                        for t in terms {
                            if let QueryTerm::Aspect(a) = t {
                                push_aspect_term_bounds(card, a, &mut bounds);
                            }
                        }
                    }
                }
            }
            for (lkey, val) in &ps.legacy {
                legacy_gates.push((lkey.clone(), CmpOp::Eq, *val));
            }
            if bounds.is_empty() && legacy_gates.is_empty() {
                Vec::new()
            } else {
                vec![Clause {
                    complete_bounds: bounds,
                    legacy_gates,
                    ..Clause::default()
                }]
            }
        }
        GoalPredicate::And(ps) => ps
            .iter()
            .map(|p| extract_goal_clauses(p, content))
            .fold(Vec::new(), combine_and_clauses),
        GoalPredicate::Or(ps) => {
            let mut out = Vec::new();
            for p in ps {
                let sub = extract_goal_clauses(p, content);
                if sub.is_empty() {
                    return Vec::new();
                }
                out.extend(sub);
            }
            out
        }
        GoalPredicate::Not(inner) => match inner.as_ref() {
            GoalPredicate::Expr(e) => expr_clauses(e, true, content),
            _ => Vec::new(),
        },
        _ => Vec::new(),
    }
}

fn push_aspect_term_bounds(card: &CardId, a: &AspectTerm, out: &mut Vec<Bound>) {
    let (card, key) = (card.clone(), a.key.clone());
    match a.op {
        None => out.push(Bound::Low { card, key, n: 1 }),
        Some(CmpOp::Ge) => out.push(Bound::Low {
            card,
            key,
            n: a.value,
        }),
        Some(CmpOp::Gt) => out.push(Bound::Low {
            card,
            key,
            n: a.value.saturating_add(1),
        }),
        Some(CmpOp::Le) => out.push(Bound::High {
            card,
            key,
            n: a.value,
        }),
        Some(CmpOp::Lt) => out.push(Bound::High {
            card,
            key,
            n: a.value.saturating_sub(1),
        }),
        Some(CmpOp::Eq) => {
            out.push(Bound::Low {
                card: card.clone(),
                key: key.clone(),
                n: a.value,
            });
            out.push(Bound::High {
                card,
                key,
                n: a.value,
            });
        }
        Some(CmpOp::Ne) => {}
    }
}

fn combine_and_clauses(a: Vec<Clause>, b: Vec<Clause>) -> Vec<Clause> {
    if a.is_empty() {
        return b;
    }
    if b.is_empty() {
        return a;
    }
    let mut out = Vec::new();
    for ca in &a {
        for cb in &b {
            if let (Some(ra), Some(rb)) = (&ca.recipe, &cb.recipe) {
                if ra != rb {
                    continue;
                }
            }
            let mut merged = ca.clone();
            if merged.recipe.is_none() {
                merged.recipe = cb.recipe.clone();
            }
            merged.min_open_clock = merged.min_open_clock.max(cb.min_open_clock);
            merged.open_bounds.extend(cb.open_bounds.iter().cloned());
            merged
                .complete_bounds
                .extend(cb.complete_bounds.iter().cloned());
            merged.legacy_gates.extend(cb.legacy_gates.iter().cloned());
            out.push(merged);
        }
    }
    out
}

fn expr_clauses(e: &Expr, negated: bool, content: &Content) -> Vec<Clause> {
    match e {
        Expr::Not(inner) => expr_clauses(inner, !negated, content),
        Expr::Logic { op, left, right } => {
            let is_and = matches!((op, negated), (LogicOp::And, false) | (LogicOp::Or, true));
            let l = expr_clauses(left, negated, content);
            let r = expr_clauses(right, negated, content);
            if is_and {
                combine_and_clauses(l, r)
            } else if l.is_empty() || r.is_empty() {
                Vec::new()
            } else {
                l.into_iter().chain(r).collect()
            }
        }
        Expr::Cmp { op, left, right } => {
            let eff_op = if negated { negate_cmp(*op) } else { *op };
            let mut c = Clause::default();
            extract_cmp_into(eff_op, left, right, content, &mut c, false);
            if c.complete_bounds.is_empty() && c.legacy_gates.is_empty() {
                Vec::new()
            } else {
                vec![c]
            }
        }
        _ => Vec::new(),
    }
}

fn negate_cmp(op: CmpOp) -> CmpOp {
    match op {
        CmpOp::Ge => CmpOp::Lt,
        CmpOp::Gt => CmpOp::Le,
        CmpOp::Le => CmpOp::Gt,
        CmpOp::Lt => CmpOp::Ge,
        CmpOp::Eq => CmpOp::Ne,
        CmpOp::Ne => CmpOp::Eq,
    }
}

fn flip_cmp(op: CmpOp) -> CmpOp {
    match op {
        CmpOp::Ge => CmpOp::Le,
        CmpOp::Gt => CmpOp::Lt,
        CmpOp::Le => CmpOp::Ge,
        CmpOp::Lt => CmpOp::Gt,
        CmpOp::Eq => CmpOp::Eq,
        CmpOp::Ne => CmpOp::Ne,
    }
}

fn extract_cmp_into(
    op: CmpOp,
    left: &Expr,
    right: &Expr,
    content: &Content,
    clause: &mut Clause,
    is_open: bool,
) {
    let (lhs, eff_op, n) = match (left, right) {
        (_, Expr::Lit(n)) => (left, op, *n),
        (Expr::Lit(n), _) => (right, flip_cmp(op), *n),
        _ => return,
    };
    let target_vec = if is_open {
        &mut clause.open_bounds
    } else {
        &mut clause.complete_bounds
    };
    match lhs {
        Expr::NamePath { card, key } if is_unique_card(content, card) => {
            let (card, key) = (card.clone(), key.clone());
            match eff_op {
                CmpOp::Ge => target_vec.push(Bound::Low { card, key, n }),
                CmpOp::Gt => target_vec.push(Bound::Low {
                    card,
                    key,
                    n: n.saturating_add(1),
                }),
                CmpOp::Le => target_vec.push(Bound::High { card, key, n }),
                CmpOp::Lt => target_vec.push(Bound::High {
                    card,
                    key,
                    n: n.saturating_sub(1),
                }),
                CmpOp::Eq => {
                    target_vec.push(Bound::Low {
                        card: card.clone(),
                        key: key.clone(),
                        n,
                    });
                    target_vec.push(Bound::High { card, key, n });
                }
                CmpOp::Ne => {}
            }
        }
        Expr::Legacy(lkey) => clause.legacy_gates.push((lkey.clone(), eff_op, n)),
        Expr::Clock if is_open => match eff_op {
            CmpOp::Ge | CmpOp::Eq => clause.min_open_clock = clause.min_open_clock.max(n),
            CmpOp::Gt => clause.min_open_clock = clause.min_open_clock.max(n.saturating_add(1)),
            _ => {}
        },
        _ => {}
    }
}

pub(super) fn walk_conjunctive_cond(
    e: &Expr,
    negated: bool,
    content: &Content,
    clause: &mut Clause,
    is_open: bool,
) {
    match e {
        Expr::Not(inner) => walk_conjunctive_cond(inner, !negated, content, clause, is_open),
        Expr::Logic { op, left, right } => {
            if matches!((op, negated), (LogicOp::And, false) | (LogicOp::Or, true)) {
                walk_conjunctive_cond(left, negated, content, clause, is_open);
                walk_conjunctive_cond(right, negated, content, clause, is_open);
            }
        }
        Expr::Cmp { op, left, right } => {
            let eff_op = if negated { negate_cmp(*op) } else { *op };
            extract_cmp_into(eff_op, left, right, content, clause, is_open);
        }
        _ => {}
    }
}

fn ending_clauses(id: &EndingId, content: &Content) -> Vec<Clause> {
    let mut non_recipe_reaches = false;
    visit_non_recipe_books(content, &mut |b, _| {
        if b.rules.iter().any(|r| effects_reach_ending(&r.effects, id)) {
            non_recipe_reaches = true;
        }
    });
    if non_recipe_reaches {
        return Vec::new();
    }

    let mut clauses = Vec::new();
    for verb in content.verbs.values() {
        let mut recipes: Vec<&RecipeDecl> = verb.recipes.values().collect();
        recipes.sort_by_key(|r| (r.priority, r.decl_index));

        for (idx, rec) in recipes.iter().enumerate() {
            let mut paths: Vec<Vec<(Expr, bool)>> = Vec::new();
            let mut cur_path: Vec<(Expr, bool)> = Vec::new();
            collect_ending_paths(&rec.effects, id, &mut cur_path, &mut paths);
            if paths.is_empty() {
                continue;
            }

            let mut base = Clause {
                recipe: Some(rec.id.clone()),
                ..Clause::default()
            };
            if let Some(w) = &rec.when {
                walk_conjunctive_cond(w, false, content, &mut base, true);
            }
            for g in &rec.guards {
                walk_conjunctive_cond(&g.when, true, content, &mut base, true);
            }
            for earlier in &recipes[..idx] {
                if (verb.mode == VerbMode::Autonomous || earlier.craftable)
                    && rec.slots.is_empty()
                    && earlier.slots.is_empty()
                    && earlier.max.is_none()
                    && earlier.guards.is_empty()
                    && !effects_reach_ending(&earlier.effects, id)
                {
                    if let Some(w) = &earlier.when {
                        walk_conjunctive_cond(w, true, content, &mut base, true);
                    }
                }
            }

            for path in paths {
                let mut c = base.clone();
                for (cond, neg) in path {
                    walk_conjunctive_cond(&cond, neg, content, &mut c, false);
                }
                clauses.push(c);
            }
        }
    }
    clauses
}

fn effects_reach_ending(effects: &[EffectNode], id: &EndingId) -> bool {
    let mut hit = false;
    visit_effects(effects, &mut |n| {
        if matches!(n, EffectNode::Ending { id: end } if end == id) {
            hit = true;
        }
    });
    hit
}

fn collect_ending_paths(
    effects: &[EffectNode],
    id: &EndingId,
    cur: &mut Vec<(Expr, bool)>,
    out: &mut Vec<Vec<(Expr, bool)>>,
) {
    for e in effects {
        match e {
            EffectNode::Ending { id: end } => {
                if end == id {
                    out.push(cur.clone());
                }
                return;
            }
            EffectNode::Branch { cond, then, els } => {
                cur.push((cond.clone(), false));
                collect_ending_paths(then, id, cur, out);
                cur.pop();

                cur.push((cond.clone(), true));
                collect_ending_paths(els, id, cur, out);
                cur.pop();
            }
            _ => {}
        }
    }
}
