//! The analysis host — ts: src/reach/reach.ts:456-978 (the WorldFacts
//! interface, computeWorldFacts' pre-fixpoint half) plus the identity-cache
//! replacement (REACH-07): TS memoizes world facts per (content, initial) in
//! two nested WeakMaps and the trigger index in a third; the port collapses
//! all three into one `Analyzer<'c>` holding `(content, initial)` with a
//! lazily-computed [`WorldFacts`]. The world-static predicate EVALUATORS
//! (evalStatic/pathOpen/writablePaths) live here; the static-shape PREDICATES
//! (`predStatic`) live in census.rs — TS's collectSites itself consumes
//! predStatic while tagging sites (reach.ts:307/320-322).

use std::sync::OnceLock;

use indexmap::{IndexMap, IndexSet};

use crate::content::content::{CompiledContent, CompiledTrigger, PrototypeDef};
use crate::core::predicate::{Pred, ValExpr};
use crate::core::query::{match_query, Query, QueryTerm};
use crate::core::types::{Aspects, Entity, State};
use crate::js::{ChorusError, Result};

use super::census::{
    collect_sites, draw_deck, object_always, pred_static, Census, RawSite, SiteKind, TrigIndex,
};

/// TS `Infinity` for the freeze-distance comparisons (plan §3.11): every
/// observed value is a first-crossing distance (1..=1440), so `i64::MAX`
/// preserves every comparison direction (`fc < freezeAt` always true when
/// unfrozen, `fc >= freezeDist` always false).
pub(crate) const INFINITY: i64 = i64::MAX;

// ---------------------------------------------------------------------------
// world-static predicate evaluation — ts: reach.ts:476-529 (verified B12)
// ---------------------------------------------------------------------------

/// The fixpoint's view of the available world (ts: reach.ts:481): distinct
/// facts with count-capped weights. Query evaluation over weights is EXACTLY
/// the evaluation over the flattened entity array — flatten pushed
/// min(count, 32) instances per fact.
#[derive(Debug, Clone)]
pub(crate) struct CardWorld {
    /// the flatCache version stamp (ported verbatim — see [`WorldFacts::push`])
    pub ver: u64,
    pub by_name: IndexMap<String, i64>,
    pub distinct: Vec<DistEntry>,
}

/// ts: reach.ts:481 — `{ e: Entity; w: number }`.
#[derive(Debug, Clone)]
pub(crate) struct DistEntry {
    pub e: Entity,
    pub w: i64,
}

/// ts: reach.ts:498-506
fn eval_expr_static(v: &ValExpr, world: &CardWorld) -> Result<i64> {
    match v {
        ValExpr::Int { v } => Ok(*v),
        ValExpr::CountQuery { query } => Ok(world
            .distinct
            .iter()
            .filter(|d| match_query(&d.e, query))
            .map(|d| d.w)
            .sum()),
        // ts: reach.ts:505 — preserved as Err (TS throws; capturable)
        _ => Err(ChorusError::new(
            "internal: evalExprStatic over a non-static operand",
        )),
    }
}

/// ts: reach.ts:508-523
pub(crate) fn eval_static(p: &Pred, world: &CardWorld) -> Result<bool> {
    match p {
        Pred::And(a, b) => {
            if !eval_static(a, world)? {
                Ok(false)
            } else {
                eval_static(b, world)
            }
        }
        Pred::Or(a, b) => {
            if eval_static(a, world)? {
                Ok(true)
            } else {
                eval_static(b, world)
            }
        }
        Pred::Not(a) => Ok(!eval_static(a, world)?),
        Pred::Cmp { l, op, r } => {
            let l = eval_expr_static(l, world)?;
            let r = eval_expr_static(r, world)?;
            Ok(op.num_cmp(l, r))
        }
        Pred::Exists { name } => Ok(world.by_name.contains_key(name)),
        Pred::ExistsQuery { query } => Ok(world
            .distinct
            .iter()
            .any(|d| match_query(&d.e, query))),
        // ts: reach.ts:521 — preserved as Err (TS throws; capturable)
        _ => Err(ChorusError::new(
            "internal: evalStatic over a non-static predicate",
        )),
    }
}

