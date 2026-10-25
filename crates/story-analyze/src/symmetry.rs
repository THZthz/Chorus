//! Entity symmetry (story-analyze spec §5.2): the inductive equivalence
//! signature of live entities, and the contiguity refinement that keeps
//! oldest-first targeting semantics exact. The canonical state key
//! (`canonical`) orders entities class-contiguously; the canonical binding
//! generation that also consumes these classes lands with the search.

use std::collections::{BTreeMap, BTreeSet};

use story_content::{Content, ir::QueryTerm};
use story_core::{CardId, EntityId, MintPolicy, SituationId, SlotName, State, ZoneId, ZoneKind};

use crate::causal::MatchOracle;

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

/// The zone class of the §5.2 signature: declared zones compare by their
/// content-fixed id (boot assigns them in declaration order, so the id is
/// stable across every path through one content), situation interiors by the
/// owning situation, and carrier zones abstractly — the specific carrier
/// enters through the carrier's own contents multiset.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ZoneClass {
    Fixed(ZoneId),
    InteriorOf(SituationId),
    Carrier,
}

/// The equivalence signature of one live entity (story-analyze spec §5.2):
/// every observable entity field, plus the canonical multiset of its carrier
/// contents, recursively. Computed bottom-up over carrier nesting, so the
/// definition is inductive, not circular. Two entities with identical fields
/// are not interchangeable if one carries cards and the other does not.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct EntityEquivSig {
    pub prototype: CardId,
    pub name: String,
    pub kind: Option<String>,
    pub aspects: BTreeMap<String, i64>,
    pub attrs: BTreeMap<String, i64>,
    pub zone_class: ZoneClass,
    pub mint: MintKey,
    pub expires_at: Option<i64>,
    /// The canonical multiset of the occupants of this entity's carrier
    /// zone (`ZoneKind::Of(entity_id)`), keyed by their own signatures.
    pub contents: BTreeMap<Box<EntityEquivSig>, u32>,
}

/// The signature of one live entity. A carrier-nesting cycle (a host
/// hand-built `State` can create one) is cut at the deepest occurrence: the
/// in-progress entity contributes an empty contents multiset there.
pub fn signature(state: &State, e: EntityId) -> EntityEquivSig {
    let mut in_progress = BTreeSet::new();
    sig_of(state, e, &mut in_progress)
}

fn sig_of(state: &State, e: EntityId, in_progress: &mut BTreeSet<EntityId>) -> EntityEquivSig {
    let ent = &state.entities[&e];
    let zone_class = match state.zones.get(&ent.zone).map(|z| &z.kind) {
        Some(ZoneKind::SituationInterior(s)) => ZoneClass::InteriorOf(*s),
        Some(ZoneKind::Of(_)) | None => ZoneClass::Carrier,
        Some(ZoneKind::Table) | Some(ZoneKind::Hidden) => ZoneClass::Fixed(ent.zone),
    };
    let mut contents = BTreeMap::new();
    if in_progress.insert(e) {
        if let Some(z) = state
            .zones
            .values()
            .find(|z| z.kind == ZoneKind::Of(e))
            .map(|z| z.id)
        {
            for occupant in state.entities.values().filter(|x| x.zone == z) {
                let sig = sig_of(state, occupant.id, in_progress);
                *contents.entry(Box::new(sig)).or_insert(0) += 1;
            }
        }
        in_progress.remove(&e);
    }
    EntityEquivSig {
        prototype: ent.prototype.clone(),
        name: ent.name.clone(),
        kind: ent.kind.clone(),
        aspects: ent.aspects.clone(),
        attrs: ent.attrs.clone(),
        zone_class,
        mint: MintKey::from(&ent.mint),
        expires_at: ent.expires_at,
        contents,
    }
}

/// Whether the entity is bound by any live instance (locks are derived by
/// walking situation bindings — spec.md §4.1).
pub fn is_bound(state: &State, e: EntityId) -> bool {
    state
        .situations
        .values()
        .any(|s| s.bindings.values().any(|b| b.iter().any(|b| b.entity == e)))
}

