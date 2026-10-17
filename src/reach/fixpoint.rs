//! The committability/provenance fixpoint: `slot_support` + the loop, and
//! `site_proven` with its helpers — plus the [`CardBlocker`] ladder, the
//! structured verdict the demotion rules yield as they fire
//! ([`WorldFacts::site_ladder`]; `site_proven` is its `.ok()` face, the
//! `--why` diagnostics its Err face — one pass, one source).
//!
//! The loop is a NAIVE round-robin — every round
//! re-tests all unproven verbs (content.verbs order) and then all unproven
//! sites (live order) until a full pass changes nothing. Worklist/event-driven
//! rewrites are FORBIDDEN: `count(X) <= k` gates make `path_open` anti-monotone,
//! so proven-set membership depends on the test schedule, and the
//! proven-once-never-removed semantics couple the schedule to the output.

use indexmap::{IndexMap, IndexSet};

use crate::content::walk_outcomes;
use crate::content::{CompiledSlot, CompiledTrigger};
use crate::core::predicate::{Pred, ValExpr};
use crate::core::query::{match_query, QueryTerm};
use crate::core::r#ref::Ref;
use crate::core::roles::Role;
use crate::core::sentence::{Arg, IntoTarget};
use crate::core::types::Entity;

use super::census::{
    draw_blinds_bare, draw_deck, mint_stacks, object_always, op_throws, site_mints, trigger_owner,
    RawSite, SetSite, SiteKind, DERIVED_NAME, MANY,
};
use super::render::pred_text;
use super::world::{
    eval_static_ok, path_open_first_failure, pred_static, CardFact, CardWorld, SlotFiller,
    SlotProof, WorldFacts,
};

// ---------------------------------------------------------------------------
// gate-name collectors
// ---------------------------------------------------------------------------

/// The per-slot candidate cap: a stackable fact contributes at most this
/// many `name#copy` instances to a slot's candidate list.
const SLOT_CANDIDATE_CAP: i64 = 4;

/// Name terms mentioned by a value expression (the
/// bootstrap-gate check: `count(name:X) >= k` gates on X's aliveness exactly
/// like exists(X); polarity is ignored on purpose — the check only DEMOTES).
fn expr_exists_names(v: &ValExpr, acc: &mut IndexSet<String>) {
    if let ValExpr::CountQuery { query } = v {
        // names of ANY group (§4.5); polarity is ignored on purpose
        for t in query.iter().flatten() {
            if let QueryTerm::Name { name, .. } = t {
                acc.insert(name.clone());
            }
        }
    }
}

