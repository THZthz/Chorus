//! The parked interior (§6.3, v0.10.0): the `SPAWN X parked` mint modifier,
//! the third sum scope `total(parked, aspect:k)`, and the visibility law —
//! the world forms (`exists`, `count(<query>)`, `total(table, …)`) read
//! UNPARKED alive cards only, slots refuse parked cards, and name/#id refs
//! still reach them. No new op: park/unpark are `SET(…, state.parked, …)`
//! writes, and every face is a concrete-row replay — play == fold == undo
//! throughout.
//!
//! Conventions follow tests/engine/held.rs: a fresh dictionary per test,
//! script-text rigs, the whole log accumulated per turn and folded as one,
//! fold equality through the shared `snap`.

use crate::common::rig::tmp_log_path;
use crate::common::snap::snap;
use chorus::content::script::compile_script;
use chorus::content::CompiledContent;
use chorus::core::clock::{parse_clock, Clock};
use chorus::core::dict::{core_dictionary, Dictionary};
use chorus::core::parse::parse_sentence;
use chorus::core::predicate::{eval_pred, parse_predicate, PredCtx, RoleBindings};
use chorus::core::r#ref::{name_ref, Ref};
use chorus::core::sentence::Arg;
use chorus::core::types::{AttrValue, State};
use chorus::engine::fold::{fold, fold_up_to};
use chorus::engine::store::{load_log, save_log};
use chorus::engine::{execute_hold, execute_step, ActionRecord, ExecOptions, ExecResult, Step};

// ---------------------------------------------------------------- rigs ------

/// The parked-crisis rig: the ambient DESPAIR station mints a DREAD parked
/// every completion (the pump is the when-less default — parked dread is
/// invisible to every world form, so nothing stops it), and the death
/// recipe's `total(parked, aspect:dread) >= 3` gate (first-match, ahead of
/// the pump) fires ENDING NO_MORE — the measured probe's exact shape. The
/// other verbs drive the individual faces: MINT mints parked, HEX parks a
/// bound item, RECLAIM unparks BY NAME (binding no slot — refs reach the
/// interior), WISPVISIT walks the pool-ladder faces, SPAWNPAIR mints an
/// unparked pair, LADDER carries the three-scope gate, BURN parks a bound
/// idea mid-window (the SPRITE completion-trigger composition). The DREAD
/// prototype also carries `miseria` — an aspect NO other card bears — so the
/// pure-visibility probes can read it alone.
const SCRIPT: &str = "\
script: Parked Interior Probe
player: ANA
start: 0:0
seed: 0

entities:
  ANA is character
  BOOK is text
  LAMP is item with dread:1
  SPRITE is idea with dread:1
  on complete -> (IF total(parked, aspect:dread) >= 2 -> (SPAWN MARK))

prototypes:
  DREAD is idea stackable with dread:1, miseria:1
  WISP is idea stackable with dread:1
  MARK is idea
  ASH is resource

verbs:
  MINT:
    actors: 1 kind:character
    duration: 0m
    recipes:
      default -> SPAWN DREAD parked, SPAWN DREAD parked, SPAWN DREAD parked

  HEX:
    actors: 1 kind:character
    object: 1 kind:item
    duration: 0m
    recipes:
      default -> SET(object, state.parked, true), NARRATE \"hexed.\"

  RECLAIM:
    actors: 1 kind:character
    duration: 0m
    recipes:
      default -> SET(LAMP, state.parked, false)

  SPAWNPAIR:
    actors: 1 kind:character
    duration: 0m
    recipes:
      default -> SPAWN WISP, SPAWN WISP

  WISPVISIT:
    actors: 1 kind:character
    duration: 0m
    recipes:
      wisit.spawn when NOT exists(name:WISP) -> SPAWN WISP, SPAWN WISP, SET(WISP, state.parked, true)
      wisit.kill when total(parked, aspect:dread) >= 1 -> DESTROY WISP
      wisit.idle -> NARRATE \"idle.\"

  LADDER:
    actors: 1 kind:character
    object: 0..1 kind:idea
    with: 0..1 kind:idea
    duration: 0m
    recipes:
      ladder.gate when total(bound, aspect:dread) >= 2 OR total(table, aspect:dread) >= 2 OR \
total(parked, aspect:dread) >= 2 -> SPAWN MARK
      ladder.default -> NARRATE \"no gate.\"

  BURN:
    actors: 1 kind:character
    object: 0..1 kind:idea
    duration: 10m
    recipes:
      burn.park -> SET(object, state.parked, true)

  PARKHELD:
    actors: 1 kind:character
    object: 1 kind:text
    held: 0..1 aspect:dread
    duration: 10m
    recipes:
      default -> NARRATE \"studied.\"

endings:
  NO_MORE: \"No More\"
";

/// A fresh fixture: dictionary, content, and the pristine pre-play state
/// (compile_script is pure, so a fresh compile IS the base state).
fn rig() -> (Dictionary, CompiledContent, State) {
    let dict = core_dictionary();
    let compiled = compile_script(SCRIPT, &dict).unwrap();
    (compiled.dict, compiled.content, compiled.state)
}

