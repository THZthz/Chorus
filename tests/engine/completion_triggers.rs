//! `on complete` — completion triggers on a card, and the recipe `aspects`
//! token that gates them (spec §6.7): the bare/aspect forms and the authored
//! emitter gate, the scope (the window's committed locked roles — actors
//! stay free, unbound cards never fire), the dedupe and the fire order
//! (declaration order per card, locked order across cards), the logging
//! deviation (concrete rows appended to the COMPLETE row after the recipe's
//! own — no EVENT row, no `state.fired`), RUN inside the body (cross-verb
//! through the global recipe table, the parent's own window excluded from
//! the busy check, the second-RUN refusal), the window-refuse on a body
//! failure, the load checks (REFUSE ban in the new home, global id
//! uniqueness, both `max`/`aspects` orders).
//!
//! Conventions follow tests/run_chains.rs: a fresh dictionary per test, error
//! patterns copied CHARACTER FOR CHARACTER from the frozen literals
//! (regex-escaped), and play/fold equality asserted through the shared
//! `snap`.

use crate::common::harness::assert_err_msg;
use crate::common::rig::{begin_recipes, compile, kind_spine, narrated_of, ops_of, settle_line};
use crate::common::snap::snap;
use chorus::engine::settle::refusal;

use chorus::content::script::compile_script;
use chorus::core::dice::roll_die;
use chorus::core::dict::core_dictionary;
use chorus::core::sentence::Arg;
use chorus::core::types::{clone_state, AspectVal};
use chorus::engine::fold::fold;
use chorus::engine::store::{load_log, save_log};
use chorus::engine::ExecResult;

// ---------------------------------------------------------------- rigs ------

/// The rig: the caller writes the whole entities block (§6.7 lines are
/// entity-scoped, so the tests place the `on complete` lines themselves) and
/// the verbs block; prototypes are optional.
fn rig_full(entities: &str, verbs: &str, prototypes: Option<&str>) -> String {
    let prototypes = match prototypes {
        Some(p) => format!("\nprototypes:\n{p}"),
        None => String::new(),
    };
    format!(
        "
script: Completion Trigger Rig
player: ANA
start: 0:0
seed: 0

entities:
{entities}{prototypes}

verbs:
{verbs}
"
    )
}

fn rig(entities: &str, verbs: &str) -> String {
    rig_full(entities, verbs, None)
}

/// The zero-duration one-actor DREAM body (slots and recipes per test).
fn dream(slots: &str, recipes: &str) -> String {
    format!(
        "  DREAM:\n    actors: 1 kind:character\n{slots}    duration: 0m\n    recipes:\n{recipes}"
    )
}

/// Living entities carrying this name.
fn living_named(r: &ExecResult, name: &str) -> usize {
    r.state.entities.values().filter(|e| e.name == name && e.dead_action.is_none()).count()
}

// ----------------------------------------------- bare form + REPLACE self ----

/// The CS xtrigger shape: a bare `on complete` fires on every completion
/// binding the card, and its body REPLACEs the carrier itself
/// (health → fatigue). Fold agrees.
#[test]
fn a_bare_trigger_fires_and_replaces_self() {
    let dict = core_dictionary();
    let (c, s) = compile(
        &dict,
        &rig_full(
            "  ANA is character at HOUSE
  HOUSE is place
  HEALTH is resource
  on complete -> (REPLACE HEALTH WITH FATIGUE)",
            &dream(
                "    object: 0..1 kind:resource\n",
                "      rest when count(actors) >= 1 -> NARRATE \"rested\"",
            ),
            Some("  FATIGUE is idea"),
        ),
    );
    let base = clone_state(&s);
    let r = settle_line(&c, &dict, s, "0:0 @ ANA DREAM HEALTH", 1).unwrap();
    assert_eq!(r.rejected, None);
    assert_eq!(kind_spine(&r), ["action", "complete", "wait"], "no EVENT rows");
    // the recipe's own NARRATE first; the trigger's REPLACE logged as its
    // concrete SPAWN+DESTROY pair, appended after
    assert_eq!(ops_of(&r, 1), ["NARRATE", "SPAWN", "DESTROY"]);
    assert_eq!(living_named(&r, "HEALTH"), 0, "the carrier was replaced");
    assert_eq!(living_named(&r, "FATIGUE"), 1);
    // fold agrees
    let replayed = fold(&r.records, &base, &dict, &c).unwrap();
    assert_eq!(snap(&replayed), snap(&r.state));
}

