//! The §6.7 chain ops: a completion chaining into named follow-up recipes —
//! RUN's in-order first-match candidate walk, FORK's all-match fan-out, the
//! inline top-level-REFUSE gate, and the synthesized follow-up BEGINs (the
//! player-step record shape).

use indexmap::IndexMap;

use crate::content::{CompiledContent, CompiledRecipe, CompiledVerb};
use crate::core::dict::Dictionary;
use crate::core::predicate::{eval_pred, PredCtx, RoleBindings};
use crate::core::r#ref::Ref;
use crate::core::roles::{Role, ROLES};
use crate::core::sentence::{Arg, Clause, Outcome};
use crate::core::types::clone_state;
use crate::error::{ChorusError, Result};

use super::super::record::{make_begin_record, ActionRecord, Step};
use super::super::validate::{clause_refs, resolve_role_refs, Prepared};
use super::{apply_outcome, refuse_marker, ApplyCtx, ApplyResult};

/// The §6.7 follow-up clause built from the parent window's authored role
/// refs, PROJECTED onto the candidate verb's declared slots: a role the
/// candidate verb does not declare is dropped (§6.7 — the synthesized clause
/// must be a shape a player step could produce, and a player step cannot bind
/// an undeclared role; the dropped cards therefore never lock —
/// `register_windows` locks what the clause carries). Declared roles inherit
/// unchanged: actors/objects/with carry their lists; at/about/to keep their
/// 0-or-1 arrays (the log row's shape). A same-verb candidate declares every
/// role the parent validated against, so the projection is the identity
/// there. The follow-up re-resolves the surviving roles at its own completion
/// (§6.7), exactly as any player-begun window does. The parent window's own
/// refs and locks are untouched.
fn refs_to_clause(verb: &CompiledVerb, refs: &IndexMap<Role, Vec<Ref>>) -> Clause {
    let declared = |role: Role| verb.slot_by_role.contains_key(role.as_str());
    Clause {
        actors: if declared(Role::Actors) {
            refs.get(&Role::Actors).cloned().unwrap_or_default()
        } else {
            Vec::new()
        },
        at: if declared(Role::At) {
            refs.get(&Role::At).and_then(|v| v.first().cloned())
        } else {
            None
        },
        verb: verb.name.clone(),
        to: if declared(Role::To) {
            refs.get(&Role::To).and_then(|v| v.first().cloned())
        } else {
            None
        },
        objects: if declared(Role::Object) {
            refs.get(&Role::Object).cloned().unwrap_or_default()
        } else {
            Vec::new()
        },
        with: if declared(Role::With) {
            refs.get(&Role::With).cloned().unwrap_or_default()
        } else {
            Vec::new()
        },
        about: if declared(Role::About) {
            refs.get(&Role::About).and_then(|v| v.first().cloned())
        } else {
            None
        },
    }
}

