//! The committability/provenance fixpoint: `slot_support` + the loop, and
//! `site_proven` with its helpers.
//!
//! The loop is a NAIVE round-robin — every round
//! re-tests all unproven verbs (content.verbs order) and then all unproven
//! sites (live order) until a full pass changes nothing. Worklist/event-driven
//! rewrites are FORBIDDEN: `count(X) <= k` gates make `path_open` anti-monotone,
//! so proven-set membership depends on the test schedule, and the
//! proven-once-never-removed semantics couple the schedule to the output.

use indexmap::{IndexMap, IndexSet};

use crate::content::content::{CompiledSlot, CompiledTrigger};
use crate::core::predicate::{Pred, ValExpr};
use crate::core::query::{match_query, QueryTerm};
use crate::core::types::Entity;

use super::census::{
    draw_blinds_bare, draw_deck, mint_stacks, object_always, RawSite, SetSite, SiteKind,
    DERIVED_NAME, MANY,
};
use super::census::pred_static;
use super::world::{
    eval_static, path_open, CardFact, CardWorld, SlotFiller, SlotProof, WorldFacts,
};

// ---------------------------------------------------------------------------
// gate-name collectors
// ---------------------------------------------------------------------------

/// Name terms mentioned by a value expression (the
/// bootstrap-gate check: `count(name:X) >= k` gates on X's aliveness exactly
/// like exists(X); polarity is ignored on purpose — the check only DEMOTES).
fn expr_exists_names(v: &ValExpr, acc: &mut IndexSet<String>) {
    if let ValExpr::CountQuery { query } = v {
        for t in query {
            if let QueryTerm::Name { name, .. } = t {
                acc.insert(name.clone());
            }
        }
    }
}

/// Name terms mentioned anywhere in a predicate.
pub(crate) fn pred_exists_names(p: &Pred, acc: &mut IndexSet<String>) {
    match p {
        Pred::And(a, b) | Pred::Or(a, b) => {
            pred_exists_names(a, acc);
            pred_exists_names(b, acc);
        }
        Pred::Not(a) => pred_exists_names(a, acc),
        Pred::Cmp { l, r, .. } => {
            expr_exists_names(l, acc);
            expr_exists_names(r, acc);
        }
        Pred::Exists { name } => {
            acc.insert(name.clone());
        }
        Pred::ExistsQuery { query } => {
            for t in query {
                if let QueryTerm::Name { name, .. } = t {
                    acc.insert(name.clone());
                }
            }
        }
        _ => {}
    }
}

// ---------------------------------------------------------------------------
// slot_support
// ---------------------------------------------------------------------------

/// A candidate instance row (a `how` provenance string plus a
/// `name#copy` key).
#[derive(Debug, Clone)]
struct CandInst {
    how: String,
    key: String,
}