/// ts: reach.ts:527-529 — every condition on the path must be static AND true
/// against the current known world; anything else demotes the site.
pub(crate) fn path_open(path: &[Pred], world: &CardWorld) -> Result<bool> {
    for p in path {
        if pred_static(p) && eval_static(p, world)? {
            continue;
        }
        return Ok(false);
    }
    Ok(true)
}

/// ts: reach.ts:566-575 — the writable paths a query's aspect/state thresholds
/// demand, in SET-key form ('aspect:x' / 'state.x').
pub(crate) fn writable_paths(query: &[QueryTerm]) -> Vec<String> {
    query
        .iter()
        .filter_map(|t| match t {
            QueryTerm::Aspect { neg: false, path, .. } => Some(format!("aspect:{path}")),
            QueryTerm::State { neg: false, key, .. } => Some(format!("state.{key}")),
            _ => None,
        })
        .collect()
}

// ---------------------------------------------------------------------------
// the structured committability proof — ts: reach.ts:102-106
// ---------------------------------------------------------------------------

/// ts: reach.ts:102 — `{ name, how }` filler pair.
#[derive(Debug, Clone)]
pub struct SlotFiller {
    pub name: String,
    pub how: String,
}

/// ts: reach.ts:102-106 — one filled slot of a verb's committability proof.
#[derive(Debug, Clone)]
pub struct SlotProof {
    pub role: String,
    pub min: i64,
    pub query: Option<Query>,
    pub fillers: Vec<SlotFiller>,
}

// ---------------------------------------------------------------------------
// the available-card fact — ts: reach.ts:129-133
// ---------------------------------------------------------------------------

/// ts: reach.ts:129-133 — one available card: a static descriptor + how many
/// live copies it can contribute.
#[derive(Debug, Clone)]
pub(crate) struct CardFact {
    pub entity: Entity,
    pub count: i64,
    pub how: String,
}

// ---------------------------------------------------------------------------
// WorldFacts — ts: reach.ts:587-613
// ---------------------------------------------------------------------------

/// Everything target-independent the per-card analysis reads (ts: reach.ts
/// :579-586). Deterministic in (content, initial) — the analyzer never
/// mutates either. Identity keys are indices (plan §2.6.5): `committable` /
/// `fillers` are keyed by the content.verbs insertion index, `proven` /
/// `mint_sources` by the census site arena index; every insertion-ordered TS
/// Map/Set is an IndexMap/IndexSet (REACH-03: avail push order → slotSupport
/// candidate order → first-fit proofs; proven order = proof order → chain
/// first-hit and paths order).
pub(crate) struct WorldFacts<'c> {
    pub content: &'c CompiledContent,
    pub initial: &'c State,
    pub trig_idx: TrigIndex<'c>,
    /// the site census (arena + live/refused index lists)
    pub census: Census<'c>,
    /// available cards by name, fact lists in push order
    pub avail: IndexMap<String, Vec<CardFact>>,
    pub avail_ver: u64,
    /// the flatCache (ts: reach.ts:638) — ported verbatim including its
    /// version stamp
    pub flat_cache: Option<CardWorld>,
    /// one representative per name (descriptors; later-wins over
    /// State.entities order — same-name stackable mints in mid-game States)
    pub init_names: IndexMap<String, &'c Entity>,
    /// trigger owner → its non-stackable mint names (hoisted for siteProven's
    /// trigger-vs-recipe rule, 81915)
    pub trigger_mint_names: IndexMap<String, IndexSet<String>>,
    pub set_writes: IndexSet<String>,
    /// trigger owners with a deterministic refusal at their slot
    pub risky_owners: IndexSet<String>,
    pub frozen: bool,
    /// the freeze boundary in FIRST-CROSSING distance units; [`INFINITY`]
    /// when unfrozen
    pub freeze_dist: i64,
    /// owners whose every draft is refused at completion
    pub dirty_owners: IndexSet<String>,
    /// world-level notes, copied into every report
    pub notes: Vec<String>,
    /// verb index → its structured committability proof (proof order)
    pub committable: IndexMap<usize, Vec<SlotProof>>,
    /// site arena index → support lines (proof order)
    pub proven: IndexMap<usize, Vec<String>>,
    /// bare name → PROVEN sites minting it, in proof order (mintSources)
    pub mint_sources: IndexMap<String, Vec<usize>>,
    /// verb index → names its committability proof filled slots with
    pub fillers: IndexMap<usize, IndexSet<String>>,
    /// the universe of cards that can ever exist (static descriptors)
    pub universe: Vec<Entity>,
    /// an authored aspects= pair we cannot parse — cannot bound
    pub opaque_spawn_aspects: bool,
    pub derive_paths: IndexSet<String>,
    /// verbs still uncommittable at fixpoint whose min>0 slots can never be
    /// filled (by verb index)
    pub dead_verbs: IndexSet<usize>,
    /// deck id → capacity from proven sites
    pub deck_cap: IndexMap<String, i64>,
}

