//! The scarcity bound, the numeric half of the feasibility gate: for goals
//! that provably need unique-card aspects to satisfy lower or upper bounds,
//! bound the achievable values from a state and prune when every disjunctive
//! goal clause is unreachable within the remaining horizon.

use std::collections::{BTreeMap, BTreeSet};

use story_content::ir::{
    self, ArithOp, AspectKey, AspectTerm, Book, CmpOp, EffectNode, Expr, InstanceCap, Into,
    LegacyKey, LogicOp, QueryTerm, RecipeDecl, SetOp, Target, VerbDecl, VerbMode,
};
use story_content::Content;
use story_core::{CardId, EndingId, RecipeId, SituationId, State, VerbId, ZoneKind};

use crate::causal::{visit_content_effects, MatchOracle};

/// One necessary bound on a unique card's aspect: either `card.key >= n`
/// (`Low`) or `card.key <= n` (`High`).
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Bound {
    Low {
        card: CardId,
        key: AspectKey,
        n: i64,
    },
    High {
        card: CardId,
        key: AspectKey,
        n: i64,
    },
}

/// One conjunctive clause of a goal. A goal is a disjunction of clauses:
/// the state is dead when every clause in `clauses` is dead.
#[derive(Clone, Debug, Default)]
struct Clause {
    recipe: Option<RecipeId>,
    min_open_clock: i64,
    open_bounds: Vec<Bound>,
    complete_bounds: Vec<Bound>,
    legacy_gates: Vec<(LegacyKey, CmpOp, i64)>,
}

/// Execution-count constraints shared by gain and drop recipe routes.
#[derive(Clone, Debug)]
struct RouteCap {
    recipe: RecipeId,
    verb: VerbId,
    verb_instances: Option<u32>,
    duration: i64,
    max_runs: Option<u32>,
    static_legacy_low: Vec<(LegacyKey, i64)>,
    consumed: Vec<(AspectKey, i64)>,
    carrier_moved: Vec<(CardId, AspectKey, i64)>,
    reservoirs: Vec<(CardId, AspectKey, i64)>,
    required_live: Vec<CardId>,
}

/// One additive term of a recipe's gain per execution.
#[derive(Clone, Debug)]
enum GainTerm {
    Literal(i64),
    Stock {
        key: AspectKey,
        per_run_cap: Option<i64>,
        exclude_card: Option<CardId>,
        zone_only: bool,
    },
}

#[derive(Clone, Debug)]
struct GainRoute {
    cap: RouteCap,
    terms: Vec<GainTerm>,
}

/// An autonomous verb that periodically drains `(card, key)` whenever
/// `card.key >= 1`.
#[derive(Clone, Debug)]
struct AutonomousDrain {
    verb: VerbId,
    drain_recipe: RecipeId,
    cycle_duration: i64,
    sub: i64,
}

#[derive(Clone, Debug)]
struct GainModel {
    unbounded: bool,
    routes: Vec<GainRoute>,
    drains: Vec<AutonomousDrain>,
}

#[derive(Clone, Debug)]
struct DropRoute {
    cap: RouteCap,
    drop_per_run: i64,
}

#[derive(Clone, Debug)]
struct DropModel {
    unbounded: bool,
    routes: Vec<DropRoute>,
}

/// The numeric scarcity model for one goal.
pub(crate) struct Scarcity {
    goal_ending: Option<EndingId>,
    clauses: Vec<Clause>,
    gains: BTreeMap<(CardId, AspectKey), GainModel>,
    drops: BTreeMap<(CardId, AspectKey), DropModel>,
    recipe_mintable: BTreeSet<CardId>,
    dealt: BTreeSet<CardId>,
}

