//! The interpreter (spec §6.6): the four-phase step — validation → selection
//! → the §6.5 step-atomic REFUSE pre-pass → BEGIN — plus `register_windows`,
//! the ONE window-registration path play and fold share (the divergence
//! guard, §6.4).

use crate::content::CompiledContent;
use crate::core::clock::{abs_min, clock_from_abs_min, format_clock, parse_clock};
use crate::core::dict::Dictionary;
use crate::core::parse::parse_sentence;
use crate::core::predicate::RoleBindings;
use crate::core::roles::ROLES;
use crate::core::sentence::{Clause, Outcome};
use crate::core::types::{clone_state, OpenWindow, State};
use crate::error::{ChorusError, Result};

use super::advance::resolve_due;
use super::outcomes::{apply_outcome, ApplyCtx};
use super::record::{
    checked_step, make_begin_record, rejected, ActionRecord, ExecOptions, ExecResult, Step,
};
use super::validate::{
    busy_reason, clause_refs, resolve_ref, resolve_role_refs, select_recipe, validate_slots,
    Prepared, COMMITTED_ROLES,
};

/// minutes in a day (48 halfhours of 30)
const MIN_PER_DAY: i64 = 48 * 30;

/// §6.6: the farthest one step may advance the clock — seven days. The
/// advance pass is linear in the halfhours crossed, so this bounds a single
/// step's work; nothing in a real log comes near it.
const MAX_ADVANCE_MIN: i64 = 7 * MIN_PER_DAY;

