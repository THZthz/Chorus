//! The Tier 2 transition model (story-analyze spec §5.1, §5.4): the legal
//! player steps at a state — commits with canonical binding sets, mid-run
//! binds, the atomic `advance(turn)`, and event-driven time jumps to the
//! next critical moment — and their execution into `TransitionRecord`s
//! through a real kernel engine. Branching modes fork chance points through
//! the forking settlement face (`stochastic`); this module consumes it.

use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::sync::Arc;

use story_content::ir::{
    const_eval, ArithOp, AspectKey, CmpOp, EffectNode, Expr, LogicOp, RecipeDecl, SlotDecl,
    VerbMode,
};
use story_content::Content;
use story_core::{EntityId, RecipeId, SlotName, State, Time, VerbId};
use story_engine::{CommitAction, Engine, Event};

use crate::query::{GoalCtx, GoalPredicate, RngMode};
use crate::stochastic;
use crate::symmetry::{self, BindingSet, SymmetryCache};

/// One legal player step (story-analyze spec §5.1).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PlayerStep {
    Commit {
        verb: VerbId,
        bindings: BindingSet,
    },
    Bind {
        sit: story_core::SituationId,
        slot: SlotName,
        entity: EntityId,
    },
    Advance {
        dt: Time,
    },
}

/// One executed transition (story-analyze spec §5.1): the step, the recorded
/// chance outcomes (branching modes replay them through the forking face),
/// the event stream, the clock before and after, and the resulting state.
#[derive(Clone, Debug)]
pub struct TransitionRecord {
    pub step: PlayerStep,
    /// The chance outcomes this settlement took, in firing order (empty in
    /// `Deterministic`).
    pub outcomes: Vec<story_engine::ChanceOutcome>,
    /// Exact probability of this settlement's outcome branch (1.0 in
    /// `Deterministic`).
    pub probability: f64,
    pub events: Vec<Event>,
    pub time_before: Time,
    pub time_after: Time,
    pub state_after: State,
    /// The recipe the step engaged: the commit's selection, the bound
    /// situation's recipe, or `None` for an advance.
    pub recipe: Option<RecipeId>,
    /// For an advance: the due moments (live situations' `ends_at`, unlocked
    /// entities' `expires_at`) the kernel settled strictly inside the span.
    /// The kernel's advance accepts only positive turn multiples, so a due
    /// moment off the turn grid cannot be a landing; these are the moments
    /// whose settlement the endpoint state already includes while no
    /// endpoint sits on them (§5.4). Empty for commits and binds.
    pub due_moments: Vec<Time>,
}

impl TransitionRecord {
    /// Commits + binds along this record's step (advances are free).
    pub fn actions(&self) -> usize {
        match self.step {
            PlayerStep::Advance { .. } => 0,
            _ => 1,
        }
    }

    /// The transition count this record contributes.
    pub fn depth(&self) -> usize {
        1
    }
}

/// A deterministic witness path (story-analyze spec §5.1): the transition
/// records (advances coalesced per §5.4), the start state, the action count,
/// the transition depth, the final clock, and the cumulative path
/// probability — exact under `Deterministic` (1.0) and
/// `OptimisticBranching`; `None` under `Adversarial`, whose witnesses carry
/// no cumulative probability (§6).
#[derive(Clone, Debug)]
pub struct WitnessPath {
    pub start: State,
    pub records: Vec<TransitionRecord>,
    pub actions: usize,
    pub depth: usize,
    pub probability: Option<f64>,
}

impl WitnessPath {
    /// The witness's final state.
    pub fn final_state(&self) -> &State {
        self.records
            .last()
            .map(|r| &r.state_after)
            .unwrap_or(&self.start)
    }

    /// The final clock.
    pub fn final_clock(&self) -> Time {
        self.final_state().now
    }

    /// The recipe sequence the witness engages, in order (the DiverseWays
    /// trace signature, §5.5).
    pub fn recipe_trace(&self) -> Vec<Option<RecipeId>> {
        self.records.iter().map(|r| r.recipe.clone()).collect()
    }

    /// Every event stream of the witness, flattened in order.
    pub fn events(&self) -> impl Iterator<Item = &Event> {
        self.records.iter().flat_map(|r| r.events.iter())
    }
}

