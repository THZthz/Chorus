//! The §6.7 RUN chain: a completion chaining into named follow-up recipes —
//! the in-order candidate walk, the inline top-level-REFUSE gate, and the
//! synthesized follow-up BEGIN (the player-step record shape).

use indexmap::IndexMap;

use crate::content::{CompiledRecipe, CompiledVerb};
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
/// ends, silently (CS's chain ending, not a refusal). The pinned recipe's
/// top-level REFUSE still gates inline against the draft — a Phase-3-shaped
/// trial whose refusal rejects the PARENT window (§6.7's window-refuse); a
/// SECOND RUN op reached in one completion refuses the parent window at that
/// outcome (a station holds one window).
///
/// §6.7: a RUN reached inside an `on complete` body (the chain's
/// `in_complete` phase) resolves candidates against the GLOBAL recipe table
/// — cross-verb legal — and any busy station other than the parent's own
/// window skips the candidate silently (a same-verb hand-off must not see
/// its own station busy; the ordering hazard is specified away, not left to
/// implementation order). Recipe-body RUN ops reached while `deferring` is
/// set park on the chain instead: the completion fires its `on complete`
/// triggers AFTER the recipe's own outcomes and BEFORE any RUN, and the
/// parked ops replay in list order afterwards.
pub(super) fn apply_run(o: &Outcome, ctx: &mut ApplyCtx) -> Result<ApplyResult> {
    let args = &o.args;
    // §6.7: a recipe-body RUN during the deferral window parks here — not
    // yet REACHED (the one-chain-slot detector stays untouched), zero-concrete
    if ctx.chain.as_ref().is_some_and(|c| c.deferring) {
        if let Some(c) = ctx.chain.as_mut() {
            c.deferred.push(o.clone());
        }
        return Ok(ApplyResult { concrete: Vec::new(), refused: false });
    }
    // snapshot the parent-window identity off the chain channel; a None
    // channel is unreachable on every authored path (slot/expire triggers
    // reject RUN at load, the Phase-3 trial skips it, and the log never
    // carries one for the fold to replay)
    let (parent_verb_name, parent_key, parent_end, next_record_id, parent_refs, in_complete) =
        match ctx.chain.as_ref() {
            Some(c) => (
                c.parent_verb.clone(),
                c.parent_key.clone(),
                c.parent_end,
                c.next_record_id,
                c.parent_refs.clone(),
                c.in_complete,
            ),
            None => {
                return Err(ChorusError::new(
                    "internal: RUN applied outside a window completion (§6.7)",
                ));
            }
        };
    if ctx.chain.as_ref().is_some_and(|c| c.run_taken) {
        return Ok(ApplyResult {
            concrete: vec![refuse_marker(
                "a second RUN in one completion — a station holds one window (§6.7)".to_string(),
            )],
            refused: true,
        });
    }
    // the op is REACHED — from here a later RUN in this same completion
    // refuses, even when this one pins nothing
    if let Some(c) = ctx.chain.as_mut() {
        c.run_taken = true;
    }
    let parent_verb = ctx.content.verbs.get(&parent_verb_name);
    let parent_verb = match parent_verb {
        Some(v) => v,
        None => {
            return Err(ChorusError::new(format!(
                "internal: RUN's parent verb '{parent_verb_name}' left the compiled content (§6.7)"
            )));
        }
    };
    // the candidate walk IN ORDER. Recipe-body candidates were load-checked
    // against this very list (§6.7's walk), so an unresolved id is impossible
    // in compiled content — skip rather than throw, keeping that invariant's
    // failure shape silent instead of a masked drift error. Completion-trigger
    // candidates (§6.7) were load-checked against the GLOBAL table, which the
    // collision guard keeps unambiguous; resolution consults it here.
    let mut pinned: Option<(&str, &CompiledRecipe)> = None;
    for a in args {
        let id = match a {
            Arg::Ref { r#ref: Ref::Name { name, .. } } => name,
            _ => continue,
        };
        let Some((cand_verb_name, cand)) = candidate_of(ctx, parent_verb, id, in_complete) else {
            continue;
        };
        // §6.7: a busy follow-up station skips the candidate silently —
        // every open window on the candidate's verb EXCEPT the parent's own
        // (which is still in the table mid-completion; the same-verb hand-off
        // must not see itself busy)
        if in_complete
            && ctx.state.windows.values().any(|w| w.verb == cand_verb_name && w.key != parent_key)
        {
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
        break;
    }
    let Some((cand_verb_name, cand)) = pinned else {
        // no candidate matches: the chain ends silently — no window, no
        // refusal, no log
        return Ok(ApplyResult { concrete: Vec::new(), refused: false });
    };
    if let Some(marker) = run_inline_gate(ctx, cand, parent_end) {
        // the refusal rejects the PARENT window: the COMPLETE row carries the
        // marker, the partial effects die with the scratch, no follow-up
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

/// Resolve the candidate's owning verb + recipe.
fn candidate_of<'a>(
    ctx: &'a ApplyCtx,
    parent_verb: &'a CompiledVerb,
    id: &str,
    in_complete: bool,
) -> Option<(&'a str, &'a CompiledRecipe)> {
    if in_complete {
        // §6.7: the global table owns the id (the collision guard keeps
        // it unambiguous)
        let vn = ctx.content.recipe_verbs.get(id)?;
        let v = ctx.content.verbs.get(vn)?;
        match v.recipes.iter().find(|r| r.id == *id) {
            Some(r) => Some((v.name.as_str(), r)),
            None => None,
        }
    } else {
        parent_verb.recipes.iter().find(|r| r.id == *id).map(|r| (parent_verb.name.as_str(), r))
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
/// skipped, IF opaque; the pinned body's own RUN belongs to the follow-up's
/// completion — one walk per completion; scratch rng/id consumption dies
/// with the clone). Only a REFUSE is decisive; an application ERROR is not
/// a gate — it defers to the follow-up's own completion (§6.6's
/// table-trial discipline). Returns the REFUSE marker when the gate refused
/// the parent window.
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
            if o.op == "PROGRESS" || o.op == "IF" || o.op == "RUN" {
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
