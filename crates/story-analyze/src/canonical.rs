//! The canonical state key (story-analyze spec §5.3): different action paths
//! that reach the same logical state collide in the visited set. Live
//! situations re-index densely in relative mint order, live entities
//! re-index in the symmetry order (equivalence-class-contiguous, ascending
//! within a class), and every reference — zones, bindings, relations —
//! rewrites to the canonical numbering. Allocator watermarks (`next_entity`,
//! `next_situation`, `next_zone`) are stripped; the kernel never reads a
//! situation's `source` tag after materialization, so it is dropped.
//!
//! The key carries a 128-bit fingerprint of the canonical form instead of
//! the form itself: the form serializes to a length-prefixed byte stream
//! (fixed field order, BTreeMap order, symmetry order), and two
//! domain-separated SipHash streams digest it. Logically equal states
//! serialize identically and always key equal; distinct states collide only
//! on a 128-bit collision, far past practical reach.

use std::collections::{BTreeMap, HashMap};
use std::hash::{Hash, Hasher};

use story_content::Content;
use story_core::{
    Binding, EntityId, MintPolicy, SituationId, State, Time, ZoneId, ZoneKind,
};

use crate::symmetry::{symmetry_classes_with, SymmetryCache};

/// The canonical key of a state (§5.3). Two states key equal exactly when
/// they are the same logical state up to entity interchange within symmetry
/// classes and situation mint order.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct CanonicalStateKey(u128);

impl CanonicalStateKey {
    /// Canonicalize one state. `content` supplies the relation kinds'
    /// symmetric flags (for the relation re-canonicalization) and the
    /// candidacy surfaces behind the symmetry classes.
    pub fn of(state: &State, content: &Content) -> CanonicalStateKey {
        Self::of_with(state, content, &SymmetryCache::new(content))
    }

    /// The hot path: the same canonicalization with a caller-held cache, so
    /// the content-wide walks run once per analysis, not once per state.
    pub fn of_with(
        state: &State,
        content: &Content,
        cache: &SymmetryCache,
    ) -> CanonicalStateKey {
        let classes = symmetry_classes_with(state, content, cache);

        // Canonical entity order: symmetry classes occupy contiguous index
        // ranges, members ascending within a class; the class walk covers
        // every live entity exactly once.
        let mut entity_index: HashMap<EntityId, u64> = HashMap::with_capacity(classes.len());
        let mut next = 0u64;
        for class in &classes {
            for (offset, &e) in class.iter().enumerate() {
                entity_index.insert(e, next + offset as u64);
            }
            next += class.len() as u64;
        }

        // Canonical situation order: relative mint order (ascending id).
        let mut situation_index: HashMap<SituationId, u64> =
            HashMap::with_capacity(state.situations.len());
        for (idx, (&sid, _)) in state.situations.iter().enumerate() {
            situation_index.insert(sid, idx as u64);
        }

        let mut buf = Vec::with_capacity(512);
        put_i64(&mut buf, state.now);
        put_u64(&mut buf, state.rng_counter);
        match &state.ended {
            None => buf.push(0),
            Some(e) => {
                buf.push(1);
                put_str(&mut buf, e.as_str());
            }
        }
        put_u64(&mut buf, state.decks.len() as u64);
        for (deck, cursor) in &state.decks {
            put_str(&mut buf, deck.as_str());
            put_u64(&mut buf, *cursor);
        }
        put_u64(&mut buf, state.recipe_runs.len() as u64);
        for (recipe, runs) in &state.recipe_runs {
            put_str(&mut buf, recipe.as_str());
            put_u64(&mut buf, *runs as u64);
        }
        put_u64(&mut buf, state.fired_once.len() as u64);
        for rule in &state.fired_once {
            put_str(&mut buf, rule.as_str());
        }
        put_u64(&mut buf, state.legacy.len() as u64);
        for (key, value) in &state.legacy {
            put_str(&mut buf, key);
            put_i64(&mut buf, *value);
        }

        // Zones in the canonical ZoneRef order: address kind, address,
        // policy bits.
        let mut zone_refs: Vec<(u8, u64, u8, u8)> = Vec::with_capacity(state.zones.len());
        for (&zid, zone) in &state.zones {
            let (kind, addr) = match &zone.kind {
                ZoneKind::SituationInterior(s) => match situation_index.get(s) {
                    Some(&idx) => (1u8, idx),
                    None => (3u8, s.0),
                },
                ZoneKind::Of(e) => match entity_index.get(e) {
                    Some(&idx) => (2u8, idx),
                    None => (4u8, e.0),
                },
                _ => (0u8, zid.0),
            };
            zone_refs.push((kind, addr, zone.public as u8, zone.bindable as u8));
        }
        zone_refs.sort_unstable();
        for (kind, addr, public, bindable) in zone_refs {
            buf.push(kind);
            put_u64(&mut buf, addr);
            buf.push(public);
            buf.push(bindable);
        }

        // Entities in symmetry order.
        for class in &classes {
            for &e in class {
                let ent = &state.entities[&e];
                put_str(&mut buf, &ent.name);
                put_str(&mut buf, ent.prototype.as_str());
                match &ent.kind {
                    None => buf.push(0),
                    Some(k) => {
                        buf.push(1);
                        put_str(&mut buf, k);
                    }
                }
                put_map(&mut buf, &ent.aspects);
                put_map(&mut buf, &ent.attrs);
                put_zone(&mut buf, ent.zone, state, &entity_index, &situation_index);
                match &ent.mint {
                    MintPolicy::SingleCopy => buf.push(0),
                    MintPolicy::Stackable => buf.push(1),
                    MintPolicy::Unique => buf.push(2),
                    MintPolicy::Group(g) => {
                        buf.push(3);
                        put_str(&mut buf, g);
                    }
                }
                put_opt_time(&mut buf, ent.expires_at);
            }
        }

        // Situations in relative mint order; bindings in bind order.
        put_u64(&mut buf, state.situations.len() as u64);
        for sit in state.situations.values() {
            put_str(&mut buf, sit.verb.as_str());
            put_str(&mut buf, sit.recipe.as_str());
            put_u64(&mut buf, sit.bindings.len() as u64);
            for (slot, binds) in &sit.bindings {
                put_str(&mut buf, slot.as_str());
                let mut ordered: Vec<&Binding> = binds.iter().collect();
                ordered.sort_by_key(|b| b.seq);
                put_u64(&mut buf, ordered.len() as u64);
                for b in ordered {
                    put_u64(&mut buf, entity_index.get(&b.entity).copied().unwrap_or(u64::MAX));
                    put_zone(&mut buf, b.bound_from, state, &entity_index, &situation_index);
                    put_u64(&mut buf, b.seq);
                }
            }
            put_i64(&mut buf, sit.started_at);
            put_i64(&mut buf, sit.ends_at);
        }

        // Relations: one row per logical edge, re-indexed under the
        // canonical numbering; a symmetric kind's row lands under the
        // smaller endpoint of the new numbering. Rows sort as
        // (subject, kind, object), the key's own order.
        let symmetric = |kind: &str| content.relation_kind(kind).is_some_and(|k| k.symmetric);
        let mut rows: Vec<(u64, &str, u64)> = Vec::new();
        for (&subject, kinds) in &state.relations {
            for (kind, objects) in kinds {
                for object in objects {
                    let (Some(&s), Some(&o)) =
                        (entity_index.get(&subject), entity_index.get(object))
                    else {
                        // An edge over an unindexed endpoint cannot exist in
                        // a kernel-maintained state; skip rather than miskey.
                        continue;
                    };
                    if symmetric(kind) {
                        rows.push((s.min(o), kind, s.max(o)));
                    } else {
                        rows.push((s, kind, o));
                    }
                }
            }
        }
        rows.sort_unstable();
        for (subject, kind, object) in rows {
            put_u64(&mut buf, subject);
            put_str(&mut buf, kind);
            put_u64(&mut buf, object);
        }

        let mut a = std::collections::hash_map::DefaultHasher::new();
        a.write_u8(1);
        a.write(&buf);
        let mut b = std::collections::hash_map::DefaultHasher::new();
        b.write_u8(2);
        b.write(&buf);
        CanonicalStateKey(((a.finish() as u128) << 64) | b.finish() as u128)
    }
}