/// The generator configuration.
#[derive(Clone)]
pub struct GeneratorConfig {
    /// Content-fixed or host seed; the engine boots with it and the replay
    /// oracle must boot with the same.
    pub seed: u64,
    /// Legacy overrides at boot; the replay oracle must boot with the same.
    pub legacy_in: Option<BTreeMap<String, i64>>,
    pub rng_mode: RngMode,
    pub allow_mid_run_bind: bool,
    /// Clock horizon; advance steps past it are discarded (§5.5).
    pub max_time: Option<Time>,
    /// The invariant every successor state must hold (§3.2); violating
    /// successors are discarded.
    pub invariant: Option<GoalPredicate>,
    /// Extra static critical moments: the query's own `clock` thresholds.
    pub query_thresholds: BTreeSet<Time>,
    /// Verbs permitted in this exploration; if `Some`, commits and binds
    /// outside this set are skipped.
    pub allowed_verbs: Option<BTreeSet<VerbId>>,
    /// Suppress advance steps when the state is completely quiescent.
    pub quiescence_pruning: bool,
}

/// The Tier 2 transition generator: enumerates the legal steps at a state
/// and executes them through the kernel. One scratch engine is reused; the
/// states swap in through `Engine::state_mut`, so no engine clone happens in
/// the search inner loop.
pub struct TransitionGenerator {
    content: Arc<Content>,
    engine: Engine,
    config: GeneratorConfig,
    /// Static critical moments: every `clock` threshold extracted from the
    /// content's recipe/card/rule `when`s, guards, and `Branch` conditions
    /// (§5.4), plus the query's thresholds.
    crit_moments: BTreeSet<Time>,
    symmetry: Arc<SymmetryCache>,
    committable_verbs: Vec<VerbId>,
    craftable_recipes: BTreeMap<VerbId, Vec<RecipeId>>,
    recipe_required_aspects: BTreeMap<RecipeId, Vec<AspectKey>>,
}

impl TransitionGenerator {
    /// Build a generator over compiled content. Boots one scratch engine
    /// (the start state is swapped in per call).
    pub fn new(content: Arc<Content>, engine: Engine, config: GeneratorConfig) -> TransitionGenerator {
        let symmetry = Arc::new(SymmetryCache::new(&content));
        Self::with_symmetry(content, engine, config, symmetry)
    }

    /// Build a generator reusing a caller-held symmetry cache so content-wide
    /// surfaces are not re-walked.
    pub fn with_symmetry(
        content: Arc<Content>,
        engine: Engine,
        config: GeneratorConfig,
        symmetry: Arc<SymmetryCache>,
    ) -> TransitionGenerator {
        let mut crit_moments = static_thresholds(&content);
        crit_moments.extend(config.query_thresholds.iter().copied());

        let mut committable_verbs = Vec::new();
        let mut craftable_recipes = BTreeMap::new();
        let mut recipe_required_aspects = BTreeMap::new();
        let mut verbs: Vec<&story_content::ir::VerbDecl> = content.verbs.values().collect();
        verbs.sort_by_key(|v| v.decl_index);

        for verb in verbs {
            if verb.mode != VerbMode::Committable {
                continue;
            }
            committable_verbs.push(verb.id.clone());
            let mut recipes: Vec<&RecipeDecl> = verb
                .recipe_order
                .iter()
                .filter_map(|n| content.recipe(&format!("{}.{}", verb.id, n)))
                .filter(|r| r.craftable && !r.chain_only)
                .collect();
            recipes.sort_by_key(|r| (r.priority, r.decl_index));
            for r in &recipes {
                if let Some(w) = &r.when {
                    let mut reqs = BTreeSet::new();
                    extract_required_aspects(w, &content, &mut reqs);
                    if !reqs.is_empty() {
                        recipe_required_aspects.insert(r.id.clone(), reqs.into_iter().collect());
                    }
                }
            }
            craftable_recipes.insert(
                verb.id.clone(),
                recipes.into_iter().map(|r| r.id.clone()).collect(),
            );
        }

        TransitionGenerator {
            content,
            engine,
            config,
            crit_moments,
            symmetry,
            committable_verbs,
            craftable_recipes,
            recipe_required_aspects,
        }
    }