/// commit one step WITHOUT settling — the window (if any) stays open. The
/// result's records extend the caller's running log.
fn begin(
    content: &CompiledContent,
    dict: &Dictionary,
    state: &State,
    src: &str,
    first_id: i64,
) -> ExecResult {
    let s = parse_sentence(src, dict).unwrap();
    execute_step(
        content,
        dict,
        state.clone(),
        Step {
            start_min: None,
            clock: Some(s.clock.unwrap_or(Clock { day: 0, tick: 0 })),
            clauses: s.clauses,
            t_real: None,
        },
        ExecOptions { first_record_id: first_id, id_gen: None },
    )
    .unwrap()
}

/// advance to `minute` with no clauses — the pure pass that resolves every
/// window due by then (and runs the §6.9 sweep).
fn advance_to(
    content: &CompiledContent,
    dict: &Dictionary,
    state: &State,
    minute: &str,
    first_id: i64,
) -> ExecResult {
    execute_step(
        content,
        dict,
        state.clone(),
        Step {
            start_min: None,
            clock: Some(parse_clock(minute).unwrap()),
            clauses: vec![],
            t_real: None,
        },
        ExecOptions { first_record_id: first_id, id_gen: None },
    )
    .unwrap()
}

/// The one log the fold reads: every turn's records concatenated.
fn whole_log(turns: &[&ExecResult]) -> Vec<ActionRecord> {
    turns.iter().flat_map(|r| r.records.iter().cloned()).collect()
}

/// The living copies of a name.
fn living_named(state: &State, name: &str) -> usize {
    state.entities.values().filter(|e| e.dead_action.is_none() && e.name == name).count()
}

/// The parked living copies of a name (`state.parked` exact true — the
/// engine's own `is_parked` face).
fn parked_named(state: &State, name: &str) -> usize {
    state
        .entities
        .values()
        .filter(|e| {
            e.dead_action.is_none()
                && e.name == name
                && e.attrs.get("state.parked") == Some(&AttrValue::Bool(true))
        })
        .count()
}

/// Evaluate one `when:` over the state with empty bindings (a world-form
/// probe — every form below reads the world, no role).
fn world_probe(state: &State, src: &str) -> bool {
    let dict = core_dictionary();
    let p = parse_predicate(src, &dict).unwrap();
    let b = RoleBindings::new();
    eval_pred(&p, &PredCtx { b: &b, state, strict: false, roll: None }).unwrap()
}

// ------------------------------------------------------- the mint modifier ----

/// T-P1 + the row shape: `SPAWN DREAD parked` ×3 — the mints are born inside
/// the interior: exists/count/total(table) all read 0, total(parked) reads 3
/// (the probes read `miseria` — the aspect only the parked DREADs bear).
#[test]
fn parked_mints_are_invisible_to_the_world_forms_and_sum_in_the_parked_scope() {
    let (dict, c, base) = rig();
    let r1 = begin(&c, &dict, &base, "0:0 @ ANA MINT", 1);
    assert_eq!(r1.rejected, None);
    let r2 = advance_to(&c, &dict, &r1.state, "0:1", 2);
    assert_eq!(r2.rejected, None);
    let state = &r2.state;
    assert_eq!(living_named(state, "DREAD"), 3);
    assert_eq!(parked_named(state, "DREAD"), 3, "every mint born parked");
    // the visibility law, each form one probe
    assert!(!world_probe(state, "exists(aspect:miseria >= 1)"), "exists is parked-blind");
    assert!(!world_probe(state, "exists(name:DREAD)"), "the name existential is parked-blind");
    assert!(world_probe(state, "count(aspect:miseria >= 1) = 0"), "count is parked-blind");
    assert!(world_probe(state, "total(table, aspect:miseria) = 0"));
    assert!(world_probe(state, "total(parked, aspect:miseria) = 3"));
    // play == fold across the minting turn
    let records = whole_log(&[&r1, &r2]);
    let replayed = fold(&records, &base, &dict, &c).unwrap();
    assert_eq!(snap(&replayed), snap(state));
}

