//! The effect registry and the built-in interpreter (kernel-api.md §4,
//! spec.md §4.5). The authored vocabulary is closed; host Rust extends the
//! kernel by registering `Host` closures under op names.

use crate::eval::Ctx;
use crate::mint::Fail;
use crate::select::{Selection, Source};
use crate::settle::Run;
use crate::{Engine, Event, StartSkipReason};
use std::collections::BTreeMap;
use std::sync::Arc;
use story_content::ir::{EffectNode, Expr, Into as IntoZone};
use story_content::runtime::DiagKind;
use story_core::{EntityId, State};

/// The public effect seam: a host-registered operation applies onto the state
/// and returns events. Evaluation totality applies to host ops as well.
pub trait Effect: Send + Sync {
    fn apply(&self, state: &mut State, ctx: &mut EffectCtx) -> Vec<Event>;
}

impl<F> Effect for F
where
    F: Fn(&mut State, &mut EffectCtx) -> Vec<Event> + Send + Sync,
{
    fn apply(&self, state: &mut State, ctx: &mut EffectCtx) -> Vec<Event> {
        self(state, ctx)
    }
}

/// The per-resolution-unit effect context (kernel-api.md §4). `subject` for
/// card-completion and expiry books rides in `handles` under that name, which
/// is how the compiled IR addresses it.
pub struct EffectCtx {
    /// `None` in context-free books (expire, event) and commit selection.
    pub sit: Option<story_core::SituationId>,
    /// The stepped now of the running resolution.
    pub now: story_core::Time,
    /// Last-wins roll binding.
    pub roll: Option<i64>,
    /// Spawn-handle pools, mint-ordered; cleared when their effect list ends.
    pub handles: BTreeMap<String, Vec<EntityId>>,
}

impl EffectCtx {
    pub(crate) fn eval_ctx(&self, recipe: Option<&story_core::RecipeId>) -> Ctx {
        Ctx {
            sit: self.sit,
            sel_bindings: None,
            subject: self.handles.get("subject").and_then(|v| v.first()).copied(),
            roll: self.roll,
            recipe: recipe.cloned(),
        }
    }
}

/// One registry entry: precompiled nodes or a host closure, addressed by op
/// name through `EffectNode::Op`.
pub enum RegistryEntry {
    Compiled(Vec<EffectNode>),
    Host(Arc<dyn Effect>),
}

/// The op-name registry; `boot` verifies every referenced op is registered.
#[derive(Default)]
pub struct EffectRegistry {
    pub(crate) ops: BTreeMap<String, RegistryEntry>,
}

impl EffectRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn register(&mut self, name: impl Into<String>, entry: RegistryEntry) {
        self.ops.insert(name.into(), entry);
    }

    pub fn register_host(
        &mut self,
        name: impl Into<String>,
        f: impl Fn(&mut State, &mut EffectCtx) -> Vec<Event> + Send + Sync + 'static,
    ) {
        self.register(name, RegistryEntry::Host(Arc::new(f)));
    }

    pub(crate) fn contains_op(&self, name: &str) -> bool {
        self.ops.contains_key(name)
    }
}

