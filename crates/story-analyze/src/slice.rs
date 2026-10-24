//! Modular slicing and assume–guarantee reasoning (story-analyze spec):
//! analyze one verb, narrative arc, or sub-story at a time over interface
//! facts, never exploring the combinatorial product of the full state space.
//!
//! A slice is defined by a target subset of verbs ([`SliceTarget`]).
//! Interface boundaries ([`InterfaceBoundary`]) partition the hyperedges into:
//! - **Inputs / Assumed facts**: prerequisites needed by the slice that must
//!   originate outside it (or from initial boot state).
//! - **Outputs / Guaranteed facts**: capabilities produced by the slice that
//!   are observable externally or satisfy query goals.
//! - **Internal facts**: capabilities produced and consumed strictly inside
//!   the slice.
//!
//! Verification runs within the isolated slice by restricting transition
//! generation to the slice's active verbs and seeding the starting state with
//! assumed interface resources ([`Assumption`]). Compositional verification
//! ([`verify_composition`]) discharges assume–guarantee contracts across
//! sequential or interconnected slices.

use std::collections::BTreeSet;
use std::sync::Arc;

use story_content::ir::VerbMode;
use story_content::Content;
use story_core::{CardId, EntityId, State, VerbId, ZoneKind};

use crate::causal::{self, CausalGraph, Fact, Site};
use crate::mdp::{self, MdpConfig, MdpOutcome};
use crate::query::{GoalPredicate, ReachQuery};
use crate::search::{Analyzer, ExploreConfig, ExploredGraph, SolveOutcome};
use crate::transitions::WitnessPath;

/// Specification of the verbs constituting a slice.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SliceTarget {
    /// Active verbs permitted in the slice.
    Verbs(BTreeSet<VerbId>),
}

impl SliceTarget {
    /// A single committable or autonomous verb.
    pub fn single(verb: VerbId) -> Self {
        Self::Verbs(BTreeSet::from([verb]))
    }

    /// A narrative arc consisting of multiple collaborating verbs.
    pub fn arc(verbs: impl IntoIterator<Item = VerbId>) -> Self {
        Self::Verbs(verbs.into_iter().collect())
    }

    /// Whether a verb belongs to this slice.
    pub fn allows_verb(&self, verb: &VerbId) -> bool {
        match self {
            Self::Verbs(set) => set.contains(verb),
        }
    }

    /// The set of verbs in this slice.
    pub fn verbs(&self) -> &BTreeSet<VerbId> {
        match self {
            Self::Verbs(set) => set,
        }
    }
}

/// Boundary preconditions initialized before exploring a slice.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Assumption {
    /// Injects `count` copies of a declared card onto the table zone.
    Card { id: CardId, count: u32 },
    /// Overrides an initial integer value for a legacy key.
    Legacy { key: String, value: i64 },
    /// Sets an aspect value on a card entity on the table.
    Aspect { card: CardId, key: String, value: i64 },
    /// An invariant predicate that must hold on every explored state.
    Invariant(GoalPredicate),
}

/// Boundary classification of interface facts for a slice.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct InterfaceBoundary {
    /// Facts required by the slice that are produced externally.
    pub inputs: BTreeSet<Fact>,
    /// Facts produced by the slice that are consumed externally (or by goals).
    pub outputs: BTreeSet<Fact>,
    /// Facts private to the slice's internal mechanics.
    pub internal: BTreeSet<Fact>,
}