impl<'c> WorldFacts<'c> {
    /// The structured committability proof for one
    /// verb. Roles take DISTINCT instances (name#copy), so the same instance
    /// cannot fill two roles and a min:2 slot needs two copies; the first-fit
    /// assign walks the candidate order (avail push order).
    pub(crate) fn slot_support(&mut self, verb_idx: usize) -> Option<Vec<SlotProof>> {
        // `slot_by_role` (last def per role wins at commit);
        // modeling shadowed duplicates as extra requirements killed verbs the
        // engine commits fine (fuzz seed 262)
        let Some((_name, v)) = self.content.verbs.get_index(verb_idx) else {
            unreachable!("verb index {verb_idx} came from content.verbs enumeration");
        };
        let slots: Vec<CompiledSlot> = v
            .slot_by_role
            .values()
            .filter(|s| s.min > 0)
            .cloned()
            .collect();
        if slots.is_empty() {
            // an empty proof is TRUTHY: the verb
            // commits with no slot requirements
            return Some(Vec::new());
        }
        // the per-slot candidate cap
        let cap: i64 = 4;
        let cand: Vec<Vec<CandInst>> = slots
            .iter()
            .map(|slot| {
                let mut list: Vec<CandInst> = Vec::new();
                for facts in self.avail.values() {
                    for f in facts {
                        let fits = slot
                            .query
                            .as_ref()
                            .is_none_or(|q| match_query(&f.entity, q));
                        if !fits {
                            continue;
                        }
                        let copies = if f.entity.stackable {
                            std::cmp::min(if f.count == MANY { cap } else { f.count }, cap)
                        } else {
                            1
                        };
                        for c in 0..copies {
                            list.push(CandInst {
                                how: f.how.clone(),
                                key: format!("{}#{}", f.entity.name, c),
                            });
                        }
                    }
                }
                list
            })
            .collect();
        let mut used: IndexSet<String> = IndexSet::new();
        let mut chosen: Vec<Vec<CandInst>> = slots.iter().map(|_| Vec::new()).collect();
        if !assign(0, 0, &slots, &cand, &mut used, &mut chosen) {
            return None;
        }
        let proofs: Vec<SlotProof> = slots
            .iter()
            .enumerate()
            .map(|(i, slot)| SlotProof {
                role: slot.role.clone(),
                min: slot.min,
                query: slot.query.clone(),
                fillers: chosen[i]
                    .iter()
                    .map(|inst| SlotFiller {
                        // the name head of the
                        // `name#copy` key (splitting always yields a first
                        // piece, even an empty one)
                        name: inst.key.split('#').next().unwrap_or_default().to_string(),
                        how: inst.how.clone(),
                    })
                    .collect(),
            })
            .collect();
        // the filler NAMES this proof relied on
        // (the gate-source-kills-filler rule, seed 12460)
        let mut names: IndexSet<String> = IndexSet::new();
        for pf in &proofs {
            for f in &pf.fillers {
                names.insert(f.name.clone());
            }
        }
        self.fillers.insert(verb_idx, names);
        Some(proofs)
    }

    /// Proven provenance: which PROVEN sites mint each
    /// name (bare names only — derive-mints behind object-mandatory verbs
    /// carry `<PROTO>_<OBJECT>` names).
    pub(crate) fn record_mint_source(&mut self, si: usize) {
        let content = self.content;
        let site = self.census.sites[si].clone();
        let names: Vec<String> = if site.kind == SiteKind::Draw {
            let deck = draw_deck(&site);
            let mut v: Vec<String> = deck.order.iter().map(|c| c.name.clone()).collect();
            if let Some(dc) = &deck.defaultcard {
                v.push(dc.name.clone());
            }
            v
        } else {
            vec![site.name.clone()]
        };
        for n in names {
            let blind = if site.kind == SiteKind::Draw {
                draw_blinds_bare(content, &site, &n)
            } else {
                site.derives && object_always(content, site.verb)
            };
            if blind {
                continue;
            }
            self.mint_sources.entry(n).or_default().push(si);
        }
    }