    pub fn content(&self) -> &Arc<Content> {
        &self.content
    }

    pub fn config(&self) -> &GeneratorConfig {
        &self.config
    }

    /// The static critical moments (§5.4).
    pub fn crit_moments(&self) -> &BTreeSet<Time> {
        &self.crit_moments
    }

    fn install(&mut self, state: &State) {
        self.engine.state_mut().clone_from(state);
    }

    /// The scratch engine with the last installed state (the evaluation
    /// face for goal predicates inside the solver engines).
    pub fn engine(&self) -> &Engine {
        &self.engine
    }

    /// Install a state without enumerating (evaluation-only use).
    pub fn install_state(&mut self, state: &State) {
        self.install(state);
    }

    /// The installed state.
    pub fn state(&self) -> &State {
        self.engine.state()
    }

    /// The legal steps at `state` (§5.1). An ended state is terminal (T7).
    pub fn steps(&mut self, state: &State) -> Vec<PlayerStep> {
        if state.ended.is_some() {
            return Vec::new();
        }
        self.install(state);
        let state_classes = symmetry::symmetry_classes_with(state, self.content.as_ref(), &self.symmetry);
        let mut steps = Vec::new();
        {
            let _span = tracing::info_span!("commit_steps").entered();
            steps.extend(self.commit_steps(state, &state_classes));
        }
        if self.config.allow_mid_run_bind {
            let _span = tracing::info_span!("bind_steps").entered();
            steps.extend(self.bind_steps(&state_classes));
        }
        {
            let _span = tracing::info_span!("advance_steps").entered();
            steps.extend(self.advance_steps(state));
        }
        steps
    }

    fn bindable_entities(state: &State) -> Vec<EntityId> {
        let mut out = Vec::new();
        for (&eid, ent) in &state.entities {
            let Some(z) = state.zones.get(&ent.zone) else {
                continue;
            };
            if !z.bindable || symmetry::is_bound(state, eid) {
                continue;
            }
            out.push(eid);
        }
        out
    }

    /// Commit steps: every committable verb below its instance cap, every
    /// craftable recipe whose `max` counter is unspent, every canonical
    /// binding set, validated with `preview_commit` and deduplicated.
    fn commit_steps(&self, state: &State, state_classes: &[Vec<EntityId>]) -> Vec<PlayerStep> {
        let content = &self.content;
        let mut steps = Vec::new();
        let bindable = Self::bindable_entities(state);
        let mut query_cache: BTreeMap<(usize, usize), Vec<EntityId>> = BTreeMap::new();
        let greedy_pool = self.engine.greedy_pool();

        let present_aspects: HashSet<&AspectKey> = state
            .entities
            .values()
            .flat_map(|e| e.aspects.iter().filter(|(_, &v)| v > 0).map(|(k, _)| k))
            .collect();

        for verb_id in &self.committable_verbs {
            if let Some(allowed) = &self.config.allowed_verbs {
                if !allowed.contains(verb_id) {
                    continue;
                }
            }
            let Some(verb) = content.verbs.get(verb_id.as_str()) else {
                continue;
            };
            // Instance cap: one refusal of that kind skips the verb.
            if self.verb_full(verb) {
                continue;
            }
            let Some(recipe_ids) = self.craftable_recipes.get(verb_id) else {
                continue;
            };
            let mut seen_bindings: BTreeSet<BindingSet> = BTreeSet::new();
            for rec_id in recipe_ids {
                if let Some(reqs) = self.recipe_required_aspects.get(rec_id) {
                    if reqs.iter().any(|a| !present_aspects.contains(a)) {
                        continue;
                    }
                }
                let Some(rec) = content.recipe(rec_id.as_str()) else {
                    continue;
                };
                if rec.max.is_some_and(|m| state.recipe_runs.get(&rec.id).copied().unwrap_or(0) >= m)
                {
                    continue;
                }
                for bindings in self.commit_binding_sets(
                    rec,
                    state_classes,
                    &bindable,
                    &mut query_cache,
                    &greedy_pool,
                ) {
                    if seen_bindings.contains(&bindings) {
                        continue;
                    }
                    if self
                        .engine
                        .preview_commit(&CommitAction {
                            verb: verb.id.clone(),
                            bindings: bindings.clone(),
                        })
                        .is_ok()
                    {
                        seen_bindings.insert(bindings.clone());
                        steps.push(PlayerStep::Commit {
                            verb: verb.id.clone(),
                            bindings,
                        });
                    } else {
                        seen_bindings.insert(bindings);
                    }
                }
            }
        }
        steps
    }

