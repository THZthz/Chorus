//! Boot, the settlement loop, and the completion sequence (kernel-api.md §6,
//! spec.md §4.2, §4.6).

use crate::effects::EffectCtx;
use crate::mint::Fail;
use crate::select::{Selection, Source};
use crate::{bind_count, DiagExt, Engine, Event, StartSkipReason};
use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;
use story_content::ir::{Book, EffectNode, SeedSpec, VerbMode};
use story_content::runtime::{DiagKind, RuntimeDiagnostic};
use story_content::Content;
use story_core::rng::splitmix64;
use story_core::{
    Binding, CardId, DeckId, EntityId, SituationId, SituationSource, State, Time, VerbId,
    Zone, ZoneId, ZoneKind,
};

/// The budget guard of one engine call: the entry checkpoint for the only
/// whole-call rollback (Livelock), the resolution counter, and the native
/// recursion depth of inline resolution (instant start chains, signal
/// cycles), capped well below the stack limit.
pub(crate) struct Run {
    pub(crate) entry: State,
    pub(crate) units: u64,
    pub(crate) depth: u32,
}

impl Run {
    pub(crate) fn new(entry: State) -> Self {
        Run {
            entry,
            units: 0,
            depth: 0,
        }
    }
}

/// One due item of the derived queue: a situation completion (phase 0) or an
/// unbound entity's expiry (phase 1); autonomous checkpoints (phase 2) run
/// after the 0/1 phases drain at each settled moment.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Due {
    Completion(SituationId, Time),
    Expiry(EntityId, Time),
}

impl Due {
    fn moment(&self) -> Time {
        match *self {
            Due::Completion(_, m) | Due::Expiry(_, m) => m,
        }
    }

    fn phase(&self) -> u8 {
        match *self {
            Due::Completion(..) => 0,
            Due::Expiry(..) => 1,
        }
    }

    fn id(&self) -> u64 {
        match *self {
            Due::Completion(s, _) => s.0,
            Due::Expiry(e, _) => e.0,
        }
    }
}

// ---------------------------------------------------------------------------
// Boot (kernel-api.md §1, spec.md §4.8)
// ---------------------------------------------------------------------------

