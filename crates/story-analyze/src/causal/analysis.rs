//! Search support (story-analyze spec §4 outputs 3 and 4): the delete-relaxation heuristic, grounded state facts, backward cones, and verb relevance.

use std::collections::{BTreeMap, BTreeSet};

use story_content::ir::{CmpOp, QueryTerm};
use story_content::Content;
use story_core::{CardId, RecipeId, VerbId};

use super::fact::{Fact, Hyperedge, Site};
use super::CausalGraph;

// ---------------------------------------------------------------------------
// Search support (story-analyze spec §4 outputs 3 and 4): the delete-
// relaxation heuristic and dead-mechanism pruning.
// ---------------------------------------------------------------------------

#[derive(Debug, Default)]
pub(crate) struct IndexedCausal {
    pub fact_to_id: BTreeMap<Fact, u32>,
    pub edge_produces: Vec<Vec<u32>>,
    pub edge_recipe_max: Vec<Option<RecipeId>>,
    pub consumers: Vec<Vec<u32>>,
    pub aspect_bridge: Vec<Option<u32>>,
    pub card_live_bridges: Vec<Vec<u32>>,
    pub base_unsatisfied: Vec<usize>,
    pub zero_req_edges: Vec<u32>,
}

const MAX_BUCKETS: usize = 2048;
const MAX_COST: usize = MAX_BUCKETS - 1;

pub(crate) struct BucketQueue {
    buckets: Vec<Vec<u32>>,
    min_bucket: usize,
    max_bucket: usize,
    count: usize,
}

impl BucketQueue {
    pub fn new() -> Self {
        Self {
            buckets: vec![Vec::new(); MAX_BUCKETS],
            min_bucket: usize::MAX,
            max_bucket: 0,
            count: 0,
        }
    }

    #[inline]
    pub fn clear(&mut self) {
        if self.min_bucket <= self.max_bucket {
            let start = self.min_bucket;
            let end = self.max_bucket.min(self.buckets.len() - 1);
            if start <= end {
                for b in &mut self.buckets[start..=end] {
                    b.clear();
                }
            }
        }
        self.min_bucket = usize::MAX;
        self.max_bucket = 0;
        self.count = 0;
    }

    #[inline]
    pub fn push(&mut self, cost: u32, fid: u32) {
        let c = (cost as usize).min(MAX_COST);
        self.buckets[c].push(fid);
        if c < self.min_bucket {
            self.min_bucket = c;
        }
        if c > self.max_bucket {
            self.max_bucket = c;
        }
        self.count += 1;
    }

    #[inline]
    pub fn pop(&mut self) -> Option<(u32, u32)> {
        if self.count == 0 {
            return None;
        }
        loop {
            if let Some(fid) = self.buckets[self.min_bucket].pop() {
                self.count -= 1;
                return Some((self.min_bucket as u32, fid));
            }
            self.min_bucket += 1;
            if self.min_bucket > self.max_bucket {
                return None;
            }
        }
    }
}

pub(crate) struct HAddWorkspace {
    dist: Vec<u32>,
    finalized: Vec<bool>,
    unsatisfied: Vec<usize>,
    req_sum: Vec<u32>,
    fact_gen: Vec<u32>,
    edge_gen: Vec<u32>,
    current_generation: u32,
    pq: BucketQueue,
}

impl HAddWorkspace {
    pub fn new(idx: &IndexedCausal) -> Self {
        let num_facts = idx.fact_to_id.len();
        let num_edges = idx.base_unsatisfied.len();
        Self {
            dist: vec![u32::MAX; num_facts],
            finalized: vec![false; num_facts],
            unsatisfied: idx.base_unsatisfied.clone(),
            req_sum: vec![0; num_edges],
            fact_gen: vec![0; num_facts],
            edge_gen: vec![0; num_edges],
            current_generation: 1,
            pq: BucketQueue::new(),
        }
    }
}

