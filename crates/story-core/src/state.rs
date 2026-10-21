//! The state value (kernel-api.md §2). The snapshot is the save: every fact
//! the engine knows lives in this plain serializable value (L2).

use crate::ids::{
    CardId, DeckId, EndingId, EntityId, RecipeId, SituationId, SlotName, TriggerRuleId, VerbId,
    ZoneId,
};
use crate::Time;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Debug, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct State {
    pub now: Time,
    pub rng_counter: u64,
    pub next_entity: u64,
    pub next_situation: u64,
    pub next_zone: u64,
    pub entities: BTreeMap<EntityId, Entity>,
    pub zones: BTreeMap<ZoneId, Zone>,
    pub situations: BTreeMap<SituationId, Situation>,
    /// Per-deck permutation cursors.
    pub decks: BTreeMap<DeckId, u64>,
    /// Per-recipe run counters; +1 at instantiation.
    pub recipe_runs: BTreeMap<RecipeId, u32>,
    /// Spent once-rules for the run.
    pub fired_once: BTreeSet<TriggerRuleId>,
    pub legacy: BTreeMap<String, i64>,
    /// Set by an `ending` effect; further entry points refuse.
    pub ended: Option<EndingId>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Entity {
    pub id: EntityId,
    pub name: String,
    pub prototype: CardId,
    /// Matches `kind:<k>` query terms.
    pub kind: Option<String>,
    /// The only card-carried integers the authored language sees.
    pub aspects: BTreeMap<String, i64>,
    /// Kernel-internal; host ops and bookkeeping only. No authored operand or
    /// effect writes an attribute.
    pub attrs: BTreeMap<String, i64>,
    pub zone: ZoneId,
    pub mint: MintPolicy,
    pub expires_at: Option<Time>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum MintPolicy {
    /// Default: one live copy per name; a mint onto a live same-name card is
    /// the hard `NameInUse` failure.
    SingleCopy,
    /// Multiple live copies of one name.
    Stackable,
    /// One live copy; a mint onto a live same-name card is skipped.
    Unique,
    /// While any member of the group lives, no other member mints.
    Group(String),
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Zone {
    pub id: ZoneId,
    pub kind: ZoneKind,
    pub public: bool,
    pub bindable: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ZoneKind {
    /// Declared zones.
    Table,
    /// Host-created hidden zones.
    Hidden,
    /// Situation interiors.
    SituationInterior(SituationId),
    /// Card carriers (`into <card>`).
    Of(EntityId),
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Situation {
    pub id: SituationId,
    pub verb: VerbId,
    pub source: SituationSource,
    pub recipe: RecipeId,
    pub interior: ZoneId,
    /// Each binding remembers the zone the card was bound from; locks are
    /// derived by walking these bindings, never stored twice.
    pub bindings: BTreeMap<SlotName, Vec<Binding>>,
    pub started_at: Time,
    pub ends_at: Time,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum SituationSource {
    Commit,
    Autonomous,
    Start { parent: SituationId },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Binding {
    pub entity: EntityId,
    pub bound_from: ZoneId,
    /// Bind order within the situation; card completion books walk bound
    /// cards ordered by first bind (kernel-api.md §8).
    pub seq: u64,
}