/// The concrete row shape: the COMPLETE row's SPAWN outcomes carry the
/// `attrs=` pair with `state.parked` true and NO `parked` arg — the modifier
/// is authored-sugar only (the expires discipline), so the logged bytes
/// re-parse under a grammar that has never heard of the modifier, and the
/// canonical spelling round-trips (`parked` prints bare).
#[test]
fn the_parked_modifier_materializes_into_the_attrs_pair_and_never_logs() {
    let (dict, c, base) = rig();
    let r1 = begin(&c, &dict, &base, "0:0 @ ANA MINT", 1);
    let r2 = advance_to(&c, &dict, &r1.state, "0:1", 2);
    let complete = r2.records.iter().find(|rec| rec.kind == "complete").unwrap();
    let mut spawn_rows = 0;
    for o in &complete.outcomes {
        if o.op != "SPAWN" {
            continue;
        }
        spawn_rows += 1;
        assert!(
            !o.args.iter().any(|a| matches!(a, Arg::Parked)),
            "the authored modifier never reaches the log"
        );
        let attrs = o.args.iter().find_map(|a| match a {
            Arg::Pair { k, v } if k == "attrs" => Some(v.clone()),
            _ => None,
        });
        let Some(AttrValue::Str(json)) = attrs else {
            panic!("every parked SPAWN carries the attrs= pair");
        };
        assert!(json.contains("state.parked"), "the pair carries the flag: {json}");
    }
    assert_eq!(spawn_rows, 3);
    let s = parse_sentence("0:0 @ ANA MINT -> SPAWN DREAD parked", &dict).unwrap();
    let text = chorus::core::format::format_sentence(&s, &dict).unwrap();
    assert_eq!(text, "0:0 @ ANA MINT -> SPAWN DREAD parked");
    let s2 = parse_sentence(&text, &dict).unwrap();
    assert_eq!(chorus::core::format::format_sentence(&s2, &dict).unwrap(), text);
}

// ------------------------------------------------------- the SET faces ----

/// T-P2: SET-park by role, unpark by NAME — both directions of the
/// visibility flip, the interior draining back to the world.
#[test]
fn set_park_and_unpark_flip_visibility_both_ways() {
    let (dict, c, base) = rig();
    // park LAMP by role
    let r1 = begin(&c, &dict, &base, "0:0 @ ANA HEX LAMP", 1);
    assert_eq!(r1.rejected, None);
    let r2 = advance_to(&c, &dict, &r1.state, "0:1", 2);
    assert!(!world_probe(&r2.state, "exists(name:LAMP)"), "parked LAMP left the world");
    assert!(world_probe(&r2.state, "total(parked, aspect:dread) = 1"));
    // unpark by NAME (the name ref reaches the interior; RECLAIM binds no
    // slot, so the binding law never fires)
    let r3 = begin(&c, &dict, &r2.state, "0:1 @ ANA RECLAIM", 3);
    assert_eq!(r3.rejected, None);
    let r4 = advance_to(&c, &dict, &r3.state, "0:2", 4);
    assert!(world_probe(&r4.state, "exists(name:LAMP)"), "unparked LAMP returned");
    assert!(world_probe(&r4.state, "total(parked, aspect:dread) = 0"));
    let records = whole_log(&[&r1, &r2, &r3, &r4]);
    let replayed = fold(&records, &base, &dict, &c).unwrap();
    assert_eq!(snap(&replayed), snap(&r4.state));
}

/// T-P4: the pool head parked by a name-SET, a name DESTROY still reaches
/// it (refs keep full reach; parking does not reorder `by_name`), and the
/// surviving copy is the UNPARKED second one.
#[test]
fn the_pool_head_park_survives_a_name_destroy() {
    let (dict, c, base) = rig();
    // wisit.spawn: two WISPs, then the name-SET parks the HEAD (the oldest)
    let r1 = begin(&c, &dict, &base, "0:0 @ ANA WISPVISIT", 1);
    assert_eq!(r1.rejected, None);
    let r2 = advance_to(&c, &dict, &r1.state, "0:1", 2);
    let state = &r2.state;
    assert_eq!(living_named(state, "WISP"), 2, "the pair minted");
    assert_eq!(parked_named(state, "WISP"), 1, "the SET parked the pool head");
    assert!(world_probe(state, "exists(name:WISP)"), "the unparked copy keeps the name true");
    assert!(world_probe(state, "total(parked, aspect:dread) = 1"));
    // wisit.kill: the gate holds (parked dread 1) — DESTROY WISP addresses
    // the HEAD, the parked copy; parking did not reorder the pool
    let r3 = begin(&c, &dict, state, "0:1 @ ANA WISPVISIT", 3);
    assert_eq!(r3.rejected, None);
    let r4 = advance_to(&c, &dict, &r3.state, "0:2", 4);
    let state = &r4.state;
    assert_eq!(living_named(state, "WISP"), 1, "the head died");
    assert_eq!(parked_named(state, "WISP"), 0, "the survivor is the unparked copy");
    assert!(world_probe(state, "exists(name:WISP)"));
    assert!(world_probe(state, "total(parked, aspect:dread) = 0"));
    let records = whole_log(&[&r1, &r2, &r3, &r4]);
    let replayed = fold(&records, &base, &dict, &c).unwrap();
    assert_eq!(snap(&replayed), snap(state));
}

// ---------------------------------------------------- the binding law ----

