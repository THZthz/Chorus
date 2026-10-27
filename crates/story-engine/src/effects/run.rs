//! The effect-list interpreter: per-`EffectNode` application, list running,
//! trigger-book walking, and count validation.

use super::{EffectCtx, RegistryEntry};
use crate::eval::Ctx;
use crate::mint::Fail;
use crate::settle::Run;
use crate::{Engine, Event};
use std::collections::BTreeMap;
use story_content::ir::{EffectNode, Expr, Into as IntoZone};
use story_content::runtime::DiagKind;

impl Engine {
    /// Effect-time count validation (language.md §4.3): a count is a full
    /// integer expression and must evaluate to a non-negative value; a
    /// negative takes the effect-failure path (a constant negative is
    /// refused at compile time as [E246]).
    pub(crate) fn eval_count(&self, e: &Expr, ctx: &Ctx) -> Result<i64, Fail> {
        let n = self.eval_expr_ctx(e, ctx);
        if n < 0 {
            return Err(Fail::new(
                DiagKind::EffectFailure,
                format!("count must be non-negative; the expression evaluated to {n}"),
            ));
        }
        Ok(n)
    }

    /// Run one effect list. `clear_handles` marks an owning list (a recipe's
    /// effects, a rule's effects): its handle pools are cleared at the end.
    /// Branch bodies are continuations of the enclosing list and share the
    /// pools (language.md §4.3). The native recursion this descends into
    /// (inline instant chains, signal cycles) is depth-capped; a breach takes
    /// the Livelock rollback path instead of exhausting the stack.
    pub(crate) fn run_list(
        &mut self,
        nodes: &[EffectNode],
        fctx: &mut EffectCtx,
        recipe: Option<story_core::RecipeId>,
        ev: &mut Vec<Event>,
        run: &mut Run<'_>,
        clear_handles: bool,
    ) -> Result<(), Fail> {
        if run.depth >= crate::MAX_RESOLUTION_DEPTH {
            self.state = run.entry.clone();
            return Err(Fail::new(
                DiagKind::Livelock,
                format!(
                    "resolution nesting exceeds the depth cap of {}",
                    crate::MAX_RESOLUTION_DEPTH
                ),
            ));
        }
        run.depth += 1;
        let out = self.run_list_inner(nodes, fctx, recipe, ev, run, clear_handles);
        run.depth -= 1;
        out
    }

    fn run_list_inner(
        &mut self,
        nodes: &[EffectNode],
        fctx: &mut EffectCtx,
        recipe: Option<story_core::RecipeId>,
        ev: &mut Vec<Event>,
        run: &mut Run<'_>,
        clear_handles: bool,
    ) -> Result<(), Fail> {
        for node in nodes {
            self.run_effect(node, fctx, recipe.clone(), ev, run)?;
        }
        if clear_handles {
            fctx.handles.clear();
        }
        Ok(())
    }