    fn verb_full(&self, verb: &story_content::ir::VerbDecl) -> bool {
        match verb.instances {
            story_content::ir::InstanceCap::Any => false,
            story_content::ir::InstanceCap::AtMost(n) => self
                .engine
                .state()
                .situations
                .values()
                .filter(|s| s.verb == verb.id)
                .count()
                >= n as usize,
        }
    }

    /// The canonical binding sets of one recipe (§5.2): per slot, the
    /// equivalence-class prefixes of the candidate pool; across slots, the
    /// Cartesian product without reuse. Greedy slots baseline at exactly
    /// `min` explicit bindings, with one displacement variant per
    /// equivalence class whose representative the greedy pull cannot reach.
    /// Internal slots are engine machinery, never player choices: a
    /// `min >= 1` internal slot contributes exactly one choice — the
    /// host-staged fill of the first `min` kernel candidates the set under
    /// construction does not already use — and a `min = 0` internal slot
    /// contributes the empty binding (no branching).
    fn commit_binding_sets(
        &self,
        rec: &RecipeDecl,
        state_classes: &[Vec<EntityId>],
        bindable: &[EntityId],
        query_cache: &mut BTreeMap<(usize, usize), Vec<EntityId>>,
        greedy_pool: &[EntityId],
    ) -> Vec<BindingSet> {
        let mut per_slot: Vec<(SlotName, SlotChoices)> = Vec::new();
        for sd in &rec.slots {
            if sd.internal {
                // The kernel's deterministic candidate list is the same one
                // commit materialization stages from; presence-blind and
                // mechanical.
                let candidates = self
                    .engine
                    .slot_candidates(rec.id.clone(), sd.name.clone());
                if candidates.len() < sd.min as usize {
                    // No host-staged fill can ever be built; the recipe's
                    // candidacy dies.
                    return Vec::new();
                }
                per_slot.push((
                    sd.name.clone(),
                    SlotChoices::Internal {
                        min: sd.min,
                        candidates,
                    },
                ));
                continue;
            }
            let candidates = self.slot_pool(sd, bindable, query_cache);
            if candidates.len() < sd.min as usize {
                // A slot that cannot be filled kills the recipe's candidacy.
                return Vec::new();
            }
            let slot_classes = symmetry::partition_candidates_by_classes(state_classes, &candidates);
            let choices = if sd.greedy {
                Self::greedy_slot_choices(sd, &slot_classes, &candidates, greedy_pool)
            } else {
                symmetry::slot_choices_from_classes(&slot_classes, sd.min, sd.max)
            };
            if choices.is_empty() {
                return Vec::new();
            }
            per_slot.push((sd.name.clone(), SlotChoices::Player(choices)));
        }
        // Cross-slot product without reuse (§5.2).
        let mut out = Vec::new();
        let mut acc: BindingSet = Vec::new();
        product_slots(&per_slot, 0, &mut acc, &mut out);
        out
    }

    /// The commit candidates of one slot: alive, unlocked, in a bindable
    /// zone, filter-matching (the checks `preview_commit` applies). The
    /// candidate set of a commit is wider than the greedy pull's public
    /// pool: any bindable zone qualifies.
    fn slot_pool(
        &self,
        sd: &SlotDecl,
        bindable: &[EntityId],
        query_cache: &mut BTreeMap<(usize, usize), Vec<EntityId>>,
    ) -> Vec<EntityId> {
        let key = (sd.query.as_ptr() as usize, sd.query.len());
        if let Some(cached) = query_cache.get(&key) {
            return cached.clone();
        }
        let mut out = Vec::new();
        for &eid in bindable {
            if self.engine.matches(eid, &sd.query) {
                out.push(eid);
            }
        }
        out.sort_unstable();
        query_cache.insert(key, out.clone());
        out
    }