impl IndexedCausal {
    pub fn new(
        edges: &[Hyperedge],
        content: &Content,
        card_declared_aspects: &BTreeMap<CardId, Vec<story_content::ir::AspectKey>>,
    ) -> Self {
        fn get_or_insert_id(fact_to_id: &mut BTreeMap<Fact, u32>, f: &Fact) -> u32 {
            let next = fact_to_id.len() as u32;
            *fact_to_id.entry(f.clone()).or_insert(next)
        }

        let mut fact_to_id = BTreeMap::new();

        for card_id in content.cards.keys() {
            get_or_insert_id(&mut fact_to_id, &Fact::CardLive(card_id.clone()));
        }

        let mut edge_requires = Vec::with_capacity(edges.len());
        let mut edge_produces = Vec::with_capacity(edges.len());
        let mut edge_recipe_max = Vec::with_capacity(edges.len());
        let mut base_unsatisfied = Vec::with_capacity(edges.len());
        let mut zero_req_edges = Vec::new();

        for (ei, edge) in edges.iter().enumerate() {
            let is_boot_or_deck =
                matches!(edge.site, Site::Boot(_) | Site::BootState | Site::Deck(_));

            let recipe_max = match &edge.site {
                Site::Recipe(r) | Site::RecipeAbort(r) => {
                    if content.recipe(r.as_str()).and_then(|d| d.max).is_some() {
                        Some(r.clone())
                    } else {
                        None
                    }
                }
                _ => None,
            };
            edge_recipe_max.push(recipe_max);

            let req_ids: Vec<u32> = edge
                .requires
                .iter()
                .map(|r| get_or_insert_id(&mut fact_to_id, r))
                .collect();
            let prod_ids: Vec<u32> = edge
                .produces
                .iter()
                .map(|p| get_or_insert_id(&mut fact_to_id, p))
                .collect();

            if is_boot_or_deck {
                base_unsatisfied.push(usize::MAX);
            } else {
                base_unsatisfied.push(req_ids.len());
                if req_ids.is_empty() {
                    zero_req_edges.push(ei as u32);
                }
            }
            edge_requires.push(req_ids);
            edge_produces.push(prod_ids);
        }

        let mut aspect_bridge = vec![None; fact_to_id.len()];
        let mut card_live_bridges = vec![Vec::new(); fact_to_id.len()];

        let current_facts: Vec<(Fact, u32)> =
            fact_to_id.iter().map(|(f, &id)| (f.clone(), id)).collect();
        for (f, id) in current_facts {
            if let Fact::AspectCanIncrease {
                card: Some(c),
                key,
                ..
            } = f
            {
                let bridged = Fact::CardHasAspect(c, key);
                let target_id = get_or_insert_id(&mut fact_to_id, &bridged);
                if id as usize >= aspect_bridge.len() {
                    aspect_bridge.resize(id as usize + 1, None);
                }
                aspect_bridge[id as usize] = Some(target_id);
            } else if let Fact::CardLive(c) = f {
                if let Some(aspects) = card_declared_aspects.get(&c) {
                    let mut targets = Vec::new();
                    for key in aspects {
                        let bridged = Fact::CardHasAspect(c.clone(), key.clone());
                        targets.push(get_or_insert_id(&mut fact_to_id, &bridged));
                    }
                    if !targets.is_empty() {
                        if id as usize >= card_live_bridges.len() {
                            card_live_bridges.resize(id as usize + 1, Vec::new());
                        }
                        card_live_bridges[id as usize] = targets;
                    }
                }
            }
        }
        aspect_bridge.resize(fact_to_id.len(), None);
        card_live_bridges.resize(fact_to_id.len(), Vec::new());

        let mut consumers = vec![Vec::new(); fact_to_id.len()];
        for (ei, reqs) in edge_requires.iter().enumerate() {
            for &r in reqs {
                consumers[r as usize].push(ei as u32);
            }
        }
        for list in &mut consumers {
            list.sort_unstable();
            list.dedup();
        }

        Self {
            fact_to_id,
            edge_produces,
            edge_recipe_max,
            consumers,
            aspect_bridge,
            card_live_bridges,
            base_unsatisfied,
            zero_req_edges,
        }
    }
}