pub(crate) fn boot(
    content: Arc<Content>,
    registry: crate::EffectRegistry,
    seed: u64,
    legacy_in: Option<BTreeMap<String, i64>>,
) -> (Engine, Vec<RuntimeDiagnostic>) {
    let mut diags: Vec<RuntimeDiagnostic> = Vec::new();
    let (turn, home_name) = content
        .game
        .as_ref()
        .map(|g| (g.turn, g.home.clone()))
        .unwrap_or((1, String::new()));

    // Seed handling: a content-fixed seed must match the boot argument.
    if let Some(SeedSpec::Fixed(n)) = content.game.as_ref().map(|g| g.seed.clone()) {
        if n != seed {
            diags.push(DiagKind::SeedMismatch.in_context(
                "boot",
                format!("content fixes seed {n} but boot received {seed}"),
            ));
        }
    }

    // Every referenced host op must be registered (`UnregisteredOp`).
    let mut ops = BTreeSet::new();
    collect_ops(&content, &mut ops);
    for op in &ops {
        if !registry.contains_op(op) {
            diags.push(DiagKind::UnregisteredOp.in_context(
                "boot",
                format!("content references op `{op}` which no host registered"),
            ));
        }
    }

    let mut state = State::default();
    state.next_entity = 1;
    state.next_situation = 1;
    state.next_zone = 1;
    state.rng_counter = 0;

    // Declared zones, in declaration order (Table: public, bindable).
    let mut zone_ids: BTreeMap<String, ZoneId> = BTreeMap::new();
    let mut zone_order: BTreeMap<ZoneId, u32> = BTreeMap::new();
    let mut zone_parent: BTreeMap<ZoneId, Option<ZoneId>> = BTreeMap::new();
    let mut zones_sorted: Vec<&story_content::ir::ZoneDecl> = content.zones.values().collect();
    zones_sorted.sort_by_key(|z| z.decl_index);
    for z in &zones_sorted {
        let id = ZoneId(state.next_zone);
        state.next_zone += 1;
        state.zones.insert(
            id,
            Zone {
                id,
                kind: ZoneKind::Table,
                public: true,
                bindable: true,
            },
        );
        zone_ids.insert(z.name.clone(), id);
        zone_order.insert(id, z.decl_index);
        zone_parent.insert(id, None);
    }
    for z in content.zones.values() {
        if let Some(p) = &z.parent {
            if let (Some(&child), Some(&parent)) = (zone_ids.get(&z.name), zone_ids.get(p)) {
                zone_parent.insert(child, Some(parent));
            }
        }
    }
    let home = zone_ids.get(&home_name).copied().unwrap_or_else(|| {
        // The compiler validates `home`; the fallback keeps boot total.
        zone_ids.values().copied().next().unwrap_or(ZoneId(0))
    });

    // Legacy: declared defaults, then the host's incoming values folded over.
    for l in content.legacies.values() {
        state.legacy.insert(l.key.clone(), l.default);
    }
    if let Some(incoming) = legacy_in {
        for (k, v) in incoming {
            if !state.legacy.contains_key(&k) {
                diags.push(DiagKind::UnresolvableRef.warning(format!(
                    "host legacy key `{k}` is not declared in content; folded as {v}"
                )));
            }
            state.legacy.insert(k, v);
        }
    }

    // Deck cursors and the seeded permutations. The seed participates in the
    // permutation setup; the run RNG counter is untouched by it.
    let mut deck_orders: BTreeMap<DeckId, Vec<CardId>> = BTreeMap::new();
    for (name, decl) in &content.decks {
        state.decks.insert(decl.id.clone(), 0);
        let mut perm: Vec<CardId> = decl.entries.clone();
        let mut ctr = seed ^ story_core::fingerprint::fingerprint(name);
        for i in (1..perm.len()).rev() {
            ctr = ctr.wrapping_add(0x9e37_79b9_7f4a_7c15);
            let j = (splitmix64(ctr) % (i as u64 + 1)) as usize;
            perm.swap(i, j);
        }
        deck_orders.insert(decl.id.clone(), perm);
        if decl.fallback.is_none() && !decl.wrap {
            diags.push(DiagKind::DeckExhausted.warning(format!(
                "deck `{name}` declares neither fallback nor wrap; a draw past its entries will fail its resolution"
            )));
        }
    }

    let mut engine = Engine {
        registry,
        state,
        seed,
        zone_ids,
        zone_order,
        zone_parent,
        home,
        turn,
        deck_orders,
        content,
    };

    // Arm boot instances from the cards' own declarations (mint aliasing
    // never applies at boot), lifetimes armed at t=0. Aspects, kind, and the
    // uniqueness policy all read the own declaration; the `mint <alias>` body
    // applies only to mints (spec.md §4.5).
    let mut card_ids: Vec<(u32, story_core::CardId, story_content::ir::BootFace)> = Vec::new();
    for decl in engine.content.cards.values() {
        if let Some(face) = decl.boot.clone() {
            card_ids.push((decl.decl_index, decl.id.clone(), face));
        }
    }
    card_ids.sort_by_key(|(idx, _, _)| *idx);
    let fctx = EffectCtx {
        sit: None,
        now: 0,
        roll: None,
        handles: BTreeMap::new(),
        continued: None,
    };
    let mut ev = Vec::new();
    for (_, id, face) in card_ids {
        let Some(&z) = engine.zone_ids.get(&face.zone) else {
            continue;
        };
        let Some(own) = engine.content.card(&id).cloned() else {
            continue;
        };
        for _ in 0..face.count {
            if engine.mint_collision(&own.mint, &id) {
                diags.push(DiagKind::NameInUse.warning(format!(
                    "boot instance of `{id}` skipped: a live copy blocks the mint"
                )));
                continue;
            }
            let _ = engine.mint_body(&own, &id, z, &fctx, &mut ev);
        }
    }

    // One autonomous checkpoint at t=0; its events are dropped by design of
    // the boot signature (opens are silent).
    let mut run = Run::new(engine.state.clone());
    let _ = checkpoint(&mut engine, &mut ev, &mut run);

    (engine, diags)
}