    fn greedy_slot_choices(
        sd: &SlotDecl,
        classes: &[Vec<EntityId>],
        candidates: &[EntityId],
        greedy_pool: &[EntityId],
    ) -> Vec<Vec<EntityId>> {
        // Baseline: exactly `min` explicit bindings; the engine's greedy
        // top-up produces the same post-materialization state as
        // pre-filling. An open-ended greedy slot pulls nothing on a commit
        // (the kernel tops toward `max` only), so the baseline is the
        // whole story for it.
        let mut out = symmetry::slot_choices_from_classes(classes, sd.min, Some(sd.min));
        // Displacement variants (§5.2): the player forces a card the
        // greedy pull cannot reach. One variant per distinct equivalence
        // class, emitted only where the single explicit binding is
        // affordable (it must satisfy the slot minimum on its own). The
        // pull depth is the kernel's top-up target: `max`, or nothing for
        // an open-ended slot. The expansion budget is the only global cap.
        if sd.min > 1 {
            return out;
        }
        let pull_depth = sd.max.map(|m| m as usize).unwrap_or(0);
        let reachable: BTreeSet<EntityId> = if pull_depth > 0 {
            greedy_pool
                .iter()
                .filter(|e| candidates.binary_search(e).is_ok())
                .take(pull_depth)
                .copied()
                .collect()
        } else {
            BTreeSet::new()
        };
        for class in classes {
            let rep = class[0];
            if !reachable.contains(&rep) {
                out.push(vec![rep]);
            }
        }
        out
    }

    /// Bind steps: every live situation, every `open` slot below `max`, one
    /// canonical representative per equivalence class passing `can_bind`
    /// (§5.1.2, §5.2).
    fn bind_steps(&self, state_classes: &[Vec<EntityId>]) -> Vec<PlayerStep> {
        let mut steps = Vec::new();
        let state = self.engine.state();
        let content = &self.content;
        let sits: Vec<story_core::SituationId> = state.situations.keys().copied().collect();
        for sit in sits {
            let situation = &state.situations[&sit];
            if let Some(allowed) = &self.config.allowed_verbs {
                if !allowed.contains(&situation.verb) {
                    continue;
                }
            }
            let Some(rec) = content.recipe(situation.recipe.as_str()) else {
                continue;
            };
            for sd in &rec.slots {
                if !sd.open {
                    continue;
                }
                let bound = situation.bindings.get(&sd.name).map(|b| b.len()).unwrap_or(0);
                if let Some(max) = sd.max {
                    if bound >= max as usize {
                        continue;
                    }
                }
                let candidates: Vec<EntityId> = state
                    .entities
                    .keys()
                    .copied()
                    .filter(|e| self.engine.can_bind(sit, sd.name.clone(), *e).is_ok())
                    .collect();
                for class in symmetry::partition_candidates_by_classes(state_classes, &candidates) {
                    steps.push(PlayerStep::Bind {
                        sit,
                        slot: sd.name.clone(),
                        entity: class[0],
                    });
                }
            }
        }
        steps
    }

    /// Advance steps (§5.4): the atomic `advance(turn)` — always legal at a
    /// live state — plus the jump walk to the next critical moment when it
    /// lies beyond one turn. The walk emits one step per landing: every due
    /// moment strictly inside the span lands on its own step — exactly on
    /// the moment when it sits on the turn grid, else on its enclosing grid
    /// point (the kernel's advance accepts only positive turn multiples,
    /// `dt = k·turn`) — and the walk continues to the aligned grid target
    /// as its own step. A jump never lands past the nearest due moment.
    fn advance_steps(&mut self, state: &State) -> Vec<PlayerStep> {
        if self.config.quiescence_pruning && self.is_quiescent(state) {
            return Vec::new();
        }
        let turn = self.engine.turn();
        if let Some(h) = self.config.max_time {
            if state.now + turn > h {
                return Vec::new();
            }
        }
        let mut steps = vec![PlayerStep::Advance { dt: turn }];
        let Some(t) = self.next_critical_moment(state) else {
            return steps;
        };
        let raw = t - state.now;
        let target = state.now + (raw + turn - 1) / turn * turn;
        // Due moments inside the span become landings of their own (§5.4):
        // the moment itself on the grid, its enclosing grid point otherwise.
        let mut landings = vec![target];
        for m in due_moments(state) {
            if m > state.now && m < target {
                landings.push(state.now + (m - state.now + turn - 1) / turn * turn);
            }
        }
        landings.sort_unstable();
        landings.dedup();
        let mut prev = state.now;
        for landing in landings {
            if landing <= prev {
                continue;
            }
            let within_horizon = self
                .config
                .max_time
                .is_none_or(|h| landing <= h);
            if !within_horizon {
                break;
            }
            steps.push(PlayerStep::Advance { dt: landing - prev });
            prev = landing;
        }
        steps
    }

