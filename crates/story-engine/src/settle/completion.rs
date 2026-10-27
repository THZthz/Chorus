//! The completion sequence and the `continue via` adoption transition
//! (spec.md §4.2).

use super::Run;
use crate::effects::EffectCtx;
use crate::mint::Fail;
use crate::{Engine, Event};
use std::collections::{BTreeMap, BTreeSet};
use story_content::ir::EffectNode;
use story_content::runtime::DiagKind;
use story_core::{EntityId, SituationId, ZoneId};

// ---------------------------------------------------------------------------
// Completion (spec.md §4.2)
// ---------------------------------------------------------------------------

/// The `continue via` targets of a recipe's effect list, in static pre-order
/// — the walk order the arms were emitted in.
fn continue_arms(effects: &[EffectNode]) -> Vec<EffectNode> {
    let mut out = Vec::new();
    for node in effects {
        match node {
            EffectNode::Continue { .. } => out.push(node.clone()),
            EffectNode::Branch { then, els, .. } => {
                out.extend(continue_arms(then));
                out.extend(continue_arms(els));
            }
            _ => {}
        }
    }
    out
}

impl Engine {
    /// The advance-path completion of one queued situation: the unit
    /// checkpoints before its first effect; an effect failure rolls the unit
    /// back to that checkpoint and aborts the situation through the release
    /// path with `ResolutionFailed`; the advance continues (T4).
    pub(crate) fn settle_completion(
        &mut self,
        sit: SituationId,
        ev: &mut Vec<Event>,
        run: &mut Run<'_>,
    ) -> Result<(), Fail> {
        let checkpoint = self.state.clone();
        match self.completion_steps(sit, ev, run) {
            Ok(()) => Ok(()),
            // The budget guard already restored the entry state; the whole
            // call unwinds (T5).
            Err(f) if f.kind == DiagKind::Livelock => Err(f),
            Err(f) => {
                self.state = checkpoint;
                self.abort_situation(sit, &f, ev, run);
                Ok(())
            }
        }
    }