/// Name terms mentioned anywhere in a predicate.
fn pred_exists_names(p: &Pred, acc: &mut IndexSet<String>) {
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
            // names of ANY group (§4.5); polarity is ignored on purpose
            for t in query.iter().flatten() {
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
        let slots: Vec<CompiledSlot> =
            v.slot_by_role.values().filter(|s| s.min > 0).cloned().collect();
        if slots.is_empty() {
            // an empty proof is TRUTHY: the verb
            // commits with no slot requirements
            return Some(Vec::new());
        }
        // the per-slot candidate cap
        let cap: i64 = SLOT_CANDIDATE_CAP;
        let cand: Vec<Vec<CandInst>> = slots
            .iter()
            .map(|slot| {
                let mut list: Vec<CandInst> = Vec::new();
                for facts in self.avail.values() {
                    for f in facts {
                        let fits = slot.query.as_ref().is_none_or(|q| match_query(&f.entity, q));
                        if !fits {
                            continue;
                        }
                        let copies =
                            if f.entity.stackable { std::cmp::min(f.count, cap) } else { 1 };
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
        let names: Vec<String> = site_mints(&site).iter().map(|(n, _)| (*n).to_string()).collect();
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
                let infinite =
                    deck.reset || deck.defaultcard.as_ref().is_some_and(|dc| dc.name == *name);
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
                    CardFact { entity, count: if infinite { MANY } else { *n }, how },
                );
            }
            if let Some(dc) = &deck.defaultcard {
                if site.cap > deck.order.len() as i64 && !counts.contains_key(&dc.name) {
                    let proto = content.prototypes.get(&dc.name);
                    let blind = draw_blinds_bare(content, &site, &dc.name);
                    let key = if blind { DERIVED_NAME } else { dc.name.as_str() };
                    let entity = self.synth(key, &dc.kind, proto, None);
                    let how = format!("{} ← deck {} defaultcard", dc.name, deck.id);
                    self.push(key.to_string(), CardFact { entity, count: MANY, how });
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
        // mint exactly like name-level ones (§4.2, seed 81401)
        let count =
            if mint_stacks(content, &site.name, site.kind_override.as_deref()) { MANY } else { 1 };
        let entity = self.synth(key, &kind, proto, None);
        let how = format!(
            "{} {} by {}{}",
            if site.kind == SiteKind::Replace { "REPLACE" } else { "SPAWN" },
            site.name,
            site.where_,
            site.kind_override
                .as_ref()
                .map(|ko| format!(" (kind overridden to {ko})"))
                .unwrap_or_default(),
        );
        self.push(key.to_string(), CardFact { entity, count, how });
    }

    /// Whether a site proves this round: [`Self::site_ladder`]'s verdict,
    /// with the expiry note (the ladder's only write) recorded here. Returns
    /// the site's support lines, or None when
    /// the site stays unproven this round. The demotion rules run in exactly
    /// this order (the ladder IS the semantics).
    pub(crate) fn site_proven(&mut self, si: usize) -> Option<Vec<String>> {
        let world = self.cards_of();
        let (res, note) = self.site_ladder(si, &world);
        // the note is a disjoint-field write here (expiry_support only reads);
        // site_proven runs at most once per site (proven-once-never-removed),
        // so the note cannot duplicate
        if let Some(n) = note {
            self.notes.push(n);
        }
        res.ok()
    }

    /// THE ladder — one `&self` pass over the demotion rules in order, each
    /// failing guard YIELDING its [`CardBlocker`] (judgment and reason are
    /// the same computation; there is no post-hoc re-derivation). `world` is
    /// the caller's snapshot (`site_proven` passes the cached `cards_of`
    /// build; the `--why` diagnostics pass the read-only rebuild). On the
    /// proven path the second element carries the expiry arming note for the
    /// caller to record; on a blocker it is always None.
    pub(crate) fn site_ladder(&self, si: usize, world: &CardWorld) -> Ladder {
        let site = &self.census.sites[si];
        if site.fallback {
            return (Err(CardBlocker::Fallback), None); // ELSE runs only when earlier arms are false — unprovable
        }
        if site.killed_in_list {
            return (Err(CardBlocker::KilledInList), None); // the mint is destroyed later in the same list
        }
        if site.dup_risk {
            return (Err(CardBlocker::DupRisk), None); // a duplicate non-stackable mint refuses the draft
        }
        if self_gated(self, site) {
            return (Err(CardBlocker::SelfGated), None); // gated on (or filled by) a card the list itself mints
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
            && self.avail.contains_key(site.replace_target.as_deref().unwrap_or(""))
            && other_killers == 0
            && !site.target_killed_in_list;
        if site.throw_ops - i64::from(self_safe) > 0 {
            return (Err(CardBlocker::ThrowOps), None);
        }
        if site.shadow_risk {
            return (Err(CardBlocker::ShadowRisk), None); // first-match: an earlier recipe may shadow
        }
        // the internal throws sit behind pred_static
        // guards and are unreachable here; debug-trap if a bug
        // ever reaches one (eval_static_ok's debug_assert)
        if let Some(cond) = super::world::path_open_first_failure(&site.path, world) {
            return (Err(CardBlocker::PathClosed { cond: pred_text(cond) }), None);
        }
        let gated_names = static_gate_names(site);
        // a static gate on exists(X) is only as
        // durable as X's aliveness (seed 6028)
        if let Some(n) = gated_names.iter().find(|n| self.census.destroyed_by.contains_key(*n)) {
            return (Err(CardBlocker::GateNameDestroyed { name: n.clone() }), None);
        }
        // gate-timing (seed 40449)
        if let Some(n) = gate_timing_unsatisfiable(self, site, &gated_names) {
            return (Err(CardBlocker::GateTimingUnsatisfiable { name: n }), None);
        }
        // gate-source kills a slot filler (12460)
        if let Some(n) = filler_doomed(self, site, &gated_names) {
            return (Err(CardBlocker::FillerDoomed { name: n }), None);
        }
        // cross-list trigger-vs-recipe mint collision (81915)
        let verb_duration: Option<i64> =
            site.verb.map(|vi| match self.content.verbs.get_index(vi) {
                Some((_, v)) => v.duration,
                None => unreachable!("verb index {vi} came from content.verbs enumeration"),
            });
        if let Some(n) = cross_list_mint_collision(self, site, verb_duration) {
            return (Err(CardBlocker::CrossListMintCollision { name: n }), None);
        }
        // frozen-completion rule (seed 296):
        // provable only for duration-0 recipes
        if self.frozen && site.verb.is_some() && verb_duration.unwrap_or(0) > 0 {
            return (
                Err(CardBlocker::FrozenDuration { duration: verb_duration.unwrap_or(0) }),
                None,
            );
        }
        // recipe-owned site: the `when` must be
        // static-true and the verb committable
        if let Some(vi) = site.verb {
            if let Some(when) = &site.when {
                let holds = pred_static(when) && eval_static_ok(when, world);
                if !holds {
                    return (Err(CardBlocker::WhenNotStaticTrue), None);
                }
            }
            let Some(proofs) = self.committable.get(&vi) else {
                let verb_name = match self.content.verbs.get_index(vi) {
                    Some((name, _)) => name.clone(),
                    None => unreachable!("verb index {vi} came from content.verbs enumeration"),
                };
                return (Err(CardBlocker::VerbNotCommittable { verb: verb_name }), None);
            };
            // completion-draft atomicity for the recipe's OWN mints (§6.7):
            // the outcomes land on the completion draft, and a complete
            // trigger that fires on this window and refuses discards the
            // whole scratch — the recipe's mints included. When every
            // completion of this verb force-locks such a trigger's home (a
            // min≥1 committed slot whose every world fit is named the
            // home), the recipe's completion always refuses and its mints
            // never land (fuzz seed 162: travel_to.r0's P3 against the t1
            // body's own P3 mint).
            if let Some(ri) = site.recipe {
                if let Some(name) = self.completion_contaminated(world, vi, ri) {
                    return (Err(CardBlocker::CompleteDraftCollision { name }), None);
                }
            }
            let verb_name = match self.content.verbs.get_index(vi) {
                Some((name, _)) => name.clone(),
                None => unreachable!("verb index {vi} came from content.verbs enumeration"),
            };
            return (Ok(slot_proof_lines(&verb_name, proofs)), None);
        }
        // trigger owner
        let Some((trig, _)) = self.trig_idx.get(&site.owner) else {
            return (Err(CardBlocker::UnknownGuard), None);
        };
        if let CompiledTrigger::Slot { id, slot, once, .. } = trig {
            let once_note = if *once {
                " (once: true — still fires its first crossing)"
            } else {
                " (every crossing)"
            };
            return (
                Ok(vec![format!(
                    "trigger {id} fires on slot {slot}{once_note} — the clock is unbounded forward"
                )]),
                None,
            );
        }
        if let CompiledTrigger::Complete { id, home, aspect, .. } = trig {
            // The tightened §6.7 model (Wave 3): "home obtainable" alone was
            // over-generous — the fuzz campaign's three classes of false
            // OBTAINABLE. The trigger is provable only when a completion
            // firing it is CONSTRUCTIBLE, in three rungs:
            //
            // ① the window witness — a committable verb whose committed-role
            //   slots can lock the home card, under a recipe that completes
            //   (selectable, no decided REFUSE, no deterministic
            //   completion-time error, no unconditional mint collision);
            // ② the aspect gate — the completed recipe must DECLARE the
            //   trigger's aspect (advance.rs matches `recipe.aspects`), so a
            //   selectable completable recipe must carry it;
            // ③ the completion draft — the window refusal is atomic (the
            //   whole scratch dies with any refusal), so the body's
            //   non-stackable mints must miss the draft occupants: the
            //   locked home itself, the completing recipe's own mints, or
            //   a same-home sibling trigger's mints. Judged PER WITNESS
            //   PAIR — complete triggers fire on every completion (no
            //   once), so reachability is existential over the pairs: one
            //   clean pair (that pair's completion draft holds no body
            //   mint) lets the mint land, and the site demotes only when
            //   EVERY pair is dirty.
            let Some(desc) = self.complete_home_desc(home) else {
                return (Err(CardBlocker::CompleteHomeMissing { home: home.clone() }), None);
            };
            let pairs = self.complete_window_pairs(world, &desc);
            if pairs.is_empty() {
                return (Err(CardBlocker::CompleteWindowUnbindable { home: home.clone() }), None);
            }
            let mut pairs = pairs;
            if let Some(a) = aspect {
                pairs.retain(|&(vi, ri)| {
                    let (_, v) = match self.content.verbs.get_index(vi) {
                        Some((n, v)) => (n, v),
                        None => unreachable!("verb index {vi} came from content.verbs enumeration"),
                    };
                    v.recipes[ri].aspects.iter().any(|x| x == a)
                });
                if pairs.is_empty() {
                    return (
                        Err(CardBlocker::CompleteAspectUnreachable {
                            home: home.clone(),
                            aspect: a.clone(),
                        }),
                        None,
                    );
                }
            }
            let clean =
                match self.complete_draft_clean_pairs(world, &pairs, home, desc.stackable, site) {
                    Ok(clean) => clean,
                    Err(name) => return (Err(CardBlocker::CompleteDraftCollision { name }), None),
                };
            // The first CLEAN witness renders the line (deterministic:
            // verbs in content.verbs order, recipes in declaration order) —
            // the completion walk it names survives its own draft.
            let (vi, ri) = clean[0];
            let (verb_name, v) = match self.content.verbs.get_index(vi) {
                Some((n, v)) => (n, v),
                None => unreachable!("verb index {vi} came from content.verbs enumeration"),
            };
            let role = self.locking_role(vi, &desc).map(|r| r.as_str()).unwrap_or("object");
            let aspect_note = if let Some(a) = aspect {
                format!(" and declares the '{a}' aspect")
            } else {
                String::new()
            };
            return (Ok(vec![format!(
                "trigger {id} fires when a window completes binding '{home}' — completion is arrangeable (commit {verb_name} locking '{home}' in its {role} slot; recipe {} completes the window{aspect_note})",
                v.recipes[ri].id
            )]), None);
        }
        let CompiledTrigger::Expire { id, on_expire_of, .. } = trig else {
            unreachable!("a compiled trigger is either Slot, Expire or Complete");
        };
        // expiry trigger — see expiry_support
        let Some(initial) = expiry_support(self, world, on_expire_of) else {
            return (Err(CardBlocker::ExpiryUnsupported { card: on_expire_of.clone() }), None);
        };
        let available = if initial { "initial" } else { "produced" };
        let writable_suffix = if initial { "" } else { " or minted" };
        let note = format!(
            "expiry trigger {id} reaches its body only while a living '{on_expire_of}' can expire"
        );
        (Ok(vec![format!(
            "trigger {id} fires on the expiry of {on_expire_of} (available: {available}; expires writable{writable_suffix})"
        )]), Some(note))
    }
}

// ---------------------------------------------------------------------------
// the blocker ladder (judgment and reason from ONE pass)
// ---------------------------------------------------------------------------

/// Why one site stays unproven — the FIRST ladder guard it fails. The
/// variants are the ladder rungs in evaluation order (the doc comments quote
/// the guards); [`CardBlocker::as_str`] renders the human text, aligned with
/// `render::site_reason`'s wording.
///
/// Extension seam: the Complete arm is known to be over-generous (fuzz is
/// exposing classes of false OBTAINABLE); the coming tightening adds its
/// demotion variants HERE — the ladder tail is the insertion point, existing
/// variants stay stable.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CardBlocker {
    /// `site.fallback` — ELSE runs only when earlier arms are false.
    Fallback,
    /// `site.killed_in_list` — the mint is destroyed later in the same list.
    KilledInList,
    /// `site.dup_risk` — a duplicate non-stackable mint refuses the draft.
    DupRisk,
    /// `self_gated` — gated on (or filled by) a card the list itself mints.
    SelfGated,
    /// a throwing op in the list (no self-safe REPLACE escape).
    ThrowOps,
    /// `site.shadow_risk` — an earlier recipe may shadow (first-match).
    ShadowRisk,
    /// `path_open` failed: the FIRST condition that is not a static truth,
    /// in the design-time spelling (from `path_open_first_failure` — the
    /// very walk that demotes).
    PathClosed { cond: String },
    /// a static gate names a card another owner destroys/replaces (seed
    /// 6028): the gate is only as durable as that name's aliveness.
    GateNameDestroyed { name: String },
    /// gate-timing (seed 40449): every proven producer of the gated name
    /// fires at or after the owner's own first crossing.
    GateTimingUnsatisfiable { name: String },
    /// gate-source kills a slot filler (seed 12460): every proven source of
    /// the gated name destroys one of the proof's filler names.
    FillerDoomed { name: String },
    /// cross-list trigger-vs-recipe mint collision (seed 81915): a slot
    /// trigger minting the gated name fires within this recipe's duration.
    CrossListMintCollision { name: String },
    /// frozen-completion rule (seed 296): only duration-0 recipes prove.
    FrozenDuration { duration: i64 },
    /// the recipe's `when` is not a static truth (non-static or false).
    WhenNotStaticTrue,
    /// the owning verb has no committability proof.
    VerbNotCommittable { verb: String },
    /// owner tail — expiry trigger: the named card has no obtainable
    /// descriptor that can be armed to expire.
    ExpiryUnsupported { card: String },
    /// owner tail — completion trigger (§6.7): the home card is not
    /// obtainable, so the trigger is never arrangeable.
    CompleteHomeMissing { home: String },
    /// owner tail — completion trigger, the window rung: no committable
    /// verb has a committed-role slot that can lock the home card under a
    /// completable recipe, so no window completing the binding exists.
    CompleteWindowUnbindable { home: String },
    /// owner tail — completion trigger, the aspect rung: the completed
    /// recipe must declare the aspect (advance.rs matches `recipe.aspects`),
    /// and no selectable completable recipe of any home-locking verb
    /// declares it — the filter never passes.
    CompleteAspectUnreachable { home: String, aspect: String },
    /// owner tail — completion trigger / completing recipe, the draft rung:
    /// the completion refusal is atomic (the whole scratch dies with any
    /// refusal), and a mint collides with a name the completion draft
    /// already holds — the locked home, the completing recipe's own mints,
    /// or a sibling trigger's mints — for EVERY (verb, recipe) witness pair
    /// (one clean pair would let the completion land), so the mint never
    /// lands.
    CompleteDraftCollision { name: String },
    /// the site's owner is not in the trigger index — defensively unreachable
    /// (mirrors `site_reason`'s "unknown guard" fallback).
    UnknownGuard,
}

impl CardBlocker {
    /// The human text (English, `render::site_reason`'s phrasing — several
    /// sentences are reused verbatim).
    #[must_use]
    pub fn as_str(&self) -> String {
        match self {
            CardBlocker::Fallback => {
                "ELSE fallback body — runs only when earlier arms are false — unprovable"
                    .to_string()
            }
            CardBlocker::KilledInList => "the mint is destroyed/Replaced later in the same outcome list — it never survives a step"
                .to_string(),
            CardBlocker::DupRisk => "this list mints a name that can collide with a living card (duplicate mint or an initial copy) — the refusal would discard the draft"
                .to_string(),
            CardBlocker::SelfGated => {
                "gated on (or filled by) a card the list itself mints".to_string()
            }
            CardBlocker::ThrowOps => {
                "a throwing op in this list may discard the whole draft at completion".to_string()
            }
            CardBlocker::ShadowRisk => {
                "an earlier recipe of the same verb may shadow it (first-match)".to_string()
            }
            CardBlocker::PathClosed { cond } => format!(
                "IF condition '{cond}' is not a static truth — the guarded arm never applies (anything non-static demotes)"
            ),
            CardBlocker::GateNameDestroyed { name } => format!(
                "a static gate names '{name}', which another owner destroys/replaces — the gate is only as durable as the aliveness of '{name}'"
            ),
            CardBlocker::GateTimingUnsatisfiable { name } => format!(
                "every proven producer of '{name}' fires at or after this trigger's own first crossing — '{name}' is not yet alive when the gate reads"
            ),
            CardBlocker::FillerDoomed { name } => format!(
                "every proven source of '{name}' destroys one of this verb's slot fillers (the gate-source-kills-filler rule)"
            ),
            CardBlocker::CrossListMintCollision { name } => format!(
                "a slot trigger minting '{name}' fires at or before this recipe's completion — the window boundary has already fired it or rolled the pass back"
            ),
            CardBlocker::FrozenDuration { duration } => format!(
                "the world is frozen past the first risky crossing — a duration-{duration} recipe completes there, and only duration-0 recipes prove"
            ),
            CardBlocker::WhenNotStaticTrue => {
                "the recipe `when` guard is not a static truth — it reads runtime state or evaluates false"
                    .to_string()
            }
            CardBlocker::VerbNotCommittable { verb } => format!(
                "verb '{verb}' committability unknown — no proof fills its min>0 slot(s)"
            ),
            CardBlocker::ExpiryUnsupported { card } => format!(
                "the expiry trigger fires only while a living '{card}' can expire — no obtainable '{card}' can be armed to expire"
            ),
            CardBlocker::CompleteHomeMissing { home } => format!(
                "the completion trigger fires only when a window completes binding '{home}' — '{home}' is not obtainable, so the trigger never fires"
            ),
            CardBlocker::CompleteWindowUnbindable { home } => format!(
                "no committable verb can lock '{home}' under a completable recipe — no window completing the binding exists, so the trigger never fires"
            ),
            CardBlocker::CompleteAspectUnreachable { home, aspect } => format!(
                "the completed recipe must declare the '{aspect}' aspect, and no selectable completable recipe of any verb that can lock '{home}' declares it — the filter never passes"
            ),
            CardBlocker::CompleteDraftCollision { name } => format!(
                "the completion draft already holds '{name}' (the locked home card, the completing recipe's own mints, or a sibling trigger's mint) — the body's mint refuses the draft and the whole completion is discarded"
            ),
            CardBlocker::UnknownGuard => "unknown guard".to_string(),
        }
    }
}

/// The producer kind of a blocked site (the census `SiteKind`, spelled out
/// for the public report shape).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SiteOp {
    Spawn,
    Replace,
    Draw,
}

impl SiteOp {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            SiteOp::Spawn => "SPAWN",
            SiteOp::Replace => "REPLACE",
            SiteOp::Draw => "DRAW",
        }
    }
}

/// One live unproven serving site with the ladder's first blocker — the
/// `--why` table's row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SiteBlocker {
    /// display owner: `recipe <id> of VERB` or `trigger <id>`
    pub owner: String,
    pub op: SiteOp,
    /// spawn/replace: the minted name; draw: the deck id
    pub name: String,
    pub blocker: CardBlocker,
}

