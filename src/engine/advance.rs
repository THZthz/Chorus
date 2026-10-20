//! Time (spec §6.6, §6.7): the atomic advance pass — due windows closing in
//! (end, begin_id, clause) order, slot triggers and expiries at each crossed
//! tick, the WAIT row that alone moves the clock — and the window
//! completion (`fire_window`): recipe outcomes, the `on complete` trigger
//! phase, and the deferred RUN replay, all under the window-refuse
//! discipline.

use std::collections::HashSet;

use indexmap::IndexMap;

use crate::content::{CompiledContent, CompiledRecipe, CompiledTrigger};
use crate::core::clock::{MIN_PER_TICK, TICKS_PER_DAY};
use crate::core::dict::Dictionary;
use crate::core::predicate::RoleBindings;
use crate::core::r#ref::id_ref;
use crate::core::roles::{Role, ROLES};
use crate::core::sentence::{Arg, Outcome};
use crate::core::types::{clone_state, AttrValue, OpenWindow, State};
use crate::error::{ChorusError, Result};

use super::exec::{count_deferred, register_windows};
use super::outcomes::{
    apply_outcome, apply_outcomes_with_refuse, refuse_marker, ApplyCtx, ChainCtx,
};
use super::record::{
    checked_step, make_begin_record, make_complete_record, make_event_record, make_wait_record,
    ActionRecord, Step,
};
use super::validate::{clause_refs, defers_selection, resolve_role_refs, select_recipe, Prepared};

/// The advance pass result: the rewritten state plus the records minted.
pub(super) struct PassResult {
    pub(super) state: State,
    pub(super) records: Vec<ActionRecord>,
}

/// One hoisted expiry snapshot entry: `id` and `at`. fire_slot re-reads the
/// LIVE entity's name, so a snapshot name field would be carried data with no
/// consumer.
struct Expiring {
    id: String,
    at: i64,
}