fn collect_ops(content: &Content, ops: &mut BTreeSet<String>) {
    fn walk_book(b: &Book, ops: &mut BTreeSet<String>) {
        for r in &b.rules {
            walk_effects(&r.effects, ops);
        }
    }
    fn walk_effects(e: &[story_content::ir::EffectNode], ops: &mut BTreeSet<String>) {
        for n in e {
            walk_node(n, ops);
        }
    }
    fn walk_node(n: &story_content::ir::EffectNode, ops: &mut BTreeSet<String>) {
        match n {
            story_content::ir::EffectNode::Op { name, .. } => {
                ops.insert(name.clone());
            }
            story_content::ir::EffectNode::Branch { then, els, .. } => {
                walk_effects(then, ops);
                walk_effects(els, ops);
            }
            _ => {}
        }
    }
    for v in content.verbs.values() {
        for r in v.recipes.values() {
            walk_effects(&r.effects, ops);
            if let Some(b) = &r.on_complete {
                walk_book(b, ops);
            }
        }
    }
    for c in content.cards.values() {
        for (_, b) in &c.on_complete {
            walk_book(b, ops);
        }
        if let Some(b) = &c.on_expire {
            walk_book(b, ops);
        }
    }
    for b in content.events.values() {
        walk_book(b, ops);
    }
}

// ---------------------------------------------------------------------------
// The settlement loop (kernel-api.md §6)
// ---------------------------------------------------------------------------

pub(crate) fn advance(engine: &mut Engine, dt: Time) -> Result<Vec<Event>, RuntimeDiagnostic> {
    let Some(target) = engine.state.now.checked_add(dt) else {
        return Err(DiagKind::BadDuration.refused(format!(
            "advance to now {} + {dt} overflows the clock",
            engine.state.now
        )));
    };
    let mut run = Run::new(engine.state.clone());
    let mut ev: Vec<Event> = Vec::new();
    // Each distinct settled moment polls once, after its 0/1 phases finish;
    // the end of the advance polls once (deduplicated against it).
    let mut polled: BTreeSet<Time> = BTreeSet::new();
    loop {
        // Drain the due queue in (moment, phase, id) order; when the next
        // item lies past the moment just settled, that moment's 0/1 phases
        // are finished and it polls — then the queue is re-taken, because
        // the checkpoint may open instances at moments in between.
        let mut last: Option<Time> = None;
        while let Some(item) = next_due(engine, target) {
            let m = item.moment();
            if let Some(pm) = last {
                if m > pm && !polled.contains(&pm) {
                    polled.insert(pm);
                    step_now(engine, pm);
                    checkpoint(engine, &mut ev, &mut run)?;
                    if engine.state.ended.is_some() {
                        return Ok(ev);
                    }
                    continue;
                }
            }
            last = Some(m);
            step_now(engine, m);
            match item {
                Due::Completion(sit, _) => {
                    engine.charge(&mut run).map_err(Fail::into_diagnostic)?;
                    engine
                        .settle_completion(sit, &mut ev, &mut run)
                        .map_err(Fail::into_diagnostic)?;
                }
                Due::Expiry(e, _) => {
                    engine.charge(&mut run).map_err(Fail::into_diagnostic)?;
                    settle_expiry(engine, e, &mut ev, &mut run).map_err(Fail::into_diagnostic)?;
                }
            }
            if engine.state.ended.is_some() {
                // The ending closed the run at this instant: the remaining
                // queue is discarded and the clock freezes here (T7).
                return Ok(ev);
            }
        }
        // Drained: the last settled moment polls now (it may open instances
        // that are due again before `target`), so re-scan.
        if let Some(pm) = last {
            if !polled.contains(&pm) {
                polled.insert(pm);
                step_now(engine, pm);
                checkpoint(engine, &mut ev, &mut run)?;
                if engine.state.ended.is_some() {
                    return Ok(ev);
                }
                continue;
            }
        }
        // The end-of-advance checkpoint runs once the drain is complete; a
        // freed past-due card or an instant pulse can re-arm the queue, so
        // re-scan afterwards too.
        if !polled.contains(&target) {
            step_now(engine, target);
            polled.insert(target);
            checkpoint(engine, &mut ev, &mut run)?;
            if engine.state.ended.is_some() {
                return Ok(ev);
            }
            continue;
        }
        break;
    }
    step_now(engine, target);
    Ok(ev)
}