/// The symmetry classes of every live entity (story-analyze spec §5.2):
/// identical signatures place entities in one class, refined by contiguity —
/// two entities whose signatures agree still split when a live unbound
/// entity strictly between them in mint order carries a different signature
/// while sharing candidacy space with them. Batch-spawned cards allocate
/// contiguous ids, so full batch symmetry is preserved. Classes come back
/// signature-sorted, members ascending by id — the canonical entity order.
pub fn symmetry_classes(state: &State, content: &Content) -> Vec<Vec<EntityId>> {
    let cache = SymmetryCache::new(content);
    symmetry_classes_with(state, content, &cache)
}

/// The content-derived cache behind the symmetry classifier: the match
/// oracle plus the candidacy surfaces its contiguity refinement splits on.
/// Both depend only on the content, so the analyzer builds one per boot and
/// every state's classification reuses it.
#[derive(Clone, Debug)]
pub struct SymmetryCache {
    card_surfaces: BTreeMap<CardId, Vec<u32>>,
}

impl SymmetryCache {
    pub fn new(content: &Content) -> SymmetryCache {
        let _span = tracing::info_span!("symmetry_build").entered();
        let oracle = MatchOracle::new(content);
        let surfaces = candidacy_surfaces(content, &oracle);
        let mut card_surfaces: BTreeMap<CardId, Vec<u32>> = BTreeMap::new();
        for (idx, surface) in surfaces.into_iter().enumerate() {
            let idx = idx as u32;
            for card in surface {
                card_surfaces.entry(card).or_default().push(idx);
            }
        }
        SymmetryCache { card_surfaces }
    }

    /// Whether two card names can both match some targeting surface (slot
    /// filters, `Target::Query`/`BareQuery` resolution, destructive targets):
    /// some surface's can-match set contains both. Unknown prototypes share
    /// candidacy with everything (conservative splitting).
    pub fn shares_candidacy(&self, content: &Content, a: &CardId, b: &CardId) -> bool {
        if a == b {
            return true;
        }
        let known = |c: &CardId| content.card(c).is_some();
        if !known(a) || !known(b) {
            return true;
        }
        let (Some(sa), Some(sb)) = (self.card_surfaces.get(a), self.card_surfaces.get(b)) else {
            return false;
        };
        let mut i = 0;
        let mut j = 0;
        while i < sa.len() && j < sb.len() {
            match sa[i].cmp(&sb[j]) {
                std::cmp::Ordering::Equal => return true,
                std::cmp::Ordering::Less => i += 1,
                std::cmp::Ordering::Greater => j += 1,
            }
        }
        false
    }
}

/// The hot path: the same classification with a caller-held cache, so the
/// content-wide walks run once per analysis, not once per state.
pub fn symmetry_classes_with(
    state: &State,
    content: &Content,
    cache: &SymmetryCache,
) -> Vec<Vec<EntityId>> {
    let _span = tracing::info_span!("symmetry_classes").entered();

    // Ascending id = mint order. Signatures are built once and compared in
    // place — the grouping never clones one.
    let ids: Vec<EntityId> = state.entities.keys().copied().collect();
    let sigs: Vec<EntityEquivSig> = ids.iter().map(|&e| signature(state, e)).collect();
    let unbound: BTreeSet<EntityId> = ids
        .iter()
        .copied()
        .filter(|&e| !is_bound(state, e))
        .collect();

    // Signature-sorted order, members ascending by id (a stable sort keeps
    // the ascending ids in order within equal signatures).
    let mut order: Vec<usize> = (0..ids.len()).collect();
    order.sort_by(|&a, &b| sigs[a].cmp(&sigs[b]));

    // Walk the sorted order: a signature change closes the class and starts
    // a new group; inside a group, an interleaved unbound foreign card that
    // shares candidacy space splits the class (the group's signature
    // reference carries over the split).
    let mut classes: Vec<Vec<EntityId>> = Vec::new();
    let mut current: Vec<EntityId> = Vec::new();
    let mut group_head: Option<usize> = None;
    for &i in &order {
        if let Some(h) = group_head {
            if sigs[i] != sigs[h] {
                classes.push(std::mem::take(&mut current));
                group_head = None;
            }
        }
        if group_head.is_none() {
            group_head = Some(i);
        }
        if let Some(&prev) = current.last() {
            let sig = &sigs[group_head.unwrap()];
            let e = ids[i];
            let kidx = |k: EntityId| {
                ids.binary_search(&k).expect("an unbound live entity")
            };
            let blocked = unbound.iter().any(|&k| {
                k > prev
                    && k < e
                    && sigs[kidx(k)] != *sig
                    && cache.shares_candidacy(
                        content,
                        &sigs[kidx(k)].prototype,
                        &sig.prototype,
                    )
            });
            if blocked {
                classes.push(std::mem::take(&mut current));
            }
        }
        current.push(ids[i]);
    }
    if !current.is_empty() {
        classes.push(current);
    }

    classes
}