    /// A state is quiescent if advancing the clock cannot cause any state change:
    /// no situation is running (no completions due), no entity has an expiration timer,
    /// no autonomous verb can start a recipe, and no static critical moment exists in the future.
    pub fn is_quiescent(&mut self, state: &State) -> bool {
        if !due_moments(state).is_empty() {
            return false;
        }
        if self.can_autonomous_start(state) {
            return false;
        }
        if self.crit_moments.iter().any(|&t| t > state.now) {
            return false;
        }
        true
    }

    fn can_autonomous_start(&mut self, state: &State) -> bool {
        self.install(state);
        self.engine.can_autonomous_start(self.config.allowed_verbs.as_ref())
    }

    /// The nearest critical moment past `now + turn`: the next due
    /// `ends_at`/`expires_at`, or the next static `clock` threshold (§5.4).
    pub fn next_critical_moment(&self, state: &State) -> Option<Time> {
        let mut best: Option<Time> = None;
        let mut consider = |t: Time| {
            if t > state.now {
                best = Some(best.map_or(t, |b: Time| b.min(t)));
            }
        };
        for s in state.situations.values() {
            consider(s.ends_at);
        }
        for e in state.entities.values() {
            if let Some(at) = e.expires_at {
                consider(at);
            }
        }
        for &t in &self.crit_moments {
            consider(t);
        }
        // A moment within one turn is handled by the atomic step.
        best.filter(|&t| t > state.now + self.engine.turn())
    }

    /// Execute one step, returning every transition record the RNG mode
    /// produces (§6): one in `Deterministic`, the enumerated outcome classes
    /// in the branching modes. Refused actions produce no record (T3); an
    /// effect failure inside a successful action is the action's event
    /// traffic and stays a record. Advance records are marked with the due
    /// moments settled strictly inside their span (§5.4), and every record
    /// must hold the configured invariant — at its endpoint and, for a
    /// marked advance, at each marked moment's enclosing-grid window
    /// checkpoint; violating records are discarded (§3.2).
    pub fn execute(&mut self, state: &State, step: &PlayerStep) -> Vec<TransitionRecord> {
        let _span = tracing::info_span!("transition_exec").entered();
        if state.ended.is_some() {
            return Vec::new();
        }
        let recipe = self.engaged_recipe(state, step);
        let settlements = stochastic::settlements(
            &self.content,
            &self.engine,
            self.config.seed,
            state,
            step,
            self.config.rng_mode,
        );
        let advance = matches!(step, PlayerStep::Advance { .. });
        let mut records: Vec<TransitionRecord> = settlements
            .into_iter()
            .filter_map(|s| {
                s.result.ok().map(|(events, state_after)| TransitionRecord {
                    step: step.clone(),
                    outcomes: s.outcomes,
                    probability: s.probability,
                    due_moments: if advance {
                        due_moments(state)
                            .into_iter()
                            .filter(|&m| m > state.now && m < state_after.now)
                            .collect()
                    } else {
                        Vec::new()
                    },
                    events,
                    time_before: state.now,
                    time_after: state_after.now,
                    state_after,
                    recipe: recipe.clone(),
                })
            })
            .collect();
        if let Some(inv) = self.config.invariant.clone() {
            let mut kept: Vec<TransitionRecord> = Vec::new();
            for record in records {
                if self.invariant_safe(state, &record, &inv) {
                    kept.push(record);
                }
            }
            records = kept;
        }
        records
    }