    /// Register the proven site's product into the
    /// available world (capacity-aware for draws, blinded for derive-mints).
    pub(crate) fn add_product(&mut self, si: usize) {
        let content = self.content;
        let site = self.census.sites[si].clone();
        if site.kind == SiteKind::Draw {
            let deck = draw_deck(&site);
            // capacity-aware: only the first `site.cap`
            // order positions are ever served (seed 150846 phantom entries)
            let served_len = std::cmp::min(site.cap as usize, deck.order.len());
            let mut counts: IndexMap<String, i64> = IndexMap::new();
            for c in &deck.order[..served_len] {
                *counts.entry(c.name.clone()).or_insert(0) += 1;
            }
            for (name, n) in &counts {
                // the FIRST order entry with the
                // name; its index is the position (the find and the
                // position scan agree)
                let Some(card) = deck.order.iter().find(|c| c.name == *name) else {
                    unreachable!("counts keys come from deck.order");
                };
                let Some(pos) = deck.order.iter().position(|c| c.name == *name) else {
                    unreachable!("counts keys come from deck.order");
                };
                let proto = content.prototypes.get(name);
                let infinite = deck.reset
                    || deck
                        .defaultcard
                        .as_ref()
                        .is_some_and(|dc| dc.name == *name);
                // a derive-card drawn behind an
                // object-mandatory verb mints under the DERIVED name (90936)
                let blind = draw_blinds_bare(content, &site, name);
                let key = if blind { DERIVED_NAME } else { name.as_str() };
                let entity = self.synth(key, &card.kind, proto, None);
                let how = format!(
                    "{name} ← deck {} order[{pos}]{}",
                    deck.id,
                    if infinite {
                        if deck.reset {
                            " (reset — periodic)"
                        } else {
                            " (defaultcard)"
                        }
                    } else {
                        ""
                    }
                );
                self.push(
                    key.to_string(),
                    CardFact {
                        entity,
                        count: if infinite { MANY } else { *n },
                        how,
                    },
                );
            }
            if let Some(dc) = &deck.defaultcard {
                if site.cap > deck.order.len() as i64 && !counts.contains_key(&dc.name) {
                    let proto = content.prototypes.get(&dc.name);
                    let blind = draw_blinds_bare(content, &site, &dc.name);
                    let key = if blind { DERIVED_NAME } else { dc.name.as_str() };
                    let entity = self.synth(key, &dc.kind, proto, None);
                    let how = format!("{} ← deck {} defaultcard", dc.name, deck.id);
                    self.push(
                        key.to_string(),
                        CardFact {
                            entity,
                            count: MANY,
                            how,
                        },
                    );
                }
            }
            return;
        }
        let proto = content.prototypes.get(&site.name);
        let kind = site
            .kind_override
            .clone()
            .or_else(|| proto.map(|p| p.kind.clone()))
            .unwrap_or_else(|| "item".to_string());
        // a derive-mint behind an object-mandatory
        // verb never carries the bare name — register it blinded (90936)
        let blind = site.derives && object_always(content, site.verb);
        let key = if blind { DERIVED_NAME } else { site.name.as_str() };
        // KIND-level stackable declarations stack the
        // mint exactly like name-level ones (§9.38, seed 81401)
        let count = if mint_stacks(content, &site.name, site.kind_override.as_deref()) {
            MANY
        } else {
            1
        };
        let entity = self.synth(key, &kind, proto, None);
        let how = format!(
            "{} {} by {}{}",
            if site.kind == SiteKind::Replace {
                "REPLACE"
            } else {
                "SPAWN"
            },
            site.name,
            site.where_,
            site.kind_override
                .as_ref()
                .map(|ko| format!(" (kind overridden to {ko})"))
                .unwrap_or_default(),
        );
        self.push(
            key.to_string(),
            CardFact {
                entity,
                count,
                how,
            },
        );
    }