impl Scarcity {
    /// Build the scarcity model for `goal` over compiled `content`.
    pub(crate) fn build(
        goal: &crate::query::GoalPredicate,
        content: &Content,
        recipe_mintable: &BTreeSet<CardId>,
    ) -> Scarcity {
        let mut host_opaque = false;
        visit_content_effects(content, &mut |n| {
            if matches!(n, EffectNode::Op { .. }) {
                host_opaque = true;
            }
        });
        if host_opaque {
            return Scarcity::empty();
        }
        let dealt: BTreeSet<CardId> = content
            .decks
            .values()
            .flat_map(|d| d.entries.iter().chain(d.fallback.iter()))
            .cloned()
            .collect();
        let oracle = MatchOracle::new(content);
        let static_legacies = non_terminal_unwritten_legacies(content);

        let goal_ending = match goal {
            crate::query::GoalPredicate::Ending(Some(id)) => Some(id.clone()),
            _ => None,
        };
        let clauses = extract_goal_clauses(goal, content);

        let mut gains: BTreeMap<(CardId, AspectKey), GainModel> = BTreeMap::new();
        let mut drops: BTreeMap<(CardId, AspectKey), DropModel> = BTreeMap::new();

        let mut low_targets: BTreeSet<(CardId, AspectKey)> = BTreeSet::new();
        let mut high_targets: BTreeSet<(CardId, AspectKey)> = BTreeSet::new();
        for c in &clauses {
            for b in c.open_bounds.iter().chain(c.complete_bounds.iter()) {
                match b {
                    Bound::Low { card, key, .. } => {
                        low_targets.insert((card.clone(), key.clone()));
                    }
                    Bound::High { card, key, .. } => {
                        high_targets.insert((card.clone(), key.clone()));
                    }
                }
            }
        }

        let mut queue: Vec<(CardId, AspectKey)> = low_targets.into_iter().collect();
        while let Some((card, key)) = queue.pop() {
            if gains.contains_key(&(card.clone(), key.clone())) {
                continue;
            }
            let model = build_gain_model(
                &card,
                &key,
                content,
                &oracle,
                recipe_mintable,
                &dealt,
                &static_legacies,
            );
            for route in &model.routes {
                for (rc, rk, _) in &route.cap.reservoirs {
                    if !gains.contains_key(&(rc.clone(), rk.clone())) {
                        queue.push((rc.clone(), rk.clone()));
                    }
                }
            }
            gains.insert((card, key), model);
        }

        for (card, key) in high_targets {
            let model = build_drop_model(
                &card,
                &key,
                content,
                &oracle,
                recipe_mintable,
                &dealt,
                &static_legacies,
            );
            for route in &model.routes {
                for (rc, rk, _) in &route.cap.reservoirs {
                    if !gains.contains_key(&(rc.clone(), rk.clone())) {
                        let rmodel = build_gain_model(
                            rc,
                            rk,
                            content,
                            &oracle,
                            recipe_mintable,
                            &dealt,
                            &static_legacies,
                        );
                        gains.insert((rc.clone(), rk.clone()), rmodel);
                    }
                }
            }
            drops.insert((card, key), model);
        }

        Scarcity {
            goal_ending,
            clauses,
            gains,
            drops,
            recipe_mintable: recipe_mintable.clone(),
            dealt,
        }
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.clauses.is_empty() && self.goal_ending.is_none()
    }

    pub(crate) fn empty() -> Scarcity {
        Scarcity {
            goal_ending: None,
            clauses: Vec::new(),
            gains: BTreeMap::new(),
            drops: BTreeMap::new(),
            recipe_mintable: BTreeSet::new(),
            dealt: BTreeSet::new(),
        }
    }

    /// True when the goal provably cannot be reached from `state` (within
    /// `max_time` when specified).
    pub(crate) fn dead(&self, state: &State, content: &Content, max_time: Option<i64>) -> bool {
        if self.is_empty() {
            return false;
        }
        if let Some(goal_id) = &self.goal_ending {
            if state.ended.as_ref() == Some(goal_id) {
                return false;
            }
            if state.ended.is_some() {
                return true;
            }
        }
        if let Some(h) = max_time {
            if state.now > h {
                return true;
            }
        }

        let in_flight_end = earliest_in_flight_ending(state, content);
        if let (Some(goal_id), Some((end_time, end_sit, ref endings))) =
            (&self.goal_ending, &in_flight_end)
        {
            if !endings.contains(goal_id)
                && !self
                    .clauses
                    .iter()
                    .any(|c| clause_can_beat_ending(c, state, content, (*end_time, *end_sit)))
            {
                return true;
            }
        }

        !self.clauses.is_empty()
            && self
                .clauses
                .iter()
                .all(|c| self.clause_dead(c, state, content, max_time, in_flight_end.as_ref()))
    }