/// On a PROTOTYPE the trigger inherits like other prototype members: the
/// mint carries the prototype's name, so the name-keyed gate fires when a
/// completion binds the mint. A mint spawned during one completion is not
/// bound by it (the scan set is the BEGIN-time lock list) — it fires on the
/// NEXT completion that binds it.
#[test]
fn a_prototype_trigger_reaches_its_mints() {
    let dict = core_dictionary();
    let (c, s) = compile(
        &dict,
        &rig_full(
            "  ANA is character at HOUSE
  HOUSE is place",
            &dream(
                "    object: 0..1 kind:idea\n",
                "      hold when count(object) >= 1 -> NARRATE \"held\"\n      spawn -> SPAWN SPRITE",
            ),
            Some("  SPRITE is idea\n  on complete -> (NARRATE \"sprite done\")"),
        ),
    );
    let base = clone_state(&s);
    // completion 1: `spawn` is the default — the mint is born, not bound
    let r1 = settle_line(&c, &dict, s, "0:0 @ ANA DREAM", 1).unwrap();
    assert_eq!(r1.rejected, None);
    assert_eq!(ops_of(&r1, 1), ["SPAWN"], "the mint is not bound by its own birth");
    // completion 2 binds the mint: `hold` matches, the prototype's trigger fires
    let r2 = settle_line(&c, &dict, r1.state, "0:0 @ ANA DREAM SPRITE", 100).unwrap();
    assert_eq!(r2.rejected, None);
    assert_eq!(narrated_of(&r2, 1), ["held", "sprite done"]);
    // fold agrees across both plays
    let mut records = r1.records.clone();
    records.extend(r2.records.iter().cloned());
    let replayed = fold(&records, &base, &dict, &c).unwrap();
    assert_eq!(snap(&replayed), snap(&r2.state));
}

// ------------------------------------------------------------- the gate ------