/// The four-phase step (§6.6). Only the Phase-4 `register_windows` can
/// hard-error (the verb-drift throw) — every other failure is a `rejected`
/// result by design.
pub fn execute_step(
    content: &CompiledContent,
    dict: &Dictionary,
    state_in: State,
    step: Step,
    opts: ExecOptions,
) -> Result<ExecResult> {
    // game-over (§4.4): once the story has ended nothing further executes,
    // and nothing is logged — undo past the ending record reopens the game
    // (it derives from the fold, so no special-case is needed)
    if let Some(ended) = &state_in.ended {
        return Ok(rejected(format!("the story has ended — {}", ended.title), state_in));
    }
    // v0.5: the begin minute — start_min wins (a mid-halfhour begin is
    // representable), the quantized clock field is the input syntax.
    // A DEFINED start_min wins even when it is 0 (0 wins over the clock):
    // Option + unwrap_or_else, never a 0-sentinel.
    let start_min = match step.start_min {
        Some(m) => m,
        None => abs_min(step.clock.unwrap_or_else(|| clock_from_abs_min(state_in.now))),
    };
    // monotonic mechanics time (§9.2): a step may never begin before now
    if start_min < state_in.now {
        let now = clock_from_abs_min(state_in.now);
        return Ok(rejected(
            format!(
                "clock {} is before state.now ({}:{})",
                format_clock(clock_from_abs_min(start_min)),
                now.day,
                now.halfhour
            ),
            state_in,
        ));
    }
    // §6.6: one step may advance the clock at most MAX_ADVANCE_MIN ahead —
    // the pass walks every halfhour it crosses (recurring triggers fire on
    // each), so an absurd clock (`9999999999:1`) would otherwise spin for
    // days of CPU and unbounded memory; a legitimate night never needs more
    if start_min - state_in.now > MAX_ADVANCE_MIN {
        let now = clock_from_abs_min(state_in.now);
        return Ok(rejected(
            format!(
                "clock {} is more than {} days ahead of state.now ({}:{}) — advance in shorter steps",
                format_clock(clock_from_abs_min(start_min)),
                MAX_ADVANCE_MIN / MIN_PER_DAY,
                now.day,
                now.halfhour
            ),
            state_in,
        ));
    }
    if step.clauses.is_empty() {
        let due_now = state_in.windows.values().any(|w| w.end <= start_min);
        if start_min == state_in.now && !due_now {
            let now = clock_from_abs_min(state_in.now);
            return Ok(rejected(
                format!(
                    "nothing to do — the clock already reads {}:{} (commit an action, or name a later minute to wait)",
                    now.day, now.halfhour
                ),
                state_in,
            ));
        }
    }

    let ExecOptions { first_record_id, id_gen } = opts;

    // deterministic default mint (§9.1)
    let mut mint: i64 = 0;
    let mut default_gen = move || {
        mint += 1;
        format!("e{first_record_id}_{mint}")
    };
    let id_gen: &mut dyn FnMut() -> String = match id_gen {
        Some(g) => g,
        None => &mut default_gen,
    };

    // Phase 1 — the advance (v0.5): everything due at or before the begin
    // minute resolves first — due windows in (end, begin_id, clause) order,
    // slot events at each crossed halfhour, completions before same-minute
    // events. Atomic with the whole step: any rejection below discards the
    // pass — nothing logged, clock unmoved (§9.2).
    let draft = clone_state(&state_in);
    let (mut draft, advance) =
        match resolve_due(content, dict, draft, start_min, first_record_id, id_gen) {
            Ok(pass) => (pass.state, pass.records),
            // ANY throw out of the pass rejects the step
            Err(err) => return Ok(rejected(err.msg, state_in)),
        };
    let next_id = first_record_id + advance.len() as i64;

    // a bare clock step is a pure advance — its wait row already moved the clock
    if step.clauses.is_empty() {
        return Ok(ExecResult { records: advance, state: draft, rejected: None });
    }

    // Phase 2 — validation, busy checks, selection, all against the
    // POST-ADVANCE draft: locks release when their windows close during the
    // pass, so a card freeing exactly at the begin minute is usable (§6.1/§6.4)
    let mut prepared: Vec<Prepared> = Vec::new();
    for clause in &step.clauses {
        let Some(verb) = content.verbs.get(&clause.verb) else {
            return Ok(rejected(format!("{} is not a content verb", clause.verb), state_in));
        };
        let refs = clause_refs(clause);
        // Channel note: the reachable validate_slots errors reject here with
        // the `<verb>: ` prefix (the `{error}` return channel), while
        // resolve_ref's param-form error finds no rejection path in Phase 2
        // and escapes execute_step as a hard Err (busy_reason/select_recipe
        // map their errors to the Err channel — see the `?` below). Merging
        // both into one `Result<ChorusError>` means that internal error would
        // surface as a rejection here instead. Unreachable on every input
        // path (§10): the parser only produces param refs while parsing a
        // function-body template and rejects one after
        // `kind:`, FN expansion substitutes every parameter
        // before compile, and the lenient/record paths only produce name/id
        // refs — no caller can build a param-ref clause.
        // validate_slots' messages each name the verb themselves — no prefix
        let bindings = match validate_slots(verb, &refs, &draft) {
            Ok(b) => b,
            Err(e) => return Ok(rejected(e.msg, state_in)),
        };
        if let Some(busy) = busy_reason(&draft, &clause.verb, &refs)? {
            return Ok(rejected(busy, state_in));
        }
        let Some(recipe) = select_recipe(verb, &bindings, &draft)? else {
            return Ok(rejected(format!("{}: no recipe matched", verb.name), state_in));
        };
        prepared.push(Prepared { clause, verb, refs, recipe });
    }

    // Phase 3 — the top-level REFUSE pre-pass (v0.5): authored gates reject AT
    // THE TABLE, not after the window. The trial runs on a scratch draft; only
    // a REFUSE is decisive here — an application ERROR defers to completion
    // (the state will have changed by then; the window refuses there, the
    // completion analog of §9.2). The trial's dice and spawns die with it.
    let (refused, collected) =
        table_gate_trial(content, dict, &draft, &prepared, start_min, next_id);
    if refused {
        // refused BEGIN: attempt logged, zero time, the advance discarded with
        // it. The record takes `first_record_id` (the advance's ids die with
        // the draft and are REUSED next round; fold_up_to's `r.id <
        // upto_exclusive` filter depends on the recycle — never make the id
        // sequence monotonic across the refusal).
        let mut record =
            make_begin_record(first_record_id, start_min, &step, &prepared, collected, dict)?;
        record.advanced_min = 0; // §6.5: a refusal collapses the cycle — no window elapses
        return Ok(ExecResult { records: vec![record], state: state_in, rejected: None });
    }

    // Phase 4 — BEGIN (v0.5): one row per step, one window per clause. No
    // outcome applies here: the row carries the PROGRESS ticks (journal), the
    // commit-time recipe ids, and the window table does the locking. The
    // recipe's real outcomes land on the COMPLETE row at the window's close.
    let progress: Vec<Outcome> = prepared
        .iter()
        .flat_map(|p| p.recipe.out.iter())
        .filter(|o| o.op == "PROGRESS")
        .cloned()
        .collect();
    let begin = make_begin_record(next_id, start_min, &step, &prepared, progress, dict)?;
    register_windows(&begin, &mut draft, dict, content)?;
    let mut records = advance;
    records.push(begin);
    Ok(ExecResult { records, state: draft, rejected: None })
}