impl<'c> WorldFacts<'c> {
    /// ts: reach.ts:684-687 — minutes until the FIRST crossing of slot s (a
    /// start-slot trigger's first crossing is the next cycle — boundary ==
    /// now is already spent). The `d === 0 → 1440` branch is part of the
    /// semantics (REACH-14).
    pub(crate) fn first_cross(&self, slot: i64) -> i64 {
        let d = (slot * 30 - self.initial.now).rem_euclid(1440);
        if d == 0 {
            1440
        } else {
            d
        }
    }

    /// ts: reach.ts:676 — `protoOf`.
    pub(crate) fn proto_of(&self, name: &str) -> Option<&'c PrototypeDef> {
        self.content.prototypes.get(name)
    }

    /// ts: reach.ts:688-697 — synthesize a static entity descriptor (id
    /// `~${name}`; bornAction 0; deadAction null).
    pub(crate) fn synth(
        &self,
        name: &str,
        kind: &str,
        proto: Option<&PrototypeDef>,
        aspects_override: Option<Aspects>,
    ) -> Entity {
        Entity {
            id: format!("~{name}"),
            kind: kind.to_string(),
            name: name.to_string(),
            aspects: aspects_override
                .or_else(|| proto.and_then(|p| p.aspects.clone()))
                .unwrap_or_default(),
            attrs: proto.and_then(|p| p.attrs.clone()).unwrap_or_default(),
            born_action: Some(0),
            dead_action: None,
            stackable: proto.is_some_and(|p| p.stackable),
        }
    }

    /// ts: reach.ts:639-652 — `cardsOf`, over the flatCache. Verbatim
    /// mechanics: [`WorldFacts::push`] updates the cached view but not its
    /// `ver`, so every push invalidates and the next call rebuilds from
    /// `avail` (insertion order preserved — the TS "O(1) warmth" comment is
    /// defeated by the stale ver; reproduced, not repaired).
    pub(crate) fn cards_of(&mut self) -> CardWorld {
        if let Some(cache) = &self.flat_cache {
            if cache.ver == self.avail_ver {
                return cache.clone();
            }
        }
        let mut by_name: IndexMap<String, i64> = IndexMap::new();
        let mut distinct: Vec<DistEntry> = Vec::new();
        for list in self.avail.values() {
            for f in list {
                let w = std::cmp::min(f.count, 32);
                by_name.insert(
                    f.entity.name.clone(),
                    by_name.get(&f.entity.name).copied().unwrap_or(0) + w,
                );
                distinct.push(DistEntry {
                    e: f.entity.clone(),
                    w,
                });
            }
        }
        let cache = CardWorld {
            ver: self.avail_ver,
            by_name,
            distinct,
        };
        self.flat_cache = Some(cache.clone());
        cache
    }

    /// ts: reach.ts:653-664 — `push`.
    pub(crate) fn push(&mut self, name: String, fact: CardFact) {
        match self.avail.get_mut(&name) {
            Some(list) => list.push(fact.clone()),
            None => {
                self.avail.insert(name.clone(), vec![fact.clone()]);
            }
        }
        // ts: reach.ts:657-663 — the cache update without the ver bump (the
        // exact TS mechanics; see cards_of)
        if let Some(cache) = &mut self.flat_cache {
            let w = std::cmp::min(fact.count, 32);
            cache.by_name.insert(
                name.clone(),
                cache.by_name.get(&name).copied().unwrap_or(0) + w,
            );
            cache.distinct.push(DistEntry {
                e: fact.entity.clone(),
                w,
            });
        }
        self.avail_ver += 1;
    }
}