/// The ladder's outcome: the proof-or-blocker verdict plus the expiry
/// arming note to record on the proven path (the ONLY write the ladder
/// performs, hoisted so the walk itself stays `&self` — the same pass feeds
/// `site_proven` and the `--why` diagnostics).
type Ladder = (Result<Vec<String>, CardBlocker>, Option<String>);

/// The content.verbs insertion index of a verb name (identity-key maps are
/// index-keyed; verb names are unique per content).
fn verb_idx_of(wf: &WorldFacts, verb_name: &str) -> usize {
    match wf.content.verbs.get_index_of(verb_name) {
        Some(vi) => vi,
        None => unreachable!("verb '{verb_name}' came from content.verbs enumeration"),
    }
}

/// The static gate names on a site's IF path and recipe `when` — the
/// exists/count names a static-true condition reads (polarity ignored on
/// purpose; the gate checks only DEMOTE).
fn static_gate_names(site: &RawSite) -> IndexSet<String> {
    let mut gated_names: IndexSet<String> = IndexSet::new();
    for p in site.path.iter().chain(site.when.iter()) {
        if pred_static(p) {
            pred_exists_names(p, &mut gated_names);
        }
    }
    gated_names
}

/// Gate-timing (seed 40449): a SLOT trigger's
/// static gate names a card whose only proven producers fire at (or
/// after) the trigger's own first crossing. Rules over recipe-owned sites
/// and non-slot owners do not apply. Returns the offending gated name (the
/// blocker's evidence) — None when the rule does not fire.
fn gate_timing_unsatisfiable(
    wf: &WorldFacts,
    site: &RawSite,
    gated_names: &IndexSet<String>,
) -> Option<String> {
    if site.verb.is_some() {
        return None;
    }
    let Some((CompiledTrigger::Slot { slot: owner_slot, .. }, owner_decl)) =
        wf.trig_idx.get(&site.owner)
    else {
        return None;
    };
    let my_first_cross = wf.first_cross(*owner_slot);
    let my_decl = owner_decl;
    let dead_at_my_firing = |src: &RawSite| -> bool {
        if src.verb.is_some() {
            return false; // recipe commit — player-timed
        }
        match wf.trig_idx.get(&src.owner) {
            None => true,
            Some((CompiledTrigger::Expire { .. }, _)) => false, // arrangeable
            // §6.7: a completion trigger mints when some window
            // completes binding its home card — not statically
            // timed, so its mints are never "dead at my firing"
            Some((CompiledTrigger::Complete { .. }, _)) => false,
            Some((CompiledTrigger::Slot { slot: s2, .. }, decl2)) => {
                if *s2 == *owner_slot {
                    decl2 > my_decl // same slot, later declaration
                } else {
                    wf.first_cross(*s2) > my_first_cross // not yet alive
                }
            }
        }
    };
    gated_names
        .iter()
        .find(|n| {
            let sources = wf.mint_sources.get(*n);
            !wf.init_names.contains_key(*n)
                && !site.list_gate_mints.contains(*n)
                && sources.is_some_and(|v| !v.is_empty())
                && sources
                    .is_some_and(|v| v.iter().all(|&src| dead_at_my_firing(&wf.census.sites[src])))
        })
        .cloned()
}

