//! The scarcity bound, the numeric half of the feasibility gate: for goals
//! that provably need unique-card aspects to satisfy lower or upper bounds,
//! bound the achievable values from a state and prune when every disjunctive
//! goal clause is unreachable within the remaining horizon.

use std::collections::{BTreeMap, BTreeSet};

use story_content::ir::{AspectKey, EffectNode};
use story_content::Content;
use story_core::{CardId, EndingId, SituationId, State};

use crate::causal::{visit_content_effects, MatchOracle};

mod build;
mod extract;
mod helpers;
mod model;

use build::{autonomous_drain_before, build_drop_model, build_gain_model};
use extract::extract_goal_clauses;
use helpers::{
    aspect_stock, clause_can_beat_ending, cmp_eval, earliest_feeder_finish,
    earliest_in_flight_ending, non_terminal_unwritten_legacies, read_card_aspect, verb_free_at,
};
use model::{Bound, Clause, DropModel, GainModel, GainTerm, RouteCap};

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