    /// Whether a site proves this round. Returns the site's
    /// support lines, or None when the site
    /// stays unproven this round.
    pub(crate) fn site_proven(&mut self, si: usize) -> Option<Vec<String>> {
        // the flat snapshot first (it needs &mut), then BORROW the site —
        // a per-call RawSite clone here ran once per unproven site per
        // round and was the fixpoint's second hot spot. The borrow splits
        // cleanly: `site` reads self.census while every other access is a
        // disjoint field or an immutable whole-self reborrow.
        let world = self.cards_of();
        let site = &self.census.sites[si];
        if site.fallback {
            return None; // ELSE runs only when earlier arms are false — unprovable
        }
        if site.killed_in_list {
            return None; // the mint is destroyed later in the same list
        }
        if site.dup_risk {
            return None; // a duplicate non-stackable mint refuses the draft
        }
        if self_gated(self, &site) {
            return None; // gated on (or filled by) a card the list itself mints
        }
        // throwing ops demote the list, EXCEPT a
        // REPLACE whose own name target is provably available and whose death
        // is not scheduled anywhere else (seed 5070)
        let killers: IndexSet<String> = match &site.replace_target {
            Some(t) => self.census.destroyed_by.get(t).cloned().unwrap_or_default(),
            None => IndexSet::new(),
        };
        let other_killers = killers.iter().filter(|o| **o != site.owner).count();
        let self_safe = site.kind == SiteKind::Replace
            && site.replace_target.is_some()
            && self
                .avail
                .contains_key(site.replace_target.as_deref().unwrap_or(""))
            && other_killers == 0
            && !site.target_killed_in_list;
        if site.throw_ops - i64::from(self_safe) > 0 {
            return None;
        }
        if site.shadow_risk {
            return None; // first-match: an earlier recipe may shadow
        }
        match path_open(&site.path, &world) {
            Ok(true) => {}
            Ok(false) => return None,
            // the internal throws sit behind pred_static
            // guards and are unreachable here; debug-trap if a bug
            // ever reaches one
            Err(e) => {
                debug_assert!(false, "internal: {e}");
                return None;
            }
        }
        // a static gate on exists(X) is only as
        // durable as X's aliveness (seed 6028)
        let mut gated_names: IndexSet<String> = IndexSet::new();
        for p in site.path.iter().chain(site.when.iter()) {
            if pred_static(p) {
                pred_exists_names(p, &mut gated_names);
            }
        }
        if gated_names
            .iter()
            .any(|n| self.census.destroyed_by.contains_key(n))
        {
            return None;
        }
        // gate-timing (seed 40449): a SLOT trigger's
        // static gate names a card whose only proven producers fire at (or
        // after) the trigger's own first crossing
        if site.verb.is_none() {
            if let Some((CompiledTrigger::Slot { slot: owner_slot, .. }, owner_decl)) =
                self.trig_idx.get(&site.owner)
            {
                let my_first_cross = self.first_cross(*owner_slot);
                let my_decl = owner_decl;
                let dead_at_my_firing = |src: &RawSite| -> bool {
                    if src.verb.is_some() {
                        return false; // recipe commit — player-timed
                    }
                    match self.trig_idx.get(&src.owner) {
                        None => true,
                        Some((CompiledTrigger::Expire { .. }, _)) => false, // arrangeable
                        Some((CompiledTrigger::Slot { slot: s2, .. }, decl2)) => {
                            if *s2 == *owner_slot {
                                decl2 > my_decl // same slot, later declaration
                            } else {
                                self.first_cross(*s2) > my_first_cross // not yet alive
                            }
                        }
                    }
                };
                let gate_unsatisfiable_at_firing = gated_names.iter().any(|n| {
                    let sources = self.mint_sources.get(n);
                    !self.init_names.contains_key(n)
                        && !site.list_gate_mints.contains(n)
                        && sources.is_some_and(|v| !v.is_empty())
                        && sources.is_some_and(|v| {
                            v.iter()
                                .all(|&src| dead_at_my_firing(&self.census.sites[src]))
                        })
                });
                if gate_unsatisfiable_at_firing {
                    return None;
                }
            }
        }
        // gate-source kills a slot filler (12460)
        if let Some(vi) = site.verb {
            if let Some(f) = self.fillers.get(&vi) {
                if !f.is_empty() {
                    let kills_filler = |src: &RawSite| -> bool {
                        f.iter().any(|y| {
                            self.census
                                .destroyed_by
                                .get(y)
                                .is_some_and(|set| set.contains(&src.owner))
                        })
                    };
                    let filler_doomed = gated_names.iter().any(|n| {
                        let sources = self.mint_sources.get(n);
                        !self.init_names.contains_key(n)
                            && sources.is_some_and(|v| !v.is_empty())
                            && sources.is_some_and(|v| {
                                v.iter().all(|&src| kills_filler(&self.census.sites[src]))
                            })
                    });
                    if filler_doomed {
                        return None;
                    }
                }
            }
        }
        // cross-list trigger-vs-recipe mint collision
        // (81915): every window completes at begin+duration ≥ B, so the
        // boundary at B has already fired the trigger or rolled the pass back
        let verb_duration: Option<i64> = site.verb.map(|vi| match self.content.verbs.get_index(vi) {
            Some((_, v)) => v.duration,
            None => unreachable!("verb index {vi} came from content.verbs enumeration"),
        });
        if let Some(dur) = verb_duration {
            for (owner2, mints2) in &self.trigger_mint_names {
                if mints2.is_empty() {
                    continue;
                }
                let Some((CompiledTrigger::Slot { slot: s2, .. }, _)) = self.trig_idx.get(owner2)
                else {
                    continue;
                };
                if dur < self.first_cross(*s2) {
                    continue;
                }
                for n in mints2 {
                    if site.list_gate_mints.contains(n) {
                        return None;
                    }
                }
            }
        }
        // frozen-completion rule (seed 296):
        // provable only for duration-0 recipes
        if self.frozen && site.verb.is_some() && verb_duration.unwrap_or(0) > 0 {
            return None;
        }
        // recipe-owned site: the `when` must be
        // static-true and the verb committable
        if let Some(vi) = site.verb {
            if let Some(when) = &site.when {
                let holds = pred_static(when)
                    && match eval_static(when, &world) {
                        Ok(b) => b,
                        Err(e) => {
                            debug_assert!(false, "internal: {e}");
                            false
                        }
                    };
                if !holds {
                    return None;
                }
            }
            let proofs = self.committable.get(&vi)?;
            let verb_name = match self.content.verbs.get_index(vi) {
                Some((name, _)) => name.clone(),
                None => unreachable!("verb index {vi} came from content.verbs enumeration"),
            };
            return Some(slot_proof_lines(&verb_name, proofs));
        }
        // trigger owner
        let (trig, _) = self.trig_idx.get(&site.owner)?;
        if let CompiledTrigger::Slot { id, slot, once, .. } = trig {
            let once_note = if *once {
                " (once: true — still fires its first crossing)"
            } else {
                " (every crossing)"
            };
            return Some(vec![format!(
                "trigger {id} fires on slot {slot}{once_note} — the clock is unbounded forward"
            )]);
        }
        let CompiledTrigger::Expire { id, on_expire_of, .. } = trig else {
            unreachable!("a compiled trigger is either Slot or Expire");
        };
        // expiry trigger: the named card must be
        // obtainable AND able to expire. The arming-witness recursion is the
        // seed-100625 rule; `visiting` cuts arming cycles (the seed-41247
        // self-arming check is subsumed), and the memo caches only cycle-free
        // (top-level) verdicts.
        let desc: Option<Entity> = self
            .init_names
            .get(on_expire_of)
            .map(|e| (*e).clone())
            .or_else(|| {
                self.avail
                    .get(on_expire_of)
                    .and_then(|list| list.first())
                    .map(|f| f.entity.clone())
            });
        let mut expiry_memo: IndexMap<String, bool> = IndexMap::new();
        let expirable = desc.is_some()
            && expirable_card(self, &world, &mut expiry_memo, on_expire_of, &IndexSet::new());
        if desc.is_none() || !expirable {
            return None;
        }
        self.notes.push(format!(
            "expiry trigger {id} reaches its body only while a living '{on_expire_of}' can expire"
        ));
        let available = if self.init_names.contains_key(on_expire_of) {
            "initial"
        } else {
            "produced"
        };
        let writable_suffix = if self.init_names.contains_key(on_expire_of) {
            ""
        } else {
            " or minted"
        };
        Some(vec![format!(
            "trigger {id} fires on the expiry of {on_expire_of} (available: {available}; expires writable{writable_suffix})"
        )])
    }
}

