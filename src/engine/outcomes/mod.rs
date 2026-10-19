//! Outcome application (spec §4.4): the [`ApplyCtx`] borrow face, the
//! 20-op dispatch, and the small ops. The SPAWN/REPLACE mint rules live in
//! `mint`; the §6.7 RUN chain in `chain`.
//!
//! Ownership model: [`ApplyCtx`] holds `&mut State`; bindings hold entity
//! IDS (`RoleBindings = IndexMap<Role, Vec<String>>`), so every read goes
//! through `state.entities.get(&id)` and sees the CURRENT `dead_action`
//! (the dead-stamp rule); `expand` resolves to ids and the caller re-gets
//! each entity, which splits the `&`/`&mut` borrows cleanly. No
//! Rc/Arc/RefCell/Cell anywhere.

use indexmap::IndexMap;

use crate::content::{draw_from, CompiledContent};
use crate::core::clock::parse_duration;
use crate::core::dice::roll_die;
use crate::core::dict::Dictionary;

use crate::core::predicate::{eval_expr, eval_pred, PredCtx, RoleBindings};
use crate::core::r#ref::{id_ref, name_ref, Ref};
use crate::core::roles::Role;
use crate::core::sentence::{Arg, IfArm, IntoTarget, Outcome};
use crate::core::types::{
    aspect_entry, aspect_set, relation_key, release_name_entry, AspectEntry, AttrValue, Attrs,
    Ending, Entity, Relation, State,
};
use crate::error::{ChorusError, Result};

mod chain;
mod mint;

use self::chain::{apply_fork, apply_run};
use self::mint::{apply_replace, apply_spawn};
use super::record::{checked_step, ActionRecord};
use super::validate::resolve_ref;