/// Gate-source kills a slot filler (12460): every proven source of a gated
/// name is a site whose list destroys one of the proof's filler names.
/// Returns the offending gated name — None when the rule does not fire.
fn filler_doomed(
    wf: &WorldFacts,
    site: &RawSite,
    gated_names: &IndexSet<String>,
) -> Option<String> {
    let vi = site.verb?;
    let f = wf.fillers.get(&vi)?;
    if f.is_empty() {
        return None;
    }
    let kills_filler = |src: &RawSite| -> bool {
        f.iter().any(|y| wf.census.destroyed_by.get(y).is_some_and(|set| set.contains(&src.owner)))
    };
    gated_names
        .iter()
        .find(|n| {
            let sources = wf.mint_sources.get(*n);
            !wf.init_names.contains_key(*n)
                && sources.is_some_and(|v| !v.is_empty())
                && sources.is_some_and(|v| v.iter().all(|&src| kills_filler(&wf.census.sites[src])))
        })
        .cloned()
}

/// Cross-list trigger-vs-recipe mint collision
/// (81915): every window completes at begin+duration ≥ B, so the
/// boundary at B has already fired the trigger or rolled the pass back.
/// Returns the colliding mint name — None when the rule does not fire.
fn cross_list_mint_collision(
    wf: &WorldFacts,
    site: &RawSite,
    verb_duration: Option<i64>,
) -> Option<String> {
    let dur = verb_duration?;
    for (owner2, mints2) in &wf.trigger_mint_names {
        if mints2.is_empty() {
            continue;
        }
        let Some((CompiledTrigger::Slot { slot: s2, .. }, _)) = wf.trig_idx.get(owner2) else {
            continue;
        };
        if dur < wf.first_cross(*s2) {
            continue;
        }
        for n in mints2 {
            if site.list_gate_mints.contains(n) {
                return Some(n.clone());
            }
        }
    }
    None
}