impl Engine {
    /// Effect-time count validation (language.md §4.3): a count is a full
    /// integer expression and must evaluate to a non-negative value; a
    /// negative takes the effect-failure path (a constant negative is
    /// refused at compile time as [E246]).
    pub(crate) fn eval_count(&self, e: &Expr, ctx: &Ctx) -> Result<i64, Fail> {
        let n = self.eval_expr(e, ctx);
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
        run: &mut Run,
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
        run: &mut Run,
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
        run: &mut Run,
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
                let v = self.eval_expr(expr, &fctx.eval_ctx(recipe.as_ref()));
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
                let v = self.eval_expr(expr, &fctx.eval_ctx(recipe.as_ref()));
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
            EffectNode::Narrate { template } => {
                let text = self.interpolate(template, fctx, recipe.as_ref());
                ev.push(Event::Narration { text });
            }
            EffectNode::Roll {
                count,
                sides,
                bonus,
                bind: _,
            } => {
                let ctx = fctx.eval_ctx(recipe.as_ref());
                let count = self.eval_expr(count, &ctx);
                let sides = self.eval_expr(sides, &ctx);
                let mut total = self.eval_expr(bonus, &ctx);
                // Operands are validated at effect time; a violation is an
                // effect failure rolling the unit back (T11).
                if count < 0 || sides < 1 {
                    return Err(Fail::new(
                        story_content::runtime::DiagKind::EffectFailure,
                        format!("invalid roll operands: {count}d{sides}"),
                    ));
                }
                for _ in 0..count {
                    let die = (story_core::rng::splitmix64(self.seed ^ self.state.rng_counter)
                        % sides as u64) as i64
                        + 1;
                    self.state.rng_counter = self.state.rng_counter.wrapping_add(1);
                    total = total.saturating_add(die);
                }
                fctx.roll = Some(total);
            }
            EffectNode::Draw { deck, count, into } => {
                let n = self.eval_count(count, &fctx.eval_ctx(recipe.as_ref()))?;
                let into = into.clone().unwrap_or(IntoZone::Interior);
                for _ in 0..n {
                    let card = self.draw_card(deck)?;
                    self.mint(&card, &into, fctx, ev)?;
                }
            }
            EffectNode::Start { verb, via, count } => {
                let n = self.eval_count(count, &fctx.eval_ctx(recipe.as_ref()))?;
                self.apply_start(verb, via.as_ref(), n, fctx, ev, run)?;
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
                }
            }
            EffectNode::Branch { cond, then, els } => {
                let taken = if self.eval_cond(cond, &fctx.eval_ctx(recipe.as_ref())) {
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

    /// `start <verb> [via <recipe>] [count <n>]`: fan-out is best-effort —
    /// one branch's failure never rolls back its siblings or the parent
    /// resolution (spec.md §4.2).
    pub(crate) fn apply_start(
        &mut self,
        verb: &story_core::VerbId,
        via: Option<&story_core::RecipeId>,
        n: i64,
        fctx: &mut EffectCtx,
        ev: &mut Vec<Event>,
        run: &mut Run,
    ) -> Result<(), Fail> {
        let Some(verb_decl) = self.content.verbs.get(verb.as_str()).cloned() else {
            return Err(Fail::new(
                story_content::runtime::DiagKind::UnknownVerb,
                format!("unknown verb `{verb}`"),
            ));
        };
        for _ in 0..n {
            match self.select(Source::Start, &verb_decl, via, None, fctx.sit) {
                Selection::Nothing | Selection::GuardRefused(_) => {
                    // The chain ends silently: no eligible recipe, every
                    // `when` false, or the winner's guard refused.
                    ev.push(Event::ChainEnded { verb: verb.clone() });
                    return Ok(());
                }
                Selection::Matched(rec) => {
                    if self.verb_full(&verb_decl) {
                        ev.push(Event::StartSkipped {
                            verb: verb.clone(),
                            reason: StartSkipReason::InstanceCap,
                        });
                        return Ok(());
                    }
                    match self.materialize(
                        &verb_decl,
                        &rec,
                        story_core::SituationSource::Start {
                            parent: fctx.sit.unwrap_or_default(),
                        },
                        &[],
                        ev,
                    ) {
                        Err(()) => {
                            ev.push(Event::StartSkipped {
                                verb: verb.clone(),
                                reason: StartSkipReason::MaterializationFailed,
                            });
                            return Ok(());
                        }
                        Ok(sit) => {
                            if rec.duration == 0 {
                                // A zero-duration recipe resolves inline
                                // within the current resolution unit, each
                                // inline completion counting against the
                                // per-advance budget; a failure propagates to
                                // the enclosing unit (spec.md §4.2).
                                self.charge(run)?;
                                self.completion_steps(sit, ev, run)?;
                            }
                        }
                    }
                }
            }
        }
        Ok(())
    }

    /// `swap`: destroy + spawn atomized; the dying cards are excluded from
    /// the uniqueness check and each successor inherits its card's zone
    /// unless an `into` overrides (spec.md §4.5).
    fn apply_swap(
        &mut self,
        target: &story_content::ir::Target,
        count: &story_content::ir::Expr,
        to: &story_core::CardId,
        into: Option<&IntoZone>,
        fctx: &mut EffectCtx,
        recipe: Option<story_core::RecipeId>,
        ev: &mut Vec<Event>,
    ) -> Result<(), Fail> {
        let ctx = fctx.eval_ctx(recipe.as_ref());
        let ids = self.resolve_target(target, &ctx, &fctx.handles);
        let n = self.eval_count(count, &ctx)? as usize;
        let dying: Vec<story_core::EntityId> = ids.into_iter().take(n).collect();
        if dying.is_empty() {
            return Ok(());
        }
        let decl = self.template(to);
        // Uniqueness against the entities live at this instant, the dying
        // cards excluded (swapping oneself = retire and re-mint). Name-aware
        // like every mint: SingleCopy/Unique block on a live entity of that
        // name, Group on a live member, each minus the dying set (spec.md
        // §4.5).
        if self.mint_collision_except(&decl.mint, to, &dying) {
            match decl.mint {
                story_core::MintPolicy::SingleCopy => {
                    return Err(Fail::new(
                        story_content::runtime::DiagKind::NameInUse,
                        format!("a live `{to}` blocks the swap"),
                    ));
                }
                _ => {
                    ev.push(Event::SpawnSkipped {
                        name: to.to_string(),
                    });
                    return Ok(());
                }
            }
        }
        let zones: Vec<story_core::ZoneId> = dying
            .iter()
            .map(|d| match into {
                Some(i) => self.zone_of_into(i, fctx.sit),
                None => Ok(self
                    .state
                    .entities
                    .get(d)
                    .map(|e| e.zone)
                    .unwrap_or(self.home)),
            })
            .collect::<Result<_, _>>()?;
        for d in &dying {
            self.destroy_entity(*d);
        }
        for zone in zones {
            self.mint_body(&decl, to, zone, fctx, ev)?;
        }
        Ok(())
    }

    /// Draw one card from a deck: the compile-time permutation plus the state
    /// cursor; exhaustion walks fallback -> wrap -> the failure (spec.md §5).
    fn draw_card(&mut self, deck: &story_core::DeckId) -> Result<story_core::CardId, Fail> {
        let Some(decl) = self.content.decks.get(deck.as_str()) else {
            return Err(Fail::new(
                story_content::runtime::DiagKind::UnknownDeck,
                format!("unknown deck `{deck}`"),
            ));
        };
        let cursor = self.state.decks.get(deck).copied().unwrap_or(0);
        let perm = self.deck_orders.get(deck).cloned().unwrap_or_default();
        let card = if let Some(c) = perm.get(cursor as usize) {
            c.clone()
        } else if let Some(f) = &decl.fallback {
            f.clone()
        } else if decl.wrap && !perm.is_empty() {
            perm[(cursor % perm.len() as u64) as usize].clone()
        } else {
            return Err(Fail::new(
                story_content::runtime::DiagKind::DeckExhausted,
                format!("deck `{deck}` is exhausted and declares no relief path"),
            ));
        };
        self.state.decks.insert(deck.clone(), cursor + 1);
        Ok(card)
    }

    /// Walk a trigger book under the selection law: the first unspent rule
    /// whose `when` holds fires (spec.md §4.6).
    pub(crate) fn fire_book(
        &mut self,
        book: &story_content::ir::Book,
        fctx: &mut EffectCtx,
        recipe: Option<&story_core::RecipeId>,
        ev: &mut Vec<Event>,
        run: &mut Run,
    ) -> Result<(), Fail> {
        let mut rules: Vec<&story_content::ir::Rule> = book
            .rules
            .iter()
            .filter(|r| !self.state.fired_once.contains(&r.id))
            .collect();
        rules.sort_by_key(|r| (r.priority, r.decl_index));
        for r in rules {
            if let Some(w) = &r.when {
                if !self.eval_cond(w, &fctx.eval_ctx(recipe)) {
                    continue;
                }
            }
            if r.once {
                self.state.fired_once.insert(r.id.clone());
            }
            return self.run_list(&r.effects, fctx, recipe.cloned(), ev, run, true);
        }
        Ok(())
    }

    /// Prose token interpolation (language.md §7): slots, unique-card name
    /// paths, roll bindings, spawn handles, `{legacy.<key>}`, and `subject`.
    pub(crate) fn interpolate(
        &self,
        template: &str,
        fctx: &EffectCtx,
        recipe: Option<&story_core::RecipeId>,
    ) -> String {
        let chars: Vec<char> = template.chars().collect();
        let mut out = String::new();
        let mut i = 0;
        while i < chars.len() {
            if chars[i] == '{' {
                if let Some(close) = chars[i..].iter().position(|c| *c == '}') {
                    let tok: String = chars[i + 1..i + close].iter().collect();
                    out.push_str(&self.render_token(&tok, fctx, recipe));
                    i += close + 1;
                    continue;
                }
            }
            out.push(chars[i]);
            i += 1;
        }
        out
    }

    fn render_token(
        &self,
        tok: &str,
        fctx: &EffectCtx,
        recipe: Option<&story_core::RecipeId>,
    ) -> String {
        let ctx = fctx.eval_ctx(recipe);
        if let Some(key) = tok.strip_prefix("legacy.") {
            return self.state.legacy.get(key).copied().unwrap_or(0).to_string();
        }
        if let Some((head, key)) = tok.split_once('.') {
            if head == "subject" {
                return match ctx.subject {
                    Some(s) => self.read(s, key).to_string(),
                    None => "0".to_string(),
                };
            }
            if let Some(pool) = fctx.handles.get(head) {
                return match pool.first() {
                    Some(&oldest) => self.read(oldest, key).to_string(),
                    None => "0".to_string(),
                };
            }
            return match self.named_entity(head) {
                Some(e) => self.read(e, key).to_string(),
                None => "0".to_string(),
            };
        }
        // A slot name renders its bound cards' names; a handle renders its
        // pool; a roll binding renders the last result; `subject` the card.
        let slot_cards = self.slot_bindings(ctx.sit, None, &story_core::SlotName::from(tok));
        if !slot_cards.is_empty() {
            return self.render_names(&slot_cards);
        }
        if let Some(pool) = fctx.handles.get(tok) {
            return self.render_names(pool);
        }
        if let Some(roll) = fctx.roll {
            return roll.to_string();
        }
        if let Some(s) = ctx.subject {
            return self
                .state
                .entities
                .get(&s)
                .map(|e| e.name.clone())
                .unwrap_or_default();
        }
        String::new()
    }

    fn render_names(&self, ids: &[EntityId]) -> String {
        ids.iter()
            .filter_map(|id| self.state.entities.get(id))
            .map(|e| e.name.clone())
            .collect::<Vec<_>>()
            .join(", ")
    }
}
