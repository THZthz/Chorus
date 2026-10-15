//! The interpreter (spec §6.6: validation → selection → application → time
//! → events, with step-atomic REFUSE (§6.5) and event-sourced records (§7)).
//!
//! Ownership model: [`ApplyCtx`] holds `&mut State`; bindings hold entity
//! IDS (`RoleBindings = IndexMap<Role, Vec<String>>`), so every read goes
//! through `state.entities.get(&id)` and sees the CURRENT `dead_action`
//! (the dead-stamp rule); `expand` resolves to ids and the caller re-gets
//! each entity, which splits the `&`/`&mut` borrows cleanly. No
//! Rc/Arc/RefCell/Cell anywhere.

use std::collections::HashSet;

use indexmap::IndexMap;
use serde::{Deserialize, Serialize};

use crate::content::content::{draw_from, CompiledContent, CompiledRecipe, CompiledTrigger, CompiledVerb, DeckCard};
use crate::core::clock::{
    abs_min, clock_from_abs_min, format_clock, parse_clock, parse_duration, Clock, HALFHOURS_PER_DAY, MIN_PER_HALFHOUR,
};
use crate::core::dice::roll_die;
use crate::core::dict::Dictionary;
use crate::core::format::format_sentence;
use crate::core::parse::parse_sentence;
use crate::core::predicate::{eval_expr, eval_pred, PredCtx, RoleBindings};
use crate::core::query::match_query;
use crate::core::r#ref::{id_ref, name_ref, sort_refs, Ref};
use crate::core::roles::{Role, ROLES};
use crate::core::sentence::{Arg, Clause, IfArm, IntoTarget, Outcome, Sentence, SentenceKind};
use crate::core::types::{
    add_entity, aspect_entry, aspect_get, aspect_set, clone_state, get_alive, oldest_named, relation_key,
    release_name_entry, AspectEntry, AspectVal, Aspects, AttrValue, Attrs, Entity, Ending, OpenWindow, Relation, State,
};
use crate::js::{
    clone_map_in_js_order, jsjson, jsjson_obj_in_js_order, ordinary_own_property_keys, ChorusError, JsJson, Result,
};

/// The step input: a quantized `clock` and/or a minute-precise `start_min`,
/// plus the parsed clauses. `Clone`/`Debug` support callers that copy steps
/// into scratch drafts (e.g. the test-side candidate generators); steps are
/// freely copied, never shared — no aliasing is observable.
#[derive(Debug, Clone)]
pub struct Step {
    /// begin at this absolute minute (v0.5, minute-precise — wins over clock)
    pub start_min: Option<i64>,
    /// quantized begin clock — the input syntax; the default begin is now
    pub clock: Option<Clock>,
    pub clauses: Vec<Clause>,
    pub t_real: Option<String>,
}

/// The log row. The serde field order is pinned: every make*Record
/// construction site builds the same literal shape, and the `Option`s
/// WITHOUT `skip_serializing_if` serialize as explicit `null` fields.
/// `kind` stays a raw String: replay switches on it and ANY other
/// value falls through to the action branch — the String preserves that
/// open-world behavior (and JSON round-trips losslessly).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ActionRecord {
    pub id: i64,
    pub kind: String,
    pub trigger: Option<String>,
    #[serde(rename = "tGame")]
    pub t_game: String, // display clock "day:halfhour" (floored — display only)
    #[serde(rename = "tReal")]
    pub t_real: Option<String>,
    #[serde(rename = "advancedMin")]
    pub advanced_min: i64, // BEGIN: window length; wait: the span; else 0
    /// v0.5 — exact minutes: BEGIN = begin minute, wait = span start. The
    /// quantized tGame cannot carry mid-halfhour minutes, and fold must land
    /// them exactly (expires anchoring, §9.46)
    #[serde(rename = "startMin")]
    pub start_min: Option<i64>,
    /// v0.5 — COMPLETE rows only: the window key they close
    pub window: Option<String>,
    pub actors: Vec<Ref>, // sorted union across clauses
    pub verb: String,     // first clause's verb ('' for events/waits; the verb of a COMPLETE row)
    pub objects: Vec<Ref>,
    pub instruments: Vec<Ref>,
    pub at: Option<Ref>,
    pub about: Option<Ref>,
    pub to: Option<Ref>,
    pub outcomes: Vec<Outcome>, // concrete: ids minted, sugar resolved (§9.15)
    pub recipes: Vec<String>,   // BEGIN: fired recipe ids, clause order; COMPLETE: the one recipe
    pub sentence: String,       // canonical, lossless
}

/// One executed step: the records minted, the resulting state (unchanged
/// when rejected or refused), and the rejection reason if any.
pub struct ExecResult {
    pub records: Vec<ActionRecord>,
    pub state: State, // unchanged when rejected or refused
    pub rejected: Option<String>,
}

/// Execution options: the injected `id_gen` closure rides as
/// `&mut dyn FnMut`; `None` selects the deterministic default minted inside
/// [`execute_step`] (§9.3).
pub struct ExecOptions<'a> {
    pub first_record_id: i64,
    pub id_gen: Option<&'a mut dyn FnMut() -> String>,
}

// ---------- role binding & slot validation (§6.1) ----------

/// One validated, recipe-selected clause.
struct Prepared<'a> {
    clause: &'a Clause,
    verb: &'a CompiledVerb,
    refs: IndexMap<Role, Vec<Ref>>,
    /// Resolved against the INPUT state (validation & selection). Never read
    /// back: Phase 3 re-resolves the clause refs against the SCRATCH draft by
    /// design; the field is dead but kept so the struct carries what
    /// validation produced.
    #[allow(dead_code)]
    bindings: RoleBindings,
    recipe: &'a CompiledRecipe,
}

/// Clause refs per role; the IndexMap keys follow the ROLES declaration
/// order for the `refs[role]` lookups.
fn clause_refs(c: &Clause) -> IndexMap<Role, Vec<Ref>> {
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
fn validate_slots(
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
        // a JS Infinity max never trips the over-max check;
        // `Option<i64>` None models that
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
        // §2.6.2: the bindings store IDS — mid-list DESTROY must be visible
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
/// (§9.40) — the error propagates uncaught through every caller (never a
/// rejection). The one documented merge point
/// is execute_step's Phase-2 `validate_slots` fold (unreachable input — see
/// the channel note there).
pub fn resolve_ref<'a>(state: &'a State, r: &Ref) -> Result<Option<&'a Entity>> {
    match r {
        Ref::Id { id } => Ok(get_alive(state, id)),
        Ref::Param { .. } => Err(ChorusError::new(
            "internal: a function parameter ref survived expansion",
        )),
        // §9.38 pool addressing: a name held by N > 1 living copies addresses
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
fn show(r: &Ref) -> String {
    match r {
        Ref::Id { id } => format!("#{id}"),
        Ref::Name { name, .. } | Ref::Param { name } => name.clone(),
    }
}

// ---------- recipe selection (§6.4): first match wins ----------

/// First-match recipe selection; `None` = "no recipe matched" — the caller
/// owns the message text.
fn select_recipe<'a>(
    verb: &'a CompiledVerb,
    bindings: &RoleBindings,
    state: &State,
) -> Result<Option<&'a CompiledRecipe>> {
    let ctx = PredCtx {
        b: bindings,
        state,
        strict: false,
        roll: None,
    };
    for r in &verb.recipes {
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

// ---------- execution (§6.6) ----------

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
    // game-over (§9.35): once the story has ended nothing further executes,
    // and nothing is logged — undo past the ending record reopens the game
    // (it derives from the fold, so no special-case is needed)
    if let Some(ended) = &state_in.ended {
        return Ok(rejected(
            format!("the story has ended — {}", ended.title),
            state_in,
        ));
    }
    // v0.5: the begin minute — start_min wins (a mid-halfhour begin is
    // representable), the quantized clock field is the input syntax.
    // A DEFINED start_min wins even when it is 0 (0 wins over the clock):
    // Option + unwrap_or_else, never a 0-sentinel.
    let start_min = match step.start_min {
        Some(m) => m,
        None => abs_min(
            step.clock
                .unwrap_or_else(|| clock_from_abs_min(state_in.now)),
        ),
    };
    // monotonic mechanics time (§9.5): a step may never begin before now
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

    let ExecOptions {
        first_record_id,
        id_gen,
    } = opts;

    // deterministic default mint (§9.3)
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
    // pass — nothing logged, clock unmoved (§9.19).
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
        return Ok(ExecResult {
            records: advance,
            state: draft,
            rejected: None,
        });
    }

    // Phase 2 — validation, busy checks, selection, all against the
    // POST-ADVANCE draft: locks release when their windows close during the
    // pass, so a card freeing exactly at the begin minute is usable (§6.1/§6.4)
    let mut prepared: Vec<Prepared> = Vec::new();
    for clause in &step.clauses {
        let Some(verb) = content.verbs.get(&clause.verb) else {
            return Ok(rejected(
                format!("{} is not a content verb", clause.verb),
                state_in,
            ));
        };
        let refs = clause_refs(clause);
        // Channel note: the reachable validate_slots errors reject here with
        // the `<verb>: ` prefix (the `{error}` return channel), while
        // resolve_ref's param-form error finds no rejection path in Phase 2
        // and escapes execute_step as a hard Err (busy_reason/select_recipe
        // map their errors to the Err channel — see the `?` below). Merging
        // both into one `Result<ChorusError>` means that internal error would
        // surface as a rejection here instead. Unreachable on every input
        // path (§9.40): the parser only produces param refs while parsing a
        // function-body template and rejects one after
        // `kind:`, FN expansion substitutes every parameter
        // before compile, and the lenient/record paths only produce name/id
        // refs — no caller can build a param-ref clause.
        let bindings = match validate_slots(verb, &refs, &draft) {
            Ok(b) => b,
            Err(e) => return Ok(rejected(format!("{}: {}", clause.verb, e.msg), state_in)),
        };
        if let Some(busy) = busy_reason(&draft, &clause.verb, &refs)? {
            return Ok(rejected(busy, state_in));
        }
        let Some(recipe) = select_recipe(verb, &bindings, &draft)? else {
            return Ok(rejected(
                format!("{}: no recipe matched", verb.name),
                state_in,
            ));
        };
        prepared.push(Prepared {
            clause,
            verb,
            refs,
            bindings,
            recipe,
        });
    }

    // Phase 3 — the top-level REFUSE pre-pass (v0.5): authored gates reject AT
    // THE TABLE, not after the window. The trial runs on a scratch draft; only
    // a REFUSE is decisive here — an application ERROR defers to completion
    // (the state will have changed by then; the window refuses there, the
    // completion analog of §9.19). The trial's dice and spawns die with it.
    let mut scratch = clone_state(&draft);
    let mut smint: i64 = 0;
    let mut scratch_gen = move || {
        smint += 1;
        format!("s{next_id}_{smint}")
    };
    let refused: bool;
    let mut collected: Vec<Outcome> = Vec::new();
    {
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
        };
        // The closure shape (a try/catch around the whole pre-pass loop): a
        // THROW here is NOT a gate — it defers to completion time and clears
        // the trial.
        let gate: Result<bool> = (|| {
            for p in &prepared {
                // §9.41: the dice binding is PER CLAUSE — a roll in clause one
                // is not visible to clause two's conditions
                sctx.roll = None;
                let mut rebound = RoleBindings::new();
                for role in ROLES {
                    let mut ids = Vec::new();
                    for r in &p.refs[&role] {
                        if let Some(e) = resolve_ref(sctx.state, r)? {
                            ids.push(e.id.clone());
                        }
                    }
                    rebound.insert(role, ids);
                }
                sctx.bindings = rebound;
                for outcome in &p.recipe.out {
                    // an IF is opaque to the gate check: its branches —
                    // including any REFUSE inside them — belong to the
                    // completion, where the world the window imagined actually
                    // exists (§9.39 v0.5)
                    if outcome.op == "IF" {
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
        refused = match gate {
            Ok(r) => r,
            Err(_) => {
                collected.clear();
                false
            }
        };
    }
    if refused {
        // refused BEGIN: attempt logged, zero time, the advance discarded with
        // it. The record takes `first_record_id` (the advance's ids die with
        // the draft and are REUSED next round; fold_up_to's `r.id <
        // upto_exclusive` filter depends on the recycle — never make the id
        // sequence monotonic across the refusal).
        let mut record =
            make_begin_record(first_record_id, start_min, &step, &prepared, collected, dict)?;
        record.advanced_min = 0; // §6.5: a refusal collapses the cycle — no window elapses
        return Ok(ExecResult {
            records: vec![record],
            state: state_in,
            rejected: None,
        });
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
    Ok(ExecResult {
        records,
        state: draft,
        rejected: None,
    })
}

/// Roles a card is COMMITTED to; the
/// actors stay free (the player is never locked, or no two verbs could ever
/// run in parallel)
const COMMITTED_ROLES: [Role; 5] = [Role::Object, Role::With, Role::At, Role::About, Role::To];

/// §6.7 v0.5 lock checks, against the post-advance draft: one window per
/// verb station; committed cards busy while any open window holds them.
/// First match wins (the iteration order over `windows` is observable
/// through this message).
fn busy_reason(
    state: &State,
    verb: &str,
    refs: &IndexMap<Role, Vec<Ref>>,
) -> Result<Option<String>> {
    for w in state.windows.values() {
        if w.verb == verb {
            let end = clock_from_abs_min(w.end);
            return Ok(Some(format!(
                "{verb} is already processing — its station frees at {}:{}",
                end.day, end.halfhour
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
                        w.verb, end.day, end.halfhour
                    )));
                }
            }
        }
    }
    Ok(None)
}

/// Build the BEGIN row: merged deduped actors, the first clause's role refs,
/// durations summed into `advanced_min`.
fn make_begin_record(
    id: i64,
    start_min: i64,
    step: &Step,
    prepared: &[Prepared],
    outcomes: Vec<Outcome>,
    dict: &Dictionary,
) -> Result<ActionRecord> {
    let advanced_min = prepared
        .iter()
        .fold(0i64, |s, p| s.saturating_add(p.verb.duration));
    let first = prepared.first();
    let mut seen: HashSet<String> = HashSet::new();
    let actors: Vec<Ref> = sort_refs(
        &prepared
            .iter()
            .flat_map(|p| p.clause.actors.iter().cloned())
            .collect::<Vec<Ref>>(),
    )
    .into_iter()
    .filter(|r| {
        // dedupe on `#id` / name keys
        let k = match r {
            Ref::Id { id } => format!("#{id}"),
            Ref::Name { name, .. } | Ref::Param { name } => name.clone(),
        };
        if seen.contains(&k) {
            return false;
        }
        seen.insert(k);
        true
    })
    .collect();
    let verb = first.map(|p| p.clause.verb.clone()).unwrap_or_default();
    let objects = first.map(|p| p.clause.objects.clone()).unwrap_or_default();
    let instruments = first.map(|p| p.clause.with.clone()).unwrap_or_default();
    let at = first.and_then(|p| p.clause.at.clone());
    let about = first.and_then(|p| p.clause.about.clone());
    let to = first.and_then(|p| p.clause.to.clone());
    let recipes: Vec<String> = prepared.iter().map(|p| p.recipe.id.clone()).collect();
    let mut record = ActionRecord {
        id,
        kind: "action".to_string(),
        trigger: None,
        t_game: format_clock(clock_from_abs_min(start_min)),
        t_real: step.t_real.clone(),
        advanced_min, // the window length — NOT a clock advance (v0.5)
        start_min: Some(start_min), // exact begin minute (fold lands it exactly)
        window: None,
        actors,
        verb,
        objects,
        instruments,
        at,
        about,
        to,
        outcomes,
        recipes,
        sentence: String::new(),
    };
    record.sentence = format_sentence(
        &Sentence {
            kind: SentenceKind::Action,
            clock: Some(clock_from_abs_min(start_min)),
            trigger: None,
            clauses: step.clauses.clone(),
            outcomes: record.outcomes.clone(),
            span_min: None,
        },
        Some(dict),
    )?;
    Ok(record)
}

/// Register one window per BEGIN clause —
/// the SAME code path play and fold run (the divergence guard): clauses come
/// from the row's canonical sentence (law 2 makes that lossless), durations
/// and recipe ids from content.
fn register_windows(
    record: &ActionRecord,
    state: &mut State,
    dict: &Dictionary,
    content: &CompiledContent,
) -> Result<()> {
    // a BEGIN row's clauses ride its canonical sentence (law 2 makes that
    // lossless); a degenerate row with no parseable clauses opens no window.
    // QUIRK(Q6): the format∘parse round-trip failure SILENTLY opens no
    // window (the clauses are dropped to empty) — frozen; do NOT turn this
    // into an error and do NOT pass clauses through memory (both change
    // behavior on degenerate inputs).
    let clauses: Vec<Clause> = match parse_sentence(&record.sentence, dict) {
        Ok(s) => s.clauses,
        Err(_) => Vec::new(),
    };
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
                recipe: record.recipes.get(ci).cloned().unwrap_or_default(),
                refs,
                locked,
            },
        );
        offset = checked_step(offset, verb.duration, "window offset");
    }
    Ok(())
}

