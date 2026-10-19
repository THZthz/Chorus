//! Validation and selection (spec §6.1, §6.4, §6.7): slot validation,
//! first-match recipe selection, ref resolution (the §4.2 oldest-pool rule),
//! the §6.7 station/card busy checks, and the display form of a ref.

use std::collections::HashSet;

use indexmap::IndexMap;

use crate::content::{CompiledRecipe, CompiledVerb};
use crate::core::clock::clock_from_abs_min;
use crate::core::predicate::{eval_pred, PredCtx, RoleBindings};
use crate::core::query::match_query;
use crate::core::r#ref::Ref;
use crate::core::roles::{Role, ROLES};
use crate::core::sentence::Clause;
use crate::core::types::{get_alive, oldest_named, Entity, State};
use crate::error::{ChorusError, Result};

// ---------- role binding & slot validation (§6.1) ----------

/// One validated, recipe-selected clause.
pub(super) struct Prepared<'a> {
    pub(super) clause: &'a Clause,
    pub(super) verb: &'a CompiledVerb,
    pub(super) refs: IndexMap<Role, Vec<Ref>>,
    pub(super) recipe: &'a CompiledRecipe,
}

/// Clause refs per role; the IndexMap keys follow the ROLES declaration
/// order for the `refs[role]` lookups.
pub(super) fn clause_refs(c: &Clause) -> IndexMap<Role, Vec<Ref>> {
    let mut m = IndexMap::new();
    m.insert(Role::Actors, c.actors.clone());
    m.insert(Role::Object, c.objects.clone());
    m.insert(Role::With, c.with.clone());
    m.insert(Role::At, c.at.iter().cloned().collect());
    m.insert(Role::About, c.about.iter().cloned().collect());
    m.insert(Role::To, c.to.iter().cloned().collect());
    m
}

/// Slot validation. Error texts come WITHOUT the `<verb>: ` prefix; the
/// caller prepends it. Q4: slot roles are never validated — `slot_by_role`
/// is keyed by the RAW role string, so a typo'd slot silently never matches
/// (frozen).
pub(super) fn validate_slots(
    verb: &CompiledVerb,
    refs: &IndexMap<Role, Vec<Ref>>,
    state: &State,
) -> Result<RoleBindings> {
    let mut bindings = RoleBindings::new();
    for role in ROLES {
        let provided = &refs[&role];
        let slot = verb.slot_by_role.get(role.as_str());
        let Some(slot) = slot else {
            if !provided.is_empty() {
                return Err(ChorusError::new(format!(
                    "{} does not accept '{}'",
                    verb.name,
                    role.as_str()
                )));
            }
            bindings.insert(role, Vec::new());
            continue;
        };
        let len = provided.len() as i64;
        // an unbounded slot (max = None) has no upper bound, so it never
        // trips the over-max check
        if len < slot.min || slot.max.is_some_and(|max| len > max) {
            let range = match slot.max {
                None => format!("{}..", slot.min),
                Some(max) => format!("{}..{max}", slot.min),
            };
            return Err(ChorusError::new(format!(
                "{}: '{}' takes {range}, got {len}",
                verb.name,
                role.as_str()
            )));
        }
        // §6.8: the bindings store IDS — mid-list DESTROY must be visible
        // to every later read through entities.get (dead-stamp semantics)
        let mut placed: HashSet<String> = HashSet::new();
        let mut ids: Vec<String> = Vec::with_capacity(provided.len());
        for r in provided {
            let Some(e) = resolve_ref(state, r)? else {
                return Err(ChorusError::new(format!("unresolvable ref {}", show(r))));
            };
            if placed.contains(&e.id) {
                return Err(ChorusError::new(format!(
                    "{} is already placed in '{}'",
                    show(r),
                    role.as_str()
                )));
            }
            placed.insert(e.id.clone());
            if let Some(query) = &slot.query {
                if !match_query(e, query) {
                    return Err(ChorusError::new(format!(
                        "{} does not fit slot '{}' of {}",
                        show(r),
                        role.as_str(),
                        verb.name
                    )));
                }
            }
            ids.push(e.id.clone());
        }
        bindings.insert(role, ids);
    }
    Ok(bindings)
}