/// The expiry trigger's provability: the named card must be
/// obtainable AND able to expire. The arming-witness recursion is the
/// seed-100625 rule; `visiting` cuts arming cycles (the seed-41247
/// self-arming check is subsumed), and the memo caches only cycle-free
/// (top-level) verdicts. Returns whether the named card is an initial
/// entity (the support line's available-label); None when the trigger
/// stays unproven.
fn expiry_support(wf: &WorldFacts, world: &CardWorld, on_expire_of: &str) -> Option<bool> {
    let desc: Option<Entity> =
        wf.init_names.get(on_expire_of).map(|e| (*e).clone()).or_else(|| {
            wf.avail.get(on_expire_of).and_then(|list| list.first()).map(|f| f.entity.clone())
        });
    let mut expiry_memo: IndexMap<String, bool> = IndexMap::new();
    let expirable = desc.is_some()
        && expirable_card(wf, world, &mut expiry_memo, on_expire_of, &IndexSet::new());
    if desc.is_none() || !expirable {
        return None;
    }
    Some(wf.init_names.contains_key(on_expire_of))
}

// ---------------------------------------------------------------------------
// the completion-trigger model (§6.7) — the Wave-3 tightening
// ---------------------------------------------------------------------------

/// The committed roles whose bindings a window locks at commit — engine
/// exec.rs's COMMITTED_ROLES. The actors stay free (a player is never
/// locked), so an actors-only verb can never arrange the binding.
const COMMITTED_ROLES: [Role; 5] = [Role::Object, Role::With, Role::At, Role::About, Role::To];