    fn clause_dead(
        &self,
        clause: &Clause,
        state: &State,
        content: &Content,
        horizon: Option<i64>,
        in_flight_end: Option<&(i64, SituationId, BTreeSet<EndingId>)>,
    ) -> bool {
        for (lkey, op, target_val) in &clause.legacy_gates {
            let cur = state
                .legacy
                .get(lkey)
                .copied()
                .unwrap_or_else(|| content.legacies.get(lkey).map(|d| d.default).unwrap_or(0));
            if !cmp_eval(*op, cur, *target_val)
                && non_terminal_unwritten_legacies(content).contains(lkey)
            {
                return true;
            }
        }

        let mut in_flight_sit: Option<&story_core::Situation> = None;
        let mut min_complete_time = state.now;
        let mut deadline: Option<(i64, Option<SituationId>)> = horizon.map(|h| (h, None));

        if let Some((end_t, end_s, endings)) = in_flight_end {
            if !self
                .goal_ending
                .as_ref()
                .is_some_and(|g| endings.contains(g))
            {
                deadline = Some(match deadline {
                    Some((d, ds)) if d < *end_t => (d, ds),
                    _ => (*end_t, Some(*end_s)),
                });
            }
        }

        if let Some(rec_id) = &clause.recipe {
            in_flight_sit = state.situations.values().find(|s| &s.recipe == rec_id);
            if let Some(sit) = in_flight_sit {
                min_complete_time = sit.ends_at;
                let sit_cap = (sit.ends_at, Some(sit.id));
                deadline = Some(match deadline {
                    Some((d, ds)) if (d, ds) < sit_cap => (d, ds),
                    _ => sit_cap,
                });
            } else if let Some(rec) = content.recipe(rec_id.as_str()) {
                if rec
                    .max
                    .is_some_and(|m| state.recipe_runs.get(rec_id).copied().unwrap_or(0) >= m)
                {
                    return true;
                }
                let verb_free = content
                    .verbs
                    .values()
                    .find(|v| v.recipes.contains_key(rec_id.as_str()))
                    .map(|v| verb_free_at(state, v))
                    .unwrap_or(state.now);
                let earliest_open = state.now.max(clause.min_open_clock).max(verb_free);
                min_complete_time = earliest_open.saturating_add(rec.duration);
            }
        }

        if let Some((dl_time, dl_sit)) = deadline {
            if min_complete_time > dl_time {
                return true;
            }
            if let Some(s_end) = dl_sit {
                if let Some(sit) = in_flight_sit {
                    if (sit.ends_at, sit.id) > (dl_time, s_end) {
                        return true;
                    }
                } else if min_complete_time == dl_time {
                    return true;
                }
            }
        }

        if in_flight_sit.is_none() {
            let open_deadline = deadline.map(|(t, s)| {
                let dur = clause
                    .recipe
                    .as_ref()
                    .and_then(|r| content.recipe(r.as_str()))
                    .map(|r| r.duration)
                    .unwrap_or(0);
                (t.saturating_sub(dur), s)
            });
            if clause
                .open_bounds
                .iter()
                .any(|b| self.bound_dead(b, state, content, state.now, open_deadline))
            {
                return true;
            }
        }

        clause
            .complete_bounds
            .iter()
            .any(|b| self.bound_dead(b, state, content, min_complete_time, deadline))
    }

    fn bound_dead(
        &self,
        bound: &Bound,
        state: &State,
        content: &Content,
        min_goal_time: i64,
        deadline: Option<(i64, Option<SituationId>)>,
    ) -> bool {
        match bound {
            Bound::Low { card, key, n } => self
                .gains
                .get(&(card.clone(), key.clone()))
                .and_then(|m| {
                    self.max_achievable(card, key, m, state, content, min_goal_time, deadline)
                })
                .is_some_and(|v| v < *n),
            Bound::High { card, key, n } => self
                .drops
                .get(&(card.clone(), key.clone()))
                .and_then(|m| self.min_achievable(card, key, m, state, content, deadline))
                .is_some_and(|v| v > *n),
        }
    }

