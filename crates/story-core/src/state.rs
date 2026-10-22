//! The state value (kernel-api.md §2). The snapshot is the save: every fact
//! the engine knows lives in this plain serializable value (L2).

use crate::ids::{
    CardId, DeckId, EndingId, EntityId, RecipeId, SituationId, SlotName, TriggerRuleId, VerbId,
    ZoneId,
};
use crate::Time;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

/// The relation-edge store: subject -> kind -> objects. One logical edge
/// occupies exactly one row; a symmetric kind's row lives under the canonical
/// subject `min(from, to)` and reads direction-agnostically. Destruction
/// drops every edge touching the dying card silently; `move` never touches
/// edges (relations are location-independent facts).
pub type Relations = BTreeMap<EntityId, BTreeMap<String, BTreeSet<EntityId>>>;

/// One removed logical edge, reported in its stored orientation.
pub type RemovedEdge = (EntityId, String, EntityId);

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
    /// Flat, directed, typed logical edges (spec.md §4.9); see `Relations`.
    pub relations: Relations,
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

// ---------------------------------------------------------------------------
// Pure edge operations (spec.md §4.9). Map ops only: the engine passes the
// declared kind's flags; nothing here reads `Content`.
// ---------------------------------------------------------------------------

/// Insert one logical edge. A symmetric kind stores it once, under the
/// canonical subject `min(from, to)`; a directed kind stores its own
/// orientation. Returns true when the edge was newly written.
pub fn relate_edge(
    relations: &mut Relations,
    from: EntityId,
    kind: &str,
    to: EntityId,
    symmetric: bool,
) -> bool {
    if from == to {
        return false;
    }
    let (subject, object) = if symmetric && to < from {
        (to, from)
    } else {
        (from, to)
    };
    relations
        .entry(subject)
        .or_default()
        .entry(kind.to_string())
        .or_default()
        .insert(object)
}

/// Remove one logical edge, trying both orientations; a symmetric row is
/// removed once. Empty kind and subject maps are pruned. Returns true when an
/// edge was removed.
pub fn unrelate_edge(relations: &mut Relations, from: EntityId, kind: &str, to: EntityId) -> bool {
    for (subject, object) in [(from, to), (to, from)] {
        let Some(kinds) = relations.get_mut(&subject) else {
            continue;
        };
        let Some(objects) = kinds.get_mut(kind) else {
            continue;
        };
        if objects.remove(&object) {
            if objects.is_empty() {
                kinds.remove(kind);
            }
            if kinds.is_empty() {
                relations.remove(&subject);
            }
            return true;
        }
    }
    false
}

/// Report every logical edge of `kind` whose object is `to` (the directed
/// exclusive steal), in map order.
pub fn edges_into(relations: &Relations, kind: &str, to: EntityId) -> Vec<RemovedEdge> {
    let mut out = Vec::new();
    for (subject, kinds) in relations {
        if let Some(objects) = kinds.get(kind) {
            if objects.contains(&to) {
                out.push((*subject, kind.to_string(), to));
            }
        }
    }
    out
}

/// Remove nothing; report every logical edge of `kind` touching either
/// endpoint (the symmetric exclusive steal), in map order.
pub fn edges_touching(
    relations: &Relations,
    kind: &str,
    a: EntityId,
    b: EntityId,
) -> Vec<RemovedEdge> {
    let mut out = Vec::new();
    for (subject, kinds) in relations {
        if let Some(objects) = kinds.get(kind) {
            for object in objects {
                if *subject == a || *subject == b || *object == a || *object == b {
                    out.push((*subject, kind.to_string(), *object));
                }
            }
        }
    }
    out
}

/// Remove every logical edge between the two cards across all kinds, either
/// direction (the endpoint set of each removed edge is exactly `{a, b}`),
/// each once, in map order. Returns the removed edges in stored orientation.
pub fn sever_pair(relations: &mut Relations, a: EntityId, b: EntityId) -> Vec<RemovedEdge> {
    let mut removed = Vec::new();
    for (subject, object) in [(a, b), (b, a)] {
        let Some(kinds) = relations.get_mut(&subject) else {
            continue;
        };
        let mut empty_kinds: Vec<String> = Vec::new();
        for (kind, objects) in kinds.iter_mut() {
            if objects.remove(&object) {
                removed.push((subject, kind.clone(), object));
            }
            if objects.is_empty() {
                empty_kinds.push(kind.clone());
            }
        }
        for kind in empty_kinds {
            kinds.remove(&kind);
        }
        if kinds.is_empty() {
            relations.remove(&subject);
        }
    }
    removed
}

/// Remove every logical edge touching `e` (death): its subject entry plus a
/// scan of rows where it appears as an object, each once, in map order.
/// Returns the removed edges in stored orientation; the caller decides
/// whether to speak (destruction stays silent).
pub fn drop_entity_edges(relations: &mut Relations, e: EntityId) -> Vec<RemovedEdge> {
    let mut removed = Vec::new();
    if let Some(kinds) = relations.remove(&e) {
        for (kind, objects) in kinds {
            for object in objects {
                removed.push((e, kind.clone(), object));
            }
        }
    }
    let mut subjects: Vec<EntityId> = Vec::new();
    for (subject, kinds) in relations.iter_mut() {
        let mut kind_names: Vec<String> = Vec::new();
        for (kind, objects) in kinds.iter_mut() {
            if objects.remove(&e) {
                removed.push((*subject, kind.clone(), e));
            }
            if objects.is_empty() {
                kind_names.push(kind.clone());
            }
        }
        for kind in kind_names {
            kinds.remove(&kind);
        }
        if kinds.is_empty() {
            subjects.push(*subject);
        }
    }
    for subject in subjects {
        relations.remove(&subject);
    }
    removed
}

/// Orientation-aware membership: a symmetric kind's edge holds either way.
pub fn edge_exists(
    relations: &Relations,
    from: EntityId,
    kind: &str,
    to: EntityId,
    symmetric: bool,
) -> bool {
    if relations
        .get(&from)
        .and_then(|k| k.get(kind))
        .is_some_and(|o| o.contains(&to))
    {
        return true;
    }
    if symmetric {
        return relations
            .get(&to)
            .and_then(|k| k.get(kind))
            .is_some_and(|o| o.contains(&from));
    }
    false
}
