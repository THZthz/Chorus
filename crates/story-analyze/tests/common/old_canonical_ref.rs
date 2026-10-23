//! The canonical state key (story-analyze spec §5.3): different action paths
//! that reach the same logical state collide in the visited set. Live
//! situations re-index densely in relative mint order, live entities
//! re-index in the symmetry order (equivalence-class-contiguous, ascending
//! within a class), and every reference — zones, bindings, relations —
//! rewrites to the canonical numbering. Allocator watermarks (`next_entity`,
//! `next_situation`, `next_zone`) are stripped; the kernel never reads a
//! situation's `source` tag after materialization, so it is dropped.

use std::collections::{BTreeMap, BTreeSet};

use story_content::Content;
use story_core::{
    Binding, CardId, DeckId, EndingId, EntityId, MintPolicy, RecipeId, SituationId, SlotName,
    State, Time, TriggerRuleId, VerbId, Zone, ZoneId, ZoneKind,
};

use story_analyze::symmetry::symmetry_classes;

/// A zone address under the canonical numbering. Declared zones keep their
/// content-fixed ids; only kernel-minted zone addresses — situation interiors
/// and carriers — re-index. The policy bits ride along.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ZoneAddr {
    /// Declared (and host-created) zones: content-fixed id.
    Fixed(ZoneId),
    /// A situation interior, addressed by the canonical situation index.
    Interior(u64),
    /// A card carrier, addressed by the canonical entity index.
    Carrier(u64),
    /// Kernel-minted zone whose owner is absent (a host hand-built the
    /// state); keyed by the raw owner id to stay total.
    OrphanInterior(SituationId),
    OrphanCarrier(EntityId),
}

/// One zone of the key: address plus the `public`/`bindable` policy bits.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ZoneRef {
    pub addr: ZoneAddr,
    pub public: bool,
    pub bindable: bool,
}

/// The mint policy as an orderable mirror of `story_core::MintPolicy`.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum MintKey {
    SingleCopy,
    Stackable,
    Unique,
    Group(String),
}

impl From<&MintPolicy> for MintKey {
    fn from(m: &MintPolicy) -> MintKey {
        match m {
            MintPolicy::SingleCopy => MintKey::SingleCopy,
            MintPolicy::Stackable => MintKey::Stackable,
            MintPolicy::Unique => MintKey::Unique,
            MintPolicy::Group(g) => MintKey::Group(g.clone()),
        }
    }
}

/// A canonical entity: every `Entity` field except the re-indexed id, with
/// the zone rewritten to its zone reference.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct CanonicalEntity {
    pub name: String,
    pub prototype: CardId,
    pub kind: Option<String>,
    pub aspects: BTreeMap<String, i64>,
    pub attrs: BTreeMap<String, i64>,
    pub zone: ZoneRef,
    pub mint: MintKey,
    pub expires_at: Option<Time>,
}

/// One binding of the key: the bound entity's canonical index, the zone it
/// was bound from, and the bind order (`Binding.seq` is observable in
/// card-book walks and `table` scope, so it is preserved).
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct CanonicalBinding {
    pub entity: u64,
    pub bound_from: ZoneRef,
    pub seq: u64,
}

/// A canonical situation: every `Situation` field except the re-indexed id
/// and the dropped `source` tag; the interior rides the zone list.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct CanonicalSituation {
    pub verb: VerbId,
    pub recipe: RecipeId,
    pub bindings: BTreeMap<SlotName, Vec<CanonicalBinding>>,
    pub started_at: Time,
    pub ends_at: Time,
}

/// One logical relation edge under the canonical numbering. A symmetric
/// kind's canonical subject is the smaller endpoint under the NEW numbering,
/// so a row stored under the mint-order subject re-canonicalizes when the
/// symmetry order flips the endpoints.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct RelationRow {
    pub subject: u64,
    pub kind: String,
    pub object: u64,
}

/// The canonical key of a state (story-analyze spec §5.3). Two states key
/// equal exactly when they are the same logical state up to entity
/// interchange within symmetry classes and situation mint order.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct CanonicalStateKey {
    pub now: Time,
    pub rng_counter: u64,
    pub ended: Option<EndingId>,
    pub decks: BTreeMap<DeckId, u64>,
    pub recipe_runs: BTreeMap<RecipeId, u32>,
    pub fired_once: BTreeSet<TriggerRuleId>,
    pub legacy: BTreeMap<String, i64>,
    pub zones: BTreeSet<ZoneRef>,
    pub entities: BTreeMap<u64, CanonicalEntity>,
    pub situations: BTreeMap<u64, CanonicalSituation>,
    pub relations: BTreeSet<RelationRow>,
}

