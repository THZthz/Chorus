//! State and content helpers shared by the scarcity analysis.

use std::collections::BTreeSet;

use story_content::ir::{
    self, AspectKey, Book, CmpOp, EffectNode, Expr, InstanceCap, Into, LegacyKey, LogicOp,
    QueryTerm,
};
use story_content::Content;
use story_core::{CardId, EndingId, SituationId, State, ZoneKind};

use crate::causal::visit_content_effects;

use super::model::{Clause, GainModel};

// ---------------------------------------------------------------------------
// State & content helpers
// ---------------------------------------------------------------------------

pub(super) fn visit_effects(effects: &[EffectNode], f: &mut impl FnMut(&EffectNode)) {
    let mut stack: Vec<&EffectNode> = effects.iter().collect();
    while let Some(e) = stack.pop() {
        f(e);
        if let EffectNode::Branch { then, els, .. } = e {
            stack.extend(then.iter().chain(els.iter()));
        }
    }
}

pub(super) fn visit_non_recipe_books(content: &Content, f: &mut impl FnMut(&Book, Option<&CardId>)) {
    for v in content.verbs.values() {
        for r in v.recipes.values() {
            if let Some(b) = &r.on_complete {
                f(b, None);
            }
        }
    }
    for c in content.cards.values() {
        for (_, b) in &c.on_complete {
            f(b, Some(&c.id));
        }
        if let Some(b) = &c.on_expire {
            f(b, Some(&c.id));
        }
    }
    for b in content.events.values() {
        f(b, None);
    }
}

pub(super) fn is_unique_card(content: &Content, card: &CardId) -> bool {
    content.card(card).is_some_and(|d| {
        matches!(
            d.mint,
            story_core::MintPolicy::Unique | story_core::MintPolicy::SingleCopy
        )
    })
}

pub(super) fn read_card_aspect(state: &State, content: &Content, card: &CardId, key: &AspectKey) -> i64 {
    state
        .entities
        .values()
        .find(|e| &e.prototype == card)
        .and_then(|e| e.aspects.get(key))
        .copied()
        .unwrap_or_else(|| content.aspects.get(key).map(|a| a.default).unwrap_or(0))
}

pub(super) fn aspect_stock(
    state: &State,
    key: &AspectKey,
    exclude_card: Option<&CardId>,
    zone_only: bool,
) -> i64 {
    state
        .entities
        .values()
        .filter(|e| {
            exclude_card != Some(&e.prototype)
                && (!zone_only
                    || state
                        .zones
                        .get(&e.zone)
                        .is_some_and(|z| !matches!(z.kind, ZoneKind::Of(_))))
        })
        .map(|e| e.aspects.get(key).copied().unwrap_or(0).max(0))
        .sum()
}

pub(super) fn verb_free_at(state: &State, verb: &ir::VerbDecl) -> i64 {
    let InstanceCap::AtMost(n) = verb.instances else {
        return state.now;
    };
    let mut ends: Vec<i64> = state
        .situations
        .values()
        .filter(|s| s.verb == verb.id)
        .map(|s| s.ends_at)
        .collect();
    if ends.len() < n as usize {
        state.now
    } else {
        ends.sort_unstable();
        ends.first().copied().unwrap_or(state.now).max(state.now)
    }
}

pub(super) fn earliest_feeder_finish(rmodel: &GainModel, state: &State, content: &Content) -> Option<i64> {
    if rmodel.unbounded {
        return Some(state.now);
    }
    let mut best: Option<i64> = None;
    for r in &rmodel.routes {
        for sit in state.situations.values() {
            if sit.recipe == r.cap.recipe {
                best = Some(best.map_or(sit.ends_at, |b| b.min(sit.ends_at)));
            }
        }
        let vfree = content
            .verbs
            .get(r.cap.verb.as_str())
            .map(|v| verb_free_at(state, v))
            .unwrap_or(state.now);
        let finish = state.now.max(vfree).saturating_add(r.cap.duration);
        best = Some(best.map_or(finish, |b| b.min(finish)));
    }
    best
}

pub(super) fn content_moves_into_zone(content: &Content) -> bool {
    let mut hit = false;
    visit_content_effects(content, &mut |n| {
        if matches!(
            n,
            EffectNode::Move {
                into: Into::Zone(_),
                ..
            }
        ) {
            hit = true;
        }
    });
    hit
}

