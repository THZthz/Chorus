//! The reachability solver: best-first search with dominance rules, feasibility gates, and replay-verified witnesses.

use std::cmp::Reverse;
use std::collections::{BTreeMap, BTreeSet};

use story_core::{CardId, RecipeId};
use story_engine::{ChanceOutcome, ChancePoint};

use super::frontier::{path, priority, ArenaNode, Key};
use super::Analyzer;
use crate::canonical::CanonicalStateKey;
use crate::causal;
use crate::query::{GoalCtx, ReachQuery, RngMode, SearchObjective};
use crate::replay::ReplayError;
use crate::transitions::{PlayerStep, TransitionRecord, WitnessPath};

impl Analyzer {
    /// Solve one reach query (§5.5): best-first search over the transition
    /// space with the objective's dominance rules; every returned witness
    /// has passed the replay oracle (§5.6). Existential under every RNG
    /// mode — guarantee queries go through the AND-OR solver (`mdp`).
    pub fn solve(&mut self, query: &ReachQuery) -> SolveOutcome {
        let _span = tracing::info_span!("solve").entered();
        let mut gen = self.generator(query);
        let goal_facts = causal::goal_facts(&query.goal);
        let cone = {
            let _span = tracing::info_span!("backward_cone").entered();
            self.causal.backward_cone(&goal_facts)
        };
        let relevant = self.causal.relevant_verbs(&self.content, &cone);

        let mut h_ws = self.causal.workspace();

        // The start node; the invariant must hold there too.
        if let Some(inv) = &query.invariant {
            let start = self.start.clone();
            let ctx = self.scratch(&start);
            if !inv.holds(&ctx) {
                return SolveOutcome::empty();
            }
        }
        let start_h = if self.heuristic || query.prune {
            *self.engine.state_mut() = self.start.clone();
            let raw = self.causal.h_add_with_workspace(
                &self.start,
                &self.engine,
                &goal_facts,
                &mut h_ws,
            );
            i64::from(raw.min(4096))
        } else {
            0
        };
        let mut arena: Vec<ArenaNode> = vec![ArenaNode {
            state: Some(self.start.clone()),
            parent: None,
            record: None,
            actions: 0,
            depth: 0,
            sig: Vec::new(),
            dist_to_spine: 0,
            h: start_h,
            h_computed: true,
            order: 0,
        }];

        let diverse = matches!(query.objective, SearchObjective::DiverseWays { .. });
        let max_ways = match &query.objective {
            SearchObjective::DiverseWays { max_ways } => *max_ways,
            _ => 1,
        };

        let mut visited: BTreeMap<CanonicalStateKey, Vec<i64>> = BTreeMap::new();
        let mut sig_visited: BTreeSet<(CanonicalStateKey, Vec<Option<RecipeId>>)> = BTreeSet::new();
        let mut queue: std::collections::BinaryHeap<Reverse<Key>> =
            std::collections::BinaryHeap::new();
        queue.push(Reverse(Key {
            parts: [0, 0, 0],
            seq: 0,
        }));
        let mut expansions = 0usize;
        let mut truncated = false;
        let mut pruned = 0usize;
        // The extinction half of the feasibility gate: goal cards that are
        // gone for good once they leave the table.
        let goal_cards: Vec<CardId> = if query.prune && !self.causal.host_opaque() {
            goal_facts
                .iter()
                .filter_map(|f| match f {
                    causal::Fact::CardLive(c)
                    | causal::Fact::CardInZone(c, _)
                    | causal::Fact::CardHasAspect(c, _) => Some(c.clone()),
                    _ => None,
                })
                .collect()
        } else {
            Vec::new()
        };
        let recipe_mintable = if query.prune {
            self.causal.recipe_mintable_cards()
        } else {
            BTreeSet::new()
        };
        let scarcity = if query.prune && !self.causal.host_opaque() {
            crate::scarcity::Scarcity::build(&query.goal, &self.content, &recipe_mintable)
        } else {
            crate::scarcity::Scarcity::empty()
        };
        let mut witnesses: Vec<WitnessPath> = Vec::new();
        let mut witnessed_sigs: BTreeSet<Vec<Option<RecipeId>>> = BTreeSet::new();
        let mut dropped = 0usize;
        let replayer = self.replayer();

        // Explored envelope trackers
        let mut time_min = self.start.now;
        let mut time_max = self.start.now;
        let mut verbs_used = BTreeSet::new();
        let mut recipes_initiated = BTreeSet::new();
        let mut recipes_completed = BTreeSet::new();
        let mut endings_encountered = BTreeSet::new();
        let mut frontier_boundary_edges = 0usize;
        let mut incumbent_cost: Option<Vec<i64>> = None;
        let mut best_witness: Option<WitnessPath> = None;
        let mut min_dist: BTreeMap<CanonicalStateKey, usize> = BTreeMap::new();

        let start_key = CanonicalStateKey::of_with(&self.start, &self.content, &self.symmetry);
        min_dist.insert(start_key, 0);

        for sit in self.start.situations.values() {
            verbs_used.insert(sit.verb.clone());
            recipes_initiated.insert(sit.recipe.clone());
        }
        if let Some(ending) = &self.start.ended {
            endings_encountered.insert(ending.clone());
        }

        // -------------------------------------------------------------------
        // Seed Trajectories & Tube Search
        // -------------------------------------------------------------------
        let mut seed_paths: Vec<WitnessPath> = Vec::new();
        if let Some(trace) = &query.seed_trace {
            match trace.execute_with_seed(self.content.clone(), self.registry.clone(), self.seed) {
                Ok(w) => seed_paths.push(w),
                Err(_) => dropped += 1,
            }
        }
        for trace in &query.seed_traces {
            match trace.execute_with_seed(self.content.clone(), self.registry.clone(), self.seed) {
                Ok(w) => seed_paths.push(w),
                Err(_) => dropped += 1,
            }
        }
        if query.seed_samples > 0 {
            let sampler = crate::sample::RolloutSampler::new(
                &self.content,
                &self.causal,
                &query.goal,
                crate::sample::RolloutConfig {
                    max_steps: 128,
                    max_time: query.max_time,
                    rng_mode: query.rng_mode,
                    p_advance: 0.40,
                },
            );
            for i in 0..query.seed_samples {
                let sample_seed = self
                    .seed
                    .wrapping_add((i as u64 + 1).wrapping_mul(0x517cc1b727220a95));
                let path = sampler.sample_path(&mut gen, &self.start, &query.goal, sample_seed);
                if !path.records.is_empty() {
                    seed_paths.push(path);
                }
            }
        }

        let is_seeded = query.seed_trace.is_some() || query.seed_samples > 0;
        let max_radius = query
            .tube_radius
            .unwrap_or(if is_seeded { 1 } else { usize::MAX });

        for path in &seed_paths {
            let final_state = path.final_state();
            *self.engine.state_mut() = final_state.clone();
            let last_events = path
                .records
                .last()
                .map(|r| r.events.as_slice())
                .unwrap_or(&[]);
            let ctx = GoalCtx {
                engine: &self.engine,
                events: last_events,
            };
            if query.goal.holds(&ctx) {
                let coalesced = self.coalesce(path.clone());
                if replayer
                    .replay(&coalesced, Some(&query.goal), query.invariant.as_ref())
                    .is_ok()
                {
                    if !diverse {
                        let w_cost = match &query.objective {
                            SearchObjective::FewestActions => {
                                vec![coalesced.actions as i64, coalesced.final_clock()]
                            }
                            SearchObjective::EarliestTime => {
                                vec![coalesced.final_clock(), coalesced.actions as i64]
                            }
                            SearchObjective::ShortestDepth | SearchObjective::DiverseWays { .. } => {
                                vec![coalesced.depth as i64]
                            }
                        };
                        if incumbent_cost.as_ref().map_or(true, |best| w_cost < *best) {
                            incumbent_cost = Some(w_cost);
                            best_witness = Some(coalesced);
                        }
                    } else if witnessed_sigs.insert(coalesced.recipe_trace()) {
                        witnesses.push(coalesced);
                    }
                }
            }

            let mut cur_parent = 0usize;
            let mut cur_actions = 0usize;
            let mut cur_depth = 0usize;
            let mut cur_sig = Vec::new();

            for rec in &path.records {
                let state_after = rec.state_after.clone();
                let key =
                    CanonicalStateKey::of_with(&state_after, &self.content, &self.symmetry);
                cur_actions += rec.actions();
                cur_depth += rec.depth();
                if let Some(r) = &rec.recipe {
                    cur_sig.push(Some(r.clone()));
                }
                let raw_h = if self.heuristic || query.prune {
                    *self.engine.state_mut() = state_after.clone();
                    self.causal.h_add_with_workspace(&state_after, &self.engine, &goal_facts, &mut h_ws)
                } else {
                    0
                };
                let h = i64::from(raw_h.min(4096));
                let child_idx = arena.len();
                arena.push(ArenaNode {
                    state: None,
                    parent: Some(cur_parent),
                    record: Some(rec.clone()),
                    actions: cur_actions,
                    depth: cur_depth,
                    sig: cur_sig.clone(),
                    dist_to_spine: 0,
                    h,
                    h_computed: true,
                    order: 0,
                });
                cur_parent = child_idx;

                time_min = time_min.min(state_after.now);
                time_max = time_max.max(state_after.now);
                if let Some(ending) = &state_after.ended {
                    endings_encountered.insert(ending.clone());
                }
                if let PlayerStep::Commit { verb, .. } = &rec.step {
                    verbs_used.insert(verb.clone());
                }
                if let Some(r) = &rec.recipe {
                    recipes_initiated.insert(r.clone());
                }
                for ev in &rec.events {
                    if let story_engine::Event::SituationCompleted { recipe, .. } = ev {
                        recipes_completed.insert(recipe.clone());
                    }
                }
                let cost = match &query.objective {
                    SearchObjective::FewestActions => vec![cur_actions as i64, state_after.now],
                    SearchObjective::EarliestTime => vec![state_after.now, cur_actions as i64],
                    SearchObjective::ShortestDepth | SearchObjective::DiverseWays { .. } => {
                        vec![cur_depth as i64]
                    }
                };
                min_dist.insert(key, 0);
                visited.insert(key, cost);
                queue.push(Reverse(Key {
                    parts: priority(
                        &query.objective,
                        cur_actions,
                        cur_depth,
                        state_after.now,
                        h,
                    ),
                    seq: child_idx,
                }));
            }
        }

        let start_wall = std::time::Instant::now();
        let mut budget_depleted = false;
        let mut last_popped_idx = None;

        while let Some(Reverse(k)) = queue.pop() {
            let idx = k.seq;
            last_popped_idx = Some(idx);
            if let Some(budget) = query.time_budget {
                if start_wall.elapsed() >= budget {
                    truncated = true;
                    budget_depleted = true;
                    break;
                }
            }
            if !arena[idx].h_computed {
                let raw_h = {
                    *self.engine.state_mut() = arena[idx].state().clone();
                    self.causal.h_add_with_workspace(
                        arena[idx].state(),
                        &self.engine,
                        &goal_facts,
                        &mut h_ws,
                    )
                };
                arena[idx].h_computed = true;
                if query.prune && !self.causal.host_opaque() && raw_h >= u32::MAX / 4 {
                    *self.engine.state_mut() = arena[idx].state().clone();
                    let events = arena[idx]
                        .record
                        .as_ref()
                        .map(|r| r.events.as_slice())
                        .unwrap_or(&[]);
                    let ctx = GoalCtx {
                        engine: &self.engine,
                        events,
                    };
                    if !query.goal.holds(&ctx) {
                        pruned += 1;
                        continue;
                    }
                }
                let h = i64::from(raw_h.min(4096));
                arena[idx].h = h;
                let order = arena[idx].order;
                queue.push(Reverse(Key {
                    parts: priority(
                        &query.objective,
                        arena[idx].actions,
                        arena[idx].depth,
                        arena[idx].state().now,
                        h + order,
                    ),
                    seq: idx,
                }));
                continue;
            }
            expansions += 1;
            if expansions > query.max_states {
                truncated = true;
                break;
            }
            let node = &arena[idx];
            let node_state = node.state().clone();

            // Goal on pop: with the heuristic off this is the optimal
            // witness for the objective's cost vector (§5.5).
            {
                *self.engine.state_mut() = node_state.clone();
                let events = node
                    .record
                    .as_ref()
                    .map(|r| r.events.clone())
                    .unwrap_or_default();
                let ctx = GoalCtx {
                    engine: &self.engine,
                    events: &events,
                };
                if query.goal.holds(&ctx) {
                    // DiverseWays counts structurally distinct recipe
                    // sequences: one witness per signature (§5.5).
                    if diverse && !witnessed_sigs.insert(arena[idx].sig.clone()) {
                        continue;
                    }
                    let records = path(&arena, idx);
                    // Adversarial witnesses carry no cumulative probability
                    // (§6); the other modes record the exact product.
                    let probability = if matches!(query.rng_mode, RngMode::Adversarial) {
                        None
                    } else {
                        Some(records.iter().map(|r| r.probability).product::<f64>())
                    };
                    let witness = self.coalesce(WitnessPath {
                        start: self.start.clone(),
                        records,
                        actions: node.actions,
                        depth: node.depth,
                        probability,
                    });
                    match replayer.replay(&witness, Some(&query.goal), query.invariant.as_ref()) {
                        Ok(_) => {
                            let w_cost = match &query.objective {
                                SearchObjective::FewestActions => {
                                    vec![witness.actions as i64, witness.final_clock()]
                                }
                                SearchObjective::EarliestTime => {
                                    vec![witness.final_clock(), witness.actions as i64]
                                }
                                SearchObjective::ShortestDepth
                                | SearchObjective::DiverseWays { .. } => {
                                    vec![witness.depth as i64]
                                }
                            };
                            if !diverse {
                                if incumbent_cost.as_ref().map_or(true, |best| w_cost < *best) {
                                    incumbent_cost = Some(w_cost);
                                    best_witness = Some(witness);
                                }
                                if !self.heuristic {
                                    break;
                                }
                            } else {
                                witnesses.push(witness);
                                if witnesses.len() >= max_ways {
                                    break;
                                }
                            }
                        }
                        Err(ReplayError::GoalUnmet) => {}
                        // A replay failure is an analyzer bug; the witness
                        // is dropped and counted (§5.6: every reported
                        // witness goes through the oracle).
                        Err(_) => dropped += 1,
                    }
                    continue;
                }
            }

            // Branch-and-bound pruning against incumbent witness
            if !diverse {
                if let Some(ref best) = incumbent_cost {
                    let cur_cost = match &query.objective {
                        SearchObjective::FewestActions => vec![node.actions as i64, node_state.now],
                        SearchObjective::EarliestTime => vec![node_state.now, node.actions as i64],
                        SearchObjective::ShortestDepth | SearchObjective::DiverseWays { .. } => {
                            vec![node.actions as i64]
                        }
                    };
                    if cur_cost >= *best {
                        continue;
                    }
                }
            }
            if node_state.ended.is_some() {
                continue; // terminal states have no outgoing transitions (T7)
            }
            let steps = {
                let _span = tracing::info_span!("gen_steps").entered();
                gen.steps(&node_state)
            };
            for step in steps {
                // Dead-mechanism deprioritization (§4 output 3): steps of
                // irrelevant verbs generate after relevant ones; insertion
                // order is the priority queue's final tie-break, so
                // deprioritization never prunes.
                let step_is_core = match &step {
                    PlayerStep::Commit { verb, .. } => relevant.contains(verb),
                    _ => true,
                };
                let order = if step_is_core { 0 } else { 1 };
                let records = gen.execute(&node_state, &step);
                for record in records {
                    let child_dist = arena[idx].dist_to_spine + 1;
                    if child_dist > max_radius {
                        frontier_boundary_edges += 1;
                        continue;
                    }
                    let state_after = record.state_after.clone();
                    if let Some(horizon) = query.max_time {
                        if state_after.now > horizon as i64 {
                            continue; // advance steps beyond the horizon are discarded (§5.5)
                        }
                    }
                    // The invariant gates every successor (§3.2).
                    if let Some(inv) = &query.invariant {
                        *self.engine.state_mut() = state_after.clone();
                        let ctx = GoalCtx {
                            engine: &self.engine,
                            events: &record.events,
                        };
                        if !inv.holds(&ctx) {
                            continue;
                        }
                    }
                    let key = {
                        let _span = tracing::info_span!("canonical_key").entered();
                        CanonicalStateKey::of_with(&state_after, &self.content, &self.symmetry)
                    };
                    let actions = arena[idx].actions + record.actions();
                    let depth = arena[idx].depth + record.depth();

                    time_min = time_min.min(state_after.now);
                    time_max = time_max.max(state_after.now);
                    if let Some(ending) = &state_after.ended {
                        endings_encountered.insert(ending.clone());
                    }
                    if let PlayerStep::Commit { verb, .. } = &record.step {
                        verbs_used.insert(verb.clone());
                    }
                    if let Some(r) = &record.recipe {
                        recipes_initiated.insert(r.clone());
                    }
                    for ev in &record.events {
                        if let story_engine::Event::SituationCompleted { recipe, .. } = ev {
                            recipes_completed.insert(recipe.clone());
                        }
                    }
                    // Visited rules (§5.5): the objective's cost vector is
                    // the dominance test; DiverseWays augments the key with
                    // the recipe-trace signature instead.
                    let sig = if diverse {
                        let mut s = arena[idx].sig.clone();
                        if let Some(r) = &record.recipe {
                            s.push(Some(r.clone()));
                        }
                        if !sig_visited.insert((key, s.clone())) {
                            continue;
                        }
                        s
                    } else {
                        let cost = match &query.objective {
                            SearchObjective::FewestActions => {
                                vec![actions as i64, state_after.now]
                            }
                            SearchObjective::EarliestTime => {
                                vec![state_after.now, actions as i64]
                            }
                            SearchObjective::ShortestDepth
                            | SearchObjective::DiverseWays { .. } => {
                                vec![depth as i64]
                            }
                        };
                        let dominated_by_cost = visited
                            .get(&key)
                            .is_some_and(|best| best.iter().zip(&cost).all(|(b, c)| b <= c));
                        let closer_to_spine = max_radius < usize::MAX
                            && min_dist.get(&key).map_or(true, |&d| child_dist < d);

                        if dominated_by_cost && !closer_to_spine {
                            continue;
                        }
                        if closer_to_spine {
                            min_dist.insert(key, child_dist);
                        }
                        let best = visited.entry(key).or_insert_with(|| cost.clone());
                        for (b, c) in best.iter_mut().zip(&cost) {
                            *b = (*b).min(*c);
                        }
                        Vec::new()
                    };
                    let gate_fired = query.prune
                        && !goal_cards.is_empty()
                        && goal_cards.iter().any(|c| {
                            !state_after.entities.values().any(|e| &e.prototype == c)
                                && !recipe_mintable.contains(c)
                        })
                        || (query.prune
                            && !scarcity.is_empty()
                            && scarcity.dead(&state_after, &self.content, query.max_time));
                    if gate_fired {
                        *self.engine.state_mut() = state_after.clone();
                        let ctx = GoalCtx {
                            engine: &self.engine,
                            events: &record.events,
                        };
                        if !query.goal.holds(&ctx) {
                            pruned += 1;
                            continue;
                        }
                    }
                    let (h, h_computed) = if self.heuristic || query.prune {
                        (arena[idx].h, false)
                    } else {
                        (0, true)
                    };
                    let child = arena.len();
                    arena.push(ArenaNode {
                        state: None,
                        parent: Some(idx),
                        record: Some(record),
                        actions,
                        depth,
                        sig,
                        dist_to_spine: child_dist,
                        h,
                        h_computed,
                        order,
                    });
                    queue.push(Reverse(Key {
                        parts: priority(
                            &query.objective,
                            actions,
                            depth,
                            state_after.now,
                            h + order,
                        ),
                        seq: child,
                    }));
                }
            }
        }

        if !diverse {
            if let Some(w) = best_witness {
                witnesses.push(w);
            }
        }

        let envelope = if query.envelope {
            let total_recipes: usize = self.content.verbs.values().map(|v| v.recipes.len()).sum();
            let recipe_coverage_ratio = if total_recipes > 0 {
                recipes_completed.len() as f64 / total_recipes as f64
            } else {
                0.0
            };
            let all_landmarks: BTreeSet<_> = self.causal.facts().cloned().collect();
            let total_causal_landmarks = all_landmarks.len();
            let mut grounded_facts = BTreeSet::new();
            for node in &arena {
                let st = node.state();
                *self.engine.state_mut() = st.clone();
                for f in self.causal.grounded_state_facts(st, &self.engine) {
                    if all_landmarks.contains(&f) {
                        grounded_facts.insert(f);
                    }
                }
            }

            let mut aspect_ranges: BTreeMap<(CardId, story_content::ir::AspectKey), (i64, i64)> =
                BTreeMap::new();
            for node in &arena {
                for ent in node.state().entities.values() {
                    for (k, val) in &ent.aspects {
                        let entry = aspect_ranges
                            .entry((ent.prototype.clone(), k.clone()))
                            .or_insert((*val, *val));
                        entry.0 = entry.0.min(*val);
                        entry.1 = entry.1.max(*val);
                    }
                }
            }

            let unique_canonical_states = if diverse {
                sig_visited.iter().map(|(k, _)| k).collect::<BTreeSet<_>>().len()
            } else {
                visited.len()
            };

            Some(crate::envelope::ExploredEnvelope {
                states_evaluated: expansions,
                unique_canonical_states,
                time_span: (time_min, time_max),
                verbs_used,
                recipes_initiated,
                recipes_completed,
                recipe_coverage_ratio,
                aspect_ranges,
                endings_encountered,
                causal_landmarks_reached: grounded_facts.len(),
                total_causal_landmarks,
                seed_trajectories: seed_paths.len(),
                exhaustive_radius: max_radius,
                frontier_boundary_edges,
            })
        } else {
            None
        };

        let mut resume_traces = Vec::new();
        if budget_depleted {
            let mut frontier_indices = Vec::new();
            if let Some(idx) = last_popped_idx {
                frontier_indices.push(idx);
            }
            while let Some(Reverse(item)) = queue.pop() {
                frontier_indices.push(item.seq);
                if frontier_indices.len() >= 50 {
                    break;
                }
            }
            frontier_indices.dedup();
            for f_idx in frontier_indices {
                if f_idx < arena.len() {
                    let records = path(&arena, f_idx);
                    let w = WitnessPath {
                        start: self.start.clone(),
                        records,
                        actions: arena[f_idx].actions,
                        depth: arena[f_idx].depth,
                        probability: None,
                    };
                    let trace = crate::trace::PlayTrace::from_witness(
                        &w,
                        self.engine.turn(),
                        self.seed,
                        self.legacy_in.clone(),
                    );
                    resume_traces.push(trace);
                }
            }
        }

        SolveOutcome {
            witnesses,
            truncated,
            budget_depleted,
            resume_traces,
            expansions,
            pruned,
            witnesses_dropped_by_the_oracle: dropped,
            envelope,
        }
    }

