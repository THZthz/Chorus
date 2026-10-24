//! The play trace format: human-readable, serializable player action traces
//! that can be exported from `story-cli play` and replayed or injected into
//! `story-analyze` as seed trajectories.

use std::collections::BTreeMap;
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use story_content::Content;
use story_core::{CardId, EntityId, SituationId, SlotName, State, Time, VerbId};
use story_engine::{CommitAction, EffectRegistry, Engine};

use crate::transitions::{PlayerStep, TransitionRecord, WitnessPath};

/// A recorded play trace from game boot to a terminal or intermediate state.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlayTrace {
    /// Format version (always 1).
    pub format: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub game: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub content_fingerprint: Option<String>,
    pub seed: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub legacy_in: Option<BTreeMap<String, i64>>,
    pub steps: Vec<TraceStep>,
}

/// One player step within a play trace.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum TraceStep {
    Advance {
        turns: i64,
    },
    Commit {
        verb: VerbId,
        bindings: Vec<(SlotName, String)>,
    },
    Bind {
        situation: SituationId,
        slot: SlotName,
        card: String,
    },
}

impl PlayTrace {
    /// Create a new play trace.
    pub fn new(seed: u64, legacy_in: Option<BTreeMap<String, i64>>) -> PlayTrace {
        PlayTrace {
            format: 1,
            game: None,
            content_fingerprint: None,
            seed,
            legacy_in,
            steps: Vec::new(),
        }
    }

    /// Serialize trace to JSON string.
    pub fn to_json(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string_pretty(self)
    }

    /// Deserialize trace from JSON string.
    pub fn from_json(json: &str) -> Result<PlayTrace, serde_json::Error> {
        serde_json::from_str(json)
    }

    /// Reconstruct a PlayTrace from a WitnessPath and engine environment.
    pub fn from_witness(
        witness: &WitnessPath,
        turn: Time,
        seed: u64,
        legacy_in: Option<BTreeMap<String, i64>>,
    ) -> PlayTrace {
        let mut steps = Vec::new();
        for r in &witness.records {
            match &r.step {
                PlayerStep::Advance { dt } => {
                    let turns = if turn > 0 { (*dt / turn).max(1) } else { 1 };
                    steps.push(TraceStep::Advance { turns });
                }
                PlayerStep::Commit { verb, bindings } => {
                    let b = bindings
                        .iter()
                        .map(|(slot, ent)| (slot.clone(), format!("#{ent}")))
                        .collect();
                    steps.push(TraceStep::Commit {
                        verb: verb.clone(),
                        bindings: b,
                    });
                }
                PlayerStep::Bind { sit, slot, entity } => {
                    steps.push(TraceStep::Bind {
                        situation: *sit,
                        slot: slot.clone(),
                        card: format!("#{entity}"),
                    });
                }
            }
        }
        PlayTrace {
            format: 1,
            game: None,
            content_fingerprint: None,
            seed,
            legacy_in,
            steps,
        }
    }

    /// Execute the play trace against an engine booted from content, yielding
    /// a replay-verified WitnessPath.
    pub fn execute(
        &self,
        content: Arc<Content>,
        registry: EffectRegistry,
    ) -> Result<WitnessPath, String> {
        self.execute_with_seed(content, registry, self.seed)
    }

    /// Execute the play trace using a specific engine seed.
    pub fn execute_with_seed(
        &self,
        content: Arc<Content>,
        registry: EffectRegistry,
        seed: u64,
    ) -> Result<WitnessPath, String> {
        let (mut engine, diags) = Engine::boot(
            content,
            registry,
            seed,
            self.legacy_in.clone(),
        );
        if diags.iter().any(|d| d.is_error()) {
            return Err("boot diagnostics reported errors".into());
        }
        let start = engine.state().clone();
        let turn = engine.turn();
        let mut records = Vec::new();
        let mut actions = 0;

        for (idx, step) in self.steps.iter().enumerate() {
            let time_before = engine.state().now;
            match step {
                TraceStep::Advance { turns } => {
                    let dt = turn
                        .checked_mul(*turns)
                        .ok_or_else(|| format!("step {idx}: advance overflowed time"))?;
                    let events = engine
                        .advance(dt)
                        .map_err(|d| format!("step {idx} advance refused: {d}"))?;
                    let time_after = engine.state().now;
                    records.push(TransitionRecord {
                        step: PlayerStep::Advance { dt },
                        outcomes: Vec::new(),
                        probability: 1.0,
                        due_moments: Vec::new(),
                        events,
                        time_before,
                        time_after,
                        state_after: engine.state().clone(),
                        recipe: None,
                    });
                }
                TraceStep::Commit { verb, bindings } => {
                    let mut resolved = Vec::new();
                    for (slot, card_spec) in bindings {
                        let ent = resolve_entity(engine.state(), card_spec).ok_or_else(|| {
                            format!("step {idx}: card `{card_spec}` not found in state")
                        })?;
                        resolved.push((slot.clone(), ent));
                    }
                    let action = CommitAction {
                        verb: verb.clone(),
                        bindings: resolved.clone(),
                    };
                    let events = engine
                        .commit(action)
                        .map_err(|d| format!("step {idx} commit refused: {d}"))?;
                    let time_after = engine.state().now;
                    let recipe = events.iter().find_map(|e| match e {
                        story_engine::Event::SituationCompleted { recipe, .. } => {
                            Some(recipe.clone())
                        }
                        _ => None,
                    });
                    actions += 1;
                    records.push(TransitionRecord {
                        step: PlayerStep::Commit {
                            verb: verb.clone(),
                            bindings: resolved,
                        },
                        outcomes: Vec::new(),
                        probability: 1.0,
                        due_moments: Vec::new(),
                        events,
                        time_before,
                        time_after,
                        state_after: engine.state().clone(),
                        recipe,
                    });
                }
                TraceStep::Bind {
                    situation,
                    slot,
                    card,
                } => {
                    let ent = resolve_entity(engine.state(), card).ok_or_else(|| {
                        format!("step {idx}: card `{card}` not found in state")
                    })?;
                    let events = engine
                        .bind(*situation, slot.clone(), ent)
                        .map_err(|d| format!("step {idx} bind refused: {d}"))?;
                    let time_after = engine.state().now;
                    let recipe = engine
                        .state()
                        .situations
                        .get(situation)
                        .map(|s| s.recipe.clone());
                    actions += 1;
                    records.push(TransitionRecord {
                        step: PlayerStep::Bind {
                            sit: *situation,
                            slot: slot.clone(),
                            entity: ent,
                        },
                        outcomes: Vec::new(),
                        probability: 1.0,
                        due_moments: Vec::new(),
                        events,
                        time_before,
                        time_after,
                        state_after: engine.state().clone(),
                        recipe,
                    });
                }
            }
        }

        let depth = records.len();
        Ok(WitnessPath {
            start,
            records,
            actions,
            depth,
            probability: Some(1.0),
        })
    }
}

fn resolve_entity(state: &State, spec: &str) -> Option<EntityId> {
    if let Some(num) = spec.strip_prefix('#') {
        if let Ok(id) = num.parse::<u64>() {
            let ent_id = EntityId(id);
            if state.entities.contains_key(&ent_id) {
                return Some(ent_id);
            }
        }
    }
    // Match by entity name or prototype
    state
        .entities
        .values()
        .find(|e| e.name == spec || e.prototype == CardId::from(spec))
        .map(|e| e.id)
}