/// A rejection result: no records, the input state untouched.
fn rejected(reason: String, state: State) -> ExecResult {
    ExecResult {
        records: Vec::new(),
        state,
        rejected: Some(reason),
    }
}

/// Role args → concrete id refs captured
/// from the draft bindings (§9.15: the log never contains sugar)
fn concretize(args: &[Arg], ctx: &ApplyCtx) -> Vec<Arg> {
    let mut out: Vec<Arg> = Vec::new();
    for a in args {
        if let Arg::Role { role } = a {
            for id in ctx
                .bindings
                .get(role)
                .map(|v| v.as_slice())
                .unwrap_or(&[])
            {
                out.push(Arg::Ref {
                    r#ref: id_ref(id.clone()),
                });
            }
        } else {
            out.push(a.clone());
        }
    }
    out
}

/// §9.38: the mint's stackability from its
/// RESOLVED name+kind — a NAME declared stackable, or a KIND carrying a
/// stackable declaration. Concrete SPAWN rows always carry the resolved
/// `kind:K`, so the fold recomputes from the same (name, kind) pair: play
/// must decide from the resolved kind too, or a plain spawn of a
/// non-stackable prototype in a stackable KIND would mint non-stackable at
/// play but stackable at fold (snap would diverge).
fn mint_is_stackable(content: &CompiledContent, name: &str, kind: Option<&str>) -> bool {
    content.stackable_names.contains(name)
        || kind.is_some_and(|k| content.stackable_kinds.contains(k))
}

/// The bound id ref for a role target, or the target itself when unbound.
fn id_or_self(e: Option<&Entity>, fallback: IntoTarget) -> IntoTarget {
    match e {
        Some(e) => IntoTarget::Ref(id_ref(e.id.clone())),
        None => fallback,
    }
}

// ---------- outcome application (§4.4) ----------

/// The outcome-application context. `id_gen` is the injected id closure;
/// `roll` is `None` until the list's ROLL applies (reset per clause of a
/// multi-clause step).
struct ApplyCtx<'a> {
    state: &'a mut State,
    dict: &'a Dictionary,
    content: &'a CompiledContent,
    bindings: RoleBindings,
    now: i64, // absolute minutes at application time
    record_id: i64,
    id_gen: &'a mut dyn FnMut() -> String,
    extra_advance_min: i64,
    /// §9.41: the per-clause dice binding — the last ROLL of THIS clause's
    /// outcome list (last-wins, rebinds on a second ROLL). Lives on the apply
    /// context, never on State: a REFUSE discards the draft along with it, so
    /// a refused step's dice never moved the real counter. Reset per clause of
    /// a multi-clause step; a fresh event context starts unbound. Function
    /// expansion is compile-time, so expanded bodies read the caller's list
    /// binding naturally.
    roll: Option<i64>,
}

/// What one outcome application produced.
struct ApplyResult {
    concrete: Vec<Outcome>, // what the log stores: ids minted, sugar resolved, deltas computed (§9.15, §9.31)
    refused: bool,
}

/// A `{ op, args }` outcome literal — the only shape that reaches the log.
fn plain(op: String, args: Vec<Arg>) -> Outcome {
    Outcome {
        op,
        args,
        arms: None,
        fallback: None,
        call: None,
    }
}

/// §9.41 {roll} interpolation. A NARRATE string's
/// `{roll}` token is rewritten AT EXECUTION, so the concrete row carries the
/// number — the journal never re-derives dice. QUIRK(Q7): a NARRATE in
/// a REFUSED step is journal-visible WITH its number even though the counter
/// never moved (the draft died — the documented, deterministic re-roll
/// oddity); frozen, do not "fix".
fn rewrite_roll_prose(o: &Outcome, ctx: &ApplyCtx) -> Outcome {
    let Some(Arg::Str { v: s }) = o.args.first() else {
        return o.clone();
    };
    let Some(roll) = ctx.roll else {
        return o.clone();
    };
    if !s.contains("{roll}") {
        return o.clone();
    }
    Outcome {
        op: o.op.clone(),
        args: vec![Arg::Str {
            v: s.replace("{roll}", &roll.to_string()),
        }],
        arms: None,
        fallback: None,
        call: None,
    }
}