impl CanonicalStateKey {
    /// Canonicalize one state. `content` supplies the relation kinds'
    /// symmetric flags (for the relation re-canonicalization) and the
    /// candidacy surfaces behind the symmetry classes.
    pub fn of(state: &State, content: &Content) -> CanonicalStateKey {
        // Canonical entity order: symmetry classes, signature-sorted,
        // ascending within a class.
        let classes = symmetry_classes(state, content);
        let mut entity_index: BTreeMap<EntityId, u64> = BTreeMap::new();
        let mut next = 0u64;
        for class in classes.iter() {
            for (offset, &e) in class.iter().enumerate() {
                let inserted = entity_index.insert(e, next + offset as u64);
                debug_assert!(inserted.is_none(), "an entity sits in one class");
            }
            next += class.len() as u64;
        }

        // Canonical situation order: relative mint order (ascending id),
        // which is the observable `(moment, phase, id)` tie-break order.
        let mut situation_index: BTreeMap<SituationId, u64> = BTreeMap::new();
        for (idx, (&sid, _)) in state.situations.iter().enumerate() {
            situation_index.insert(sid, idx as u64);
        }

        let zone_ref = |z: ZoneId, state: &State| -> ZoneRef {
            let (public, bindable) = state
                .zones
                .get(&z)
                .map(|z: &Zone| (z.public, z.bindable))
                .unwrap_or((false, false));
            let addr = match state.zones.get(&z).map(|z| &z.kind) {
                Some(ZoneKind::SituationInterior(s)) => match situation_index.get(s) {
                    Some(&idx) => ZoneAddr::Interior(idx),
                    None => ZoneAddr::OrphanInterior(*s),
                },
                Some(ZoneKind::Of(e)) => match entity_index.get(e) {
                    Some(&idx) => ZoneAddr::Carrier(idx),
                    None => ZoneAddr::OrphanCarrier(*e),
                },
                _ => ZoneAddr::Fixed(z),
            };
            ZoneRef {
                addr,
                public,
                bindable,
            }
        };

        // Every zone with its policy bits.
        let mut zones = BTreeSet::new();
        for &zid in state.zones.keys() {
            zones.insert(zone_ref(zid, state));
        }

        let mut entities = BTreeMap::new();
        for (&eid, ent) in &state.entities {
            let zone = zone_ref(ent.zone, state);
            let idx = entity_index.get(&eid).copied().unwrap_or(u64::MAX);
            entities.insert(
                idx,
                CanonicalEntity {
                    name: ent.name.clone(),
                    prototype: ent.prototype.clone(),
                    kind: ent.kind.clone(),
                    aspects: ent.aspects.clone(),
                    attrs: ent.attrs.clone(),
                    zone,
                    mint: MintKey::from(&ent.mint),
                    expires_at: ent.expires_at,
                },
            );
        }

        let bind = |b: &Binding, state: &State| CanonicalBinding {
            entity: entity_index.get(&b.entity).copied().unwrap_or(u64::MAX),
            bound_from: zone_ref(b.bound_from, state),
            seq: b.seq,
        };
        let mut situations = BTreeMap::new();
        for (&sid, sit) in &state.situations {
            let mut bindings: BTreeMap<SlotName, Vec<CanonicalBinding>> = BTreeMap::new();
            for (slot, binds) in &sit.bindings {
                let mut ordered: Vec<&Binding> = binds.iter().collect();
                ordered.sort_by_key(|b| b.seq);
                bindings.insert(
                    slot.clone(),
                    ordered.into_iter().map(|b| bind(b, state)).collect(),
                );
            }
            let idx = situation_index.get(&sid).copied().unwrap_or(u64::MAX);
            situations.insert(
                idx,
                CanonicalSituation {
                    verb: sit.verb.clone(),
                    recipe: sit.recipe.clone(),
                    bindings,
                    started_at: sit.started_at,
                    ends_at: sit.ends_at,
                },
            );
        }

        // Relations: one row per logical edge, re-indexed under the canonical
        // numbering; a symmetric kind's row lands under the smaller endpoint
        // of the new numbering.
        let symmetric = |kind: &str| {
            content
                .relation_kind(kind)
                .is_some_and(|k| k.symmetric)
        };
        let mut relations = BTreeSet::new();
        for (&subject, kinds) in &state.relations {
            for (kind, objects) in kinds {
                for object in objects {
                    let (s_idx, o_idx) = match (
                        entity_index.get(&subject),
                        entity_index.get(object),
                    ) {
                        (Some(&s), Some(&o)) => (s, o),
                        // An edge over an unindexed endpoint cannot exist in
                        // a kernel-maintained state; skip rather than miskey.
                        _ => continue,
                    };
                    let row = if symmetric(kind) {
                        RelationRow {
                            subject: s_idx.min(o_idx),
                            kind: kind.clone(),
                            object: s_idx.max(o_idx),
                        }
                    } else {
                        RelationRow {
                            subject: s_idx,
                            kind: kind.clone(),
                            object: o_idx,
                        }
                    };
                    relations.insert(row);
                }
            }
        }

        CanonicalStateKey {
            now: state.now,
            rng_counter: state.rng_counter,
            ended: state.ended.clone(),
            decks: state.decks.clone(),
            recipe_runs: state.recipe_runs.clone(),
            fired_once: state.fired_once.clone(),
            legacy: state.legacy.clone(),
            zones,
            entities,
            situations,
            relations,
        }
    }
}