/// The content.verbs insertion index of a verb name (identity-key maps are
/// index-keyed; verb names are unique per content).
fn verb_idx_of(wf: &WorldFacts, verb_name: &str) -> usize {
    match wf.content.verbs.get_index_of(verb_name) {
        Some(vi) => vi,
        None => unreachable!("verb '{verb_name}' came from content.verbs enumeration"),
    }
}

/// The display lines of a verb's structured
/// committability proof (byte-identical to the pre-structure format).
pub(crate) fn slot_proof_lines(verb_name: &str, proofs: &[SlotProof]) -> Vec<String> {
    proofs
        .iter()
        .map(|pf| {
            let mut whos: IndexSet<String> = IndexSet::new();
            for f in &pf.fillers {
                whos.insert(f.how.clone());
            }
            let whos: Vec<String> = whos.into_iter().take(3).collect();
            format!(
                "{verb_name} committable: {} ×{} ← {}",
                pf.role,
                pf.min,
                whos.join(", ")
            )
        })
        .collect()
}

/// `assign(i, from)`: fill slot i with exactly
/// slot.min DISTINCT instances (name#copy keys — no reuse within or across
/// roles); first-fit over the fixed candidate order.
fn assign(
    i: usize,
    from: usize,
    slots: &[CompiledSlot],
    cand: &[Vec<CandInst>],
    used: &mut IndexSet<String>,
    chosen: &mut Vec<Vec<CandInst>>,
) -> bool {
    if i == slots.len() {
        return true;
    }
    if chosen[i].len() == slots[i].min as usize {
        return assign(i + 1, 0, slots, cand, used, chosen);
    }
    for k in from..cand[i].len() {
        let inst = cand[i][k].clone();
        if used.contains(&inst.key) {
            continue;
        }
        used.insert(inst.key.clone());
        chosen[i].push(inst);
        if assign(i, k + 1, slots, cand, used, chosen) {
            return true;
        }
        let popped = chosen[i].pop();
        if let Some(p) = popped {
            used.swap_remove(&p.key);
        }
    }
    false
}

