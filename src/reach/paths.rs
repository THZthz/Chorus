//! Path export and the per-card report. The universe/dead-verbs/
//! deck-cap block finalizes the world facts after the fixpoint; [`analyze_card`]
//! folds the target-specific verdict, chain, path trees and notes on top.

use indexmap::{IndexMap, IndexSet};

use crate::content::content::{CompiledContent, CompiledDeck, CompiledTrigger};
use crate::core::query::{match_query, parse_query, Query};
use crate::core::types::{Aspects, Entity, State};
use crate::error::Result;

use super::census::{
    draw_blinds_bare, draw_deck, object_always, parse_aspects_json, pred_static, site_aspects,
    RawSite, SiteKind, MANY,
};
use super::render::pred_text;
use super::world::{writable_paths, Analyzer, WorldFacts};

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
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PathOp {
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
#[derive(Debug, Clone, PartialEq)]
pub struct PathDeck {
    pub id: String,
    pub position: Option<usize>,
    pub defaultcard: bool,
    pub capacity: Option<i64>,
}

/// One prerequisite card with its own provenance.
#[derive(Debug, Clone, PartialEq)]
pub struct Require {
    pub name: String,
    pub how: String,
    pub paths: Vec<ReachPath>,
}

/// One node of an exported acquisition path.
#[derive(Debug, Clone, PartialEq)]
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
#[derive(Debug, Clone)]
pub struct CardCandidate {
    pub via: String,
    pub reason: String,
}

/// The per-card report.
#[derive(Debug, Clone)]
pub struct CardReport {
    pub target: String,
    pub verdict: CardVerdict,
    pub existence: Existence,
    pub requested_usable: bool,
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

/// Flatten with multiplicity (capped at 32 per fact)
/// so count(<query>) thresholds can read it.
fn flatten(avail: &IndexMap<String, Vec<super::world::CardFact>>) -> Vec<Entity> {
    let mut out: Vec<Entity> = Vec::new();
    for list in avail.values() {
        for f in list {
            out.push(f.entity.clone());
            let n = std::cmp::min(f.count, 32);
            for _ in 1..n {
                out.push(f.entity.clone());
            }
        }
    }
    out
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
        // `slot_by_role`, not verb.slots: a shadowed
        // duplicate def must not doom the verb (fuzz seed 262)
        let Some((_, verb)) = content.verbs.get_index(vi) else {
            unreachable!("verb index {vi} came from content.verbs enumeration");
        };
        let hopeless = verb.slot_by_role.values().any(|slot| {
            slot.min > 0 && !wf.can_ever_match(slot.query.as_ref())
        });
        if hopeless {
            wf.dead_verbs.insert(vi);
        }
    }
    // deck capacity from proven sites (the freeze
    // model: pre-S* trigger draws count 1, at/after-S* count 0; recipe-owned
    // draws stay unbounded)
    let proven: Vec<usize> = wf.proven.keys().copied().collect();
    for si in proven {
        let site = &wf.census.sites[si];
        if site.kind != SiteKind::Draw {
            continue;
        }
        let mut cap = site.cap;
        if wf.frozen {
            if site.verb.is_some() {
                cap = site.cap; // recipe-owned: commits at/short of the freeze are still unbounded
            } else {
                cap = match wf.trig_idx.get(&site.owner) {
                    Some((CompiledTrigger::Slot { slot, .. }, _))
                        if wf.first_cross(*slot) < wf.freeze_dist =>
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
    /// Can a card matching `query` EVER exist? No memo is needed: the
    /// function is pure over fields that are
    /// final once the world facts are built, so memoization cannot change any
    /// result (a value-equal query yields a value-equal answer).
    pub(crate) fn can_ever_match(&self, query: Option<&Query>) -> bool {
        let Some(q) = query else {
            return true;
        };
        if q.is_empty() {
            return true;
        }
        if self.universe.iter().any(|u| match_query(u, q)) {
            return true;
        }
        if self.opaque_spawn_aspects {
            return true;
        }
        let paths = writable_paths(q);
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
                    && served_by_cap(
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
    /// absorb it), so the candidate list here is the old whole-`proven` scan
    /// filtered to the same hits in the same order — the cap check then keeps
    /// the identical survivors. Selection order (first-fit path export) is
    /// untouched.
    pub(crate) fn proven_sources(&self, name: &str) -> Vec<(usize, Vec<String>)> {
        let mut out: Vec<(usize, Vec<String>)> = Vec::new();
        if let Some(candidates) = self.mint_sources.get(name) {
            for &si in candidates {
                let site = &self.census.sites[si];
                if site.kind == SiteKind::Draw
                    && !served_by_cap(
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
                out.push((si, sup));
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
    /// yields exactly the per-branch membership the old clone-per-source
    /// built (the set is membership-only; its iteration order is unobservable).
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
        let mut guards: Vec<String> = site
            .path
            .iter()
            .filter(|p| pred_static(p))
            .map(pred_text)
            .collect();
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

/// The initial-entity provenance line.
fn initial_decl_line(name: &str, e: &Entity) -> String {
    let aspects = if e.aspects.is_empty() {
        String::new()
    } else {
        format!(", aspects {}", crate::engine::engine::aspects_to_json(&e.aspects))
    };
    format!("{name} is declared in the initial entities ({}{aspects})", e.kind)
}

/// Does the deck carry the target at all?
pub(crate) fn deck_serves(deck: &CompiledDeck, target: &str) -> bool {
    deck.order.iter().any(|c| c.name == target)
        || deck
            .defaultcard
            .as_ref()
            .is_some_and(|dc| dc.name == target)
}

/// Does `cap` total draws suffice to serve `target`?
/// order positions need cap > idx; the defaultcard needs the cursor to pass
/// the order's end; reset only makes the sequence periodic (seed 74985).
pub(crate) fn served_by_cap(deck: &CompiledDeck, target: &str, cap: i64) -> bool {
    match deck.order.iter().position(|c| c.name == target) {
        Some(idx) => cap > idx as i64,
        None => {
            if deck.reset {
                return false;
            }
            if !deck
                .defaultcard
                .as_ref()
                .is_some_and(|dc| dc.name == target)
            {
                return false;
            }
            cap > deck.order.len() as i64
        }
    }
}

/// The display chain of one proven site.
pub(crate) fn site_chain(site: &RawSite, target: &str, supports: &[String]) -> Vec<String> {
    let mut lines: Vec<String> = Vec::new();
    if site.kind == SiteKind::Draw {
        let deck = draw_deck(site);
        let pos = deck.order.iter().position(|c| c.name == target);
        let where_ = match pos {
            Some(p) => format!(
                "deck {} serves '{}' at order position {} (the permutation is seed-fixed at load — pure in seed × deck id)",
                deck.id, target, p
            ),
            None => format!(
                "deck {} serves '{}' as defaultcard{}",
                deck.id,
                target,
                if deck.reset { " (reset — periodic)" } else { "" }
            ),
        };
        lines.push(where_);
        if site.cap != MANY {
            lines.push(format!(
                "  finite draw capacity: {} once-trigger draw(s) — only order[0..{}] are ever served",
                site.cap,
                site.cap - 1
            ));
        }
        lines.push(format!("  ← {}", site.where_));
    } else {
        lines.push(format!(
            "{} mints '{}' — {}",
            if site.kind == SiteKind::Replace {
                "REPLACE"
            } else {
                "SPAWN"
            },
            target,
            site.where_
        ));
        if let Some(ko) = &site.kind_override {
            lines.push(format!(
                "  minted kind overridden to '{ko}' (kind: syntax overrides kind, it does not look up by kind)"
            ));
        }
    }
    for s in supports {
        lines.push(format!("  · {s}"));
    }
    lines
}

/// Why a guarded site stays a candidate.
pub(crate) fn site_reason(
    site: &RawSite,
    init_names: &IndexMap<String, &Entity>,
) -> String {
    let mut bits: Vec<String> = Vec::new();
    if site.fallback {
        bits.push("ELSE fallback body".to_string());
    }
    if site.killed_in_list {
        bits.push(
            "the mint is destroyed/Replaced later in the same outcome list — it never survives a step"
                .to_string(),
        );
    }
    if site.dup_risk {
        bits.push(
            "this list mints a name that can collide with a living card (duplicate mint or an initial copy) — the refusal would discard the draft"
                .to_string(),
        );
    }
    if site.throw_ops > 0 {
        bits.push("a throwing op in this list may discard the whole draft at completion".to_string());
    }
    if site.shadow_risk {
        bits.push("an earlier recipe of the same verb may shadow it (first-match)".to_string());
    }
    // QUIRK(Q17): reachable for zero-entity scripts (the language
    // allows a script with no entities:) — this branch must stay
    if site.verb.is_some() && init_names.is_empty() {
        bits.push("verb committability unknown".to_string());
    }
    if !site.path.is_empty() {
        bits.push(format!("{} IF condition(s) on the path", site.path.len()));
    }
    if site.verb.is_some() && site.when.is_some() {
        bits.push("recipe `when` guard".to_string());
    }
    if bits.is_empty() {
        "unknown guard".to_string()
    } else {
        bits.join(", ")
    }
}

// ---------------------------------------------------------------------------
// analyze_card
// ---------------------------------------------------------------------------

impl<'c> Analyzer<'c> {
    /// The per-card report; world facts are shared across
    /// calls. `target` is ignored under `demand`.
    pub fn analyze_card(&self, target: &str, opts: CardOptions) -> Result<CardReport> {
        let facts = self.facts();
        let content = self.content;
        let mut notes: Vec<String> = facts.notes.clone();
        let mut chain: Vec<String> = Vec::new();
        let mut paths: Vec<ReachPath> = Vec::new();
        let mut candidates: Vec<CardCandidate> = Vec::new();
        let mut existence = Existence::None;
        let mut demand_match: Option<Entity> = None;
        let demand_q: Option<Query> = match opts.demand.as_deref() {
            // an EMPTY demand string counts as no demand (the frozen truthiness of
            // `''`); parse_query("") would return an empty query
            // without erroring and must not enter demand mode
            Some(d) if !d.is_empty() => Some(parse_query(d)?),
            _ => None,
        };

        if let Some(dq) = &demand_q {
            // semantics (c): ANY obtainable card
            // satisfying the demand query, matched against each source's ACTUAL
            // product descriptor, never the bare prototype
            for e in facts.init_names.values() {
                if !match_query(e, dq) {
                    continue;
                }
                if existence != Existence::Proven {
                    existence = Existence::Proven;
                    demand_match = Some((*e).clone());
                    chain.push(demand_chain_head(&e.name, &e.kind, &e.aspects, opts.demand.as_deref()));
                    match facts.proven_source_for(&e.name) {
                        Some(lines) => chain.extend(lines),
                        None => unreachable!(
                            "initNames hit ⇒ provenSourceFor returns the initial line"
                        ),
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
                        let mut entries: Vec<&crate::content::content::DeckCard> =
                            deck.order.iter().collect();
                        if let Some(dc) = &deck.defaultcard {
                            entries.push(dc);
                        }
                        entries
                            .iter()
                            .map(|c| {
                                let desc = facts.synth(
                                    &c.name,
                                    &c.kind,
                                    content.prototypes.get(&c.name),
                                    None,
                                );
                                // a draw of a
                                // derive-card behind an object-mandatory verb
                                // never carries the bare name (12320)
                                let blind = draw_blinds_bare(content, site, &c.name);
                                let mut desc = desc;
                                if blind {
                                    desc.name = super::census::DERIVED_NAME.to_string();
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
                            desc.name = super::census::DERIVED_NAME.to_string();
                        }
                        vec![(site.name.clone(), desc, blind)]
                    };
                    for (name, desc, derived) in descs {
                        if !match_query(&desc, dq) {
                            continue;
                        }
                        if site.kind == SiteKind::Draw
                            && !served_by_cap(
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
                            chain.push(demand_chain_head(
                                &shown,
                                &desc.kind,
                                &desc.aspects,
                                opts.demand.as_deref(),
                            ));
                            chain.extend(site_chain(site, &name, sup));
                        }
                        paths.push(facts.path_for(
                            si,
                            &name,
                            sup,
                            0,
                            &mut IndexSet::new(),
                            derived,
                        ));
                    }
                }
            }
            if existence == Existence::None {
                // a static descriptor may match behind
                // guarded sites; a threshold may be pumped by SET or fed by
                // derive; any of these keeps 'none' unprovable
                let matches_somewhere = facts.universe.iter().any(|u| match_query(u, dq));
                let write_paths = writable_paths(dq);
                let pumps: Vec<&String> = write_paths
                    .iter()
                    .filter(|p| facts.set_writes.contains(*p))
                    .collect();
                let derives: Vec<&String> = write_paths
                    .iter()
                    .filter(|p| facts.derive_paths.contains(*p))
                    .collect();
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
        } else if facts.init_names.contains_key(target) {
            // an initial entity is proven outright
            existence = Existence::Proven;
            match facts.proven_source_for(target) {
                Some(lines) => chain.extend(lines),
                None => unreachable!(
                    "initNames hit ⇒ provenSourceFor returns the initial line"
                ),
            }
            paths.push(facts.initial_path(target));
        } else {
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
                        && served_by_cap(
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
                paths.push(facts.path_for(
                    si,
                    target,
                    sup,
                    0,
                    &mut IndexSet::new(),
                    false,
                ));
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
                            // source in a frozen world (81401); unfrozen caps
                            // are exact structural facts
                            && (!facts.proven.contains_key(&i)
                                || (facts.frozen
                                    && !served_by_cap(
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
                let derived_only = facts.census.live.iter().any(|&i| {
                    let s = &facts.census.sites[i];
                    // a dead verb's sites never
                    // produce; they must not negate the derive-drop escape
                    // (seed 91492)
                    s.verb.is_none_or(|v| !facts.dead_verbs.contains(&v))
                        && (if s.kind == SiteKind::Draw {
                            !draw_blinds_bare(content, s, target)
                                && deck_serves(draw_deck(s), target)
                        } else {
                            s.name == target
                        })
                        && !(s.derives && object_always(content, s.verb))
                        && !facts.init_names.contains_key(target)
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
                p.derive
                    .as_ref()
                    .and_then(|d| d.name_from.as_deref())
                    == Some("object")
                    && target.starts_with(&format!("{pname}_"))
            });
            if existence == Existence::None && derived_pattern {
                existence = Existence::Candidate;
                notes.push(format!(
                    "'{target}' matches a derived-mint shape (deriving name from object) — the object half is chosen at play time, so structural absence cannot be proven"
                ));
            }
        }

        // (b) usability
        let mut usable_via: Vec<String> = Vec::new();
        let descriptor: Option<Entity> = match (&demand_match, facts.init_names.get(target)) {
            (Some(d), _) => Some(d.clone()),
            (None, Some(e)) => Some((*e).clone()),
            (None, None) => facts.proto_of(target).map(|p| {
                facts.synth(target, &p.kind, Some(p), None)
            }),
        };
        if existence == Existence::Proven && descriptor.is_none() {
            notes.push(format!(
                "no static descriptor for '{target}' (minted without a declared prototype?) — usability could not be checked"
            ));
        }
        if let Some(descriptor) = &descriptor {
            for (vi, (_name, verb)) in content.verbs.iter().enumerate() {
                if !facts.committable.contains_key(&vi) {
                    continue;
                }
                for slot in &verb.slots {
                    let fits = slot
                        .query
                        .as_ref()
                        .is_none_or(|q| match_query(descriptor, q));
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
        }

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

        // guarded (candidate) sources listed flat
        if demand_q.is_none() && existence == Existence::Candidate && chain.is_empty() {
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
                    let reason = site_reason(s, &facts.init_names);
                    notes.push(format!(
                        "candidate source: {} — guarded ({})",
                        s.where_, reason
                    ));
                    candidates.push(CardCandidate {
                        via: s.where_.clone(),
                        reason,
                    });
                }
            }
        }

        Ok(CardReport {
            target: target.to_string(),
            verdict,
            existence,
            requested_usable: opts.usable,
            chain,
            paths,
            candidates,
            usable_via,
            notes,
        })
    }
}

/// The demand chain head line.
fn demand_chain_head(shown: &str, kind: &str, aspects: &Aspects, demand: Option<&str>) -> String {
    let aspects_part = if aspects.is_empty() {
        String::new()
    } else {
        format!(", aspects {}", crate::engine::engine::aspects_to_json(aspects))
    };
    format!(
        "{shown} ({kind}{aspects_part}) satisfies the demand '{}'",
        demand.unwrap_or("")
    )
}

/// The module-level entry (a fresh world per call — a fresh [`Analyzer`] is
/// observably identical to reuse; the CLI should hold one Analyzer across
/// cards instead).
pub fn analyze_card(
    content: &CompiledContent,
    initial: &State,
    target: &str,
    opts: CardOptions,
) -> Result<CardReport> {
    Analyzer::new(content, initial).analyze_card(target, opts)
}