impl CausalGraph {
    /// The `h_add` value of the goal's facts at a state (§4 output 4): the
    /// classic delete-relaxation estimate — fact cost 0 when the fact holds,
    /// otherwise the minimum over producer edges of `1 + Σ` requirement
    /// costs; the goal value sums its conjuncts. It orders the open queue
    /// only: it is inadmissible under decrease-requiring preconditions and
    /// never affects soundness.
    pub fn h_add(
        &self,
        state: &story_core::State,
        engine: &story_engine::Engine,
        goals: &BTreeSet<Fact>,
    ) -> u32 {
        let mut ws = self.workspace();
        self.h_add_with_workspace(state, engine, goals, &mut ws)
    }

    pub(crate) fn workspace(&self) -> HAddWorkspace {
        HAddWorkspace::new(&self.indexed)
    }

    pub(crate) fn h_add_with_workspace(
        &self,
        state: &story_core::State,
        engine: &story_engine::Engine,
        goals: &BTreeSet<Fact>,
        ws: &mut HAddWorkspace,
    ) -> u32 {
        let _span = tracing::info_span!("h_add").entered();
        let idx = &self.indexed;

        ws.current_generation = ws.current_generation.wrapping_add(1);
        if ws.current_generation == 0 {
            ws.fact_gen.fill(0);
            ws.edge_gen.fill(0);
            ws.current_generation = 1;
        }
        let gen = ws.current_generation;
        ws.pq.clear();

        let grounded = self.grounded_state_facts(state, engine);
        for fact in &grounded {
            if let Some(&fid) = idx.fact_to_id.get(fact) {
                let u = fid as usize;
                ws.fact_gen[u] = gen;
                ws.dist[u] = 0;
                ws.finalized[u] = false;
                ws.pq.push(0, fid);
            }
        }

        let mut goal_ids: Vec<(u32, bool)> = Vec::with_capacity(goals.len());
        let mut ungrounded_goals = 0usize;
        for g in goals {
            if let Some(&gid) = idx.fact_to_id.get(g) {
                let u = gid as usize;
                let already = ws.fact_gen[u] == gen && ws.dist[u] == 0;
                if !already {
                    ungrounded_goals += 1;
                }
                goal_ids.push((gid, already));
            }
        }

        if ungrounded_goals == 0
            && goals.iter().all(|g| {
                grounded.contains(g)
                    || idx
                        .fact_to_id
                        .get(g)
                        .is_some_and(|&id| ws.fact_gen[id as usize] == gen && ws.dist[id as usize] == 0)
            })
        {
            return 0;
        }

        let content = engine.content();

        for &ei_u32 in &idx.zero_req_edges {
            let ei = ei_u32 as usize;
            if let Some(r) = &idx.edge_recipe_max[ei] {
                let exhausted = content
                    .recipe(r.as_str())
                    .and_then(|d| d.max)
                    .is_some_and(|m| state.recipe_runs.get(r).copied().unwrap_or(0) >= m)
                    && !state.situations.values().any(|s| &s.recipe == r);
                if exhausted {
                    continue;
                }
            }
            for &p in &idx.edge_produces[ei] {
                let next_cost = 1;
                let pv = p as usize;
                let cur_dist = if ws.fact_gen[pv] == gen {
                    ws.dist[pv]
                } else {
                    u32::MAX
                };
                if cur_dist > next_cost {
                    ws.fact_gen[pv] = gen;
                    ws.dist[pv] = next_cost;
                    ws.finalized[pv] = false;
                    ws.pq.push(next_cost, p);
                }
            }
        }

        while let Some((cost, fid)) = ws.pq.pop() {
            let u = fid as usize;
            if ws.fact_gen[u] == gen && ws.finalized[u] {
                continue;
            }
            ws.fact_gen[u] = gen;
            ws.finalized[u] = true;

            for (gid, satisfied) in &mut goal_ids {
                if *gid == fid && !*satisfied {
                    *satisfied = true;
                    ungrounded_goals -= 1;
                }
            }
            if ungrounded_goals == 0 {
                break;
            }

            // Aspect bridging:
            // 1) AspectCanIncrease -> CardHasAspect
            if let Some(bridged) = idx.aspect_bridge[u] {
                let v = bridged as usize;
                let cur_dist = if ws.fact_gen[v] == gen {
                    ws.dist[v]
                } else {
                    u32::MAX
                };
                if cur_dist > cost {
                    ws.fact_gen[v] = gen;
                    ws.dist[v] = cost;
                    ws.finalized[v] = false;
                    ws.pq.push(cost, bridged);
                }
            }

            // 2) CardLive -> CardHasAspect for declared aspects >= 1
            for &bridged in &idx.card_live_bridges[u] {
                let v = bridged as usize;
                let cur_dist = if ws.fact_gen[v] == gen {
                    ws.dist[v]
                } else {
                    u32::MAX
                };
                if cur_dist > cost {
                    ws.fact_gen[v] = gen;
                    ws.dist[v] = cost;
                    ws.finalized[v] = false;
                    ws.pq.push(cost, bridged);
                }
            }

            // Notify consumers
            for &ei_u32 in &idx.consumers[u] {
                let ei = ei_u32 as usize;
                if ws.edge_gen[ei] != gen {
                    ws.edge_gen[ei] = gen;
                    ws.unsatisfied[ei] = idx.base_unsatisfied[ei];
                    ws.req_sum[ei] = 0;
                }
                if ws.unsatisfied[ei] == 0 || ws.unsatisfied[ei] == usize::MAX {
                    continue;
                }
                ws.req_sum[ei] = ws.req_sum[ei].saturating_add(cost);
                ws.unsatisfied[ei] -= 1;
                if ws.unsatisfied[ei] == 0 {
                    if let Some(r) = &idx.edge_recipe_max[ei] {
                        let exhausted = content
                            .recipe(r.as_str())
                            .and_then(|d| d.max)
                            .is_some_and(|m| state.recipe_runs.get(r).copied().unwrap_or(0) >= m)
                            && !state.situations.values().any(|s| &s.recipe == r);
                        if exhausted {
                            continue;
                        }
                    }
                    let edge_cost = ws.req_sum[ei].saturating_add(1).min(MAX_COST as u32);
                    for &p in &idx.edge_produces[ei] {
                        let pv = p as usize;
                        let cur_dist = if ws.fact_gen[pv] == gen {
                            ws.dist[pv]
                        } else {
                            u32::MAX
                        };
                        if cur_dist > edge_cost {
                            ws.fact_gen[pv] = gen;
                            ws.dist[pv] = edge_cost;
                            ws.finalized[pv] = false;
                            ws.pq.push(edge_cost, p);
                        }
                    }
                }
            }
        }

        goals
            .iter()
            .map(|g| {
                if let Some(&fid) = idx.fact_to_id.get(g) {
                    let u = fid as usize;
                    if ws.fact_gen[u] == gen && ws.dist[u] < u32::MAX {
                        ws.dist[u]
                    } else {
                        u32::MAX / 4
                    }
                } else if grounded.contains(g) {
                    0
                } else {
                    u32::MAX / 4
                }
            })
            .fold(0u32, |a, b| a.saturating_add(b))
    }