// ---------------------------------------------------------------------------
// self_gated
// ---------------------------------------------------------------------------

/// Does a static-true condition gate this list on a
/// card THE LIST ITSELF mints (any op)? Post-bootstrap the gate is true and
/// the self-mint collides — and a completion refusal freezes the dice counter,
/// so the escape roll never comes (seeds 523, 5631).
pub(crate) fn self_gated(wf: &WorldFacts, site: &RawSite) -> bool {
    let mut gates: IndexSet<String> = IndexSet::new();
    for p in site.path.iter().chain(site.when.iter()) {
        if pred_static(p) {
            pred_exists_names(p, &mut gates);
        }
    }
    let mints = &site.list_gate_mints;
    if gates.iter().any(|g| mints.contains(g)) {
        return true;
    }
    // own-list STACKABLE mint behind a gate: the gate
    // reads FALSE at a once-trigger's single firing; recurring and recipe
    // lists bootstrap on their second application — only once-owners are doomed
    if site.verb.is_none() {
        if let Some((CompiledTrigger::Slot { once, .. }, _)) = wf.trig_idx.get(&site.owner) {
            if *once && gates.iter().any(|g| site.list_stack_mints.contains(g)) {
                return true;
            }
        }
    }
    // mints-driven, not slots×world-driven: mints is
    // the (small) set of names this list itself mints
    if let Some(vi) = site.verb {
        let Some((_vn, verb)) = wf.content.verbs.get_index(vi) else {
            unreachable!("verb index {vi} came from content.verbs enumeration");
        };
        for mint_name in mints.iter() {
            let Some(facts) = wf.avail.get(mint_name) else {
                continue;
            };
            for f in facts {
                for slot in &verb.slots {
                    if slot.min == 0 {
                        continue;
                    }
                    let fits = slot
                        .query
                        .as_ref()
                        .is_none_or(|q| match_query(&f.entity, q));
                    if fits {
                        return true;
                    }
                }
            }
        }
    }
    false
}

// ---------------------------------------------------------------------------
// the expiry arming recursion
// ---------------------------------------------------------------------------

/// The arming SET's owner is an expiry trigger on the
/// very card being armed (circular arming).
fn self_arm(wf: &WorldFacts, owner: &str, card_name: &str) -> bool {
    match wf.trig_idx.get(owner) {
        Some((CompiledTrigger::Expire { on_expire_of, .. }, _)) => on_expire_of == card_name,
        _ => false,
    }
}

/// The owner's arming chain: verbs are checked above;
/// slot triggers fire unconditionally; an expiry trigger needs its own card
/// expirable.
fn owner_armable(
    wf: &WorldFacts,
    world: &CardWorld,
    memo: &mut IndexMap<String, bool>,
    owner: &str,
    visiting: &IndexSet<String>,
) -> bool {
    match wf.trig_idx.get(owner) {
        None => true,
        Some((CompiledTrigger::Slot { .. }, _)) => true,
        Some((CompiledTrigger::Expire { on_expire_of, .. }, _)) => {
            expirable_card(wf, world, memo, on_expire_of, visiting)
        }
    }
}