/// T-P3: a parked card refuses slot binds (the pinned message) and the hold
/// step; a name-SET unpark restores the bind.
#[test]
fn a_parked_card_refuses_slot_binds_until_unparked() {
    let (dict, c, base) = rig();
    let r1 = begin(&c, &dict, &base, "0:0 @ ANA HEX LAMP", 1);
    assert_eq!(r1.rejected, None);
    let r2 = advance_to(&c, &dict, &r1.state, "0:1", 2);
    // the slot bind refuses with the pinned message — nothing logs, state
    // rides back
    let r3 = begin(&c, &dict, &r2.state, "0:1 @ ANA HEX LAMP", 3);
    assert_eq!(
        r3.rejected.as_deref(),
        Some(
            "LAMP is parked inside a situation — it cannot bind into 'object' until unparked (§6.3)"
        )
    );
    assert!(r3.records.is_empty());
    // the held slot's face: the same law through the HOLD step — the
    // station opens first (BOOK commits), then the parked LAMP refuses
    let rs = begin(&c, &dict, &r2.state, "0:1 @ ANA PARKHELD BOOK", 3);
    assert_eq!(rs.rejected, None);
    let r4 =
        execute_hold(&c, &dict, rs.state.clone(), "PARKHELD", name_ref("LAMP", None), 4).unwrap();
    assert_eq!(
        r4.rejected.as_deref(),
        Some(
            "LAMP is parked inside a situation — it cannot bind into 'held' until unparked (§6.3)"
        )
    );
    assert!(r4.records.is_empty());
    // unpark by NAME (RECLAIM binds no slot), then the bind lands
    let r5 = begin(&c, &dict, &rs.state, "0:1 @ ANA RECLAIM", 5);
    assert_eq!(r5.rejected, None);
    let r6 = advance_to(&c, &dict, &r5.state, "0:2", 4);
    let r7 = begin(&c, &dict, &r6.state, "0:2 @ ANA HEX LAMP", 5);
    assert_eq!(r7.rejected, None, "unparked LAMP binds again");
    let records = whole_log(&[&r1, &r2, &rs, &r5, &r6, &r7]);
    let replayed = fold(&records, &base, &dict, &c).unwrap();
    assert_eq!(snap(&replayed), snap(&r7.state));
}

// ------------------------------------------------------ the measured probe ----

/// T-P5, the measured probe promoted: the ambient chain mints DREAD parked
/// every completion, the `total(parked, aspect:dread) >= 3` death gate (the
/// first-match recipe, ahead of the when-less pump) fires ENDING NO_MORE
/// once the interior holds three, and play == fold == undo across the whole
/// parked session INCLUDING the ending.
#[test]
fn the_parked_crisis_session_reaches_its_death_ending_and_snaps() {
    let script = "\
script: Parked Crisis Probe
player: ANA
start: 0:0
seed: 0

entities:
  ANA is character

prototypes:
  DREAD is idea stackable with dread:1

verbs:
  DESPAIR:
    ambient
    duration: 0m
    recipes:
      despair.death when total(parked, aspect:dread) >= 3 -> ENDING NO_MORE
      despair.pump -> SPAWN DREAD parked

endings:
  NO_MORE: \"No More\"
";
    let core = core_dictionary();
    let compiled = compile_script(script, &core).unwrap();
    let dict = compiled.dict.clone();
    let (c, base) = (compiled.content, compiled.state);
    // the crisis: pass after pass, the interior fills and the gate latches
    let mut turns: Vec<ExecResult> = Vec::new();
    let mut state = base.clone();
    let mut next_id = 1;
    for _ in 0..12 {
        let r = advance_to(&c, &dict, &state, "1:0", next_id);
        assert_eq!(r.rejected, None);
        next_id += r.records.len() as i64;
        let ended = r.state.ended.is_some();
        turns.push(r);
        state = turns.last().unwrap().state.clone();
        if ended {
            break;
        }
    }
    // the death: the gate fired and the table never saw a single dread
    let ended = state.ended.as_ref().expect("the death ENDING fired within 12 passes");
    assert_eq!(ended.id, "NO_MORE");
    assert!(parked_named(&state, "DREAD") >= 3, "the interior held the gate's three");
    assert!(world_probe(&state, "total(table, aspect:dread) = 0"), "the table never sees one");
    assert!(world_probe(&state, "total(parked, aspect:dread) >= 3"));
    // play == fold across the whole session, ending included
    let turns_ref: Vec<&ExecResult> = turns.iter().collect();
    let records = whole_log(&turns_ref);
    let replayed = fold(&records, &base, &dict, &c).unwrap();
    assert_eq!(snap(&replayed), snap(&state));
    // the log round-trips through the JSONL store and re-folds identically
    let path = tmp_log_path("parked-session");
    save_log(&path, &records).unwrap();
    let loaded = load_log(&path).unwrap();
    let replayed2 = fold(&loaded, &base, &dict, &c).unwrap();
    assert_eq!(snap(&replayed2), snap(&state));
    let _ = std::fs::remove_file(&path);
    // undo past the ending: the prefix fold (cut BEFORE the ending row)
    // reopens the world — the pre-ending prefix carries no ending
    let cut = records
        .iter()
        .find(|rec| rec.outcomes.iter().any(|o| o.op == "ENDING"))
        .map(|rec| rec.id)
        .unwrap_or(0);
    let prefix = fold_up_to(&records, &base, &dict, &c, cut).unwrap();
    assert!(prefix.ended.is_none(), "the prefix predates the ending");
}

// ------------------------------------------------------------- expiry ----