/// v0.5 — advance the world from `state.now` to `to_min` in ONE atomic pass.
/// Candidates merge chronologically: due windows close at their `end` (ordered
/// by end, then begin_id, then clause — a total order with no ties); authored
/// slot triggers and expiries fire at each crossed tick (§6.6 order:
/// triggers in declaration order, then expiries from the hoisted snapshot).
/// A completion at minute m fires BEFORE slot-m events. The pass ends with one
/// WAIT row — the only record that moves the clock — spanning exactly
/// `[state.now, to_min]`, so wait spans partition time (repeating triggers
/// cannot double-fire across windows). `ADVANCE` outcomes land while windows
/// complete and EXTEND the pass target; the extension rides the wait row's
/// span, which is what makes it replay-safe (fold never re-derives it).
pub(super) fn resolve_due(
    content: &CompiledContent,
    dict: &Dictionary,
    state_in: State,
    to_min: i64,
    first_record_id: i64,
    id_gen: &mut dyn FnMut() -> String,
) -> Result<PassResult> {
    // a zero-span pass is meaningful only when a due window exists (a
    // zero-duration verb completes exactly at its begin minute).
    // The FIRST disjunct (`to_min < state.now`) is dead —
    // execute_step guarantees `start_min >= state.now` —
    // but the SECOND disjunct is the live `to_min == now && !has_due` path
    // and MUST stay.
    let has_due = state_in.windows.values().any(|w| w.end <= to_min);
    if to_min < state_in.now || (to_min == state_in.now && !has_due) {
        return Ok(PassResult { state: state_in, records: Vec::new() });
    }
    let from_min = state_in.now;
    let mut state = state_in;
    let mut records: Vec<ActionRecord> = Vec::new();
    // §6.6: the expiry snapshot is taken once per pass and hoisted over every
    // completion and slot — no cascades within one advance; expiries created
    // by this pass's outcomes resolve at the NEXT advance. Snapshot order =
    // entities insertion order.
    let expiring: Vec<Expiring> = state
        .entities
        .values()
        .filter(|e| {
            e.dead_action.is_none() && matches!(e.attrs.get("expiresAt"), Some(AttrValue::Num(_)))
        })
        .map(|e| Expiring {
            id: e.id.clone(),
            at: match e.attrs.get("expiresAt") {
                Some(AttrValue::Num(n)) => *n,
                _ => unreachable!("filtered to a number above"),
            },
        })
        .collect();
    let mut cursor = from_min;
    let mut target = to_min;
    let mut id = first_record_id;
    // §6.7's livelock guard: window completions COUNTED per advance pass.
    // Cycles are legal (needs→discardseason→drawseason→needs stays legal) —
    // the budget only bounds the unbounded case, a cycle of ZERO-DURATION
    // follow-ups generating due work inside one pass. The counter is
    // per-pass and stateless: finite chains resolve across passes, and the
    // fold never re-derives any of it (a tripped pass threw before the wait
    // row, so nothing logged).
    let mut completions: i64 = 0;
    loop {
        // due windows in (end, begin_id, clause)
        // order: a total order with no ties — so the ordered MINIMUM is
        // exactly the element a full sort-then-first picked, found in one
        // pass without the per-iteration Vec + sort. Same selection; the
        // clone stays because the borrow must end before `state` moves
        // into fire_window.
        let next_w = state
            .windows
            .values()
            .filter(|w| w.end <= target)
            .min_by(|a, b| {
                a.end.cmp(&b.end).then(a.begin_id.cmp(&b.begin_id)).then(a.clause.cmp(&b.clause))
            })
            .cloned();
        let slot_abs = next_slot_after(cursor, target);
        if let Some(w) = next_w {
            if slot_abs.is_none_or(|s| w.end <= s) {
                completions += 1;
                if completions > WINDOW_COMPLETION_BUDGET {
                    // loud, per §10's posture: the throw rejects the whole
                    // step — nothing logged, clock unmoved (§9.2)
                    return Err(ChorusError::new(format!(
                        "livelock guard: the advance pass completed {completions} windows — \
                         over the {WINDOW_COMPLETION_BUDGET} budget; a RUN cycle of zero-duration \
                         follow-ups is spinning (§6.7)"
                    )));
                }
                let w_end = w.end;
                let fired = fire_window(content, dict, state, &w, id, id_gen)?;
                state = fired.state;
                records.push(fired.record);
                // §6.7: the synthesized follow-up BEGINs ride after the
                // parent's COMPLETE row and own their record ids
                id += 1 + fired.extra_records.len() as i64;
                records.extend(fired.extra_records);
                // NOTE: the slot cursor does NOT jump to the window's end —
                // the ticks the window spanned must still strike (a
                // completion at minute m precedes slot-m events, but
                // slot-(<m) events fire after it in the same pass, §6.6)
                if fired.advance_extra > 0 {
                    target = checked_step(target, fired.advance_extra, "advance extension");
                }
                // §4.4: the story ended inside this completion — the pass
                // closes at its minute (the completion's own ADVANCE
                // extension still counts: it is part of the application).
                // Nothing further fires; later due windows and slots wait for
                // a night that will not come (undo past the ending reopens
                // them)
                if state.ended.is_some() {
                    target = checked_step(w_end, fired.advance_extra, "advance extension");
                    break;
                }
                // §6.9: the ambient check AFTER EVERY COMPLETION — an ambient
                // verb with a free station whose §6.4 walk selects over the
                // world opens its window here, beginning at the completion
                // minute (the chain follow-up's parent_end face). Its BEGIN
                // rows ride after the chain follow-ups', in verb declaration
                // order.
                let ambient = ambient_sweep(content, dict, &mut state, w_end, id)?;
                id += ambient.len() as i64;
                records.extend(ambient);
                continue;
            }
        }
        if let Some(slot_abs) = slot_abs {
            // `fire_slot` speaks in tick INDICES (§6.6: slot = abs % 144,
            // minute = abs * 10); the walk speaks in minutes
            let slot_records = fire_slot(
                content,
                dict,
                &mut state,
                slot_abs / MIN_PER_TICK,
                &expiring,
                id,
                id_gen,
            )?;
            id += slot_records.len() as i64;
            records.extend(slot_records);
            cursor = slot_abs;
            // §4.4: an ending fired at this tick — the pass closes here;
            // later slots and still-open windows wait for a night that will
            // not come (undo past the ending reopens them)
            if state.ended.is_some() {
                target = slot_abs;
                break;
            }
            // §6.9: the ambient check AT EVERY BOUNDARY CROSSING — after the
            // tick's event rows (and their chain follow-ups), a free ambient
            // station whose walk selects opens at the crossing minute.
            let ambient = ambient_sweep(content, dict, &mut state, slot_abs, id)?;
            id += ambient.len() as i64;
            records.extend(ambient);
            continue;
        }
        break;
    }
    records.push(make_wait_record(id, from_min, target - from_min, dict)?);
    state.now = target;
    Ok(PassResult { state, records })
}