    fn max_achievable(
        &self,
        card: &CardId,
        key: &AspectKey,
        model: &GainModel,
        state: &State,
        content: &Content,
        min_goal_time: i64,
        deadline: Option<(i64, Option<SituationId>)>,
    ) -> Option<i64> {
        if model.unbounded {
            return None;
        }
        let v = read_card_aspect(state, content, card, key);
        let drain_total: i64 = model
            .drains
            .iter()
            .map(|d| autonomous_drain_before(d, state, min_goal_time, deadline))
            .fold(0i64, i64::saturating_add);
        let base = if drain_total > 0 {
            v.saturating_sub(drain_total).max(0)
        } else {
            v
        };

        let mut total_gain = 0i64;
        let mut counted_stocks: BTreeSet<(AspectKey, Option<CardId>, bool)> = BTreeSet::new();

        for route in &model.routes {
            let uses = self.route_max_uses(&route.cap, state, content, deadline)?;
            if uses == 0 {
                continue;
            }
            for term in &route.terms {
                match term {
                    GainTerm::Literal(g) => {
                        total_gain = total_gain.saturating_add(g.saturating_mul(uses));
                    }
                    GainTerm::Stock {
                        key: skey,
                        per_run_cap,
                        exclude_card,
                        zone_only,
                    } => {
                        let stock = aspect_stock(state, skey, exclude_card.as_ref(), *zone_only);
                        let capped =
                            per_run_cap.map_or(stock, |cap| stock.min(cap.saturating_mul(uses)));
                        if counted_stocks.insert((skey.clone(), exclude_card.clone(), *zone_only)) {
                            total_gain = total_gain.saturating_add(capped);
                        }
                    }
                }
            }
        }

        Some(base.saturating_add(total_gain))
    }

    fn min_achievable(
        &self,
        card: &CardId,
        key: &AspectKey,
        model: &DropModel,
        state: &State,
        content: &Content,
        deadline: Option<(i64, Option<SituationId>)>,
    ) -> Option<i64> {
        if model.unbounded {
            return None;
        }
        let v = read_card_aspect(state, content, card, key);
        let mut total_drop = 0i64;
        for route in &model.routes {
            let uses = self.route_max_uses(&route.cap, state, content, deadline)?;
            total_drop = total_drop.saturating_add(route.drop_per_run.saturating_mul(uses));
        }
        Some(v.saturating_sub(total_drop))
    }

    fn route_max_uses(
        &self,
        cap: &RouteCap,
        state: &State,
        content: &Content,
        deadline: Option<(i64, Option<SituationId>)>,
    ) -> Option<i64> {
        if cap.required_live.iter().any(|c| {
            !self.recipe_mintable.contains(c)
                && !self.dealt.contains(c)
                && !state.entities.values().any(|e| &e.prototype == c)
        }) {
            return Some(0);
        }
        for (lkey, min_val) in &cap.static_legacy_low {
            let cur = state
                .legacy
                .get(lkey)
                .copied()
                .unwrap_or_else(|| content.legacies.get(lkey).map(|d| d.default).unwrap_or(0));
            if cur < *min_val {
                return Some(0);
            }
        }

        let in_flight_valid = state
            .situations
            .values()
            .filter(|sit| {
                sit.recipe == cap.recipe
                    && match deadline {
                        None => true,
                        Some((dl_time, None)) => sit.ends_at <= dl_time,
                        Some((dl_time, Some(dl_sit))) => (sit.ends_at, sit.id) < (dl_time, dl_sit),
                    }
            })
            .count() as i64;

        let mut new_opens: Option<i64> = cap.max_runs.map(|m| {
            let used = state.recipe_runs.get(&cap.recipe).copied().unwrap_or(0);
            (m.saturating_sub(used)) as i64
        });

        if let Some((dl_time, dl_sit)) = deadline {
            let latest_finish = if dl_sit.is_some() {
                dl_time - 1
            } else {
                dl_time
            };
            let verb_free = content
                .verbs
                .get(cap.verb.as_str())
                .map(|v| verb_free_at(state, v))
                .unwrap_or(state.now);

            let mut earliest_open = state.now.max(verb_free);
            for (rc, rk, per) in &cap.reservoirs {
                if read_card_aspect(state, content, rc, rk) < *per {
                    if let Some(rmodel) = self.gains.get(&(rc.clone(), rk.clone())) {
                        if let Some(min_feed_finish) =
                            earliest_feeder_finish(rmodel, state, content)
                        {
                            earliest_open = earliest_open.max(min_feed_finish);
                        } else {
                            new_opens = Some(0);
                        }
                    }
                }
            }

            if earliest_open + cap.duration > latest_finish {
                new_opens = Some(0);
            } else if cap.duration > 0 {
                let instances = cap.verb_instances.unwrap_or(64).min(64) as i64;
                let time_bound =
                    ((latest_finish - earliest_open) / cap.duration).saturating_mul(instances);
                new_opens = Some(new_opens.map_or(time_bound, |u| u.min(time_bound)));
            }
        }

        let mut uses = new_opens.map(|n| n.saturating_add(in_flight_valid));

        for (k, per) in &cap.consumed {
            let stock = aspect_stock(state, k, None, false);
            let from_stock = if *per > 0 { stock / *per } else { i64::MAX };
            uses = Some(uses.map_or(from_stock, |u| u.min(from_stock)));
        }

        for (carrier, k, per) in &cap.carrier_moved {
            let stock = aspect_stock(state, k, Some(carrier), true);
            let from_stock = if *per > 0 { stock / *per } else { i64::MAX };
            uses = Some(uses.map_or(from_stock, |u| u.min(from_stock)));
        }

        for (rc, rk, per) in &cap.reservoirs {
            let rmodel = self.gains.get(&(rc.clone(), rk.clone()))?;
            let max_res =
                self.max_achievable(rc, rk, rmodel, state, content, state.now, deadline)?;
            let from_res = if *per > 0 { max_res / *per } else { i64::MAX };
            uses = Some(uses.map_or(from_res, |u| u.min(from_res)));
        }

        uses
    }
}