    /// The invariant gate over one record (§3.2, §5.4): the endpoint must
    /// hold the invariant, and every due moment marked on an advance must
    /// hold it at its enclosing grid point — materialized by a sub-advance
    /// from the record's entry state (the closest checkpoint the kernel's
    /// `dt = k·turn` rule admits; it coincides with the endpoint for the
    /// jumps the generator emits, since those already land on the enclosing
    /// grid point of the nearest due moment).
    fn invariant_safe(
        &mut self,
        state: &State,
        record: &TransitionRecord,
        inv: &GoalPredicate,
    ) -> bool {
        if !self.holds(&record.state_after, inv, &record.events) {
            return false;
        }
        let turn = self.engine.turn();
        for &m in &record.due_moments {
            let g = state.now + (m - state.now + turn - 1) / turn * turn;
            if g >= record.time_after {
                continue; // the endpoint checkpoint already covers it
            }
            self.install(state);
            if self.engine.advance(g - state.now).is_err() {
                return false;
            }
            let window = self.engine.state().clone();
            if !self.holds(&window, inv, &record.events) {
                return false;
            }
        }
        true
    }

    /// Evaluate a predicate with the given state installed.
    fn holds(&mut self, state: &State, inv: &GoalPredicate, events: &[Event]) -> bool {
        self.install(state);
        let ctx = GoalCtx {
            engine: self.engine(),
            events,
        };
        inv.holds(&ctx)
    }

    /// The recipe a step engages (the DiverseWays trace signature entry).
    fn engaged_recipe(&mut self, state: &State, step: &PlayerStep) -> Option<RecipeId> {
        match step {
            PlayerStep::Commit { verb, bindings } => {
                self.install(state);
                self.engine
                    .preview_commit(&CommitAction {
                        verb: verb.clone(),
                        bindings: bindings.clone(),
                    })
                    .ok()
            }
            PlayerStep::Bind { sit, .. } => state.situations.get(sit).map(|s| s.recipe.clone()),
            PlayerStep::Advance { .. } => None,
        }
    }
}

/// One slot's contribution to the cross-slot product: the player choices of
/// a visible slot, or the single host-staged fill of an internal one.
enum SlotChoices {
    Player(Vec<Vec<EntityId>>),
    Internal {
        min: u32,
        candidates: Vec<EntityId>,
    },
}

/// The cross-slot product of per-slot choice lists without reuse: a later
/// slot's picks never intersect an earlier slot's (§5.2). An internal slot
/// takes its single host-staged fill here against the set under
/// construction; a branch that leaves fewer than `min` candidates yields no
/// sets.
fn product_slots(
    per_slot: &[(SlotName, SlotChoices)],
    idx: usize,
    acc: &mut BindingSet,
    out: &mut Vec<BindingSet>,
) {
    if idx == per_slot.len() {
        out.push(acc.clone());
        return;
    }
    let (slot, choices) = &per_slot[idx];
    match choices {
        SlotChoices::Player(cs) => {
            for choice in cs {
                if choice.iter().any(|e| acc.iter().any(|(_, b)| b == e)) {
                    continue;
                }
                let mark = acc.len();
                acc.extend(choice.iter().map(|e| (slot.clone(), *e)));
                product_slots(per_slot, idx + 1, acc, out);
                acc.truncate(mark);
            }
        }
        SlotChoices::Internal { min, candidates } => {
            let min = *min as usize;
            let mut fill: Vec<EntityId> = Vec::with_capacity(min);
            for c in candidates {
                if fill.len() == min {
                    break;
                }
                if acc.iter().any(|(_, b)| b == c) {
                    continue;
                }
                fill.push(*c);
            }
            if fill.len() < min {
                // This branch leaves fewer candidates than the slot needs:
                // the commit cannot be built here.
                return;
            }
            let mark = acc.len();
            acc.extend(fill.into_iter().map(|e| (slot.clone(), e)));
            product_slots(per_slot, idx + 1, acc, out);
            acc.truncate(mark);
        }
    }
}

/// The due moments of `state` (§5.4): every live situation's `ends_at` and
/// every unlocked entity's `expires_at` — the moments the kernel settles
/// inside an advance. A bound (locked) card's timer is deferred while
/// locked (spec.md §4.6), so its expiry is not due.
fn due_moments(state: &State) -> Vec<Time> {
    let mut out: Vec<Time> = state.situations.values().map(|s| s.ends_at).collect();
    out.extend(
        state
            .entities
            .values()
            .filter(|e| e.expires_at.is_some() && !symmetry::is_bound(state, e.id))
            .map(|e| e.expires_at.unwrap()),
    );
    out.sort_unstable();
    out.dedup();
    out
}