impl<'c> WorldFacts<'c> {
    /// The home card's static descriptor — the expiry_support desc-lookup
    /// shape: the initial representative, else the first avail fact. Its
    /// presence is the old `complete_home_obtainable` verdict; its
    /// kind/aspect body feeds the slot-binding rungs.
    fn complete_home_desc(&self, home: &str) -> Option<Entity> {
        self.init_names.get(home).map(|e| (*e).clone()).or_else(|| {
            self.avail.get(home).and_then(|facts| facts.first()).map(|f| f.entity.clone())
        })
    }

    /// Can this committed-role slot lock a card with `desc`? The slot must
    /// accept one copy (max >= 1, query fits) and its min must be
    /// satisfiable AROUND the home: `slot.min` distinct matching instances
    /// must exist with the home among them (the same instance-counting as
    /// `slot_support`'s candidate lists). A 0..1 slot accepts an optional
    /// fill, so the player can still include the home in the clause — the
    /// lock is arrangeable.
    fn slot_can_lock(&self, slot: &CompiledSlot, desc: &Entity) -> bool {
        if slot.max.is_some_and(|max| max < 1) {
            return false;
        }
        if !slot.query.as_ref().is_none_or(|q| match_query(desc, q)) {
            return false;
        }
        if slot.min <= 1 {
            return true;
        }
        // min >= 2: the home is one of >= min DISTINCT matching instances
        let mut pool = 0i64;
        for facts in self.avail.values() {
            for f in facts {
                if slot.query.as_ref().is_none_or(|q| match_query(&f.entity, q)) {
                    pool += if f.entity.stackable {
                        std::cmp::min(f.count, SLOT_CANDIDATE_CAP)
                    } else {
                        1
                    };
                }
            }
        }
        pool >= slot.min
    }

    /// The first committed role whose slot can lock `desc` (the support
    /// line's witness; COMMITTED_ROLES order keeps it deterministic).
    fn locking_role(&self, vi: usize, desc: &Entity) -> Option<Role> {
        let (_, v) = self.content.verbs.get_index(vi)?;
        COMMITTED_ROLES.into_iter().find(|role| {
            v.slot_by_role.get(role.as_str()).is_some_and(|slot| self.slot_can_lock(slot, desc))
        })
    }

    /// Whether EVERY world descriptor fitting one of the verb's min≥1
    /// committed slots is named `home` — that slot force-locks a home-named
    /// card on every commit, so EVERY completion of the verb fires the
    /// home's (aspect-permitting) triggers. A min-0 slot (e.g. 0..1) is
    /// never a forced lock: the player can leave it empty and commit the
    /// completion without binding the home at all. `desc` is the home's own
    /// descriptor; it must fit the slot too (an unobtainable home cannot be
    /// locked).
    fn verb_force_locks(&self, vi: usize, world: &CardWorld, home: &str, desc: &Entity) -> bool {
        let Some((_, v)) = self.content.verbs.get_index(vi) else {
            unreachable!("verb index {vi} came from content.verbs enumeration");
        };
        COMMITTED_ROLES.into_iter().any(|role| {
            let Some(slot) = v.slot_by_role.get(role.as_str()) else {
                return false;
            };
            if slot.min < 1 {
                return false; // an optional slot can be left empty — nothing is forced
            }
            if slot.max.is_some_and(|max| max < 1) {
                return false;
            }
            if !slot.query.as_ref().is_none_or(|q| match_query(desc, q)) {
                return false;
            }
            world
                .distinct
                .iter()
                .filter(|d| slot.query.as_ref().is_none_or(|q| match_query(&d.e, q)))
                .all(|d| d.e.name == home)
        })
    }

    /// The non-stackable names an owner's live, path-open, non-fallback,
    /// not-killed-in-list sites put on a completion draft (a stackable mint
    /// never refuses a draft, §9.2; a draw contributes its deterministic
    /// first served position — the analyzed world's deck cursor starts at 0,
    /// and a refused draft never advanced it).
    fn owner_draft_names(&self, world: &CardWorld, owner: &str) -> IndexSet<String> {
        let mut out: IndexSet<String> = IndexSet::new();
        for &i in &self.census.live {
            let s = &self.census.sites[i];
            if s.owner != owner || s.fallback || s.killed_in_list {
                continue;
            }
            if path_open_first_failure(&s.path, world).is_some() {
                continue; // roll/role-gated or statically false: not unconditional
            }
            if s.kind == SiteKind::Draw && s.deck.is_some() {
                let deck = draw_deck(s);
                if s.cap >= 1 {
                    if let Some(c) = deck.order.first() {
                        if !mint_stacks(self.content, &c.name, Some(c.kind.as_str())) {
                            out.insert(c.name.clone());
                        }
                    }
                }
            } else if !mint_stacks(self.content, &s.name, s.kind_override.as_deref()) {
                out.insert(s.name.clone());
            }
        }
        out
    }