    /// The `h_add` value of the goal facts at a state (§4 output 4); a
    /// saturated estimate caps the penalty so the queue keeps a sensible
    /// order.
    /// Replay-verified coalescing of consecutive Advance records (§5.4):
    /// two records merge only if executing `advance(dt1 + dt2)` from the
    /// first record's entry state reproduces a canonical key equal to the
    /// second record's resulting state (and the same events).
    fn coalesce(&self, mut witness: WitnessPath) -> WitnessPath {
        let mut i = 0;
        while i + 1 < witness.records.len() {
            let (d1, d2) = match (&witness.records[i].step, &witness.records[i + 1].step) {
                (PlayerStep::Advance { dt: d1 }, PlayerStep::Advance { dt: d2 }) => (*d1, *d2),
                _ => {
                    i += 1;
                    continue;
                }
            };
            let a = witness.records[i].clone();
            let b = witness.records[i + 1].clone();
            let before = if i == 0 {
                witness.start.clone()
            } else {
                witness.records[i - 1].state_after.clone()
            };
            let mut outcomes = a.outcomes.clone();
            outcomes.extend(b.outcomes.iter().cloned());
            let mut engine = self.engine.clone();
            *engine.state_mut() = before;
            let run = if outcomes.is_empty() {
                engine.advance(d1 + d2)
            } else {
                let mut cursor = 0usize;
                let mut hook = |_: &ChancePoint| -> Option<ChanceOutcome> {
                    let o = outcomes.get(cursor).cloned();
                    if o.is_some() {
                        cursor += 1;
                    }
                    o
                };
                engine.advance_forked(d1 + d2, &mut hook)
            };
            let merged_ok = match run {
                Err(_) => false,
                Ok(events) => {
                    let key =
                        CanonicalStateKey::of_with(engine.state(), &self.content, &self.symmetry);
                    let want =
                        CanonicalStateKey::of_with(&b.state_after, &self.content, &self.symmetry);
                    events == b.events && key == want
                }
            };
            if merged_ok {
                let mut due = a.due_moments.clone();
                due.extend(b.due_moments.iter().copied());
                due.sort_unstable();
                due.dedup();
                witness.records[i] = TransitionRecord {
                    step: PlayerStep::Advance { dt: d1 + d2 },
                    outcomes,
                    probability: a.probability * b.probability,
                    due_moments: due,
                    events: b.events,
                    time_before: a.time_before,
                    time_after: b.time_after,
                    state_after: engine.state().clone(),
                    recipe: None,
                };
                witness.records.remove(i + 1);
                // The merged record may merge again with its successor.
            } else {
                i += 1;
            }
        }
        // The cumulative probability is value-preserving under coalescing
        // (the merged record's branch probability is the pair's product).
        witness.depth = witness.records.len();
        witness
    }
}