    fn run_effect(
        &mut self,
        node: &EffectNode,
        fctx: &mut EffectCtx,
        recipe: Option<story_core::RecipeId>,
        ev: &mut Vec<Event>,
        run: &mut Run<'_>,
    ) -> Result<(), Fail> {
        match node {
            EffectNode::Spawn {
                prototype,
                count,
                into,
                handle,
            } => {
                let n = self.eval_count(count, &fctx.eval_ctx(recipe.as_ref()))?;
                for _ in 0..n {
                    if let Some(id) = self.mint(prototype, into, fctx, ev)? {
                        if let Some(h) = handle {
                            fctx.handles.entry(h.clone()).or_default().push(id);
                        }
                    }
                }
            }
            EffectNode::Destroy { target, count } => {
                let ids =
                    self.resolve_target(target, &fctx.eval_ctx(recipe.as_ref()), &fctx.handles);
                let n = match count {
                    story_content::ir::DestroyCount::All => ids.len(),
                    story_content::ir::DestroyCount::AtMost(e) => {
                        self.eval_count(e, &fctx.eval_ctx(recipe.as_ref()))? as usize
                    }
                };
                for id in ids.into_iter().take(n) {
                    self.destroy_entity(id);
                }
            }
            EffectNode::Consume { target, count } => {
                let ids =
                    self.resolve_target(target, &fctx.eval_ctx(recipe.as_ref()), &fctx.handles);
                let n = self.eval_count(count, &fctx.eval_ctx(recipe.as_ref()))? as usize;
                if ids.len() < n {
                    return Err(Fail::new(
                        story_content::runtime::DiagKind::EffectFailure,
                        format!(
                            "consume shortage: target holds {}, {} requested",
                            ids.len(),
                            n
                        ),
                    ));
                }
                for id in ids.into_iter().take(n) {
                    self.destroy_entity(id);
                }
            }
            EffectNode::Swap {
                target,
                count,
                to,
                into,
            } => {
                self.apply_swap(target, count, to, into.as_ref(), fctx, recipe, ev)?;
            }
            EffectNode::Set {
                target,
                key,
                op,
                expr,
                unclamped,
            } => {
                let ids =
                    self.resolve_target(target, &fctx.eval_ctx(recipe.as_ref()), &fctx.handles);
                let v = self.eval_expr_ctx(expr, &fctx.eval_ctx(recipe.as_ref()));
                for id in ids {
                    let cur = self.read(id, key);
                    let mut nv = match op {
                        story_content::ir::SetOp::Assign => v,
                        story_content::ir::SetOp::Add => cur.saturating_add(v),
                        story_content::ir::SetOp::Sub => cur.saturating_sub(v),
                    };
                    if !*unclamped && nv < 0 {
                        nv = 0;
                    }
                    if let Some(ent) = self.state.entities.get_mut(&id) {
                        ent.aspects.insert(key.clone(), nv);
                    }
                }
            }
            EffectNode::Move { target, into } => {
                let ids =
                    self.resolve_target(target, &fctx.eval_ctx(recipe.as_ref()), &fctx.handles);
                for id in ids {
                    let zone = self.zone_of_into(into, fctx.sit)?;
                    if let Some(ent) = self.state.entities.get_mut(&id) {
                        ent.zone = zone;
                    }
                }
            }
            EffectNode::Legacy { key, op, expr } => {
                let v = self.eval_expr_ctx(expr, &fctx.eval_ctx(recipe.as_ref()));
                let cur = self.state.legacy.get(key).copied().unwrap_or(0);
                let mut nv = match op {
                    story_content::ir::SetOp::Assign => v,
                    story_content::ir::SetOp::Add => cur.saturating_add(v),
                    story_content::ir::SetOp::Sub => cur.saturating_sub(v),
                };
                if nv < 0 {
                    nv = 0;
                }
                self.state.legacy.insert(key.clone(), nv);
            }
            EffectNode::Cue { id } => {
                ev.push(Event::NarrativeCue { cue_id: id.clone() });
            }
            EffectNode::Roll {
                count,
                sides,
                bonus,
                bind,
            } => {
                let ctx = fctx.eval_ctx(recipe.as_ref());
                let count = self.eval_expr_ctx(count, &ctx);
                let sides = self.eval_expr_ctx(sides, &ctx);
                let mut total = self.eval_expr_ctx(bonus, &ctx);
                // Operands are validated at effect time; a violation is an
                // effect failure rolling the unit back (T11).
                if count < 0 || sides < 1 {
                    return Err(Fail::new(
                        story_content::runtime::DiagKind::EffectFailure,
                        format!("invalid roll operands: {count}d{sides}"),
                    ));
                }
                // The fork hook drives the node when it answers (story-analyze
                // spec §2.2): a chance point fires for count >= 1, sides >= 1;
                // count 0 applies the bonus only. The supplied value is the
                // full bound result, bonus included, and `rng_counter`
                // advances by `count` uniformly — exactly the deterministic
                // path's charge — so a recorded outcome reproduces the
                // unforked state bit-for-bit.
                let mut chosen = None;
                if count >= 1 {
                    if let Some(hook) = run.fork.as_mut() {
                        let point = crate::ChancePoint::Roll {
                            count: u32::try_from(count).unwrap_or(u32::MAX),
                            sides: u32::try_from(sides).unwrap_or(u32::MAX),
                            bonus: total,
                            bind: bind.clone(),
                        };
                        chosen = hook(&point);
                    }
                }
                match chosen {
                    Some(crate::ChanceOutcome::Roll(v)) => {
                        self.state.rng_counter = self.state.rng_counter.wrapping_add(count as u64);
                        fctx.roll = Some(v);
                    }
                    _ => {
                        for _ in 0..count {
                            let die =
                                (story_core::rng::splitmix64(self.seed ^ self.state.rng_counter)
                                    % sides as u64) as i64
                                    + 1;
                            self.state.rng_counter = self.state.rng_counter.wrapping_add(1);
                            total = total.saturating_add(die);
                        }
                        fctx.roll = Some(total);
                    }
                }
            }
            EffectNode::Draw {
                deck,
                count,
                into: into_opt,
            } => {
                let n = self.eval_count(count, &fctx.eval_ctx(recipe.as_ref()))?;
                let into = into_opt.clone().unwrap_or(IntoZone::Interior);
                // A chance point fires when the requested count is served
                // entirely from the permutation; a draw beyond the remaining
                // permutation falls through to the kernel's fallback/wrap
                // path unchanged (story-analyze spec §2.2). The supplied cards
                // mint in order and the cursor still advances by the
                // requested count uniformly across branches, so `decks` stays
                // canonical-key material.
                let mut chosen = None;
                if n >= 1 {
                    let known = self.content.decks.contains_key(deck.as_str());
                    let cursor = self.state.decks.get(deck).copied().unwrap_or(0);
                    let remaining = self
                        .deck_orders
                        .get(deck)
                        .map(|p| p.len() as u64)
                        .unwrap_or(0);
                    if known && cursor.wrapping_add(n as u64) <= remaining {
                        if let Some(hook) = run.fork.as_mut() {
                            let point = crate::ChancePoint::Draw {
                                deck: deck.clone(),
                                count: u32::try_from(n).unwrap_or(u32::MAX),
                                into: into_opt.clone(),
                            };
                            chosen = hook(&point);
                        }
                    }
                }
                match chosen {
                    Some(crate::ChanceOutcome::Draw(cards)) => {
                        for card in &cards {
                            self.mint(card, &into, fctx, ev)?;
                        }
                        let cursor = self.state.decks.get(deck).copied().unwrap_or(0);
                        self.state
                            .decks
                            .insert(deck.clone(), cursor.wrapping_add(n as u64));
                    }
                    _ => {
                        for _ in 0..n {
                            let card = self.draw_card(deck)?;
                            self.mint(&card, &into, fctx, ev)?;
                        }
                    }
                }
            }
            EffectNode::Start { verb, via, count } => {
                let n = self.eval_count(count, &fctx.eval_ctx(recipe.as_ref()))?;
                self.apply_start(verb, via.as_ref(), n, fctx, ev, run)?;
            }
            EffectNode::Continue { recipe } => {
                // Recorded, not walked: the completion path turns the
                // executed `continue` into the adoption transition of
                // spec.md §4.2 after the books phase. Branch semantics are
                // ordinary — a `continue` inside an untaken branch never
                // records.
                fctx.continued = Some(recipe.clone());
            }
            EffectNode::Relate { from, to, kind } => {
                self.apply_relate(from, to, kind, fctx, recipe, ev);
            }
            EffectNode::Unrelate { from, to, kind } => {
                self.apply_unrelate(from, to, kind.as_deref(), fctx, recipe, ev);
            }
            EffectNode::Signal { event } => {
                let Some(book) = self.content.events.get(event).cloned() else {
                    return Err(Fail::new(
                        story_content::runtime::DiagKind::UndeclaredEvent,
                        format!("no `on event` book declares `{event}`"),
                    ));
                };
                // Event books are context-free: no situation, no subject, no
                // roll of the signaling list (kernel-api.md §8). The
                // signaling list's own context is untouched.
                let mut fresh = EffectCtx {
                    sit: None,
                    now: fctx.now,
                    roll: None,
                    handles: BTreeMap::new(),
                    continued: None,
                };
                self.fire_book(&book, &mut fresh, None, ev, run)?;
            }
            EffectNode::Ending { id } => {
                // The first ending wins; further endings in the same advance
                // are no-ops. The remaining due queue is discarded by the
                // settlement loop (T7).
                if self.state.ended.is_none() {
                    if !self.content.endings.contains_key(id.as_str()) {
                        return Err(Fail::new(
                            story_content::runtime::DiagKind::UnknownEnding,
                            format!("unknown ending `{id}`"),
                        ));
                    }
                    self.state.ended = Some(id.clone());
                    ev.push(Event::Ended { ending: id.clone() });
                }
            }
            EffectNode::Branch { cond, then, els } => {
                let taken = if self.eval_cond_ctx(cond, &fctx.eval_ctx(recipe.as_ref())) {
                    then
                } else {
                    els
                };
                self.run_list(taken, fctx, recipe, ev, run, false)?;
            }
            EffectNode::Op { name, args: _ } => {
                let entry = self.registry.ops.get(name);
                match entry {
                    Some(RegistryEntry::Host(f)) => {
                        let events = f.apply(&mut self.state, fctx);
                        ev.extend(events);
                    }
                    Some(RegistryEntry::Compiled(nodes)) => {
                        let nodes = nodes.clone();
                        self.run_list(&nodes, fctx, recipe, ev, run, false)?;
                    }
                    None => {
                        return Err(Fail::new(
                            story_content::runtime::DiagKind::UnregisteredOp,
                            format!("op `{name}` is not registered"),
                        ));
                    }
                }
            }
        }
        Ok(())
    }

    /// Walk a trigger book under the selection law: the first unspent rule
    /// whose `when` holds fires (spec.md §4.6).
    pub(crate) fn fire_book(
        &mut self,
        book: &story_content::ir::Book,
        fctx: &mut EffectCtx,
        recipe: Option<&story_core::RecipeId>,
        ev: &mut Vec<Event>,
        run: &mut Run<'_>,
    ) -> Result<(), Fail> {
        let mut rules: Vec<&story_content::ir::Rule> = book
            .rules
            .iter()
            .filter(|r| !self.state.fired_once.contains(&r.id))
            .collect();
        rules.sort_by_key(|r| (r.priority, r.decl_index));
        for r in rules {
            if let Some(w) = &r.when {
                if !self.eval_cond_ctx(w, &fctx.eval_ctx(recipe)) {
                    continue;
                }
            }
            if r.once {
                self.state.fired_once.insert(r.id.clone());
            }
            ev.push(Event::RuleFired {
                rule_id: r.id.to_string(),
            });
            return self.run_list(&r.effects, fctx, recipe.cloned(), ev, run, true);
        }
        Ok(())
    }
}
