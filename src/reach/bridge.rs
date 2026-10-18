//! The lint bridge (§13.1 → §13.2): the reach analyzer's global-dynamic
//! findings surfaced as ADVISORY lint findings. The load-time lint (§13.2)
//! is per-site local — it can read one list, one `when`, one slot, never the
//! whole world; the settled WorldFacts ([`Analyzer::facts`]) know what the
//! lint cannot see. This module is a RE-RENDER of those facts, zero new
//! analysis (the ninth seam module, after `cost` — `world_report`, `export`
//! and `cost` set the zero-new-analysis precedent): one [`Analyzer`] per
//! call, every finding read off
//! the fixpoint's verdicts — `dead_verbs`, the site ladder's
//! [`CardBlocker::SelfGated`] rung, `risky_owners`, the census draw caps,
//! the freeze boundary and the refused census.
//!
//! Contract: every finding is WARNING-class (the §13.2 discipline — a reach
//! finding is a design smell, never an error; findings never change the exit
//! code), owner-addressed (reach has no line numbers), deduplicated per
//! owner and emitted in a deterministic order (class order below, then
//! content order — verbs before triggers, the census walk; every walk is an
//! IndexMap/IndexSet insertion order). The message texts reuse the lint's
//! house style (lowercase diagnoses, the § reference where apt) and the
//! lint's own [`LintFinding`] type, so `bin/lint-scripts --reach` prints one
//! uniform stream.

use indexmap::IndexSet;

use crate::content::script::{LintFinding, LintSeverity};
use crate::content::{CompiledContent, CompiledTrigger};
use crate::core::dict::Dictionary;
use crate::core::types::State;

use super::census::{trigger_owner, SiteKind, MANY};
use super::fixpoint::CardBlocker;
use super::render::query_text;
use super::world::Analyzer;