// ---------------------------------------------------------------------------
// The outcome type
// ---------------------------------------------------------------------------

/// The result of `solve`. Claims are bounds-conditional (§1.2): `truncated`
/// means the search hit `max_states` before exhausting the space, so "no
/// witness" is conditional on the bounds; an untruncated empty result is a
/// definitive unreachability answer over the explored space.
#[derive(Clone, Debug)]
pub struct SolveOutcome {
    pub witnesses: Vec<WitnessPath>,
    pub truncated: bool,
    pub budget_depleted: bool,
    pub resume_traces: Vec<crate::trace::PlayTrace>,
    pub expansions: usize,
    /// Branches skipped by the heuristic feasibility gate (`prune`): states
    /// whose goal facts cannot be grounded even in the delete relaxation.
    /// Witnesses are unaffected — they replay either way.
    pub pruned: usize,
    pub witnesses_dropped_by_the_oracle: usize,
    pub envelope: Option<crate::envelope::ExploredEnvelope>,
}

impl SolveOutcome {
    fn empty() -> SolveOutcome {
        SolveOutcome {
            witnesses: Vec::new(),
            truncated: false,
            budget_depleted: false,
            resume_traces: Vec::new(),
            expansions: 0,
            pruned: 0,
            witnesses_dropped_by_the_oracle: 0,
            envelope: None,
        }
    }

    /// Whether the query is answered definitively within the bounds (a
    /// witness exists, or the space was exhausted without one).
    pub fn conclusive(&self) -> bool {
        !self.truncated && self.pruned == 0
    }
}