    /// The completion sequence without its own checkpoint: an instant
    /// recipe's open-complete cycle sits inside the atomic action of its
    /// opener (the commit's or the resolution unit's checkpoint), so a
    /// failure propagates to the enclosing unit (spec.md §4.2).
    pub(crate) fn completion_steps(
        &mut self,
        sit: SituationId,
        ev: &mut Vec<Event>,
        run: &mut Run<'_>,
    ) -> Result<(), Fail> {
        let Some(s) = self.state.situations.get(&sit) else {
            return Ok(());
        };
        let recipe_id = s.recipe.clone();
        let content = self.content.clone();
        let rec = content.recipe(recipe_id.as_str()).unwrap_or_else(|| {
            crate::kernel_invariant("UnknownRecipe", format!("unknown recipe `{recipe_id}`"))
        });
        // Greedy top-up just before the completion evaluates (spec.md
        // §4.4): unfilled greedy slots pull cards that became available
        // mid-flight, so the completion's own conditions read them.
        self.top_up_greedy(sit, rec, ev);
        let mut final_bindings: BTreeMap<story_core::SlotName, Vec<story_core::CardId>> =
            BTreeMap::new();
        if let Some(s) = self.state.situations.get(&sit) {
            for (slot, b_list) in &s.bindings {
                let cards: Vec<story_core::CardId> = b_list
                    .iter()
                    .filter_map(|b| {
                        self.state
                            .entities
                            .get(&b.entity)
                            .map(|e| e.prototype.clone())
                    })
                    .collect();
                final_bindings.insert(slot.clone(), cards);
            }
        }
        let mut fctx = EffectCtx {
            sit: Some(sit),
            now: self.state.now,
            roll: None,
            handles: BTreeMap::new(),
            continued: None,
        };
        // Step 1: the recipe's effects, in order.
        self.run_list(
            &rec.effects,
            &mut fctx,
            Some(recipe_id.clone()),
            ev,
            run,
            true,
        )?;
        // The continuation gate is decided here, from the pool as the
        // executing recipe left it — the bound cards still standing —
        // before the books walk can transmute or burn the evidence an
        // arm's `when` counts. The transition itself still runs after the
        // books; each arm's `max` is consulted at adoption (spec.md §4.2).
        let when_passed: Vec<story_core::RecipeId> = if self.state.ended.is_none() {
            match fctx.continued.clone() {
                Some(executed) => {
                    let arms = continue_arms(&rec.effects);
                    let mut ids: Vec<story_core::RecipeId> = arms
                        .into_iter()
                        .filter_map(|n| match n {
                            EffectNode::Continue { recipe } => Some(recipe),
                            _ => None,
                        })
                        .collect();
                    if !ids.is_empty() {
                        let start = ids.iter().position(|a| *a == executed).unwrap_or(0);
                        let remaining: Vec<story_core::RecipeId> = ids.drain(start..).collect();
                        remaining
                            .into_iter()
                            .filter(|arm| {
                                let rec = content.recipe(arm.as_str()).unwrap_or_else(|| {
                                    crate::kernel_invariant(
                                        "UnknownRecipe",
                                        format!("unknown recipe `{arm}`"),
                                    )
                                });
                                match &rec.when {
                                    None => true,
                                    Some(w) => {
                                        let ctx = crate::eval::Ctx {
                                            sit: Some(sit),
                                            sel_bindings: None,
                                            subject: None,
                                            roll: None,
                                            recipe: Some(rec.id.clone()),
                                        };
                                        self.eval_cond_ctx(w, &ctx)
                                    }
                                }
                            })
                            .collect()
                    } else {
                        Vec::new()
                    }
                }
                None => Vec::new(),
            }
        } else {
            Vec::new()
        };
        // Step 2: consuming slots burn their surviving bound cards, in slot
        // declaration order, then bind order.
        for sd in &rec.slots {
            if !sd.consuming {
                continue;
            }
            let bound = self
                .state
                .situations
                .get(&sit)
                .map(|s| s.bindings.get(&sd.name).cloned().unwrap_or_default())
                .unwrap_or_default();
            for b in bound {
                self.destroy_entity(b.entity);
            }
        }
        // Step 3: the recipe's completion book first, then every interior
        // card's book — bound bindings in bind order, then unbound interior
        // storage in mint order, deduplicated by entity; an aspect-qualified
        // book fires when the completing recipe declares the qualifier or the
        // situation's interior aspect pool contains it (T12).
        if let Some(book) = rec.on_complete.clone() {
            let mut fctx = EffectCtx {
                sit: Some(sit),
                now: self.state.now,
                roll: None,
                handles: BTreeMap::new(),
                continued: None,
            };
            self.fire_book(&book, &mut fctx, Some(&recipe_id), ev, run)?;
        }
        if self.state.ended.is_none() {
            self.fire_card_books(sit, &rec, ev, run)?;
        }
        // The books stop at the first ending: an ending inside one book closes
        // the run and the situation falls through to its release.
        if self.state.ended.is_none() && !when_passed.is_empty() {
            // `continue via`: the books have run on the executing
            // settlement; the transition adopts the first pre-gated arm
            // whose `max` has room, and completes when none does
            // (spec.md §4.2).
            return self.continue_transition(sit, recipe_id, when_passed, ev, run);
        }
        // Steps 4-5: release, interior cleanup, removal, report.
        self.release_and_remove(sit);
        ev.push(Event::SituationCompleted {
            sit,
            recipe: recipe_id,
            bindings: final_bindings,
        });
        Ok(())
    }