    /// The facts the state itself grounds (the heuristic's base costs).
    pub(crate) fn grounded_state_facts(
        &self,
        state: &story_core::State,
        engine: &story_engine::Engine,
    ) -> BTreeSet<Fact> {
        let _span = tracing::info_span!("grounded_state_facts").entered();
        use story_content::ir::{Scope, VerbMode};
        let content = engine.content();
        let mut out = BTreeSet::new();
        let mut live: BTreeMap<CardId, Vec<story_core::EntityId>> = BTreeMap::new();
        for (&eid, ent) in &state.entities {
            live.entry(ent.prototype.clone()).or_default().push(eid);
        }
        for (card, ids) in &live {
            out.insert(Fact::CardLive(card.clone()));
            for eid in ids {
                let ent = &state.entities[eid];
                // Key presence grounds the read: the heuristic's landmark
                // for "this mechanism has been engaged". Gating on the
                // value makes h_add forget the landmark exactly once a
                // drain clamps the value back to 0, and the ordering
                // collapses to near-breadth-first (an 8x expansion
                // regression). Ordering-only; never affects soundness.
                for (key, _) in &ent.aspects {
                    out.insert(Fact::CardHasAspect(card.clone(), key.clone()));
                }
                for (key, decl) in &content.aspects {
                    if decl.default >= 1 && !ent.aspects.contains_key(key) {
                        out.insert(Fact::CardHasAspect(card.clone(), key.clone()));
                    }
                }
            }
        }
        // Zone facts resolve through the kernel's scope face (zone scopes
        // include nested subtrees).
        for (card, zone) in &self.zone_facts {
            let scope = Scope::Zone(zone.clone());
            if engine.scope_entities(&scope).iter().any(|e| {
                state
                    .entities
                    .get(e)
                    .is_some_and(|ent| &ent.prototype == card)
            }) {
                out.insert(Fact::CardInZone(card.clone(), zone.clone()));
            }
        }
        for (l, &val) in &state.legacy {
            let default = content
                .legacies
                .get(l.as_str())
                .map(|d| d.default)
                .unwrap_or(0);
            if val != default || val >= 1 {
                out.insert(Fact::LegacyCanChange(l.clone()));
            }
        }
        for sit in state.situations.values() {
            out.insert(Fact::RecipeCanOpen(sit.recipe.clone()));
        }
        // Committable and autonomous verbs ground their open affordances
        // when the recipe's run cap has not been exhausted.
        for verb in content.verbs.values() {
            if !matches!(verb.mode, VerbMode::Committable | VerbMode::Autonomous) {
                continue;
            }
            for rec in verb.recipes.values() {
                let exhausted = rec
                    .max
                    .is_some_and(|m| state.recipe_runs.get(&rec.id).copied().unwrap_or(0) >= m);
                if !exhausted {
                    out.insert(Fact::RecipeCanOpen(rec.id.clone()));
                }
            }
        }
        out
    }

