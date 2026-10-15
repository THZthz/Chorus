//! Settle helper — the v0.5 parallel-station test adapter. Under parallel
//! verb stations a committed action only BEGINS its window; these helpers
//! complete it (a pure advance to the last window's end) so outcome-level
//! tests can assert on settled state the way the pre-v0.5 engine behaved
//! (now = start + Σdurations — the same minute, by construction). Timing-
//! specific behavior (locks, windows, completion order) is tested WITHOUT
//! this helper (tests/engine_ordering.rs).
//!
//! The ONE `run`-helper shape: parallel's bare-clock advance branch + the
//! settle helpers' id-generator channel + an injectable generator (stackable's
//! `pool_dict` is the caller's dictionary choice, not this function's
//! business).

//! (Shared test lib: every helper is the common discipline, not per-file
//! dead code — each mounting test crate uses a different subset, exactly like
//! harness.rs. The `allow` lives in this one place.)
#![allow(dead_code)]

use chorus::content::content::CompiledContent;
use chorus::core::dict::Dictionary;
use chorus::core::sentence::Arg;
use chorus::core::types::State;
use chorus::engine::engine::{execute_step, ActionRecord, ExecOptions, ExecResult, Step};
use chorus::error::Result;

/// The two `execute_step` calls share the caller's id-generator channel via a
/// fresh reborrow each time (one generator object seen by both, so ids stay
/// unique across the turn).
fn opts<'a, 'b>(
    first_record_id: i64,
    id_gen: &'b mut Option<&'a mut dyn FnMut() -> String>,
) -> ExecOptions<'b> {
    // the match pins the reborrow to the SHORTER lifetime ('b) — a closure
    // here would unify the output lifetime with 'a
    let g: Option<&'b mut dyn FnMut() -> String> = match id_gen.as_mut() {
        Some(f) => {
            let r: &'b mut dyn FnMut() -> String = &mut **f;
            Some(r)
        }
        None => None,
    };
    ExecOptions {
        first_record_id,
        id_gen: g,
    }
}

/// run one step, then a pure advance closing every window it left open.
/// Records come back chronological ([advance…, begin, complete…, wait]);
/// a failure anywhere rejects the whole turn — nothing logged.
pub fn settle_step(
    content: &CompiledContent,
    dict: &Dictionary,
    state: State,
    step: Step,
    first_record_id: i64,
    id_gen: Option<&mut dyn FnMut() -> String>,
) -> Result<ExecResult> {
    // The clone is the rollback leg: on a refused settle advance the ORIGINAL
    // (never-mutated) `state` argument rides back (§9.19) — Rust
    // `execute_step` consumes it, so the untouched copy is returned instead.
    let rollback = state.clone();
    let mut id_gen = id_gen;
    let first = execute_step(
        content,
        dict,
        state,
        step,
        opts(first_record_id, &mut id_gen),
    )?;
    if first.rejected.is_some() || first.state.windows.is_empty() {
        return Ok(first);
    }
    // jump to the LAST open window's end:
    let mut end: Option<i64> = None;
    for w in first.state.windows.values() {
        end = Some(match end {
            Some(m) => m.max(w.end),
            None => w.end,
        });
    }
    let end = end.unwrap_or_else(|| unreachable!("windows non-empty checked above"));
    let pass = execute_step(
        content,
        dict,
        first.state,
        Step {
            start_min: Some(end),
            clock: None,
            clauses: vec![],
            t_real: None,
        },
        opts(first_record_id + first.records.len() as i64, &mut id_gen),
    )?;
    if let Some(rejected) = pass.rejected {
        // the turn rolls back whole (§9.19)
        return Ok(ExecResult {
            records: vec![],
            state: rollback,
            rejected: Some(rejected),
        });
    }
    let mut records = first.records;
    records.extend(pass.records);
    Ok(ExecResult {
        records,
        state: pass.state,
        rejected: None,
    })
}

/// the BEGIN row of a settled turn — the record the pre-v0.5 tests indexed as
/// records[0] (under v0.5 a turn may open with a bridge wait or close with a
/// completion; the begin row is its spine). Falls back to the last record.
#[must_use]
pub fn act(r: &ExecResult) -> Option<&ActionRecord> {
    r.records
        .iter()
        .find(|x| x.kind == "action")
        .or_else(|| r.records.last())
}

/// the COMPLETE row of a settled turn — where the recipe's real outcomes
/// landed (v0.5: BEGIN carries only PROGRESS ticks). Falls back to the last
/// record (a refused turn has no completion; its REFUSE rides the BEGIN).
#[must_use]
pub fn done(r: &ExecResult) -> Option<&ActionRecord> {
    r.records
        .iter()
        .find(|x| x.kind == "complete")
        .or_else(|| r.records.last())
}

/// the refusal reason of a turn — an authored gate (REFUSE on the BEGIN row)
/// or a completion-time failure (REFUSE on the COMPLETE row, §9.19's
/// completion analog); None when the turn did not refuse. A REFUSE without a
/// string argument yields `Some("")` (the empty-string fallback).
#[must_use]
pub fn refusal(r: &ExecResult) -> Option<String> {
    for rec in &r.records {
        let Some(ref_outcome) = rec.outcomes.iter().find(|o| o.op == "REFUSE") else {
            continue;
        };
        return Some(
            ref_outcome
                .args
                .iter()
                .find_map(|a| match a {
                    Arg::Str { v } => Some(v.clone()),
                    _ => None,
                })
                .unwrap_or_default(),
        );
    }
    None
}