/// T-P6: a parked card keeps its authored expiry — `SPAWN WISP expires 30m
/// parked` dies on schedule and its authored `on expire` trigger fires
/// (the port's decay is authored-only; parking composes).
#[test]
fn an_authored_expiry_fires_on_a_parked_card_on_schedule() {
    let script = "\
script: Parked Expiry Probe
player: ANA
start: 0:0
seed: 0

entities:
  ANA is character

prototypes:
  WISP is idea stackable with dread:1
  ASH is resource stackable

triggers:
  t_wisp_gone on expire of WISP -> DESTROY WISP, SPAWN ASH

verbs:
  MINT:
    actors: 1 kind:character
    duration: 0m
    recipes:
      default -> SPAWN WISP expires 30m parked, SPAWN ASH
";
    let core = core_dictionary();
    let compiled = compile_script(script, &core).unwrap();
    let dict = compiled.dict.clone();
    let (c, base) = (compiled.content, compiled.state);
    let r1 = begin(&c, &dict, &base, "0:0 @ ANA MINT", 1);
    assert_eq!(r1.rejected, None);
    let r2 = advance_to(&c, &dict, &r1.state, "0:1", 2);
    assert_eq!(parked_named(&r2.state, "WISP"), 1, "born parked AND armed to expire");
    // before the expiry the card lives
    let r3 = advance_to(&c, &dict, &r2.state, "0:2", 3);
    assert_eq!(living_named(&r3.state, "WISP"), 1, "30m has not passed");
    // advance past the expiry: the parked card dies on schedule and the
    // authored trigger fires its body
    let r4 = advance_to(&c, &dict, &r3.state, "0:4", 4);
    assert_eq!(r4.rejected, None);
    assert_eq!(living_named(&r4.state, "WISP"), 0, "the parked card expired on schedule");
    assert_eq!(living_named(&r4.state, "ASH"), 2, "the mint's ASH + the on-expire ASH");
    let records = whole_log(&[&r1, &r2, &r3, &r4]);
    let replayed = fold(&records, &base, &dict, &c).unwrap();
    assert_eq!(snap(&replayed), snap(&r4.state));
}

/// The order-free mint modifiers (§4.4): `SPAWN WISP parked expires 30m`
/// and `SPAWN WISP expires 30m parked` both LOAD and materialize the
/// IDENTICAL row — the attrs= pair carries `state.parked`, `expires` and
/// `expiresAt` either way (the engine face of the parse-side red: the
/// parked-first spelling was a trailing-token load error before the
/// order-free collection).
#[test]
fn modifier_order_is_free_and_both_spellings_materialize_identically() {
    fn rig_with(recipe: &str) -> (Dictionary, CompiledContent, State) {
        let script = format!(
            "\
script: Modifier Order Probe
player: ANA
start: 0:0
seed: 0

entities:
  ANA is character

prototypes:
  WISP is idea stackable

verbs:
  MINT:
    actors: 1 kind:character
    duration: 0m
    recipes:
      default -> {recipe}
"
        );
        let dict = core_dictionary();
        let compiled = compile_script(&script, &dict).unwrap();
        (compiled.dict, compiled.content, compiled.state)
    }
    /// The `attrs=` pair of the completion's SPAWN row (the materialized
    /// mint — the only place the modifiers survive to the log).
    fn mint_attrs(r: &ExecResult) -> String {
        let complete = r.records.iter().find(|rec| rec.kind == "complete").unwrap();
        let spawn = complete.outcomes.iter().find(|o| o.op == "SPAWN").unwrap();
        let attrs = spawn.args.iter().find_map(|a| match a {
            Arg::Pair { k, v } if k == "attrs" => Some(v.clone()),
            _ => None,
        });
        let Some(AttrValue::Str(json)) = attrs else {
            panic!("the mint carries the attrs= pair");
        };
        json
    }
    let (dict_a, c_a, base_a) = rig_with("SPAWN WISP parked expires 30m");
    let (dict_b, c_b, base_b) = rig_with("SPAWN WISP expires 30m parked");
    let ra1 = begin(&c_a, &dict_a, &base_a, "0:0 @ ANA MINT", 1);
    let rb1 = begin(&c_b, &dict_b, &base_b, "0:0 @ ANA MINT", 1);
    assert_eq!(ra1.rejected, None);
    assert_eq!(rb1.rejected, None);
    let ra2 = advance_to(&c_a, &dict_a, &ra1.state, "0:1", 2);
    let rb2 = advance_to(&c_b, &dict_b, &rb1.state, "0:1", 2);
    let ja = mint_attrs(&ra2);
    let jb = mint_attrs(&rb2);
    assert_eq!(ja, jb, "both spellings materialize the identical row");
    assert!(ja.contains("state.parked"), "the flag rides the pair: {ja}");
    assert!(ja.contains("expiresAt"), "the computed expiry rides the pair: {ja}");
    assert!(ja.contains("\"expires\":"), "the authored form rides the pair: {ja}");
}

// -------------------------------------------- completion triggers on parked ----