/// The Phase-3 trial itself, on a scratch clone of the post-advance draft:
/// the prepared clauses' outcome lists replay (IF/RUN skipped as
/// completion-owned) and the concrete outcomes collect; a THROW here is NOT
/// a gate — the trial is cleared and the step defers to completion. Returns
/// the decisive REFUSE flag and the collected concrete outcomes (the
/// refused BEGIN row carries them as journal history).
fn table_gate_trial(
    content: &CompiledContent,
    dict: &Dictionary,
    draft: &State,
    prepared: &[Prepared],
    start_min: i64,
    next_id: i64,
) -> (bool, Vec<Outcome>) {
    let mut scratch = clone_state(draft);
    let mut smint: i64 = 0;
    let mut scratch_gen = move || {
        smint += 1;
        format!("s{next_id}_{smint}")
    };
    let mut collected: Vec<Outcome> = Vec::new();
    let refused = {
        let mut sctx = ApplyCtx {
            state: &mut scratch,
            dict,
            content,
            bindings: RoleBindings::new(),
            now: start_min,
            record_id: next_id,
            id_gen: &mut scratch_gen,
            extra_advance_min: 0,
            roll: None,
            chain: None, // the table trial never chains — RUN is skipped below
        };
        // The closure shape (a try/catch around the whole pre-pass loop): a
        // THROW here is NOT a gate — it defers to completion time and clears
        // the trial.
        let gate: Result<bool> = (|| {
            for p in prepared {
                // §4.4: the dice binding is PER CLAUSE — a roll in clause one
                // is not visible to clause two's conditions
                sctx.roll = None;
                let mut rebound = RoleBindings::new();
                for role in ROLES {
                    let ids = resolve_role_refs(sctx.state, &p.refs[&role])?;
                    rebound.insert(role, ids);
                }
                sctx.bindings = rebound;
                for outcome in &p.recipe.out {
                    // an IF is opaque to the gate check: its branches —
                    // including any REFUSE inside them — belong to the
                    // completion, where the world the window imagined actually
                    // exists (§6.8 v0.5). RUN likewise belongs to the
                    // completion: its candidate walk and inline guard run
                    // there (§6.7), and a no-match chain ends silently, so
                    // there is nothing to gate at the table.
                    if outcome.op == "IF" || outcome.op == "RUN" {
                        continue;
                    }
                    let res = apply_outcome(outcome, &mut sctx)?;
                    collected.extend(res.concrete);
                    if res.refused {
                        // exits the closure with the decisive gate
                        return Ok(true);
                    }
                }
            }
            Ok(false)
        })();
        // an error is not a gate; the trial is cleared
        match gate {
            Ok(r) => r,
            Err(_) => {
                collected.clear();
                false
            }
        }
    };
    (refused, collected)
}

/// Register one window per BEGIN clause —
/// the SAME code path play and fold run (the divergence guard): clauses come
/// from the row's canonical sentence (law 2 makes that lossless), durations
/// and recipe ids from content.
pub(super) fn register_windows(
    record: &ActionRecord,
    state: &mut State,
    dict: &Dictionary,
    content: &CompiledContent,
) -> Result<()> {
    // a BEGIN row's clauses ride its canonical sentence (law 2 makes that
    // lossless). Q6 (fixed, batch 3): a row whose sentence no longer
    // re-parses is LOG DRIFT, not an empty window — the failure is loud at
    // both play time and fold time, like the drift errors above.
    let clauses: Vec<Clause> =
        parse_sentence(&record.sentence, dict).map(|s| s.clauses).map_err(|e| {
            ChorusError::new(format!(
                "begin row {}: the recorded sentence does not re-parse ('{}': {}) — drift",
                record.id, record.sentence, e.msg
            ))
        })?;
    let start_min = match record.start_min {
        Some(m) => m,
        None => abs_min(parse_clock(&record.t_game)?),
    };
    let mut offset: i64 = 0;
    for (ci, clause) in clauses.iter().enumerate() {
        let Some(verb) = content.verbs.get(&clause.verb) else {
            // this hard-error is NOT caught by execute_step — drift is never
            // a rejection
            return Err(ChorusError::new(format!(
                "begin row {}: verb '{}' is not in the compiled content — drift",
                record.id, clause.verb
            )));
        };
        let refs = clause_refs(clause);
        let mut locked: Vec<String> = Vec::new();
        for role in COMMITTED_ROLES {
            for r in &refs[&role] {
                if let Some(e) = resolve_ref(state, r)? {
                    locked.push(e.id.clone());
                }
            }
        }
        let key = format!("{}:{}", record.id, ci);
        let recipe = record.recipes.get(ci).cloned().unwrap_or_default();
        // §6.4: the count rides the ONE registration path play and fold
        // share — a registered window increments its clause's commit-time
        // recipe id ON THE DRAFT. A table-refused step opens no window and
        // counts nothing (execute_step returns before Phase 4); a window
        // that later refuses still counted (it registered at begin).
        *state.executed.entry(recipe.clone()).or_insert(0) += 1;
        state.windows.insert(
            key.clone(),
            OpenWindow {
                key,
                begin_id: record.id,
                clause: ci as i64,
                verb: clause.verb.clone(),
                start: checked_step(start_min, offset, "window start"),
                end: checked_step(
                    checked_step(start_min, offset, "window end"),
                    verb.duration,
                    "window end",
                ),
                recipe,
                refs,
                locked,
            },
        );
        offset = checked_step(offset, verb.duration, "window offset");
    }
    Ok(())
}