/// The first tick boundary strictly
/// after `cursor`, or None past `target` — the (from, to] crossing rule of
/// §6.6, one boundary at a time
fn next_slot_after(cursor: i64, target: i64) -> Option<i64> {
    debug_assert!(
        cursor >= 0,
        "next_slot_after: negative cursor {cursor} (clocks are non-negative)"
    );
    // Math.floor == truncated division on the non-negative domain
    let abs = (cursor / MIN_PER_TICK + 1) * MIN_PER_TICK;
    if abs <= target {
        Some(abs)
    } else {
        None
    }
}

/// §6.9 — the ambient sweep: every `ambient` verb, in declaration order,
/// with NO open window whose §6.4 walk selects a recipe over the CURRENT
/// world opens its window here. The walk runs over EMPTY bindings (the
/// §6.6 event-binding discipline: a role-path comparison reads false;
/// counts/sums over a role read 0, while the world forms — `exists`,
/// `count(name:…)`, `total(table, …)` — are the selection vocabulary). The open is the player commit's ONE path:
/// `make_begin_record` over the §5.1 actor-less clause (the same shape a
/// chain follow-up onto a slotless verb synthesizes), registered through
/// `register_windows` — the execution count lands at begin and the window
/// length is the selected recipe's `dur`, else the verb's `duration:`.
/// The row is an ordinary BEGIN row; the minimal marker is `trigger =
/// "ambient"` — a field the action branch of every reader (the fold's
/// `apply_record` included) never reads, so replay is untouched and a
/// chain-PINNED follow-up onto the same verb stays unmarked (the pin, not
/// the sweep, opened it). The sweep runs ONLY inside a live advance pass —
/// after every window completion (begin minute = the completion's end) and
/// at every crossed tick (begin minute = the crossing) — never at fold:
/// the fold replays the logged BEGIN rows and re-derives nothing (the
/// §6.6 event-row discipline). A station already processing skips the
/// verb silently (one window per station, §6.7 — the verb's own chain
/// follow-up included); the re-arm cadence is therefore natural — the
/// window re-opens at the first check after its completion, so a
/// self-chaining ambient recipe pulses at its own window length. A
/// zero-duration self-chain (recipe `dur 0m` or a zero `duration:` verb)
/// opens, completes and re-opens inside one pass — the §6.7
/// WINDOW_COMPLETION_BUDGET trips loudly, the one guard, no new one.
fn ambient_sweep(
    content: &CompiledContent,
    dict: &Dictionary,
    state: &mut State,
    at_min: i64,
    first_record_id: i64,
) -> Result<Vec<ActionRecord>> {
    let mut records: Vec<ActionRecord> = Vec::new();
    if state.ended.is_some() {
        return Ok(records);
    }
    for verb in content.verbs.values() {
        if !verb.ambient {
            continue;
        }
        if state.windows.values().any(|w| w.verb == verb.name) {
            continue; // the station is processing — one window per station
        }
        let bindings = RoleBindings::new();
        let Some(recipe) = select_recipe(verb, &bindings, state)? else {
            continue; // nothing matches over the world: the verb idles
        };
        let mut clause = crate::core::sentence::empty_clause(Vec::new());
        clause.verb = verb.name.clone();
        let step = Step {
            start_min: Some(at_min),
            clock: None,
            clauses: vec![clause.clone()],
            t_real: None,
        };
        let prepared =
            [Prepared { clause: &clause, verb, refs: clause_refs(&clause), recipe: Some(recipe) }];
        let progress: Vec<Outcome> =
            recipe.out.iter().filter(|o| o.op == "PROGRESS").cloned().collect();
        let mut record = make_begin_record(
            first_record_id + records.len() as i64,
            at_min,
            &step,
            &prepared,
            progress,
            dict,
        )?;
        record.trigger = Some("ambient".to_string());
        register_windows(&record, state, dict, content)?;
        records.push(record);
    }
    Ok(records)
}