/// The op dispatch (17 ops). The match arms follow the authored op list in
/// order; the four big SPAWN /
/// REPLACE / SET / DRAW cases are pulled into helper fns purely for file
/// size (one arm each).
fn apply_outcome(o: &Outcome, ctx: &mut ApplyCtx) -> Result<ApplyResult> {
    let args = &o.args;
    match o.op.as_str() {
        "IF" => apply_if(o, ctx),
        "SPAWN" => apply_spawn(args, ctx),
        "DESTROY" => {
            let concrete_args = concretize(args, ctx);
            for id in expand(&concrete_args, ctx) {
                // stamp + release on the CURRENT entity (§2.6.1: entities are
                // never removed, so the id always resolves)
                let name = match ctx.state.entities.get(&id) {
                    Some(e) => e.name.clone(),
                    None => continue, // unreachable under §2.6.1
                };
                if let Some(e) = ctx.state.entities.get_mut(&id) {
                    e.dead_action = Some(ctx.record_id);
                    // names are unique among the LIVING (§9.14): the name
                    // frees only when the LAST pool copy dies (§9.38)
                }
                release_name_entry(ctx.state, &name, &id);
            }
            Ok(ApplyResult {
                concrete: vec![plain(o.op.clone(), concrete_args)],
                refused: false,
            })
        }
        "REPLACE" => apply_replace(args, ctx),
        "SET" => apply_set(args, ctx),
        "MOVE" => {
            let Some(Arg::Into { r#ref: into_ref }) =
                args.iter().find(|a| matches!(a, Arg::Into { .. }))
            else {
                return Err(ChorusError::new("MOVE needs INTO"));
            };
            let into_concrete: Arg = match into_ref {
                IntoTarget::Role { role } => {
                    let bound = ctx
                        .bindings
                        .get(role)
                        .and_then(|v| v.first())
                        .and_then(|id| ctx.state.entities.get(id));
                    Arg::Into {
                        r#ref: id_or_self(bound, IntoTarget::Role { role: *role }),
                    }
                }
                into_ref @ IntoTarget::Ref(..) => Arg::Into {
                    r#ref: into_ref.clone(),
                },
            };
            let dest: Option<String> = match &into_concrete {
                Arg::Into {
                    r#ref: IntoTarget::Ref(r#ref),
                } => resolve_ref(ctx.state, r#ref)?.map(|e| e.id.clone()),
                _ => None,
            };
            let Some(dest) = dest else {
                // the `!dest` guard: an unresolvable destination is a hard error
                return Err(ChorusError::new("MOVE: unresolvable destination"));
            };
            let refs: Vec<Arg> = concretize(
                &args
                    .iter()
                    .filter(|a| !matches!(a, Arg::Into { .. }))
                    .cloned()
                    .collect::<Vec<Arg>>(),
                ctx,
            );
            for id in expand(&refs, ctx) {
                if let Some(e) = ctx.state.entities.get_mut(&id) {
                    e.attrs
                        .insert("container".to_string(), AttrValue::Str(dest.clone()));
                }
            }
            let mut concrete_args = refs;
            concrete_args.push(into_concrete);
            Ok(ApplyResult {
                concrete: vec![plain(o.op.clone(), concrete_args)],
                refused: false,
            })
        }
        "REVEAL" => {
            let concrete_args = concretize(args, ctx);
            for id in expand(&concrete_args, ctx) {
                if let Some(e) = ctx.state.entities.get_mut(&id) {
                    e.attrs
                        .insert("state.hidden".to_string(), AttrValue::Bool(false));
                }
            }
            Ok(ApplyResult {
                concrete: vec![plain(o.op.clone(), concrete_args)],
                refused: false,
            })
        }
        "ADVANCE" => {
            let Some(Arg::Dur { v }) = args.first() else {
                return Err(ChorusError::new("ADVANCE needs a duration"));
            };
            ctx.extra_advance_min = checked_step(ctx.extra_advance_min, *v, "ADVANCE");
            Ok(ApplyResult {
                concrete: vec![o.clone()],
                refused: false,
            })
        }
        "NARRATE" => Ok(ApplyResult {
            concrete: vec![rewrite_roll_prose(o, ctx)],
            refused: false,
        }),
        "PROGRESS" => {
            // §6.7: a read-bar tick — journal-only, no state effect, no time effect
            let ok = matches!(args.first(), Some(Arg::Dur { .. }))
                && matches!(args.get(1), Some(Arg::Str { .. }));
            if !ok {
                return Err(ChorusError::new("PROGRESS needs (duration, string)"));
            }
            Ok(ApplyResult {
                concrete: vec![o.clone()],
                refused: false,
            })
        }
        "RELATE" => {
            let refs: Vec<&Ref> = args
                .iter()
                .filter_map(|a| match a {
                    Arg::Ref { r#ref } => Some(r#ref),
                    _ => None,
                })
                .collect();
            let value = args.iter().find_map(|a| match a {
                Arg::Int { v } => Some(*v),
                _ => None,
            });
            if refs.len() < 3 {
                return Err(ChorusError::new("RELATE needs (ref, ref, type[, value])"));
            }
            let a_id = resolve_ref(ctx.state, refs[0])?
                .map(|e| e.id.clone())
                .ok_or_else(|| ChorusError::new("RELATE: unresolvable ref"))?;
            let b_id = resolve_ref(ctx.state, refs[1])?
                .map(|e| e.id.clone())
                .ok_or_else(|| ChorusError::new("RELATE: unresolvable ref"))?;
            let type_ = match refs[2] {
                Ref::Name { name, .. } => name.clone(),
                _ => return Err(ChorusError::new("RELATE: relation type must be a name")),
            };
            let def = ctx.dict.relation_def(&type_)?;
            let key = relation_key(&a_id, &b_id, &type_, def.symmetric);
            ctx.state.relations.insert(
                key,
                Relation {
                    a: a_id,
                    b: b_id,
                    type_,
                    value: value.unwrap_or(1),
                    born_action: ctx.record_id,
                    dead_action: None,
                },
            );
            Ok(ApplyResult {
                concrete: vec![o.clone()],
                refused: false,
            })
        }
        "UNRELATE" => {
            // §7: stamp, don't erase — undo re-folds
            let refs: Vec<&Ref> = args
                .iter()
                .filter_map(|a| match a {
                    Arg::Ref { r#ref } => Some(r#ref),
                    _ => None,
                })
                .collect();
            if refs.len() < 3 {
                return Err(ChorusError::new("UNRELATE needs (ref, ref, type)"));
            }
            let a_id = resolve_ref(ctx.state, refs[0])?.map(|e| e.id.clone());
            let b_id = resolve_ref(ctx.state, refs[1])?.map(|e| e.id.clone());
            let type_ = match refs[2] {
                Ref::Name { name, .. } => Some(name.clone()),
                _ => None,
            };
            if let (Some(a_id), Some(b_id), Some(type_)) = (a_id, b_id, type_) {
                let def = ctx.dict.relation_def(&type_)?;
                let key = relation_key(&a_id, &b_id, &type_, def.symmetric);
                if let Some(rel) = ctx.state.relations.get_mut(&key) {
                    rel.dead_action = Some(ctx.record_id);
                }
            }
            Ok(ApplyResult {
                concrete: vec![o.clone()],
                refused: false,
            })
        }
        "ENDING" => {
            // §9.35: the FIRST ENDING to apply wins;
            // later ones are logged no-ops. Title/flavour resolve here from
            // content — the one code path both play and fold share.
            let id = match args.first() {
                Some(Arg::Ref {
                    r#ref: Ref::Name { name, .. },
                }) => Some(name.clone()),
                _ => None,
            };
            let Some(id) = id else {
                return Err(ChorusError::new("ENDING needs an id: ENDING ENDING_ID"));
            };
            let Some(ending) = ctx.content.endings.get(&id) else {
                return Err(ChorusError::new(format!(
                    "ENDING '{id}' is not in the endings table"
                )));
            };
            if ctx.state.ended.is_none() {
                ctx.state.ended = Some(Ending {
                    id,
                    title: ending.title.clone(),
                    flavour: ending.flavour.clone(),
                });
            }
            Ok(ApplyResult {
                concrete: vec![o.clone()],
                refused: false,
            })
        }
        "REFUSE" => Ok(ApplyResult {
            concrete: vec![o.clone()],
            refused: true,
        }),
        "ROLL" => apply_roll(args, ctx),
        "ROLLED" => {
            // §9.41, fold side: set the counter,
            // nothing else (the total is journal data; replay never re-derives
            // a die, law 3). Play never applies this op: authored ROLLED is a
            // load error, so the only path here is `apply_record` over a logged
            // row.
            let Some(Arg::Int { v: ctr_after }) = args.get(1) else {
                return Err(ChorusError::new("ROLLED needs (total, ctrAfter)"));
            };
            ctx.state.rng = *ctr_after;
            Ok(ApplyResult {
                concrete: vec![o.clone()],
                refused: false,
            })
        }
        "DRAW" => apply_draw(args, ctx),
        "DRAWN" => {
            // §9.42, fold side: advance the cursor,
            // ASSERTING the recorded name is exactly what the compiled
            // permutation serves at that cursor (play and fold share
            // `draw_from`, so only content drift can disagree). Play never
            // applies this op: authored DRAWN is a load error, so the only
            // path here is `apply_record` over a logged row.
            let (deck_name, recorded) = match (args.first(), args.get(1)) {
                (
                    Some(Arg::Ref {
                        r#ref: Ref::Name { name: d, .. },
                    }),
                    Some(Arg::Ref {
                        r#ref: Ref::Name { name: n, .. },
                    }),
                ) => (d.clone(), n.clone()),
                _ => return Err(ChorusError::new("DRAWN needs (deck, name)")),
            };
            let Some(deck) = ctx.content.decks.get(&deck_name) else {
                return Err(ChorusError::new(format!(
                    "DRAWN: deck '{deck_name}' is not in the compiled decks — content drift between play and fold (§9.42)"
                )));
            };
            let cursor = ctx.state.decks.get(&deck.id).copied().unwrap_or(0);
            let expected = draw_from(deck, cursor)?; // throws loud on a drifted-away exhaustion rule
            if expected.name != recorded {
                return Err(ChorusError::new(format!(
                    "DRAWN: deck '{}' draw #{cursor} recorded '{recorded}' but the compiled deck serves '{}' — content drift between play and fold (§9.42)",
                    deck.id, expected.name
                )));
            }
            ctx.state
                .decks
                .insert(deck.id.clone(), checked_step(cursor, 1, "deck cursor"));
            Ok(ApplyResult {
                concrete: vec![o.clone()],
                refused: false,
            })
        }
        _ => Err(ChorusError::new(format!("unknown outcome op '{}'", o.op))),
    }
}

/// The SPAWN case.
fn apply_spawn(args: &[Arg], ctx: &mut ApplyCtx) -> Result<ApplyResult> {
    // the first ref arg must be a NAME ref
    let ref_arg = args.iter().find_map(|a| match a {
        Arg::Ref { r#ref } => Some(r#ref),
        _ => None,
    });
    let Some(Ref::Name {
        name: proto_name,
        kind: ref_kind,
    }) = ref_arg
    else {
        return Err(ChorusError::new(
            "SPAWN needs a name ref (prototype or kind:NAME)",
        ));
    };
    let proto = ctx.content.prototypes.get(proto_name);
    let kind = match ref_kind {
        Some(k) => Some(k.clone()),
        None => proto.map(|p| p.kind.clone()),
    };
    let Some(kind) = kind else {
        return Err(ChorusError::new(format!(
            "SPAWN {proto_name}: no kind (prototype or kind:NAME required)"
        )));
    };
    let mut name = proto_name.clone();
    // the attrs spread is a JS key-order point (integer-like keys front)
    let mut aspects: Aspects = match proto.and_then(|p| p.aspects.as_ref()) {
        Some(a) => clone_aspects(a),
        None => Aspects::new(),
    };
    let mut attrs: Attrs = match proto.and_then(|p| p.attrs.as_ref()) {
        Some(a) => clone_map_in_js_order(a),
        None => Attrs::new(),
    };
    // derive (`name from object`, §6.4)
    if let Some(derive) = proto.and_then(|p| p.derive.as_ref()) {
        let obj_id = ctx
            .bindings
            .get(&Role::Object)
            .and_then(|v| v.first())
            .cloned();
        let obj = obj_id.and_then(|id| ctx.state.entities.get(&id));
        if let Some(obj) = obj {
            if derive.name_from.as_deref() == Some("object") {
                name = format!("{name}_{}", obj.name);
            }
            if let Some(rules) = &derive.aspects {
                // `Object.entries(derive.aspects)` — the
                // JS key order is observable (aspects insertion order)
                for key in ordinary_own_property_keys(rules) {
                    let rule = &rules[&key];
                    if let Some(base) = aspect_get(obj, &rule.from_object_aspect) {
                        // JS numbers subtract in f64; saturating subtraction
                        // is the documented i64 divergence direction
                        aspects.insert(key, AspectVal::Num(base.saturating_sub(rule.minus)));
                    }
                }
            }
        }
    }
    // §9.38: the flag comes from the mint's RESOLVED name+kind declaration
    // (not the authored ref's form — the concrete row always carries the
    // resolved kind, and the fold must recompute the same flag) and applies
    // to whatever name the mint ends up carrying (derived names stack when
    // the prototype is declared stackable)
    let is_stack = mint_is_stackable(ctx.content, proto_name, Some(&kind));
    // §9.14 + §9.38: a non-stackable mint demands a free name; a stackable
    // mint stacks with its own kind of pool but may not collide with a
    // distinct NON-stackable living name (shared namespace)
    if let Some(pool) = ctx.state.by_name.get(name.as_str()) {
        if !pool.is_empty() {
            let all_stackable = pool
                .iter()
                .all(|pid| ctx.state.entities.get(pid).is_some_and(|e| e.stackable));
            if !is_stack || !all_stackable {
                return Err(ChorusError::new(format!(
                    "SPAWN: name '{name}' already in use"
                )));
            }
        }
    }
    // an explicit AS id, else the mint generator
    let as_id = args.iter().find_map(|a| match a {
        Arg::AsId { id } => Some(id.clone()),
        _ => None,
    });
    let id = match as_id {
        Some(id) => id,
        None => (ctx.id_gen)(),
    };
    // container: INTO arg, else the `at` binding,
    // else the first actor's `container` attr (raw value, truthiness-gated)
    let into = args.iter().find_map(|a| match a {
        Arg::Into { r#ref } => Some(r#ref),
        _ => None,
    });
    let container: Option<AttrValue> = if let Some(into_ref) = into {
        match into_ref {
            IntoTarget::Role { role } => ctx
                .bindings
                .get(role)
                .and_then(|v| v.first())
                .map(|id| AttrValue::Str(id.clone())),
            IntoTarget::Ref(r#ref) => {
                resolve_ref(ctx.state, r#ref)?.map(|e| AttrValue::Str(e.id.clone()))
            }
        }
    } else {
        match ctx.bindings.get(&Role::At).and_then(|v| v.first()).cloned() {
            Some(at_id) => Some(AttrValue::Str(at_id)),
            None => ctx
                .bindings
                .get(&Role::Actors)
                .and_then(|v| v.first())
                .and_then(|id| ctx.state.entities.get(id))
                .and_then(|e| e.attrs.get("container"))
                .cloned(),
        }
    };
    // the container attr is written only when truthy (JS truthiness)
    if js_truthy(container.as_ref()) {
        if let Some(c) = &container {
            attrs.insert("container".to_string(), c.clone());
        }
    }
    // fold-time reconstruction: a recorded SPAWN
    // (asId present) may carry materialized aspects/attrs as JSON pairs —
    // prefer them over derive. An explicit attrs= pair is authoritative
    // (§9.18): it already carries the mint's location (container and/or
    // zone), so a mint with no location must not grow one from the replay
    // bindings.
    let aspects_arg = args.iter().find_map(|a| match a {
        Arg::Pair { k, v } if k == "aspects" => Some(v),
        _ => None,
    });
    let attrs_arg = args.iter().find_map(|a| match a {
        Arg::Pair { k, v } if k == "attrs" => Some(v),
        _ => None,
    });
    if let Some(AttrValue::Str(json)) = aspects_arg {
        aspects = aspects_from_json(json)?;
    }
    if let Some(AttrValue::Str(json)) = attrs_arg {
        let materialized = attrs_from_json(json)?;
        // Object.assign keeps existing keys' slots and
        // appends new ones in the source's order (IndexMap::insert matches);
        // the container survives only if the pair carried one
        let had_container = materialized.contains_key("container");
        for (k, v) in materialized {
            attrs.insert(k, v);
        }
        if !had_container {
            attrs.shift_remove("container"); // JS delete keeps the other keys' slots (order-preserving)
        }
    }
    add_entity(
        ctx.state,
        Entity {
            id: id.clone(),
            kind: kind.clone(),
            name: name.clone(),
            aspects: aspects.clone(),
            attrs: attrs.clone(),
            born_action: Some(ctx.record_id),
            dead_action: None,
            stackable: is_stack,
        },
    )?;
    // concrete outcome: everything a fold needs,
    // nothing from the step context
    let mut concrete_args: Vec<Arg> = vec![Arg::Ref {
        r#ref: name_ref(name, Some(kind)),
    }];
    if js_truthy(container.as_ref()) {
        if let Some(c) = &container {
            // `id_ref` over the string-cast value
            concrete_args.push(Arg::Into {
                r#ref: IntoTarget::Ref(id_ref(c.to_js_string())),
            });
        }
    }
    concrete_args.push(Arg::AsId { id });
    // The pair STRINGS are built through the JS key order (the
    // `SET(x,"5",…)` path can put integer-like keys into attrs).
    if !aspects.is_empty() {
        concrete_args.push(Arg::Pair {
            k: "aspects".to_string(),
            v: AttrValue::Str(aspects_to_json(&aspects)),
        });
    }
    if !attrs.is_empty() {
        concrete_args.push(Arg::Pair {
            k: "attrs".to_string(),
            v: AttrValue::Str(attrs_to_json(&attrs)),
        });
    }
    Ok(ApplyResult {
        concrete: vec![plain("SPAWN".to_string(), concrete_args)],
        refused: false,
    })
}

/// The REPLACE case: §4.4 REPLACE ref WITH
/// prototype — mint from the prototype into the dying card's CURRENT
/// container, then soft-destroy it. The log carries the concrete SPAWN+
/// DESTROY pair, so the fold replays it exactly (§9.33).
fn apply_replace(args: &[Arg], ctx: &mut ApplyCtx) -> Result<ApplyResult> {
    let target_arg = args.first();
    let proto_arg = args.get(1);
    let proto_name = match proto_arg {
        Some(Arg::Ref {
            r#ref: Ref::Name { name, .. },
        }) => name.clone(),
        _ => {
            return Err(ChorusError::new(
                "REPLACE needs (ref|ROLE) WITH a prototype or kind:NAME ref",
            ))
        }
    };
    // plain prototype mint — REPLACE has no clause, so prototype derivation
    // (`name from object`, §6.4) is unavailable here (§9.33)
    let proto = ctx.content.prototypes.get(&proto_name);
    let kind = match proto_arg {
        Some(Arg::Ref {
            r#ref: Ref::Name { kind: Some(k), .. },
        }) => Some(k.clone()),
        _ => proto.map(|p| p.kind.clone()),
    };
    let Some(kind) = kind else {
        return Err(ChorusError::new(format!(
            "REPLACE {proto_name}: no kind (prototype or kind:NAME required)"
        )));
    };
    // §9.38: same computation as SPAWN — the RESOLVED name+kind, so the
    // fold's recompute from the concrete SPAWN row agrees (a stackable
    // successor may share the dying card's name: REPLACE X WITH X succeeds —
    // spawn appends, destroy pops)
    let is_stack = mint_is_stackable(ctx.content, &proto_name, Some(&kind));
    let aspects_tpl: Aspects = match proto.and_then(|p| p.aspects.as_ref()) {
        Some(a) => clone_aspects(a),
        None => Aspects::new(),
    };
    let targets: Vec<String> = match target_arg {
        Some(a) => expand(std::slice::from_ref(a), ctx),
        // unreachable through authored paths (parse enforces the arity);
        // the empty-target error below is the degenerate shape's destination
        // either way
        None => Vec::new(),
    };
    if targets.is_empty() {
        return Err(ChorusError::new(
            "REPLACE: unresolvable ref (nothing to replace)",
        ));
    }
    let mut concrete: Vec<Outcome> = Vec::new();
    for dying_id in targets {
        // spawn happens while the dying card is still alive, so a
        // non-stackable successor's name collides with any living holder —
        // whole-step rejection (§9.33); a stackable successor may share a
        // stackable pool
        if let Some(pool) = ctx.state.by_name.get(proto_name.as_str()) {
            if !pool.is_empty() {
                let all_stackable = pool
                    .iter()
                    .all(|pid| ctx.state.entities.get(pid).is_some_and(|e| e.stackable));
                if !is_stack || !all_stackable {
                    return Err(ChorusError::new(format!(
                        "REPLACE: name '{proto_name}' already in use"
                    )));
                }
            }
        }
        let id = (ctx.id_gen)();
        let mut attrs: Attrs = match proto.and_then(|p| p.attrs.as_ref()) {
            Some(a) => clone_map_in_js_order(a),
            None => Attrs::new(),
        };
        // container / zone ride off the dying card's CURRENT attrs
        let (container, zone, dying_name) = {
            let Some(dying) = ctx.state.entities.get(&dying_id) else {
                continue; // unreachable under §2.6.1
            };
            let container = match dying.attrs.get("container") {
                Some(AttrValue::Str(s)) => Some(s.clone()),
                _ => None,
            };
            // zone-tagged (§10): the grouping has no card, so the tag passes
            // on — as a plain attr pair, NOT as INTO (the fold's SPAWN replay
            // would materialize a container the mint never had)
            let zone = if container.is_none() {
                match dying.attrs.get("zone") {
                    Some(AttrValue::Str(s)) => Some(s.clone()),
                    _ => None,
                }
            } else {
                None
            };
            (container, zone, dying.name.clone())
        };
        if let Some(c) = &container {
            attrs.insert("container".to_string(), AttrValue::Str(c.clone()));
        }
        if let Some(z) = &zone {
            attrs.insert("zone".to_string(), AttrValue::Str(z.clone()));
        }
        add_entity(
            ctx.state,
            Entity {
                id: id.clone(),
                kind: kind.clone(),
                name: proto_name.clone(),
                aspects: clone_aspects(&aspects_tpl),
                attrs: attrs.clone(),
                born_action: Some(ctx.record_id),
                dead_action: None,
                stackable: is_stack,
            },
        )?;
        // soft-destroy AFTER the spawn
        if let Some(d) = ctx.state.entities.get_mut(&dying_id) {
            d.dead_action = Some(ctx.record_id);
        }
        release_name_entry(ctx.state, &dying_name, &dying_id);
        let mut spawn_args: Vec<Arg> = vec![Arg::Ref {
            r#ref: name_ref(proto_name.clone(), Some(kind.clone())),
        }];
        if let Some(c) = &container {
            spawn_args.push(Arg::Into {
                r#ref: IntoTarget::Ref(id_ref(c.clone())),
            });
        }
        spawn_args.push(Arg::AsId { id });
        if !aspects_tpl.is_empty() {
            spawn_args.push(Arg::Pair {
                k: "aspects".to_string(),
                v: AttrValue::Str(aspects_to_json(&aspects_tpl)),
            });
        }
        // always materialize attrs, even when empty (§9.18): the pair pins the
        // mint's location — with no pair the fold would re-add the bound
        // actor's container to a location-less successor
        spawn_args.push(Arg::Pair {
            k: "attrs".to_string(),
            v: AttrValue::Str(attrs_to_json(&attrs)),
        });
        concrete.push(plain("SPAWN".to_string(), spawn_args));
        concrete.push(plain(
            "DESTROY".to_string(),
            vec![Arg::Ref {
                r#ref: id_ref(dying_id.clone()),
            }],
        ));
    }
    Ok(ApplyResult {
        concrete,
        refused: false,
    })
}

/// The SET case (§4.4 arity: SET(ref, key,
/// value) — positional).
fn apply_set(args: &[Arg], ctx: &mut ApplyCtx) -> Result<ApplyResult> {
    let (ref_arg, key_arg, val_arg) = match (args.first(), args.get(1), args.get(2)) {
        (Some(r), Some(k), Some(v)) => (r, k, v),
        _ => return Err(ChorusError::new("SET needs (ref, key, value)")),
    };
    let raw_key: Option<String> = match key_arg {
        Arg::Key { v } | Arg::Str { v } => Some(v.clone()),
        Arg::Ref {
            r#ref: Ref::Name { name, .. },
        } => Some(name.clone()),
        _ => None,
    };
    let Some(raw_key) = raw_key else {
        return Err(ChorusError::new("SET key must be a name or string"));
    };
    // an `aspect:` prefix names the aspects map (§4.2); every other key is an
    // attr (§9.31)
    let aspect_path = raw_key.strip_prefix("aspect:");
    let key = aspect_path.map_or(raw_key.clone(), |p| p.to_string());
    // the concrete key keeps its authored form: a quoted key must stay quoted
    // or the record stops round-tripping (§9.15)
    let key_concrete: Arg = match key_arg {
        Arg::Str { .. } => key_arg.clone(),
        Arg::Ref { .. } if aspect_path.is_none() => key_arg.clone(),
        _ => Arg::Key { v: raw_key.clone() },
    };
    // the write closure
    macro_rules! write {
        ($ctx:expr, $e:expr, $value:expr) => {{
            let value: &AttrValue = $value;
            if let Some(path) = aspect_path {
                let AttrValue::Num(n) = value else {
                    return Err(ChorusError::new(format!(
                        "SET: aspect '{raw_key}' needs a number"
                    )));
                };
                aspect_set($e, path, *n);
            } else {
                $e.attrs.insert(key.clone(), value.clone());
                if key == "expires" {
                    let mins = to_minutes(value)?;
                    $e.attrs.insert(
                        "expiresAt".to_string(),
                        AttrValue::Num(checked_step($ctx.now, mins, "SET expires")),
                    );
                }
            }
        }};
    }
    if let Arg::Delta { v: delta } = val_arg {
        // signed value (§9.31): add to the entry's current number (missing = 0,
        // negative results allowed) and log the CONCRETE ABSOLUTE result — the
        // fold replays literals and never redoes arithmetic
        let targets = expand(std::slice::from_ref(ref_arg), ctx);
        let mut concrete: Vec<Outcome> = Vec::with_capacity(targets.len());
        for id in &targets {
            let cur: Option<i64> = {
                let Some(e) = ctx.state.entities.get(id) else {
                    continue; // unreachable under §2.6.1
                };
                if let Some(path) = aspect_path {
                    // raw entry, not `aspect_get`: a sub-map must REFUSE the
                    // delta, not count as missing and be clobbered (§9.31
                    // non-numeric rule)
                    match aspect_entry(e, path) {
                        Some(AspectEntry::Num(n)) => Some(n),
                        Some(AspectEntry::Sub(_)) => {
                            return Err(ChorusError::new(format!(
                                "SET: cannot add to non-numeric '{raw_key}'"
                            )))
                        }
                        None => None,
                    }
                } else {
                    match e.attrs.get(&key) {
                        Some(AttrValue::Num(n)) => Some(*n),
                        Some(_) => {
                            return Err(ChorusError::new(format!(
                                "SET: cannot add to non-numeric '{raw_key}'"
                            )))
                        }
                        None => None,
                    }
                }
            };
            // JS numbers add in f64; saturating add is the documented i64
            // divergence direction
            let next = cur.unwrap_or(0).saturating_add(*delta);
            if let Some(e) = ctx.state.entities.get_mut(id) {
                write!(ctx, e, &AttrValue::Num(next));
            }
            // one concrete SET per card, so replay needs no step context (§9.15)
            let ref_concrete: Arg = if targets.len() == 1 && !matches!(ref_arg, Arg::Role { .. }) {
                ref_arg.clone()
            } else {
                Arg::Ref {
                    r#ref: id_ref(id.clone()),
                }
            };
            concrete.push(plain(
                "SET".to_string(),
                vec![
                    ref_concrete,
                    key_concrete.clone(),
                    Arg::Int { v: next },
                ],
            ));
        }
        return Ok(ApplyResult {
            concrete,
            refused: false,
        });
    }
    let value: AttrValue = match val_arg {
        Arg::Int { v } => AttrValue::Num(*v),
        Arg::Str { v } => AttrValue::Str(v.clone()),
        Arg::Bool { v } => AttrValue::Bool(*v),
        Arg::Dur { v } => AttrValue::Num(*v),
        _ => {
            return Err(ChorusError::new(
                "SET value must be a string, number, boolean or duration",
            ))
        }
    };
    let targets = expand(std::slice::from_ref(ref_arg), ctx);
    for id in &targets {
        if let Some(e) = ctx.state.entities.get_mut(id) {
            write!(ctx, e, &value);
        }
    }
    // §9.15: the log never contains sugar — a ROLE arg must not reach the
    // record (the fold rebuilds whole-record bindings, so over a multi-clause
    // step it would attribute the role to the wrong clause). One concrete SET
    // per card, exactly like the delta path above; name and kind refs replay
    // fine and stay as written.
    if matches!(ref_arg, Arg::Role { .. }) {
        let concrete = targets
            .iter()
            .map(|id| {
                plain(
                    "SET".to_string(),
                    vec![
                        Arg::Ref {
                            r#ref: id_ref(id.clone()),
                        },
                        key_concrete.clone(),
                        val_arg.clone(),
                    ],
                )
            })
            .collect();
        return Ok(ApplyResult {
            concrete,
            refused: false,
        });
    }
    Ok(ApplyResult {
        concrete: vec![plain(
            "SET".to_string(),
            vec![ref_arg.clone(), key_concrete, val_arg.clone()],
        )],
        refused: false,
    })
}

/// ROLL (§9.41), the engine's ONLY
/// randomness. The args are §6.3 value expressions evaluated against this
/// clause's bindings and the DRAFT (strictly: a path over an unbound role
/// rejects the step, like an IF condition). A roll of m dice consumes exactly
/// m counter values c .. c+m-1; the concrete row carries the total AND the
/// counter after, so the fold lands the counter without deriving anything
/// (law 3). count = 0 is legal (total = bonus, counter unmoved); sides < 2 or
/// count < 0 reject the step (§9.19 — nothing logged, clock unmoved).
fn apply_roll(args: &[Arg], ctx: &mut ApplyCtx) -> Result<ApplyResult> {
    let (count_v, sides_v, bonus_v) = match (args.first(), args.get(1), args.get(2)) {
        (Some(Arg::Vexpr { v: c }), Some(Arg::Vexpr { v: s }), b) => {
            let bonus = match b {
                None => None,
                Some(Arg::Vexpr { v }) => Some(v),
                Some(_) => {
                    return Err(ChorusError::new(
                        "internal: ROLL arguments must be value expressions",
                    ))
                }
            };
            (c, s, bonus)
        }
        _ => {
            return Err(ChorusError::new(
                "internal: ROLL arguments must be value expressions",
            ))
        }
    };
    let (count, sides, bonus) = {
        let pctx = PredCtx {
            b: &ctx.bindings,
            state: ctx.state,
            strict: true,
            roll: ctx.roll,
        };
        let count = eval_expr(count_v, &pctx)?;
        let sides = eval_expr(sides_v, &pctx)?;
        let bonus = match bonus_v {
            Some(v) => eval_expr(v, &pctx)?,
            None => Some(0),
        };
        (count, sides, bonus)
    };
    // one strict discipline for all three operands (§9.39's rule, applied to
    // dice): an operand that cannot resolve — a missing aspect/attr over its
    // role — rejects the step, never a silent 0-dice roll. (`Number.isInteger`
    // is vacuous over i64 — the reachable checks are the undefined and range
    // ones; the message texts are frozen either way.)
    let Some(count) = count else {
        return Err(ChorusError::new(
            "ROLL: dice count must be an integer >= 0, got nothing",
        ));
    };
    if count < 0 {
        return Err(ChorusError::new(format!(
            "ROLL: dice count must be an integer >= 0, got {count}"
        )));
    }
    let Some(sides) = sides else {
        return Err(ChorusError::new(
            "ROLL: sides must be an integer >= 2, got nothing",
        ));
    };
    if sides < 2 {
        return Err(ChorusError::new(format!(
            "ROLL: sides must be an integer >= 2, got {sides}"
        )));
    }
    let Some(bonus) = bonus else {
        return Err(ChorusError::new(
            "ROLL: bonus must be an integer, got nothing",
        ));
    };
    let c = ctx.state.rng;
    let mut total = bonus;
    for i in 0..count {
        // JS numbers sum in f64; saturating add is the documented i64
        // divergence direction
        total = total.saturating_add(roll_die(ctx.content.seed, c + i, sides));
    }
    ctx.roll = Some(total); // the per-clause binding: `roll` and {roll} read this
    // consume the counters ON THE DRAFT — the fold lands the same spot via
    // the ROLLED row (law 1)
    let ctr_after = checked_step(c, count, "rng counter");
    ctx.state.rng = ctr_after;
    Ok(ApplyResult {
        concrete: vec![plain(
            "ROLLED".to_string(),
            vec![Arg::Int { v: total }, Arg::Int { v: ctr_after }],
        )],
        refused: false,
    })
}

/// The DRAW case (§9.42): draw the deck's
/// next card. The deck table lives on compiled content (the permutation is
/// pure in seed+id, law 5); the cursor lives on State and advances ON THE
/// DRAFT here — a later failure or REFUSE discards it with the whole draft
/// (law 4). Concretizes to TWO rows: the engine-written `DRAWN(<deck>,
/// <name>)` (the fold's cursor row, carrying the SPEC entry name so its
/// assertion reads the permutation) followed by the standard concrete SPAWN —
/// which owns every mint rule (prototype aspects/derive, §9.38 stackability,
/// materialization pairs), so DRAW adds none of its own. Authored DRAW never
/// reaches the log, like ROLL and IF.
fn apply_draw(args: &[Arg], ctx: &mut ApplyCtx) -> Result<ApplyResult> {
    let deck_id = match args.first() {
        Some(Arg::Ref {
            r#ref: Ref::Name { name, .. },
        }) => name.clone(),
        _ => {
            return Err(ChorusError::new(
                "DRAW needs a deck id: DRAW <DECK> [INTO target]",
            ))
        }
    };
    let Some(deck) = ctx.content.decks.get(&deck_id) else {
        return Err(ChorusError::new(format!("DRAW: unknown deck '{deck_id}'")));
    };
    let into = args.iter().find_map(|a| match a {
        Arg::Into { r#ref } => Some(r#ref),
        _ => None,
    });
    // §9.42: DRAW's INTO must RESOLVE — a role over empty event bindings or
    // an unresolvable ref would otherwise materialize the card nowhere,
    // silently. (SPAWN's own INTO stays lenient; a trigger that wants a DRAWN
    // card lands it with an explicit INTO — event bindings are empty, so the
    // no-INTO default below reads nothing in events either.)
    let into_concrete: Option<Arg> = match into {
        Some(into_ref) => {
            let dest: Option<String> = match into_ref {
                IntoTarget::Role { role } => ctx
                    .bindings
                    .get(role)
                    .and_then(|v| v.first())
                    .cloned(),
                IntoTarget::Ref(r#ref) => {
                    resolve_ref(ctx.state, r#ref)?.map(|e| e.id.clone())
                }
            };
            let Some(dest) = dest else {
                return Err(ChorusError::new(format!(
                    "DRAW: INTO target of '{deck_id}' cannot resolve"
                )));
            };
            Some(Arg::Into {
                r#ref: IntoTarget::Ref(id_ref(dest)),
            })
        }
        None => None,
    };
    // serve + advance the cursor; `draw_from` throws on an exhausted deck with
    // neither `reset` nor `defaultcard` — §9.19 (nothing logged, cursor
    // unmoved: the mutation below happens on the draft only after it returns)
    let cursor = ctx.state.decks.get(&deck_id).copied().unwrap_or(0);
    let drawn: DeckCard = draw_from(deck, cursor)?;
    ctx.state
        .decks
        .insert(deck_id.clone(), checked_step(cursor, 1, "deck cursor"));
    let mut spawn_args: Vec<Arg> = vec![Arg::Ref {
        r#ref: name_ref(drawn.name.clone(), Some(drawn.kind.clone())),
    }];
    if let Some(ic) = into_concrete {
        spawn_args.push(ic);
    }
    spawn_args.push(Arg::AsId { id: (ctx.id_gen)() });
    let spawn = plain("SPAWN".to_string(), spawn_args);
    let spawn_concrete = apply_outcome(&spawn, ctx)?.concrete;
    let mut concrete = vec![plain(
        "DRAWN".to_string(),
        vec![
            Arg::Ref {
                r#ref: name_ref(deck_id, None),
            },
            Arg::Ref {
                r#ref: name_ref(drawn.name, None),
            },
        ],
    )];
    concrete.extend(spawn_concrete);
    Ok(ApplyResult {
        concrete,
        refused: false,
    })
}

/// §9.39 IF / ELSE IF / ELSE in an outcome
/// list. Arms evaluate in order against the DRAFT (this list's earlier
/// outcomes — SET, SPAWN, DESTROY — are visible to later IFs, unlike recipe
/// selection which reads the input state); the first true condition wins;
/// ELSE is the default; no true arm and no ELSE applies nothing, without
/// error. Concretization follows the SET-delta precedent (§9.31): the log
/// records ONLY the taken branch's outcomes, flattened in position — the
/// record never contains an IF op, and the untaken branches are authored
/// discipline the log cannot recover. Conditions evaluate STRICTLY (§9.19):
/// an operand that cannot resolve — a path over a role with no bound card,
/// F4's unbound `roll` — rejects the whole step, where a `when:` predicate
/// would have read false.
fn apply_if(o: &Outcome, ctx: &mut ApplyCtx) -> Result<ApplyResult> {
    let arms: &[IfArm] = o.arms.as_deref().unwrap_or(&[]);
    for arm in arms {
        // the first true condition wins — and since a taken arm returns
        // immediately, no later arm's condition ever sees a prior arm's
        // effects (scoping the state borrow here is behavior-neutral)
        let chosen = {
            let pctx = PredCtx {
                b: &ctx.bindings,
                state: ctx.state,
                strict: true,
                roll: ctx.roll,
            };
            eval_pred(&arm.when, &pctx)?
        };
        if chosen {
            return apply_all(&arm.then, ctx);
        }
    }
    match o.fallback.as_deref() {
        Some(fallback) if !fallback.is_empty() => apply_all(fallback, ctx),
        _ => Ok(ApplyResult {
            concrete: Vec::new(),
            refused: false,
        }),
    }
}

/// Apply a branch body in order; a REFUSE
/// inside a taken branch refuses the whole step as usual (§6.5 — draft
/// discarded, attempt logged)
fn apply_all(outcomes: &[Outcome], ctx: &mut ApplyCtx) -> Result<ApplyResult> {
    let mut concrete: Vec<Outcome> = Vec::new();
    for sub in outcomes {
        let res = apply_outcome(sub, ctx)?;
        concrete.extend(res.concrete);
        if res.refused {
            return Ok(ApplyResult {
                concrete,
                refused: true,
            });
        }
    }
    Ok(ApplyResult {
        concrete,
        refused: false,
    })
}

/// `expires` materialization (§4.2): numbers are
/// already minutes — the lexer pre-converts bare duration literals (§5.1)
/// before SET sees them; quoted strings go through the one duration parser,
/// so every §5.1 form (`2halfhour`, `2h`, `90m`, `1d`) materializes
/// `expiresAt`.
fn to_minutes(v: &AttrValue) -> Result<i64> {
    match v {
        AttrValue::Num(n) => Ok(*n),
        AttrValue::Str(s) => parse_duration(s),
        AttrValue::Bool(b) => Err(ChorusError::new(format!("bad duration '{b}'"))),
    }
}

/// Resolve ref/role args to entity IDS
/// (role refs expand across the binding). §2.6.3: the ids come back (not
/// live entity handles) and each caller re-gets the entity,
/// so a mid-list DESTROY is seen by every later read (dead-stamp rule).
/// Role bindings push ALL their ids (dead ones included); name/id refs
/// resolve alive-only.
fn expand(args: &[Arg], ctx: &ApplyCtx) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for a in args {
        match a {
            Arg::Role { role } => out.extend(
                ctx.bindings
                    .get(role)
                    .map(|v| v.as_slice())
                    .unwrap_or(&[])
                    .iter()
                    .cloned(),
            ),
            Arg::Ref { r#ref } => {
                if let Ok(Some(e)) = resolve_ref(ctx.state, r#ref) {
                    out.push(e.id.clone());
                }
            }
            _ => {}
        }
    }
    out
}

/// The aspects clone: the top-level
/// `Object.entries` AND each sub-map spread are JS key-order points
/// (`{ ...v }` fronts integer-like keys).
fn clone_aspects(a: &Aspects) -> Aspects {
    let mut out = Aspects::new();
    for k in ordinary_own_property_keys(a) {
        if let Some(v) = a.get(&k) {
            out.insert(
                k,
                match v {
                    AspectVal::Num(n) => AspectVal::Num(*n),
                    AspectVal::Sub(m) => AspectVal::Sub(clone_map_in_js_order(m)),
                },
            );
        }
    }
    out
}

// ---------- fold support (§7: state = fold(log)) ----------

/// Apply one record's concrete outcomes to
/// a state (replay). Refused records are state no-ops: their logged outcomes
/// are journal history, never replayed effects (§6.5).
pub fn apply_record(
    record: &ActionRecord,
    state: &mut State,
    dict: &Dictionary,
    content: &CompiledContent,
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
    // never feeds mechanics, §9.46)
    if record.kind == "complete" {
        // the window SHOULD be registered by the turn's BEGIN row (fold
        // rebuilds it first); a row closing an unknown window is tolerated
        // with a tGame-anchored fallback so bare-row fold probes and
        // pre-window logs still replay — the START-clock anchoring that
        // matters for `expires` is exercised on the real path, where the
        // window exists (this fallback branch is load-bearing for
        // bare-row probes and MUST stay)
        let w_start = record
            .window
            .as_ref()
            .and_then(|k| state.windows.get(k))
            .map(|w| w.start);
        if let Some(k) = &record.window {
            state.windows.shift_remove(k); // JS Map.delete preserves insertion order
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
        };
        for o in &record.outcomes {
            apply_outcome(o, &mut ctx)?;
        }
        // ADVANCE replayed here only touches the inert `extra_advance_min` — the
        // extension lives in the wait row's span (§9.46 replay safety)
        return Ok(());
    }
    // refused: state no-op, clock untouched (§6.5) — this guard ALSO covers
    // action rows, so a refused BEGIN never opens its windows on replay
    if record.outcomes.iter().any(|o| o.op == "REFUSE") {
        return Ok(());
    }
    // events don't own the clock — only wait rows move it (§7, v0.5)
    if record.kind == "event" {
        // once-triggers replay as fired (§9.29); a missing trigger name folds
        // as the empty string
        state
            .fired
            .insert(record.trigger.clone().unwrap_or_default());
    }
    let start_min = abs_min(parse_clock(&record.t_game)?);
    if record.kind == "event" {
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
    let flat = |refs: &[Ref]| -> Result<Vec<String>> {
        let mut out = Vec::with_capacity(refs.len());
        for r in refs {
            if let Some(e) = resolve_ref(state, r)? {
                out.push(e.id.clone());
            }
        }
        Ok(out)
    };
    let mut b = RoleBindings::new();
    b.insert(Role::Actors, flat(&record.actors)?);
    b.insert(Role::Object, flat(&record.objects)?);
    b.insert(Role::With, flat(&record.instruments)?);
    b.insert(Role::At, flat(record.at.as_slice())?);
    b.insert(Role::About, flat(record.about.as_slice())?);
    b.insert(Role::To, flat(record.to.as_slice())?);
    Ok(b)
}

// ---------- events & completions: the advance pass (§6.6, §6.7, §9.16) ----------

/// The advance pass result: the rewritten state plus the records minted.
struct PassResult {
    state: State,
    records: Vec<ActionRecord>,
}

/// One hoisted expiry snapshot entry: `id`, `name`,
/// `at`. `name` is never read (fire_slot re-reads the LIVE entity's name);
/// kept to mirror the snapshot shape.
struct Expiring {
    id: String,
    #[allow(dead_code)]
    name: String,
    at: i64,
}

/// v0.5 — advance the world from `state.now` to `to_min` in ONE atomic pass.
/// Candidates merge chronologically: due windows close at their `end` (ordered
/// by end, then begin_id, then clause — a total order with no ties); authored
/// slot triggers and expiries fire at each crossed halfhour (§9.16 order:
/// triggers in declaration order, then expiries from the hoisted snapshot).
/// A completion at minute m fires BEFORE slot-m events. The pass ends with one
/// WAIT row — the only record that moves the clock — spanning exactly
/// `[state.now, to_min]`, so wait spans partition time (repeating triggers
/// cannot double-fire across windows). `ADVANCE` outcomes land while windows
/// complete and EXTEND the pass target; the extension rides the wait row's
/// span, which is what makes it replay-safe (fold never re-derives it).
fn resolve_due(
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
        return Ok(PassResult {
            state: state_in,
            records: Vec::new(),
        });
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
            name: e.name.clone(),
            at: match e.attrs.get("expiresAt") {
                Some(AttrValue::Num(n)) => *n,
                _ => unreachable!("filtered to a number above"),
            },
        })
        .collect();
    let mut cursor = from_min;
    let mut target = to_min;
    let mut id = first_record_id;
    loop {
        // due windows in (end, begin_id, clause)
        // order: a total order with no ties
        let mut due: Vec<&OpenWindow> =
            state.windows.values().filter(|w| w.end <= target).collect();
        due.sort_by(|a, b| {
            a.end
                .cmp(&b.end)
                .then(a.begin_id.cmp(&b.begin_id))
                .then(a.clause.cmp(&b.clause))
        });
        let next_w = due.first().map(|w| (*w).clone());
        drop(due);
        let slot_abs = next_slot_after(cursor, target);
        if let Some(w) = next_w {
            if slot_abs.is_none_or(|s| w.end <= s) {
                let fired = fire_window(content, dict, state, &w, id, id_gen)?;
                state = fired.state;
                records.push(fired.record);
                id += 1;
                // NOTE: the slot cursor does NOT jump to the window's end —
                // the halfhours the window spanned must still strike (a
                // completion at minute m precedes slot-m events, but
                // slot-(<m) events fire after it in the same pass, §6.6)
                if fired.advance_extra > 0 {
                    target = checked_step(target, fired.advance_extra, "advance extension");
                }
                continue;
            }
        }
        if let Some(slot_abs) = slot_abs {
            // `fire_slot` speaks in halfhour INDICES (§9.16: slot = abs % 48,
            // minute = abs * 30); the walk speaks in minutes
            let slot_records = fire_slot(
                content,
                dict,
                &mut state,
                slot_abs / MIN_PER_HALFHOUR,
                &expiring,
                id,
                id_gen,
            )?;
            id += slot_records.len() as i64;
            records.extend(slot_records);
            cursor = slot_abs;
            continue;
        }
        break;
    }
    records.push(make_wait_record(id, from_min, target - from_min, dict)?);
    state.now = target;
    Ok(PassResult { state, records })
}

/// The first halfhour boundary strictly
/// after `cursor`, or None past `target` — the (from, to] crossing rule of
/// §9.28, one boundary at a time
fn next_slot_after(cursor: i64, target: i64) -> Option<i64> {
    debug_assert!(
        cursor >= 0,
        "next_slot_after: negative cursor {cursor} (clocks are non-negative)"
    );
    // Math.floor == truncated division on the non-negative domain
    let abs = (cursor / MIN_PER_HALFHOUR + 1) * MIN_PER_HALFHOUR;
    if abs <= target {
        Some(abs)
    } else {
        None
    }
}

/// Close one window: re-resolve its
/// clause bindings (dead refs DROP — events do not respect locks, §6.7 v0.5),
/// apply the commit-time recipe's outcomes anchored to the window's START
/// clock (§6.7: clock-anchored effects use the start clock). A REFUSE that
/// only decides now, or an outcome that cannot apply, refuses the WINDOW: the
/// partial effects are discarded with the scratch, the attempt is logged with
/// a REFUSE marker (the fold's existing no-op rule), and the window still
/// closes — a pass that rolled back would leave the window retrying forever
/// (§9.19's completion analog).
struct FireWindowResult {
    state: State,
    record: ActionRecord,
    advance_extra: i64,
}

fn fire_window(
    content: &CompiledContent,
    dict: &Dictionary,
    state_in: State,
    w: &OpenWindow,
    record_id: i64,
    id_gen: &mut dyn FnMut() -> String,
) -> Result<FireWindowResult> {
    // the drift guard throws OUT of fire_window (and
    // out of the pass; the caller rejects the step)
    let recipe = content
        .verbs
        .get(&w.verb)
        .and_then(|v| v.recipes.iter().find(|r| r.id == w.recipe));
    let recipe = match recipe {
        Some(r) => r,
        None => {
            return Err(ChorusError::new(format!(
                "completion of {} (begin row {}): recipe '{}' is not in the compiled content — content drift between commit and completion",
                w.verb, w.begin_id, w.recipe
            )))
        }
    };
    let mut scratch = clone_state(&state_in);
    let mut bindings = RoleBindings::new();
    for role in ROLES {
        let mut ids = Vec::new();
        for r in &w.refs[&role] {
            // dead refs DROP
            if let Some(e) = resolve_ref(&scratch, r)? {
                ids.push(e.id.clone());
            }
        }
        bindings.insert(role, ids);
    }
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
    };
    let mut concrete: Vec<Outcome> = Vec::new();
    let mut refused = false;
    // the try/catch spans the outcome loop: an
    // outcome that cannot apply refuses the WINDOW with a REFUSE marker
    // carrying the message.
    for o in &recipe.out {
        if o.op == "PROGRESS" {
            continue; // the ticks rode the BEGIN row
        }
        match apply_outcome(o, &mut ctx) {
            Ok(res) => {
                concrete.extend(res.concrete);
                if res.refused {
                    refused = true;
                    break;
                }
            }
            Err(err) => {
                refused = true;
                concrete.clear();
                concrete.push(plain("REFUSE".to_string(), vec![Arg::Str { v: err.msg }]));
                break;
            }
        }
    }
    let advance_extra = ctx.extra_advance_min;
    drop(ctx);
    let mut out = if refused { state_in } else { scratch };
    out.windows.shift_remove(&w.key); // JS Map.delete preserves insertion order
    Ok(FireWindowResult {
        state: out,
        record: make_complete_record(record_id, w, concrete, dict)?,
        advance_extra,
    })
}

/// Build the COMPLETE row for one closed window.
fn make_complete_record(
    id: i64,
    w: &OpenWindow,
    outcomes: Vec<Outcome>,
    dict: &Dictionary,
) -> Result<ActionRecord> {
    let clock = clock_from_abs_min(w.end);
    let refs_of = |role: &Role| w.refs.get(role).cloned().unwrap_or_default();
    let mut rec = ActionRecord {
        id,
        kind: "complete".to_string(),
        trigger: Some(w.verb.clone()),
        t_game: format_clock(clock), // the completion clock — display only; mechanics anchor via the window
        t_real: None,
        advanced_min: 0, // complete rows never move the clock — waits do
        start_min: None,
        window: Some(w.key.clone()),
        actors: refs_of(&Role::Actors),
        verb: w.verb.clone(),
        objects: refs_of(&Role::Object),
        instruments: refs_of(&Role::With),
        at: w.refs.get(&Role::At).and_then(|v| v.first().cloned()),
        about: w.refs.get(&Role::About).and_then(|v| v.first().cloned()),
        to: w.refs.get(&Role::To).and_then(|v| v.first().cloned()),
        outcomes,
        recipes: vec![w.recipe.clone()],
        sentence: String::new(),
    };
    rec.sentence = format_sentence(
        &Sentence {
            kind: SentenceKind::Complete,
            clock: Some(clock),
            trigger: Some(w.verb.clone()),
            clauses: Vec::new(),
            outcomes: rec.outcomes.clone(),
            span_min: None,
        },
        Some(dict),
    )?;
    Ok(rec)
}

/// Build the WAIT row spanning exactly `[from_min, from_min + span_min]`.
fn make_wait_record(
    id: i64,
    from_min: i64,
    span_min: i64,
    dict: &Dictionary,
) -> Result<ActionRecord> {
    let clock = clock_from_abs_min(from_min);
    let mut rec = ActionRecord {
        id,
        kind: "wait".to_string(),
        trigger: None,
        t_game: format_clock(clock),
        t_real: None,
        advanced_min: span_min,
        start_min: Some(from_min), // exact span start — fold asserts continuation
        window: None,
        actors: Vec::new(),
        verb: String::new(),
        objects: Vec::new(),
        instruments: Vec::new(),
        at: None,
        about: None,
        to: None,
        outcomes: Vec::new(),
        recipes: Vec::new(),
        sentence: String::new(),
    };
    rec.sentence = format_sentence(
        &Sentence {
            kind: SentenceKind::Wait,
            clock: Some(clock),
            trigger: None,
            clauses: Vec::new(),
            outcomes: Vec::new(),
            span_min: Some(span_min),
        },
        Some(dict),
    )?;
    Ok(rec)
}

/// Fire one crossed halfhour: authored
/// triggers (declaration order) first, then expiries from the pass's snapshot
/// — §9.16 order, unchanged (§6.6).
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
        let CompiledTrigger::Slot {
            id: trig_id,
            slot,
            once,
            out,
        } = trig
        else {
            continue; // expiry triggers ride the expiry path below
        };
        if *slot != abs_slot % HALFHOURS_PER_DAY {
            continue;
        }
        if *once && state.fired.contains(trig_id) {
            continue; // §9.29: once means once
        }
        let mut outcomes: Vec<Outcome> = Vec::new();
        {
            let mut ctx = make_event_ctx(
                state,
                dict,
                content,
                abs_slot,
                first_event_id + records.len() as i64,
                id_gen,
            );
            for o in out {
                outcomes.extend(apply_outcome(o, &mut ctx)?.concrete);
            }
        }
        state.fired.insert(trig_id.clone()); // on the draft: a later rejection discards it
        let rid = first_event_id + records.len() as i64;
        records.push(make_event_record(rid, trig_id, abs_slot, outcomes, dict)?);
    }
    // expiries from the hoisted snapshot, in
    // snapshot order (= entities insertion order)
    for exp in expiring {
        if exp.at > abs_slot * MIN_PER_HALFHOUR {
            continue;
        }
        let ent_name = match state.entities.get(&exp.id) {
            Some(e) if e.dead_action.is_none() => e.name.clone(),
            _ => continue,
        };
        // an authored `on expire of NAME` trigger (§9.32) replaces the
        // built-in destroy; FIRST match in declaration order wins. Its
        // outcomes ARE the event record's outcomes (record.trigger = the
        // authored id). An outcome that cannot apply throws — the whole step
        // is rejected (§9.19).
        // §9.38 pool asymmetry (documented ruling): the trigger matches by
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
                {
                    let mut ctx = make_event_ctx(
                        state,
                        dict,
                        content,
                        abs_slot,
                        first_event_id + records.len() as i64,
                        id_gen,
                    );
                    for o in a.out() {
                        outcomes.extend(apply_outcome(o, &mut ctx)?.concrete);
                    }
                }
                (a.id().to_string(), outcomes)
            }
            None => {
                // engine-written destroys are keyed
                // to the EXPIRING entity and concretize to DESTROY #id —
                // under a pool a name-addressed destroy would kill the oldest
                // copy while the real expiring card re-fires
                let destroy = plain(
                    "DESTROY".to_string(),
                    vec![Arg::Ref {
                        r#ref: id_ref(exp.id.clone()),
                    }],
                );
                let mut outcomes: Vec<Outcome> = Vec::new();
                {
                    let mut ctx = make_event_ctx(
                        state,
                        dict,
                        content,
                        abs_slot,
                        first_event_id + records.len() as i64,
                        id_gen,
                    );
                    outcomes.extend(apply_outcome(&destroy, &mut ctx)?.concrete);
                }
                (format!("expire:{ent_name}"), outcomes)
            }
        };
        // play marks fired exactly as the fold's `apply_record` does — one entry
        // per EVENT row's trigger (§9.34); on the draft, so a rejection
        // discards it
        state.fired.insert(trigger_id.clone());
        let rid = first_event_id + records.len() as i64;
        records.push(make_event_record(rid, &trigger_id, abs_slot, outcomes, dict)?);
    }
    Ok(records)
}

/// Event bindings are EMPTY ({}), and
/// `now` is the crossed boundary in absolute minutes
/// (abs_slot * MIN_PER_HALFHOUR).
fn make_event_ctx<'a>(
    state: &'a mut State,
    dict: &'a Dictionary,
    content: &'a CompiledContent,
    abs_slot: i64,
    record_id: i64,
    id_gen: &'a mut dyn FnMut() -> String,
) -> ApplyCtx<'a> {
    ApplyCtx {
        state,
        dict,
        content,
        bindings: RoleBindings::new(),
        now: abs_slot * MIN_PER_HALFHOUR,
        record_id,
        id_gen,
        extra_advance_min: 0,
        roll: None,
    }
}

/// Build the EVENT row for one fired trigger at one crossed boundary.
fn make_event_record(
    id: i64,
    trigger: &str,
    abs_slot: i64,
    outcomes: Vec<Outcome>,
    dict: &Dictionary,
) -> Result<ActionRecord> {
    let clock = clock_from_abs_min(abs_slot * MIN_PER_HALFHOUR);
    let mut rec = ActionRecord {
        id,
        kind: "event".to_string(),
        trigger: Some(trigger.to_string()),
        t_game: format_clock(clock),
        t_real: None,
        advanced_min: 0,
        start_min: None,
        window: None,
        actors: Vec::new(),
        verb: String::new(),
        objects: Vec::new(),
        instruments: Vec::new(),
        at: None,
        about: None,
        to: None,
        outcomes,
        recipes: Vec::new(),
        sentence: String::new(),
    };
    rec.sentence = format_sentence(
        &Sentence {
            kind: SentenceKind::Event,
            clock: Some(clock),
            trigger: Some(trigger.to_string()),
            clauses: Vec::new(),
            outcomes: rec.outcomes.clone(),
            span_min: None,
        },
        Some(dict),
    )?;
    Ok(rec)
}

// ---------- arithmetic / format helpers (borrow-shape glue) ----------

/// §3.3 accumulation discipline: JS numbers accumulate rng counters, deck
/// cursors, advance targets, window offsets and wait spans in f64 (exact to
/// 2^53). i64 `checked_add` overflow is unreachable for any real log — the
/// documented divergence direction is saturation, with a debug assert on the
/// way.
fn checked_step(a: i64, b: i64, what: &str) -> i64 {
    match a.checked_add(b) {
        Some(v) => v,
        None => {
            debug_assert!(
                false,
                "{what}: i64 overflow {a} + {b} (f64 accumulation kept going; documented saturation divergence)"
            );
            i64::MAX
        }
    }
}

/// JS truthiness over container-shaped values (the `if (container)` guards):
/// '' / 0 / false are falsy.
fn js_truthy(v: Option<&AttrValue>) -> bool {
    match v {
        None => false,
        Some(AttrValue::Str(s)) => !s.is_empty(),
        Some(AttrValue::Num(n)) => *n != 0,
        Some(AttrValue::Bool(b)) => *b,
    }
}

/// `JSON.stringify(aspects)`. Plain JSON.stringify
/// applies the JS key order (OrdinaryOwnPropertyKeys), so the map goes
/// through [`jsjson_obj_in_js_order`].
/// pub(crate): the reach analyzer re-uses this exact frozen format for its
/// provenance/demand chain lines.
pub(crate) fn aspects_to_json(a: &Aspects) -> String {
    let mut m: IndexMap<String, JsJson> = IndexMap::new();
    for (k, v) in a {
        m.insert(
            k.clone(),
            match v {
                AspectVal::Num(n) => JsJson::Int(*n),
                AspectVal::Sub(sub) => JsJson::Obj(
                    sub.iter()
                        .map(|(k2, n2)| (k2.clone(), JsJson::Int(*n2)))
                        .collect(),
                ),
            },
        );
    }
    jsjson(&jsjson_obj_in_js_order(&m))
}

/// `JSON.stringify(attrs)` (same JS key-order rule).
fn attrs_to_json(a: &Attrs) -> String {
    let mut m: IndexMap<String, JsJson> = IndexMap::new();
    for (k, v) in a {
        m.insert(
            k.clone(),
            match v {
                AttrValue::Str(s) => JsJson::Str(s.clone()),
                AttrValue::Num(n) => JsJson::Int(*n),
                AttrValue::Bool(b) => JsJson::Bool(*b),
            },
        );
    }
    jsjson(&jsjson_obj_in_js_order(&m))
}

/// `JSON.parse(pair.v) as Aspects`. Values are
/// numbers or one-level sub-maps of numbers (the Q13 model).
fn aspects_from_json(s: &str) -> Result<Aspects> {
    let v: serde_json::Value = serde_json::from_str(s)
        .map_err(|e| ChorusError::new(format!("bad aspects pair: {e}")))?;
    let serde_json::Value::Object(map) = v else {
        return Err(ChorusError::new("bad aspects pair: not an object"));
    };
    let mut out = Aspects::new();
    for (k, val) in map {
        out.insert(k, json_to_aspect_val(&val)?);
    }
    Ok(out)
}

fn json_to_aspect_val(v: &serde_json::Value) -> Result<AspectVal> {
    match v {
        serde_json::Value::Number(n) => Ok(AspectVal::Num(
            n.as_i64()
                .ok_or_else(|| ChorusError::new("bad aspects pair: non-integer number"))?,
        )),
        serde_json::Value::Object(sub) => {
            let mut m = IndexMap::new();
            for (k, val) in sub {
                let serde_json::Value::Number(n) = val else {
                    return Err(ChorusError::new(
                        "bad aspects pair: sub-map values must be numbers",
                    ));
                };
                m.insert(
                    k.clone(),
                    n.as_i64()
                        .ok_or_else(|| ChorusError::new("bad aspects pair: non-integer number"))?,
                );
            }
            Ok(AspectVal::Sub(m))
        }
        _ => Err(ChorusError::new(
            "bad aspects pair: values must be numbers",
        )),
    }
}

/// Parse an attrs JSON pair: a flat object whose values are
/// strings/numbers/booleans.
/// Key order follows the document (serde_json
/// preserve_order), which is what `Object.assign` appends in.
fn attrs_from_json(s: &str) -> Result<Attrs> {
    let v: serde_json::Value = serde_json::from_str(s)
        .map_err(|e| ChorusError::new(format!("bad attrs pair: {e}")))?;
    let serde_json::Value::Object(map) = v else {
        return Err(ChorusError::new("bad attrs pair: not an object"));
    };
    let mut out = Attrs::new();
    for (k, val) in map {
        out.insert(k, json_to_attr_value(&val)?);
    }
    Ok(out)
}

fn json_to_attr_value(v: &serde_json::Value) -> Result<AttrValue> {
    match v {
        serde_json::Value::String(s) => Ok(AttrValue::Str(s.clone())),
        serde_json::Value::Number(n) => Ok(AttrValue::Num(
            n.as_i64()
                .ok_or_else(|| ChorusError::new("bad attrs pair: non-integer number"))?,
        )),
        serde_json::Value::Bool(b) => Ok(AttrValue::Bool(*b)),
        _ => Err(ChorusError::new(
            "bad attrs pair: values must be strings, numbers or booleans",
        )),
    }
}

// --------------------------------------------------------------------------
// Smoke tests: minimal per-op / per-phase proof-of-life.
// --------------------------------------------------------------------------
#[cfg(test)]
mod smoke {
    use super::*;
    use crate::content::content::{compile_content, ContentRaw, PrototypeDef, RecipeDef, SlotDef, VerbContent};
    use crate::core::dict::core_dictionary;
    use crate::core::types::new_state;

    fn pace(duration: i64, out: &[&str]) -> VerbContent {
        VerbContent {
            slots: vec![SlotDef {
                role: "actors".into(),
                min: 1,
                max: Some(1),
                query: Some("kind:character".into()),
                shown: None,
            }],
            duration,
            recipes: vec![RecipeDef {
                id: None,
                when: None,
                shown: None,
                hint: None,
                out: out.iter().map(|s| s.to_string()).collect(),
            }],
        }
    }

    /// Test rig — code-registered verbs (§10).
    fn rig(verbs: Vec<(&str, VerbContent)>, extra: impl FnOnce(&mut ContentRaw)) -> (Dictionary, CompiledContent) {
        let mut dict = core_dictionary();
        let mut map: IndexMap<String, VerbContent> = IndexMap::new();
        for (name, v) in verbs {
            if !dict.is_verb(name) {
                dict.register_verb(name).unwrap();
            }
            map.insert(name.to_string(), v);
        }
        let mut raw = ContentRaw {
            verbs: map,
            ..ContentRaw::default()
        };
        extra(&mut raw);
        let content = compile_content(&raw, &dict, &[]).unwrap();
        (dict, content)
    }

    fn seed_state() -> State {
        let mut s = new_state(0);
        let card = |id: &str, kind: &str, name: &str, container: Option<&str>| Entity {
            id: id.to_string(),
            kind: kind.to_string(),
            name: name.to_string(),
            aspects: Aspects::new(),
            attrs: match container {
                Some(c) => Attrs::from([("container".to_string(), AttrValue::Str(c.to_string()))]),
                None => Attrs::new(),
            },
            born_action: None,
            dead_action: None,
            stackable: false,
        };
        add_entity(&mut s, card("house", "place", "HOUSE", None)).unwrap();
        add_entity(&mut s, card("ana", "character", "ANA", Some("house"))).unwrap();
        add_entity(&mut s, card("book", "text", "MAGICK_BOOK", Some("house"))).unwrap();
        s
    }

    fn run(content: &CompiledContent, dict: &Dictionary, state: State, src: &str, first: i64) -> ExecResult {
        let s = parse_sentence(src, dict).unwrap();
        execute_step(
            content,
            dict,
            state,
            Step {
                start_min: None,
                clock: Some(s.clock.unwrap_or(Clock { day: 0, halfhour: 0 })),
                clauses: s.clauses,
                t_real: None,
            },
            ExecOptions {
                first_record_id: first,
                id_gen: None,
            },
        )
        .unwrap()
    }

    fn advance(content: &CompiledContent, dict: &Dictionary, state: State, clock: &str, first: i64) -> ExecResult {
        execute_step(
            content,
            dict,
            state,
            Step {
                start_min: None,
                clock: Some(parse_clock(clock).unwrap()),
                clauses: Vec::new(),
                t_real: None,
            },
            ExecOptions {
                first_record_id: first,
                id_gen: None,
            },
        )
        .unwrap()
    }

    #[test]
    fn begin_advance_complete_walks_the_four_phases() {
        let (dict, content) = rig(vec![("PACE", pace(30, &["NARRATE \"done.\""]))], |_| {});
        let r = run(&content, &dict, seed_state(), "0:0 @ ANA PACE", 1);
        assert_eq!(r.rejected, None);
        assert_eq!(r.records.len(), 1);
        assert_eq!(r.records[0].kind, "action");
        assert_eq!(r.records[0].recipes, ["pace.default"]);
        assert_eq!(r.records[0].advanced_min, 30);
        assert_eq!(r.state.now, 0, "beginning moves nothing");
        assert_eq!(r.state.windows.len(), 1);
        // the pass: completion then the wait row
        let adv = advance(&content, &dict, r.state, "0:1", 2);
        assert_eq!(adv.records.iter().map(|x| x.kind.as_str()).collect::<Vec<_>>(), ["complete", "wait"]);
        assert_eq!(adv.state.now, 30);
        assert_eq!(adv.state.windows.len(), 0);
    }

    #[test]
    fn all_seventeen_ops_smoke() {
        // one recipe exercising every op; ENDING first-wins, REFUSE checked
        // separately (it refuses the step), ROLLED/DRAWN engine-written.
        let out = [
            "RELATE(ANA, MAGICK_BOOK, parent_child)",
            "SPAWN CLUE_SECRET_HIDEOUT INTO at AS #e_minted",
            "SET(MAGICK_BOOK, aspect:secret, 3)",
            "SET(MAGICK_BOOK, curse, +2)",
            "SET(MAGICK_BOOK, note, \"scribbled\")",
            "REVEAL MAGICK_BOOK",
            "MOVE MAGICK_BOOK INTO ANA",
            "ADVANCE(1h)",
            "NARRATE \"bump.\"",
            "PROGRESS(30m, \"ticking.\")",
            "SPAWN CLUE_SECRET_HIDEOUT AS #e_extra",
            "DESTROY OBJECT",
            "ENDING QUIET_DUSK",
            "ENDING OTHER_END",
        ];
        let mut verbs: Vec<(&str, VerbContent)> = vec![(
            "STUDY",
            VerbContent {
                slots: vec![
                    SlotDef { role: "actors".into(), min: 1, max: Some(1), query: Some("kind:character".into()), shown: None },
                    SlotDef { role: "object".into(), min: 1, max: Some(1), query: None, shown: None },
                    SlotDef { role: "at".into(), min: 0, max: Some(1), query: Some("kind:place".into()), shown: None },
                ],
                duration: 30,
                recipes: vec![RecipeDef {
                    id: None,
                    when: None,
                    shown: None,
                    hint: None,
                    out: out.iter().map(|s| s.to_string()).collect(),
                }],
            },
        )];
        verbs.push(("REL", pace(0, &["UNRELATE(ANA, MAGICK_BOOK, parent_child)"])));
        let (dict, content) = rig(verbs, |raw| {
            raw.endings = Some(
                [
                    (
                        "QUIET_DUSK".to_string(),
                        crate::content::content::EndingDef {
                            title: "A quiet dusk.".to_string(),
                            flavour: Some("calm".to_string()),
                        },
                    ),
                    (
                        "OTHER_END".to_string(),
                        crate::content::content::EndingDef {
                            title: "Another.".to_string(),
                            flavour: None,
                        },
                    ),
                ]
                .into_iter()
                .collect(),
            );
            raw.prototypes = Some(
                [(
                    "CLUE_SECRET_HIDEOUT".to_string(),
                    PrototypeDef {
                        kind: "idea".to_string(),
                        stackable: true,
                        aspects: Some(Attrs::from([("secret".to_string(), AttrValue::Num(1))])
                            .into_iter()
                            .map(|(k, v)| (k, AspectVal::Num(match v { AttrValue::Num(n) => n, _ => 0 })))
                            .collect()),
                        attrs: None,
                        derive: None,
                    },
                )]
                .into_iter()
                .collect(),
            );
        });
        // the study turn: BEGIN carries only PROGRESS
        let r = run(&content, &dict, seed_state(), "0:0 @ ANA STUDY MAGICK_BOOK AT HOUSE", 1);
        assert_eq!(r.rejected, None, "rejected: {:?}", r.rejected);
        assert!(r.records[0]
            .outcomes
            .iter()
            .all(|o| o.op == "PROGRESS"), "begin carries only the PROGRESS tick");
        assert_eq!(r.state.ended, None, "the ending rides the completion");
        // the completion: the recipe's real outcomes land there
        let adv = advance(&content, &dict, r.state, "0:1", 2);
        assert_eq!(adv.rejected, None, "rejected: {:?}", adv.rejected);
        let complete = adv.records.iter().find(|x| x.kind == "complete").unwrap();
        let mut ops: Vec<&str> = complete.outcomes.iter().map(|o| o.op.as_str()).collect();
        // IF/ROLL/DRAW/ROLLED/DRAWN are exercised by their own smokes below;
        // REFUSE cannot co-apply (it discards the draft) — its smoke is
        // separate. PROGRESS rode the BEGIN row (skipped at completion).
        // ENDING x2: the second is a logged no-op.
        for op in ["RELATE", "SPAWN", "SET", "SET", "SET", "REVEAL", "MOVE", "ADVANCE", "NARRATE", "SPAWN", "DESTROY", "ENDING", "ENDING"] {
            assert_eq!(ops.first().copied(), Some(op), "op order at {ops:?}");
            ops.remove(0);
        }
        let st = &adv.state;
        assert_eq!(st.ended.as_ref().unwrap().id, "QUIET_DUSK", "first ENDING wins");
        // SET aspect + delta + str all landed
        let book = st.entities.get("book").unwrap();
        assert_eq!(book.aspects.get("secret"), Some(&AspectVal::Num(3)));
        assert_eq!(book.attrs.get("curse"), Some(&AttrValue::Num(2)));
        assert_eq!(book.attrs.get("note"), Some(&AttrValue::Str("scribbled".into())));
        assert_eq!(book.attrs.get("state.hidden"), Some(&AttrValue::Bool(false)));
        assert_eq!(book.attrs.get("container"), Some(&AttrValue::Str("ana".into())));
        // RELATE landed, UNRELATE (second verb) will stamp it
        assert!(st.relations.contains_key("ana|book|parent_child"));
        // both spawns share the stackable name pool
        assert_eq!(st.by_name.get("CLUE_SECRET_HIDEOUT").map(|v| v.len()), Some(2));
        // ADVANCE extended the pass target: 30 (window) + 60 (ADVANCE)
        assert_eq!(adv.state.now, 90);
    }

    #[test]
    fn unrelate_stamps_the_relation_dead() {
        let rel_verb = VerbContent {
            slots: vec![
                SlotDef { role: "actors".into(), min: 1, max: Some(1), query: Some("kind:character".into()), shown: None },
                SlotDef { role: "object".into(), min: 1, max: Some(1), query: None, shown: None },
            ],
            duration: 0,
            recipes: vec![RecipeDef {
                id: None,
                when: None,
                shown: None,
                hint: None,
                out: vec!["RELATE(ANA, MAGICK_BOOK, parent_child)".to_string()],
            }],
        };
        let unrel_verb = VerbContent {
            slots: rel_verb.slots.clone(),
            duration: 0,
            recipes: vec![RecipeDef {
                id: None,
                when: None,
                shown: None,
                hint: None,
                out: vec!["UNRELATE(ANA, MAGICK_BOOK, parent_child)".to_string()],
            }],
        };
        let (dict, content) = rig(vec![("LINK", rel_verb), ("UNLINK", unrel_verb)], |_| {});
        let s1 = run(&content, &dict, seed_state(), "0:0 @ ANA LINK MAGICK_BOOK", 1);
        assert_eq!(s1.rejected, None, "{:?}", s1.rejected);
        let c1 = advance(&content, &dict, s1.state, "0:0", 2);
        assert!(c1
            .state
            .relations
            .contains_key("ana|book|parent_child"));
        let s2 = run(&content, &dict, c1.state, "0:0 @ ANA UNLINK MAGICK_BOOK", 5);
        assert_eq!(s2.rejected, None, "{:?}", s2.rejected);
        let c2 = advance(&content, &dict, s2.state, "0:0", 6);
        let rel = c2.state.relations.get("ana|book|parent_child").unwrap();
        assert!(rel.dead_action.is_some(), "UNRELATE stamps, never erases (§7)");
    }

    #[test]
    fn refuse_gate_rejects_at_the_table_with_id_recycle() {
        let (dict, content) = rig(
            vec![
                ("PACE", pace(30, &["NARRATE \"done.\""])),
                ("GATE", pace(30, &["REFUSE(\"no.\")"])),
            ],
            |_| {},
        );
        let state = seed_state();
        let r1 = run(&content, &dict, state, "0:0 @ ANA PACE", 1);
        assert_eq!(r1.state.windows.len(), 1);
        // the GATE step is refused: the advance (complete+wait) is discarded,
        // the attempt row takes first_record_id = 2, state untouched
        let r2 = run(&content, &dict, clone_state(&r1.state), "0:1 @ ANA GATE", 2);
        assert_eq!(r2.rejected, None);
        assert_eq!(r2.records.len(), 1);
        assert_eq!(r2.records[0].id, 2, "the refusal row reuses firstRecordId");
        assert_eq!(r2.records[0].advanced_min, 0);
        assert!(r2.records[0].outcomes.iter().any(|o| o.op == "REFUSE"));
        assert_eq!(r2.state.now, 0, "clock unmoved");
        assert_eq!(r2.state.windows.len(), 1, "the window the pass would have closed is still open");
        // same inputs again → the SAME id is minted again (id recycle)
        let r3 = run(&content, &dict, r1.state, "0:1 @ ANA GATE", 2);
        assert_eq!(r3.records[0].id, 2);
    }

    #[test]
    fn roll_binds_per_clause_and_rewrites_roll_prose() {
        let roll_verb = VerbContent {
            slots: vec![SlotDef {
                role: "actors".into(),
                min: 1,
                max: Some(1),
                query: Some("kind:character".into()),
                shown: None,
            }],
            duration: 0,
            recipes: vec![RecipeDef {
                id: None,
                when: None,
                shown: None,
                hint: None,
                out: vec![
                    "ROLL(2, 6)".to_string(),
                    "NARRATE \"rolled {roll}.\"".to_string(),
                ],
            }],
        };
        let (dict, content) = rig(vec![("CAST", roll_verb)], |_| {});
        let seed = content.seed;
        let r = run(&content, &dict, seed_state(), "0:0 @ ANA CAST", 1);
        assert_eq!(r.rejected, None);
        assert!(r.records[0].outcomes.is_empty(), "ROLL is not PROGRESS — BEGIN carries nothing");
        let adv = advance(&content, &dict, r.state, "0:0", 2);
        assert_eq!(adv.rejected, None);
        let complete = adv.records.iter().find(|x| x.kind == "complete").unwrap();
        let outcomes = &complete.outcomes;
        assert_eq!(outcomes[0].op, "ROLLED");
        let (Arg::Int { v: total }, Arg::Int { v: ctr_after }) = (&outcomes[0].args[0], &outcomes[0].args[1]) else {
            panic!("ROLLED(total, ctrAfter)");
        };
        let expect: i64 = (0..2).map(|i| roll_die(seed, i, 6)).sum();
        assert_eq!(*total, expect);
        assert_eq!(*ctr_after, 2);
        assert_eq!(adv.state.rng, 2);
        // the concrete NARRATE carries the NUMBER, not {roll}
        let Arg::Str { v } = &outcomes[1].args[0] else {
            panic!("NARRATE str");
        };
        assert_eq!(v, &format!("rolled {expect}."));
        // the fold lands the same counter from the ROLLED row (law 3)
        let mut folded = new_state(0);
        for rec in &adv.records {
            apply_record(rec, &mut folded, &dict, &content).unwrap();
        }
        assert_eq!(folded.rng, 2);
    }

    #[test]
    fn draw_concretizes_to_drawn_plus_spawn_and_fold_asserts() {
        let draw_verb = VerbContent {
            slots: vec![SlotDef {
                role: "actors".into(),
                min: 1,
                max: Some(1),
                query: Some("kind:character".into()),
                shown: None,
            }],
            duration: 0,
            recipes: vec![RecipeDef {
                id: None,
                when: None,
                shown: None,
                hint: None,
                out: vec!["DRAW RUMOURS".to_string()],
            }],
        };
        let (dict, content) = rig(vec![("DRAWV", draw_verb)], |raw| {
            raw.decks = Some(vec![crate::content::content::DeckRaw {
                id: "RUMOURS".to_string(),
                spec: vec!["HEARSAY".to_string(), "SCANDAL".to_string()],
                defaultcard: None,
                reset: false,
            }]);
            raw.prototypes = Some(
                [("HEARSAY", "idea"), ("SCANDAL", "idea")]
                    .into_iter()
                    .map(|(n, k)| {
                        (
                            n.to_string(),
                            PrototypeDef {
                                kind: k.to_string(),
                                ..PrototypeDef::default()
                            },
                        )
                    })
                    .collect(),
            );
        });
        let r = run(&content, &dict, seed_state(), "0:0 @ ANA DRAWV", 1);
        assert_eq!(r.rejected, None, "{:?}", r.rejected);
        let adv = advance(&content, &dict, r.state, "0:0", 2);
        assert_eq!(adv.rejected, None, "{:?}", adv.rejected);
        let complete = adv.records.iter().find(|x| x.kind == "complete").unwrap();
        let ops: Vec<&str> = complete.outcomes.iter().map(|o| o.op.as_str()).collect();
        assert_eq!(ops, ["DRAWN", "SPAWN"], "DRAW concretizes to DRAWN+SPAWN");
        assert_eq!(adv.state.decks.get("RUMOURS"), Some(&1));
        // fold: the DRAWN row advances the cursor; the fold state matches play
        let mut folded = new_state(0);
        for rec in &adv.records {
            apply_record(rec, &mut folded, &dict, &content).unwrap();
        }
        assert_eq!(folded.decks, adv.state.decks);
        // drift turns loud: a DRAWN row naming a card the deck never serves
        let mut drifted = adv.records.clone();
        for o in &mut drifted[0].outcomes {
            if o.op == "DRAWN" {
                if let Arg::Ref {
                    r#ref: Ref::Name { name, .. },
                } = &mut o.args[1]
                {
                    *name = "NOSUCH".to_string();
                }
            }
        }
        let mut bad = new_state(0);
        let err = apply_record(&drifted[0], &mut bad, &dict, &content).unwrap_err();
        assert!(err.msg.contains("content drift between play and fold"), "{err}");
    }

    #[test]
    fn if_flattens_the_taken_branch_and_defers_to_completion() {
        let late = VerbContent {
            slots: vec![SlotDef {
                role: "actors".into(),
                min: 1,
                max: Some(1),
                query: Some("kind:character".into()),
                shown: None,
            }],
            duration: 30,
            recipes: vec![RecipeDef {
                id: None,
                when: None,
                shown: None,
                hint: None,
                out: vec![
                    "IF exists(HOUSE) -> (SPAWN PARANOIA, REFUSE(\"too late.\")) ELSE -> (NARRATE \"calm.\")".to_string(),
                ],
            }],
        };
        let (dict, content) = rig(vec![("LATE", late)], |raw| {
            raw.prototypes = Some(
                [(
                    "PARANOIA".to_string(),
                    PrototypeDef {
                        kind: "state".to_string(),
                        ..PrototypeDef::default()
                    },
                )]
                .into_iter()
                .collect(),
            );
        });
        // the IF is opaque to the gate: the window OPENS
        let r = run(&content, &dict, seed_state(), "0:0 @ ANA LATE", 1);
        assert_eq!(r.rejected, None);
        assert_eq!(r.state.windows.len(), 1);
        assert!(!r.state.by_name.contains_key("PARANOIA"));
        // the completion evaluates the IF against the completion-time world:
        // HOUSE exists → SPAWN then REFUSE; the spawn dies with the scratch…
        let begin_state = clone_state(&r.state);
        let adv = advance(&content, &dict, r.state, "0:31", 2);
        assert_eq!(adv.rejected, None);
        let complete = adv.records.iter().find(|x| x.kind == "complete").unwrap();
        let ops: Vec<&str> = complete.outcomes.iter().map(|o| o.op.as_str()).collect();
        assert_eq!(ops, ["SPAWN", "REFUSE"], "only the TAKEN branch is recorded");
        assert!(!adv.state.by_name.contains_key("PARANOIA"), "the scratch died with the refusal");
        assert_eq!(adv.state.windows.len(), 0, "…and the window still closed");
        // the fold replays the SPAWN (the refused COMPLETION is a no-op, but
        // the SPAWN is ON the refused row) — so the fold must NOT spawn either
        let mut folded = begin_state;
        for rec in &adv.records {
            apply_record(rec, &mut folded, &dict, &content).unwrap();
        }
        assert!(!folded.by_name.contains_key("PARANOIA"));
        assert_eq!(folded.windows.len(), 0);
    }

    #[test]
    fn register_windows_roundtrips_the_sentence_and_silently_skips_degenerate_rows() {
        // Q6: a BEGIN row whose sentence fails to parse opens NO window
        let (dict, content) = rig(vec![("PACE", pace(30, &["NARRATE \"d.\""]))], |_| {});
        let mut rec = ActionRecord {
            id: 7,
            kind: "action".to_string(),
            trigger: None,
            t_game: "0:0".to_string(),
            t_real: None,
            advanced_min: 30,
            start_min: Some(0),
            window: None,
            actors: vec![],
            verb: String::new(),
            objects: vec![],
            instruments: vec![],
            at: None,
            about: None,
            to: None,
            outcomes: vec![],
            recipes: vec!["pace.default".to_string()],
            // a degenerate sentence: quotes never close — unparseable
            sentence: "0:0 @ \"unclosed PACE".to_string(),
        };
        let mut st = seed_state();
        register_windows(&rec, &mut st, &dict, &content).unwrap();
        assert_eq!(st.windows.len(), 0, "round-trip failure silently opens no window");
        // the healthy row: the SAME code path re-parses the canonical sentence
        rec.sentence = "0:0 @ ANA PACE".to_string();
        register_windows(&rec, &mut st, &dict, &content).unwrap();
        assert_eq!(st.windows.len(), 1);
        let w = st.windows.get("7:0").unwrap();
        assert_eq!((w.start, w.end, w.verb.as_str()), (0, 30, "PACE"));
        assert!(w.locked.is_empty(), "actors are never locked (COMMITTED_ROLES exclude actors)");
    }

    #[test]
    fn records_serialize_in_the_jsonl_wire_field_order() {
        // the JSONL wire order is the pinned object-literal order.
        let (dict, content) = rig(
            vec![("PACE", pace(30, &["NARRATE \"d.\""]))],
            |_| {},
        );
        let r = run(&content, &dict, seed_state(), "0:0 @ ANA PACE", 1);
        let line = serde_json::to_string(&r.records[0]).unwrap();
        // hand-written JSON.stringify shape for a BEGIN row
        let expected = concat!(
            r#"{"id":1,"kind":"action","trigger":null,"tGame":"0:0","tReal":null,"advancedMin":30,"startMin":0,"#,
            r#""window":null,"actors":[{"form":"name","name":"ANA"}],"verb":"PACE","objects":[],"instruments":[],"#,
            r#""at":null,"about":null,"to":null,"outcomes":[],"recipes":["pace.default"],"sentence":"0:0 @ ANA PACE"}"#
        );
        assert_eq!(line, expected);
        // parse-back is lossless
        let back: ActionRecord = serde_json::from_str(&line).unwrap();
        assert_eq!(back, r.records[0]);
    }

    #[test]
    fn rejected_messages_are_byte_exact() {
        let (dict, content) = rig(vec![("PACE", pace(30, &["NARRATE \"d.\""]))], |_| {});
        // before-now
        let r = execute_step(
            &content,
            &dict,
            {
                let mut s = seed_state();
                s.now = 60;
                s
            },
            Step {
                start_min: None,
                clock: Some(Clock { day: 0, halfhour: 1 }),
                clauses: vec![],
                t_real: None,
            },
            ExecOptions { first_record_id: 1, id_gen: None },
        )
        .unwrap();
        assert_eq!(
            r.rejected.as_deref(),
            Some("clock 0:1 is before state.now (0:2)")
        );
        // nothing to do (the live to_min == now && !has_due branch)
        let r = advance(&content, &dict, seed_state(), "0:0", 1);
        assert!(
            r.rejected
                .as_deref()
                .is_some_and(|m| m.starts_with("nothing to do — the clock already reads 0:0")),
            "{:?}",
            r.rejected
        );
        // story ended
        let mut ended = seed_state();
        ended.ended = Some(Ending {
            id: "E".into(),
            title: "It is over.".into(),
            flavour: "calm".into(),
        });
        let r = run(&content, &dict, ended, "0:1 @ ANA PACE", 1);
        assert_eq!(r.rejected.as_deref(), Some("the story has ended — It is over."));
        // wait-partition fold assertion
        let mut st = new_state(5);
        let err = apply_record(
            &ActionRecord {
                id: 1,
                kind: "wait".to_string(),
                trigger: None,
                t_game: "0:0".to_string(),
                t_real: None,
                advanced_min: 30,
                start_min: Some(0),
                window: None,
                actors: vec![],
                verb: String::new(),
                objects: vec![],
                instruments: vec![],
                at: None,
                about: None,
                to: None,
                outcomes: vec![],
                recipes: vec![],
                sentence: String::new(),
            },
            &mut st,
            &dict,
            &content,
        )
        .unwrap_err();
        assert!(err.msg.contains("wait spans must partition time"), "{err}");
    }
}