// ---------------------------------------------------------------------------
// Analyzer — the REACH-07 replacement for worldCache/trigIndexCache
// ---------------------------------------------------------------------------

/// The per-(content, initial) analysis host. Deterministic in its inputs; the
/// world facts are computed once on first use and shared by every subsequent
/// [`Analyzer::analyze_card`] call (the WeakMap-keyed worldFor shape). The
/// caller must not mutate content or initial between calls.
pub struct Analyzer<'c> {
    pub(crate) content: &'c CompiledContent,
    pub(crate) initial: &'c State,
    world: OnceLock<WorldFacts<'c>>,
}

impl<'c> Analyzer<'c> {
    #[must_use]
    pub fn new(content: &'c CompiledContent, initial: &'c State) -> Self {
        Analyzer {
            content,
            initial,
            world: OnceLock::new(),
        }
    }

    /// ts: reach.ts:617-623 — `worldFor` (compute once per pair).
    pub(crate) fn facts(&self) -> &WorldFacts<'c> {
        self.world.get_or_init(|| compute_world_facts(self.content, self.initial))
    }
}

// ---------------------------------------------------------------------------
// computeWorldFacts — ts: reach.ts:625-978 (pre-fixpoint half)
// ---------------------------------------------------------------------------

pub(crate) fn compute_world_facts<'c>(
    content: &'c CompiledContent,
    initial: &'c State,
) -> WorldFacts<'c> {
    let trig_idx = TrigIndex::build(content);

    // ts: reach.ts:665-671 — initial living entities, grouped by name; the
    // LAST entity with a name wins the representative slot (JS Map.set keeps
    // the original insertion position — IndexMap::insert likewise).
    let mut init_names: IndexMap<String, &'c Entity> = IndexMap::new();
    let mut init_count: IndexMap<String, i64> = IndexMap::new();
    for e in initial.entities.values() {
        if e.dead_action.is_some() {
            continue;
        }
        init_names.insert(e.name.clone(), e);
        *init_count.entry(e.name.clone()).or_insert(0) += 1;
    }

    let mut wf = WorldFacts {
        content,
        initial,
        trig_idx,
        census: Census::default(),
        avail: IndexMap::new(),
        avail_ver: 0,
        flat_cache: None,
        init_names,
        trigger_mint_names: IndexMap::new(),
        set_writes: IndexSet::new(),
        risky_owners: IndexSet::new(),
        frozen: false,
        freeze_dist: INFINITY,
        dirty_owners: IndexSet::new(),
        notes: Vec::new(),
        committable: IndexMap::new(),
        proven: IndexMap::new(),
        mint_sources: IndexMap::new(),
        fillers: IndexMap::new(),
        universe: Vec::new(),
        opaque_spawn_aspects: false,
        derive_paths: IndexSet::new(),
        dead_verbs: IndexSet::new(),
        deck_cap: IndexMap::new(),
    };

    // ts: reach.ts:672-674 — one avail fact per representative name
    let pushes: Vec<(String, CardFact)> = wf
        .init_names
        .iter()
        .map(|(name, e)| {
            (
                name.clone(),
                CardFact {
                    entity: (*e).clone(),
                    count: init_count.get(name).copied().unwrap_or(1),
                    how: format!("initial entity {name} ({})", e.kind),
                },
            )
        })
        .collect();
    for (name, fact) in pushes {
        wf.push(name, fact);
    }

    // ts: reach.ts:699-702 — the census (collisionNames seeded with the
    // initial non-stackable names inside)
    let census = collect_sites(content, &wf.init_names, &wf.trig_idx);
    wf.census = census;
    // ts: reach.ts:703 — the SET-written paths
    wf.set_writes = wf
        .census
        .set_sites
        .iter()
        .map(|s| s.key.clone())
        .collect();

    // ts: reach.ts:712-953 — the risky-trigger demotions (list-level mint
    // risk, same-slot pair rule, cross-slot timing, freeze line)
    risky_block(&mut wf);

    // ts: reach.ts:957-962 — dirtyOwners
    for &i in wf.census.live.iter().chain(&wf.census.refused) {
        let s = &wf.census.sites[i];
        if s.dup_risk {
            wf.dirty_owners.insert(s.owner.clone());
        }
    }
    // ts: reach.ts:965-971 — the freeze line: FIRST-CROSSING distance units,
    // NOT cyclic order (verification agent D, seed 65415 shape)
    let risky_triggers: Vec<&CompiledTrigger> = wf
        .content
        .triggers
        .iter()
        .filter(|t| {
            matches!(t, CompiledTrigger::Slot { .. })
                && wf.risky_owners.contains(&format!("trigger {}", t.id()))
        })
        .collect();
    wf.frozen = !risky_triggers.is_empty();
    if wf.frozen {
        debug_assert!(!risky_triggers.is_empty(), "frozen ⇒ at least one risky trigger");
        wf.freeze_dist = risky_triggers
            .iter()
            .map(|t| match t {
                CompiledTrigger::Slot { slot, .. } => wf.first_cross(*slot),
                CompiledTrigger::Expire { .. } => {
                    unreachable!("riskyTriggers filters to slot triggers (ts: reach.ts:965)")
                }
            })
            .min()
            .unwrap_or(INFINITY);
    }
    // ts: reach.ts:972-974 — the refused note
    if !wf.census.refused.is_empty() {
        wf.notes.push(format!(
            "{} producer site(s) sit behind an authored REFUSE — they can never apply and are excluded",
            wf.census.refused.len()
        ));
    }

    // ts: reach.ts:1061-1078 — the fixpoint (naive round-robin, REACH-10)
    super::fixpoint::run_fixpoint(&mut wf);

    // ts: reach.ts:1080-1263 — universe, dead verbs, deck caps
    super::paths::build_post_fixpoint(&mut wf);

    wf
}

