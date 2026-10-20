//! Path export and the per-card report. The universe/dead-verbs/
//! deck-cap block finalizes the world facts after the fixpoint; [`analyze_card`]
//! folds the target-specific verdict, chain, path trees and notes on top —
//! notes under the convergence rule (a note naming a card rides only on
//! that name's report). The diagnostic faces live here too: [`Analyzer::card_blockers`]
//! (the `--why` table — the fixpoint ladder's Err side over the live
//! unproven serving sites) and [`Analyzer::verb_report`] (the `--verb`
//! report — the settled facts about one verb, zero new analysis).

use indexmap::{IndexMap, IndexSet};

use crate::content::{CompiledContent, CompiledDeck, CompiledTrigger, DeckCard};
use crate::core::dict::Dictionary;
use crate::core::query::{match_conjunction, match_query, parse_query, Query, QueryTerm};
use crate::core::types::{Aspects, Entity, State};
use crate::error::Result;

use super::census::{
    draw_blinds_bare, draw_deck, object_always, parse_aspects_json, site_aspects, RawSite,
    SiteKind, DERIVED_NAME, MANY,
};
use super::fixpoint::{slot_proof_lines, CardBlocker, SiteBlocker, SiteOp};
use super::render::{
    demand_chain_head, initial_decl_line, pred_text, query_text, site_chain, site_reason,
};
use super::world::{
    pred_static, writable_paths, writable_paths_of, Analyzer, CardFact, WorldFacts, INFINITY,
};

// ---------------------------------------------------------------------------
// report types
// ---------------------------------------------------------------------------

/// The analysis request knobs.
#[derive(Debug, Clone, Default)]
pub struct CardOptions {
    /// semantics (b): the card must also be bindable into some committable
    /// verb slot
    pub usable: bool,
    /// semantics (c): a §4.5 query the card must SATISFY; when set, `target`
    /// is ignored
    pub demand: Option<String>,
}

/// The verdict tier, surfaced as the report strings
/// `obtainable` / `unobtainable` / `unproven` / `unusable`.
///
/// The `Serialize` derive (the `--json` face) renames each variant to its
/// [`CardVerdict::as_str`] spelling — the JSON strings equal the report
/// strings, never the Rust variant names.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "lowercase")]
pub enum CardVerdict {
    Obtainable,
    Unobtainable,
    Unproven,
    Unusable,
}

impl CardVerdict {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            CardVerdict::Obtainable => "obtainable",
            CardVerdict::Unobtainable => "unobtainable",
            CardVerdict::Unproven => "unproven",
            CardVerdict::Unusable => "unusable",
        }
    }
}

/// The existence tier (report strings `proven` / `candidate` / `none`)
/// before usable is folded in.
///
/// The `Serialize` derive renames each variant to its
/// [`Existence::as_str`] spelling.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Existence {
    Proven,
    Candidate,
    None,
}

impl Existence {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Existence::Proven => "proven",
            Existence::Candidate => "candidate",
            Existence::None => "none",
        }
    }
}

/// The path op tag.
///
/// The `Serialize` derive renames each variant to its [`PathOp::as_str`]
/// spelling (`initial` lower-case, the site ops upper-case).
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum PathOp {
    #[serde(rename = "initial")]
    Initial,
    Spawn,
    Replace,
    Draw,
}

impl PathOp {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            PathOp::Initial => "initial",
            PathOp::Spawn => "SPAWN",
            PathOp::Replace => "REPLACE",
            PathOp::Draw => "DRAW",
        }
    }
}

/// DRAW only: where the card sits in the seed-fixed
/// permutation.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct PathDeck {
    pub id: String,
    pub position: Option<usize>,
    pub defaultcard: bool,
    pub capacity: Option<i64>,
}

/// One prerequisite card with its own provenance.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct Require {
    pub name: String,
    pub how: String,
    pub paths: Vec<ReachPath>,
}

/// One node of an exported acquisition path.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct ReachPath {
    /// display owner: `recipe work.r0 of WORK`, `trigger t3`,
    /// or `initial entity X`
    pub via: String,
    pub op: PathOp,
    /// the card this hop makes available
    pub mints: String,
    /// `mints` is a derived shape (`<PROTO>_<OBJECT>`), not the bare name
    pub derived_name: bool,
    pub deck: Option<PathDeck>,
    /// static-true IF / `when` conditions on the path, rendered
    pub guards: Vec<String>,
    /// proven support lines (committability / trigger firing / expiry arming)
    pub supports: Vec<String>,
    /// cards this hop needs, each with their own provenance
    pub requires: Vec<Require>,
}

/// A guarded source that keeps existence at `candidate`.
#[derive(Debug, Clone, serde::Serialize)]
pub struct CardCandidate {
    pub via: String,
    pub reason: String,
}

/// The per-card report.
///
/// The `Serialize` derive is the `--json` face (reach::export): the JSON
/// keys are the fields in declaration order (target / verdict / existence /
/// chain / paths / candidates / usable_via / notes — the text report's own
/// section order), the enums serialize as their `as_str` strings, and every
/// field is JSON-native (`String`/`bool`/`Option`/scalars/`Vec` — no
/// `IndexMap`, no `Entity`), so the bytes are deterministic and the derive
/// needs nothing beyond serde itself.
#[derive(Debug, Clone, serde::Serialize)]
pub struct CardReport {
    pub target: String,
    pub verdict: CardVerdict,
    pub existence: Existence,
    /// the FIRST proven path only (byte-stable; the full enumeration is
    /// `paths`)
    pub chain: Vec<String>,
    pub paths: Vec<ReachPath>,
    pub candidates: Vec<CardCandidate>,
    pub usable_via: Vec<String>,
    pub notes: Vec<String>,
}

/// The best-effort path depth backstop.
const MAX_PATH_DEPTH: usize = 12;

// ---------------------------------------------------------------------------
// post-fixpoint world facts
// ---------------------------------------------------------------------------