/// T-P7: a parked-then-locked card still fires its own `on complete` — the
/// completion's outcome list parks the bound card mid-list (parked + locked
/// compose; the lock holds to window close, the flag outlives it) — and the
/// trigger's IF reads `total(parked, …)` and decides. The read is DECISIVE:
/// LAMP is parked first, so at SPRITE's completion the parked scope holds 2
/// (the IF's threshold) while the table scope holds 0 — a table-reading IF
/// would never fire.
#[test]
fn a_parked_locked_card_fires_its_on_complete_and_the_if_reads_the_parked_scope() {
    let (dict, c, base) = rig();
    // SPRITE carries `on complete -> (IF total(parked, aspect:dread) >= 2
    // -> (SPAWN MARK))` — raise the entity line's threshold via the rig
    // constant below; here LAMP parks first (a 0m window), then BURN binds
    // SPRITE for 10m and its body parks the binding at completion
    let r1 = begin(&c, &dict, &base, "0:0 @ ANA HEX LAMP", 1);
    assert_eq!(r1.rejected, None);
    let r2 = advance_to(&c, &dict, &r1.state, "0:1", 2);
    assert!(world_probe(&r2.state, "total(parked, aspect:dread) = 1"), "LAMP parked");
    let r3 = begin(&c, &dict, &r2.state, "0:1 @ ANA BURN SPRITE", 3);
    assert_eq!(r3.rejected, None);
    // the completion: the body parks SPRITE mid-list (the lock rides to
    // window close, the flag outlives it), the trigger phase sees SPRITE
    // alive (parked), fires the on complete, and the IF reads the parked
    // scope — 2 >= 2 — and takes the arm
    let r4 = advance_to(&c, &dict, &r3.state, "0:4", 4);
    assert_eq!(r4.rejected, None);
    assert_eq!(parked_named(&r4.state, "SPRITE"), 1, "the flag outlives the lock");
    assert_eq!(parked_named(&r4.state, "LAMP"), 1);
    assert!(world_probe(&r4.state, "total(table, aspect:dread) = 0"), "the table is empty");
    assert_eq!(living_named(&r4.state, "MARK"), 1, "the parked-scope IF spawned the marker");
    let records = whole_log(&[&r1, &r2, &r3, &r4]);
    let replayed = fold(&records, &base, &dict, &c).unwrap();
    assert_eq!(snap(&replayed), snap(&r4.state));
}

/// The T-P7 world with the completion trigger reading the WINDOW scope
/// instead: `total(bound, aspect:dread) >= 1`. The visibility law's
/// complement (§6.3): the role and window scopes are deliberately NOT
/// parked-blind — parking never unbinds, so the binding parked mid-list by
/// its own completion body still counts in the bound scope and the arm
/// fires on it (the table scope reads 0 in the same world).
#[test]
fn the_bound_scope_still_counts_a_binding_parked_mid_window() {
    let script = SCRIPT.replace(
        "on complete -> (IF total(parked, aspect:dread) >= 2 -> (SPAWN MARK))",
        "on complete -> (IF total(bound, aspect:dread) >= 1 -> (SPAWN MARK))",
    );
    assert!(script.contains("total(bound, aspect:dread) >= 1"), "the variant's trigger swapped");
    let core = core_dictionary();
    let compiled = compile_script(&script, &core).unwrap();
    let dict = compiled.dict.clone();
    let (c, base) = (compiled.content, compiled.state);
    // LAMP parks first (a 0m window), then BURN binds SPRITE for 10m and
    // its body parks the binding at completion
    let r1 = begin(&c, &dict, &base, "0:0 @ ANA HEX LAMP", 1);
    assert_eq!(r1.rejected, None);
    let r2 = advance_to(&c, &dict, &r1.state, "0:1", 2);
    assert!(world_probe(&r2.state, "total(parked, aspect:dread) = 1"), "LAMP parked");
    let r3 = begin(&c, &dict, &r2.state, "0:1 @ ANA BURN SPRITE", 3);
    assert_eq!(r3.rejected, None);
    // the completion: the body parks SPRITE mid-list, and the trigger phase
    // evaluates the IF over the CLOSING window's bindings — SPRITE is bound
    // AND parked, and the bound scope has no parked filter
    let r4 = advance_to(&c, &dict, &r3.state, "0:4", 4);
    assert_eq!(r4.rejected, None);
    assert_eq!(parked_named(&r4.state, "SPRITE"), 1, "the body parked the binding");
    assert_eq!(parked_named(&r4.state, "LAMP"), 1);
    assert!(world_probe(&r4.state, "total(table, aspect:dread) = 0"), "the table is empty");
    assert_eq!(
        living_named(&r4.state, "MARK"),
        1,
        "the bound-scope IF fired on the PARKED binding"
    );
    let records = whole_log(&[&r1, &r2, &r3, &r4]);
    let replayed = fold(&records, &base, &dict, &c).unwrap();
    assert_eq!(snap(&replayed), snap(&r4.state));
}