    /// The backward cone of the goal facts (§4 output 3): every fact some
    /// chain of producer requirements reaches them through.
    pub fn backward_cone(&self, goals: &BTreeSet<Fact>) -> BTreeSet<Fact> {
        // No CardHasAspect → AspectCanIncrease bridging here: bridging pulls
        // every verb that can touch the key into the cone, which erases the
        // irrelevant-verb demotion and collapses the search ordering. The
        // cone keeps the strict fact-to-fact closure; relevant_verbs does
        // its own bridging where verb-level routing is the point.
        let mut cone = goals.clone();
        let mut queue: Vec<Fact> = goals.iter().cloned().collect();
        let mut visited_edges = vec![false; self.edges.len()];

        while let Some(fact) = queue.pop() {
            if let Some(edge_indices) = self.producers.get(&fact) {
                for &ei in edge_indices {
                    if visited_edges[ei] {
                        continue;
                    }
                    visited_edges[ei] = true;
                    for r in &self.edges[ei].requires {
                        if cone.insert(r.clone()) {
                            queue.push(r.clone());
                        }
                    }
                }
            }
        }
        cone
    }

    /// The verbs whose transitive effects can influence a fact of the cone
    /// or reach an ending (§4 output 3): verbs outside the set are
    /// deprioritized in the search, never pruned. Autonomous verbs always
    /// qualify — time itself drives them, and their endings are active
    /// threats.
    pub fn relevant_verbs(&self, content: &Content, cone: &BTreeSet<Fact>) -> BTreeSet<VerbId> {
        let _span = tracing::info_span!("relevant_verbs").entered();
        let mut relevant = BTreeSet::new();

        let mut targets: BTreeSet<Fact> = cone.clone();
        // Active threats: every ending the content can reach is an
        // influence target (an ending-bearing autonomous recipe must never
        // be deprioritized).
        for edge in &self.edges {
            for p in &edge.produces {
                if matches!(p, Fact::EndingReached(_)) {
                    targets.insert(p.clone());
                }
            }
        }

        // Collect non-boot edges and build index for forward chaining.
        let non_boot_edges: Vec<&Hyperedge> = self
            .edges
            .iter()
            .filter(|e| !matches!(e.site, Site::BootState | Site::Boot(_)))
            .collect();

        let mut req_to_edge_indices: BTreeMap<&Fact, Vec<usize>> = BTreeMap::new();
        let base_unsatisfied: Vec<usize> = non_boot_edges
            .iter()
            .enumerate()
            .map(|(i, edge)| {
                for r in &edge.requires {
                    req_to_edge_indices.entry(r).or_default().push(i);
                }
                edge.requires.len()
            })
            .collect();

        // Index seeds by recipe id.
        let mut recipe_produces: BTreeMap<&RecipeId, Vec<&Fact>> = BTreeMap::new();
        for edge in &self.edges {
            match &edge.site {
                Site::Recipe(id) | Site::RecipeAbort(id) => {
                    recipe_produces.entry(id).or_default().extend(&edge.produces);
                }
                _ => {}
            }
        }

        for verb in content.verbs.values() {
            let mut seeds: BTreeSet<Fact> = BTreeSet::new();
            for rec in verb.recipes.values() {
                if let Some(prods) = recipe_produces.get(&rec.id) {
                    for &p in prods {
                        seeds.insert(p.clone());
                    }
                }
            }
            if seeds.is_empty() {
                // A verb with no extracted edge (opaque or slot-less) stays
                // relevant: Tier-1 silence is not evidence of deadness.
                relevant.insert(verb.id.clone());
                continue;
            }
            if verb.mode == story_content::ir::VerbMode::Autonomous {
                relevant.insert(verb.id.clone());
                continue;
            }
            if seeds.iter().any(|f| targets.contains(f)) {
                relevant.insert(verb.id.clone());
                continue;
            }

            let mut unsatisfied = base_unsatisfied.clone();
            let mut set: BTreeSet<Fact> = BTreeSet::new();
            let mut queue: Vec<Fact> = seeds.into_iter().collect();
            let mut touches_target = false;

            while let Some(fact) = queue.pop() {
                if !set.insert(fact.clone()) {
                    continue;
                }
                if targets.contains(&fact) {
                    touches_target = true;
                    break;
                }
                if let Fact::AspectCanIncrease {
                    card: Some(card),
                    key,
                    ..
                } = &fact
                {
                    let bridged = Fact::CardHasAspect(card.clone(), key.clone());
                    if !set.contains(&bridged) {
                        queue.push(bridged);
                    }
                }
                if let Some(edge_indices) = req_to_edge_indices.get(&fact) {
                    for &ei in edge_indices {
                        unsatisfied[ei] -= 1;
                        if unsatisfied[ei] == 0 {
                            for p in &non_boot_edges[ei].produces {
                                if !set.contains(p) {
                                    queue.push(p.clone());
                                }
                            }
                        }
                    }
                }
            }

            if touches_target {
                relevant.insert(verb.id.clone());
            }
        }
        relevant
    }
}