/// §6.7's livelock budget: an advance pass that completes MORE than this
/// many windows fails loud (a documented guard with a pinned test, the
/// §10 posture). Per-pass, stateless, fold-unaffected.
const WINDOW_COMPLETION_BUDGET: i64 = 1024;

/// Close one window: re-resolve its
/// clause bindings (dead refs DROP — events do not respect locks, §6.7 v0.5),
/// apply the commit-time recipe's outcomes anchored to the window's START
/// clock (§6.7: clock-anchored effects use the start clock). A REFUSE that
/// only decides now, or an outcome that cannot apply, refuses the WINDOW: the
/// partial effects are discarded with the scratch, the attempt is logged with
/// a REFUSE marker (the fold's existing no-op rule), and the window still
/// closes — a pass that rolled back would leave the window retrying forever
/// (§9.2's completion analog).
struct FireWindowResult {
    state: State,
    record: ActionRecord,
    advance_extra: i64,
    /// §6.7: the synthesized follow-up BEGIN rows (one per pinned chain
    /// branch — a second pinning chain op refuses the window), logged AFTER
    /// the parent's COMPLETE row; their windows are already inserted into
    /// `state`.
    extra_records: Vec<ActionRecord>,
}

fn fire_window(
    content: &CompiledContent,
    dict: &Dictionary,
    state_in: State,
    w: &OpenWindow,
    record_id: i64,
    id_gen: &mut dyn FnMut() -> String,
) -> Result<FireWindowResult> {
    let mut scratch = clone_state(&state_in);
    let mut bindings = RoleBindings::new();
    for role in ROLES {
        // dead refs DROP
        let ids = resolve_role_refs(&scratch, &w.refs[&role])?;
        bindings.insert(role, ids);
    }
    // The §6.4 selection face of this window: an empty `w.recipe` is the
    // DEFERRAL mark — a held-capable verb's player-committed window selects
    // NOW, over the completion bindings (held included), through the same
    // first-match walk the table runs (the max discipline reading the same
    // executed map — the window's own count has not landed yet; it lands
    // below, on survival). Nothing matching: the completion analog of the
    // commit-time "no recipe matched" rejection — the window refuses, closes,
    // and logs its marker (the fold replays the no-op). A non-empty id looks
    // its recipe up in compiled content (drift fails loud, as always).
    let (recipe, deferred) = if w.recipe.is_empty() {
        let Some(v) = content.verbs.get(&w.verb) else {
            return Err(ChorusError::new(format!(
                "completion of {} (begin row {}): verb is not in the compiled content — drift",
                w.verb, w.begin_id
            )));
        };
        // the deferral mark is ONLY ever written for a verb that declares a
        // held slot (execute_step's commit-time walk rejects a held-less verb
        // whose selection finds nothing). An empty entry on a held-less verb
        // would re-run selection over the drifted world here — the mid-window
        // ghost the commit-time rejection exists to prevent — so it fails
        // loud, like every drift.
        if !defers_selection(v) {
            return Err(ChorusError::new(format!(
                "completion of {} (begin row {}): the empty recipe entry is the §6.4 deferral mark, but {} declares no held slot — a held-less verb selects at commit (drift)",
                w.verb, w.begin_id, w.verb
            )));
        }
        match select_recipe(v, &bindings, &scratch)? {
            Some(r) => (r, true),
            None => {
                // the refused deferred completion: state untouched, window
                // closed, the marker journals the miss. No count lands (the
                // draft died — the same rule the fold replays).
                let mut out = state_in;
                out.windows.shift_remove(&w.key);
                let concrete = vec![refuse_marker(format!("{}: no recipe matched", w.verb))];
                let mut record = make_complete_record(record_id, w, concrete, dict)?;
                record.recipes = Vec::new();
                return Ok(FireWindowResult {
                    state: out,
                    record,
                    advance_extra: 0,
                    extra_records: Vec::new(),
                });
            }
        }
    } else {
        let found =
            content.verbs.get(&w.verb).and_then(|v| v.recipes.iter().find(|r| r.id == w.recipe));
        match found {
            Some(r) => (r, false),
            None => {
                return Err(ChorusError::new(format!(
                    "completion of {} (begin row {}): recipe '{}' is not in the compiled content — content drift between commit and completion",
                    w.verb, w.begin_id, w.recipe
                )));
            }
        }
    };
    let mut ctx = ApplyCtx {
        state: &mut scratch,
        dict,
        content,
        bindings,
        now: w.start, // §6.7: expires and friends anchor to the START clock
        record_id,
        id_gen,
        extra_advance_min: 0,
        roll: None,
        // §6.7: the chain channel — the parent-window identity the RUN arm
        // walks against (its end clock is the follow-up's begin, its authored
        // refs become the follow-up clause). §6.7: recipe-body RUN ops DEFER
        // while the completion's `on complete` triggers fire — a completion
        // applies the recipe's own outcomes, then the triggers, then any RUN.
        chain: Some(ChainCtx {
            parent_key: w.key.clone(),
            parent_end: w.end,
            parent_refs: w.refs.clone(),
            next_record_id: record_id + 1,
            run_taken: false,
            extra_records: Vec::new(),
            deferring: true,
            deferred: Vec::new(),
            event: false,
        }),
    };
    let mut concrete: Vec<Outcome> = Vec::new();
    // the try/catch spans the outcome loop: an
    // outcome that cannot apply refuses the WINDOW with a REFUSE marker
    // carrying the message. (PROGRESS on a PINNED window is skipped — the
    // ticks rode the BEGIN row; a DEFERRED window's ticks were unknown at
    // begin, so they journal here, on the COMPLETE row — display-only rows
    // either way.)
    let mut refused = apply_outcomes_with_refuse(
        // pinned: PROGRESS rode the BEGIN row — skipped here. Deferred
        // (§6.4): the ticks were unknown at begin, so they journal on this
        // COMPLETE row.
        recipe.out.iter().filter(|o| deferred || o.op != "PROGRESS"),
        &mut ctx,
        &mut concrete,
    );
    // §6.7 (v0.9.9): the consuming burn — the surviving held cards of a
    // consuming slot die HERE, between the recipe's own outcomes and the
    // completion-trigger phase (CS-faithful: the consumed card is gone
    // before CompleteState's induction pass, so a burned card fires no
    // `on complete`). The burns are engine-written `DESTROY #id` concrete
    // rows appended to the COMPLETE row's outcome list — the fold replays
    // them with no new machinery (the completion-trigger rows' precedent).
    if !refused {
        refused = burn_consuming_held(content, w, &mut ctx, &mut concrete);
    }
    if !refused {
        refused = fire_completion_triggers(content, w, recipe, &mut ctx, &mut concrete);
    }
    // §6.7: the recipe's own RUN ops — parked while the triggers
    // fired — replay now, in list order, under the same window-refuse
    // handling as the outcome loop above. A refusal here (the inline guard,
    // the second-RUN rule) still rejects the PARENT window.
    if !refused {
        let deferred: Vec<Outcome> =
            ctx.chain.as_ref().map(|c| c.deferred.clone()).unwrap_or_default();
        refused = apply_outcomes_with_refuse(deferred.iter(), &mut ctx, &mut concrete);
    }
    let advance_extra = ctx.extra_advance_min;
    let chain = ctx.chain.take();
    drop(ctx);
    let mut extra_records: Vec<ActionRecord> = Vec::new();
    let mut out = if refused { state_in } else { scratch };
    // the parent leaves the open table FIRST (removal preserves the remaining
    // keys' insertion order) — the follow-up's registration sees the station
    // free of its parent (§6.7; the busy check proper is apply_run's, with
    // the parent's own window excluded, §6.7)
    out.windows.shift_remove(&w.key);
    if !refused {
        // §6.4: a deferred window's execution count lands at its successful
        // completion, through the ONE rule play and fold share (a refused
        // completion counted nothing — its draft died with the scratch)
        if deferred {
            count_deferred(&mut out, content, &w.verb, &recipe.id);
        }
        // §6.7: each follow-up registers through the ONE registration path
        // play and fold share — the §6.4 count and the window land
        // identically on both paths, so the fold's BEGIN replay cannot drift
        // from the play-side insert. A refused parent takes its follow-up
        // with it (no follow-up on a window-refuse).
        if let Some(chain) = chain {
            for rec in &chain.extra_records {
                register_windows(rec, &mut out, dict, content)?;
            }
            extra_records = chain.extra_records;
        }
    }
    let mut record = make_complete_record(record_id, w, concrete, dict)?;
    // §6.4: the deferred window's row carries the id SELECTED at completion
    // (the window's own entry stayed empty — the deferral mark)
    if deferred {
        record.recipes = vec![recipe.id.clone()];
    }
    Ok(FireWindowResult { state: out, record, advance_extra, extra_records })
}