impl InterfaceBoundary {
    /// Extract the interface boundary facts between a target slice and the rest
    /// of the story using the Tier-1 causal hypergraph.
    pub fn extract(graph: &CausalGraph, target: &SliceTarget) -> Self {
        let slice_verbs = target.verbs();
        let mut slice_req = BTreeSet::new();
        let mut slice_prod = BTreeSet::new();
        let mut ext_req = BTreeSet::new();
        let mut boot_facts = BTreeSet::new();
        let mut slice_edges = Vec::new();

        for edge in graph.edges() {
            match &edge.site {
                Site::Boot(_) | Site::BootState => {
                    boot_facts.extend(edge.produces.iter().cloned());
                }
                _ => {}
            }

            let in_slice = match &edge.site {
                Site::Recipe(r) | Site::RecipeAbort(r) => {
                    r.as_str().split_once('.').is_some_and(|(v, _)| {
                        slice_verbs.contains(&VerbId::from(v))
                    })
                }
                _ => false,
            };

            if in_slice {
                slice_edges.push(edge);
                slice_req.extend(edge.requires.iter().cloned());
                slice_prod.extend(edge.produces.iter().cloned());
            } else if !matches!(edge.site, Site::Boot(_) | Site::BootState) {
                ext_req.extend(edge.requires.iter().cloned());
            }
        }

        expand_implied(&mut boot_facts);
        expand_implied(&mut slice_prod);
        expand_implied(&mut slice_req);
        expand_implied(&mut ext_req);

        // Identify facts produced within the slice.
        let mut slice_producers = BTreeSet::new();
        for edge in &slice_edges {
            slice_producers.extend(edge.produces.iter().cloned());
        }
        expand_implied(&mut slice_producers);

        // Inputs (assumptions): facts required by the slice that cannot be
        // produced internally, plus any required facts that form ungrounded
        // cycles (such as catalysts) that must be seeded from outside.
        let mut inputs = BTreeSet::new();
        let mut slice_grounded = boot_facts.clone();

        // 1. Facts required by the slice that have no producer in the slice
        // and are not present at boot must be supplied externally.
        for f in &slice_req {
            if !slice_grounded.contains(f) && !slice_producers.contains(f) {
                inputs.insert(f.clone());
                slice_grounded.insert(f.clone());
            }
        }
        expand_implied(&mut slice_grounded);

        // 2. Fixpoint reachable from boot + external inputs.
        // If some required facts remain ungrounded, they are part of a cyclic
        // requirement (e.g. catalyst) and must also be supplied as inputs.
        loop {
            let mut changed = false;
            for edge in &slice_edges {
                if edge.requires.iter().all(|r| slice_grounded.contains(r)) {
                    for p in &edge.produces {
                        changed |= slice_grounded.insert(p.clone());
                    }
                }
            }
            expand_implied(&mut slice_grounded);
            if !changed {
                if let Some(unmet) = slice_req.iter().find(|f| !slice_grounded.contains(f)) {
                    inputs.insert(unmet.clone());
                    slice_grounded.insert(unmet.clone());
                    expand_implied(&mut slice_grounded);
                    continue;
                }
                break;
            }
        }

        // Outputs (guarantees): produced by slice, consumed by external sites,
        // or leading directly to an ending.
        let outputs: BTreeSet<Fact> = slice_prod
            .iter()
            .filter(|f| ext_req.contains(f) || matches!(f, Fact::EndingReached(_)))
            .cloned()
            .collect();

        // Internal: produced and consumed inside slice, not inputs and not outputs.
        let internal: BTreeSet<Fact> = slice_prod
            .intersection(&slice_req)
            .filter(|f| !inputs.contains(f) && !outputs.contains(f))
            .cloned()
            .collect();

        InterfaceBoundary {
            inputs,
            outputs,
            internal,
        }
    }
}

fn expand_implied(facts: &mut BTreeSet<Fact>) {
    let mut implied = Vec::new();
    for f in facts.iter() {
        match f {
            Fact::AspectCanIncrease {
                card: Some(c),
                key,
                ..
            } => {
                implied.push(Fact::CardHasAspect(c.clone(), key.clone()));
                implied.push(Fact::CardLive(c.clone()));
            }
            Fact::CardHasAspect(c, _) => {
                implied.push(Fact::CardLive(c.clone()));
            }
            Fact::CardInZone(c, _) => {
                implied.push(Fact::CardLive(c.clone()));
            }
            _ => {}
        }
    }
    facts.extend(implied);
}