/// The goal facts of a query predicate (the heuristic's and the pruning's
/// input): a conservative extraction of necessary facts over the goal's
/// structure. Conjunctive branches union; disjunctive branches intersect;
/// negated branches and upper bounds contribute nothing.
pub fn goal_facts(goal: &crate::query::GoalPredicate) -> BTreeSet<Fact> {
    use crate::query::{EventMatcher, GoalPredicate};
    use story_content::ir::{Expr, LogicOp, Scope};
    fn positive_read(e: &Expr, n: i64, out: &mut BTreeSet<Fact>) {
        if n < 1 {
            return;
        }
        match e {
            Expr::NamePath { card, key } => {
                out.insert(Fact::CardHasAspect(card.clone(), key.clone()));
            }
            Expr::Legacy(key) => {
                out.insert(Fact::LegacyCanChange(key.clone()));
            }
            _ => {}
        }
    }
    fn expr_facts(e: &Expr) -> BTreeSet<Fact> {
        let mut out = BTreeSet::new();
        match e {
            Expr::NamePath { card, key } => {
                out.insert(Fact::CardHasAspect(card.clone(), key.clone()));
            }
            Expr::Legacy(key) => {
                out.insert(Fact::LegacyCanChange(key.clone()));
            }
            Expr::Alive(card) => {
                out.insert(Fact::CardLive(card.clone()));
            }
            Expr::Cmp { op, left, right } => match (op, left.as_ref(), right.as_ref()) {
                (CmpOp::Ge | CmpOp::Eq, _, Expr::Lit(n)) => positive_read(left, *n, &mut out),
                (CmpOp::Gt, _, Expr::Lit(n)) => positive_read(left, n.saturating_add(1), &mut out),
                (CmpOp::Le | CmpOp::Eq, Expr::Lit(n), _) => positive_read(right, *n, &mut out),
                (CmpOp::Lt, Expr::Lit(n), _) => positive_read(right, n.saturating_add(1), &mut out),
                _ => {}
            },
            Expr::Logic {
                op: LogicOp::And,
                left,
                right,
            } => {
                out.extend(expr_facts(left));
                out.extend(expr_facts(right));
            }
            Expr::Logic {
                op: LogicOp::Or,
                left,
                right,
            } => {
                let l = expr_facts(left);
                let r = expr_facts(right);
                out.extend(l.intersection(&r).cloned());
            }
            Expr::Not(_) => {}
            _ => {}
        }
        out
    }
    let mut out = BTreeSet::new();
    match goal {
        GoalPredicate::Expr(e) => out.extend(expr_facts(e)),
        GoalPredicate::CardMatch {
            scope,
            prototype: Some(p),
            terms,
            min_count,
            ..
        } if *min_count >= 1 => {
            out.insert(Fact::CardLive(p.clone()));
            if let Scope::Zone(z) = scope {
                out.insert(Fact::CardInZone(p.clone(), z.clone()));
            }
            for t in terms {
                if let QueryTerm::Aspect(a) = t {
                    let low = match a.op {
                        None => 1,
                        Some(CmpOp::Ge | CmpOp::Eq) => a.value,
                        Some(CmpOp::Gt) => a.value.saturating_add(1),
                        _ => 0,
                    };
                    if low >= 1 {
                        out.insert(Fact::CardHasAspect(p.clone(), a.key.clone()));
                    }
                }
            }
        }
        GoalPredicate::Ending(Some(id)) => {
            out.insert(Fact::EndingReached(id.clone()));
        }
        GoalPredicate::Ending(None) => {}
        GoalPredicate::SituationActive {
            recipe: Some(r), ..
        } => {
            out.insert(Fact::RecipeCanOpen(r.clone()));
        }
        GoalPredicate::TransitionEvent(EventMatcher::SituationCompleted { recipe }) => {
            out.insert(Fact::RecipeCanComplete(recipe.clone()));
        }
        GoalPredicate::TransitionEvent(EventMatcher::Continued { recipe }) => {
            out.insert(Fact::RecipeCanOpen(recipe.clone()));
        }
        GoalPredicate::PartialState(ps) => {
            for (card, key, val) in &ps.aspects {
                if *val >= 1 {
                    out.insert(Fact::CardHasAspect(card.clone(), key.clone()));
                }
            }
            for (scope, proto, terms) in &ps.cards {
                if let Some(p) = proto {
                    out.insert(Fact::CardLive(p.clone()));
                    if let Scope::Zone(z) = scope {
                        out.insert(Fact::CardInZone(p.clone(), z.clone()));
                    }
                    for t in terms {
                        if let QueryTerm::Aspect(a) = t {
                            let low = match a.op {
                                None => 1,
                                Some(CmpOp::Ge | CmpOp::Eq) => a.value,
                                Some(CmpOp::Gt) => a.value.saturating_add(1),
                                _ => 0,
                            };
                            if low >= 1 {
                                out.insert(Fact::CardHasAspect(p.clone(), a.key.clone()));
                            }
                        }
                    }
                }
            }
            for (lkey, val) in &ps.legacy {
                if *val >= 1 {
                    out.insert(Fact::LegacyCanChange(lkey.clone()));
                }
            }
        }
        GoalPredicate::And(ps) => {
            for p in ps {
                out.extend(goal_facts(p));
            }
        }
        GoalPredicate::Or(ps) => {
            let mut iter = ps.iter().map(goal_facts);
            if let Some(first) = iter.next() {
                out = iter.fold(first, |acc, next| {
                    acc.intersection(&next).cloned().collect()
                });
            }
        }
        GoalPredicate::Not(_) => {}
        _ => {}
    }
    out
}