fn step_now(engine: &mut Engine, m: Time) {
    engine.state.now = engine.state.now.max(m);
}

/// The least due item at or before `target` under the total-order key
/// `(moment, phase, id)`. Bound (locked) cards are not due: their expiry is
/// deferred while bound.
fn next_due(engine: &Engine, target: Time) -> Option<Due> {
    let completion = engine
        .state
        .situations
        .values()
        .filter(|s| s.ends_at <= target)
        .map(|s| Due::Completion(s.id, s.ends_at))
        .min_by_key(|d| (d.moment(), d.phase(), d.id()));
    let expiry = engine
        .state
        .entities
        .values()
        .filter_map(|e| {
            let t = e.expires_at?;
            if t > target || engine.is_locked(e.id) {
                None
            } else {
                Some(Due::Expiry(e.id, t))
            }
        })
        .min_by_key(|d| (d.moment(), d.phase(), d.id()));
    match (completion, expiry) {
        (Some(c), Some(e)) => {
            if (c.moment(), c.phase(), c.id()) <= (e.moment(), e.phase(), e.id()) {
                Some(c)
            } else {
                Some(e)
            }
        }
        (Some(c), None) => Some(c),
        (None, Some(e)) => Some(e),
        (None, None) => None,
    }
}

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
        run: &mut Run,
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
        run: &mut Run,
    ) -> Result<(), Fail> {
        let Some(s) = self.state.situations.get(&sit) else {
            return Ok(());
        };
        let recipe_id = s.recipe.clone();
        let rec = self.recipe_of(&recipe_id);
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
        if self.state.ended.is_none() {
            // `continue via`: the recipe's execution has run its effects,
            // consuming settlement, and books — a `continue` the list
            // executed (branch semantics are ordinary) picks the walk's
            // starting arm, and the walk re-checks the arms — a max-exhausted
            // target missing like a failed `when` — in emission order against
            // the post-book interior (the linked hand-off order). The first
            // arm that passes adopts; a miss on every arm completes
            // (spec.md §4.2).
            if let Some(executed) = fctx.continued.clone() {
                let arms = continue_arms(&rec.effects);
                let mut arms: Vec<story_core::RecipeId> = arms
                    .into_iter()
                    .filter_map(|n| match n {
                        EffectNode::Continue { recipe } => Some(recipe),
                        _ => None,
                    })
                    .collect();
                if !arms.is_empty() {
                    let start = arms
                        .iter()
                        .position(|a| *a == executed)
                        .unwrap_or(0);
                    let remaining: Vec<story_core::RecipeId> =
                        arms.drain(start..).collect();
                    return self
                        .continue_transition(sit, recipe_id, remaining, ev, run);
                }
            }
        }
        // Steps 4-5: release, interior cleanup, removal, report.
        self.release_and_remove(sit);
        ev.push(Event::SituationCompleted {
            sit,
            recipe: recipe_id,
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
        run: &mut Run,
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
        run: &mut Run,
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
        run: &mut Run,
    ) -> Result<(), Fail> {
        let interior = self.state.situations[&sit].interior;
        // (1) The executing recipe's consuming slots burned at settlement.
        // (2) Every other binding is released into the interior as unbound
        // storage: the cards stay part of the situation — interior, unlocked,
        // never returned to `bound_from`, never re-bound.
        if let Some(s) = self.state.situations.get_mut(&sit) {
            s.bindings.clear();
        }
        // (3) The transition's one gate, decided pre-materialization: the
        // walk's arms re-check their `when`s in order against the released
        // pool — the situation's interior sums — and a max-exhausted target
        // is a miss like a failed `when`. The first arm that passes adopts;
        // a miss on every arm falls through to the normal completion
        // sequence.
        let mut adopted: Option<story_core::RecipeId> = None;
        for arm in &arms {
            let rec = self.recipe_of(arm);
            let when_holds = match &rec.when {
                None => true,
                Some(w) => {
                    let ctx = crate::eval::Ctx {
                        sit: Some(sit),
                        sel_bindings: None,
                        subject: None,
                        roll: None,
                        recipe: Some(rec.id.clone()),
                    };
                    self.eval_cond(w, &ctx)
                }
            };
            if when_holds && self.max_left(&rec) {
                adopted = Some(rec.id.clone());
                break;
            }
        }
        let Some(target_id) = adopted else {
            self.release_and_remove(sit);
            ev.push(Event::SituationCompleted { sit, recipe: current });
            return Ok(());
        };
        let target = self.recipe_of(&target_id);
        // (4) The adopted recipe materializes with no further gate: greedy
        // slots pull from the world per the standard greedy order (the
        // interior is not bindable, so released storage is never re-bound);
        // open slots re-open for mid-run `bind`. Greedy is best-effort; no
        // minimum is validated here — the pre-pull walk was the gate.
        let pool = self.greedy_pool();
        for sd in &target.slots {
            if !sd.greedy {
                continue;
            }
            let Some(max) = sd.max else { continue };
            let mut taken: Vec<EntityId> = Vec::new();
            for pick in &pool {
                if taken.len() >= max as usize {
                    break;
                }
                if taken.contains(pick) || self.is_locked(*pick) {
                    continue;
                }
                let ctx = crate::eval::Ctx::default();
                if !self.matches_slot(pick, sd, &ctx) {
                    continue;
                }
                taken.push(*pick);
            }
            for e in taken {
                let bound_from = self.state.entities[&e].zone;
                if let Some(ent) = self.state.entities.get_mut(&e) {
                    ent.zone = interior;
                }
                let seq = bind_count(&self.state, sit);
                let situation = self.state.situations.get_mut(&sit).expect("live sit");
                situation
                    .bindings
                    .entry(sd.name.clone())
                    .or_default()
                    .push(Binding {
                        entity: e,
                        bound_from,
                        seq,
                    });
                ev.push(Event::Bound {
                    sit,
                    slot: sd.name.clone(),
                    entity: e,
                });
            }
        }
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
            situation: sit,
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
    fn abort_situation(&mut self, sit: SituationId, f: &Fail, ev: &mut Vec<Event>, run: &mut Run) {
        ev.push(Event::ResolutionFailed {
            sit,
            reason: f.display_reason(),
        });
        if !self.state.situations.contains_key(&sit) {
            return;
        }
        let rec = self.recipe_of(&self.state.situations[&sit].recipe);
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

// ---------------------------------------------------------------------------
// Expiry (spec.md §4.6)
// ---------------------------------------------------------------------------

/// One expiry settlement: the timer is one-shot, cleared at settlement; a
/// declared `on expire` book replaces default destruction and addresses the
/// dying card as `subject`. Context-free: `interior`, `slot:`, and `bound`
/// read empty (T9).
fn settle_expiry(
    engine: &mut Engine,
    e: EntityId,
    ev: &mut Vec<Event>,
    run: &mut Run,
) -> Result<(), Fail> {
    let Some(ent) = engine.state.entities.get(&e) else {
        return Ok(());
    };
    let prototype = ent.prototype.clone();
    let book = engine.template(&prototype).on_expire.clone();
    let checkpoint = engine.state.clone();
    if let Some(ent) = engine.state.entities.get_mut(&e) {
        ent.expires_at = None;
    }
    if let Some(book) = book {
        let mut fctx = EffectCtx {
            sit: None,
            now: engine.state.now,
            roll: None,
            handles: BTreeMap::from([("subject".to_string(), vec![e])]),
            continued: None,
        };
        match engine.fire_book(&book, &mut fctx, None, ev, run) {
            Ok(()) => {}
            // The budget guard already restored the entry state.
            Err(f) if f.kind == DiagKind::Livelock => return Err(f),
            // Per-unit policy: roll back to the checkpoint. The timer is
            // spent regardless — expiry is one-shot — so the loop progresses.
            Err(_) => {
                engine.state = checkpoint;
                if let Some(ent) = engine.state.entities.get_mut(&e) {
                    ent.expires_at = None;
                }
            }
        }
    } else {
        engine.destroy_entity(e);
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Autonomous checkpoints (spec.md §4.6)
// ---------------------------------------------------------------------------

/// One autonomous checkpoint: in verb declaration order, each autonomous verb
/// not at its instance cap runs the selection law over empty bindings; a hit
/// materializes at most one instance; a miss is silent. An all-instant verb
/// pulses exactly once: one selection, one inline open-complete cycle.
pub(crate) fn checkpoint(
    engine: &mut Engine,
    ev: &mut Vec<Event>,
    run: &mut Run,
) -> Result<(), RuntimeDiagnostic> {
    engine.charge(run).map_err(Fail::into_diagnostic)?;
    let verbs: Vec<VerbId> = {
        let mut vs: Vec<&story_content::ir::VerbDecl> = engine.content.verbs.values().collect();
        vs.sort_by_key(|v| v.decl_index);
        vs.into_iter()
            .filter(|v| v.mode == VerbMode::Autonomous)
            .map(|v| v.id.clone())
            .collect()
    };
    for verb in verbs {
        let Some(decl) = engine.content.verbs.get(verb.as_str()).cloned() else {
            continue;
        };
        if engine.verb_full(&decl) {
            continue;
        }
        match engine.select(Source::Poll, &decl, None, None, None) {
            Selection::Matched(rec) => {
                // Per-verb checkpoint: a failure rolls this verb's open back
                // only — earlier verbs' opens survive with their events.
                let cp = engine.state.clone();
                let sit = engine.materialize(&decl, &rec, SituationSource::Autonomous, &[], ev);
                match sit {
                    Err(()) => ev.push(Event::StartSkipped {
                        verb: verb.clone(),
                        reason: StartSkipReason::MaterializationFailed,
                    }),
                    Ok(sit) => {
                        if rec.duration == 0 {
                            match engine.completion_steps(sit, ev, run) {
                                Ok(()) => {}
                                // The Livelock breach already restored the
                                // entry state; the whole call unwinds (T5).
                                Err(f) if f.kind == DiagKind::Livelock => {
                                    return Err(f.into_diagnostic());
                                }
                                Err(f) => {
                                    // The event names the situation while it
                                    // is still live, before its rollback.
                                    ev.push(Event::ResolutionFailed {
                                        sit,
                                        reason: f.display_reason(),
                                    });
                                    engine.state = cp;
                                }
                            }
                        }
                    }
                }
            }
            // A miss is silent; a guard refusing at a poll is a silent miss.
            Selection::Nothing | Selection::GuardRefused(_) => {}
        }
        if engine.state.ended.is_some() {
            return Ok(());
        }
    }
    Ok(())
}