/// One static descriptor per available fact. The universe is scanned only
/// through `any(match…)` existence tests (`can_ever_match`, the demand
/// mode's `matches_somewhere`), so per-fact multiplicity would be
/// unobservable and is not materialized — exactly one descriptor per fact.
fn flatten(avail: &IndexMap<String, Vec<CardFact>>) -> Vec<Entity> {
    avail.values().flatten().map(|f| f.entity.clone()).collect()
}

/// The universe of cards that can ever exist, the
/// dead-verb set and the deck capacities from proven sites.
pub(crate) fn build_post_fixpoint(wf: &mut WorldFacts) {
    let content = wf.content;
    // Static mint-time descriptors: initial ∪ prototypes ∪ deck cards ∪ mint
    // names (with kind overrides and authored `aspects=` pair overrides).
    // Aspect values here are bounds, not facts.
    let mut universe: Vec<Entity> = flatten(&wf.avail);
    for (name, p) in &content.prototypes {
        universe.push(wf.synth(name, &p.kind, Some(p), None));
    }
    for deck in content.decks.values() {
        for c in &deck.order {
            universe.push(wf.synth(&c.name, &c.kind, content.prototypes.get(&c.name), None));
        }
        if let Some(dc) = &deck.defaultcard {
            universe.push(wf.synth(&dc.name, &dc.kind, content.prototypes.get(&dc.name), None));
        }
    }
    let mut opaque_spawn_aspects = false;
    for &i in &wf.census.live {
        let s = &wf.census.sites[i];
        if s.kind != SiteKind::Spawn {
            continue;
        }
        let p = content.prototypes.get(&s.name);
        let mut override_: Option<Aspects> = None;
        if let Some(json) = &s.aspects_json {
            match parse_aspects_json(json) {
                Ok(parsed) => {
                    // the authored pair overrides the prototype's aspects,
                    // key by key;
                    // the key order is canonicalized at the stringify points, so a
                    // plain insertion merge is observably identical
                    let mut merged = p.and_then(|p| p.aspects.clone()).unwrap_or_default();
                    for (k, v) in parsed {
                        merged.insert(k, v);
                    }
                    override_ = Some(merged);
                }
                Err(_) => {
                    // authored pair we cannot parse —
                    // cannot bound
                    opaque_spawn_aspects = true;
                }
            }
        }
        let kind = s
            .kind_override
            .clone()
            .or_else(|| p.map(|pp| pp.kind.clone()))
            .unwrap_or_else(|| "item".to_string());
        universe.push(wf.synth(&s.name, &kind, p, override_));
    }
    wf.universe = universe;
    wf.opaque_spawn_aspects = opaque_spawn_aspects;
    // derive-fed aspect paths
    for p in content.prototypes.values() {
        if let Some(d) = &p.derive {
            if let Some(aspects) = &d.aspects {
                for k in aspects.keys() {
                    wf.derive_paths.insert(format!("aspect:{k}"));
                }
            }
        }
    }
    // dead verbs
    for vi in 0..content.verbs.len() {
        if wf.committable.contains_key(&vi) {
            continue;
        }
        // §6.7: a chain-pinned verb's follow-up window is synthesized onto the
        // station directly (`synthesize_followup` → `register_windows`) and
        // validates no slots — over the empty event/parent bindings its
        // projection locks nothing, so the pinned recipe completes regardless
        // of the slot queries the dead-verb rule reads. The exemption only
        // widens (dead → alive): the verb stays uncommittable, so none of its
        // sites gains a proof — its mints hold existence at candidate.
        if wf.chain_pinned.contains(&vi) {
            continue;
        }
        // `slot_by_role`, not verb.slots: a shadowed
        // duplicate def must not doom the verb (fuzz seed 262)
        let Some((_, verb)) = content.verbs.get_index(vi) else {
            unreachable!("verb index {vi} came from content.verbs enumeration");
        };
        let hopeless = verb
            .slot_by_role
            .values()
            .any(|slot| slot.min > 0 && !wf.can_ever_match(slot.query.as_ref()));
        if hopeless {
            wf.dead_verbs.insert(vi);
        }
    }
    // deck capacity from proven sites (the horizon
    // model: pre-S* trigger draws count 1, at/after-S* count 0 — S* being
    // the freeze boundary and the --by window taking the tighter; on the
    // default face the horizon IS the freeze distance; recipe-owned
    // draws stay unbounded — commits at/short of the horizon are still
    // player-timed)
    let proven: Vec<usize> = wf.proven.keys().copied().collect();
    for si in proven {
        let site = &wf.census.sites[si];
        if site.kind != SiteKind::Draw {
            continue;
        }
        let mut cap = site.cap;
        if wf.horizon() != INFINITY {
            if site.verb.is_some() {
                cap = site.cap; // recipe-owned: commits at/short of the horizon are still unbounded
            } else {
                cap = match wf.trig_idx.get(&site.owner) {
                    Some((CompiledTrigger::Slot { slot, .. }, _))
                        if wf.first_cross(*slot) < wf.horizon() =>
                    {
                        1
                    }
                    _ => 0,
                };
            }
        }
        let entry = wf.deck_cap.entry(site.name.clone()).or_insert(0);
        *entry = (*entry).max(cap);
    }
}

impl<'c> WorldFacts<'c> {
    /// The §4.2 unique/group deck filter's dead-position test (the serving
    /// face of the engine's lazy draw rule): a position is dead when its
    /// card names a unique that is already MANIFESTED — in the initial
    /// state's set (the midgame truth, rebuilt through the mint path) or
    /// minted in-play by a proven SPAWN/REPLACE (the engine's mint face
    /// records the name permanently, whether or not the mint lands) — or a
    /// member of a GROUP that is already manifested (the same two-sided
    /// union on the group sets; one member's manifestation kills every
    /// member's entries) — or when it is a LATER occurrence of any unique
    /// OR member name — the earliest copy's serve (or skip) always
    /// manifests first, so a later occurrence never serves in any
    /// execution.
    pub(crate) fn deck_position_dead(&self, deck: &CompiledDeck, idx: usize) -> bool {
        let c = &deck.order[idx];
        let unique = self.content.mint_is_unique(&c.name);
        let group = self.content.mint_group(&c.name);
        (unique
            && (self.initial.manifested_uniques.contains(&c.name)
                || self.minted_manifested.contains(&c.name)))
            || (group.is_some_and(|g| {
                self.initial.manifested_groups.contains(g) || self.minted_groups.contains(g)
            }))
            || ((unique || group.is_some()) && deck.order[..idx].iter().any(|p| p.name == c.name))
    }