/// Project content for a slice: verbs outside the target have their mode set to
/// `VerbMode::Uncommittable`. This silences player commits and autonomous
/// background polling for external verbs, while preserving card definitions,
/// relation kinds, and lookup targets.
pub fn slice_content(content: &Content, target: &SliceTarget) -> Content {
    let mut sliced = content.clone();
    for (name, verb) in &mut sliced.verbs {
        if !target.allows_verb(&VerbId::from(name.as_str())) {
            verb.mode = VerbMode::Uncommittable;
        }
    }
    sliced
}

/// Materialize one boundary assumption into the starting state.
pub fn apply_assumption(
    state: &mut State,
    content: &Content,
    assumption: &Assumption,
) -> Result<(), String> {
    match assumption {
        Assumption::Card { id, count } => {
            let Some(decl) = content.cards.get(id) else {
                return Err(format!("unknown card `{id}` in assumption"));
            };
            if *count == 0 {
                return Err(format!("cannot assume 0 copies of card `{id}`"));
            }
            if matches!(
                decl.mint,
                story_core::MintPolicy::SingleCopy | story_core::MintPolicy::Unique
            ) {
                if *count > 1 {
                    return Err(format!("cannot assume {count} copies of unique card `{id}`"));
                }
                if state
                    .entities
                    .values()
                    .any(|e| &e.prototype == id || e.name == id.as_str())
                {
                    return Err(format!(
                        "cannot assume unique card `{id}`: already present in starting state"
                    ));
                }
            }
            let Some(home) = state
                .zones
                .values()
                .find(|z| matches!(z.kind, ZoneKind::Table))
                .map(|z| z.id)
                .or_else(|| state.zones.keys().next().copied())
            else {
                return Err("cannot apply card assumption: state has no declared zones".to_string());
            };

            for _ in 0..*count {
                let eid = EntityId(state.next_entity);
                state.next_entity += 1;
                state.entities.insert(
                    eid,
                    story_core::Entity {
                        id: eid,
                        name: decl.id.to_string(),
                        prototype: decl.id.clone(),
                        kind: decl.kind.clone(),
                        aspects: decl.aspects.clone(),
                        attrs: Default::default(),
                        zone: home,
                        mint: decl.mint.clone(),
                        expires_at: None,
                    },
                );
            }
            Ok(())
        }
        Assumption::Legacy { key, value } => {
            state.legacy.insert(key.clone(), *value);
            Ok(())
        }
        Assumption::Aspect { card, key, value } => {
            let found = state
                .entities
                .values_mut()
                .find(|e| &e.prototype == card || e.name == card.as_str());

            if let Some(entity) = found {
                entity.aspects.insert(key.clone(), *value);
            } else {
                // If not yet present, mint a copy onto table and set aspect.
                let Some(decl) = content.cards.get(card) else {
                    return Err(format!("unknown card `{card}` in aspect assumption"));
                };
                if matches!(
                    decl.mint,
                    story_core::MintPolicy::SingleCopy | story_core::MintPolicy::Unique
                ) && state
                    .entities
                    .values()
                    .any(|e| &e.prototype == card || e.name == card.as_str())
                {
                    return Err(format!(
                        "cannot assume unique card `{card}`: already present in starting state"
                    ));
                }
                let Some(home) = state
                    .zones
                    .values()
                    .find(|z| matches!(z.kind, ZoneKind::Table))
                    .map(|z| z.id)
                    .or_else(|| state.zones.keys().next().copied())
                else {
                    return Err(
                        "cannot apply aspect assumption: state has no declared zones".to_string(),
                    );
                };
                let eid = EntityId(state.next_entity);
                state.next_entity += 1;
                let mut aspects = decl.aspects.clone();
                aspects.insert(key.clone(), *value);
                state.entities.insert(
                    eid,
                    story_core::Entity {
                        id: eid,
                        name: decl.id.to_string(),
                        prototype: decl.id.clone(),
                        kind: decl.kind.clone(),
                        aspects,
                        attrs: Default::default(),
                        zone: home,
                        mint: decl.mint.clone(),
                        expires_at: None,
                    },
                );
            }
            Ok(())
        }
        Assumption::Invariant(_) => Ok(()),
    }
}

