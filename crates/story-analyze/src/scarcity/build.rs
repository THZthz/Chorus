//! Gain and drop model construction from recipe effect lists.

use std::collections::BTreeSet;

use story_content::ir::{
    self, ArithOp, AspectKey, CmpOp, EffectNode, Expr, InstanceCap, Into, LegacyKey, QueryTerm,
    RecipeDecl, SetOp, Target, VerbDecl, VerbMode,
};
use story_content::Content;
use story_core::{CardId, SituationId, State};

use crate::causal::{visit_content_effects, MatchOracle};

use super::extract::walk_conjunctive_cond;
use super::helpers::{
    collect_when_required_cards, content_moves_into_zone, guaranteed_endings, is_unique_card,
    visit_effects, visit_non_recipe_books,
};
use super::model::{
    AutonomousDrain, Bound, Clause, DropModel, DropRoute, GainModel, GainRoute, GainTerm, RouteCap,
};

// ---------------------------------------------------------------------------
// Gain & Drop model construction
// ---------------------------------------------------------------------------

pub(super) fn build_gain_model(
    card: &CardId,
    key: &AspectKey,
    content: &Content,
    oracle: &MatchOracle,
    recipe_mintable: &BTreeSet<CardId>,
    dealt: &BTreeSet<CardId>,
    static_legacies: &BTreeSet<LegacyKey>,
) -> GainModel {
    if !is_unique_card(content, card) || non_recipe_writes_aspect(content, oracle, card, key, true)
    {
        return GainModel {
            unbounded: true,
            routes: Vec::new(),
            drains: Vec::new(),
        };
    }

    let any_move_to_zone = content_moves_into_zone(content);
    let mut routes = Vec::new();

    for verb in content.verbs.values() {
        for rec in verb.recipes.values() {
            if recipe_has_unbounded_write(content, oracle, rec, card, key, true) {
                return GainModel {
                    unbounded: true,
                    routes: Vec::new(),
                    drains: Vec::new(),
                };
            }
            let raw_gains = collect_set_ops(&rec.effects, card, key, SetOp::Add);
            if raw_gains.is_empty() || guaranteed_endings(&rec.effects).is_some() {
                continue;
            }

            let mut terms = Vec::new();
            for expr in raw_gains {
                let Some(mut t) = parse_gain_expr(
                    expr,
                    rec,
                    content,
                    oracle,
                    recipe_mintable,
                    dealt,
                    any_move_to_zone,
                ) else {
                    return GainModel {
                        unbounded: true,
                        routes: Vec::new(),
                        drains: Vec::new(),
                    };
                };
                terms.append(&mut t);
            }
            if terms.is_empty() {
                continue;
            }

            let Some(cap) = build_route_cap(
                verb,
                rec,
                card,
                key,
                content,
                recipe_mintable,
                dealt,
                static_legacies,
                any_move_to_zone,
            ) else {
                return GainModel {
                    unbounded: true,
                    routes: Vec::new(),
                    drains: Vec::new(),
                };
            };

            routes.push(GainRoute { cap, terms });
        }
    }

    GainModel {
        unbounded: false,
        routes,
        drains: find_autonomous_drains(card, key, content),
    }
}

pub(super) fn build_drop_model(
    card: &CardId,
    key: &AspectKey,
    content: &Content,
    oracle: &MatchOracle,
    recipe_mintable: &BTreeSet<CardId>,
    dealt: &BTreeSet<CardId>,
    static_legacies: &BTreeSet<LegacyKey>,
) -> DropModel {
    if !is_unique_card(content, card) || non_recipe_writes_aspect(content, oracle, card, key, false)
    {
        return DropModel {
            unbounded: true,
            routes: Vec::new(),
        };
    }

    let any_move_to_zone = content_moves_into_zone(content);
    let mut routes = Vec::new();

    for verb in content.verbs.values() {
        for rec in verb.recipes.values() {
            if recipe_has_unbounded_write(content, oracle, rec, card, key, false) {
                return DropModel {
                    unbounded: true,
                    routes: Vec::new(),
                };
            }
            let subs = collect_set_ops(&rec.effects, card, key, SetOp::Sub);
            if subs.is_empty() || guaranteed_endings(&rec.effects).is_some() {
                continue;
            }
            let mut drop_per_run = 0i64;
            for expr in subs {
                match expr {
                    Expr::Lit(n) if *n > 0 => drop_per_run = drop_per_run.saturating_add(*n),
                    Expr::Lit(_) => {}
                    _ => {
                        return DropModel {
                            unbounded: true,
                            routes: Vec::new(),
                        };
                    }
                }
            }
            if drop_per_run <= 0 {
                continue;
            }

            let Some(cap) = build_route_cap(
                verb,
                rec,
                card,
                key,
                content,
                recipe_mintable,
                dealt,
                static_legacies,
                any_move_to_zone,
            ) else {
                return DropModel {
                    unbounded: true,
                    routes: Vec::new(),
                };
            };

            routes.push(DropRoute { cap, drop_per_run });
        }
    }

    DropModel {
        unbounded: false,
        routes,
    }
}