/// Every static `clock` threshold in the content (§5.4): recipe/card/rule
/// `when`s, guards, and `Branch` conditions.
fn static_thresholds(content: &Content) -> BTreeSet<Time> {
    let mut out = BTreeSet::new();
    let mut cond = |e: &Expr| crate::query::clock_thresholds(e, &mut out);
    for verb in content.verbs.values() {
        for rec in verb.recipes.values() {
            if let Some(w) = &rec.when {
                cond(w);
            }
            for g in &rec.guards {
                cond(&g.when);
            }
            walk_conds(&rec.effects, &mut cond);
            if let Some(book) = &rec.on_complete {
                for r in &book.rules {
                    if let Some(w) = &r.when {
                        cond(w);
                    }
                    walk_conds(&r.effects, &mut cond);
                }
            }
        }
    }
    for card in content.cards.values() {
        for (_, book) in &card.on_complete {
            for r in &book.rules {
                if let Some(w) = &r.when {
                    cond(w);
                }
                walk_conds(&r.effects, &mut cond);
            }
        }
        if let Some(book) = &card.on_expire {
            for r in &book.rules {
                if let Some(w) = &r.when {
                    cond(w);
                }
                walk_conds(&r.effects, &mut cond);
            }
        }
    }
    for book in content.events.values() {
        for r in &book.rules {
            if let Some(w) = &r.when {
                cond(w);
            }
            walk_conds(&r.effects, &mut cond);
        }
    }
    out
}

fn walk_conds(nodes: &[EffectNode], cond: &mut impl FnMut(&Expr)) {
    for n in nodes {
        if let EffectNode::Branch { cond: c, then, els } = n {
            cond(c);
            walk_conds(then, cond);
            walk_conds(els, cond);
        }
    }
}

fn extract_single_aspect_key<'a>(expr: &'a Expr) -> Option<&'a str> {
    let mut stack = vec![expr];
    let mut expected_key: Option<&'a str> = None;

    while let Some(e) = stack.pop() {
        match e {
            Expr::Aspect { key, .. } | Expr::NamePath { key, .. } => {
                match expected_key {
                    None => expected_key = Some(key.as_str()),
                    Some(k) if k == key.as_str() => {}
                    _ => return None,
                }
            }
            Expr::Arith {
                op: ArithOp::Add,
                left,
                right,
            } => {
                stack.push(right);
                stack.push(left);
            }
            _ => return None,
        }
    }

    expected_key
}

fn extract_required_aspects(
    expr: &Expr,
    content: &Content,
    reqs: &mut BTreeSet<AspectKey>,
) {
    match expr {
        Expr::Logic {
            op: LogicOp::And,
            left,
            right,
        } => {
            extract_required_aspects(left, content, reqs);
            extract_required_aspects(right, content, reqs);
        }
        Expr::Logic {
            op: LogicOp::Or,
            left,
            right,
        } => {
            let mut left_reqs = BTreeSet::new();
            extract_required_aspects(left, content, &mut left_reqs);
            let mut right_reqs = BTreeSet::new();
            extract_required_aspects(right, content, &mut right_reqs);
            reqs.extend(left_reqs.intersection(&right_reqs).cloned());
        }
        Expr::Cmp { op, left, right } => {
            let (target_expr, min_val) = match op {
                CmpOp::Ge => (Some(left.as_ref()), const_eval(right)),
                CmpOp::Gt => (
                    Some(left.as_ref()),
                    const_eval(right).and_then(|v| v.checked_add(1)),
                ),
                CmpOp::Le => (Some(right.as_ref()), const_eval(left)),
                CmpOp::Lt => (
                    Some(right.as_ref()),
                    const_eval(left).and_then(|v| v.checked_add(1)),
                ),
                _ => (None, None),
            };
            if let (Some(target), Some(min)) = (target_expr, min_val) {
                if min >= 1 {
                    if let Some(key) = extract_single_aspect_key(target) {
                        let default = content.aspects.get(key).map_or(0, |d| d.default);
                        if default <= 0 {
                            reqs.insert(key.to_string());
                        }
                    }
                }
            }
        }
        _ => {}
    }
}