    /// The card-book walk of a completion or continue step: every interior
    /// card, bound bindings first in bind order (a card bound in two slots
    /// walks once), then unbound interior storage in mint order, deduplicated
    /// by entity across both groups (kernel-api.md §8). An aspect-qualified
    /// book fires under either catalyst pass: the completing recipe's
    /// `declares` list, or the situation's interior aspect pool — the read
    /// sums over the interior, bound cards included — containing the
    /// qualifier. The books stop at the first ending.
    fn fire_card_books(
        &mut self,
        sit: SituationId,
        rec: &story_content::ir::RecipeDecl,
        ev: &mut Vec<Event>,
        run: &mut Run<'_>,
    ) -> Result<(), Fail> {
        let interior = match self.state.situations.get(&sit) {
            Some(s) => s.interior,
            None => return Ok(()),
        };
        let mut seen = BTreeSet::new();
        let mut walk: Vec<EntityId> = self
            .all_bindings(Some(sit))
            .into_iter()
            .filter(|e| seen.insert(*e))
            .collect();
        for e in self.zone_entities(interior) {
            if seen.insert(e) {
                walk.push(e);
            }
        }
        for e in walk {
            if self.state.ended.is_some() {
                break;
            }
            let (decl, own_keys) = {
                let Some(ent) = self.state.entities.get(&e) else {
                    continue;
                };
                // The stack's own declaration — element id and aspect keys,
                // mutations included — is where its candidate triggers are
                // read from.
                (self.template(&ent.prototype), ent.aspects.clone())
            };
            for (qual, book) in &decl.on_complete {
                if self.state.ended.is_some() {
                    break;
                }
                // Two conjuncts over the two catalyst passes: a recipe-declared
                // qualifier (pass 1) fires the book; the interior pool
                // (pass 2) fires it only on a stack whose own keys carry the
                // qualifier — a stack's candidate triggers are those declared
                // on its own elements.
                let fires = match qual {
                    None => true,
                    Some(k) => {
                        rec.declares.iter().any(|d| d == k)
                            || (own_keys.get(k).copied().unwrap_or(0) >= 1
                                && self.interior_aspect_total(interior, k) >= 1)
                    }
                };
                if !fires {
                    continue;
                }
                let mut fctx = EffectCtx {
                    sit: Some(sit),
                    now: self.state.now,
                    roll: None,
                    handles: BTreeMap::from([("subject".to_string(), vec![e])]),
                    continued: None,
                };
                self.fire_book(book, &mut fctx, Some(&rec.id), ev, run)?;
            }
        }
        Ok(())
    }

    /// The interior aspect pool: the read-value sum of one key over every
    /// card in the interior, bound cards included (spec.md §4.2).
    fn interior_aspect_total(&self, interior: ZoneId, key: &str) -> i64 {
        self.zone_entities(interior)
            .iter()
            .map(|e| self.read(*e, key))
            .sum()
    }

    /// The continue path: consuming settlement and the books have run; the
    /// situation adopts the target recipe instead of completing. The step
    /// charges the per-advance budget, and the native recursion of inline
    /// instant targets is depth-capped — a breach takes the Livelock rollback
    /// path (spec.md §4.2, kernel-api.md §6).
    fn continue_transition(
        &mut self,
        sit: SituationId,
        current: story_core::RecipeId,
        arms: Vec<story_core::RecipeId>,
        ev: &mut Vec<Event>,
        run: &mut Run<'_>,
    ) -> Result<(), Fail> {
        self.charge(run)?;
        if run.depth >= crate::MAX_RESOLUTION_DEPTH {
            self.state = run.entry.clone();
            return Err(Fail::new(
                DiagKind::Livelock,
                format!(
                    "inline continue resolution exceeds the depth cap of {}",
                    crate::MAX_RESOLUTION_DEPTH
                ),
            ));
        }
        run.depth += 1;
        let out = self.continue_transition_inner(sit, current, arms, ev, run);
        run.depth -= 1;
        out
    }