/// The arming SET's role target must bind a card
/// whose descriptor fits the role's slot query.
fn role_slot_fits(wf: &WorldFacts, s: &SetSite, role: &str, d: &Entity) -> bool {
    let v = s
        .verb_name
        .as_deref()
        .and_then(|vn| wf.content.verbs.get(vn));
    let slot = v.and_then(|v| v.slot_by_role.get(role));
    match slot {
        Some(slot) => match &slot.query {
            Some(q) => match_query(d, q),
            None => true,
        },
        None => false,
    }
}

/// Can `card_name` ever be ARMED to expire?
fn expirable_card(
    wf: &WorldFacts,
    world: &CardWorld,
    memo: &mut IndexMap<String, bool>,
    card_name: &str,
    visiting: &IndexSet<String>,
) -> bool {
    if let Some(&hit) = memo.get(card_name) {
        return hit;
    }
    if visiting.contains(card_name) {
        return false; // arming cycle — cannot bootstrap
    }
    let next: IndexSet<String> = visiting
        .iter()
        .cloned()
        .chain(std::iter::once(card_name.to_string()))
        .collect();
    let d: Option<Entity> = wf
        .init_names
        .get(card_name)
        .map(|e| (*e).clone())
        .or_else(|| {
            wf.avail
                .get(card_name)
                .and_then(|list| list.first())
                .map(|f| f.entity.clone())
        });
    let mut result = d
        .as_ref()
        .is_some_and(|de| de.attrs.contains_key("expires"));
    if !result {
        result = wf.census.set_sites.iter().any(|s| {
            (s.key == "expires" || s.key == "expiresAt")
                && !s.refused
                && !s.shadowed
                && !wf.dirty_owners.contains(&s.owner)
                && !self_arm(wf, &s.owner, card_name)
                // the arming SET must actually APPLY:
                // a runtime-gated path can be durably false, a frozen world
                // hides owner firings past the freeze boundary, and a
                // duration>0 recipe window completes inside the failing step
                && !s.path_non_static
                && !(wf.frozen
                    && (match &s.verb_name {
                        Some(vn) => wf.content.verbs.get(vn).is_some_and(|v| v.duration > 0),
                        None => s
                            .owner_slot
                            .is_some_and(|os| wf.first_cross(os) >= wf.freeze_dist),
                    }))
                && (match &s.verb_name {
                    None => true,
                    Some(vn) => wf.content.verbs.get(vn).is_some_and(|_| {
                        wf.committable.contains_key(&verb_idx_of(wf, vn))
                    }),
                })
                && (match &s.when_pred {
                    None => true,
                    Some(wp) => {
                        pred_static(wp)
                            && match eval_static(wp, world) {
                                Ok(b) => b,
                                Err(e) => {
                                    debug_assert!(false, "internal: {e}");
                                    false
                                }
                            }
                    }
                })
                && (s.target_name.as_deref() == Some(card_name)
                    || match (&s.target_role, &d) {
                        (Some(role), Some(de)) => role_slot_fits(wf, s, role, de),
                        _ => false,
                    })
                // a witness inside ANOTHER expiry
                // trigger only lands if that trigger's own card can expire
                // (seed 100625)
                && owner_armable(wf, world, memo, &s.owner, &next)
        });
    }
    if visiting.is_empty() {
        memo.insert(card_name.to_string(), result);
    }
    result
}

// ---------------------------------------------------------------------------
// the loop (naive round-robin ONLY)
// ---------------------------------------------------------------------------

pub(crate) fn run_fixpoint(wf: &mut WorldFacts) {
    let mut changed = true;
    while changed {
        changed = false;
        // verbs in content.verbs order
        for vi in 0..wf.content.verbs.len() {
            if wf.committable.contains_key(&vi) {
                continue;
            }
            if let Some(sup) = wf.slot_support(vi) {
                wf.committable.insert(vi, sup);
                changed = true;
            }
        }
        // sites in live order; proven-once-never-
        // removed (the anti-monotone count-gate semantics live exactly here)
        let live: Vec<usize> = wf.census.live.clone();
        for si in live {
            if wf.proven.contains_key(&si) {
                continue;
            }
            if let Some(sup) = wf.site_proven(si) {
                wf.proven.insert(si, sup);
                wf.record_mint_source(si);
                wf.add_product(si);
                changed = true;
            }
        }
    }
}