/// One zone reference: address kind, address, and the policy bits. The kind
/// tags follow the `ZoneAddr` variant order (fixed, interior, carrier,
/// orphan interior, orphan carrier), so the entity-sort order of the old
/// full-form key is preserved where it still applies.
fn put_zone(
    buf: &mut Vec<u8>,
    z: ZoneId,
    state: &State,
    entity_index: &HashMap<EntityId, u64>,
    situation_index: &HashMap<SituationId, u64>,
) {
    let (kind, addr) = match state.zones.get(&z).map(|zone| &zone.kind) {
        Some(ZoneKind::SituationInterior(s)) => match situation_index.get(s) {
            Some(&idx) => (1u8, idx),
            None => (3u8, s.0),
        },
        Some(ZoneKind::Of(e)) => match entity_index.get(e) {
            Some(&idx) => (2u8, idx),
            None => (4u8, e.0),
        },
        _ => (0u8, z.0),
    };
    buf.push(kind);
    put_u64(buf, addr);
}

fn put_map(buf: &mut Vec<u8>, map: &BTreeMap<String, i64>) {
    put_u64(buf, map.len() as u64);
    for (k, v) in map {
        put_str(buf, k);
        put_i64(buf, *v);
    }
}

fn put_opt_time(buf: &mut Vec<u8>, t: Option<Time>) {
    match t {
        None => buf.push(0),
        Some(t) => {
            buf.push(1);
            put_i64(buf, t);
        }
    }
}

fn put_u64(buf: &mut Vec<u8>, v: u64) {
    buf.extend_from_slice(&v.to_le_bytes());
}

fn put_i64(buf: &mut Vec<u8>, v: i64) {
    put_u64(buf, v as u64);
}

fn put_str(buf: &mut Vec<u8>, s: &str) {
    put_u64(buf, s.len() as u64);
    buf.extend_from_slice(s.as_bytes());
}