    /// The deck serving predicate with the midgame deck cursor folded in —
    /// ONE serving predicate, one home. `cap`
    /// counts the draws still AVAILABLE (the freeze/`--by` rewrite's firing
    /// count, or MANY) and they serve the position window `[cursor, …)`
    /// — on a non-reset deck the positions before the replayed cursor are
    /// already consumed and never return, and the §4.2 dead positions (a
    /// manifested unique, or a later occurrence of any unique) cost no draw:
    /// a target's reach is measured in LIVE positions before it, and the
    /// defaultcard's face at true exhaustion over the LIVE order length. The
    /// default face reads no cursors and no manifested set (script states
    /// carry none), so the arithmetic IS the old raw-position face verbatim
    /// there — the graph face (export.rs) shares this exact verdict with
    /// analyze_name / analyze_demand.
    pub(crate) fn deck_serves_cap(&self, deck: &CompiledDeck, target: &str, cap: i64) -> bool {
        let n = deck.order.len();
        // live positions in order[..to]
        let live_before = |to: usize| -> i64 {
            deck.order[..to]
                .iter()
                .enumerate()
                .filter(|(i, _)| !self.deck_position_dead(deck, *i))
                .count() as i64
        };
        let position = deck.order.iter().position(|c| c.name == target);
        match position {
            Some(idx) => {
                if self.deck_position_dead(deck, idx) {
                    return false;
                }
                if deck.reset {
                    // the order is periodic, but a UNIQUE or group-member
                    // target serves at most once ever (its first serve —
                    // or skip — manifests the name or the group — every
                    // later period skips it), so the first-period face is
                    // the only one; with no uniques this is the raw
                    // `cap > idx`
                    return cap > live_before(idx);
                }
                let cursor = self.initial.decks.get(&deck.id).copied().unwrap_or(0);
                if (idx as i64) < cursor {
                    // a consumed order position never serves again
                    return false;
                }
                let consumed_live = live_before(cursor.clamp(0, n as i64) as usize);
                cap > live_before(idx) - consumed_live
            }
            None => {
                if !deck.defaultcard.as_ref().is_some_and(|dc| dc.name == target) {
                    return false;
                }
                let live_len = live_before(n);
                if deck.reset {
                    // a reset deck reaches the defaultcard only at TRUE
                    // exhaustion: every position dead (its every unique
                    // manifested — the B9.4 filtered reshuffle leaving an
                    // empty pile, B10's defaultcard face)
                    return live_len == 0 && cap > 0;
                }
                let cursor = self.initial.decks.get(&deck.id).copied().unwrap_or(0);
                let consumed_live = live_before(cursor.clamp(0, n as i64) as usize);
                cap > live_len - consumed_live
            }
        }
    }

    /// Can a card matching `query` EVER exist? No memo is needed: the
    /// function is pure over fields that are
    /// final once the world facts are built, so memoization cannot change any
    /// result (a value-equal query yields a value-equal answer).
    pub(crate) fn can_ever_match(&self, query: Option<&Query>) -> bool {
        let Some(q) = query else {
            return true;
        };
        // §4.5: a DNF can ever match iff ANY group can — each group is a
        // flat conjunction, so the possibility analysis is
        // per group; the zero-group query stays vacuous-true
        q.is_empty() || q.iter().any(|g| self.group_can_ever_match(g))
    }

    /// The possibility check for ONE conjunction group (the
    /// pre-§4.5 body, unchanged). No memo is needed: the function is pure
    /// over fields that are final once the world facts are built.
    fn group_can_ever_match(&self, group: &[QueryTerm]) -> bool {
        if self.universe.iter().any(|u| match_conjunction(u, group)) {
            return true;
        }
        if self.opaque_spawn_aspects {
            return true;
        }
        let paths = writable_paths_of(group);
        if paths.is_empty() {
            return false; // purely structural — the universe is exhaustive
        }
        if paths.iter().any(|p| self.set_writes.contains(p)) {
            return true;
        }
        if paths.iter().any(|p| self.derive_paths.contains(p)) {
            return true;
        }
        false
    }

    /// The provenance chain, one line per hop (first
    /// proven source only).
    pub(crate) fn proven_source_for(&self, name: &str) -> Option<Vec<String>> {
        if let Some(e) = self.init_names.get(name) {
            return Some(vec![initial_decl_line(name, e)]);
        }
        for (&si, sup) in &self.proven {
            let site = &self.census.sites[si];
            let hit = if site.kind == SiteKind::Draw {
                !draw_blinds_bare(self.content, site, name)
                    && self.deck_serves_cap(
                        draw_deck(site),
                        name,
                        self.deck_cap.get(&site.name).copied().unwrap_or(0),
                    )
            } else {
                site.name == name
            };
            if hit {
                return Some(site_chain(site, name, sup));
            }
        }
        None
    }

    /// Every proven source for a name — the SAME
    /// predicate as the chain walk (derive-behind-mandatory-object sites never
    /// mint the bare name; a draw must be unblinded and its position within
    /// deck capacity).
    ///
    /// Indexed: `mint_sources` was built at each proof in `proven`'s insertion
    /// order under EXACTLY this predicate minus the deck-cap test (the cap
    /// freezes only after `build_post_fixpoint`, which is why the index can't
    /// absorb it), so the candidate list here is the whole-`proven` scan
    /// filtered to the same hits in the same order — the cap check then keeps
    /// the identical survivors. Selection order (first-fit path export) is
    /// untouched.
    pub(crate) fn proven_sources(&self, name: &str) -> Vec<(usize, Vec<String>)> {
        let mut out: Vec<(usize, Vec<String>)> = Vec::new();
        if let Some(candidates) = self.mint_sources.get(name) {
            for &si in candidates {
                let site = &self.census.sites[si];
                if site.kind == SiteKind::Draw
                    && !self.deck_serves_cap(
                        draw_deck(site),
                        name,
                        self.deck_cap.get(&site.name).copied().unwrap_or(0),
                    )
                {
                    continue;
                }
                let sup = self
                    .proven
                    .get(&si)
                    .unwrap_or_else(|| unreachable!("mint_sources entries are always proven"))
                    .clone();
                let pair = (si, sup);
                // content may list one name twice in a deck's order (a legal
                // multiset — only stackable draws consume copies); the old
                // whole-`proven` scan visited each proven site once, so keep
                // the indexed candidates unique the same way
                if out.last() != Some(&pair) {
                    out.push(pair);
                }
            }
        }
        out
    }

