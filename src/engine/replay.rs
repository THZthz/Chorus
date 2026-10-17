//! The fold half of replay (spec §7): one record's concrete outcomes back
//! onto a state. Wait rows are the ONLY clock movers; refused rows are
//! journal history, never replayed effects (§6.5); a BEGIN re-registers its
//! windows through the one shared registration path.

use crate::core::clock::{abs_min, parse_clock};
use crate::core::dict::Dictionary;
use crate::core::predicate::RoleBindings;
use crate::core::roles::Role;
use crate::core::types::State;
use crate::error::{ChorusError, Result};

use super::exec::register_windows;
use super::outcomes::{apply_outcome, ApplyCtx};
use super::record::{checked_step, ActionRecord};
use super::validate::resolve_role_refs;

/// Apply one record's concrete outcomes to
/// a state (replay). Refused records are state no-ops: their logged outcomes
/// are journal history, never replayed effects (§6.5).
pub fn apply_record(
    record: &ActionRecord,
    state: &mut State,
    dict: &Dictionary,
    content: &crate::content::CompiledContent,
) -> Result<()> {
    // v0.5 — the pure clock row: the ONLY record that moves `now`, so the
    // fold's clock is a function of the wait spans alone (BEGIN/COMPLETE never
    // touch it, which is what makes interleaved windows replay exactly)
    if record.kind == "wait" {
        let start = match record.start_min {
            Some(m) => m,
            None => abs_min(parse_clock(&record.t_game)?),
        };
        if state.now != start {
            return Err(ChorusError::new(format!(
                "wait row {} starts at {} but the folded clock reads {} — wait spans must partition time (log drift, or a pre-v0.5 log)",
                record.id, record.t_game, state.now
            )));
        }
        state.now = checked_step(start, record.advanced_min, "wait span");
        return Ok(());
    }
    // v0.5 — a window closing: remove it first (a refused completion still
    // closed its window), then apply outcomes anchored to the window's START
    // clock read from the windows table — the same source play used, so SET
    // expires and friends replay identically (the completion clock on tGame
    // never feeds mechanics, §6.7)
    if record.kind == "complete" {
        // the window SHOULD be registered by the turn's BEGIN row (fold
        // rebuilds it first); a row closing an unknown window is tolerated
        // with a tGame-anchored fallback so bare-row fold probes and
        // pre-window logs still replay — the START-clock anchoring that
        // matters for `expires` is exercised on the real path, where the
        // window exists (this fallback branch is load-bearing for
        // bare-row probes and MUST stay)
        let w_start = record.window.as_ref().and_then(|k| state.windows.get(k)).map(|w| w.start);
        if let Some(k) = &record.window {
            state.windows.shift_remove(k); // removal preserves the remaining keys' insertion order
        }
        // refused: window closed, effects discarded (§6.5)
        if record.outcomes.iter().any(|o| o.op == "REFUSE") {
            return Ok(());
        }
        let start_min = match w_start {
            Some(s) => s,
            None => abs_min(parse_clock(&record.t_game)?),
        };
        // the shared ctx construction; the branch skeleton stays here
        let bindings = fold_bindings(record, state)?;
        let mut n: i64 = 0;
        let mut gen = move || {
            n += 1;
            format!("r{}s{n}", record.id)
        };
        let mut ctx = ApplyCtx {
            state,
            dict,
            content,
            bindings,
            now: start_min,
            record_id: record.id,
            id_gen: &mut gen,
            extra_advance_min: 0,
            roll: None,
            chain: None, // the fold replays concrete rows only — RUN never logs
        };
        for o in &record.outcomes {
            apply_outcome(o, &mut ctx)?;
        }
        // ADVANCE replayed here only touches the inert `extra_advance_min` — the
        // extension lives in the wait row's span (§4.4 replay safety)
        return Ok(());
    }
    // refused: state no-op, clock untouched (§6.5) — this guard ALSO covers
    // action rows, so a refused BEGIN never opens its windows on replay
    if record.outcomes.iter().any(|o| o.op == "REFUSE") {
        return Ok(());
    }
    let is_event = record.kind == "event";
    if is_event {
        // events don't own the clock — only wait rows move it (§7, v0.5).
        // once-triggers replay as fired (§6.6); a missing trigger name folds
        // as the empty string
        state.fired.insert(record.trigger.clone().unwrap_or_default());
    }
    // the display clock parses for EVERY row kind — a corrupt t_game is drift
    // even where only events consume the minute (action rows re-derive
    // nothing from it; register_windows reads start_min first)
    let start_min = abs_min(parse_clock(&record.t_game)?);
    if is_event {
        // the shared ctx construction; the branch skeleton stays here
        let bindings = fold_bindings(record, state)?;
        let mut n: i64 = 0;
        let mut gen = move || {
            n += 1;
            format!("r{}s{n}", record.id)
        };
        let mut ctx = ApplyCtx {
            state,
            dict,
            content,
            bindings,
            now: start_min,
            record_id: record.id,
            id_gen: &mut gen,
            extra_advance_min: 0,
            roll: None,
            chain: None, // events have no window to chain from (RUN is a load error there anyway)
        };
        for o in &record.outcomes {
            apply_outcome(o, &mut ctx)?;
        }
        return Ok(());
    }
    // action = BEGIN (v0.5): register one window per clause — no clock move,
    // no outcome application (the PROGRESS ticks are journal-only; the
    // recipe's outcomes land on the COMPLETE row at the window's close)
    register_windows(record, state, dict, content)
}

/// The
/// complete and event branches share EXACTLY this ctx construction (the
/// bindings flattening and fixed fields); the two branch skeletons themselves
/// stay separate (window delete, fired add, and the start_min source differ).
/// Bindings hold ids resolved against the CURRENT fold state.
fn fold_bindings(record: &ActionRecord, state: &State) -> Result<RoleBindings> {
    let mut b = RoleBindings::new();
    b.insert(Role::Actors, resolve_role_refs(state, &record.actors)?);
    b.insert(Role::Object, resolve_role_refs(state, &record.objects)?);
    b.insert(Role::With, resolve_role_refs(state, &record.instruments)?);
    b.insert(Role::At, resolve_role_refs(state, record.at.as_slice())?);
    b.insert(Role::About, resolve_role_refs(state, record.about.as_slice())?);
    b.insert(Role::To, resolve_role_refs(state, record.to.as_slice())?);
    Ok(b)
}