/// The authored emitter gate: the aspect form fires iff the completed recipe
/// declares the aspect; the bare form on the same card always fires — the
/// same card bound under a recipe without the aspect wakes only the bare one.
#[test]
fn the_aspect_gate_reads_the_recipes_aspects() {
    let dict = core_dictionary();
    let (c, s) = compile(
        &dict,
        &rig(
            "  ANA is character at HOUSE
  HOUSE is place
  HEALTH is resource
  on complete employment -> (NARRATE \"gated\")
  on complete -> (NARRATE \"bare\")
  CALM is resource",
            &dream(
                "    object: 0..1 kind:resource\n    with: 0..1 kind:resource\n",
                "      plain when count(with) >= 1 -> NARRATE \"plain\"\n      tagged aspects employment when count(object) >= 1 -> NARRATE \"tagged\"",
            ),
        ),
    );
    // completion 1: the recipe declares `employment` — gated (declared
    // first) then bare, in declaration order
    let r1 = settle_line(&c, &dict, clone_state(&s), "0:0 @ ANA DREAM HEALTH", 1).unwrap();
    assert_eq!(r1.rejected, None);
    assert_eq!(narrated_of(&r1, 1), ["tagged", "gated", "bare"]);
    // completion 2: the SAME card under the aspect-less recipe — the gated
    // form stays silent, the bare one fires
    let r2 = settle_line(&c, &dict, s, "0:0 @ ANA DREAM HEALTH WITH CALM", 100).unwrap();
    assert_eq!(r2.rejected, None);
    assert_eq!(narrated_of(&r2, 1), ["plain", "bare"]);
}

/// A dotted aspect names the same keys in both grammars: the head token
/// class IS the `aspects`-member class, and the gate matches the full
/// dotted key (`language.latin` — the scholar/converter shape).
#[test]
fn dotted_aspects_name_the_same_key_on_both_sides() {
    let dict = core_dictionary();
    let (c, s) = compile(
        &dict,
        &rig(
            "  ANA is character at HOUSE
  HOUSE is place
  HEALTH is resource
  on complete language.latin -> (NARRATE \"latin\")",
            &dream(
                "    object: 0..1 kind:resource\n",
                "      scholar aspects language.latin when count(object) >= 1 -> NARRATE \"scholar\"\n      other -> NARRATE \"other\"",
            ),
        ),
    );
    let r = settle_line(&c, &dict, clone_state(&s), "0:0 @ ANA DREAM HEALTH", 1).unwrap();
    assert_eq!(r.rejected, None);
    assert_eq!(narrated_of(&r, 1), ["scholar", "latin"]);
    // fold agrees
    let replayed = fold(&r.records, &s, &dict, &c).unwrap();
    assert_eq!(snap(&replayed), snap(&r.state));
}

// ------------------------------------------------------------- the scope -----

/// Scope: a card NOT bound in the window never fires (the table card), and
/// actors stay free — the player's own completion-trigger card does not fire
/// from its actor binding.
#[test]
fn unbound_cards_and_actors_never_fire() {
    let dict = core_dictionary();
    let (c, s) = compile(
        &dict,
        &rig(
            "  ANA is character at HOUSE on complete -> (NARRATE \"ana fired\")
  HOUSE is place
  HEALTH is resource at HOUSE
  on complete -> (NARRATE \"health fired\")",
            &dream("", "      rest -> NARRATE \"rested\""),
        ),
    );
    let base = clone_state(&s);
    let r = settle_line(&c, &dict, s, "0:0 @ ANA DREAM", 1).unwrap();
    assert_eq!(r.rejected, None);
    assert_eq!(narrated_of(&r, 1), ["rested"], "neither the table card nor the free actor fires");
    let replayed = fold(&r.records, &base, &dict, &c).unwrap();
    assert_eq!(snap(&replayed), snap(&r.state));
}

// ------------------------------------------------------------ dedupe/order ---

/// A card bound through two roles is on the lock list twice and fires ONCE.
#[test]
fn a_card_in_two_roles_fires_once() {
    let dict = core_dictionary();
    let (c, s) = compile(
        &dict,
        &rig(
            "  ANA is character at HOUSE
  HOUSE is place
  HEALTH is resource
  on complete -> (NARRATE \"deduped\")",
            &dream(
                "    object: 1 kind:resource\n    with: 1 kind:resource\n",
                "      both when count(actors) >= 1 -> NARRATE \"both\"",
            ),
        ),
    );
    let base = clone_state(&s);
    let r = settle_line(&c, &dict, s, "0:0 @ ANA DREAM HEALTH WITH HEALTH", 1).unwrap();
    assert_eq!(r.rejected, None);
    assert_eq!(narrated_of(&r, 1), ["both", "deduped"], "exactly one firing despite two roles");
    let replayed = fold(&r.records, &base, &dict, &c).unwrap();
    assert_eq!(snap(&replayed), snap(&r.state));
}

/// Deterministic order, pinned: two triggers on one card fire in declaration
/// order; two cards fire in the lock list's order (object before with — the
/// COMMITTED_ROLES walk).
#[test]
fn fire_order_is_declaration_then_locked_order() {
    let dict = core_dictionary();
    let (c, s) = compile(
        &dict,
        &rig(
            "  ANA is character at HOUSE
  HOUSE is place
  FIRST_CARD is resource
  on complete -> (NARRATE \"first-one\")
  on complete -> (NARRATE \"first-two\")
  SECOND_CARD is resource
  on complete -> (NARRATE \"second\")",
            &dream(
                "    object: 1 kind:resource\n    with: 1 kind:resource\n",
                "      both when count(actors) >= 1 -> NARRATE \"both\"",
            ),
        ),
    );
    let base = clone_state(&s);
    let r = settle_line(&c, &dict, s, "0:0 @ ANA DREAM FIRST_CARD WITH SECOND_CARD", 1).unwrap();
    assert_eq!(r.rejected, None);
    assert_eq!(
        narrated_of(&r, 1),
        ["both", "first-one", "first-two", "second"],
        "the object card's triggers (declaration order), then the with card's"
    );
    let replayed = fold(&r.records, &base, &dict, &c).unwrap();
    assert_eq!(snap(&replayed), snap(&r.state));
}

// -------------------------------------------------------------- logging ------

/// The §6.7 logging deviation: the trigger's concrete outcomes append to
/// the COMPLETE row after the recipe's own — no EVENT row anywhere, no
/// `state.fired` entries — and the fold replays the appended list without
/// recomputing gates (structural + snap equality, JSONL included).
#[test]
fn outcomes_append_to_the_complete_row_and_fold_replays_them() {
    let dict = core_dictionary();
    let (c, s) = compile(
        &dict,
        &rig(
            "  ANA is character at HOUSE
  HOUSE is place
  HEALTH is resource
  on complete -> (SET(HEALTH, aspect:stress, +2), NARRATE \"triggered\")",
            &dream(
                "    object: 0..1 kind:resource\n",
                "      rest when count(actors) >= 1 -> NARRATE \"done\"",
            ),
        ),
    );
    let base = clone_state(&s);
    let r = settle_line(&c, &dict, s, "0:0 @ ANA DREAM HEALTH", 1).unwrap();
    assert_eq!(r.rejected, None);
    assert_eq!(kind_spine(&r), ["action", "complete", "wait"]);
    assert_eq!(ops_of(&r, 1), ["NARRATE", "SET", "NARRATE"], "recipe first, trigger appended");
    assert!(r.state.fired.is_empty(), "completion triggers never touch state.fired");
    // the SET landed (the trigger's effects are real state)
    let health = r.state.entities.values().find(|e| e.name == "HEALTH").unwrap();
    assert_eq!(health.aspects.get("stress"), Some(&AspectVal::Num(2)));
    // fold agrees — the appended rows replay as logged, gates never recomputed
    let replayed = fold(&r.records, &base, &dict, &c).unwrap();
    assert_eq!(replayed.windows, r.state.windows);
    assert_eq!(snap(&replayed), snap(&r.state));
    assert!(replayed.fired.is_empty());
    // the records survive JSONL and still fold identically
    let dir = std::env::temp_dir().join(format!("chorus-complete-triggers-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("log.jsonl");
    save_log(&path, &r.records).unwrap();
    let loaded = load_log(&path).unwrap();
    assert_eq!(loaded, r.records);
    let refolded = fold(&loaded, &base, &dict, &c).unwrap();
    assert_eq!(snap(&refolded), snap(&r.state));
}

// ------------------------------------------------------------ RUN in body ----

/// ADVANCE inside the body stays legal — a completion trigger fires inside
/// the pass its window's completion rides, so the extension reaches the
/// pass target (the two event kinds reject ADVANCE at load, §6.6: they own
/// no pass to stretch).
#[test]
fn advance_in_the_body_stays_legal_and_stretches_the_pass() {
    let dict = core_dictionary();
    let verbs = &format!(
        "  WORK:\n    actors: 1 kind:character\n    object: 1 kind:resource\n    duration: 0m\n    recipes:\n      work_job when count(object) >= 1 -> NARRATE \"worked\"\n{}",
        dream("", "      dream_rest when count(actors) >= 1 -> NARRATE \"dreamed\"")
    );
    let entities = "  ANA is character at HOUSE
  HOUSE is place
  JOB is resource
  on complete -> (ADVANCE(1h), NARRATE \"the job drags on\")";
    let (c, s) = compile(&dict, &rig(entities, verbs));
    let r = settle_line(&c, &dict, s, "0:0 @ ANA WORK JOB", 1).unwrap();
    assert_eq!(r.rejected, None, "ADVANCE in an on complete body compiles and runs");
    // the wait row closing the pass spans the authored extension
    let wait = r.records.iter().find(|x| x.kind == "wait").expect("the pass ends in a wait row");
    assert_eq!(wait.advanced_min, 60, "the completion's ADVANCE stretched the pass by 1h");
}

/// RUN inside the body hands off across verbs (the job→needs bootstrap
/// shape): a WORK completion binding the JOB card opens a DREAM window
/// through the global recipe table. The follow-up consumes the carrier (the
/// needs cycle eats the job), which is what ends the chain — a destroyed
/// card fires nothing. The same-verb form works too — the parent's own
/// window is excluded from the busy check.
#[test]
fn run_in_the_body_hands_off_across_verbs() {
    let dict = core_dictionary();
    let verbs = &format!(
        "  WORK:\n    actors: 1 kind:character\n    object: 1 kind:resource\n    duration: 0m\n    recipes:\n      work_job aspects employment when count(object) >= 1 -> NARRATE \"worked\"\n{}",
        dream("", "      dream_rest when count(actors) >= 1 -> DESTROY JOB, NARRATE \"dreamed\"")
    );
    let entities = "  ANA is character at HOUSE
  HOUSE is place
  JOB is resource
  on complete -> (RUN \"dream_rest\")";
    let (c, s) = compile(&dict, &rig(entities, verbs));
    let base = clone_state(&s);
    let r = settle_line(&c, &dict, s, "0:0 @ ANA WORK JOB", 1).unwrap();
    assert_eq!(r.rejected, None, "cross-verb candidates resolve");
    assert_eq!(
        kind_spine(&r),
        ["action", "complete", "action", "complete", "wait"],
        "the follow-up opened on the other verb"
    );
    assert_eq!(begin_recipes(&r), [vec!["work_job".to_string()], vec!["dream_rest".to_string()]]);
    assert_eq!(r.records[2].verb, "DREAM", "the follow-up's verb is the candidate's");
    assert_eq!(refusal(&r), None);
    assert_eq!(
        living_named(&r, "JOB"),
        0,
        "the follow-up consumed the carrier — the chain ends (a destroyed card fires nothing)"
    );
    // fold agrees
    let replayed = fold(&r.records, &base, &dict, &c).unwrap();
    assert_eq!(replayed.windows, r.state.windows);
    assert_eq!(snap(&replayed), snap(&r.state));

    // the same-verb hand-off: the parent's own window must not read busy
    let (c2, s2) = compile(
        &dict,
        &rig(
            "  ANA is character at HOUSE
  HOUSE is place
  HEALTH is resource
  on complete -> (RUN \"dream_rest\")",
            &dream(
                "    object: 0..1 kind:resource\n",
                "      runner when count(actors) >= 1 -> NARRATE \"ran\"\n      dream_rest when count(actors) >= 1 -> DESTROY HEALTH, NARRATE \"dreamed\"",
            ),
        ),
    );
    let base2 = clone_state(&s2);
    let r2 = settle_line(&c2, &dict, s2, "0:0 @ ANA DREAM HEALTH", 1).unwrap();
    assert_eq!(r2.rejected, None);
    assert_eq!(
        begin_recipes(&r2),
        [vec!["runner".to_string()], vec!["dream_rest".to_string()]],
        "the parent's own window is excluded from the busy check"
    );
    let replayed2 = fold(&r2.records, &base2, &dict, &c2).unwrap();
    assert_eq!(snap(&replayed2), snap(&r2.state));
}

/// Any OTHER busy station skips the candidate silently: the hand-off targets
/// a verb whose station holds an open window — no follow-up, no refusal.
#[test]
fn a_busy_station_skips_the_candidate_silently() {
    let dict = core_dictionary();
    let verbs = &format!(
        "  STUDY:\n    actors: 1 kind:character\n    duration: 2h\n    recipes:\n      study_thing when count(actors) >= 1 -> NARRATE \"studied\"\n{}",
        dream(
            "    object: 0..1 kind:resource\n",
            "      drifter -> NARRATE \"drifted\"",
        )
    );
    let entities = "  ANA is character at HOUSE
  HOUSE is place
  HEALTH is resource
  on complete -> (RUN \"study_thing\")";
    let (c, s) = compile(&dict, &rig(entities, verbs));
    // one step, two stations: the DREAM window (0m) completes while the
    // STUDY window (2h) holds its station — the body's RUN skips silently
    let r = settle_line(&c, &dict, s, "0:0 @ ANA DREAM HEALTH ; ANA STUDY", 1).unwrap();
    assert_eq!(r.rejected, None);
    assert_eq!(refusal(&r), None, "a busy station skips, it does not refuse");
    assert_eq!(kind_spine(&r), ["action", "complete", "complete", "wait"]);
    assert_eq!(
        begin_recipes(&r),
        [vec!["drifter".to_string(), "study_thing".to_string()]],
        "one BEGIN, no follow-up"
    );
    assert_eq!(narrated_of(&r, 1), ["drifted"]);
    assert_eq!(
        r.state.executed.get("study_thing"),
        Some(&1),
        "the skipped candidate never registered a second window"
    );
    // contrast: with the station free the same body opens the follow-up
    let (_, s2) = compile(&dict, &rig(entities, verbs));
    let r2 = settle_line(&c, &dict, s2, "0:0 @ ANA DREAM HEALTH", 300).unwrap();
    assert_eq!(r2.rejected, None);
    assert_eq!(begin_recipes(&r2), [vec!["drifter".to_string()], vec!["study_thing".to_string()]]);
}

// ------------------------------------------------------- regression pins -----

/// Two parked RUNs in one recipe body replay in list order: the first pins
/// and opens its follow-up, the second is REACHED at replay and refuses the
/// parent window — the NARRATE between them keeps its list position.
#[test]
fn two_parked_runs_refuse_at_replay_in_list_order() {
    let dict = core_dictionary();
    let (c, s) = compile(
        &dict,
        &rig(
            "  ANA is character at HOUSE
  HOUSE is place",
            &dream(
                "",
                "      runner when count(actors) >= 1 -> RUN \"x\", NARRATE \"mid\", RUN \"y\"\n      x when count(actors) >= 1 -> NARRATE \"x\"\n      y when count(actors) >= 1 -> NARRATE \"y\"",
            ),
        ),
    );
    let base = clone_state(&s);
    let r = settle_line(&c, &dict, s, "0:0 @ ANA DREAM", 1).unwrap();
    assert_eq!(r.rejected, None);
    assert_eq!(
        refusal(&r),
        Some("a second RUN in one completion — a station holds one window (§6.7)".to_string())
    );
    assert_eq!(kind_spine(&r), ["action", "complete", "wait"], "no follow-up survived");
    assert_eq!(ops_of(&r, 1), ["NARRATE", "REFUSE"]);
    assert_eq!(r.state.executed.get("runner"), Some(&1));
    assert!(r.state.executed.get("x").is_none(), "the parked first RUN's follow-up died too");
    let replayed = fold(&r.records, &base, &dict, &c).unwrap();
    assert_eq!(snap(&replayed), snap(&r.state));
}

/// A parked RUN whose pinned candidate's guard REFUSEs rejects the PARENT
/// window — and the completion trigger's own effects (applied in the
/// trigger phase, before the parked RUN replayed) die with the scratch.
#[test]
fn a_parked_run_refuses_through_its_candidates_guard() {
    let dict = core_dictionary();
    let (c, s) = compile(
        &dict,
        &rig(
            "  ANA is character at HOUSE
  HOUSE is place
  HEALTH is resource
  on complete -> (SET(HEALTH, aspect:touched, +1))",
            &dream(
                "    object: 0..1 kind:resource\n",
                "      runner when count(actors) >= 1 -> NARRATE \"ran\", RUN \"gated\"\n      gated when count(actors) >= 1 -> REFUSE(\"the gate says no\")",
            ),
        ),
    );
    let base = clone_state(&s);
    let r = settle_line(&c, &dict, s, "0:0 @ ANA DREAM HEALTH", 1).unwrap();
    assert_eq!(r.rejected, None);
    assert_eq!(refusal(&r), Some("the gate says no".to_string()));
    assert_eq!(kind_spine(&r), ["action", "complete", "wait"]);
    assert_eq!(
        ops_of(&r, 1),
        ["NARRATE", "SET", "REFUSE"],
        "the trigger phase ran before the parked RUN's guard"
    );
    let health = r.state.entities.values().find(|e| e.name == "HEALTH").unwrap();
    assert_eq!(
        health.aspects.get("touched"),
        None,
        "the refused window's scratch took the trigger's SET with it"
    );
    let replayed = fold(&r.records, &base, &dict, &c).unwrap();
    assert_eq!(snap(&replayed), snap(&r.state));
}

/// F-6 (the pin-earned slot, cross-phase): a trigger-body RUN pins during
/// the trigger phase, then the parked recipe-body RUN replays — the second
/// chain op refuses (the slot was earned by the trigger's pin).
#[test]
fn a_trigger_pinned_run_refuses_a_later_deferred_run() {
    let dict = core_dictionary();
    let (c, s) = compile(
        &dict,
        &rig(
            "  ANA is character at HOUSE
  HOUSE is place
  HEALTH is resource
  on complete -> (RUN \"trig\")",
            &dream(
                "    object: 0..1 kind:resource\n",
                "      runner when count(actors) >= 1 -> NARRATE \"ran\", RUN \"late\"\n      trig when count(actors) >= 1 -> NARRATE \"trig\"\n      late when count(actors) >= 1 -> NARRATE \"late\"",
            ),
        ),
    );
    let base = clone_state(&s);
    let r = settle_line(&c, &dict, s, "0:0 @ ANA DREAM HEALTH", 1).unwrap();
    assert_eq!(r.rejected, None);
    assert_eq!(
        refusal(&r),
        Some("a second RUN in one completion — a station holds one window (§6.7)".to_string())
    );
    assert_eq!(kind_spine(&r), ["action", "complete", "wait"], "no follow-up survived");
    let ops: Vec<&str> = r.records[1].outcomes.iter().map(|o| o.op.as_str()).collect();
    assert_eq!(ops, ["NARRATE", "REFUSE"]);
    let replayed = fold(&r.records, &base, &dict, &c).unwrap();
    assert_eq!(snap(&replayed), snap(&r.state));
}

/// A card the recipe's own outcomes destroy is dead by the trigger phase and
/// fires nothing — the gate skips dead locked cards the way bindings drop
/// dead refs.
#[test]
fn a_card_destroyed_by_the_recipe_fires_nothing() {
    let dict = core_dictionary();
    let (c, s) = compile(
        &dict,
        &rig(
            "  ANA is character at HOUSE
  HOUSE is place
  HEALTH is resource
  on complete -> (NARRATE \"health fired\")",
            &dream(
                "    object: 0..1 kind:resource\n",
                "      reaper when count(object) >= 1 -> DESTROY HEALTH, NARRATE \"reaped\"",
            ),
        ),
    );
    let base = clone_state(&s);
    let r = settle_line(&c, &dict, s, "0:0 @ ANA DREAM HEALTH", 1).unwrap();
    assert_eq!(r.rejected, None);
    assert_eq!(ops_of(&r, 1), ["DESTROY", "NARRATE"]);
    assert_eq!(narrated_of(&r, 1), ["reaped"], "the destroyed carrier's trigger never fires");
    assert_eq!(living_named(&r, "HEALTH"), 0);
    let replayed = fold(&r.records, &base, &dict, &c).unwrap();
    assert_eq!(snap(&replayed), snap(&r.state));
}

/// Rename-stops-matching: when the RECIPE replaces the bound card (before
/// the trigger phase), neither name fires — the old name's carrier is dead
/// and the renamed survivor is not on the lock list (the scan set is the
/// BEGIN-time lock list; the trigger that transforms the carrier must be
/// authored on the carrier itself, as in the CS story).
#[test]
fn a_recipe_replace_stops_the_old_names_trigger() {
    let dict = core_dictionary();
    let (c, s) = compile(
        &dict,
        &rig_full(
            "  ANA is character at HOUSE
  HOUSE is place
  HEALTH is resource
  on complete -> (NARRATE \"health fired\")",
            &dream(
                "    object: 0..1 kind:resource\n",
                "      replacer when count(object) >= 1 -> REPLACE HEALTH WITH FATIGUE, NARRATE \"swapped\"",
            ),
            Some("  FATIGUE is idea\n  on complete -> (NARRATE \"fatigue fired\")"),
        ),
    );
    let base = clone_state(&s);
    let r = settle_line(&c, &dict, s, "0:0 @ ANA DREAM HEALTH", 1).unwrap();
    assert_eq!(r.rejected, None);
    assert_eq!(ops_of(&r, 1), ["SPAWN", "DESTROY", "NARRATE"]);
    assert_eq!(
        narrated_of(&r, 1),
        ["swapped"],
        "neither the dead carrier's nor the survivor's trigger fires"
    );
    assert_eq!(living_named(&r, "HEALTH"), 0);
    assert_eq!(living_named(&r, "FATIGUE"), 1);
    let replayed = fold(&r.records, &base, &dict, &c).unwrap();
    assert_eq!(snap(&replayed), snap(&r.state));
}

// ----------------------------------------------- failure, REFUSE, ROLL -------

/// A failure inside a body refuses the WINDOW (§6.7's parent-window path,
/// not the event triggers' whole-step throw): the partial effects die with
/// the scratch, the marker rides the COMPLETE row, the fold no-ops.
#[test]
fn a_failure_inside_a_body_refuses_the_window() {
    let dict = core_dictionary();
    let (c, s) = compile(
        &dict,
        &rig_full(
            "  ANA is character at HOUSE
  HOUSE is place
  HEALTH is resource
  on complete -> (SPAWN TRINKET)",
            &dream(
                "    object: 0..1 kind:resource\n",
                "      spawner when count(actors) >= 1 -> SPAWN TRINKET, NARRATE \"spawned\"",
            ),
            Some("  TRINKET is idea"),
        ),
    );
    let base = clone_state(&s);
    let r = settle_line(&c, &dict, s, "0:0 @ ANA DREAM HEALTH", 1).unwrap();
    assert_eq!(r.rejected, None);
    let msg = refusal(&r).unwrap_or_else(|| panic!("expected the window refusal"));
    assert!(msg.contains("already in use"), "{msg}");
    assert_eq!(kind_spine(&r), ["action", "complete", "wait"]);
    assert_eq!(ops_of(&r, 1), ["REFUSE"], "partial effects discarded — only the marker logs");
    assert_eq!(living_named(&r, "TRINKET"), 0, "the scratch died whole");
    let replayed = fold(&r.records, &base, &dict, &c).unwrap();
    assert_eq!(snap(&replayed), snap(&r.state));
}

/// REFUSE authored in an `on complete` body is a load error (§6.6 in the
/// new home); PROGRESS likewise. And ROLL inside the body works — the dice
/// consume counters normally, {roll} rewrites, the fold replays the ROLLED.
#[test]
fn refuse_is_a_load_error_and_roll_works() {
    let dict = core_dictionary();
    // the REFUSE ban, suffix and standalone homes alike
    assert_err_msg(
        compile_script(
            &rig(
                "  ANA is character at HOUSE
  HOUSE is place
  HEALTH is resource on complete -> (REFUSE(\"no\"))",
                &dream("", "      rest -> NARRATE \"rested\""),
            ),
            &dict,
        ),
        "trigger complete:HEALTH: REFUSE belongs to verb recipes — a completion trigger cannot refuse the completion it fires in \\(§6\\.6, §6\\.7\\)",
    );
    assert_err_msg(
        compile_script(
            &rig(
                "  ANA is character at HOUSE
  HOUSE is place
  HEALTH is resource
  on complete -> (REFUSE(\"no\"))",
                &dream("", "      rest -> NARRATE \"rested\""),
            ),
            &dict,
        ),
        "trigger complete:HEALTH: REFUSE belongs to verb recipes",
    );
    // PROGRESS is the completing recipe's property, not a trigger's
    assert_err_msg(
        compile_script(
            &rig(
                "  ANA is character at HOUSE
  HOUSE is place
  HEALTH is resource
  on complete -> (PROGRESS(30m, \"tick\"))",
                &dream("", "      rest -> NARRATE \"rested\""),
            ),
            &dict,
        ),
        "trigger complete:HEALTH: PROGRESS belongs to the completing recipe — completion triggers fire after the outcome list \\(§6\\.7\\)",
    );
    // ROLL inside: deterministic from the seed, ROLLED logged, {roll} rewritten
    let (c, s) = compile(
        &dict,
        &rig(
            "  ANA is character at HOUSE
  HOUSE is place
  HEALTH is resource
  on complete -> (ROLL(1, 2), NARRATE \"rolled {roll}\")",
            &dream(
                "    object: 0..1 kind:resource\n",
                "      rest when count(actors) >= 1 -> NARRATE \"rested\"",
            ),
        ),
    );
    let base = clone_state(&s);
    let total = roll_die(0, 0, 2);
    let r = settle_line(&c, &dict, s, "0:0 @ ANA DREAM HEALTH", 1).unwrap();
    assert_eq!(r.rejected, None);
    assert_eq!(ops_of(&r, 1), ["NARRATE", "ROLLED", "NARRATE"]);
    assert_eq!(r.records[1].outcomes[1].args[0], Arg::Int { v: total });
    assert_eq!(r.records[1].outcomes[2].args[0], Arg::Str { v: format!("rolled {total}") });
    assert_eq!(r.state.rng, 1, "the counter consumed one die");
    let replayed = fold(&r.records, &base, &dict, &c).unwrap();
    assert_eq!(snap(&replayed), snap(&r.state));
}

// ------------------------------------------------------------ load checks ----

/// The load checks: a cross-verb id collision (two verbs authoring the same
/// recipe id, referenced from a trigger body) is a load error naming both
/// verbs; an unknown candidate too; `aspects` composes with `max` in both
/// orders; the role-free lint does NOT run on these bodies.
#[test]
fn the_load_checks_pin_the_table_the_ban_and_the_orders() {
    let dict = core_dictionary();
    let two_verbs = |recipes: &str| {
        rig(
            "  ANA is character at HOUSE
  HOUSE is place
  HEALTH is resource
  on complete -> (RUN \"collide\")",
            &format!(
                "  WORK:\n    actors: 1 kind:character\n    duration: 0m\n    recipes:\n{recipes}\n  DREAM:\n    actors: 1 kind:character\n    duration: 0m\n    recipes:\n      collide when count(actors) >= 1 -> NARRATE \"dream collide\""
            ),
        )
    };
    // the global table demands unique ids across verbs — the same id in two
    // verbs makes a cross-verb resolution ambiguous, so it fails loud
    assert_err_msg(
        compile_script(
            &two_verbs("      collide when count(actors) >= 1 -> NARRATE \"work collide\""),
            &dict,
        ),
        "recipe id 'collide' is declared by both verb WORK and verb DREAM — chain-op resolution needs globally unique recipe ids \\(§6\\.7\\)",
    );
    // an unknown candidate resolves nowhere — load error
    assert_err_msg(
        compile_script(
            &rig(
                "  ANA is character at HOUSE
  HOUSE is place
  HEALTH is resource
  on complete -> (RUN \"nope\")",
                &dream("", "      rest -> NARRATE \"rested\""),
            ),
            &dict,
        ),
        "trigger complete:HEALTH: RUN 'nope' does not name a compiled recipe — candidates resolve against the global recipe table \\(§6\\.7\\)",
    );
    // both positional orders parse: `max` before `aspects` and after
    compile(
        &dict,
        &rig(
            "  ANA is character at HOUSE
  HOUSE is place",
            &dream(
                "",
                "      a max 1 aspects employment when count(actors) >= 1 -> NARRATE \"a\"\n      b aspects employment max 1 when count(actors) >= 1 -> NARRATE \"b\"",
            ),
        ),
    );
    // a bare `aspects` with no name is a load error
    assert_err_msg(
        compile_script(
            &rig(
                "  ANA is character at HOUSE
  HOUSE is place",
                &dream("", "      a aspects when count(actors) >= 1 -> NARRATE \"a\""),
            ),
            &dict,
        ),
        "line \\d+: `aspects` needs at least one name — aspects <name> \\[, <name>\\]\\*",
    );
    // the role-free lint does NOT run on completion-trigger bodies: a
    // role-reading IF condition (and a role-reading ROLL operand) compile —
    // the window's bindings are live there (slot triggers reject both)
    compile(
        &dict,
        &rig(
            "  ANA is character at HOUSE
  HOUSE is place
  HEALTH is resource
  on complete -> (IF actors.aspect.mood >= 1 -> (NARRATE \"high\"), ROLL(actors.aspect.mood, 2), NARRATE \"done\")",
            &dream(
                "    object: 0..1 kind:resource\n",
                "      rest when count(actors) >= 1 -> NARRATE \"rested\"",
            ),
        ),
    );
    // the §4.4 declared-checks walk completion bodies like any other
    // trigger home: an undeclared ENDING id is a load error, a declared one
    // compiles, and a DRAW of an unknown deck too
    assert_err_msg(
        compile_script(
            &rig(
                "  ANA is character at HOUSE
  HOUSE is place
  HEALTH is resource
  on complete -> (ENDING NOPE)",
                &dream("", "      rest -> NARRATE \"rested\""),
            ),
            &dict,
        ),
        "trigger complete:HEALTH: ENDING 'NOPE' is not in the endings table",
    );
    compile(
        &dict,
        &format!(
            "{}\nendings:\n  DOOM: \"Doom\"",
            rig(
                "  ANA is character at HOUSE
  HOUSE is place
  HEALTH is resource
  on complete -> (ENDING DOOM)",
                &dream("", "      rest -> NARRATE \"rested\""),
            )
        ),
    );
    assert_err_msg(
        compile_script(
            &rig(
                "  ANA is character at HOUSE
  HOUSE is place
  HEALTH is resource
  on complete -> (DRAW NODECK)",
                &dream("", "      rest -> NARRATE \"rested\""),
            ),
            &dict,
        ),
        "trigger complete:HEALTH: DRAW 'NODECK' is not a declared deck \\(decks:\\)",
    );
    // a dotted aspect head reads the same class the `aspects` list declares;
    // a name ending at its dot is a load error
    compile(
        &dict,
        &rig(
            "  ANA is character at HOUSE
  HOUSE is place
  HEALTH is resource
  on complete language.latin -> (NARRATE \"x\")",
            &dream("", "      rest -> NARRATE \"rested\""),
        ),
    );
    assert_err_msg(
        compile_script(
            &rig(
                "  ANA is character at HOUSE
  HOUSE is place
  HEALTH is resource
  on complete language. -> (NARRATE \"x\")",
                &dream("", "      rest -> NARRATE \"rested\""),
            ),
            &dict,
        ),
        "bad aspect 'language\\.' — a dotted aspect name cannot end at its dot \\(§10\\)",
    );
    // a wide-space boundary before `on` splits exactly like an ASCII one
    // (U+3000 and U+FEFF are both in the frozen whitespace class)
    let (cw, sw) = compile(
        &dict,
        &rig(
            "  ANA is character at HOUSE
  HOUSE is place
  HEALTH is resource\u{3000}on complete -> (NARRATE \"wide\")",
            &dream(
                "    object: 0..1 kind:resource\n",
                "      rest when count(actors) >= 1 -> NARRATE \"rested\"",
            ),
        ),
    );
    let rw = settle_line(&cw, &dict, sw, "0:0 @ ANA DREAM HEALTH", 1).unwrap();
    assert_eq!(rw.rejected, None);
    assert_eq!(narrated_of(&rw, 1), ["rested", "wide"]);
    // `with` is reserved — an aspect may not collide with the clause keyword
    assert_err_msg(
        compile_script(
            &rig(
                "  ANA is character at HOUSE
  HOUSE is place
  HEALTH is resource on complete with -> (NARRATE \"x\")",
                &dream("", "      rest -> NARRATE \"rested\""),
            ),
            &dict,
        ),
        "aspect 'with' is a reserved clause keyword \\(§10\\)",
    );
    // a standalone `on complete` with no preceding declaration is a load error
    assert_err_msg(
        compile_script(
            &rig(
                "  on complete -> (NARRATE \"x\")
  ANA is character at HOUSE
  HOUSE is place",
                &dream("", "      rest -> NARRATE \"rested\""),
            ),
            &dict,
        ),
        "'on complete' needs a preceding entity declaration \\(§10\\)",
    );
}

/// F2-10: a parked recipe-body FORK replays after the trigger phase — a
/// trigger-body RUN that pins earns the slot, and the replayed fork refuses
/// with the chain-op message; the turn still folds identically.
#[test]
fn a_trigger_pinned_run_refuses_a_replayed_fork() {
    let dict = core_dictionary();
    let (c, s) = compile(
        &dict,
        &rig(
            "  ANA is character at HOUSE
  HOUSE is place
  HEALTH is resource
  on complete -> (RUN \"trig\")",
            &dream(
                "    object: 0..1 kind:resource\n",
                "      runner when count(actors) >= 1 -> NARRATE \"ran\", FORK \"trig\"\n      trig when count(actors) >= 1 -> NARRATE \"trig\"",
            ),
        ),
    );
    let base = clone_state(&s);
    let r = settle_line(&c, &dict, s, "0:0 @ ANA DREAM HEALTH", 1).unwrap();
    assert_eq!(r.rejected, None);
    assert_eq!(
        refusal(&r),
        Some("a second chain op in one completion — a station holds one window (§6.7)".to_string())
    );
    assert_eq!(kind_spine(&r), ["action", "complete", "wait"], "no branch opened");
    let ops: Vec<&str> = r.records[1].outcomes.iter().map(|o| o.op.as_str()).collect();
    assert_eq!(ops, ["NARRATE", "REFUSE"]);
    let replayed = fold(&r.records, &base, &dict, &c).unwrap();
    assert_eq!(snap(&replayed), snap(&r.state));
}
