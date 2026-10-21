//! Serializable game-state values and pure functions (spec.md §3).

pub mod fingerprint;
pub mod ids;
pub mod rng;
pub mod snapshot;
pub mod state;

pub use ids::{
    CardId, DeckId, EndingId, EntityId, RecipeId, SituationId, SlotName, TriggerRuleId, VerbId,
    ZoneId,
};
pub use snapshot::{Snapshot, FORMAT_VERSION};
pub use state::{Binding, Entity, MintPolicy, Situation, SituationSource, State, Zone, ZoneKind};

/// The time unit is the minute; `clock`, durations, and expiry moments are all
/// minute counts (L1: `turn` constrains advance granularity only).
pub type Time = i64;