/// Slicing analyzer: isolates state exploration to a designated slice.
pub struct SlicedAnalyzer {
    target: SliceTarget,
    boundary: InterfaceBoundary,
    invariants: Vec<GoalPredicate>,
    analyzer: Analyzer,
}

impl SlicedAnalyzer {
    /// Boot an isolated analyzer for the slice, calculating interface
    /// boundaries and applying initial assumptions to the start state.
    pub fn new(
        analyzer: &Analyzer,
        target: SliceTarget,
        assumptions: &[Assumption],
    ) -> Result<Self, String> {
        let boundary = InterfaceBoundary::extract(analyzer.causal(), &target);
        let sliced_content = Arc::new(slice_content(analyzer.content(), &target));
        let (mut isolated, _) = Analyzer::boot(
            sliced_content.clone(),
            analyzer.registry().clone(),
            analyzer.seed(),
            analyzer.legacy_in(),
        );

        let mut start_state = isolated.start().clone();
        let mut invariants = Vec::new();
        for a in assumptions {
            apply_assumption(&mut start_state, &sliced_content, a)?;
            if let Assumption::Invariant(inv) = a {
                invariants.push(inv.clone());
            }
        }
        isolated = isolated.with_start_state(start_state);

        Ok(SlicedAnalyzer {
            target,
            boundary,
            invariants,
            analyzer: isolated,
        })
    }

    /// The interface boundary facts of the slice.
    pub fn boundary(&self) -> &InterfaceBoundary {
        &self.boundary
    }

    /// The underlying isolated analyzer.
    pub fn analyzer(&self) -> &Analyzer {
        &self.analyzer
    }

    /// The mutable isolated analyzer.
    pub fn analyzer_mut(&mut self) -> &mut Analyzer {
        &mut self.analyzer
    }

    /// Solve reachability strictly within the slice.
    pub fn solve(&mut self, query: &mut ReachQuery) -> SolveOutcome {
        query.allowed_verbs = Some(self.target.verbs().clone());
        if !self.invariants.is_empty() {
            let mut all = self.invariants.clone();
            if let Some(existing) = query.invariant.take() {
                all.push(existing);
            }
            query.invariant = Some(if all.len() == 1 {
                all.remove(0)
            } else {
                GoalPredicate::And(all)
            });
        }
        self.analyzer.solve(query)
    }

    /// Explore the full graph strictly within the slice.
    pub fn explore(&mut self, config: &mut ExploreConfig) -> ExploredGraph {
        config.allowed_verbs = Some(self.target.verbs().clone());
        if !self.invariants.is_empty() {
            let mut all = self.invariants.clone();
            if let Some(existing) = config.invariant.take() {
                all.push(existing);
            }
            config.invariant = Some(if all.len() == 1 {
                all.remove(0)
            } else {
                GoalPredicate::And(all)
            });
        }
        self.analyzer.explore(config)
    }

    /// Solve stochastic MDP optimal policy strictly within the slice.
    pub fn solve_mdp(&self, config: &mut MdpConfig) -> MdpOutcome {
        config.allowed_verbs = Some(self.target.verbs().clone());
        if !self.invariants.is_empty() {
            let mut all = self.invariants.clone();
            if let Some(existing) = config.invariant.take() {
                all.push(existing);
            }
            config.invariant = Some(if all.len() == 1 {
                all.remove(0)
            } else {
                GoalPredicate::And(all)
            });
        }
        mdp::solve(&self.analyzer, config)
    }
}