/// The reverse IF-arm face of T-P7, run: a world-reading IF must NOT fire on
/// a parked-only world. Same shape — LAMP parks first, BURN parks SPRITE at
/// completion, the parked scope holds 2 — but the completion trigger reads
/// `total(table, aspect:dread) >= 2`, and the table holds 0: the arm never
/// fires and no MARK exists.
#[test]
fn a_table_reading_if_never_fires_on_a_parked_only_world() {
    let script = SCRIPT.replace(
        "on complete -> (IF total(parked, aspect:dread) >= 2 -> (SPAWN MARK))",
        "on complete -> (IF total(table, aspect:dread) >= 2 -> (SPAWN MARK))",
    );
    assert!(script.contains("total(table, aspect:dread) >= 2"), "the variant's trigger swapped");
    let core = core_dictionary();
    let compiled = compile_script(&script, &core).unwrap();
    let dict = compiled.dict.clone();
    let (c, base) = (compiled.content, compiled.state);
    let r1 = begin(&c, &dict, &base, "0:0 @ ANA HEX LAMP", 1);
    assert_eq!(r1.rejected, None);
    let r2 = advance_to(&c, &dict, &r1.state, "0:1", 2);
    assert!(world_probe(&r2.state, "total(parked, aspect:dread) = 1"), "LAMP parked");
    let r3 = begin(&c, &dict, &r2.state, "0:1 @ ANA BURN SPRITE", 3);
    assert_eq!(r3.rejected, None);
    let r4 = advance_to(&c, &dict, &r3.state, "0:4", 4);
    assert_eq!(r4.rejected, None);
    assert_eq!(parked_named(&r4.state, "SPRITE"), 1, "the body parked the binding");
    assert_eq!(parked_named(&r4.state, "LAMP"), 1);
    assert!(world_probe(&r4.state, "total(parked, aspect:dread) = 2"), "the interior holds 2");
    assert!(world_probe(&r4.state, "total(table, aspect:dread) = 0"), "the table is empty");
    assert_eq!(
        living_named(&r4.state, "MARK"),
        0,
        "the table-reading IF never fired on the parked-only world"
    );
    let records = whole_log(&[&r1, &r2, &r3, &r4]);
    let replayed = fold(&records, &base, &dict, &c).unwrap();
    assert_eq!(snap(&replayed), snap(&r4.state));
}

// ------------------------------------------------------ the gate ladder ----

/// T-P8: the converter's crisis scope — a gate ladder reading all three sum
/// scopes; each arm fires in its own world.
#[test]
fn the_chain_gate_ladder_reads_all_three_scopes() {
    let (dict, c, base) = rig();
    // the bound arm: two unparked dread ideas bound into the step — the
    // pair mints first, then LADDER binds SPRITE and one WISP
    let r1 = begin(&c, &dict, &base, "0:0 @ ANA SPAWNPAIR", 1);
    assert_eq!(r1.rejected, None);
    let r2 = advance_to(&c, &dict, &r1.state, "0:1", 2);
    let r3 = begin(&c, &dict, &r2.state, "0:1 @ ANA LADDER SPRITE WITH WISP", 3);
    assert_eq!(r3.rejected, None);
    let r4 = advance_to(&c, &dict, &r3.state, "0:2", 4);
    assert_eq!(living_named(&r4.state, "MARK"), 1, "total(bound,…) >= 2 fired the ladder");
    // the table arm: an unparked pair on the table, nothing bound
    let (dict, c, base) = rig();
    let r1 = begin(&c, &dict, &base, "0:0 @ ANA SPAWNPAIR", 1);
    assert_eq!(r1.rejected, None);
    let r2 = advance_to(&c, &dict, &r1.state, "0:1", 2);
    assert!(
        world_probe(&r2.state, "total(table, aspect:dread) = 4"),
        "SPRITE and LAMP from load, plus the unparked pair"
    );
    assert!(world_probe(&r2.state, "total(parked, aspect:dread) = 0"));
    let r3 = begin(&c, &dict, &r2.state, "0:1 @ ANA LADDER", 3);
    assert_eq!(r3.rejected, None);
    let r4 = advance_to(&c, &dict, &r3.state, "0:2", 4);
    assert_eq!(living_named(&r4.state, "MARK"), 1, "total(table,…) >= 2 fired the ladder");
    // the parked arm: LAMP parked, then SPRITE parked at its completion —
    // parked dread 2, the table 0, nothing bound (the completion trigger's
    // own IF fires on the same 2 — one MARK rides along)
    let (dict, c, base) = rig();
    let r1 = begin(&c, &dict, &base, "0:0 @ ANA HEX LAMP", 1);
    assert_eq!(r1.rejected, None);
    let r2 = advance_to(&c, &dict, &r1.state, "0:1", 2);
    assert!(world_probe(&r2.state, "total(parked, aspect:dread) = 1"));
    let r3 = begin(&c, &dict, &r2.state, "0:1 @ ANA BURN SPRITE", 3);
    assert_eq!(r3.rejected, None);
    let r4 = advance_to(&c, &dict, &r3.state, "0:4", 4);
    assert!(world_probe(&r4.state, "total(parked, aspect:dread) = 2"));
    assert!(world_probe(&r4.state, "total(table, aspect:dread) = 0"));
    assert_eq!(living_named(&r4.state, "MARK"), 1, "the completion trigger's parked-scope IF");
    let r5 = begin(&c, &dict, &r4.state, "0:4 @ ANA LADDER", 5);
    assert_eq!(r5.rejected, None);
    let r6 = advance_to(&c, &dict, &r5.state, "0:5", 6);
    assert_eq!(living_named(&r6.state, "MARK"), 2, "total(parked,…) >= 2 fired the ladder");
}