pub(super) fn non_terminal_unwritten_legacies(content: &Content) -> BTreeSet<LegacyKey> {
    let mut written: BTreeSet<LegacyKey> = BTreeSet::new();
    let mut record_non_terminal = |effects: &[EffectNode]| {
        if guaranteed_endings(effects).is_none() {
            visit_effects(effects, &mut |n| {
                if let EffectNode::Legacy { key, .. } = n {
                    written.insert(key.clone());
                }
            });
        }
    };
    for v in content.verbs.values() {
        for r in v.recipes.values() {
            record_non_terminal(&r.effects);
        }
    }
    visit_non_recipe_books(content, &mut |b, _| {
        for r in &b.rules {
            record_non_terminal(&r.effects);
        }
    });
    content
        .legacies
        .keys()
        .filter(|k| !written.contains(*k))
        .cloned()
        .collect()
}

pub(super) fn guaranteed_endings(effects: &[EffectNode]) -> Option<BTreeSet<EndingId>> {
    for e in effects {
        match e {
            EffectNode::Ending { id } => return Some(BTreeSet::from([id.clone()])),
            EffectNode::Branch { then, els, .. } => {
                if let (Some(mut a), Some(b)) = (guaranteed_endings(then), guaranteed_endings(els))
                {
                    a.extend(b);
                    return Some(a);
                }
            }
            EffectNode::Consume { .. }
            | EffectNode::Spawn { .. }
            | EffectNode::Swap { .. }
            | EffectNode::Move { .. }
            | EffectNode::Roll { .. }
            | EffectNode::Draw { .. }
            | EffectNode::Continue { .. }
            | EffectNode::Op { .. } => return None,
            _ => {}
        }
    }
    None
}

pub(super) fn earliest_in_flight_ending(
    state: &State,
    content: &Content,
) -> Option<(i64, SituationId, BTreeSet<EndingId>)> {
    let mut best: Option<(i64, SituationId, BTreeSet<EndingId>)> = None;
    for sit in state.situations.values() {
        let Some(rec) = content.recipe(sit.recipe.as_str()) else {
            continue;
        };
        let Some(endings) = guaranteed_endings(&rec.effects) else {
            continue;
        };
        match &best {
            Some((bt, bs, _)) if (*bt, *bs) <= (sit.ends_at, sit.id) => {}
            _ => best = Some((sit.ends_at, sit.id, endings)),
        }
    }
    best
}

pub(super) fn clause_can_beat_ending(
    clause: &Clause,
    state: &State,
    content: &Content,
    ending_cap: (i64, SituationId),
) -> bool {
    let Some(rec_id) = &clause.recipe else {
        return true;
    };
    if let Some(sit) = state.situations.values().find(|s| &s.recipe == rec_id) {
        return (sit.ends_at, sit.id) < ending_cap;
    }
    let Some(rec) = content.recipe(rec_id.as_str()) else {
        return true;
    };
    let verb_free = content
        .verbs
        .values()
        .find(|v| v.recipes.contains_key(rec_id.as_str()))
        .map(|v| verb_free_at(state, v))
        .unwrap_or(state.now);
    state
        .now
        .max(clause.min_open_clock)
        .max(verb_free)
        .saturating_add(rec.duration)
        < ending_cap.0
}

pub(super) fn collect_when_required_cards(e: &Expr, out: &mut Vec<CardId>) {
    match e {
        Expr::Logic {
            op: LogicOp::And,
            left,
            right,
        } => {
            collect_when_required_cards(left, out);
            collect_when_required_cards(right, out);
        }
        Expr::Cmp {
            op: op @ (CmpOp::Ge | CmpOp::Gt | CmpOp::Eq),
            left,
            right,
        } => {
            if let Expr::Lit(n) = right.as_ref() {
                if (if matches!(op, CmpOp::Gt) { *n + 1 } else { *n }) >= 1 {
                    match left.as_ref() {
                        Expr::Alive(c) => out.push(c.clone()),
                        Expr::Count { query, .. } => {
                            for t in query {
                                if let QueryTerm::Name(c) = t {
                                    out.push(c.clone());
                                }
                            }
                        }
                        _ => {}
                    }
                }
            }
        }
        _ => {}
    }
}

pub(super) fn cmp_eval(op: CmpOp, lhs: i64, rhs: i64) -> bool {
    match op {
        CmpOp::Ge => lhs >= rhs,
        CmpOp::Gt => lhs > rhs,
        CmpOp::Le => lhs <= rhs,
        CmpOp::Lt => lhs < rhs,
        CmpOp::Eq => lhs == rhs,
        CmpOp::Ne => lhs != rhs,
    }
}