    /// The initial-declaration leaf path.
    pub(crate) fn initial_path(&self, name: &str) -> ReachPath {
        let Some(e) = self.init_names.get(name) else {
            unreachable!("initial_path is only called under initNames.has");
        };
        ReachPath {
            via: format!("initial entity {name} ({})", e.kind),
            op: PathOp::Initial,
            mints: name.to_string(),
            derived_name: false,
            deck: None,
            guards: Vec::new(),
            supports: vec![initial_decl_line(name, e)],
            requires: Vec::new(),
        }
    }

    /// Unfold one proven site into a path node.
    /// `seen` carries the filler names already expanded on this branch (the
    /// fixpoint's strict proof order means no true cycle exists); the depth
    /// cap is the best-effort backstop. The set is threaded `&mut` — the name
    /// goes in for the subtree and comes back out for the next sibling, which
    /// yields the per-branch membership (the set is membership-only; its
    /// iteration order is unobservable).
    pub(crate) fn path_for(
        &self,
        si: usize,
        mints: &str,
        sup: &[String],
        depth: usize,
        seen: &mut IndexSet<String>,
        derived: bool,
    ) -> ReachPath {
        let site = &self.census.sites[si];
        let mut guards: Vec<String> =
            site.path.iter().filter(|p| pred_static(p)).map(pred_text).collect();
        if let Some(when) = &site.when {
            if pred_static(when) {
                guards.push(pred_text(when));
            }
        }
        let mut path = ReachPath {
            via: site.where_.clone(),
            op: match site.kind {
                SiteKind::Draw => PathOp::Draw,
                SiteKind::Replace => PathOp::Replace,
                SiteKind::Spawn => PathOp::Spawn,
            },
            mints: mints.to_string(),
            derived_name: derived,
            deck: None,
            guards,
            supports: sup.to_vec(),
            requires: Vec::new(),
        };
        if site.kind == SiteKind::Draw {
            let deck = draw_deck(site);
            let pos = deck.order.iter().position(|c| c.name == mints);
            path.deck = Some(PathDeck {
                id: deck.id.clone(),
                position: pos,
                defaultcard: pos.is_none(),
                capacity: if site.cap != MANY { Some(site.cap) } else { None },
            });
        }
        // preconditions: the chosen committability
        // proof's slot fillers, or the expiry trigger's named card; slot
        // triggers carry none
        if depth < MAX_PATH_DEPTH {
            let mut needed: Vec<(String, String)> = Vec::new();
            if let Some(vi) = site.verb {
                if let Some(proofs) = self.committable.get(&vi) {
                    for pf in proofs {
                        for f in &pf.fillers {
                            if !needed.iter().any(|(n, _)| n == &f.name) {
                                needed.push((f.name.clone(), f.how.clone()));
                            }
                        }
                    }
                }
            } else if let Some((CompiledTrigger::Expire { on_expire_of, .. }, _)) =
                self.trig_idx.get(&site.owner)
            {
                needed.push((
                    on_expire_of.clone(),
                    "must be alive and able to expire (the trigger fires only on its expiry)"
                        .to_string(),
                ));
            }
            for (name, how) in needed {
                let mut sub: Vec<ReachPath> = Vec::new();
                if self.init_names.contains_key(&name) {
                    sub.push(self.initial_path(&name));
                } else if !seen.contains(&name) {
                    let sources = self.proven_sources(&name);
                    seen.insert(name.clone());
                    for (src_si, src_sup) in sources {
                        sub.push(self.path_for(src_si, &name, &src_sup, depth + 1, seen, false));
                    }
                    seen.swap_remove(&name);
                }
                path.requires.push(Require { name, how, paths: sub });
            }
        }
        path
    }
}

/// Does the deck carry the target at all? (`pub(crate)`: the `--why`
/// table's serving predicate shares it.)
pub(crate) fn deck_serves(deck: &CompiledDeck, target: &str) -> bool {
    deck.order.iter().any(|c| c.name == target)
        || deck.defaultcard.as_ref().is_some_and(|dc| dc.name == target)
}

// ---------------------------------------------------------------------------
// analyze_card
// ---------------------------------------------------------------------------