/// RUN (§6.7) — a completion may chain into named follow-up recipes. The
/// carrying window's completion walks its candidates IN ORDER and pins the
/// FIRST whose `when:` matches the post-outcome draft — evaluated LENIENT,
/// `strict: false`, exactly like selection (§6.4), against the draft AND the
/// parent window's committed role bindings with the projected clause's own
/// bindings winning where the candidate verb declares the role (E8: the
/// cross-verb gate reads the parent situation; same-verb chains are
/// unchanged) — and whose `max` budget
/// (§6.4) is not spent; it then opens ONE follow-up window on that recipe
/// with the parent's committed role bindings, beginning at the completion
/// clock. Zero-concrete: RUN logs NOTHING of itself — the follow-up's BEGIN
/// row (synthesized here through the player-step record shape, registered by
/// fire_window through the ONE registration path once the parent has left the
/// table) is the record. No candidate matches: nothing opens — the chain
/// ends, silently (CS's chain ending, not a refusal), and the chain SLOT
/// goes unspent: a later RUN in the same completion still gets its walk
/// (fall-through; the slot is earned by a pin). The pinned recipe's
/// top-level REFUSE still gates inline against the draft — a Phase-3-shaped
/// trial whose refusal rejects the PARENT window (§6.7's window-refuse); a
/// SECOND PINNING RUN in one completion refuses the parent window at that
/// outcome (a station holds one window). §6.6: in an EVENT body (the
/// `event` channel) a chain refusal is a SILENT SKIP — an event has no step
/// to refuse — so the second pinning op ends its chain there and a
/// gate-vetoed branch opens nothing, no marker, the body continues.
///
/// §6.7: candidates resolve against the GLOBAL recipe table in every home —
/// recipe bodies and `on complete` bodies alike (ids are globally unique by
/// a load error, so the table is unambiguous) — and any busy station other
/// than the parent's own window skips the candidate silently (a same-verb
/// hand-off must not see its own station busy; a cross-verb hand-off onto a
/// busy station skips the same way). Recipe-body RUN ops reached while
/// `deferring` is set park on the chain instead: the completion fires its
/// `on complete` triggers AFTER the recipe's own outcomes and BEFORE any
/// RUN, and the parked ops replay in list order afterwards.
pub(super) fn apply_run(o: &Outcome, ctx: &mut ApplyCtx) -> Result<ApplyResult> {
    let args = &o.args;
    // §6.7: a recipe-body RUN during the deferral window parks here — the
    // slot is pin-earned, so parking consumes nothing, zero-concrete
    if ctx.chain.as_ref().is_some_and(|c| c.deferring) {
        if let Some(c) = ctx.chain.as_mut() {
            c.deferred.push(o.clone());
        }
        return Ok(ApplyResult { concrete: Vec::new(), refused: false });
    }
    // snapshot the parent-window identity off the chain channel; a None
    // channel is unreachable on every authored path (the Phase-3 trial skips
    // the chain ops, and the log never carries one for the fold to replay)
    let (parent_key, parent_end, next_record_id, parent_refs) = match ctx.chain.as_ref() {
        Some(c) => (c.parent_key.clone(), c.parent_end, c.next_record_id, c.parent_refs.clone()),
        None => {
            return Err(ChorusError::new("internal: RUN applied outside a chain channel (§6.7)"));
        }
    };
    if ctx.chain.as_ref().is_some_and(|c| c.run_taken) {
        // §6.6: in an event body the refusal is a SILENT SKIP — an event has
        // no step to refuse, so the second chain op just ends its chain
        // there (the first op's effects stand, nothing logs)
        if ctx.chain.as_ref().is_some_and(|c| c.event) {
            return Ok(ApplyResult { concrete: Vec::new(), refused: false });
        }
        return Ok(ApplyResult {
            concrete: vec![refuse_marker(
                "a second RUN in one completion — a station holds one window (§6.7)".to_string(),
            )],
            refused: true,
        });
    }
    // the candidate walk IN ORDER. Candidates were load-checked against the
    // GLOBAL table (§6.7), so an unresolved id is impossible in compiled
    // content — skip rather than throw, keeping that invariant's failure
    // shape silent instead of a masked drift error. The chain slot is
    // earned by a PIN, not by a reach (§6.7): a whiffed list spends
    // nothing, so a later RUN in this same completion gets its own walk;
    // two pinning chain ops still refuse.
    let mut pinned: Option<(&str, &CompiledRecipe)> = None;
    for a in args {
        let id = match a {
            Arg::Ref { r#ref: Ref::Name { name, .. } } => name,
            _ => continue,
        };
        let Some((cand_verb_name, cand)) = candidate_of(ctx.content, id) else {
            continue;
        };
        // §6.7: a busy follow-up station skips the candidate silently —
        // every open window on the candidate's verb EXCEPT the parent's own
        // (which is still in the table mid-completion; the same-verb
        // hand-off must not see itself busy)
        if ctx.state.windows.values().any(|w| w.verb == cand_verb_name && w.key != parent_key) {
            continue;
        }
        // §6.4: the execution budget — an exhausted candidate is simply
        // non-matching; the walk continues. Read from the draft's state map
        // (§6.6 posture), which already carries the parent's own begin.
        if let Some(max) = cand.max {
            if ctx.state.executed.get(&cand.id).is_some_and(|&n| n >= max) {
                continue;
            }
        }
        // the `when:` IS re-evaluated — the two-binding-layer gate semantics
        // (§6.7's walk, E8) live on run_gate_matches
        if !run_gate_matches(ctx, cand, cand_verb_name, &parent_refs)?.unwrap_or(false) {
            continue;
        }
        pinned = Some((cand_verb_name, cand));
        // the PIN earns the slot (§6.7): from here a later chain op in this
        // same completion refuses
        if let Some(c) = ctx.chain.as_mut() {
            c.run_taken = true;
        }
        break;
    }
    let Some((cand_verb_name, cand)) = pinned else {
        // no candidate matches: the chain ends silently — no window, no
        // refusal, no log
        return Ok(ApplyResult { concrete: Vec::new(), refused: false });
    };
    if let Some(marker) = run_inline_gate(ctx, cand, parent_end) {
        // the refusal rejects the PARENT window: the COMPLETE row carries the
        // marker, the partial effects die with the scratch, no follow-up.
        // §6.6: in an event body there is no window to refuse — the vetoed
        // branch is skipped silently (the busy-skip discipline), no marker,
        // the body continues.
        if ctx.chain.as_ref().is_some_and(|c| c.event) {
            return Ok(ApplyResult { concrete: Vec::new(), refused: false });
        }
        return Ok(ApplyResult { concrete: vec![marker], refused: true });
    }
    // synthesize the follow-up BEGIN — the same shape a player step produces
    // (make_begin_record over one prepared clause): the PINNED CANDIDATE's
    // verb (§6.7: the follow-up may live on another verb than the parent),
    // the parent window's authored role refs projected onto that verb's
    // declared slots (refs_to_clause), beginning at the completion clock. The
    // pinned recipe's PROGRESS ticks ride it, like any BEGIN.
    let cand_verb = ctx.content.verbs.get(cand_verb_name);
    let cand_verb = match cand_verb {
        Some(v) => v,
        None => {
            return Err(ChorusError::new(format!(
                "internal: RUN's candidate verb '{cand_verb_name}' left the compiled content (§6.7)"
            )));
        }
    };
    let begin =
        synthesize_followup(ctx.dict, cand_verb, cand, &parent_refs, parent_end, next_record_id)?;
    if let Some(c) = ctx.chain.as_mut() {
        c.extra_records.push(begin);
    }
    Ok(ApplyResult {
        concrete: Vec::new(), // zero-concrete: the follow-up's BEGIN row is the record
        refused: false,
    })
}

/// FORK (§6.7) — the multi-continuation chain: open ONE follow-up window
/// for EVERY matching candidate, in list order (RUN stays the ordered
/// first-match walk; FORK is the fan-out). The walk shares RUN's helpers and
/// gates — resolution (the global table), the busy check (the parent's own
/// window excluded), `max`, the merged `when:` — under two fork-local rules:
/// ONE BRANCH PER STATION (a candidate whose verb already took a branch in
/// THIS fork skips silently — the busy check reads the parent's window
/// table, where the fork's own future windows do not yet exist), and
/// DUPLICATE IDS SKIP SILENTLY (`FORK "a", "a"` opens one window). The
/// fork-local rules make list order the only decider, and FORK mutates no
/// state during the walk, so the branches are order-free.
///
/// Slot rule (§6.7): a fork that pinned ≥1 branch earns the slot — a later
/// chain op in this completion refuses the parent window; a fork that
/// pinned nothing consumes no slot (fall-through). The inline gate is
/// ATOMIC in recipe bodies: after the walk, each pinned branch's top-level
/// REFUSE trials on a scratch in list order and the FIRST refusing gate
/// rejects the PARENT window with NO branch opening — the completion draft
/// is atomic (§6.7), and a partial fork (some windows open, the completion
/// refused) is not a state any rule can describe. §6.6: in an EVENT body
/// (the `event` channel) a chain refusal is a SILENT SKIP — an event has no
/// step to refuse — so the second pinning op ends its chain there and a
/// gate-vetoed branch skips alone, the remaining branches opening.
/// Zero-concrete like RUN: each pinned branch synthesizes its own BEGIN row
/// (ids `complete_id + 1 … + n`, `next_record_id` incremented per branch);
/// nothing is logged of the FORK op itself.
pub(super) fn apply_fork(o: &Outcome, ctx: &mut ApplyCtx) -> Result<ApplyResult> {
    let args = &o.args;
    // §6.7: a recipe-body FORK parks during the deferral window exactly like
    // a RUN — the completion fires its `on complete` triggers first
    if ctx.chain.as_ref().is_some_and(|c| c.deferring) {
        if let Some(c) = ctx.chain.as_mut() {
            c.deferred.push(o.clone());
        }
        return Ok(ApplyResult { concrete: Vec::new(), refused: false });
    }
    // a None channel is unreachable on every authored path (the Phase-3
    // trial skips the chain ops, and the log never carries one for the fold
    // to replay)
    let (parent_key, parent_end, mut next_record_id, parent_refs) = match ctx.chain.as_ref() {
        Some(c) => (c.parent_key.clone(), c.parent_end, c.next_record_id, c.parent_refs.clone()),
        None => {
            return Err(ChorusError::new("internal: FORK applied outside a chain channel (§6.7)"));
        }
    };
    if ctx.chain.as_ref().is_some_and(|c| c.run_taken) {
        // §6.6: in an event body the refusal is a SILENT SKIP — an event has
        // no step to refuse, so the second chain op just ends its chain
        // there (the first op's effects stand, nothing logs)
        if ctx.chain.as_ref().is_some_and(|c| c.event) {
            return Ok(ApplyResult { concrete: Vec::new(), refused: false });
        }
        return Ok(ApplyResult {
            concrete: vec![refuse_marker(
                "a second chain op in one completion — a station holds one window (§6.7)"
                    .to_string(),
            )],
            refused: true,
        });
    }
    // the fork walk: ALL candidates, in list order, against the SAME
    // post-outcome draft. Candidates were load-checked against the GLOBAL
    // table (§6.7), so an unresolved id is impossible in compiled content —
    // skip rather than throw, keeping that invariant's failure shape silent
    // instead of a masked drift error.
    let mut branches: Vec<(&str, &CompiledRecipe)> = Vec::new();
    for a in args {
        let id = match a {
            Arg::Ref { r#ref: Ref::Name { name, .. } } => name,
            _ => continue,
        };
        let Some((cand_verb_name, cand)) = candidate_of(ctx.content, id) else {
            continue;
        };
        // one branch per station — and a duplicated id is the same verb
        if branches.iter().any(|(v, _)| *v == cand_verb_name) {
            continue;
        }
        // §6.7: a busy follow-up station skips the candidate silently —
        // every open window on the candidate's verb EXCEPT the parent's own
        if ctx.state.windows.values().any(|w| w.verb == cand_verb_name && w.key != parent_key) {
            continue;
        }
        // §6.4: the execution budget — an exhausted candidate is simply
        // non-matching; the walk continues
        if let Some(max) = cand.max {
            if ctx.state.executed.get(&cand.id).is_some_and(|&n| n >= max) {
                continue;
            }
        }
        if !run_gate_matches(ctx, cand, cand_verb_name, &parent_refs)?.unwrap_or(false) {
            continue;
        }
        branches.push((cand_verb_name, cand));
    }
    if branches.is_empty() {
        // nothing pinned: no window, no refusal, and the slot goes unspent
        // (fall-through, §6.7) — a following chain op composes
        return Ok(ApplyResult { concrete: Vec::new(), refused: false });
    }
    // the PIN earns the slot (§6.7): from here a later chain op in this
    // same completion refuses
    if let Some(c) = ctx.chain.as_mut() {
        c.run_taken = true;
    }
    // the inline gate (§6.7): each pinned branch's top-level REFUSE trials
    // on a scratch, in list order. Recipe bodies are ATOMIC — the FIRST
    // refusal rejects the PARENT window and NO branch opens (the completion
    // draft is atomic, §6.7). §6.6: an event body has no window to refuse —
    // a vetoed branch is skipped silently (the busy-skip discipline) and the
    // remaining branches open.
    let event = ctx.chain.as_ref().is_some_and(|c| c.event);
    let mut marker: Option<Outcome> = None;
    let mut open: Vec<&(&str, &CompiledRecipe)> = Vec::with_capacity(branches.len());
    for branch in &branches {
        if let Some(m) = run_inline_gate(ctx, branch.1, parent_end) {
            if event {
                continue; // the vetoed branch skips silently (§6.6)
            }
            marker = Some(m);
            break;
        }
        open.push(branch);
    }
    if let Some(marker) = marker {
        return Ok(ApplyResult { concrete: vec![marker], refused: true });
    }
    // synthesize one BEGIN per surviving branch, in list order — the ids run
    // `complete_id + 1 … + n`, registered by fire_window through the ONE
    // registration path once the parent has left the table
    for (cand_verb_name, cand) in open {
        let cand_verb = ctx.content.verbs.get(*cand_verb_name);
        let cand_verb = match cand_verb {
            Some(v) => v,
            None => {
                return Err(ChorusError::new(format!(
                    "internal: FORK's candidate verb '{cand_verb_name}' left the compiled content (§6.7)"
                )));
            }
        };
        let begin = synthesize_followup(
            ctx.dict,
            cand_verb,
            cand,
            &parent_refs,
            parent_end,
            next_record_id,
        )?;
        next_record_id = next_record_id.saturating_add(1);
        if let Some(c) = ctx.chain.as_mut() {
            c.extra_records.push(begin);
            c.next_record_id = c.next_record_id.saturating_add(1);
        }
    }
    Ok(ApplyResult {
        concrete: Vec::new(), // zero-concrete: the branches' BEGIN rows are the record
        refused: false,
    })
}

/// Resolve the candidate's owning verb + recipe — the GLOBAL id table, in
/// every home (the collision guard keeps it unambiguous).
fn candidate_of<'a>(
    content: &'a CompiledContent,
    id: &str,
) -> Option<(&'a str, &'a CompiledRecipe)> {
    let vn = content.recipe_verbs.get(id)?;
    let v = content.verbs.get(vn)?;
    match v.recipes.iter().find(|r| r.id == *id) {
        Some(r) => Some((v.name.as_str(), r)),
        None => None,
    }
}

/// The candidate's `when:` re-evaluated LENIENT against the post-outcome
/// draft and the hand-off's two binding layers. `Ok(None)` = the candidate's
/// verb left the compiled content — unresolved drift walks on (§9.2 posture).
fn run_gate_matches(
    ctx: &ApplyCtx,
    cand: &CompiledRecipe,
    cand_verb_name: &str,
    parent_refs: &IndexMap<Role, Vec<Ref>>,
) -> Result<Option<bool>> {
    // the `when:` IS re-evaluated — CS's linked hand-off is
    // requirement-disambiguated (the dream-router shape), so a
    // pinned-id-no-check rule would make them inexpressible. Lenient,
    // exactly like selection (§6.4): `roll` never binds on a `when:`.
    //
    // The gate reads the draft AND the hand-off's two binding layers
    // (§6.7's walk, E8): the PARENT window's committed bindings first,
    // overlaid with the projected clause's own — where the candidate
    // verb DECLARES the role the projected binding wins (it is what the
    // follow-up will carry, resolved against the post-outcome draft,
    // dead refs dropping like every completion resolution); where the
    // projection drops the role the parent window's binding stands, so
    // a cross-verb gate reads the parent situation (the CS linked
    // semantics). Same-verb chains keep their ordinary matches — the
    // projection is the identity over the declared roles — but the
    // declared-role bindings CHANGE SOURCE: they now come from the
    // projected clause re-resolved against the post-outcome draft, so
    // a same-verb gate whose bound card the parent's own outcomes
    // replaced or killed reads the follow-up's truth, not the
    // begin-time id snapshot. A role the projection drops is
    // GATE-VISIBLE ONLY: the gate can read it here, but the follow-up
    // never carries it — an outcome SET over that unbound role expands
    // to no targets and silently no-ops (the §4.4 role-ref law). The
    // merged bindings are EVAL CONTEXT ONLY — the synthesized BEGIN row
    // still carries the projected clause alone (§6.7 concretization).
    let matched = match &cand.when {
        None => Some(true),
        Some(w) => {
            let cand_verb = match ctx.content.verbs.get(cand_verb_name) {
                Some(v) => v,
                None => return Ok(None), // unresolved drift walks on (§9.2 posture)
            };
            let projected = clause_refs(&refs_to_clause(cand_verb, parent_refs));
            let mut merged: RoleBindings = ctx.bindings.clone();
            for role in ROLES {
                if !cand_verb.slot_by_role.contains_key(role.as_str()) {
                    continue; // the projection drops the role: the parent binding stands
                }
                let ids = resolve_role_refs(ctx.state, &projected[&role])?;
                merged.insert(role, ids);
            }
            let pctx = PredCtx { b: &merged, state: ctx.state, strict: false, roll: None };
            Some(eval_pred(w, &pctx)?)
        }
    };
    Ok(matched)
}

/// The guard survives: the pinned recipe's top-level REFUSE gates INLINE
/// against the draft — a Phase-3-shaped trial on a scratch (PROGRESS
/// skipped, IF opaque; the pinned body's own chain ops belong to the
/// follow-up's completion — one walk per completion; scratch rng/id
/// consumption dies with the clone). Only a REFUSE is decisive; an
/// application ERROR is not a gate — it defers to the follow-up's own
/// completion (§6.6's table-trial discipline). Returns the REFUSE marker
/// when the gate refused the parent window.
fn run_inline_gate(ctx: &ApplyCtx, cand: &CompiledRecipe, parent_end: i64) -> Option<Outcome> {
    let mut marker: Option<Outcome> = None;
    {
        let mut trial = clone_state(ctx.state);
        let mut tn: i64 = 0;
        let tag = ctx.record_id;
        let mut tgen = move || {
            tn += 1;
            format!("t{tag}_{tn}")
        };
        let mut tctx = ApplyCtx {
            state: &mut trial,
            dict: ctx.dict,
            content: ctx.content,
            bindings: ctx.bindings.clone(),
            now: parent_end,
            record_id: ctx.record_id,
            id_gen: &mut tgen,
            extra_advance_min: 0,
            roll: None,
            chain: None,
        };
        for o in &cand.out {
            if o.op == "PROGRESS" || o.op == "IF" || o.op == "RUN" || o.op == "FORK" {
                continue;
            }
            let res = match apply_outcome(o, &mut tctx) {
                Ok(res) => res,
                Err(_) => break, // not a gate — the follow-up's completion owns it
            };
            if res.refused {
                marker = Some(res.concrete.into_iter().find(|c| c.op == "REFUSE").unwrap_or_else(
                    || refuse_marker("the follow-up gate refused (§6.7)".to_string()),
                ));
                break;
            }
        }
    }
    marker
}

/// Build the follow-up BEGIN row (owned, not yet registered): make_begin_record
/// over one prepared clause carrying the projected clause, the pinned
/// candidate's recipe and its PROGRESS ticks. Registration happens back in
/// fire_window, through the ONE registration path play and fold share.
fn synthesize_followup(
    dict: &Dictionary,
    cand_verb: &CompiledVerb,
    cand: &CompiledRecipe,
    parent_refs: &IndexMap<Role, Vec<Ref>>,
    parent_end: i64,
    next_record_id: i64,
) -> Result<ActionRecord> {
    let clause = refs_to_clause(cand_verb, parent_refs);
    let progress: Vec<Outcome> = cand.out.iter().filter(|o| o.op == "PROGRESS").cloned().collect();
    let step = Step {
        start_min: Some(parent_end),
        clock: None,
        clauses: vec![clause.clone()],
        t_real: None,
    };
    let prepared =
        [Prepared { clause: &clause, verb: cand_verb, refs: clause_refs(&clause), recipe: cand }];
    make_begin_record(next_record_id, parent_end, &step, &prepared, progress, dict)
}