// --------------------------------------------------------- the pull face ----

/// The greedy pull is the binding law's pull face: a parked card is never a
/// pull candidate, and the pull takes the oldest UNPARKED fitting card.
#[test]
fn the_greedy_pull_stays_parked_blind() {
    let script = "\
script: Parked Pull Probe
player: ANA
start: 0:0
seed: 0

entities:
  ANA is character
  BOOK is text
  CHALICE is item with dread:1
  RELIC is item with dread:1

verbs:
  STUDY:
    actors: 1 kind:character
    object: 1 kind:text
    held: 0..2 greedy aspect:dread
    duration: 30m
    recipes:
      study.got when count(held) >= 1 -> NARRATE \"held.\"
      default -> NARRATE \"plain.\"

  HEX:
    actors: 1 kind:character
    object: 1 kind:item
    duration: 0m
    recipes:
      default -> SET(object, state.parked, true)
";
    let core = core_dictionary();
    let compiled = compile_script(script, &core).unwrap();
    let dict = compiled.dict.clone();
    let (c, base) = (compiled.content, compiled.state);
    // park RELIC (the birth-order SECOND fit — the pull must skip it and
    // take CHALICE, the oldest unparked)
    let r1 = begin(&c, &dict, &base, "0:0 @ ANA HEX RELIC", 1);
    assert_eq!(r1.rejected, None);
    let r2 = advance_to(&c, &dict, &r1.state, "0:1", 2);
    // begin the greedy station: one pull, the unparked CHALICE
    let r3 = begin(&c, &dict, &r2.state, "0:1 @ ANA STUDY BOOK", 3);
    assert_eq!(r3.rejected, None);
    let holds: Vec<&Ref> = r3
        .records
        .iter()
        .filter(|rec| rec.kind == "hold")
        .filter_map(|rec| rec.held.as_ref())
        .collect();
    assert_eq!(holds.len(), 1, "one pull — the parked RELIC is not a candidate");
    let Some(Ref::Id { id }) = holds.first() else {
        panic!("the pull binds by #id");
    };
    let pulled = r3.state.entities.get(id).map(|e| e.name.clone()).unwrap_or_default();
    assert_eq!(pulled, "CHALICE", "the pull took the oldest unparked fitting card");
    let records = whole_log(&[&r1, &r2, &r3]);
    let replayed = fold(&records, &base, &dict, &c).unwrap();
    assert_eq!(snap(&replayed), snap(&r3.state));
}

/// A world where every candidate is parked pulls nothing, and the deferred
/// selection reads the empty held scope (the default decides).
#[test]
fn an_all_parked_candidate_world_pulls_nothing() {
    let script = "\
script: Parked Empty Pull Probe
player: ANA
start: 0:0
seed: 0

entities:
  ANA is character
  BOOK is text
  RELIC is item with dread:1

verbs:
  STUDY:
    actors: 1 kind:character
    object: 1 kind:text
    held: 0..2 greedy aspect:dread
    duration: 30m
    recipes:
      study.got when count(held) >= 1 -> NARRATE \"held.\"
      default -> NARRATE \"plain.\"

  HEX:
    actors: 1 kind:character
    object: 1 kind:item
    duration: 0m
    recipes:
      default -> SET(object, state.parked, true)
";
    let core = core_dictionary();
    let compiled = compile_script(script, &core).unwrap();
    let dict = compiled.dict.clone();
    let (c, base) = (compiled.content, compiled.state);
    let r1 = begin(&c, &dict, &base, "0:0 @ ANA HEX RELIC", 1);
    assert_eq!(r1.rejected, None);
    let r2 = advance_to(&c, &dict, &r1.state, "0:1", 2);
    let r3 = begin(&c, &dict, &r2.state, "0:1 @ ANA STUDY BOOK", 3);
    assert_eq!(r3.rejected, None);
    assert!(
        !r3.records.iter().any(|rec| rec.kind == "hold"),
        "every candidate parked — nothing pulls"
    );
    // the 30m completion selects the default recipe — the held scope is
    // empty (count(held) >= 1 cannot hold)
    let r4 = advance_to(&c, &dict, &r3.state, "0:4", 4);
    let complete = r4.records.iter().find(|rec| rec.kind == "complete").unwrap();
    assert_eq!(complete.recipes, ["study.default"], "the empty held scope cannot flip");
    let records = whole_log(&[&r1, &r2, &r3, &r4]);
    let replayed = fold(&records, &base, &dict, &c).unwrap();
    assert_eq!(snap(&replayed), snap(&r4.state));
}