impl<'c> Analyzer<'c> {
    /// The per-card report; world facts are shared across
    /// calls. `target` is ignored under `demand`.
    pub fn analyze_card(&self, target: &str, opts: CardOptions) -> Result<CardReport> {
        let facts = self.facts();
        // note convergence (§13.1 revision): a world note that NAMES a card
        // rides only on that name's report — `facts.notes` is no longer
        // copied wholesale into every card (the nightjar expiry note used to
        // ride on all 44 --all reports). Notes pushed BELOW (per-report
        // candidate/demand/derive notes) are unaffected.
        let mut notes: Vec<String> = world_notes_for(facts, target);
        let mut chain: Vec<String> = Vec::new();
        let mut paths: Vec<ReachPath> = Vec::new();
        let mut candidates: Vec<CardCandidate> = Vec::new();
        let demand_q: Option<Query> = match opts.demand.as_deref() {
            // an EMPTY demand string counts as no demand (the frozen truthiness of
            // `''`); parse_query("") would return an empty query
            // without erroring and must not enter demand mode
            Some(d) if !d.is_empty() => Some(parse_query(d, self.dict)?),
            _ => None,
        };

        // the existence branch: demand mode (semantics (c)) or bare-name mode
        let (existence, demand_match) = match &demand_q {
            Some(dq) => analyze_demand(
                facts,
                dq,
                opts.demand.as_deref(),
                &mut chain,
                &mut paths,
                &mut notes,
            ),
            None => {
                let existence = analyze_name(
                    facts,
                    target,
                    &mut chain,
                    &mut paths,
                    &mut candidates,
                    &mut notes,
                );
                (existence, None)
            }
        };

        // (b) usability
        let descriptor: Option<Entity> = match (&demand_match, facts.init_names.get(target)) {
            (Some(d), _) => Some(d.clone()),
            (None, Some(e)) => Some((*e).clone()),
            (None, None) => {
                facts.proto_of(target).map(|p| facts.synth(target, &p.kind, Some(p), None))
            }
        };
        if existence == Existence::Proven && descriptor.is_none() {
            notes.push(format!(
                "no static descriptor for '{target}' (minted without a declared prototype?) — usability could not be checked"
            ));
        }
        let usable_via = match &descriptor {
            Some(d) => usable_lines(facts, d),
            None => Vec::new(),
        };

        // the verdict
        let verdict = match existence {
            Existence::None => CardVerdict::Unobtainable,
            Existence::Candidate => CardVerdict::Unproven,
            Existence::Proven => {
                if opts.usable && usable_via.is_empty() {
                    CardVerdict::Unusable
                } else {
                    CardVerdict::Obtainable
                }
            }
        };

        Ok(CardReport {
            target: target.to_string(),
            verdict,
            existence,
            chain,
            paths,
            candidates,
            usable_via,
            notes,
        })
    }
}

// ---------------------------------------------------------------------------
// the --why table and the --verb report (both over the settled facts)
// ---------------------------------------------------------------------------

/// One slot of the `--verb` report.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerbSlotRow {
    pub role: String,
    pub min: i64,
    /// design-time spelling; `None` = the slot binds anything
    pub query: Option<String>,
}

/// One mint site's ladder verdict inside a recipe row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SiteVerdict {
    Proven,
    /// the ladder's first blocker — the same [`CardBlocker`] the `--why`
    /// table reports
    Unproven(CardBlocker),
}

/// One mint site of a recipe.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerbSiteRow {
    pub op: SiteOp,
    /// spawn/replace: the minted name; draw: the deck id
    pub name: String,
    pub verdict: SiteVerdict,
}

/// One recipe line of the `--verb` report.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerbRecipeRow {
    pub id: String,
    /// design-time spelling; `None` = the default (no authored `when`)
    pub when: Option<String>,
    /// §6.4 the recipe's own window override (`dur`); `None` = the verb
    /// default
    pub dur: Option<i64>,
    /// any live site of this recipe may be shadowed by an earlier recipe
    pub shadow_risk: bool,
    /// the live mint sites, census walk order (refused sites are excluded —
    /// they can never apply)
    pub sites: Vec<VerbSiteRow>,
}

/// The `--verb` report: everything the settled facts already hold about one
/// verb — duration, slots, the committability proof (the proof lines are
/// `slot_proof_lines`' own output) and the per-recipe mint-site verdicts.
/// ZERO new analysis.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerbReport {
    pub verb: String,
    /// the window length in minutes
    pub duration: i64,
    /// `slot_by_role` order (the commit semantics — last def per role wins)
    pub slots: Vec<VerbSlotRow>,
    pub committable: bool,
    /// a min>0 slot's query can never match (`facts.dead_verbs`)
    pub dead: bool,
    pub proofs: Vec<String>,
    pub recipes: Vec<VerbRecipeRow>,
}

impl<'c> Analyzer<'c> {
    /// The `--why` table: every live unproven site serving `target` with the
    /// ladder's FIRST blocker — the very pass `site_proven` runs, over the
    /// read-only world rebuild. Serving is `analyze_name`'s candidate-scan
    /// predicate (draws unblinded and deck-carried, other sites minting the
    /// bare name; dead verbs' sites and derive-mandated mints excluded);
    /// proven sites never enter the table.
    #[must_use]
    pub fn card_blockers(&self, target: &str) -> Vec<SiteBlocker> {
        let facts = self.facts();
        let world = facts.build_world();
        let mut out: Vec<SiteBlocker> = Vec::new();
        for &i in &facts.census.live {
            if facts.proven.contains_key(&i) {
                continue;
            }
            let s = &facts.census.sites[i];
            let verb_alive = s.verb.is_none_or(|v| !facts.dead_verbs.contains(&v));
            let not_derive_mandated = !(s.derives && object_always(facts.content, s.verb));
            let serves = if s.kind == SiteKind::Draw {
                !draw_blinds_bare(facts.content, s, target) && deck_serves(draw_deck(s), target)
            } else {
                s.name == target
            };
            if !(verb_alive && not_derive_mandated && serves) {
                continue;
            }
            // post-fixpoint convergence: an unproven site's ladder is Err
            let (res, _) = facts.site_ladder(i, &world);
            if let Err(blocker) = res {
                out.push(SiteBlocker {
                    owner: s.owner.clone(),
                    op: match s.kind {
                        SiteKind::Spawn => SiteOp::Spawn,
                        SiteKind::Replace => SiteOp::Replace,
                        SiteKind::Draw => SiteOp::Draw,
                    },
                    name: s.name.clone(),
                    blocker,
                });
            } else {
                debug_assert!(false, "unproven site proved in the --why pass (schedule drift)");
            }
        }
        out
    }