/// Resolve a ref to a LIVING entity (`None` = dead, unknown, or kind
/// mismatch). A param ref here means an expansion bug
/// (§10) — the error propagates uncaught through every caller (never a
/// rejection). The one documented merge point
/// is execute_step's Phase-2 `validate_slots` fold (unreachable input — see
/// the channel note there).
pub(super) fn resolve_ref<'a>(state: &'a State, r: &Ref) -> Result<Option<&'a Entity>> {
    match r {
        Ref::Id { id } => Ok(get_alive(state, id)),
        Ref::Param { .. } => {
            Err(ChorusError::new("internal: a function parameter ref survived expansion"))
        }
        // §4.2 pool addressing: a name held by N > 1 living copies addresses
        // the OLDEST copy — the HEAD of the `by_name` pool (insertion order,
        // never id sort)
        Ref::Name { name, kind } => {
            let Some(e) = oldest_named(state, name) else {
                return Ok(None);
            };
            if let Some(k) = kind {
                if &e.kind != k {
                    return Ok(None);
                }
            }
            Ok(Some(e))
        }
    }
}

/// Display form of a ref: `#id` for ids, the bare name otherwise.
pub(super) fn show(r: &Ref) -> String {
    match r {
        Ref::Id { id } => format!("#{id}"),
        Ref::Name { name, .. } | Ref::Param { name } => name.clone(),
    }
}

/// Resolve one role's refs to entity IDS against `state` — the ONE walk
/// shared by the Phase-3 table trial, apply_run's gate, fire_window's
/// re-resolution and fold_bindings: dead/unknown refs drop, errors
/// propagate. The ids come back (not live entity handles) and each caller
/// re-gets the entity, so a mid-list DESTROY is seen by every later read
/// (dead-stamp rule, §6.8).
pub(super) fn resolve_role_refs(state: &State, refs: &[Ref]) -> Result<Vec<String>> {
    let mut ids = Vec::with_capacity(refs.len());
    for r in refs {
        if let Some(e) = resolve_ref(state, r)? {
            ids.push(e.id.clone());
        }
    }
    Ok(ids)
}

// ---------- recipe selection (§6.4): first match wins ----------

/// First-match recipe selection; `None` = "no recipe matched" — the caller
/// owns the message text.
pub(super) fn select_recipe<'a>(
    verb: &'a CompiledVerb,
    bindings: &RoleBindings,
    state: &State,
) -> Result<Option<&'a CompiledRecipe>> {
    let ctx = PredCtx { b: bindings, state, strict: false, roll: None };
    for r in &verb.recipes {
        // §6.4: the execution budget — a recipe whose count has reached its
        // `max N` is simply non-matching; the walk continues. The count is
        // read from the state map only (§6.6 posture), so a max change
        // under an old log shifts FUTURE selection only (§9.2).
        //
        // §6.4, the multi-clause draft-lag: selection reads ONE post-advance
        // draft for every clause of the step, while registration (and the
        // count it carries) lands in Phase 4 — so two clauses of one step can
        // both pin the same max-1 recipe (count +2). Permitted by the
        // ruling's letter (a registered window increments each clause's id
        // ON THE DRAFT); stated so neither side is later "fixed" into a
        // mid-step re-selection.
        if let Some(max) = r.max {
            if state.executed.get(&r.id).is_some_and(|&n| n >= max) {
                continue;
            }
        }
        match &r.when {
            None => return Ok(Some(r)),
            Some(w) => {
                if eval_pred(w, &ctx)? {
                    return Ok(Some(r));
                }
            }
        }
    }
    Ok(None)
}

/// Roles a card is COMMITTED to; the
/// actors stay free (the player is never locked, or no two verbs could ever
/// run in parallel)
pub(super) const COMMITTED_ROLES: [Role; 5] =
    [Role::Object, Role::With, Role::At, Role::About, Role::To];

/// §6.7 v0.5 lock checks, against the post-advance draft: one window per
/// verb station; committed cards busy while any open window holds them.
/// First match wins (the iteration order over `windows` is observable
/// through this message).
pub(super) fn busy_reason(
    state: &State,
    verb: &str,
    refs: &IndexMap<Role, Vec<Ref>>,
) -> Result<Option<String>> {
    for w in state.windows.values() {
        if w.verb == verb {
            let end = clock_from_abs_min(w.end);
            return Ok(Some(format!(
                "{verb} is already processing — its station frees at {}:{}",
                end.day, end.tick
            )));
        }
    }
    for role in COMMITTED_ROLES {
        for r in &refs[&role] {
            let Some(e) = resolve_ref(state, r)? else {
                continue;
            };
            for w in state.windows.values() {
                if w.locked.iter().any(|id| id == &e.id) {
                    let end = clock_from_abs_min(w.end);
                    // underscores render as spaces in the busy message
                    let name = e.name.replace('_', " ");
                    return Ok(Some(format!(
                        "{name} is busy with {} — it frees at {}:{}",
                        w.verb, end.day, end.tick
                    )));
                }
            }
        }
    }
    Ok(None)
}