/// The reach bridge over one compiled script: the global-dynamic WARNINGs
/// the per-site lint (§13.2) cannot see, read off the analyzer's settled
/// world facts. Six finding classes, in this order:
///
/// 1. **dead verb** — `facts.dead_verbs`: the verb never commits because a
///    min>0 slot's query can never match anything the world can produce
///    (the killing slot is the FIRST such slot in `slot_by_role` order —
///    exactly the slot `--world`'s dead-verbs row renders).
/// 2. **self-gated list** — live unproven sites whose ladder's FIRST
///    blocker is [`CardBlocker::SelfGated`]: the list gates on (or fills a
///    slot with) a card it itself mints. The blocker comes from the SAME
///    ladder pass the fixpoint ran (`site_ladder` over the read-only world
///    rebuild — the single-source rule: no re-derivation), one finding per
///    owner.
/// 3. **risky owner** — `facts.risky_owners`, one finding per owner: the
///    owner's non-stackable mints collide with a living card or another
///    owner's mint at a crossing, so the draft refuses.
/// 4. **deck-cap cursor freeze** — live DRAW sites whose census cap is
///    frozen below unbounded on a REPEATABLE owner (a once trigger's single
///    draw is by design): a non-stackable co-mint in the same list pins the
///    deck cursor, one finding per (owner, deck).
/// 5. **frozen world** — `facts.frozen`, one global finding: the boundary
///    minute (`initial.now + freeze_dist`) and the risky slot triggers
///    standing on it — the very owners whose first crossings the freeze
///    distance minimizes over.
/// 6. **REFUSE-doomed sites** — the refused census, one finding naming the
///    owners (the world note already carries the count; the lint form names
///    owners).
#[must_use]
pub fn reach_findings(
    content: &CompiledContent,
    initial: &State,
    dict: &Dictionary,
) -> Vec<LintFinding> {
    let analyzer = Analyzer::new(content, initial, dict);
    let facts = analyzer.facts();
    let mut findings: Vec<LintFinding> = Vec::new();
    let mut warn = |message: String| {
        findings.push(LintFinding { severity: LintSeverity::Warning, message });
    };

    // 1. dead verbs — the killing slot is the first (slot_by_role order)
    // min>0 slot whose query can never match: the very predicate
    // build_post_fixpoint condemned the verb with, walked the way
    // world_report's dead-verbs row walks it
    for &vi in &facts.dead_verbs {
        let Some((_, verb)) = content.verbs.get_index(vi) else {
            unreachable!("verb index {vi} came from content.verbs enumeration");
        };
        let Some((role, slot)) = verb
            .slot_by_role
            .iter()
            .find(|(_, s)| s.min > 0 && !facts.can_ever_match(s.query.as_ref()))
        else {
            continue; // the deadness predicate drifted (paths.rs) — omit, don't panic
        };
        let Some(q) = &slot.query else { continue };
        warn(format!(
            "verb {} never commits — its {} slot's query ({}) can never match anything the world can produce (§13.1)",
            verb.name,
            role,
            query_text(q)
        ));
    }

    // 2. self-gated lists — the ladder's own verdict: every live unproven
    // site re-walks site_ladder over the read-only world rebuild (the same
    // pass card_blockers runs; post-fixpoint convergence makes the verdict
    // Err), and only the SelfGated rung is surfaced. Dedup per owner.
    let world = facts.build_world();
    let mut seen: IndexSet<String> = IndexSet::new();
    for &i in &facts.census.live {
        if facts.proven.contains_key(&i) {
            continue;
        }
        let (res, _) = facts.site_ladder(i, &world);
        let Err(blocker) = res else {
            debug_assert!(false, "unproven site proved in the bridge pass (schedule drift)");
            continue;
        };
        if !matches!(blocker, CardBlocker::SelfGated) {
            continue;
        }
        let owner = facts.census.sites[i].owner.clone();
        if seen.insert(owner.clone()) {
            warn(format!("{owner}: {} (§13.1)", blocker.as_str()));
        }
    }

    // 3. risky owners — one finding per owner, insertion order (the risky
    // block's demotion order)
    for owner in &facts.risky_owners {
        warn(format!(
            "{owner}: its list's non-stackable mints can collide with a living card or another owner's mint at a crossing — the collision refuses the whole draft (§13.1)"
        ));
    }

    // 4. deck-cap cursor freeze — a live draw whose census cap froze below
    // unbounded on a repeatable owner: the co-mint freeze of census.rs's
    // draw_cap (a once trigger's cap-1 draw is its design, not a smell).
    // One finding per (owner, deck).
    let mut cap_seen: IndexSet<(String, String)> = IndexSet::new();
    for &i in &facts.census.live {
        let s = &facts.census.sites[i];
        if s.kind != SiteKind::Draw || s.deck.is_none() || s.cap >= MANY {
            continue;
        }
        let once = match facts.trig_idx.get(&s.owner) {
            Some((CompiledTrigger::Slot { once, .. }, _)) => *once,
            _ => false,
        };
        if once {
            continue;
        }
        if cap_seen.insert((s.owner.clone(), s.name.clone())) {
            warn(format!(
                "{}: its DRAW from deck {} freezes the cursor at {} served position(s) — a non-stackable co-mint in the same list pins the draw (§13.1)",
                s.owner, s.name, s.cap
            ));
        }
    }

    // 5. the frozen world — one global finding: the boundary minute plus the
    // risky slot triggers standing ON it. freeze_dist is the MIN first
    // crossing over the risky slot triggers (world.rs's risky block), so the
    // owners at that crossing are exactly the risky triggers whose first
    // crossing equals it.
    if facts.frozen {
        let boundary = initial.now + facts.freeze_dist;
        let mut boundary_owners: Vec<String> = Vec::new();
        for t in &content.triggers {
            let CompiledTrigger::Slot { id, slot, .. } = t else {
                continue;
            };
            let owner = trigger_owner(id);
            if facts.risky_owners.contains(&owner) && facts.first_cross(*slot) == facts.freeze_dist
            {
                boundary_owners.push(owner);
            }
        }
        if boundary_owners.is_empty() {
            // defensive: frozen worlds carry at least one risky trigger, so
            // the walk above cannot come up empty — name them all if it did
            boundary_owners.extend(facts.risky_owners.iter().cloned());
        }
        warn(format!(
            "the world is frozen — the first risky crossing lands at minute {boundary} ({}): every draft completing there or later refuses (§13.1)",
            boundary_owners.join(", ")
        ));
    }

    // 6. REFUSE-doomed sites — one finding; the world note names the count,
    // the lint form names the owners (first-mention order)
    if !facts.census.refused.is_empty() {
        let mut owners: Vec<String> = Vec::new();
        for &i in &facts.census.refused {
            let owner = facts.census.sites[i].owner.clone();
            if !owners.contains(&owner) {
                owners.push(owner);
            }
        }
        warn(format!(
            "{} producer site(s) sit behind an authored REFUSE — they can never apply: {} (§13.1)",
            facts.census.refused.len(),
            owners.join(", ")
        ));
    }

    findings
}