    /// The `--verb` report over the facts already computed — zero new
    /// analysis. `None` when no verb bears the name (the CLI's analyze
    /// error, exit 1 like a bad demand).
    #[must_use]
    pub fn verb_report(&self, name: &str) -> Option<VerbReport> {
        let facts = self.facts();
        let vi = facts.content.verbs.get_index_of(name)?;
        let v = facts.content.verbs.get(name)?;
        let world = facts.build_world();
        let slots: Vec<VerbSlotRow> = v
            .slot_by_role
            .values()
            .map(|s| VerbSlotRow {
                role: s.role.clone(),
                min: s.min,
                query: s.query.as_ref().map(query_text),
            })
            .collect();
        let committable = facts.committable.contains_key(&vi);
        let dead = facts.dead_verbs.contains(&vi);
        let proofs = match facts.committable.get(&vi) {
            Some(proofs) => slot_proof_lines(name, proofs),
            None => Vec::new(),
        };
        let mut recipes: Vec<VerbRecipeRow> = Vec::new();
        for r in &v.recipes {
            let owner = format!("recipe {} of {}", r.id, v.name);
            let mut sites: Vec<VerbSiteRow> = Vec::new();
            let mut shadow_risk = false;
            for &i in &facts.census.live {
                let s = &facts.census.sites[i];
                if s.owner != owner {
                    continue;
                }
                shadow_risk |= s.shadow_risk;
                let verdict = if facts.proven.contains_key(&i) {
                    SiteVerdict::Proven
                } else {
                    // post-fixpoint convergence: an unproven site's ladder is Err
                    let (res, _) = facts.site_ladder(i, &world);
                    match res {
                        Err(blocker) => SiteVerdict::Unproven(blocker),
                        Ok(_) => {
                            debug_assert!(
                                false,
                                "unproven site proved in the --verb pass (schedule drift)"
                            );
                            SiteVerdict::Unproven(CardBlocker::UnknownGuard)
                        }
                    }
                };
                sites.push(VerbSiteRow {
                    op: match s.kind {
                        SiteKind::Spawn => SiteOp::Spawn,
                        SiteKind::Replace => SiteOp::Replace,
                        SiteKind::Draw => SiteOp::Draw,
                    },
                    name: s.name.clone(),
                    verdict,
                });
            }
            recipes.push(VerbRecipeRow {
                id: r.id.clone(),
                when: r.when.as_ref().map(pred_text),
                dur: r.dur,
                shadow_risk,
                sites,
            });
        }
        Some(VerbReport {
            verb: v.name.clone(),
            duration: v.duration,
            slots,
            committable,
            dead,
            proofs,
            recipes,
        })
    }
}

/// Semantics (c): ANY obtainable card satisfying the demand query, matched
/// against each source's ACTUAL product descriptor, never the bare
/// prototype. Fills the chain, paths and demand notes; returns the
/// existence tier and the matching descriptor.
fn analyze_demand(
    facts: &WorldFacts,
    dq: &Query,
    demand: Option<&str>,
    chain: &mut Vec<String>,
    paths: &mut Vec<ReachPath>,
    notes: &mut Vec<String>,
) -> (Existence, Option<Entity>) {
    let content = facts.content;
    let mut existence = Existence::None;
    let mut demand_match: Option<Entity> = None;
    for e in facts.init_names.values() {
        if !match_query(e, dq) {
            continue;
        }
        if existence != Existence::Proven {
            existence = Existence::Proven;
            demand_match = Some((*e).clone());
            chain.push(demand_chain_head(&e.name, &e.kind, &e.aspects, demand));
            match facts.proven_source_for(&e.name) {
                Some(lines) => chain.extend(lines),
                None => {
                    unreachable!("initNames hit ⇒ provenSourceFor returns the initial line")
                }
            }
        }
        paths.push(facts.initial_path(&e.name));
    }
    if existence == Existence::None {
        // the proven-site descriptor scan
        for (&si, sup) in &facts.proven {
            let site = &facts.census.sites[si];
            let descs: Vec<(String, Entity, bool)> = if site.kind == SiteKind::Draw {
                let deck = draw_deck(site);
                let mut entries: Vec<&DeckCard> = deck.order.iter().collect();
                if let Some(dc) = &deck.defaultcard {
                    entries.push(dc);
                }
                entries
                    .iter()
                    .map(|c| {
                        let desc =
                            facts.synth(&c.name, &c.kind, content.prototypes.get(&c.name), None);
                        // a draw of a
                        // derive-card behind an object-mandatory verb
                        // never carries the bare name (12320)
                        let blind = draw_blinds_bare(content, site, &c.name);
                        let mut desc = desc;
                        if blind {
                            desc.name = DERIVED_NAME.to_string();
                        }
                        (c.name.clone(), desc, blind)
                    })
                    .collect()
            } else {
                let proto = content.prototypes.get(&site.name);
                let kind = site
                    .kind_override
                    .clone()
                    .or_else(|| proto.map(|pp| pp.kind.clone()))
                    .unwrap_or_else(|| "item".to_string());
                let desc = facts.synth(&site.name, &kind, proto, site_aspects(site));
                let blind = site.derives && object_always(content, site.verb);
                let mut desc = desc;
                if blind {
                    desc.name = DERIVED_NAME.to_string();
                }
                vec![(site.name.clone(), desc, blind)]
            };
            for (name, desc, derived) in descs {
                if !match_query(&desc, dq) {
                    continue;
                }
                if site.kind == SiteKind::Draw
                    && !facts.deck_serves_cap(
                        draw_deck(site),
                        &name,
                        facts.deck_cap.get(&site.name).copied().unwrap_or(0),
                    )
                {
                    continue;
                }
                if existence != Existence::Proven {
                    existence = Existence::Proven;
                    demand_match = Some(desc.clone());
                    // the chain shows the REAL
                    // name, never the blinding sentinel
                    let shown = if derived {
                        "a derived-mint card (name formed from the bound object)".to_string()
                    } else {
                        name.clone()
                    };
                    chain.push(demand_chain_head(&shown, &desc.kind, &desc.aspects, demand));
                    chain.extend(site_chain(site, &name, sup));
                }
                paths.push(facts.path_for(si, &name, sup, 0, &mut IndexSet::new(), derived));
            }
        }
    }
    if existence == Existence::None {
        // a static descriptor may match behind
        // guarded sites; a threshold may be pumped by SET or fed by
        // derive; any of these keeps 'none' unprovable
        let matches_somewhere = facts.universe.iter().any(|u| match_query(u, dq));
        let write_paths = writable_paths(dq);
        let pumps: Vec<&String> =
            write_paths.iter().filter(|p| facts.set_writes.contains(*p)).collect();
        let derives: Vec<&String> =
            write_paths.iter().filter(|p| facts.derive_paths.contains(*p)).collect();
        for p in &pumps {
            let owners: Vec<&str> = facts
                .census
                .set_sites
                .iter()
                .filter(|s| &s.key == *p)
                .map(|s| s.owner.as_str())
                .collect();
            notes.push(format!(
                "'{p}' is written by SET at: {} — the threshold can be pumped at play time",
                owners.join(", ")
            ));
        }
        for p in &derives {
            notes.push(format!(
                "'{p}' is derive-fed at mint time (from the bound object's aspects) — its threshold cannot be bounded statically"
            ));
        }
        if matches_somewhere {
            notes.push(
                "a static descriptor matches the demand but no proven source reaches it — witness needed"
                    .to_string(),
            );
        }
        existence = if matches_somewhere
            || !pumps.is_empty()
            || !derives.is_empty()
            || facts.opaque_spawn_aspects
        {
            Existence::Candidate
        } else {
            Existence::None
        };
    }
    (existence, demand_match)
}