/// The can-match set of every targeting surface in the content.
fn candidacy_surfaces(content: &Content, oracle: &MatchOracle) -> Vec<BTreeSet<CardId>> {
    let mut surfaces = Vec::new();
    let mut add = |terms: &[QueryTerm]| {
        let cards = oracle.can_match_cards(content, terms);
        if cards.len() >= 2 {
            surfaces.push(cards.into_iter().collect());
        }
    };
    for verb in content.verbs.values() {
        for rec in verb.recipes.values() {
            for sd in &rec.slots {
                add(&sd.query);
            }
        }
    }
    crate::causal::visit_content_effects(content, &mut |n| match n {
        story_content::ir::EffectNode::Destroy { target, .. }
        | story_content::ir::EffectNode::Consume { target, .. }
        | story_content::ir::EffectNode::Swap { target, .. }
        | story_content::ir::EffectNode::Set { target, .. }
        | story_content::ir::EffectNode::Move { target, .. } => {
            if let story_content::ir::Target::Query { terms, .. }
            | story_content::ir::Target::BareQuery(terms) = target
            {
                add(terms);
            }
        }
        _ => {}
    });
    surfaces
}

// ---------------------------------------------------------------------------
// Canonical binding generation (story-analyze spec §5.2)
// ---------------------------------------------------------------------------

/// One canonical binding set for a commit: `(slot, entity)` pairs.
pub type BindingSet = Vec<(SlotName, EntityId)>;

/// The equivalence classes of one slot's candidate pool, each sorted
/// ascending by id (§5.2): the candidates the kernel reports for the slot,
/// partitioned by the entity symmetry classes of the state.
pub fn slot_candidate_classes(
    state: &State,
    content: &Content,
    candidates: &[EntityId],
) -> Vec<Vec<EntityId>> {
    let cache = SymmetryCache::new(content);
    slot_candidate_classes_with(state, content, &cache, candidates)
}

/// The hot path: partition candidate pool using a caller-held symmetry cache.
pub fn slot_candidate_classes_with(
    state: &State,
    content: &Content,
    cache: &SymmetryCache,
    candidates: &[EntityId],
) -> Vec<Vec<EntityId>> {
    let classes = symmetry_classes_with(state, content, cache);
    partition_candidates_by_classes(&classes, candidates)
}

/// Partition candidate entities by precomputed state symmetry classes.
pub fn partition_candidates_by_classes(
    classes: &[Vec<EntityId>],
    candidates: &[EntityId],
) -> Vec<Vec<EntityId>> {
    let class_of: BTreeMap<EntityId, usize> = classes
        .iter()
        .enumerate()
        .flat_map(|(i, c)| c.iter().map(move |&e| (e, i)))
        .collect();
    let mut grouped: BTreeMap<usize, Vec<EntityId>> = BTreeMap::new();
    for &e in candidates {
        grouped
            .entry(class_of.get(&e).copied().unwrap_or(usize::MAX))
            .or_default()
            .push(e);
    }
    let mut out: Vec<Vec<EntityId>> = grouped.into_values().collect();
    for c in &mut out {
        c.sort_unstable();
        c.dedup();
    }
    out.sort();
    out
}