/// Role args → concrete id refs captured
/// from the draft bindings (§9.2: the log never contains sugar)
fn concretize(args: &[Arg], ctx: &ApplyCtx) -> Vec<Arg> {
    let mut out: Vec<Arg> = Vec::new();
    for a in args {
        if let Arg::Role { role } = a {
            for id in ctx.bindings.get(role).map(|v| v.as_slice()).unwrap_or(&[]) {
                out.push(Arg::Ref { r#ref: id_ref(id.clone()) });
            }
        } else {
            out.push(a.clone());
        }
    }
    out
}

/// The ref args of an outcome, in order — the `(ref, ref, type[, value])`
/// arities of §4.4 walk this.
fn ref_args(args: &[Arg]) -> Vec<&Ref> {
    args.iter()
        .filter_map(|a| match a {
            Arg::Ref { r#ref } => Some(r#ref),
            _ => None,
        })
        .collect()
}

/// The first `INTO` target arg, if any.
fn into_arg(args: &[Arg]) -> Option<&IntoTarget> {
    args.iter().find_map(|a| match a {
        Arg::Into { r#ref } => Some(r#ref),
        _ => None,
    })
}

/// The bound id ref for a role target, or the target itself when unbound.
fn id_or_self(e: Option<&Entity>, fallback: IntoTarget) -> IntoTarget {
    match e {
        Some(e) => IntoTarget::Ref(id_ref(e.id.clone())),
        None => fallback,
    }
}

/// The outcome-application context. `id_gen` is the injected id closure;
/// `roll` is `None` until the list's ROLL applies (reset per clause of a
/// multi-clause step).
pub(super) struct ApplyCtx<'a> {
    pub(super) state: &'a mut State,
    pub(super) dict: &'a Dictionary,
    pub(super) content: &'a CompiledContent,
    pub(super) bindings: RoleBindings,
    pub(super) now: i64, // absolute minutes at application time
    pub(super) record_id: i64,
    pub(super) id_gen: &'a mut dyn FnMut() -> String,
    pub(super) extra_advance_min: i64,
    /// §4.4: the per-clause dice binding — the last ROLL of THIS clause's
    /// outcome list (last-wins, rebinds on a second ROLL). Lives on the apply
    /// context, never on State: a REFUSE discards the draft along with it, so
    /// a refused step's dice never moved the real counter. Reset per clause of
    /// a multi-clause step; a fresh event context starts unbound. Function
    /// expansion is compile-time, so expanded bodies read the caller's list
    /// binding naturally.
    pub(super) roll: Option<i64>,
    /// §6.7: the completion-time chain channel — the parent window's
    /// identity plus the sink for the follow-up BEGINs. Some inside
    /// fire_window's outcome loop and inside event bodies (`make_event_ctx`'s
    /// `chained` face, the §6.6 event-chain channel); None at every other
    /// application site (the fold, the Phase-3 table trial — the chain ops
    /// never apply there: the trial skips them as completion-owned, and the
    /// log never carries one for the fold to replay).
    pub(super) chain: Option<ChainCtx>,
}

/// The §6.7 chain channel (see [`ApplyCtx::chain`]): the parent window's
/// identity the chain arms need (its end clock is the follow-ups' begin, its
/// authored refs become the follow-up clauses), the one-chain-slot detector,
/// and the sink the synthesized follow-up BEGINs ride to fire_window.
///
/// §6.7 extends the channel for completion triggers: `parent_key` excludes
/// the parent's own window from the follow-up busy check, `deferring` parks
/// recipe-body chain ops (a completion fires `on complete` triggers AFTER
/// the recipe's own outcomes and BEFORE any chain op), and `deferred` holds
/// the parked ops in list order.
pub(super) struct ChainCtx {
    pub(super) parent_key: String,
    pub(super) parent_end: i64,
    pub(super) parent_refs: IndexMap<Role, Vec<Ref>>,
    /// the next follow-up BEGIN's record id — the COMPLETE row's id + 1
    /// (a FORK's branches consume one each, in list order)
    pub(super) next_record_id: i64,
    /// the chain slot is earned by a PIN, not by a reach (§6.7): a second
    /// PINNING chain op in one completion refuses the parent window (a
    /// station holds one window) — silently skipped on the `event` channel
    /// (§6.6, see [`ChainCtx::event`]); a whiffed list spends nothing
    pub(super) run_taken: bool,
    pub(super) extra_records: Vec<ActionRecord>,
    /// §6.7: recipe-body RUN ops park here (in outcome order) while the
    /// completion's `on complete` triggers fire, then replay
    pub(super) deferring: bool,
    pub(super) deferred: Vec<Outcome>,
    /// §6.6: the event-channel flag — slot/expire trigger bodies arm the
    /// channel through `make_event_ctx` (no parent window, empty bindings);
    /// there a chain-op refusal is a SILENT SKIP (an event has no step to
    /// refuse, §6.6): the second pinning chain op ends the chain where it
    /// stands and an inline-gate veto skips its branch. The completion
    /// channel (`fire_window`) keeps the window refusals.
    pub(super) event: bool,
}

/// What one outcome application produced.
pub(super) struct ApplyResult {
    pub(super) concrete: Vec<Outcome>, // what the log stores: ids minted, sugar resolved, deltas computed (§9.2, §4.4)
    pub(super) refused: bool,
}

/// The REFUSE marker outcome — the one shape the fold treats as a no-op
/// replay (§6.5); window refusals carry the deciding message in its string.
pub(super) fn refuse_marker(msg: String) -> Outcome {
    Outcome::concrete("REFUSE", vec![Arg::Str { v: msg }])
}

/// Apply an outcome list under the completion's window-refuse discipline
/// (§6.7): a REFUSE ends the list with the partial concrete kept; an
/// application ERROR is not a gate — it refuses the WINDOW with a REFUSE
/// marker carrying the message. Returns true when the list refused.
pub(super) fn apply_outcomes_with_refuse<'a>(
    outcomes: impl IntoIterator<Item = &'a Outcome>,
    ctx: &mut ApplyCtx<'_>,
    concrete: &mut Vec<Outcome>,
) -> bool {
    for o in outcomes {
        match apply_outcome(o, ctx) {
            Ok(res) => {
                concrete.extend(res.concrete);
                if res.refused {
                    return true;
                }
            }
            Err(err) => {
                concrete.clear();
                concrete.push(refuse_marker(err.msg));
                return true;
            }
        }
    }
    false
}

/// §4.4 {roll} interpolation. A NARRATE string's
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
    Outcome::concrete(o.op.clone(), vec![Arg::Str { v: s.replace("{roll}", &roll.to_string()) }])
}

/// The op dispatch (20 ops — the §4.4 vocabulary). The match arms follow
/// the authored op list in order; the bigger cases are pulled into helper
/// fns, one arm each — the SPAWN/REPLACE mint rules in `mint`, the §6.7
/// RUN chain in `chain`, the rest here.
pub(super) fn apply_outcome(o: &Outcome, ctx: &mut ApplyCtx) -> Result<ApplyResult> {
    let args = &o.args;
    match o.op.as_str() {
        "IF" => apply_if(o, ctx),
        "RUN" => apply_run(o, ctx),
        "FORK" => apply_fork(o, ctx),
        "SPAWN" => apply_spawn(args, ctx),
        "DESTROY" => {
            let concrete_args = concretize(args, ctx);
            for id in expand(&concrete_args, ctx) {
                // stamp + release on the CURRENT entity (§7's soft destroy:
                // entities are never removed, so the id always resolves)
                let name = match ctx.state.entities.get(&id) {
                    Some(e) => e.name.clone(),
                    None => continue, // unreachable under §7's soft destroy
                };
                if let Some(e) = ctx.state.entities.get_mut(&id) {
                    e.dead_action = Some(ctx.record_id);
                    // names are unique among the LIVING (§5.1): the name
                    // frees only when the LAST pool copy dies (§4.2)
                }
                release_name_entry(ctx.state, &name, &id);
            }
            Ok(ApplyResult {
                concrete: vec![Outcome::concrete(o.op.clone(), concrete_args)],
                refused: false,
            })
        }
        "REPLACE" => apply_replace(args, ctx),
        "SET" => apply_set(args, ctx),
        "MOVE" => {
            let Some(into_ref) = into_arg(args) else {
                return Err(ChorusError::new("MOVE needs INTO"));
            };
            let into_concrete: Arg = match into_ref {
                IntoTarget::Role { role } => {
                    let bound = ctx
                        .bindings
                        .get(role)
                        .and_then(|v| v.first())
                        .and_then(|id| ctx.state.entities.get(id));
                    Arg::Into { r#ref: id_or_self(bound, IntoTarget::Role { role: *role }) }
                }
                into_ref @ IntoTarget::Ref(..) => Arg::Into { r#ref: into_ref.clone() },
            };
            let dest: Option<String> = match &into_concrete {
                Arg::Into { r#ref: IntoTarget::Ref(r#ref) } => {
                    resolve_ref(ctx.state, r#ref)?.map(|e| e.id.clone())
                }
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
                    e.attrs.insert("container".to_string(), AttrValue::Str(dest.clone()));
                }
            }
            let mut concrete_args = refs;
            concrete_args.push(into_concrete);
            Ok(ApplyResult {
                concrete: vec![Outcome::concrete(o.op.clone(), concrete_args)],
                refused: false,
            })
        }
        "REVEAL" => {
            let concrete_args = concretize(args, ctx);
            for id in expand(&concrete_args, ctx) {
                if let Some(e) = ctx.state.entities.get_mut(&id) {
                    e.attrs.insert("state.hidden".to_string(), AttrValue::Bool(false));
                }
            }
            Ok(ApplyResult {
                concrete: vec![Outcome::concrete(o.op.clone(), concrete_args)],
                refused: false,
            })
        }
        "ADVANCE" => {
            let Some(Arg::Dur { v }) = args.first() else {
                return Err(ChorusError::new("ADVANCE needs a duration"));
            };
            ctx.extra_advance_min = checked_step(ctx.extra_advance_min, *v, "ADVANCE");
            Ok(ApplyResult { concrete: vec![o.clone()], refused: false })
        }
        "NARRATE" => Ok(ApplyResult { concrete: vec![rewrite_roll_prose(o, ctx)], refused: false }),
        "PROGRESS" => {
            // §6.7: a read-bar tick — journal-only, no state effect, no time effect
            let ok = matches!(args.first(), Some(Arg::Dur { .. }))
                && matches!(args.get(1), Some(Arg::Str { .. }));
            if !ok {
                return Err(ChorusError::new("PROGRESS needs (duration, string)"));
            }
            Ok(ApplyResult { concrete: vec![o.clone()], refused: false })
        }
        "RELATE" => {
            let refs: Vec<&Ref> = ref_args(args);
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
            Ok(ApplyResult { concrete: vec![o.clone()], refused: false })
        }
        "UNRELATE" => {
            // §7: stamp, don't erase — undo re-folds
            let refs: Vec<&Ref> = ref_args(args);
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
            Ok(ApplyResult { concrete: vec![o.clone()], refused: false })
        }
        "ENDING" => {
            // §4.4: the FIRST ENDING to apply wins;
            // later ones are logged no-ops. Title/flavour resolve here from
            // content — the one code path both play and fold share.
            let id = match args.first() {
                Some(Arg::Ref { r#ref: Ref::Name { name, .. } }) => Some(name.clone()),
                _ => None,
            };
            let Some(id) = id else {
                return Err(ChorusError::new("ENDING needs an id: ENDING ENDING_ID"));
            };
            let Some(ending) = ctx.content.endings.get(&id) else {
                return Err(ChorusError::new(format!("ENDING '{id}' is not in the endings table")));
            };
            if ctx.state.ended.is_none() {
                ctx.state.ended = Some(Ending {
                    id,
                    title: ending.title.clone(),
                    flavour: ending.flavour.clone(),
                });
            }
            Ok(ApplyResult { concrete: vec![o.clone()], refused: false })
        }
        "REFUSE" => Ok(ApplyResult { concrete: vec![o.clone()], refused: true }),
        "ROLL" => apply_roll(args, ctx),
        "ROLLED" => {
            // §4.4, fold side: set the counter,
            // nothing else (the total is journal data; replay never re-derives
            // a die, law 3). Play never applies this op: authored ROLLED is a
            // load error, so the only path here is `apply_record` over a logged
            // row.
            let Some(Arg::Int { v: ctr_after }) = args.get(1) else {
                return Err(ChorusError::new("ROLLED needs (total, ctrAfter)"));
            };
            ctx.state.rng = *ctr_after;
            Ok(ApplyResult { concrete: vec![o.clone()], refused: false })
        }
        "DRAW" => apply_draw(args, ctx),
        "DRAWN" => {
            // §4.4, fold side: advance the cursor,
            // ASSERTING the recorded name is exactly what the compiled
            // permutation serves at that cursor (play and fold share
            // `draw_from`, so only content drift can disagree). Play never
            // applies this op: authored DRAWN is a load error, so the only
            // path here is `apply_record` over a logged row.
            let (deck_name, recorded) = match (args.first(), args.get(1)) {
                (
                    Some(Arg::Ref { r#ref: Ref::Name { name: d, .. } }),
                    Some(Arg::Ref { r#ref: Ref::Name { name: n, .. } }),
                ) => (d.clone(), n.clone()),
                _ => return Err(ChorusError::new("DRAWN needs (deck, name)")),
            };
            let Some(deck) = ctx.content.decks.get(&deck_name) else {
                return Err(ChorusError::new(format!(
                    "DRAWN: deck '{deck_name}' is not in the compiled decks — content drift between play and fold (§4.4)"
                )));
            };
            let cursor = ctx.state.decks.get(&deck.id).copied().unwrap_or(0);
            // the §4.2 unique deck filter is part of the shared draw rule —
            // the fold re-derives it from its own manifested set (rebuilt
            // through the mint path), so the landed cursor is identical
            let (expected, next) =
                draw_from(ctx.content, deck, cursor, &ctx.state.manifested_uniques)?; // throws loud on a drifted-away exhaustion rule
            if expected.name != recorded {
                return Err(ChorusError::new(format!(
                    "DRAWN: deck '{}' draw #{cursor} recorded '{recorded}' but the compiled deck serves '{}' — content drift between play and fold (§4.4)",
                    deck.id, expected.name
                )));
            }
            ctx.state.decks.insert(deck.id.clone(), next);
            Ok(ApplyResult { concrete: vec![o.clone()], refused: false })
        }
        _ => Err(ChorusError::new(format!("unknown outcome op '{}'", o.op))),
    }
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
        Arg::Ref { r#ref: Ref::Name { name, .. } } => Some(name.clone()),
        _ => None,
    };
    let Some(raw_key) = raw_key else {
        return Err(ChorusError::new("SET key must be a name or string"));
    };
    // an `aspect:` prefix names the aspects map (§4.2); every other key is an
    // attr (§4.4)
    let aspect_path = raw_key.strip_prefix("aspect:");
    let key = aspect_path.map_or(raw_key.clone(), |p| p.to_string());
    // the concrete key keeps its authored form: a quoted key must stay quoted
    // or the record stops round-tripping (§9.2)
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
                // the attr write + the SET-expires materialization (§4.2,
                // §4.2) — the write closure shares the SPAWN modifier's
                // helper (§6.6): the same key test, the same one duration
                // parser, the same checked arithmetic
                materialize_expires(&mut $e.attrs, &key, value, $ctx.now, "SET expires")?;
            }
        }};
    }
    if let Arg::Delta { v: delta } = val_arg {
        // signed value (§4.4): add to the entry's current number (missing = 0,
        // negative results allowed) and log the CONCRETE ABSOLUTE result — the
        // fold replays literals and never redoes arithmetic
        let targets = expand(std::slice::from_ref(ref_arg), ctx);
        let mut concrete: Vec<Outcome> = Vec::with_capacity(targets.len());
        for id in &targets {
            let cur: Option<i64> = {
                let Some(e) = ctx.state.entities.get(id) else {
                    continue; // unreachable under §7's soft destroy
                };
                if let Some(path) = aspect_path {
                    // raw entry, not `aspect_get`: a sub-map must REFUSE the
                    // delta, not count as missing and be clobbered (§4.4
                    // non-numeric rule)
                    match aspect_entry(e, path) {
                        Some(AspectEntry::Num(n)) => Some(n),
                        Some(AspectEntry::Sub(_)) => {
                            return Err(ChorusError::new(format!(
                                "SET: cannot add to non-numeric '{raw_key}'"
                            )));
                        }
                        None => None,
                    }
                } else {
                    match e.attrs.get(&key) {
                        Some(AttrValue::Num(n)) => Some(*n),
                        Some(_) => {
                            return Err(ChorusError::new(format!(
                                "SET: cannot add to non-numeric '{raw_key}'"
                            )));
                        }
                        None => None,
                    }
                }
            };
            // saturating add at the i64 bounds — the engine's defined
            // overflow direction
            let next = cur.unwrap_or(0).saturating_add(*delta);
            if let Some(e) = ctx.state.entities.get_mut(id) {
                write!(ctx, e, &AttrValue::Num(next));
            }
            // one concrete SET per card, so replay needs no step context (§9.2)
            let ref_concrete: Arg = if targets.len() == 1 && !matches!(ref_arg, Arg::Role { .. }) {
                ref_arg.clone()
            } else {
                Arg::Ref { r#ref: id_ref(id.clone()) }
            };
            concrete.push(Outcome::concrete(
                "SET".to_string(),
                vec![ref_concrete, key_concrete.clone(), Arg::Int { v: next }],
            ));
        }
        return Ok(ApplyResult { concrete, refused: false });
    }
    let value: AttrValue = match val_arg {
        Arg::Int { v } => AttrValue::Num(*v),
        Arg::Str { v } => AttrValue::Str(v.clone()),
        Arg::Bool { v } => AttrValue::Bool(*v),
        Arg::Dur { v } => AttrValue::Num(*v),
        _ => {
            return Err(ChorusError::new(
                "SET value must be a string, number, boolean or duration",
            ));
        }
    };
    let targets = expand(std::slice::from_ref(ref_arg), ctx);
    for id in &targets {
        if let Some(e) = ctx.state.entities.get_mut(id) {
            write!(ctx, e, &value);
        }
    }
    // §9.2: the log never contains sugar — a ROLE arg must not reach the
    // record (the fold rebuilds whole-record bindings, so over a multi-clause
    // step it would attribute the role to the wrong clause). One concrete SET
    // per card, exactly like the delta path above; name and kind refs replay
    // fine and stay as written.
    if matches!(ref_arg, Arg::Role { .. }) {
        let concrete = targets
            .iter()
            .map(|id| {
                Outcome::concrete(
                    "SET".to_string(),
                    vec![
                        Arg::Ref { r#ref: id_ref(id.clone()) },
                        key_concrete.clone(),
                        val_arg.clone(),
                    ],
                )
            })
            .collect();
        return Ok(ApplyResult { concrete, refused: false });
    }
    Ok(ApplyResult {
        concrete: vec![Outcome::concrete(
            "SET".to_string(),
            vec![ref_arg.clone(), key_concrete, val_arg.clone()],
        )],
        refused: false,
    })
}

/// ROLL (§4.4), the engine's ONLY
/// randomness. The args are §6.3 value expressions evaluated against this
/// clause's bindings and the DRAFT (strictly: a path over an unbound role
/// rejects the step, like an IF condition). A roll of m dice consumes exactly
/// m counter values c .. c+m-1; the concrete row carries the total AND the
/// counter after, so the fold lands the counter without deriving anything
/// (law 3). count = 0 is legal (total = bonus, counter unmoved); sides < 2 or
/// count < 0 reject the step (§9.2 — nothing logged, clock unmoved).
fn apply_roll(args: &[Arg], ctx: &mut ApplyCtx) -> Result<ApplyResult> {
    let (count_v, sides_v, bonus_v) = match (args.first(), args.get(1), args.get(2)) {
        (Some(Arg::Vexpr { v: c }), Some(Arg::Vexpr { v: s }), b) => {
            let bonus = match b {
                None => None,
                Some(Arg::Vexpr { v }) => Some(v),
                Some(_) => {
                    return Err(ChorusError::new(
                        "internal: ROLL arguments must be value expressions",
                    ));
                }
            };
            (c, s, bonus)
        }
        _ => return Err(ChorusError::new("internal: ROLL arguments must be value expressions")),
    };
    let (count, sides, bonus) = {
        let pctx = PredCtx { b: &ctx.bindings, state: ctx.state, strict: true, roll: ctx.roll };
        let count = eval_expr(count_v, &pctx)?;
        let sides = eval_expr(sides_v, &pctx)?;
        let bonus = match bonus_v {
            Some(v) => eval_expr(v, &pctx)?,
            None => Some(0),
        };
        (count, sides, bonus)
    };
    // one strict discipline for all three operands (§6.3's rule, applied to
    // dice): an operand that cannot resolve — a missing aspect/attr over its
    // role — rejects the step, never a silent 0-dice roll. (`Number.isInteger`
    // is vacuous over i64 — the reachable checks are the undefined and range
    // ones; the message texts are frozen either way.)
    let Some(count) = count else {
        return Err(ChorusError::new("ROLL: dice count must be an integer >= 0, got nothing"));
    };
    if count < 0 {
        return Err(ChorusError::new(format!(
            "ROLL: dice count must be an integer >= 0, got {count}"
        )));
    }
    let Some(sides) = sides else {
        return Err(ChorusError::new("ROLL: sides must be an integer >= 2, got nothing"));
    };
    if sides < 2 {
        return Err(ChorusError::new(format!("ROLL: sides must be an integer >= 2, got {sides}")));
    }
    let Some(bonus) = bonus else {
        return Err(ChorusError::new("ROLL: bonus must be an integer, got nothing"));
    };
    let c = ctx.state.rng;
    let mut total = bonus;
    for i in 0..count {
        // saturating add at the i64 bounds — the engine's defined
        // overflow direction
        total = total.saturating_add(roll_die(ctx.content.seed, c + i, sides));
    }
    // the per-clause binding: `roll` and {roll} read this. The counters were
    // consumed ON THE DRAFT — the fold lands the same spot via the ROLLED
    // row (law 3).
    ctx.roll = Some(total);
    let ctr_after = checked_step(c, count, "rng counter");
    ctx.state.rng = ctr_after;
    Ok(ApplyResult {
        concrete: vec![Outcome::concrete(
            "ROLLED".to_string(),
            vec![Arg::Int { v: total }, Arg::Int { v: ctr_after }],
        )],
        refused: false,
    })
}

/// The DRAW case (§4.4): draw the deck's
/// next card. The deck table lives on compiled content (the permutation is
/// pure in seed+id, law 5); the cursor lives on State and advances ON THE
/// DRAFT here — a later failure or REFUSE discards it with the whole draft
/// (law 4). Concretizes to TWO rows: the engine-written `DRAWN(<deck>,
/// <name>)` (the fold's cursor row, carrying the SPEC entry name so its
/// assertion reads the permutation) followed by the standard concrete SPAWN —
/// which owns every mint rule (prototype aspects/derive, §4.2 stackability,
/// materialization pairs), so DRAW adds none of its own. Authored DRAW never
/// reaches the log, like ROLL and IF.
fn apply_draw(args: &[Arg], ctx: &mut ApplyCtx) -> Result<ApplyResult> {
    let deck_id = match args.first() {
        Some(Arg::Ref { r#ref: Ref::Name { name, .. } }) => name.clone(),
        _ => return Err(ChorusError::new("DRAW needs a deck id: DRAW <DECK> [INTO target]")),
    };
    let Some(deck) = ctx.content.decks.get(&deck_id) else {
        return Err(ChorusError::new(format!("DRAW: unknown deck '{deck_id}'")));
    };
    let into = into_arg(args);
    // §4.4: DRAW's INTO must RESOLVE — a role over empty event bindings or
    // an unresolvable ref would otherwise materialize the card nowhere,
    // silently. (SPAWN's own INTO stays lenient; a trigger that wants a DRAWN
    // card lands it with an explicit INTO — event bindings are empty, so the
    // no-INTO default below reads nothing in events either.)
    let into_concrete: Option<Arg> = match into {
        Some(into_ref) => {
            let dest: Option<String> = match into_ref {
                IntoTarget::Role { role } => {
                    ctx.bindings.get(role).and_then(|v| v.first()).cloned()
                }
                IntoTarget::Ref(r#ref) => resolve_ref(ctx.state, r#ref)?.map(|e| e.id.clone()),
            };
            let Some(dest) = dest else {
                return Err(ChorusError::new(format!(
                    "DRAW: INTO target of '{deck_id}' cannot resolve"
                )));
            };
            Some(Arg::Into { r#ref: IntoTarget::Ref(id_ref(dest)) })
        }
        None => None,
    };
    // serve + advance the cursor; the §4.2 unique deck filter skips
    // manifested-unique positions inside the same call (the returned cursor
    // is the drawn rule's own landing), and `draw_from` throws on a truly
    // exhausted deck with neither `reset` nor `defaultcard` — §9.2 (nothing
    // logged, cursor unmoved: the mutation below happens on the draft only
    // after it returns)
    let cursor = ctx.state.decks.get(&deck_id).copied().unwrap_or(0);
    let (drawn, next) = draw_from(ctx.content, deck, cursor, &ctx.state.manifested_uniques)?;
    ctx.state.decks.insert(deck_id.clone(), next);
    let mut spawn_args: Vec<Arg> =
        vec![Arg::Ref { r#ref: name_ref(drawn.name.clone(), Some(drawn.kind.clone())) }];
    if let Some(ic) = into_concrete {
        spawn_args.push(ic);
    }
    spawn_args.push(Arg::AsId { id: (ctx.id_gen)() });
    let spawn = Outcome::concrete("SPAWN".to_string(), spawn_args);
    let spawn_concrete = apply_outcome(&spawn, ctx)?.concrete;
    let mut concrete = vec![Outcome::concrete(
        "DRAWN".to_string(),
        vec![
            Arg::Ref { r#ref: name_ref(deck_id, None) },
            Arg::Ref { r#ref: name_ref(drawn.name, None) },
        ],
    )];
    concrete.extend(spawn_concrete);
    Ok(ApplyResult { concrete, refused: false })
}

/// §6.8 IF / ELSE IF / ELSE in an outcome
/// list. Arms evaluate in order against the DRAFT (this list's earlier
/// outcomes — SET, SPAWN, DESTROY — are visible to later IFs, unlike recipe
/// selection which reads the input state); the first true condition wins;
/// ELSE is the default; no true arm and no ELSE applies nothing, without
/// error. Concretization follows the SET-delta precedent (§4.4): the log
/// records ONLY the taken branch's outcomes, flattened in position — the
/// record never contains an IF op, and the untaken branches are authored
/// discipline the log cannot recover. Conditions evaluate STRICTLY (§9.2):
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
            let pctx = PredCtx { b: &ctx.bindings, state: ctx.state, strict: true, roll: ctx.roll };
            eval_pred(&arm.when, &pctx)?
        };
        if chosen {
            return apply_all(&arm.then, ctx);
        }
    }
    match o.fallback.as_deref() {
        Some(fallback) if !fallback.is_empty() => apply_all(fallback, ctx),
        _ => Ok(ApplyResult { concrete: Vec::new(), refused: false }),
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
            return Ok(ApplyResult { concrete, refused: true });
        }
    }
    Ok(ApplyResult { concrete, refused: false })
}

/// `expires` materialization (§4.2): numbers are
/// already minutes — the lexer pre-converts bare duration literals (§5.1)
/// before SET sees them; quoted strings go through the one duration parser,
/// so every §5.1 form (`2h`, `90m`, `1d`) materializes
/// `expiresAt`. The ONE write both SET (§4.2) and the SPAWN
/// `expires <duration>` mint modifier (§4.4, §6.6) share: the authored
/// `expires` attr keeps its form (minutes for bare, the string for quoted,
/// §9.2) and `expiresAt` lands as `now + minutes` through the checked
/// stepper — `now` being the application clock (the step clock at the
/// table, the window's START clock inside a completion, §6.7). SET calls
/// this with its own `key` (the expires materialization is keyed on the
/// key being `expires`); the SPAWN modifier calls it with `expires`.
fn materialize_expires(
    attrs: &mut Attrs,
    key: &str,
    value: &AttrValue,
    now: i64,
    what: &str,
) -> Result<()> {
    attrs.insert(key.to_string(), value.clone());
    if key == "expires" {
        let mins = to_minutes(value)?;
        attrs.insert("expiresAt".to_string(), AttrValue::Num(checked_step(now, mins, what)));
    }
    Ok(())
}

/// The duration-to-minutes read every `expires` write shares: numbers are
/// already minutes — the lexer pre-converts bare duration literals (§5.1)
/// before SET sees them; quoted strings go through the one duration parser,
/// so every §5.1 form (`2h`, `90m`, `1d`) materializes
/// `expiresAt` (§4.2, §5.1).
fn to_minutes(v: &AttrValue) -> Result<i64> {
    match v {
        AttrValue::Num(n) => Ok(*n),
        AttrValue::Str(s) => parse_duration(s),
        AttrValue::Bool(b) => Err(ChorusError::new(format!("bad duration '{b}'"))),
    }
}

/// Resolve ref/role args to entity IDS
/// (role refs expand across the binding). §6.8: the ids come back (not
/// live entity handles) and each caller re-gets the entity,
/// so a mid-list DESTROY is seen by every later read (dead-stamp rule).
/// Role bindings push ALL their ids (dead ones included); name/id refs
/// resolve alive-only.
fn expand(args: &[Arg], ctx: &ApplyCtx) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for a in args {
        match a {
            Arg::Role { role } => out.extend(
                ctx.bindings.get(role).map(|v| v.as_slice()).unwrap_or(&[]).iter().cloned(),
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
