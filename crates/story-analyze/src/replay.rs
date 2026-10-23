//! The replay verification oracle (story-analyze spec §5.6): every witness
//! the analyzer emits is replayed through a fresh kernel engine and must
//! reproduce the goal state and event stream (kernel invariant T6). This is
//! the analyzer's verification backbone — a first-class checked oracle, not
//! a comment: `solve` self-checks every witness before returning it, and
//! every test asserts through it.

use std::collections::BTreeMap;
use std::sync::Arc;

use story_content::Content;
use story_core::State;
use story_engine::{ChanceOutcome, CommitAction, Engine, EffectRegistry, Event};

use crate::query::GoalCtx;
use crate::transitions::{PlayerStep, TransitionRecord, WitnessPath};

/// Why a replay failed. Every variant names the offending step.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ReplayError {
    /// The kernel refused a step the witness claims to have taken (T3:
    /// refusals never appear inside generated witnesses).
    ActionRefused {
        step: usize,
        diagnostic: String,
    },
    /// The step's event stream differs from the record's.
    EventMismatch {
        step: usize,
        expected: Vec<Event>,
        actual: Vec<Event>,
    },
    /// The step's resulting state differs logically from the record's.
    StateMismatch { step: usize },
    /// A branching record's chance points diverged from the recorded
    /// outcomes (fewer or more chance points fired).
    ChanceMismatch { step: usize },
    /// The goal does not hold at the final state.
    GoalUnmet,
    /// The invariant failed on a materialized state.
    InvariantViolated { step: usize },
}

impl std::fmt::Display for ReplayError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ReplayError::ActionRefused { step, diagnostic } => {
                write!(f, "step {step} refused: {diagnostic}")
            }
            ReplayError::EventMismatch { step, .. } => write!(f, "step {step} event stream differs"),
            ReplayError::StateMismatch { step } => write!(f, "step {step} state differs"),
            ReplayError::ChanceMismatch { step } => {
                write!(f, "step {step} chance points diverged from the record")
            }
            ReplayError::GoalUnmet => write!(f, "the goal does not hold at the final state"),
            ReplayError::InvariantViolated { step } => {
                write!(f, "the invariant failed at step {step}")
            }
        }
    }
}

/// The replay environment: the content, registry, seed, and legacy
/// overrides the exploration booted with (the oracle reboots the same
/// engine and installs the start state through `with_state`).
#[derive(Clone)]
pub struct Replayer {
    pub content: Arc<Content>,
    pub registry: EffectRegistry,
    pub seed: u64,
    pub legacy_in: Option<BTreeMap<String, i64>>,
}

impl Replayer {
    pub fn new(
        content: Arc<Content>,
        registry: EffectRegistry,
        seed: u64,
        legacy_in: Option<BTreeMap<String, i64>>,
    ) -> Replayer {
        Replayer {
            content,
            registry,
            seed,
            legacy_in,
        }
    }

    fn fresh_engine(&self, start: &State) -> Engine {
        let (engine, _) = Engine::boot(
            self.content.clone(),
            self.registry.clone(),
            self.seed,
            self.legacy_in.clone(),
        );
        engine.with_state(start.clone())
    }

    /// Replay one witness (story-analyze spec §5.6): execute the step list
    /// against a fresh engine, assert identical event streams, canonical
    /// state reproduction, invariant conformance on every materialized
    /// state, and goal satisfaction at the end. Deterministic witnesses
    /// replay through the public API; branching witnesses replay through
    /// the forking face with their recorded outcomes.
    pub fn replay(
        &self,
        witness: &WitnessPath,
        goal: Option<&crate::query::GoalPredicate>,
        invariant: Option<&crate::query::GoalPredicate>,
    ) -> Result<State, ReplayError> {
        let mut engine = self.fresh_engine(&witness.start);
        if let Some(inv) = invariant {
            let ctx = GoalCtx {
                engine: &engine,
                events: &[],
            };
            if !inv.holds(&ctx) {
                return Err(ReplayError::InvariantViolated { step: 0 });
            }
        }
        for (i, record) in witness.records.iter().enumerate() {
            let (events, state) = self.step(&mut engine, record, i)?;
            if events != record.events {
                return Err(ReplayError::EventMismatch {
                    step: i,
                    expected: record.events.clone(),
                    actual: events,
                });
            }
            // Canonical-key reproduction: the same logical state up to
            // entity interchange (§5.3). The kernel is deterministic given
            // the same actions and chance outcomes, so the keys must agree
            // exactly.
            let key = crate::canonical::CanonicalStateKey::of(&state, &self.content);
            let want = crate::canonical::CanonicalStateKey::of(&record.state_after, &self.content);
            if key != want {
                return Err(ReplayError::StateMismatch { step: i });
            }
            *engine.state_mut() = state.clone();
            if let Some(inv) = invariant {
                let ctx = GoalCtx {
                    engine: &engine,
                    events: &record.events,
                };
                if !inv.holds(&ctx) {
                    return Err(ReplayError::InvariantViolated { step: i });
                }
            }
        }
        let final_events = witness
            .records
            .last()
            .map(|r| r.events.clone())
            .unwrap_or_default();
        let ctx = GoalCtx {
            engine: &engine,
            events: &final_events,
        };
        if let Some(goal) = goal {
            if !goal.holds(&ctx) {
                return Err(ReplayError::GoalUnmet);
            }
        }
        Ok(engine.state().clone())
    }

    /// Execute one record's step, supplying the record's chance outcomes at
    /// the forking face in firing order.
    fn step(
        &self,
        engine: &mut Engine,
        record: &TransitionRecord,
        index: usize,
    ) -> Result<(Vec<Event>, State), ReplayError> {
        let outcomes = record.outcomes.clone();
        let mut cursor = 0usize;
        let mut hook = |point: &story_engine::ChancePoint| -> Option<ChanceOutcome> {
            // The record's outcomes align 1:1 with the fired chance points:
            // the enumeration only plans outcomes for points the kernel
            // fires, and the prefix is identical on replay.
            let _ = point;
            let o = outcomes.get(cursor).cloned();
            if o.is_some() {
                cursor += 1;
            }
            o
        };
        let result = match &record.step {
            PlayerStep::Commit { verb, bindings } => {
                let action = CommitAction {
                    verb: verb.clone(),
                    bindings: bindings.clone(),
                };
                if record.outcomes.is_empty() {
                    engine.commit(action)
                } else {
                    engine.commit_forked(action, &mut hook)
                }
            }
            PlayerStep::Bind { sit, slot, entity } => engine.bind(*sit, slot.clone(), *entity),
            PlayerStep::Advance { dt } => {
                if record.outcomes.is_empty() {
                    engine.advance(*dt)
                } else {
                    engine.advance_forked(*dt, &mut hook)
                }
            }
        };
        let events = result.map_err(|d| ReplayError::ActionRefused {
            step: index,
            diagnostic: d.to_string(),
        })?;
        if cursor != outcomes.len() {
            return Err(ReplayError::ChanceMismatch { step: index });
        }
        Ok((events, engine.state().clone()))
    }
}