// ---------------------------------------------------------------------------
// Goal & clause extraction
// ---------------------------------------------------------------------------

fn extract_goal_clauses(goal: &crate::query::GoalPredicate, content: &Content) -> Vec<Clause> {
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

fn walk_conjunctive_cond(
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

// ---------------------------------------------------------------------------
// Gain & Drop model construction
// ---------------------------------------------------------------------------

fn build_gain_model(
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

fn build_drop_model(
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

fn autonomous_drain_before(
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

// ---------------------------------------------------------------------------
// State & content helpers
// ---------------------------------------------------------------------------

fn visit_effects(effects: &[EffectNode], f: &mut impl FnMut(&EffectNode)) {
    let mut stack: Vec<&EffectNode> = effects.iter().collect();
    while let Some(e) = stack.pop() {
        f(e);
        if let EffectNode::Branch { then, els, .. } = e {
            stack.extend(then.iter().chain(els.iter()));
        }
    }
}

fn visit_non_recipe_books(content: &Content, f: &mut impl FnMut(&Book, Option<&CardId>)) {
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

fn is_unique_card(content: &Content, card: &CardId) -> bool {
    content.card(card).is_some_and(|d| {
        matches!(
            d.mint,
            story_core::MintPolicy::Unique | story_core::MintPolicy::SingleCopy
        )
    })
}

fn read_card_aspect(state: &State, content: &Content, card: &CardId, key: &AspectKey) -> i64 {
    state
        .entities
        .values()
        .find(|e| &e.prototype == card)
        .and_then(|e| e.aspects.get(key))
        .copied()
        .unwrap_or_else(|| content.aspects.get(key).map(|a| a.default).unwrap_or(0))
}

fn aspect_stock(
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

fn verb_free_at(state: &State, verb: &ir::VerbDecl) -> i64 {
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

fn earliest_feeder_finish(rmodel: &GainModel, state: &State, content: &Content) -> Option<i64> {
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

fn content_moves_into_zone(content: &Content) -> bool {
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

fn non_terminal_unwritten_legacies(content: &Content) -> BTreeSet<LegacyKey> {
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

fn guaranteed_endings(effects: &[EffectNode]) -> Option<BTreeSet<EndingId>> {
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

fn earliest_in_flight_ending(
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

fn clause_can_beat_ending(
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

fn collect_when_required_cards(e: &Expr, out: &mut Vec<CardId>) {
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

fn cmp_eval(op: CmpOp, lhs: i64, rhs: i64) -> bool {
    match op {
        CmpOp::Ge => lhs >= rhs,
        CmpOp::Gt => lhs > rhs,
        CmpOp::Le => lhs <= rhs,
        CmpOp::Lt => lhs < rhs,
        CmpOp::Eq => lhs == rhs,
        CmpOp::Ne => lhs != rhs,
    }
}