    /// The completing recipe's first-application draft, or None when the
    /// recipe deterministically refuses its window (the trigger phase never
    /// runs). `Some(names)` carries the non-stackable names the draft holds
    /// when the trigger phase begins.
    ///
    /// The verdict has three parts:
    /// (a) selectability — first-match (§6.4): an earlier when-less recipe
    ///     shadows permanently, and an earlier static-true `when` always
    ///     matches first; a static-false `when` of this recipe never matches;
    /// (b) the always-applying outcome walk — a decided REFUSE, or a
    ///     throwing op whose name targets cannot resolve against the world
    ///     (an application ERROR is not a gate at the table: exec.rs's
    ///     `table_gate_trial` defers it to the completion, where it refuses
    ///     the window);
    /// (c) unconditional mint collisions — a non-stackable mint (or the
    ///     deterministic first draw) colliding with a living initial card or
    ///     a sibling unconditional mint; and an unconditional draw whose
    ///     deck can never durably serve (the census froze its cap to 0).
    ///
    /// Roll/role-gated ops sit out every part: they can fail this
    /// application, and the window-refuse retry is existential.
    fn recipe_draft(&self, world: &CardWorld, vi: usize, ri: usize) -> Option<IndexSet<String>> {
        let (_, v) = self.content.verbs.get_index(vi)?;
        let r = v.recipes.get(ri)?;
        // (a) selectability
        if v.recipes[..ri].iter().any(|rr| rr.when.is_none()) {
            return None; // an earlier when-less recipe shadows permanently
        }
        if let Some(w) = &r.when {
            if pred_static(w) {
                if !eval_static_ok(w, world) {
                    return None; // never matches
                }
                if v.recipes[..ri].iter().any(|rr| {
                    rr.when.as_ref().is_some_and(|we| pred_static(we) && eval_static_ok(we, world))
                }) {
                    return None; // an earlier static-true when always beats it
                }
            }
        }
        // (b) the always-applying walk
        let mut decided = false;
        let root: Vec<Pred> = Vec::new();
        let _ = walk_outcomes(
            &r.out,
            &root,
            &mut (),
            &mut |_st, o, path| {
                if !path.iter().all(|p| pred_static(p) && eval_static_ok(p, world)) {
                    return Ok(()); // can fail this application — no decision
                }
                if o.op == "REFUSE" {
                    decided = true;
                    return Ok(());
                }
                if op_throws(o) {
                    // MOVE/RELATE/UNRELATE/REPLACE target the FIRST arg;
                    // DRAW's INTO clause targets the second. A name that
                    // cannot resolve at completion errors there and refuses
                    // the window; role targets ride the window's bindings.
                    let skips: usize = if o.op == "DRAW" { 1 } else { 0 };
                    for a in o.args.iter().skip(skips) {
                        let ok = match a {
                            Arg::Ref { r#ref: Ref::Name { name, .. } } => {
                                world.by_name.contains_key(name)
                            }
                            Arg::Role { .. } => true,
                            Arg::Into { r#ref } => match r#ref {
                                IntoTarget::Role { .. } => true,
                                IntoTarget::Ref(Ref::Name { name, .. }) => {
                                    world.by_name.contains_key(name)
                                }
                                IntoTarget::Ref(_) => false,
                            },
                            _ => true,
                        };
                        if !ok {
                            decided = true;
                        }
                    }
                }
                Ok(())
            },
            &mut |_st, arm, path| {
                let mut p = path.clone();
                p.push(arm.when.clone());
                p
            },
            &mut |_st, path| path.clone(),
        );
        if decided {
            return None;
        }
        // (c) the first-application draft from the census sites
        let owner = format!("recipe {} of {}", r.id, v.name);
        let mut draft: IndexSet<String> = IndexSet::new();
        for &i in &self.census.live {
            let s = &self.census.sites[i];
            if s.owner != owner || s.fallback || s.killed_in_list {
                continue;
            }
            if path_open_first_failure(&s.path, world).is_some() {
                continue; // roll/role-gated or statically false: not unconditional
            }
            // the site's draft mint: a draw contributes its deterministic
            // first served position; anything else mints its own name
            let (stacks, mint) = if s.kind == SiteKind::Draw && s.deck.is_some() {
                let deck = draw_deck(s);
                if s.cap < 1 {
                    return None; // the draw throws on every application
                }
                let c = deck.order.first()?;
                (mint_stacks(self.content, &c.name, Some(c.kind.as_str())), c.name.clone())
            } else {
                (mint_stacks(self.content, &s.name, s.kind_override.as_deref()), s.name.clone())
            };
            if !stacks {
                // a non-stackable draft name colliding with a sibling mint
                // or a living initial card refuses the first application
                if draft.contains(&mint) || self.init_names.get(&mint).is_some_and(|e| !e.stackable)
                {
                    return None;
                }
                draft.insert(mint);
            }
        }
        Some(draft)
    }

    /// ① the window witnesses: every (verb, recipe) pair with the verb
    /// committable and able to lock a card with `desc` in a committed-role
    /// slot, and the recipe completable (a first application reaching the
    /// trigger phase). NO duration condition applies: the engine registers
    /// and completes duration-0 windows exactly like duration>0 ones
    /// (`register_windows` filters nothing; the next advance closes them).
    fn complete_window_pairs(&self, world: &CardWorld, desc: &Entity) -> Vec<(usize, usize)> {
        let mut out = Vec::new();
        for (&vi, _) in &self.committable {
            if self.locking_role(vi, desc).is_none() {
                continue;
            }
            let Some((_, v)) = self.content.verbs.get_index(vi) else {
                unreachable!("verb index {vi} came from content.verbs enumeration");
            };
            for ri in 0..v.recipes.len() {
                if self.recipe_draft(world, vi, ri).is_some() {
                    out.push((vi, ri));
                }
            }
        }
        out
    }