fn build_route_cap(
    verb: &VerbDecl,
    rec: &RecipeDecl,
    card: &CardId,
    key: &AspectKey,
    content: &Content,
    recipe_mintable: &BTreeSet<CardId>,
    dealt: &BTreeSet<CardId>,
    static_legacies: &BTreeSet<LegacyKey>,
    any_move_to_zone: bool,
) -> Option<RouteCap> {
    let mut consumed = Vec::new();
    if !collect_top_level_consumed(&rec.effects, &mut consumed) {
        return None;
    }
    for sd in &rec.slots {
        if sd.consuming && sd.min >= 1 {
            for k in slot_required_aspects(content, &sd.query) {
                consumed.push((k, sd.min as i64));
            }
        }
    }
    consumed.retain(|(k, _)| resource_bounded(k, None, content, recipe_mintable, dealt));

    let mut carrier_moved = Vec::new();
    if !any_move_to_zone {
        for node in &rec.effects {
            if let EffectNode::Move {
                target: Target::Slot(sname),
                into: Into::Carrier(carrier_card),
            } = node
            {
                if is_unique_card(content, carrier_card) {
                    if let Some(sd) = rec.slots.iter().find(|s| &s.name == sname) {
                        if sd.min >= 1 {
                            for k in slot_required_aspects(content, &sd.query) {
                                if resource_bounded(
                                    &k,
                                    Some(carrier_card),
                                    content,
                                    recipe_mintable,
                                    dealt,
                                ) {
                                    carrier_moved.push((carrier_card.clone(), k, sd.min as i64));
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    let mut reservoirs = Vec::new();
    let mut static_legacy_low = Vec::new();
    let mut required_live = Vec::new();
    if let Some(w) = &rec.when {
        collect_when_required_cards(w, &mut required_live);
        let mut temp = Clause::default();
        walk_conjunctive_cond(w, false, content, &mut temp, true);
        for (lkey, op, val) in temp.legacy_gates {
            if static_legacies.contains(&lkey) {
                match op {
                    CmpOp::Ge | CmpOp::Eq => static_legacy_low.push((lkey, val)),
                    CmpOp::Gt => static_legacy_low.push((lkey, val.saturating_add(1))),
                    _ => {}
                }
            }
        }
        for b in temp.open_bounds {
            if let Bound::Low {
                card: rc,
                key: rk,
                n: need,
            } = b
            {
                if (&rc, &rk) != (card, key) {
                    let sub = top_level_sub(&rec.effects, &rc, &rk);
                    if sub >= 1 && need >= sub {
                        reservoirs.push((rc, rk, sub));
                    }
                }
            }
        }
    }

    let verb_instances = match verb.instances {
        InstanceCap::AtMost(n) => Some(n),
        InstanceCap::Any => None,
    };

    Some(RouteCap {
        recipe: rec.id.clone(),
        verb: verb.id.clone(),
        verb_instances,
        duration: rec.duration,
        max_runs: rec.max,
        static_legacy_low,
        consumed,
        carrier_moved,
        reservoirs,
        required_live,
    })
}

fn parse_gain_expr(
    expr: &Expr,
    rec: &RecipeDecl,
    content: &Content,
    oracle: &MatchOracle,
    recipe_mintable: &BTreeSet<CardId>,
    dealt: &BTreeSet<CardId>,
    any_move_to_zone: bool,
) -> Option<Vec<GainTerm>> {
    match expr {
        Expr::Lit(g) => Some(if *g <= 0 {
            Vec::new()
        } else {
            vec![GainTerm::Literal(*g)]
        }),
        Expr::Aspect {
            scope: ir::Scope::Slot(sname),
            key,
            ..
        } => {
            let sd = rec.slots.iter().find(|s| &s.name == sname)?;
            let mut moved_carrier: Option<&CardId> = None;
            if !any_move_to_zone {
                for node in &rec.effects {
                    if let EffectNode::Move {
                        target: Target::Slot(ms),
                        into: Into::Carrier(carrier),
                    } = node
                    {
                        if ms == sname && is_unique_card(content, carrier) {
                            moved_carrier = Some(carrier);
                            break;
                        }
                    }
                }
            }
            let per_card_max =
                max_card_aspect_for_slot(content, oracle, &sd.query, key, moved_carrier)?;
            let per_run_cap = sd.max.map(|m| (m as i64).saturating_mul(per_card_max));

            if sd.consuming && resource_bounded(key, None, content, recipe_mintable, dealt) {
                return Some(vec![GainTerm::Stock {
                    key: key.clone(),
                    per_run_cap,
                    exclude_card: None,
                    zone_only: false,
                }]);
            }

            if let Some(carrier) = moved_carrier {
                if resource_bounded(key, Some(carrier), content, recipe_mintable, dealt) {
                    return Some(vec![GainTerm::Stock {
                        key: key.clone(),
                        per_run_cap,
                        exclude_card: Some(carrier.clone()),
                        zone_only: true,
                    }]);
                }
            }

            per_run_cap.map(|cap| vec![GainTerm::Literal(cap)])
        }
        Expr::Arith {
            op: ArithOp::Add,
            left,
            right,
        } => {
            let mut l = parse_gain_expr(
                left,
                rec,
                content,
                oracle,
                recipe_mintable,
                dealt,
                any_move_to_zone,
            )?;
            let mut r = parse_gain_expr(
                right,
                rec,
                content,
                oracle,
                recipe_mintable,
                dealt,
                any_move_to_zone,
            )?;
            l.append(&mut r);
            Some(l)
        }
        _ => None,
    }
}

fn max_card_aspect_for_slot(
    content: &Content,
    oracle: &MatchOracle,
    query: &[QueryTerm],
    key: &AspectKey,
    exclude_card: Option<&CardId>,
) -> Option<i64> {
    let mut key_increased = false;
    visit_content_effects(content, &mut |n| {
        if let EffectNode::Set {
            target,
            key: k,
            op: SetOp::Add | SetOp::Assign,
            ..
        } = n
        {
            if k == key
                && !matches!(target, Target::Name { card, .. } if exclude_card == Some(card))
            {
                key_increased = true;
            }
        }
    });
    if key_increased {
        return None;
    }
    let default = content.aspects.get(key).map(|a| a.default).unwrap_or(0);
    let mut max_v = 0i64;
    for card in oracle.can_match_cards(content, query) {
        if exclude_card == Some(&card) {
            continue;
        }
        if let Some(decl) = content.card(&card) {
            max_v = max_v.max(decl.aspects.get(key).copied().unwrap_or(default));
        }
    }
    Some(max_v)
}

fn slot_required_aspects(content: &Content, query: &[QueryTerm]) -> Vec<AspectKey> {
    let mut out = Vec::new();
    let default_ok = |key: &str, low: i64| {
        low >= 1 && content.aspects.get(key).map(|a| a.default).unwrap_or(0) == 0
    };
    for t in query {
        match t {
            QueryTerm::Aspect(a) => {
                let low = match a.op {
                    None => 1,
                    Some(CmpOp::Ge | CmpOp::Eq) => a.value,
                    Some(CmpOp::Gt) => a.value.saturating_add(1),
                    _ => 0,
                };
                if default_ok(&a.key, low) {
                    out.push(a.key.clone());
                }
            }
            QueryTerm::Pred(expr) => {
                if let Expr::Cmp { op, left, right } = expr.as_ref() {
                    if let (Expr::Subject(key), Expr::Lit(n)) = (left.as_ref(), right.as_ref()) {
                        let low = match op {
                            CmpOp::Ge | CmpOp::Eq => *n,
                            CmpOp::Gt => n.saturating_add(1),
                            _ => 0,
                        };
                        if default_ok(key, low) {
                            out.push(key.clone());
                        }
                    }
                }
            }
            _ => {}
        }
    }
    out
}

fn collect_top_level_consumed(effects: &[EffectNode], out: &mut Vec<(AspectKey, i64)>) -> bool {
    for e in effects {
        if let EffectNode::Consume { target, count } = e {
            let terms = match target {
                Target::Query { terms, .. } | Target::BareQuery(terms) => terms,
                _ => continue,
            };
            let Expr::Lit(n) = count else { return false };
            if *n <= 0 {
                return false;
            }
            for term in terms {
                if let QueryTerm::Aspect(a) = term {
                    out.push((a.key.clone(), *n));
                }
            }
        }
    }
    true
}

fn top_level_sub(effects: &[EffectNode], card: &CardId, key: &AspectKey) -> i64 {
    let mut total = 0i64;
    for e in effects {
        if let EffectNode::Set {
            target: Target::Name { card: c, .. },
            key: k,
            op: SetOp::Sub,
            expr: Expr::Lit(n),
            ..
        } = e
        {
            if c == card && k == key && *n > 0 {
                total = total.saturating_add(*n);
            }
        }
    }
    total
}

fn collect_set_ops<'a>(
    effects: &'a [EffectNode],
    card: &CardId,
    key: &AspectKey,
    want_op: SetOp,
) -> Vec<&'a Expr> {
    let mut out = Vec::new();
    let mut stack: Vec<&'a EffectNode> = effects.iter().collect();
    while let Some(e) = stack.pop() {
        match e {
            EffectNode::Set {
                target: Target::Name { card: c, .. },
                key: k,
                op,
                expr,
                ..
            } if c == card && k == key && *op == want_op => out.push(expr),
            EffectNode::Branch { then, els, .. } => stack.extend(then.iter().chain(els.iter())),
            _ => {}
        }
    }
    out
}

fn resource_bounded(
    key: &AspectKey,
    exclude_card: Option<&CardId>,
    content: &Content,
    recipe_mintable: &BTreeSet<CardId>,
    dealt: &BTreeSet<CardId>,
) -> bool {
    if content.aspects.get(key).map(|a| a.default).unwrap_or(0) > 0 {
        return false;
    }
    let mut written_outside = false;
    visit_content_effects(content, &mut |n| {
        if let EffectNode::Set {
            target,
            key: k,
            op: SetOp::Add | SetOp::Assign,
            ..
        } = n
        {
            if k == key
                && !matches!(target, Target::Name { card, .. } if exclude_card == Some(card))
            {
                written_outside = true;
            }
        }
    });
    !written_outside
        && content
            .cards
            .values()
            .filter(|c| exclude_card != Some(&c.id) && c.aspects.get(key).copied().unwrap_or(0) > 0)
            .all(|c| !recipe_mintable.contains(&c.id) && !dealt.contains(&c.id))
}

fn non_recipe_writes_aspect(
    content: &Content,
    oracle: &MatchOracle,
    card: &CardId,
    key: &AspectKey,
    increase: bool,
) -> bool {
    let matches_op = |op: &SetOp, expr: &Expr| match (op, increase) {
        (SetOp::Assign, _) => true,
        (SetOp::Add, true) | (SetOp::Sub, false) => !matches!(expr, Expr::Lit(n) if *n <= 0),
        (SetOp::Sub, true) | (SetOp::Add, false) => matches!(expr, Expr::Lit(n) if *n < 0),
    };
    let mut hit = false;
    visit_non_recipe_books(content, &mut |b, subject| {
        for r in &b.rules {
            visit_effects(&r.effects, &mut |e| {
                if let EffectNode::Set {
                    target,
                    key: k,
                    op,
                    expr,
                    ..
                } = e
                {
                    if k == key
                        && matches_op(op, expr)
                        && target_can_hit_card(content, oracle, None, subject, target, card)
                    {
                        hit = true;
                    }
                }
            });
        }
    });
    hit
}

fn recipe_has_unbounded_write(
    content: &Content,
    oracle: &MatchOracle,
    rec: &RecipeDecl,
    card: &CardId,
    key: &AspectKey,
    increase: bool,
) -> bool {
    let mut hit = false;
    visit_effects(&rec.effects, &mut |e| {
        if let EffectNode::Set {
            target,
            key: k,
            op,
            expr,
            ..
        } = e
        {
            if k == key && target_can_hit_card(content, oracle, Some(rec), None, target, card) {
                let is_named = matches!(target, Target::Name { card: c, .. } if c == card);
                if matches!(
                    (op, increase),
                    (SetOp::Assign, _)
                        | (SetOp::Add, true) if !is_named
                ) || matches!((op, increase), (SetOp::Sub, false) if !is_named)
                    || matches!((op, increase, expr), (SetOp::Sub, true, Expr::Lit(n)) | (SetOp::Add, false, Expr::Lit(n)) if *n < 0)
                {
                    hit = true;
                }
            }
        }
    });
    hit
}

fn target_can_hit_card(
    content: &Content,
    oracle: &MatchOracle,
    rec: Option<&RecipeDecl>,
    subject: Option<&CardId>,
    target: &Target,
    card: &CardId,
) -> bool {
    let Some(decl) = content.card(card) else {
        return false;
    };
    match target {
        Target::Name { card: c, .. } => c == card,
        Target::Slot(sname) => rec
            .and_then(|r| r.slots.iter().find(|s| &s.name == sname))
            .is_some_and(|sd| oracle.can_match(content, &sd.query, decl)),
        Target::Query { terms, .. } | Target::BareQuery(terms) => {
            oracle.can_match(content, terms, decl)
        }
        Target::Handle(h) => h != "subject" || subject == Some(card),
    }
}

fn find_autonomous_drains(
    card: &CardId,
    key: &AspectKey,
    content: &Content,
) -> Vec<AutonomousDrain> {
    let mut out = Vec::new();
    for verb in content.verbs.values() {
        if verb.mode != VerbMode::Autonomous
            || matches!(verb.instances, InstanceCap::AtMost(0))
            || verb.recipes.is_empty()
        {
            continue;
        }
        let mut recipes: Vec<&RecipeDecl> = verb.recipes.values().collect();
        recipes.sort_by_key(|r| (r.priority, r.decl_index));

        if recipes.iter().any(|r| {
            r.duration <= 0 || !r.slots.is_empty() || r.max.is_some() || !r.guards.is_empty()
        }) {
            continue;
        }
        let max_dur = recipes.iter().map(|r| r.duration).max().unwrap_or(0);
        let top = recipes[0];
        let sub = top_level_sub(&top.effects, card, key);
        if sub < 1 {
            continue;
        }
        let mut c = Clause::default();
        if let Some(w) = &top.when {
            walk_conjunctive_cond(w, false, content, &mut c, true);
        }
        if c.open_bounds
            == [Bound::Low {
                card: card.clone(),
                key: key.clone(),
                n: 1,
            }]
            && c.legacy_gates.is_empty()
            && c.min_open_clock == 0
        {
            out.push(AutonomousDrain {
                verb: verb.id.clone(),
                drain_recipe: top.id.clone(),
                cycle_duration: max_dur,
                sub,
            });
        }
    }
    out
}

pub(super) fn autonomous_drain_before(
    drain: &AutonomousDrain,
    state: &State,
    min_goal_time: i64,
    deadline: Option<(i64, Option<SituationId>)>,
) -> i64 {
    let mut total = 0i64;
    let mut next_open = state.now;
    if let Some(sit) = state.situations.values().find(|s| s.verb == drain.verb) {
        next_open = sit.ends_at;
        let settles_before = sit.ends_at < min_goal_time
            || (sit.ends_at == min_goal_time
                && matches!(deadline, Some((dl_t, Some(dl_s))) if dl_t == min_goal_time && sit.id < dl_s));
        if sit.recipe == drain.drain_recipe && settles_before {
            total = total.saturating_add(drain.sub);
        }
    }
    if min_goal_time - 1 >= next_open && drain.cycle_duration > 0 {
        let cycles = (min_goal_time - 1 - next_open) / drain.cycle_duration;
        total = total.saturating_add(cycles.saturating_mul(drain.sub));
    }
    total
}