/// §6.7: the consuming slot's burn face — every held card the window's
/// completion bindings bound that is STILL ALIVE at burn time is destroyed.
/// The burn addresses the ids the bindings resolved BEFORE the recipe's own
/// outcomes ran (the same ids `DESTROY held` concretizes against), never a
/// re-resolution of the window's refs: a name-addressed hold re-resolved now
/// would re-address the pool, and a `DESTROY held, SPAWN <same name>` recipe
/// would burn the minted successor. The living-set intersection keeps the
/// survivor-only face: a held card the outcomes already consumed no-ops (a
/// REPLACE's successor is not a binding), a survivor burns even when a
/// same-name copy now lives. Returns true when a burn refused the window
/// (an application failure is not a gate — the §6.7 window-refuse, the
/// recipe loop's discipline).
fn burn_consuming_held(
    content: &CompiledContent,
    w: &OpenWindow,
    ctx: &mut ApplyCtx,
    concrete: &mut Vec<Outcome>,
) -> bool {
    let consuming = content
        .verbs
        .get(&w.verb)
        .and_then(|v| v.slot_by_role.get("held"))
        .is_some_and(|s| s.consuming);
    if !consuming {
        return false;
    }
    let bound = ctx.bindings.get(&Role::Held).map(Vec::as_slice).unwrap_or(&[]);
    let burns: Vec<Outcome> = bound
        .iter()
        .filter(|id| ctx.state.entities.get(*id).is_some_and(|e| e.dead_action.is_none()))
        .map(|id| Outcome::concrete("DESTROY", vec![Arg::Ref { r#ref: id_ref(id.clone()) }]))
        .collect();
    apply_outcomes_with_refuse(burns.iter(), ctx, concrete)
}

/// §6.7: the completion-trigger phase — the window's committed locked
/// cards (the §6.7 busy set, actors free; deduped, a card bound through
/// two roles fires once; in locked order) fire their authored
/// `on complete` triggers, bare ones always and aspect-gated ones iff the
/// completed recipe declares the aspect. Outcomes apply on the draft in
/// fire order and their concrete rows APPEND to the recipe's own — no
/// EVENT row, no state.fired (the fold replays the appended list without
/// recomputing gates). A failure inside a body refuses the WINDOW (the
/// §6.7 parent-window path), never a whole-step throw. RUN inside a body
/// applies through the chain channel like every RUN: global-table
/// candidates, the parent's own window excluded from the busy check.
/// Returns true when a trigger body refused the window.
fn fire_completion_triggers(
    content: &CompiledContent,
    w: &OpenWindow,
    recipe: &CompiledRecipe,
    ctx: &mut ApplyCtx,
    concrete: &mut Vec<Outcome>,
) -> bool {
    if let Some(c) = ctx.chain.as_mut() {
        c.deferring = false;
    }
    let mut refused = false;
    'complete_phase: {
        let mut seen: HashSet<&String> = HashSet::new();
        for card_id in &w.locked {
            if !seen.insert(card_id) {
                continue; // dedupe: one card in two roles fires once
            }
            // a card that died mid-window no longer carries triggers
            // (bindings drop dead refs the same way, §6.7)
            let Some(home) = ctx
                .state
                .entities
                .get(card_id)
                .filter(|e| e.dead_action.is_none())
                .map(|e| e.name.clone())
            else {
                continue;
            };
            for trig in &content.triggers {
                let CompiledTrigger::Complete { home: trig_home, aspect, out, .. } = trig else {
                    continue;
                };
                if trig_home != &home {
                    continue;
                }
                // the authored emitter gate: the aspect form fires iff
                // the completed recipe declares the aspect
                if let Some(a) = aspect {
                    if !recipe.aspects.iter().any(|x| x == a) {
                        continue;
                    }
                }
                if apply_outcomes_with_refuse(out, ctx, concrete) {
                    refused = true;
                    break 'complete_phase;
                }
            }
        }
    }
    refused
}

/// Fire one crossed tick: authored
/// triggers (declaration order) first, then expiries from the pass's snapshot
/// — §6.6 order, unchanged (§6.6).
fn fire_slot(
    content: &CompiledContent,
    dict: &Dictionary,
    state: &mut State,
    abs_slot: i64,
    expiring: &[Expiring],
    first_event_id: i64,
    id_gen: &mut dyn FnMut() -> String,
) -> Result<Vec<ActionRecord>> {
    let mut records: Vec<ActionRecord> = Vec::new();
    // authored slot triggers in DECLARATION order
    for trig in &content.triggers {
        let CompiledTrigger::Slot { id: trig_id, slot, once, out } = trig else {
            continue; // expiry triggers ride the expiry path below
        };
        if *slot != abs_slot % TICKS_PER_DAY {
            continue;
        }
        if *once && state.fired.contains(trig_id) {
            continue; // §6.6: once means once
        }
        let mut outcomes: Vec<Outcome> = Vec::new();
        let chain = {
            let mut ctx = make_event_ctx(
                state,
                dict,
                content,
                abs_slot,
                first_event_id + records.len() as i64,
                id_gen,
                true,
            );
            for o in out {
                outcomes.extend(apply_outcome(o, &mut ctx)?.concrete);
            }
            ctx.chain.take()
        };
        // §6.6/§6.7: the chain ops' follow-up BEGINs REGISTER IMMEDIATELY on
        // the pass's state — a later firing in the same crossing (the next
        // authored trigger, or an expiry) sees the station busy and skips
        // silently — and append to the record list right after their EVENT
        // row (the ids run event_id + 1 … + n; resolve_due's
        // `id += slot_records.len()` keeps every later id correct).
        let mut chain_extra: Vec<ActionRecord> = Vec::new();
        if let Some(chain) = chain {
            for rec in &chain.extra_records {
                register_windows(rec, state, dict, content)?;
            }
            chain_extra = chain.extra_records;
        }
        state.fired.insert(trig_id.clone()); // on the draft: a later rejection discards it
        let rid = first_event_id + records.len() as i64;
        records.push(make_event_record(rid, trig_id, abs_slot, outcomes, dict)?);
        records.extend(chain_extra);
    }
    // expiries from the hoisted snapshot, in
    // snapshot order (= entities insertion order)
    for exp in expiring {
        if exp.at > abs_slot * MIN_PER_TICK {
            continue;
        }
        let ent_name = match state.entities.get(&exp.id) {
            Some(e) if e.dead_action.is_none() => e.name.clone(),
            _ => continue,
        };
        // an authored `on expire of NAME` trigger (§6.6) replaces the
        // built-in destroy; FIRST match in declaration order wins. Its
        // outcomes ARE the event record's outcomes (record.trigger = the
        // authored id). An outcome that cannot apply throws — the whole step
        // is rejected (§9.2).
        // §4.2 pool asymmetry (documented ruling): the trigger matches by
        // NAME, so a crossing fires it no matter WHICH copy carries the
        // expires — but its name-addressed outcomes resolve per the pool rule
        // (the OLDEST copy), which under a pool may not be the crossing card.
        // The built-in path below has no such ambiguity: it addresses by id.
        let authored = content.triggers.iter().find(|t| {
            matches!(t, CompiledTrigger::Expire { on_expire_of, .. } if on_expire_of == &ent_name)
        });
        let (trigger_id, outcomes) = match authored {
            Some(a) => {
                let mut outcomes: Vec<Outcome> = Vec::new();
                let chain = {
                    let mut ctx = make_event_ctx(
                        state,
                        dict,
                        content,
                        abs_slot,
                        first_event_id + records.len() as i64,
                        id_gen,
                        true,
                    );
                    for o in a.out() {
                        outcomes.extend(apply_outcome(o, &mut ctx)?.concrete);
                    }
                    ctx.chain.take()
                };
                // §6.6/§6.7: the expiry hand-off registers immediately, like
                // the slot path above
                let mut chain_extra: Vec<ActionRecord> = Vec::new();
                if let Some(chain) = chain {
                    for rec in &chain.extra_records {
                        register_windows(rec, state, dict, content)?;
                    }
                    chain_extra = chain.extra_records;
                }
                state.fired.insert(a.id().to_string());
                let rid = first_event_id + records.len() as i64;
                records.push(make_event_record(rid, a.id(), abs_slot, outcomes, dict)?);
                records.extend(chain_extra);
                continue;
            }
            None => {
                // engine-written destroys are keyed
                // to the EXPIRING entity and concretize to DESTROY #id —
                // under a pool a name-addressed destroy would kill the oldest
                // copy while the real expiring card re-fires
                let destroy =
                    Outcome::concrete("DESTROY", vec![Arg::Ref { r#ref: id_ref(exp.id.clone()) }]);
                let mut outcomes: Vec<Outcome> = Vec::new();
                {
                    let mut ctx = make_event_ctx(
                        state,
                        dict,
                        content,
                        abs_slot,
                        first_event_id + records.len() as i64,
                        id_gen,
                        false, // the built-in path authors no chain op
                    );
                    outcomes.extend(apply_outcome(&destroy, &mut ctx)?.concrete);
                }
                (format!("expire:{ent_name}"), outcomes)
            }
        };
        // play marks fired exactly as the fold's `apply_record` does — one entry
        // per EVENT row's trigger (§6.6); on the draft, so a rejection
        // discards it
        state.fired.insert(trigger_id.clone());
        let rid = first_event_id + records.len() as i64;
        records.push(make_event_record(rid, &trigger_id, abs_slot, outcomes, dict)?);
    }
    Ok(records)
}

/// Event bindings are EMPTY ({}), and
/// `now` is the crossed boundary in absolute minutes
/// (abs_slot * MIN_PER_TICK). `chained` arms the §6.6 event-chain channel
/// for the authored slot/expire bodies (the built-in expiry destroy authors
/// no chain op and keeps the channel None): `parent_key` is "" (no parent
/// window — the busy exclusion excludes nothing, so a trigger never hands
/// off onto a busy station), `parent_end` is the crossing minute (the
/// follow-up BEGINS at the boundary), `parent_refs` is the empty map (the
/// projection produces a clause with NO refs — the follow-up locks nothing,
/// and its gate merge reduces to the empty bindings, so a role-path
/// comparison reads false; counts/sums over a role read 0, while the world
/// forms — `exists`, `count(name:…)` — are the selection vocabulary), and
/// `deferring` is false — event bodies have no deferral phase; chain ops
/// apply inline, in list order.
fn make_event_ctx<'a>(
    state: &'a mut State,
    dict: &'a Dictionary,
    content: &'a CompiledContent,
    abs_slot: i64,
    record_id: i64,
    id_gen: &'a mut dyn FnMut() -> String,
    chained: bool,
) -> ApplyCtx<'a> {
    let chain = chained.then(|| ChainCtx {
        parent_key: String::new(),
        parent_end: abs_slot * MIN_PER_TICK,
        parent_refs: IndexMap::new(),
        next_record_id: record_id + 1,
        run_taken: false,
        extra_records: Vec::new(),
        deferring: false,
        deferred: Vec::new(),
        event: true,
    });
    ApplyCtx {
        state,
        dict,
        content,
        bindings: RoleBindings::new(),
        now: abs_slot * MIN_PER_TICK,
        record_id,
        id_gen,
        extra_advance_min: 0,
        roll: None,
        chain,
    }
}