    /// ③ the draft rung for the trigger site, judged PER WITNESS PAIR —
    /// complete triggers fire on every completion (no once), so
    /// reachability is EXISTENTIAL over the (verb, recipe) witnesses: the
    /// site proves by walking any pair whose completion draft holds none
    /// of the body's unconditional non-stackable mints, and demotes only
    /// when EVERY pair is dirty (`Err` carries the first dirty pair's
    /// colliding name). The occupants are a property of the GIVEN pair,
    /// never a global union: the locked home itself, that pair's
    /// completing recipe's first-application mints, and the same-home
    /// sibling triggers' mints firing beside it (one shared scratch per
    /// completion pass, in declaration order — a sibling joins exactly
    /// when its aspect is absent or the PAIR's recipe declares it).
    fn complete_draft_clean_pairs(
        &self,
        world: &CardWorld,
        pairs: &[(usize, usize)],
        home: &str,
        home_stackable: bool,
        site: &RawSite<'_>,
    ) -> Result<Vec<(usize, usize)>, String> {
        let self_mints = self.owner_draft_names(world, &site.owner);
        // the trigger fires BECAUSE a living locked card is named `home` —
        // minting `home` refuses every firing, whatever the witness
        if !home_stackable && self_mints.contains(home) {
            return Err(home.to_string());
        }
        let mut clean: Vec<(usize, usize)> = Vec::new();
        let mut first_dirty: Option<String> = None;
        for &(vi, ri) in pairs {
            let v = match self.content.verbs.get_index(vi) {
                Some((_, v)) => v,
                None => unreachable!("verb index {vi} came from content.verbs enumeration"),
            };
            let r = &v.recipes[ri];
            let mut forbidden: IndexSet<String> = IndexSet::new();
            if !home_stackable {
                forbidden.insert(home.to_string());
            }
            if let Some(draft) = self.recipe_draft(world, vi, ri) {
                forbidden.extend(draft);
            }
            for t in &self.content.triggers {
                let CompiledTrigger::Complete { home: h2, aspect: a2, .. } = t else {
                    continue;
                };
                if h2 != home || trigger_owner(t.id()) == site.owner {
                    continue;
                }
                if a2.as_ref().is_none_or(|a| r.aspects.contains(a)) {
                    forbidden.extend(self.owner_draft_names(world, &trigger_owner(t.id())));
                }
            }
            if let Some(n) = self_mints.iter().find(|n| forbidden.contains(*n)) {
                if first_dirty.is_none() {
                    first_dirty = Some(n.clone());
                }
            } else {
                clean.push((vi, ri));
            }
        }
        match (clean.is_empty(), first_dirty) {
            // the window/aspect rungs guarantee a non-empty witness list, so
            // an empty clean list means every pair was dirty and the first
            // dirty name is on record (the home fallback is defensive only)
            (true, Some(n)) => Err(n),
            (true, None) => Err(home.to_string()),
            (false, _) => Ok(clean),
        }
    }

    /// The completing recipe's own contamination rung: a complete trigger
    /// whose home every commit of the verb force-locks (a min≥1 committed
    /// slot whose every world fit is named the home — a min-0 slot can be
    /// left empty, so it never forces) fires on EVERY completion of this
    /// recipe (the aspect gate permitting); when its body mints a name the
    /// draft already holds, the window refuses and the recipe's outcomes
    /// die with the scratch — the recipe's mints never land (fuzz seed
    /// 162: travel_to.r0's P3 against the t1 body's own P3 mint, the home
    /// force-locked by the verb's only text-fitting slot).
    fn completion_contaminated(&self, world: &CardWorld, vi: usize, ri: usize) -> Option<String> {
        if !self.content.triggers.iter().any(|t| matches!(t, CompiledTrigger::Complete { .. })) {
            return None;
        }
        let (_, v) = self.content.verbs.get_index(vi)?;
        let r = &v.recipes[ri];
        // the force-locked homes first (the cheap scan): nothing locked
        // means the player can complete without firing any trigger
        let mut locked: Vec<(String, String, Entity)> = Vec::new();
        for t in &self.content.triggers {
            let CompiledTrigger::Complete { id, home, aspect, .. } = t else {
                continue;
            };
            if aspect.as_ref().is_some_and(|a| !r.aspects.contains(a)) {
                continue; // the gate never passes on this recipe's completions
            }
            let Some(desc) = self.complete_home_desc(home) else {
                continue; // an unobtainable home cannot be locked
            };
            if self.verb_force_locks(vi, world, home, &desc) {
                locked.push((home.clone(), trigger_owner(id), desc));
            }
        }
        if locked.is_empty() {
            return None;
        }
        let mut draft = self.recipe_draft(world, vi, ri)?;
        for (home, owner, desc) in locked {
            if !desc.stackable {
                draft.insert(home);
            }
            let body = self.owner_draft_names(world, &owner);
            if let Some(n) = body.iter().find(|n| draft.contains(*n)) {
                return Some(n.clone());
            }
        }
        None
    }
}

/// The display lines of a verb's structured
/// committability proof (byte-identical to the pre-structure format).
/// `pub(crate)`: the `--verb` report re-renders these SAME lines — the
/// proof text has one source (zero new analysis).
pub(crate) fn slot_proof_lines(verb_name: &str, proofs: &[SlotProof]) -> Vec<String> {
    proofs
        .iter()
        .map(|pf| {
            let mut whos: IndexSet<String> = IndexSet::new();
            for f in &pf.fillers {
                whos.insert(f.how.clone());
            }
            let whos: Vec<String> = whos.into_iter().take(3).collect();
            format!("{verb_name} committable: {} ×{} ← {}", pf.role, pf.min, whos.join(", "))
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
    let gates = static_gate_names(site);
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
                    let fits = slot.query.as_ref().is_none_or(|q| match_query(&f.entity, q));
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
/// slot triggers fire unconditionally; a completion trigger is arrangeable
/// (§6.7 — begin a window binding its home card); an expiry trigger needs
/// its own card expirable.
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
        // §6.7: a completion trigger fires whenever a window completes
        // binding its home card — treated as arrangeable, the generous
        // (sound-for-'none') side
        Some((CompiledTrigger::Complete { .. }, _)) => true,
        Some((CompiledTrigger::Expire { on_expire_of, .. }, _)) => {
            expirable_card(wf, world, memo, on_expire_of, visiting)
        }
    }
}

/// The arming SET's role target must bind a card
/// whose descriptor fits the role's slot query.
fn role_slot_fits(wf: &WorldFacts, s: &SetSite, role: &str, d: &Entity) -> bool {
    let v = s.verb_name.as_deref().and_then(|vn| wf.content.verbs.get(vn));
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
    let next: IndexSet<String> =
        visiting.iter().cloned().chain(std::iter::once(card_name.to_string())).collect();
    let d: Option<Entity> = wf.init_names.get(card_name).map(|e| (*e).clone()).or_else(|| {
        wf.avail.get(card_name).and_then(|list| list.first()).map(|f| f.entity.clone())
    });
    let mut result = d.as_ref().is_some_and(|de| de.attrs.contains_key("expires"));
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
                    Some(wp) => pred_static(wp) && eval_static_ok(wp, world),
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