/// The bare-name existence branch: the initial hit, the proven-site scan,
/// then the guarded-candidate tiers and the flat candidate listing.
fn analyze_name(
    facts: &WorldFacts,
    target: &str,
    chain: &mut Vec<String>,
    paths: &mut Vec<ReachPath>,
    candidates: &mut Vec<CardCandidate>,
    notes: &mut Vec<String>,
) -> Existence {
    let content = facts.content;
    if facts.init_names.contains_key(target) {
        // an initial entity is proven outright
        match facts.proven_source_for(target) {
            Some(lines) => chain.extend(lines),
            None => unreachable!("initNames hit ⇒ provenSourceFor returns the initial line"),
        }
        paths.push(facts.initial_path(target));
        return Existence::Proven;
    }
    let mut existence = Existence::None;
    // the proven-site scan for the bare name
    for (&si, sup) in &facts.proven {
        let site = &facts.census.sites[si];
        // a derive-mint behind an
        // object-mandatory verb never mints the BARE name (12320)
        if site.derives && object_always(content, site.verb) {
            continue;
        }
        let hit = if site.kind == SiteKind::Draw {
            !draw_blinds_bare(content, site, target)
                && facts.deck_serves_cap(
                    draw_deck(site),
                    target,
                    facts.deck_cap.get(&site.name).copied().unwrap_or(0),
                )
        } else {
            site.name == target
        };
        if !hit {
            continue;
        }
        if existence != Existence::Proven {
            existence = Existence::Proven;
            chain.extend(site_chain(site, target, sup));
        }
        paths.push(facts.path_for(si, target, sup, 0, &mut IndexSet::new(), false));
    }
    if existence == Existence::None {
        // no proven path, but a live guarded
        // site still blocks 'unobtainable'
        let candidate_exists = facts.census.live.iter().any(|&i| {
            let s = &facts.census.sites[i];
            let verb_alive = s.verb.is_none_or(|v| !facts.dead_verbs.contains(&v));
            let not_derive_mandated = !(s.derives && object_always(content, s.verb));
            let serves = if s.kind == SiteKind::Draw {
                !draw_blinds_bare(content, s, target)
                    && deck_serves(draw_deck(s), target)
                    // a PROVEN draw site whose
                    // cap cannot reach this position is still a guarded
                    // source in a bounded world (81401); unbounded caps
                    // are exact structural facts
                    && (!facts.proven.contains_key(&i)
                        || (facts.horizon() != INFINITY
                            && !facts.deck_serves_cap(
                                draw_deck(s),
                                target,
                                facts.deck_cap.get(&s.name).copied().unwrap_or(0),
                            )))
            } else {
                s.name == target && !facts.proven.contains_key(&i)
            };
            verb_alive && not_derive_mandated && serves
        });
        if candidate_exists {
            existence = Existence::Candidate;
        }
    }
    if existence == Existence::None {
        // every producer of the bare name is a
        // derive-mint behind an object-mandatory verb — BUT the bare
        // name is still reachable when the object binding dies
        // mid-window (seed 30790)
        let derived_only = !facts.init_names.contains_key(target)
            && live_sites_serving(facts, target).any(|s| {
                // a dead verb's sites never
                // produce; they must not negate the derive-drop escape
                // (seed 91492)
                s.verb.is_none_or(|v| !facts.dead_verbs.contains(&v))
                    && !(s.derives && object_always(content, s.verb))
            });
        let any_derived = facts.census.live.iter().any(|&i| {
            let s = &facts.census.sites[i];
            if s.kind == SiteKind::Draw {
                draw_blinds_bare(content, s, target)
            } else {
                s.derives && s.name == target && object_always(content, s.verb)
            }
        });
        if !derived_only && any_derived {
            existence = Existence::Candidate;
            notes.push(
                format!("'{target}' derives its name from the bound object at every commit (the verb's object slot is mandatory) — normally the bare name never mints, but a mid-window death of the bound object drops the binding at completion and the derive silently no-ops")
            );
        }
    }
    // derived mints can mint names we cannot
    // enumerate; their shape must never be called structurally absent
    let derived_pattern = content.prototypes.iter().any(|(pname, p)| {
        p.derive.as_ref().and_then(|d| d.name_from.as_deref()) == Some("object")
            && target.starts_with(&format!("{pname}_"))
    });
    if existence == Existence::None && derived_pattern {
        existence = Existence::Candidate;
        notes.push(format!(
            "'{target}' matches a derived-mint shape (deriving name from object) — the object half is chosen at play time, so structural absence cannot be proven"
        ));
    }
    // guarded (candidate) sources listed flat
    if existence == Existence::Candidate && chain.is_empty() {
        for &i in &facts.census.live {
            if facts.proven.contains_key(&i) {
                continue;
            }
            let s = &facts.census.sites[i];
            let serves = if s.kind == SiteKind::Draw {
                deck_serves(draw_deck(s), target)
            } else {
                s.name == target
            };
            if serves {
                let mut reason = site_reason(s, &facts.init_names);
                if let Some(t) = time_guard_note(facts, s, target) {
                    // the bounded-time faces replace the bare "unknown
                    // guard" fallback with the honest timing reason
                    reason = if reason == "unknown guard" { t } else { format!("{reason}; {t}") };
                }
                notes.push(format!("candidate source: {} — guarded ({})", s.where_, reason));
                candidates.push(CardCandidate { via: s.where_.clone(), reason });
            }
        }
    }
    existence
}