/// ts: reach.ts:712-953 — the risky block: list-level mint risk against the
/// INITIAL world and CROSS-LIST trigger timing. Mutates census sites
/// (`dupRisk`) through arena indices exactly like the TS byList references
/// mutate the shared site objects.
fn risky_block(wf: &mut WorldFacts) {
    // ts: reach.ts:713-718 — byList: live ∪ refused grouped by owner
    let mut by_list: IndexMap<String, Vec<usize>> = IndexMap::new();
    for &i in wf.census.live.iter().chain(&wf.census.refused) {
        let owner = wf.census.sites[i].owner.clone();
        by_list.entry(owner).or_default().push(i);
    }

    /// `byList.get(owner) ?? []`
    fn idxs<'m>(m: &'m IndexMap<String, Vec<usize>>, k: &str) -> &'m [usize] {
        m.get(k).map_or(&[], Vec::as_slice)
    }

    /// ts: reach.ts:719-730 — collision-filtered mint names of a site list.
    fn mint_set_of(census: &Census, sites: &[usize]) -> IndexSet<String> {
        let mut names = IndexSet::new();
        for &i in sites {
            let s = &census.sites[i];
            if s.kind == SiteKind::Draw && s.deck.is_some() {
                let deck = draw_deck(s);
                for c in &deck.order {
                    if census.collision_names.contains(&c.name) {
                        names.insert(c.name.clone());
                    }
                }
                if let Some(dc) = &deck.defaultcard {
                    if census.collision_names.contains(&dc.name) {
                        names.insert(dc.name.clone());
                    }
                }
            } else if census.collision_names.contains(&s.name) {
                names.insert(s.name.clone());
            }
        }
        names
    }

    // ts: reach.ts:731-737 — a list minting the name of a living initial
    // non-stackable card collides on its first commit (seeds 40/43)
    let mut risky: IndexMap<String, bool> = IndexMap::new();
    for (owner, sites) in &by_list {
        let names = mint_set_of(&wf.census, sites);
        let val = names
            .iter()
            .any(|n| wf.init_names.get(n).is_some_and(|e| !e.stackable));
        risky.insert(owner.clone(), val);
    }

    // ts: reach.ts:738-744 — slot triggers by slot, declaration order
    let mut trig_by_slot: IndexMap<i64, Vec<String>> = IndexMap::new();
    for t in &wf.content.triggers {
        if let CompiledTrigger::Slot { id, slot, .. } = t {
            trig_by_slot
                .entry(*slot)
                .or_default()
                .push(format!("trigger {id}"));
        }
    }

    // ts: reach.ts:745-763 — same-slot pairs: a shared mint name makes the
    // pair mutually risky, and one risky trigger kills the whole EVENT PASS
    for owners in trig_by_slot.values() {
        for i in 0..owners.len() {
            for j in (i + 1)..owners.len() {
                let a = mint_set_of(&wf.census, idxs(&by_list, &owners[i]));
                let b = mint_set_of(&wf.census, idxs(&by_list, &owners[j]));
                if a.iter().any(|n| b.contains(n)) {
                    risky.insert(owners[i].clone(), true);
                    risky.insert(owners[j].clone(), true);
                }
            }
        }
        if owners.iter().any(|o| risky.get(o) == Some(&true)) {
            for o in owners {
                risky.insert(o.clone(), true);
            }
        }
    }

    // ts: reach.ts:771-782 — across DIFFERENT slots the LATER first crossing
    // demotes (fuzz seed 7475). trigFlat's sort is stability-irrelevant
    // (REACH-05): equal-c pairs are skipped below.
    let mut trig_flat: Vec<(String, i64)> = wf
        .content
        .triggers
        .iter()
        .filter_map(|t| match t {
            CompiledTrigger::Slot { id, slot, .. } => {
                Some((format!("trigger {id}"), wf.first_cross(*slot)))
            }
            _ => None,
        })
        .collect();
    trig_flat.sort_by(|x, y| x.1.cmp(&y.1));
    for i in 0..trig_flat.len() {
        for j in (i + 1)..trig_flat.len() {
            if trig_flat[i].1 == trig_flat[j].1 {
                continue; // same slot — handled above (both risky)
            }
            let a = mint_set_of(&wf.census, idxs(&by_list, &trig_flat[i].0));
            let b = mint_set_of(&wf.census, idxs(&by_list, &trig_flat[j].0));
            if a.iter().any(|n| b.contains(n)) {
                risky.insert(trig_flat[j].0.clone(), true);
            }
        }
    }

    // ts: reach.ts:793-801 — the gated-branch escape set: every non-stackable
    // mint name anywhere (initial copies included)
    let mut other_mints: IndexSet<String> = IndexSet::new();
    for e in wf.init_names.values() {
        if !e.stackable {
            other_mints.insert(e.name.clone());
        }
    }
    for sites in by_list.values() {
        for &i in sites {
            let s = &wf.census.sites[i];
            if s.derives && object_always(wf.content, s.verb) {
                continue; // derived mints carry a different name
            }
            if !wf.census.collision_names.contains(&s.name) {
                continue;
            }
            other_mints.insert(s.name.clone());
        }
    }

    // ts: reach.ts:802-815 — roll/role-gated mint arms join the risky set
    // when their mint name is ALSO minted elsewhere (fuzz seed 686)
    for (owner_o, sites_o) in &by_list {
        let gated: Vec<String> = sites_o
            .iter()
            .filter_map(|&i| {
                let s = &wf.census.sites[i];
                (s.runtime_gated && wf.census.collision_names.contains(&s.name))
                    .then(|| s.name.clone())
            })
            .collect();
        if gated.is_empty() {
            continue;
        }
        let elsewhere = other_mints.iter().any(|n| {
            if gated.contains(n) {
                // also minted by a DIFFERENT list?
                by_list.iter().any(|(o2, ss2)| {
                    o2 != owner_o
                        && ss2.iter().any(|&i2| {
                            let s2 = &wf.census.sites[i2];
                            s2.name == *n && !(s2.runtime_gated && gated.contains(&s2.name))
                        })
                })
            } else {
                true
            }
        });
        if elsewhere {
            for &i in sites_o {
                wf.census.sites[i].dup_risk = true;
            }
        }
    }

    // ts: reach.ts:827-830 — listBlocked
    let list_blocked = |wf: &WorldFacts, owner: &str| -> bool {
        if risky.get(owner) == Some(&true) {
            return true;
        }
        idxs(&by_list, owner)
            .iter()
            .any(|&i| wf.census.sites[i].dup_risk)
    };

    // ts: reach.ts:846-870 — deterministic-throw freeze (seeds 2876, 10388)
    let mut throw_frozen: IndexSet<String> = IndexSet::new();
    let decl_of = |owner: &str| -> i64 { wf.trig_idx.get(owner).map_or(-1, |(_, d)| d) };
    let first_cross_of = |owner: &str| -> Option<i64> {
        match wf.trig_idx.get(owner) {
            Some((CompiledTrigger::Slot { slot, .. }, _)) => Some(wf.first_cross(*slot)),
            _ => None, // recipes commit at player-chosen times
        }
    };
    for t in &wf.content.triggers {
        let CompiledTrigger::Slot { id, slot, .. } = t else {
            continue;
        };
        let owner = format!("trigger {id}");
        let my_first = wf.first_cross(*slot);
        let my_decl = decl_of(&owner);
        let replaces: Vec<&RawSite> = idxs(&by_list, &owner)
            .iter()
            .filter_map(|&i| {
                let s = &wf.census.sites[i];
                (s.kind == SiteKind::Replace && s.replace_target.is_some()).then_some(s)
            })
            .collect();
        if replaces.is_empty() {
            continue;
        }
        let self_killed = replaces.iter().any(|s| s.target_killed_in_list);
        let cross_killed = replaces.iter().any(|s| {
            wf.census
                .destroyed_by
                .get(s.replace_target.as_deref().unwrap_or(""))
                .is_some_and(|set| set.iter().any(|o| *o != owner))
        });
        if !self_killed && !cross_killed {
            continue;
        }
        let deterministic = self_killed
            || replaces.iter().any(|s| {
                wf.census
                    .destroyed_by
                    .get(s.replace_target.as_deref().unwrap_or(""))
                    .is_some_and(|set| {
                        set.iter().any(|o| {
                            if *o == owner {
                                return false;
                            }
                            match first_cross_of(o) {
                                None => false,
                                Some(fc) => fc < my_first || (fc == my_first && decl_of(o) < my_decl),
                            }
                        })
                    })
            });
        if deterministic {
            throw_frozen.insert(owner);
        }
    }

    // ts: reach.ts:871-888 — firstFreeze: the first crossing of a blocking
    // boundary freezes the clock there forever. REACH-13: the TS Map's
    // `set(d, min(get(d) ?? d, d))` is key==value — a Set; only
    // `min(keys)` is ever read.
    let mut first_freeze: Vec<i64> = Vec::new();
    for t in &wf.content.triggers {
        let CompiledTrigger::Slot { id, slot, once, .. } = t else {
            continue;
        };
        let owner = format!("trigger {id}");
        let recurring_minter =
            !once && !mint_set_of(&wf.census, idxs(&by_list, &owner)).is_empty();
        if list_blocked(wf, &owner) || recurring_minter {
            first_freeze.push(wf.first_cross(*slot));
        }
    }
    for owner in &throw_frozen {
        if let Some((CompiledTrigger::Slot { slot, .. }, _)) = wf.trig_idx.get(owner) {
            first_freeze.push(wf.first_cross(*slot));
        }
    }
    let freeze_at: i64 = first_freeze.iter().copied().min().unwrap_or(INFINITY);

    // ts: reach.ts:902-918 — same-slot pass-mates of a freezing trigger never
    // persist either; the exemption is EXACT (recurring minter standing ON
    // the boundary slot, clean first firings only)
    let boundary_slot: Option<i64> = if freeze_at != INFINITY {
        wf.content.triggers.iter().find_map(|t| match t {
            CompiledTrigger::Slot { slot, .. } if wf.first_cross(*slot) == freeze_at => Some(*slot),
            _ => None,
        })
    } else {
        None
    };
    let first_firing_throws =
        |wf: &WorldFacts, owner: &str| list_blocked(wf, owner) || throw_frozen.contains(owner);
    for t in &wf.content.triggers {
        let CompiledTrigger::Slot { id, slot, once, .. } = t else {
            continue;
        };
        let owner = format!("trigger {id}");
        let fc = wf.first_cross(*slot);
        if fc < freeze_at {
            continue;
        }
        let recurring_minter_self =
            !once && !mint_set_of(&wf.census, idxs(&by_list, &owner)).is_empty();
        let on_clean_boundary = match boundary_slot {
            Some(bs) => {
                *slot == bs
                    && recurring_minter_self
                    && !wf.content.triggers.iter().any(|o| {
                        matches!(o, CompiledTrigger::Slot { slot: s2, .. } if *s2 == *slot)
                            && first_firing_throws(wf, &format!("trigger {}", o.id()))
                    })
            }
            None => false,
        };
        if !on_clean_boundary {
            for &i in idxs(&by_list, &owner) {
                wf.census.sites[i].dup_risk = true;
            }
        }
    }

    // ts: reach.ts:919-930 — risky owners and deterministic throwers demote
    for (owner, is_risky) in &risky {
        if !*is_risky {
            continue;
        }
        wf.risky_owners.insert(owner.clone());
        for &i in idxs(&by_list, owner) {
            wf.census.sites[i].dup_risk = true;
        }
    }
    for owner in &throw_frozen {
        wf.risky_owners.insert(owner.clone());
        for &i in idxs(&by_list, owner) {
            wf.census.sites[i].dup_risk = true;
        }
    }
    // ts: reach.ts:941-947 — a draft-condemned trigger refuses deterministically
    // at its slot just the same (fuzz seed 65415)
    for t in &wf.content.triggers {
        let CompiledTrigger::Slot { id, .. } = t else {
            continue;
        };
        let owner = format!("trigger {id}");
        if !list_blocked(wf, &owner) {
            continue;
        }
        wf.risky_owners.insert(owner.clone());
        for &i in idxs(&by_list, &owner) {
            wf.census.sites[i].dup_risk = true;
        }
    }
    // ts: reach.ts:949-952 — hoisted for siteProven's trigger-vs-recipe rule
    for t in &wf.content.triggers {
        let CompiledTrigger::Slot { id, .. } = t else {
            continue;
        };
        let owner = format!("trigger {id}");
        let mints = mint_set_of(&wf.census, idxs(&by_list, &owner));
        wf.trigger_mint_names.insert(owner, mints);
    }
}