    fn continue_transition_inner(
        &mut self,
        sit: SituationId,
        current: story_core::RecipeId,
        arms: Vec<story_core::RecipeId>,
        ev: &mut Vec<Event>,
        run: &mut Run<'_>,
    ) -> Result<(), Fail> {
        // (1) The executing recipe's consuming slots burned at settlement.
        // (2) Every other binding is released into the interior as unbound
        // storage: the cards stay part of the situation — interior, unlocked,
        // never returned to `bound_from`, never re-bound.
        if let Some(s) = self.state.situations.get_mut(&sit) {
            s.bindings.clear();
        }
        // (3) The arms' `when`s were decided pre-book, from the executing
        // settlement's pool (spec.md §4.2); the walk here only consults
        // each arm's `max` at adoption. The first arm with room adopts; a
        // miss on every arm falls through to the normal completion
        // sequence.
        let content = self.content.clone();
        let mut adopted: Option<story_core::RecipeId> = None;
        for arm in &arms {
            let rec = content.recipe(arm.as_str()).unwrap_or_else(|| {
                crate::kernel_invariant("UnknownRecipe", format!("unknown recipe `{arm}`"))
            });
            if self.max_left(rec) {
                adopted = Some(rec.id.clone());
                break;
            }
        }
        let Some(target_id) = adopted else {
            let mut final_bindings: BTreeMap<story_core::SlotName, Vec<story_core::CardId>> =
                BTreeMap::new();
            if let Some(s) = self.state.situations.get(&sit) {
                for (slot, b_list) in &s.bindings {
                    let cards: Vec<story_core::CardId> = b_list
                        .iter()
                        .filter_map(|b| {
                            self.state
                                .entities
                                .get(&b.entity)
                                .map(|e| e.prototype.clone())
                        })
                        .collect();
                    final_bindings.insert(slot.clone(), cards);
                }
            }
            self.release_and_remove(sit);
            ev.push(Event::SituationCompleted {
                sit,
                recipe: current,
                bindings: final_bindings,
            });
            return Ok(());
        };
        let target = content.recipe(target_id.as_str()).unwrap_or_else(|| {
            crate::kernel_invariant("UnknownRecipe", format!("unknown recipe `{target_id}`"))
        });
        // (4) The adopted recipe materializes with no further gate: greedy
        // slots pull from the world per the standard greedy order (the
        // §4.4 top-up; the interior is not bindable, so released storage
        // is never re-bound); open slots re-open for mid-run `bind`.
        // Greedy is best-effort; no minimum is validated here — the
        // pre-pull walk was the gate.
        self.top_up_greedy(sit, target, ev);
        // (5) The situation adopts the target recipe — its verb included
        // (spec.md §4.2) — spends one `max` count (adoption materializes the
        // new recipe), and the timer re-arms.
        let now = self.state.now;
        {
            let s = self.state.situations.get_mut(&sit).expect("live sit");
            s.recipe = target.id.clone();
            s.verb = target.verb.clone();
            s.started_at = now;
            s.ends_at = now.saturating_add(target.duration);
        }
        self.state
            .recipe_runs
            .entry(target.id.clone())
            .and_modify(|n| *n += 1)
            .or_insert(1);
        ev.push(Event::Continued {
            sit,
            recipe: target.id.clone(),
        });
        if target.duration == 0 {
            // A 0-duration target resolves inline as usual, budget-charged.
            self.charge(run)?;
            self.completion_steps(sit, ev, run)?;
        }
        Ok(())
    }

    /// The kernel abort path: the unit's situation walks the recipe's
    /// completion book alone (card books belong to completion, not abortion),
    /// then the release path. The book is best-effort: a failing abort book
    /// rolls back and the release continues. Nothing authored invokes this.
    fn abort_situation(
        &mut self,
        sit: SituationId,
        f: &Fail,
        ev: &mut Vec<Event>,
        run: &mut Run<'_>,
    ) {
        ev.push(Event::ResolutionFailed {
            sit,
            reason: f.display_reason(),
        });
        if !self.state.situations.contains_key(&sit) {
            return;
        }
        let content = self.content.clone();
        let sit_recipe = &self.state.situations[&sit].recipe;
        let rec = content.recipe(sit_recipe.as_str()).unwrap_or_else(|| {
            crate::kernel_invariant("UnknownRecipe", format!("unknown recipe `{sit_recipe}`"))
        });
        if let Some(book) = rec.on_complete.clone() {
            let cp = self.state.clone();
            let mut fctx = EffectCtx {
                sit: Some(sit),
                now: self.state.now,
                roll: None,
                handles: BTreeMap::new(),
                continued: None,
            };
            if self
                .fire_book(&book, &mut fctx, Some(&rec.id), ev, run)
                .is_err()
            {
                self.state = cp;
            }
        }
        self.release_and_remove(sit);
    }
}