/// An assume-guarantee contract for a slice.
#[derive(Clone, Debug)]
pub struct SliceContract {
    pub name: String,
    pub target: SliceTarget,
    pub assumptions: Vec<Assumption>,
    pub guarantee: GoalPredicate,
}

/// Discrepancy when verifying a contract chain.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CompositionViolation {
    pub step: usize,
    pub contract_name: String,
    pub missing_assumption: String,
}

/// Report of cross-slice compositional assume-guarantee verification.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CompositionReport {
    pub satisfied: bool,
    pub discharged: Vec<(String, Fact)>,
    pub violations: Vec<CompositionViolation>,
}

/// Verify that a sequence of slice contracts soundly chains together:
/// each contract's input assumptions must be discharged by the cumulative
/// guarantees of prior contracts or declared boot facts.
pub fn verify_composition(content: &Content, contracts: &[SliceContract]) -> CompositionReport {
    let graph = CausalGraph::build(content);
    let mut accumulated = BTreeSet::new();

    // Boot facts are universally available.
    for edge in graph.edges() {
        if matches!(edge.site, Site::Boot(_) | Site::BootState) {
            accumulated.extend(edge.produces.iter().cloned());
        }
    }
    expand_implied(&mut accumulated);

    let mut discharged = Vec::new();
    let mut violations = Vec::new();

    for (step, contract) in contracts.iter().enumerate() {
        let boundary = InterfaceBoundary::extract(&graph, &contract.target);

        for a in &contract.assumptions {
            match a {
                Assumption::Card { id, .. } => {
                    let fact = Fact::CardLive(id.clone());
                    if accumulated.contains(&fact) {
                        discharged.push((contract.name.clone(), fact));
                    } else {
                        violations.push(CompositionViolation {
                            step,
                            contract_name: contract.name.clone(),
                            missing_assumption: format!("CardLive({id})"),
                        });
                    }
                }
                Assumption::Legacy { key, .. } => {
                    let fact = Fact::LegacyCanChange(key.clone());
                    if accumulated.contains(&fact) {
                        discharged.push((contract.name.clone(), fact));
                    } else {
                        violations.push(CompositionViolation {
                            step,
                            contract_name: contract.name.clone(),
                            missing_assumption: format!("LegacyCanChange({key})"),
                        });
                    }
                }
                Assumption::Aspect { card, key, .. } => {
                    let fact = Fact::CardHasAspect(card.clone(), key.clone());
                    if accumulated.contains(&fact) {
                        discharged.push((contract.name.clone(), fact));
                    } else {
                        violations.push(CompositionViolation {
                            step,
                            contract_name: contract.name.clone(),
                            missing_assumption: format!("CardHasAspect({card}, {key})"),
                        });
                    }
                }
                Assumption::Invariant(_) => {}
            }
        }

        // Add this contract's outputs and goal facts into the cumulative capabilities.
        accumulated.extend(boundary.outputs);
        accumulated.extend(causal::goal_facts(&contract.guarantee));
        expand_implied(&mut accumulated);
    }

    CompositionReport {
        satisfied: violations.is_empty(),
        discharged,
        violations,
    }
}

/// Splicing two sequential witnesses into one contiguous composite witness.
///
/// Combines execution steps, cumulative depth, actions metrics, and
/// joint probabilities across composed slices.
pub fn compose_witnesses(w1: &WitnessPath, w2: &WitnessPath) -> WitnessPath {
    let mut records = w1.records.clone();
    records.extend(w2.records.clone());
    let probability = match (w1.probability, w2.probability) {
        (Some(p1), Some(p2)) => Some(p1 * p2),
        _ => None,
    };
    WitnessPath {
        start: w1.start.clone(),
        actions: w1.actions + w2.actions,
        depth: records.len(),
        probability,
        records,
    }
}
