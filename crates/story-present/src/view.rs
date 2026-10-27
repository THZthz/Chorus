//! View models and narrative presentation types.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use story_core::{EntityId, SituationId, Time, ZoneId};
use story_engine::Event;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct StoryOutput {
    pub beats: Vec<StoryBeat>,
    pub raw_events: Vec<Event>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct StoryBeat {
    pub kind: BeatKind,
    pub text: String,
    pub clock: Option<Time>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum BeatKind {
    ActionStart,
    ActionComplete,
    ActionContinue,
    NarrativeCue,
    RuleTriggered,
    WorldUpdate,
    Ending,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BoardView {
    pub clock: Time,
    pub legacy: BTreeMap<String, i64>,
    pub zones: Vec<ZoneView>,
    pub situations: Vec<SituationView>,
    pub ended: Option<EndingView>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ZoneView {
    pub zone_id: ZoneId,
    pub name: String,
    pub kind: String,
    pub cards: Vec<CardSummaryView>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CardSummaryView {
    pub entity_id: EntityId,
    pub entity_ids: Vec<EntityId>,
    pub locked_ids: Vec<EntityId>,
    pub prototype: String,
    pub title: String,
    pub kind: Option<String>,
    pub count: u32,
    pub aspects: BTreeMap<String, i64>,
    pub expires_at: Option<Time>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SituationView {
    pub id: SituationId,
    pub verb: String,
    pub verb_title: String,
    pub recipe: String,
    pub recipe_label: String,
    pub started_at: Time,
    pub ends_at: Time,
    pub remaining_time: Time,
    pub total_duration: Time,
    pub start_text: Option<String>,
    pub slotted: Vec<SlottedCardView>,
    pub open_slots: Vec<SlotPromptView>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SlottedCardView {
    pub slot: String,
    pub entity_id: EntityId,
    pub title: String,
    pub prototype: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct VerbOpportunityView {
    pub id: String,
    pub title: String,
    pub mode: String,
    pub instances: String,
    pub description: Option<String>,
    pub recipes: Vec<RecipeOpportunityView>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RecipeOpportunityView {
    pub id: String,
    pub name: String,
    pub label: String,
    pub priority: i64,
    pub duration: Time,
    pub craftable: bool,
    pub chain_only: bool,
    pub runnable: bool,
    pub slots: Vec<SlotPromptView>,
    pub guards: Vec<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SlotPromptView {
    pub name: String,
    pub label: String,
    pub bounds: String,
    pub flags: String,
    pub description: Option<String>,
    pub criteria: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CardLoreView {
    pub entity_id: Option<EntityId>,
    pub prototype: String,
    pub title: String,
    pub kind: Option<String>,
    pub description: Option<String>,
    pub aspects: BTreeMap<String, i64>,
    pub expires_at: Option<Time>,
    pub lifetime: Option<i64>,
    pub relations: Vec<(String, String, Option<EntityId>)>, // (kind, related_card_title, related_entity_id)
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct EndingView {
    pub id: String,
    pub title: String,
    pub tier: String,
    pub text: Option<String>,
}