/// The bounded-time guard of a live site — the candidate reason's timing
/// half: a draw whose order position the replayed deck cursor already
/// consumed (a non-reset permutation never returns), a slot trigger whose
/// first crossing lies beyond the `--by` minute (it has not fired yet), or
/// an already-fired once trigger of the replayed face (it never fires
/// again). `None` when the site carries no time guard — ALWAYS `None` on
/// the default face (no cursors, no bound, no replayed firings), so the
/// listing bytes hold.
fn time_guard_note(facts: &WorldFacts, s: &RawSite, target: &str) -> Option<String> {
    if s.kind == SiteKind::Draw {
        let deck = draw_deck(s);
        if !deck.reset {
            let cursor = facts.initial.decks.get(&deck.id).copied().unwrap_or(0);
            if let Some(pos) = deck.order.iter().position(|c| c.name == target) {
                if (pos as i64) < cursor {
                    return Some(format!(
                        "deck {} has already drawn past order position {pos} (a non-reset permutation never returns)",
                        deck.id
                    ));
                }
            }
        }
    }
    if let Some(vi) = s.verb {
        // a duration>0 recipe cannot complete inside a bounded horizon —
        // the site's OWN recipe's length (`dur` overrides the verb's, §6.4)
        if let Some(m) = facts.by {
            if let Some((_, v)) = facts.content.verbs.get_index(vi) {
                let dur = super::world::effective_duration(v, s.recipe);
                if dur > 0 {
                    let owner = if s
                        .recipe
                        .is_some_and(|i| v.recipes.get(i).is_some_and(|r| r.dur.is_some()))
                    {
                        "recipe"
                    } else {
                        "verb"
                    };
                    return Some(format!(
                        "the {owner}'s {dur}m window completes past the by {m} horizon"
                    ));
                }
            }
        }
        return None;
    }
    let (CompiledTrigger::Slot { id, slot, once, .. }, _) = facts.trig_idx.get(&s.owner)? else {
        return None;
    };
    if *once && facts.fired_once_owners.contains(&s.owner) {
        return Some(format!(
            "trigger {id} already fired in the replayed log — it never fires again"
        ));
    }
    if let Some(m) = facts.by {
        let minute = facts.initial.now + facts.first_cross(*slot);
        if minute > m {
            return Some(format!(
                "trigger {id}'s first crossing at minute {minute} lies beyond the by {m} window"
            ));
        }
    }
    None
}

/// The world-level notes a report targeting `target` carries (the note
/// convergence rule, §13.1 revision): a note that NAMES a card — a token of
/// the text equal to a known card name — rides only on THAT name's report;
/// a note naming no card stays global. Tokens are maximal
/// `[A-Za-z0-9_]` runs, so an `A_B`-form name only matches itself whole
/// (underscores are name characters; every other character — quote, comma,
/// parenthesis, em dash — is a boundary). Known card names are the
/// post-fixpoint universe's names (initial ∪ prototypes ∪ deck cards ∪
/// spawn mints); the derive sentinel never tokenizes.
fn world_notes_for(facts: &WorldFacts, target: &str) -> Vec<String> {
    facts.notes.iter().filter(|n| note_rides_on(n, facts, target)).cloned().collect()
}

/// Does `note` ride on a report targeting `target`? True when the note
/// names `target`, or names no card at all (global); false when it names
/// only other cards.
fn note_rides_on(note: &str, facts: &WorldFacts, target: &str) -> bool {
    let mut names_other = false;
    for tok in note.split(|c: char| !(c.is_ascii_alphanumeric() || c == '_')) {
        if !facts.universe.iter().any(|e| e.name == tok) {
            continue;
        }
        if tok == target {
            return true;
        }
        names_other = true;
    }
    !names_other
}

/// The (b) usability lines: every committable verb slot the descriptor fits.
fn usable_lines(facts: &WorldFacts, descriptor: &Entity) -> Vec<String> {
    let mut usable_via: Vec<String> = Vec::new();
    for (vi, (_name, verb)) in facts.content.verbs.iter().enumerate() {
        if !facts.committable.contains_key(&vi) {
            continue;
        }
        for slot in &verb.slots {
            let fits = slot.query.as_ref().is_none_or(|q| match_query(descriptor, q));
            if fits {
                usable_via.push(format!(
                    "{}.{}{}",
                    verb.name,
                    slot.role,
                    if slot.query.is_some() {
                        " (query matches)"
                    } else {
                        " (no query — binds anything)"
                    }
                ));
            }
        }
    }
    usable_via
}

/// The live sites that can still serve `target`'s bare name — draws must be
/// unblinded and their deck must carry the name, other sites mint it
/// directly. The shared serve predicate of the candidate scans.
fn live_sites_serving<'a, 'c>(
    facts: &'a WorldFacts<'c>,
    target: &'a str,
) -> impl Iterator<Item = &'a RawSite<'c>> {
    facts.census.live.iter().filter_map(move |&i| {
        let s = &facts.census.sites[i];
        let serves = if s.kind == SiteKind::Draw {
            !draw_blinds_bare(facts.content, s, target) && deck_serves(draw_deck(s), target)
        } else {
            s.name == target
        };
        serves.then_some(s)
    })
}

/// The module-level entry (a fresh world per call — a fresh [`Analyzer`] is
/// observably identical to reuse; the CLI should hold one Analyzer across
/// cards instead).
pub fn analyze_card(
    content: &CompiledContent,
    initial: &State,
    dict: &Dictionary,
    target: &str,
    opts: CardOptions,
) -> Result<CardReport> {
    Analyzer::new(content, initial, dict).analyze_card(target, opts)
}