/// Canonical choices for `k` picks from one candidate class: the
/// oldest-first prefix of length `k` (§5.2).
pub fn class_prefixes(class: &[EntityId], k: u32) -> Vec<Vec<EntityId>> {
    if k == 0 {
        return vec![Vec::new()];
    }
    if class.len() < k as usize {
        return Vec::new();
    }
    vec![class[..k as usize].to_vec()]
}

/// The pick-count band above the minimum of one slot's expansion: an
/// open-ended slot (`N..`) enumerates every count from `min` up to the
/// candidate pool's size within this band, and a bounded slot's `max` is
/// clamped to it. The band is the per-slot sanity bound (the pool itself
/// can be large); the exploration budget is the only global cap, and
/// deeper stackings stay reachable through repeated commits.
pub const MAX_PICK_BAND: u32 = 8;

/// Every canonical binding multiset for one slot: for each pick count in
/// `min..=top`, the Cartesian product of the class prefixes. An open-ended
/// slot (`max_picks` given as `None`) enumerates counts from `min` up to
/// `min(candidates, min + MAX_PICK_BAND)` — the baseline through the full
/// affordable stacking of the pool; a bounded slot enumerates `min..=max`
/// within the same band.
pub fn slot_choices(
    state: &State,
    content: &Content,
    candidates: &[EntityId],
    min: u32,
    max_picks: Option<u32>,
) -> Vec<Vec<EntityId>> {
    let cache = SymmetryCache::new(content);
    slot_choices_with(state, content, &cache, candidates, min, max_picks)
}

/// The hot path: compute slot choices using a caller-held symmetry cache.
pub fn slot_choices_with(
    state: &State,
    content: &Content,
    cache: &SymmetryCache,
    candidates: &[EntityId],
    min: u32,
    max_picks: Option<u32>,
) -> Vec<Vec<EntityId>> {
    if candidates.len() < min as usize {
        return Vec::new();
    }
    let classes = slot_candidate_classes_with(state, content, cache, candidates);
    slot_choices_from_classes(&classes, min, max_picks)
}

/// Compute slot choices from already-partitioned candidate classes.
pub fn slot_choices_from_classes(
    classes: &[Vec<EntityId>],
    min: u32,
    max_picks: Option<u32>,
) -> Vec<Vec<EntityId>> {
    let pool: u32 = classes.iter().map(|c| c.len() as u32).sum();
    if pool < min {
        return Vec::new();
    }
    let band = min.saturating_add(MAX_PICK_BAND);
    let top = match max_picks {
        Some(max) => max.min(band),
        None => pool.min(band),
    };
    let mut out = Vec::new();
    for k in min..=top {
        product_of_classes(classes, k, &mut out);
    }
    out
}

/// The Cartesian product of class prefixes for `k` total picks across the
/// classes: `k` picks split over the class prefixes without reuse. With one
/// class this is its length-`k` prefix; with several, every split of `k`
/// into per-class counts (bounded by each class's length) is enumerated.
fn product_of_classes(classes: &[Vec<EntityId>], k: u32, out: &mut Vec<Vec<EntityId>>) {
    fn walk(
        classes: &[Vec<EntityId>],
        remaining: u32,
        acc: &mut Vec<EntityId>,
        out: &mut Vec<Vec<EntityId>>,
    ) {
        if classes.is_empty() {
            if remaining == 0 {
                out.push(acc.clone());
            }
            return;
        }
        let class = &classes[0];
        let top = remaining.min(class.len() as u32);
        for take in 0..=top {
            // Only the oldest-first prefix of a class is canonical (§5.2).
            acc.extend(class[..take as usize].iter().copied());
            walk(&classes[1..], remaining - take, acc, out);
            acc.truncate(acc.len() - take as usize);
        }
    }
    let mut acc = Vec::new();
    walk(classes, k, &mut acc, out);
}
