//! Engine acceptance tests (84 tests) — the spec §6.4 READ ladder, REFUSE semantics
//! (§6.5), time ownership (§6.6), events (§6.6), fold determinism (§7).
//!
//! The mod tree groups the behaviors. Every test builds a FRESH Dictionary
//! (per-test isolation — this choice affects reserved-word collision
//! behavior and must not be shared). Error-message
//! patterns are copied CHARACTER FOR CHARACTER from the frozen regex
//! literals; state equality goes through the shared `snap()` (law 6); the
//! run adapter is `common::settle::settle_step` with the id-generator closure
//! `x{first_record_id}_{n}` replicated exactly (the REPLACE sentence
//! assertions regex-match the minted `#x\d+_1` ids, so the id sequence is
//! load-bearing).
//!
//! Reference-identity semantics (`r.state === state`) have no
//! Rust observable: `execute_step` consumes the State and hands the ORIGINAL
//! back untouched on a rejection/refusal, so `snap()` equality IS the
//! semantic content of those checks (noted at each site, never weakened).

#[path = "../common/mod.rs"]
mod common;

use common::harness::assert_err_msg;
use common::seed::{seed_content, seed_state};
use common::settle::{act, done, refusal, settle_step};
use common::snap::snap;

use chorus::content::content::{
    compile_content, CompiledContent, ContentRaw, DeriveAspect, ExpireTriggerDef, PrototypeDef,
    PrototypeDerive, RecipeDef, SlotDef, SlotTriggerDef, TriggerDef, VerbContent,
};
use chorus::core::clock::{format_duration, Clock};
use chorus::core::dict::{core_dictionary, Dictionary};
use chorus::core::format::format_sentence;
use chorus::core::parse::parse_sentence;
use chorus::core::r#ref::{name_ref, Ref};
use chorus::core::sentence::{Arg, Outcome, Sentence};
use chorus::core::types::{
    add_entity, clone_state, new_state, AspectVal, Aspects, AttrValue, Attrs, Entity, State,
};
use chorus::engine::engine::{apply_record, ActionRecord, ExecResult, Step};
use chorus::engine::fold::{fold, fold_up_to};
use chorus::engine::store::{load_log, save_log, Log};
use indexmap::IndexMap;

// ---------------------------------------------------------------- rig ------

/// The file-level fixture: a fresh
/// `dict`/`content` pair per test,
/// semantically identical because both are immutable.
fn rig() -> (Dictionary, CompiledContent) {
    let dict = core_dictionary();
    let content = compile_content(&seed_content(), &dict, &[]).unwrap();
    (dict, content)
}

fn setup() -> (Dictionary, CompiledContent, State) {
    let (dict, content) = rig();
    (dict, content, seed_state())
}

/// parse + settle_step with the exact
/// id-generator closure: `x{first_record_id}_{n}`, the counter
/// incrementing once per minted id.
fn run_with(
    content: &CompiledContent,
    dict: &Dictionary,
    state: State,
    src: &str,
    first_record_id: i64,
) -> ExecResult {
    let s = parse_sentence(src, dict).unwrap();
    let mut n: i64 = 0;
    let mut id_gen = || {
        n += 1;
        format!("x{first_record_id}_{n}") // unique across steps: keyed by record id
    };
    settle_step(content, dict, state, step_of(&s), first_record_id, Some(&mut id_gen)).unwrap()
}

/// first_record_id defaults to 1.
fn run(content: &CompiledContent, dict: &Dictionary, state: State, src: &str) -> ExecResult {
    run_with(content, dict, state, src, 1)
}

/// the step clock defaults to day 0 / halfhour 0 when the sentence omits one.
fn step_of(s: &Sentence) -> Step {
    Step {
        start_min: None,
        clock: Some(s.clock.unwrap_or(Clock { day: 0, halfhour: 0 })),
        clauses: s.clauses.clone(),
        t_real: None,
    }
}

/// Temp-log directory, unique per process and tag.
fn tmp_log_dir(tag: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("chorus-log-{}-{tag}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// `re` must match `result.rejected`.
fn assert_rejected(r: &ExecResult, pattern: &str) {
    let msg = r
        .rejected
        .as_deref()
        .unwrap_or_else(|| panic!("expected a rejected step matching /{pattern}/, got None"));
    let re = regex::Regex::new(pattern).unwrap();
    assert!(re.is_match(msg), "rejected {msg:?} does not match /{pattern}/");
}

/// `refusal(r)` matches `re` over the settle adapter's refusal probe.
fn assert_refusal(r: &ExecResult, pattern: &str) {
    let msg =
        refusal(r).unwrap_or_else(|| panic!("expected a refusal matching /{pattern}/, got None"));
    let re = regex::Regex::new(pattern).unwrap();
    assert!(re.is_match(&msg), "refusal {msg:?} does not match /{pattern}/");
}

fn kinds(records: &[ActionRecord]) -> Vec<&str> {
    records.iter().map(|x| x.kind.as_str()).collect()
}

fn ops(r: &ActionRecord) -> Vec<&str> {
    r.outcomes.iter().map(|o| o.op.as_str()).collect()
}

/// Plain entity fixture shape.
#[allow(clippy::too_many_arguments)]
fn plain_card(
    s: &mut State,
    id: &str,
    kind: &str,
    name: &str,
    aspects: Aspects,
    attrs: Attrs,
    stackable: bool,
) {
    add_entity(
        s,
        Entity {
            id: id.to_string(),
            kind: kind.to_string(),
            name: name.to_string(),
            aspects,
            attrs,
            born_action: None,
            dead_action: None,
            stackable,
        },
    )
    .unwrap();
}

fn num_aspect(k: &str, v: i64) -> Aspects {
    [(k.to_string(), AspectVal::Num(v))].into_iter().collect()
}

fn container_attrs(container: &str) -> Attrs {
    [("container".to_string(), AttrValue::Str(container.to_string()))].into_iter().collect()
}

/// A hand-built COMPLETE record like the regression literals in this file
/// (id, kind "complete", trigger None, t_game set, t_real/start_min
/// None, window "fixture", advanced_min 0).
fn fixture_complete(id: i64, t_game: &str, outcomes: Vec<Outcome>) -> ActionRecord {
    ActionRecord {
        id,
        kind: "complete".to_string(),
        trigger: None,
        t_game: t_game.to_string(),
        t_real: None,
        start_min: None,
        window: Some("fixture".to_string()),
        advanced_min: 0,
        actors: vec![],
        verb: String::new(),
        objects: vec![],
        instruments: vec![],
        at: None,
        about: None,
        to: None,
        outcomes,
        recipes: vec![],
        sentence: String::new(),
    }
}

fn outcome(op: &str, args: Vec<Arg>) -> Outcome {
    Outcome { op: op.to_string(), args, arms: None, fallback: None, call: None }
}

// ------------------------------------------------------------- ladder ------

/// READ ladder — first match wins, most specific
/// first (§6.4)
mod read_ladder {
    use super::*;

    #[test]
    fn solo_reader_with_knowledge_passes_the_gate_default_narrates() {
        let (dict, content, state) = setup();
        let r = run(&content, &dict, state, "0:28 @ ANA READ MAGICK_BOOK");
        assert_eq!(r.rejected, None);
        assert_eq!(act(&r).unwrap().recipes, ["read.default"]);
        assert_eq!(act(&r).unwrap().advanced_min, 60);
        assert_eq!(r.state.now, 14 * 60 + 60);
    }

    #[test]
    fn magick_gate_refuses_an_ignorant_reader_no_state_change_no_time_attempt_logged() {
        let (dict, content, state) = setup();
        let before = snap(&state);
        let r = run(&content, &dict, state, "0:28 @ BERTHA READ MAGICK_BOOK");
        assert_eq!(r.rejected, None);
        assert_eq!(r.records.len(), 1);
        assert_eq!(ops(done(&r).unwrap()), ["REFUSE"]);
        assert_eq!(act(&r).unwrap().advanced_min, 0);
        // `r.state === state` — same object, untouched; the
        // Rust observable is value equality through snap
        assert_eq!(snap(&r.state), before);
        assert!(done(&r).unwrap().sentence.contains("REFUSE(\"The letters swim like minnows.\")"));
    }

    #[test]
    fn parent_child_reading_fairy_tales_branches_to_bonding_memory() {
        let (dict, content, state) = setup();
        let r = run(&content, &dict, state, "0:28 @ ANA, BERTHA READ FAIRY_TALES");
        assert_eq!(act(&r).unwrap().recipes, ["read.parent_child"]);
        let spawn = &done(&r).unwrap().outcomes[0];
        assert_eq!(spawn.op, "SPAWN");
        let Arg::Ref { r#ref: Ref::Name { name, .. } } = &spawn.args[0] else {
            panic!("SPAWN's first arg is a name ref, got {:?}", spawn.args[0]);
        };
        assert_eq!(name, "BONDING_MEMORY");
        assert!(r.state.by_name.contains_key("BONDING_MEMORY"));
    }

    #[test]
    fn two_unrelated_readers_get_the_poorer_outcome_no_spawn() {
        let (dict, content, state) = setup();
        let r = run(&content, &dict, state, "0:28 @ ANA, MY_FOLLOWER READ FAIRY_TALES");
        assert_eq!(act(&r).unwrap().recipes, ["read.unrelated_pair"]);
        assert!(done(&r).unwrap().outcomes.iter().all(|o| o.op == "NARRATE"));
    }

    #[test]
    fn prototype_parameterized_spawn_resolves_into_a_concrete_named_card() {
        let (dict, content, state) = setup();
        let r = run(&content, &dict, state, "0:28 @ ANA READ THE_FORGETTING");
        assert_eq!(act(&r).unwrap().recipes, ["read.secret"]);
        let spawn = &done(&r).unwrap().outcomes[0];
        let Arg::Ref { r#ref: Ref::Name { name, .. } } = &spawn.args[0] else {
            panic!("SPAWN's first arg is a name ref, got {:?}", spawn.args[0]);
        };
        assert_eq!(name, "CLUE_THE_FORGETTING");
        let as_id = spawn.args.iter().find_map(|a| match a {
            Arg::AsId { id } => Some(id),
            _ => None,
        });
        assert!(as_id.is_some());
        let minted = &r.state.by_name.get("CLUE_THE_FORGETTING").unwrap()[0];
        assert_eq!(
            r.state.entities[minted].aspects.get("secret"),
            Some(&AspectVal::Num(2)) // 4 - 2
        );
        // v0.5: the completion row mints
        assert_eq!(r.state.entities[minted].born_action, Some(done(&r).unwrap().id));
    }
}

/// Slot discipline (§6.1)
mod slot_discipline {
    use super::*;

    #[test]
    fn cardinality_violations_are_rejected_before_anything_applies() {
        let (dict, content, state) = setup();
        let r = run(&content, &dict, state, "0:28 @ ANA, BERTHA, MY_FOLLOWER READ FAIRY_TALES");
        assert_rejected(&r, r"'actors' takes 1\.\.2");
        assert_eq!(r.records.len(), 0);
    }

    #[test]
    fn slot_discipline_undeclared_roles_are_rejected() {
        let (dict, content, state) = setup();
        let r = run(&content, &dict, state, "0:28 @ ANA READ FAIRY_TALES WITH CHALICE");
        assert_rejected(&r, r"does not accept 'with'");
        assert_eq!(r.records.len(), 0);
    }

    #[test]
    fn slot_discipline_one_card_cannot_fill_a_role_twice() {
        let (dict, content, state) = setup();
        let r = run(&content, &dict, state, "0:28 @ ANA, ANA READ FAIRY_TALES");
        assert_rejected(&r, r"already placed in 'actors'");
        assert_eq!(r.records.len(), 0);
    }
}

/// TALK with ABOUT (§4.3)
mod talk_with_about {
    use super::*;

    #[test]
    fn talking_about_a_secret_idea_spawns_a_plan() {
        let (dict, content, state) = setup();
        let r = run(
            &content,
            &dict,
            state,
            "0:28 @ ANA, BERTHA AT DINING_HALL TALK ABOUT CLUE_SECRET_HIDEOUT",
        );
        assert_eq!(act(&r).unwrap().recipes, ["talk.plan"]);
        assert!(r.state.by_name.contains_key("PLAN_INFILTRATION"));
    }
}

/// Multi-clause steps and time ownership (§6.6)
mod multi_clause_steps {
    use super::*;

    #[test]
    fn travel_then_read_durations_sum_recipes_listed_in_clause_order() {
        let (dict, content, state) = setup();
        let r = run(
            &content,
            &dict,
            state,
            "0:28 @ ANA TRAVEL_TO THE_STUDY; ANA AT THE_STUDY READ THE_FORGETTING",
        );
        assert_eq!(act(&r).unwrap().recipes, ["travel_to.default", "read.secret"]);
        assert_eq!(act(&r).unwrap().advanced_min, 120);
        assert_eq!(
            r.state.entities["ana"].attrs.get("container"),
            Some(&AttrValue::Str("study".to_string()))
        );
        assert_eq!(r.state.now, 14 * 60 + 120);
    }
}

/// USE_ON rite — aggregate gate, shown hint (§6.4)
mod use_on_rite {
    use super::*;

    #[test]
    fn one_lantern_starves_the_rite_refuse_state_untouched() {
        let (dict, content, mut state) = setup();
        state.now = 14 * 60;
        let before = snap(&state);
        let r = run(&content, &dict, state, "0:28 @ ANA AT HOUSE USE_ON RITE_OF_ASCENSION");
        assert_eq!(ops(done(&r).unwrap()), ["REFUSE"]);
        assert_eq!(r.state.now, 14 * 60);
        assert_eq!(snap(&r.state), before);
    }

    #[test]
    fn pooled_lantern_meets_demand_rite_consumed_instruments_destroyed_mark_spawned_into_place() {
        let (dict, content, state) = setup();
        let r = run(
            &content,
            &dict,
            state,
            "0:28 @ ANA, MY_FOLLOWER AT HOUSE USE_ON RITE_OF_ASCENSION WITH CHALICE",
        );
        assert_eq!(r.rejected, None);
        // v0.5: the completion row consumes
        assert_eq!(r.state.entities["rite"].dead_action, Some(done(&r).unwrap().id));
        assert_eq!(r.state.entities["chalice"].dead_action, Some(done(&r).unwrap().id));
        let mark = &r.state.by_name.get("MARK_OF_ASCENSION").unwrap()[0];
        assert_eq!(
            r.state.entities[mark].attrs.get("container"),
            Some(&AttrValue::Str("house".to_string()))
        );
        assert_eq!(act(&r).unwrap().advanced_min, 180);
    }
}

/// Events — authored triggers and expiries (§6.6, §6.6)
mod events {
    use super::*;

    #[test]
    fn crossing_hour_16_fires_dusk_falls_as_its_own_event_row() {
        let (dict, content, state) = setup();
        let r1 = run(&content, &dict, state, "0:28 @ ANA WAIT"); // crosses 15 only
        let mut kinds1 = kinds(&r1.records);
        if kinds1.first() == Some(&"wait") {
            kinds1.remove(0); // the typed clock bridges from now=0
        }
        assert_eq!(kinds1, ["action", "complete", "wait"]); // v0.5 turn shape
        let r2 = run(&content, &dict, r1.state, "0:30 @ ANA WAIT"); // crosses 16 → dusk
        let event = r2.records.iter().find(|x| x.kind == "event").expect("the dusk event row");
        assert_eq!(event.trigger.as_deref(), Some("dusk_falls"));
        assert_eq!(event.t_game, "0:32");
        assert!(r2.records.iter().any(|x| x.kind == "action" && !x.sentence.is_empty()));
        assert!(r2.state.by_name.contains_key("DREAD"));
    }

    #[test]
    fn expiry_crossing_destroys_the_card_via_an_expire_event() {
        let (dict, content, mut state) = setup();
        state.entities.get_mut("chalice").unwrap().attrs.insert(
            "expiresAt".to_string(),
            AttrValue::Num(16 * 60), // 0:32
        );
        let r = run(
            &content,
            &dict,
            state,
            "0:28 @ ANA WAIT; ANA WAIT; ANA WAIT", // three staggered windows, 14 → 17
        );
        // v0.5 order: each window completes at its end; at minute 960 the
        // window completes FIRST, then slot 32 strikes — dusk (authored)
        // before the chalice expiry, deterministic
        let mut ks = kinds(&r.records);
        if ks.first() == Some(&"wait") {
            ks.remove(0); // bridge
        }
        assert_eq!(ks, ["action", "complete", "complete", "event", "event", "complete", "wait"]);
        let events: Vec<&ActionRecord> = r.records.iter().filter(|x| x.kind == "event").collect();
        assert_eq!(events[0].trigger.as_deref(), Some("dusk_falls"));
        assert_eq!(events[1].trigger.as_deref(), Some("expire:CHALICE"));
        assert_eq!(r.state.entities["chalice"].dead_action, Some(events[1].id));
    }
}

/// Fold — state = fold(log), determinism, undo (§7)
mod fold_tests {
    use super::*;

    #[test]
    fn replaying_the_log_from_seed_reproduces_the_played_state_exactly() {
        let (dict, content, state) = setup();
        let mut state = state;
        let mut log = Log::new(seed_state());
        let steps = [
            "0:28 @ ANA READ THE_FORGETTING",
            "0:30 @ ANA, BERTHA READ FAIRY_TALES", // ends 0:32 → dusk event row
            "0:32 @ ANA, BERTHA AT DINING_HALL TALK ABOUT CLUE_SECRET_HIDEOUT",
            "0:34 @ ANA TRAVEL_TO THE_STUDY; ANA AT THE_STUDY READ MAGICK_BOOK",
            "0:40 @ ANA WAIT",
            "0:42 @ ANA WAIT",
        ];
        let mut id = 1;
        for step in steps {
            let r = run_with(&content, &dict, state, step, id);
            assert_eq!(r.rejected, None);
            log.append_all(r.records.clone());
            state = r.state;
            id += r.records.len() as i64;
        }
        let replayed = fold(&log.records, &seed_state(), &dict, &content).unwrap();
        assert_eq!(snap(&replayed), snap(&state));
    }

    #[test]
    fn refused_attempts_replay_as_no_ops() {
        let (dict, content, state) = setup();
        let before = snap(&state);
        let r = run(&content, &dict, state, "0:28 @ BERTHA READ MAGICK_BOOK");
        let replayed = fold(&r.records, &seed_state(), &dict, &content).unwrap();
        assert_eq!(snap(&replayed), before);
    }

    #[test]
    fn jsonl_persistence_round_trips_the_log() {
        let (dict, content, state) = setup();
        let r = run(&content, &dict, state, "0:28 @ ANA READ THE_FORGETTING");
        let dir = tmp_log_dir("jsonl-roundtrip");
        let path = dir.join("log.jsonl");
        save_log(&path, &r.records).unwrap();
        let loaded = load_log(&path).unwrap();
        let replayed = fold(&loaded, &seed_state(), &dict, &content).unwrap();
        assert_eq!(snap(&replayed), snap(&r.state));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn undo_re_fold_without_the_tail() {
        let (dict, content, state) = setup();
        let r1 = run(&content, &dict, state, "0:28 @ ANA READ THE_FORGETTING");
        let snapshot_after_one = snap(&r1.state);
        let r2 = run_with(
            &content,
            &dict,
            r1.state,
            "0:32 @ ANA READ FAIRY_TALES",
            r1.records.len() as i64 + 1,
        );
        let mut all = r1.records.clone();
        all.extend(r2.records.clone());
        // drop turn 2
        let undone =
            fold_up_to(&all, &seed_state(), &dict, &content, r1.records.len() as i64 + 1).unwrap();
        assert_eq!(snap(&undone), snapshot_after_one);
    }
}

/// Dictionary is the law (§5.1)
mod dictionary_is_the_law {
    use super::*;

    #[test]
    fn names_of_dead_entities_are_reusable_unique_among_the_living() {
        let (dict, content, state) = setup();
        let mut r = run(
            &content,
            &dict,
            state,
            "0:28 @ ANA, MY_FOLLOWER AT HOUSE USE_ON RITE_OF_ASCENSION WITH CHALICE",
        );
        assert_eq!(r.rejected, None);
        assert!(r.state.entities["chalice"].dead_action.is_some());
        assert!(!r.state.by_name.contains_key("CHALICE"));
        // a new CHALICE can be minted after the old one died
        let rec = fixture_complete(
            99,
            "0:40",
            vec![outcome(
                "SPAWN",
                vec![
                    Arg::Ref { r#ref: name_ref("CHALICE", Some("item".to_string())) },
                    Arg::AsId { id: "new_chalice".to_string() },
                ],
            )],
        );
        apply_record(&rec, &mut r.state, &dict, &content).unwrap();
        assert_eq!(r.state.by_name.get("CHALICE").unwrap()[0], "new_chalice");
    }
}

/// Behavior regressions
mod behavior_regressions {
    use super::*;

    #[test]
    fn set_stores_the_given_value_positionally_ref_key_value() {
        let (dict, content, mut state) = setup();
        let rec = fixture_complete(
            1,
            "0:28",
            vec![outcome(
                "SET",
                vec![
                    Arg::Ref { r#ref: name_ref("CHALICE", None) },
                    Arg::Key { v: "state.sealed".to_string() },
                    Arg::Bool { v: true },
                ],
            )],
        );
        apply_record(&rec, &mut state, &dict, &content).unwrap();
        assert_eq!(
            state.entities["chalice"].attrs.get("state.sealed"),
            Some(&AttrValue::Bool(true))
        );
    }

    #[test]
    fn relation_key_symmetric_edges_share_one_canonical_key_directed_keep_order() {
        let (dict, content, mut state) = setup();
        let relate = |outcome_src: &str, id: i64, state: &mut State| {
            let s = parse_sentence(&format!("0:0 @ ANA WAIT -> {outcome_src}"), &dict).unwrap();
            let rec = fixture_complete(id, "0:0", s.outcomes);
            apply_record(&rec, state, &dict, &content).unwrap();
        };
        relate("RELATE(BERTHA, ANA, parent_child)", 1, &mut state);
        relate("RELATE(BERTHA, ANA, parent_of)", 2, &mut state);
        let keys: Vec<&String> = state.relations.keys().collect();
        let parent_child: Vec<&str> =
            keys.iter().filter(|k| k.contains("parent_child")).map(|k| k.as_str()).collect();
        assert_eq!(parent_child, ["ana|bertha|parent_child"]); // one canonical edge
        let keys: Vec<&String> = state.relations.keys().collect();
        let parent_of: Vec<&str> =
            keys.iter().filter(|k| k.contains("parent_of")).map(|k| k.as_str()).collect();
        assert_eq!(parent_of, ["bertha|ana|parent_of"]); // directed, as stored
        relate("UNRELATE(ANA, BERTHA, parent_child)", 3, &mut state);
        assert_eq!(
            state.relations.values().find(|r| r.type_ == "parent_child").unwrap().dead_action,
            Some(3)
        );
    }

    #[test]
    fn fold_clock_a_log_ending_past_an_event_keeps_the_action_end_time() {
        let (dict, content, mut state) = setup();
        state
            .entities
            .get_mut("chalice")
            .unwrap()
            .attrs
            .insert("expiresAt".to_string(), AttrValue::Num(15 * 60));
        let r = run(
            &content,
            &dict,
            state,
            "0:28 @ ANA WAIT; ANA WAIT; ANA WAIT", // 14→17, events at 15/16
        );
        assert!(r.records.iter().filter(|x| x.kind == "event").count() >= 2);
        let replayed = fold(&r.records, &seed_state(), &dict, &content).unwrap();
        assert_eq!(replayed.now, r.state.now); // events must not drag the clock back
        assert_eq!(replayed.now, 17 * 60);
    }

    #[test]
    fn role_sugar_is_concretized_in_the_log_replay_matches_play_exactly() {
        let (dict, content, state) = setup();
        let r = run(&content, &dict, state, "0:28 @ ANA, BERTHA TRAVEL_TO VILLA; MY_FOLLOWER WAIT");
        assert_eq!(r.rejected, None);
        // the recorded MOVE must name ANA and BERTHA, not ACTORS
        let move_outcome =
            done(&r).unwrap().outcomes.iter().find(|o| o.op == "MOVE").expect("the MOVE outcome");
        let mut ids: Vec<&str> = move_outcome
            .args
            .iter()
            .filter_map(|a| match a {
                Arg::Ref { r#ref: Ref::Id { id } } => Some(id.as_str()),
                _ => None,
            })
            .collect();
        ids.sort();
        assert_eq!(ids, ["ana", "bertha"]);
        let replayed = fold(&r.records, &seed_state(), &dict, &content).unwrap();
        assert_eq!(
            replayed.entities["follower"].attrs.get("container"),
            Some(&AttrValue::Str("house".to_string())) // not dragged along
        );
        assert_eq!(
            replayed.entities["ana"].attrs.get("container"),
            Some(&AttrValue::Str("villa".to_string()))
        );
    }

    #[test]
    fn steps_cannot_rewind_the_clock() {
        let (dict, content, mut state) = setup();
        state.now = 10 * 60;
        let r = run(&content, &dict, state, "0:6 @ ANA WAIT");
        assert_rejected(&r, r"before state\.now");
        assert_eq!(r.records.len(), 0);
    }

    #[test]
    fn application_failures_refuse_the_window_instead_of_throwing() {
        let (dict, content, state) = setup();
        let r = run(&content, &dict, state, "0:28 @ ANA, BERTHA READ FAIRY_TALES");
        assert_eq!(r.rejected, None);
        let r2 = run_with(
            &content,
            &dict,
            r.state,
            "0:32 @ ANA, BERTHA READ FAIRY_TALES", // duplicate mint name
            r.records.len() as i64 + 1,
        );
        assert_eq!(r2.rejected, None);
        assert_refusal(&r2, r"already in use");
        // `r2.state !== r.state` — a fresh, distinct value;
        // the semantic content is what follows: the second mint never landed
        assert_eq!(r2.state.by_name.get("BONDING_MEMORY").unwrap().len(), 1);
    }
}

/// Exists(...) recipes — world-scope conditions
/// (§6.3, CS extantreqs). Each test gets a fresh dictionary with PROBE
/// registered.
mod exists_recipes {
    use super::*;

    fn probe_dict() -> Dictionary {
        let mut d = core_dictionary();
        d.register_verb("PROBE").unwrap();
        d
    }

    /// probe_content
    fn probe_content(dict: &Dictionary, recipes: Vec<RecipeDef>) -> CompiledContent {
        let mut verbs = IndexMap::new();
        verbs.insert(
            "PROBE".to_string(),
            VerbContent {
                slots: vec![SlotDef {
                    role: "actors".to_string(),
                    min: 1,
                    max: Some(2),
                    query: Some("kind:character".to_string()),
                    shown: None,
                }],
                duration: 30,
                recipes,
            },
        );
        compile_content(&ContentRaw { verbs, ..ContentRaw::default() }, dict, &[]).unwrap()
    }

    /// Gate — two recipes, order-dependent: the
    /// gate branch needs a hunter in the world AND no ward anywhere; the
    /// default is the calm world
    fn gate(dict: &Dictionary) -> CompiledContent {
        probe_content(
            dict,
            vec![
                RecipeDef {
                    id: Some("probe.hunted".to_string()),
                    when: Some(
                        "exists(kind:character aspect:hunter>=1) AND NOT exists(kind:item aspect:ward>=1)"
                            .to_string(),
                    ),
                    shown: None,
                    hint: None,
                    max: None,
                    aspects: Vec::new(),
                    out: vec!["NARRATE(\"A hunter walks the world.\")".to_string()],
                },
                RecipeDef {
                    id: None,
                    when: None,
                    shown: None,
                    hint: None,
                    max: None,
                    aspects: Vec::new(),
                    out: vec!["NARRATE(\"The world is calm.\")".to_string()],
                },
            ],
        )
    }

    #[test]
    fn scope_proof_an_unbound_hunter_in_a_far_place_selects_the_exists_and_not_exists_branch() {
        let mut state = seed_state();
        let pd = probe_dict();
        let c = gate(&pd);
        // prowls the villa: bound to no role of this step and in another place
        // than ANA at the house — world scope is the whole point
        plain_card(
            &mut state,
            "hunter",
            "character",
            "HUNTER",
            num_aspect("hunter", 1),
            container_attrs("villa"),
            false,
        );
        let r = run_with(&c, &pd, state, "0:28 @ ANA PROBE", 1);
        assert_eq!(r.rejected, None);
        assert_eq!(act(&r).unwrap().recipes, ["probe.hunted"]);
        assert!(done(&r).unwrap().sentence.contains("A hunter walks the world."));
    }

    #[test]
    fn a_ward_anywhere_flips_the_branch_not_exists_fails() {
        let mut state = seed_state();
        let pd = probe_dict();
        plain_card(
            &mut state,
            "hunter",
            "character",
            "HUNTER",
            num_aspect("hunter", 1),
            container_attrs("villa"),
            false,
        );
        plain_card(
            &mut state,
            "ward",
            "item",
            "IRON_WARD",
            num_aspect("ward", 2),
            container_attrs("greenhouse"),
            false,
        );
        let r = run_with(&gate(&pd), &pd, state, "0:28 @ ANA PROBE", 1);
        assert_eq!(act(&r).unwrap().recipes, ["probe.default"]);
    }

    #[test]
    fn destroyed_cards_stop_existing_the_gate_un_latches_after_the_hunter_dies() {
        let mut state = seed_state();
        let pd = probe_dict();
        plain_card(
            &mut state,
            "hunter",
            "character",
            "HUNTER",
            num_aspect("hunter", 1),
            container_attrs("villa"),
            false,
        );
        // DESTROY shape: stamp + release the name (§5.1)
        state.entities.get_mut("hunter").unwrap().dead_action = Some(99);
        state.by_name.remove("HUNTER");
        let r = run_with(&gate(&pd), &pd, state, "0:28 @ ANA PROBE", 1);
        assert_eq!(act(&r).unwrap().recipes, ["probe.default"]);
    }

    #[test]
    fn name_form_exists_sees_a_named_card_across_the_map() {
        let mut state = seed_state();
        let pd = probe_dict();
        plain_card(
            &mut state,
            "sealed_door",
            "place",
            "THE_SEALED_DOOR",
            Aspects::new(),
            container_attrs("villa"),
            false,
        );
        let c = probe_content(
            &pd,
            vec![
                RecipeDef {
                    id: Some("probe.door".to_string()),
                    when: Some("exists(THE_SEALED_DOOR)".to_string()),
                    shown: None,
                    hint: None,
                    max: None,
                    aspects: Vec::new(),
                    out: vec!["NARRATE(\"The door still stands.\")".to_string()],
                },
                RecipeDef {
                    id: None,
                    when: None,
                    shown: None,
                    hint: None,
                    max: None,
                    aspects: Vec::new(),
                    out: vec!["NARRATE(\"No door.\")".to_string()],
                },
            ],
        );
        let r = run_with(&c, &pd, state, "0:28 @ ANA PROBE", 1);
        assert_eq!(act(&r).unwrap().recipes, ["probe.door"]);
    }

    #[test]
    fn relations_are_untouched_by_exists_evaluation_rel_gates_still_select_as_before() {
        let state = seed_state();
        let pd = probe_dict();
        let c = probe_content(
            &pd,
            vec![
                RecipeDef {
                    id: Some("probe.bonded".to_string()),
                    when: Some(
                        "exists(aspect:lantern>=1) AND rel(parent_child, actors)".to_string(),
                    ),
                    shown: None,
                    hint: None,
                    max: None,
                    aspects: Vec::new(),
                    out: vec!["NARRATE(\"Kin in a haunted world.\")".to_string()],
                },
                RecipeDef {
                    id: None,
                    when: None,
                    shown: None,
                    hint: None,
                    max: None,
                    aspects: Vec::new(),
                    out: vec!["NARRATE(\"No kinship.\")".to_string()],
                },
            ],
        );
        // ANA is BERTHA's parent (seed edge)
        let r = run_with(&c, &pd, state, "0:28 @ ANA, BERTHA PROBE", 1);
        assert_eq!(act(&r).unwrap().recipes, ["probe.bonded"]);
        // no parent_child edge — a fresh seed for the second probe
        let r2 = run_with(&c, &pd, seed_state(), "0:30 @ ANA, MY_FOLLOWER PROBE", 2);
        assert_eq!(act(&r2).unwrap().recipes, ["probe.default"]);
    }
}

/// Progress ticks — the processing read-bar (§6.7)
mod progress_ticks {
    use super::*;

    #[test]
    fn ticks_record_in_emission_order_touch_no_state_cost_no_extra_time() {
        let (dict, content, state) = setup();
        let r = run(
            &content,
            &dict,
            state,
            "0:28 @ ANA, MY_FOLLOWER AT HOUSE USE_ON RITE_OF_ASCENSION WITH CHALICE",
        );
        assert_eq!(r.rejected, None);
        // v0.5: top-level PROGRESS ticks ride the BEGIN row; the real
        // outcomes land on the COMPLETE row
        assert_eq!(ops(act(&r).unwrap()), ["PROGRESS", "PROGRESS"]);
        assert_eq!(ops(done(&r).unwrap()), ["DESTROY", "DESTROY", "SPAWN"]);
        assert_eq!(act(&r).unwrap().advanced_min, 180); // the verb's duration only
        assert!(act(&r)
            .unwrap()
            .sentence
            .contains("PROGRESS(1h, \"The circle begins to hum against your palms.\")"));
        // the log alone reproduces the played state exactly (ticks are
        // journal-only at fold)
        let replayed = fold(&r.records, &seed_state(), &dict, &content).unwrap();
        assert_eq!(snap(&replayed), snap(&r.state));
    }

    #[test]
    fn malformed_progress_arity_is_an_application_failure_not_a_silent_no_op() {
        let (dict, content, mut state) = setup();
        let rec = fixture_complete(
            1,
            "0:28",
            vec![outcome("PROGRESS", vec![Arg::Str { v: "no duration".to_string() }])],
        );
        assert_err_msg(apply_record(&rec, &mut state, &dict, &content), r"PROGRESS needs");
    }

    #[test]
    fn narrate_keeps_exactly_one_string_extras_are_refused_at_parse() {
        let (dict, _content, _state) = setup();
        assert_err_msg(
            parse_sentence("0:0 @ ANA WAIT -> NARRATE(\"one\", \"two\")", &dict),
            r"exactly one string",
        );
    }
}

/// Clock & duration literals
mod clock_and_duration_literals {
    use super::*;

    /// A sub-halfhour action leaves dead air; the
    /// next step resumes on the tick and the gap fires (§6.6)
    #[test]
    fn a_sub_halfhour_action_leaves_dead_air_the_next_step_resumes_on_the_tick() {
        let (dict, _c, _s) = setup();
        // a 20m verb pushes the world clock mid-tick; the timetable speaks at
        // 14:30 (slot 29)
        let mut verbs = IndexMap::new();
        verbs.insert(
            "WAIT".to_string(),
            VerbContent {
                slots: vec![SlotDef {
                    role: "actors".to_string(),
                    min: 1,
                    max: Some(1),
                    query: Some("kind:character".to_string()),
                    shown: None,
                }],
                duration: 20,
                recipes: vec![RecipeDef {
                    id: None,
                    when: None,
                    shown: None,
                    hint: None,
                    max: None,
                    aspects: Vec::new(),
                    out: vec!["NARRATE(\"{actors} waits a moment.\")".to_string()],
                }],
            },
        );
        let content20 = compile_content(
            &ContentRaw {
                verbs,
                triggers: Some(vec![TriggerDef::Slot(SlotTriggerDef {
                    id: "the_chime".to_string(),
                    slot: 29,
                    once: None,
                    out: vec![
                        "NARRATE(\"The halfhour chime sings through the frames.\")".to_string()
                    ],
                })]),
                ..ContentRaw::default()
            },
            &dict,
            &[],
        )
        .unwrap();
        let s = seed_state();
        let r1 = run_with(&content20, &dict, s, "0:28 @ ANA WAIT", 1); // 14:00 → 14:20
        assert_eq!(r1.rejected, None);
        assert_eq!(r1.state.now, 28 * 30 + 20);
        // the next step resumes on the halfhour tick (14:30); the chime
        // crosses inside the quantized gap, so it fires even though no window
        // spans 14:30
        let r2 = run_with(&content20, &dict, r1.state, "0:29 @ ANA WAIT", 1);
        assert_eq!(r2.rejected, None);
        // v0.5: the chime crosses inside the advance that leads the step,
        // before the BEGIN — the quantized dead air is a logged wait row now
        assert_eq!(kinds(&r2.records), ["event", "wait", "action", "complete", "wait"]);
        assert_eq!(r2.records[0].trigger.as_deref(), Some("the_chime"));
        assert_eq!(r2.records[0].t_game, "0:29");
    }

    #[test]
    fn durations_spill_into_the_next_day() {
        let (dict, content, state) = setup();
        let r = run(&content, &dict, state, "0:44 @ ANA WORK AT HOUSE"); // 2h → day 1, 0:00
        assert_eq!(r.state.now, 22 * 60 + 120);
        assert_eq!(act(&r).unwrap().t_game, "0:44");
    }

    #[test]
    fn format_duration_renders_zero_as_0m() {
        assert_eq!(format_duration(0).unwrap(), "0m");
    }
}

/// Record shape (§7)
mod record_shape {
    use super::*;

    #[test]
    fn records_carry_sorted_actors_provenance_canonical_sentence() {
        let (dict, content, state) = setup();
        let r = run(
            &content,
            &dict,
            state,
            "0:28 @ BERTHA, ANA AT DINING_HALL TALK ABOUT CLUE_SECRET_HIDEOUT",
        );
        let rec = act(&r).unwrap();
        let actors: Vec<&str> = rec
            .actors
            .iter()
            .filter_map(|a| match a {
                Ref::Name { name, .. } => Some(name.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(actors, ["ANA", "BERTHA"]);
        assert_eq!(rec.verb, "TALK");
        assert_eq!(rec.t_game, "0:28");
        assert!(rec.sentence.starts_with("0:28 @ ANA, BERTHA AT DINING_HALL TALK"));
    }
}

/// Empty base state fold (fresh world, no content spawns)
mod empty_base_state_fold {
    use super::*;

    #[test]
    fn fold_of_an_empty_log_returns_the_base_state() {
        let (dict, content, _state) = setup();
        assert_eq!(fold(&[], &new_state(0), &dict, &content).unwrap().entities.len(), 0);
    }
}

/// SET deltas and aspect keys (§4.4)
mod set_deltas_and_aspect_keys {
    use super::*;

    /// The section-level rig dict registers TUNE
    fn tune_dict() -> Dictionary {
        let mut d = core_dictionary();
        d.register_verb("TUNE").unwrap();
        d
    }

    /// counter_content
    fn counter_content(dict: &Dictionary, recipes: Vec<RecipeDef>) -> CompiledContent {
        let mut verbs = IndexMap::new();
        verbs.insert(
            "TUNE".to_string(),
            VerbContent {
                slots: vec![SlotDef {
                    role: "actors".to_string(),
                    min: 1,
                    max: Some(2),
                    query: Some("kind:character".to_string()),
                    shown: None,
                }],
                duration: 30,
                recipes,
            },
        );
        compile_content(&ContentRaw { verbs, ..ContentRaw::default() }, dict, &[]).unwrap()
    }

    fn recipe(out: &[&str]) -> RecipeDef {
        RecipeDef {
            id: None,
            when: None,
            shown: None,
            hint: None,
            max: None,
            aspects: Vec::new(),
            out: out.iter().map(|s| s.to_string()).collect(),
        }
    }

    /// the round-trip probe: re-formatting a parsed sentence reproduces it
    /// exactly (`format_sentence(parse_sentence(s, dict), dict) == s`).
    fn assert_round_trips(sentence: &str, dict: &Dictionary) {
        let parsed = parse_sentence(sentence, dict).unwrap();
        let formatted = format_sentence(&parsed, Some(dict)).unwrap();
        assert_eq!(formatted, sentence, "sentence must round-trip: {sentence:?}");
    }

    #[test]
    fn a_signed_value_adds_to_the_attr_missing_is_zero_and_the_log_carries_the_absolute() {
        let (dict, content, state) = setup();
        let td = tune_dict();
        let c = counter_content(&td, vec![recipe(&["SET(ANA, funds, +2)"])]);
        let r = run_with(&c, &td, state, "0:20 @ ANA TUNE", 1);
        assert_eq!(r.rejected, None);
        assert_eq!(
            r.state.entities["ana"].attrs.get("funds"),
            Some(&AttrValue::Num(2)) // missing entry counted as 0
        );
        let set = &done(&r).unwrap().outcomes[0];
        assert_eq!(set.op, "SET");
        assert_eq!(
            serde_json::to_value(&set.args[2]).unwrap(),
            serde_json::json!({ "t": "int", "v": 2 }) // the record holds the concrete absolute
        );
        assert!(done(&r).unwrap().sentence.contains("-> SET(ANA, funds, 2)")); // name ref kept, value absolutized
                                                                               // the concrete sentence round-trips
        assert_round_trips(&done(&r).unwrap().sentence.clone(), &td);
        // the next delta applies to the new value
        let r2 = run_with(&c, &td, r.state, "0:21 @ ANA TUNE", 2);
        assert_eq!(r2.state.entities["ana"].attrs.get("funds"), Some(&AttrValue::Num(4)));
        let _ = (&dict, &content);
    }

    #[test]
    fn decrements_may_go_negative_no_clamping() {
        let (dict, content, state) = setup();
        let td = tune_dict();
        let c = counter_content(&td, vec![recipe(&["SET(ACTORS, funds, -3)"])]);
        let r = run_with(&c, &td, state, "0:20 @ ANA TUNE", 1);
        assert_eq!(r.rejected, None);
        assert_eq!(r.state.entities["ana"].attrs.get("funds"), Some(&AttrValue::Num(-3)));
        assert!(done(&r).unwrap().sentence.contains("SET(#ana, funds, -3)"));
        let _ = (&dict, &content);
    }

    #[test]
    fn aspect_delta_increments_the_aspects_map_an_absolute_aspect_set_writes_it_too() {
        let (dict, content, state) = setup();
        let td = tune_dict();
        let c = counter_content(&td, vec![recipe(&["SET(ACTORS, aspect:dread, +1)"])]);
        let mut r = run_with(&c, &td, state, "0:20 @ ANA TUNE", 1);
        assert_eq!(r.rejected, None);
        assert_eq!(r.state.entities["ana"].aspects.get("dread"), Some(&AspectVal::Num(1)));
        assert!(done(&r).unwrap().sentence.contains("SET(#ana, aspect:dread, 1)"));
        // absolute form through the fold path (concrete outcomes replay as-is)
        let s =
            parse_sentence("0:0 @ ANA WAIT -> SET(#ana, aspect:language.latin, 2)", &dict).unwrap();
        let rec = fixture_complete(2, "0:0", s.outcomes);
        apply_record(&rec, &mut r.state, &dict, &content).unwrap();
        let Some(AspectVal::Sub(lang)) = r.state.entities["ana"].aspects.get("language") else {
            panic!("expected a sub-map aspect");
        };
        assert_eq!(lang.get("latin"), Some(&2));
        assert_eq!(lang.len(), 1);
    }

    #[test]
    fn absolute_values_stay_absolute_repeated_casts_write_the_same_literal() {
        let (dict, content, state) = setup();
        let td = tune_dict();
        let c = counter_content(&td, vec![recipe(&["SET(ACTORS, funds, 5)"])]);
        let r = run_with(&c, &td, state, "0:20 @ ANA TUNE", 1);
        assert_eq!(r.state.entities["ana"].attrs.get("funds"), Some(&AttrValue::Num(5)));
        let r2 = run_with(&c, &td, r.state, "0:21 @ ANA TUNE", 2);
        assert_eq!(
            r2.state.entities["ana"].attrs.get("funds"),
            Some(&AttrValue::Num(5)) // not 5+5
        );
        assert!(done(&r2).unwrap().sentence.contains("SET(#ana, funds, 5)")); // role concretized (§9.2), value literal
        let _ = (&dict, &content);
    }

    #[test]
    fn a_role_arg_absolute_set_in_a_multi_clause_step_logs_one_concrete_row_per_card() {
        let (dict, content, state) = setup();
        let td = tune_dict();
        let c = compile_content(
            &ContentRaw {
                verbs: {
                    let mut v = IndexMap::new();
                    v.insert(
                        "TUNE".to_string(),
                        VerbContent {
                            slots: vec![
                                SlotDef {
                                    role: "actors".to_string(),
                                    min: 1,
                                    max: Some(2),
                                    query: Some("kind:character".to_string()),
                                    shown: None,
                                },
                                SlotDef {
                                    role: "object".to_string(),
                                    min: 1,
                                    max: Some(1),
                                    query: Some("kind:text".to_string()),
                                    shown: None,
                                },
                            ],
                            duration: 30,
                            recipes: vec![recipe(&["SET(OBJECT, searched, 1)"])],
                        },
                    );
                    v
                },
                ..ContentRaw::default()
            },
            &td,
            &[],
        )
        .unwrap();
        let r =
            run_with(&c, &td, state, "0:20 @ ANA TUNE THE_FORGETTING; BERTHA TUNE FAIRY_TALES", 1);
        assert_eq!(r.rejected, None);
        // each clause's OBJECT becomes its own id row — the role itself never
        // reaches the log, so the fold needs no per-clause attribution
        let sets: Vec<&String> =
            r.records.iter().filter(|x| x.kind == "complete").map(|x| &x.sentence).collect();
        assert!(sets[0].contains("SET(#forgetting, searched, 1)"));
        assert!(sets[1].contains("SET(#fairy, searched, 1)"));
        // §7: the records alone reproduce the played state
        assert_eq!(snap(&fold(&r.records, &seed_state(), &td, &c).unwrap()), snap(&r.state));
        assert_eq!(r.state.entities["forgetting"].attrs.get("searched"), Some(&AttrValue::Num(1)));
        assert_eq!(r.state.entities["fairy"].attrs.get("searched"), Some(&AttrValue::Num(1)));
        let _ = (&dict, &content);
    }

    #[test]
    fn a_delta_over_a_role_ref_logs_one_concrete_set_per_card_each_with_its_own_result() {
        let (dict, content, mut state) = setup();
        let td = tune_dict();
        state.entities.get_mut("ana").unwrap().aspects.insert(
            "dread".to_string(),
            AspectVal::Num(2), // the two bases differ
        );
        let c = counter_content(
            &td,
            vec![RecipeDef {
                id: Some("tune.gain".to_string()),
                when: Some("count(actors) = 2".to_string()),
                shown: None,
                hint: None,
                max: None,
                aspects: Vec::new(),
                out: vec!["SET(ACTORS, aspect:dread, +1)".to_string()],
            }],
        );
        let r = run_with(&c, &td, state, "0:20 @ ANA, BERTHA TUNE", 1);
        assert_eq!(r.rejected, None);
        assert_eq!(done(&r).unwrap().outcomes.iter().filter(|o| o.op == "SET").count(), 2);
        assert!(done(&r)
            .unwrap()
            .sentence
            .contains("SET(#ana, aspect:dread, 3), SET(#bertha, aspect:dread, 1)"));
        assert_eq!(r.state.entities["ana"].aspects.get("dread"), Some(&AspectVal::Num(3)));
        assert_eq!(r.state.entities["bertha"].aspects.get("dread"), Some(&AspectVal::Num(1)));
        let _ = (&dict, &content);
    }

    #[test]
    fn fold_of_a_jsonl_saved_delta_log_reproduces_the_played_state() {
        let (dict, content, mut state) = setup();
        let td = tune_dict();
        let c = counter_content(
            &td,
            vec![recipe(&["SET(ACTORS, funds, +2)", "SET(ACTORS, aspect:dread, +1)"])],
        );
        state.entities.get_mut("bertha").unwrap().attrs.insert(
            "funds".to_string(),
            AttrValue::Num(10), // the cards carry different bases
        );
        state
            .entities
            .get_mut("bertha")
            .unwrap()
            .aspects
            .insert("dread".to_string(), AspectVal::Num(7));
        let mut id = 1;
        let mut all: Vec<ActionRecord> = Vec::new();
        for step in ["0:20 @ ANA, BERTHA TUNE", "0:21 @ ANA TUNE", "0:22 @ BERTHA TUNE"] {
            let r = run_with(&c, &td, state, step, id);
            assert_eq!(r.rejected, None);
            all.extend(r.records.clone());
            state = r.state;
            id += r.records.len() as i64;
        }
        assert_eq!(state.entities["ana"].attrs.get("funds"), Some(&AttrValue::Num(4)));
        assert_eq!(state.entities["ana"].aspects.get("dread"), Some(&AspectVal::Num(2)));
        assert_eq!(state.entities["bertha"].attrs.get("funds"), Some(&AttrValue::Num(14)));
        assert_eq!(state.entities["bertha"].aspects.get("dread"), Some(&AspectVal::Num(9)));
        let dir = tmp_log_dir("delta-jsonl");
        let path = dir.join("log.jsonl");
        save_log(&path, &all).unwrap();
        let replayed = fold(&load_log(&path).unwrap(), &seed_state(), &td, &c).unwrap();
        assert_eq!(snap(&replayed), snap(&state));
        let _ = std::fs::remove_dir_all(&dir);
        let _ = (&dict, &content);
    }

    #[test]
    fn refuse_after_a_delta_set_in_the_same_step_state_untouched_no_time_attempt_logged() {
        let (dict, content, state) = setup();
        let td = tune_dict();
        let before = snap(&state);
        let c = counter_content(
            &td,
            vec![recipe(&[
                "SET(ACTORS, aspect:dread, +1)",
                "SET(ACTORS, funds, +2)",
                "REFUSE(\"too costly\")",
            ])],
        );
        let r = run_with(&c, &td, state, "0:20 @ ANA TUNE", 1);
        assert_eq!(r.rejected, None);
        assert_eq!(ops(done(&r).unwrap()), ["SET", "SET", "REFUSE"]);
        assert_eq!(act(&r).unwrap().advanced_min, 0);
        // `r.state === state` — the deltas ran on the discarded
        // draft; snap equality is the Rust observable
        assert_eq!(snap(&r.state), before);
        assert_eq!(
            snap(&fold(&r.records, &seed_state(), &td, &c).unwrap()),
            before // refused record folds as a no-op
        );
        let _ = (&dict, &content);
    }

    #[test]
    fn a_delta_on_a_non_numeric_entry_refuses_the_window_nothing_applied() {
        let (dict, content, state) = setup();
        let td = tune_dict();
        let c = counter_content(&td, vec![recipe(&["SET(ACTORS, container, +1)"])]);
        // container is a string attr
        let r = run_with(&c, &td, state, "0:20 @ ANA TUNE", 1);
        assert_eq!(r.rejected, None);
        assert_refusal(&r, r"non-numeric");
        assert_eq!(r.state.entities["ana"].attrs.get("funds"), None); // nothing applied
        let _ = (&dict, &content);
    }

    #[test]
    fn a_delta_on_an_aspect_sub_map_refuses_the_window_the_map_survives() {
        let (dict, content, mut state) = setup();
        let td = tune_dict();
        state.entities.get_mut("ana").unwrap().aspects.insert(
            "language".to_string(),
            AspectVal::Sub(
                [("french".to_string(), 2)].into_iter().collect(), // a sub-map, not a number
            ),
        );
        let c = counter_content(&td, vec![recipe(&["SET(ACTORS, aspect:language, +1)"])]);
        let r = run_with(&c, &td, state, "0:20 @ ANA TUNE", 1);
        assert_eq!(r.rejected, None);
        assert_refusal(&r, r"non-numeric");
        assert_eq!(
            r.state.entities["ana"].aspects.get("language"),
            Some(&AspectVal::Sub([("french".to_string(), 2)].into_iter().collect())) // survives
        );
        let _ = (&dict, &content);
    }

    #[test]
    fn a_quoted_key_stays_quoted_in_the_concrete_record() {
        let (dict, content, state) = setup();
        let td = tune_dict();
        let c = counter_content(&td, vec![recipe(&["SET(ACTORS, \"about\", +5)"])]);
        // 'about' is a reserved role word
        let r = run_with(&c, &td, state, "0:20 @ ANA TUNE", 1);
        assert_eq!(r.rejected, None);
        assert_eq!(r.state.entities["ana"].attrs.get("about"), Some(&AttrValue::Num(5)));
        assert!(done(&r).unwrap().sentence.contains("SET(#ana, \"about\", 5)"));
        // the record must round-trip: unquoted it would re-parse as the about ROLE
        assert_round_trips(&done(&r).unwrap().sentence.clone(), &td);
        let _ = (&dict, &content);
    }
}

/// REPLACE — decay into a successor (§4.4, §4.4)
mod replace_decay {
    use super::*;

    fn decay_dict() -> Dictionary {
        let mut d = core_dictionary();
        d.register_verb("DECAY").unwrap();
        d
    }

    /// decay_content
    fn decay_content(
        dict: &Dictionary,
        out: &str,
        prototypes: IndexMap<String, PrototypeDef>,
    ) -> CompiledContent {
        let mut verbs = IndexMap::new();
        verbs.insert(
            "DECAY".to_string(),
            VerbContent {
                slots: vec![
                    SlotDef {
                        role: "actors".to_string(),
                        min: 1,
                        max: Some(1),
                        query: Some("kind:character".to_string()),
                        shown: None,
                    },
                    SlotDef {
                        role: "object".to_string(),
                        min: 1,
                        max: Some(1),
                        query: None,
                        shown: None,
                    },
                ],
                duration: 0,
                recipes: vec![RecipeDef {
                    id: None,
                    when: None,
                    shown: None,
                    hint: None,
                    max: None,
                    aspects: Vec::new(),
                    out: vec![out.to_string()],
                }],
            },
        );
        compile_content(
            &ContentRaw { verbs, prototypes: Some(prototypes), ..ContentRaw::default() },
            dict,
            &[],
        )
        .unwrap()
    }

    fn proto(kind: &str) -> PrototypeDef {
        PrototypeDef { kind: kind.to_string(), ..PrototypeDef::default() }
    }

    #[test]
    fn a_place_contained_card_decays_into_the_prototype_in_the_same_place() {
        let state = seed_state(); // CHALICE sits in HOUSE
        let dd = decay_dict();
        let mut protos = IndexMap::new();
        protos.insert(
            "ASHES".to_string(),
            PrototypeDef {
                kind: "item".to_string(),
                aspects: Some(num_aspect("spent", 1)),
                ..PrototypeDef::default()
            },
        );
        let c = decay_content(&dd, "REPLACE OBJECT WITH ASHES", protos);
        let r = run_with(&c, &dd, state, "0:20 @ ANA DECAY CHALICE", 1);
        assert_eq!(r.rejected, None);
        // zero new log surface: the record carries the concrete SPAWN+DESTROY pair (§4.4)
        assert_eq!(ops(done(&r).unwrap()), ["SPAWN", "DESTROY"]);
        let re = regex::Regex::new(r"-> SPAWN ASHES INTO #house AS #x\d+_1 ").unwrap();
        assert!(
            re.is_match(&done(&r).unwrap().sentence),
            "sentence {:?} does not match the SPAWN spelling",
            done(&r).unwrap().sentence
        );
        assert!(done(&r).unwrap().sentence.contains("DESTROY(#chalice)"));
        let ash_id = &r.state.by_name.get("ASHES").unwrap()[0];
        let ash = &r.state.entities[ash_id];
        assert_eq!(ash.kind, "item");
        assert_eq!(ash.aspects, num_aspect("spent", 1));
        assert_eq!(
            ash.attrs.get("container"),
            Some(&AttrValue::Str("house".to_string())) // inherits the dying card's place
        );
        assert_eq!(ash.born_action, Some(done(&r).unwrap().id));
        assert_eq!(r.state.entities["chalice"].dead_action, Some(done(&r).unwrap().id));
        assert!(!r.state.by_name.contains_key("CHALICE"));
        // the fold replays the pair exactly
        assert_eq!(snap(&fold(&r.records, &seed_state(), &dd, &c).unwrap()), snap(&r.state));
    }

    #[test]
    fn a_carried_card_stays_carried_by_the_same_character() {
        let (dict, content, mut state) = setup();
        let dd = decay_dict();
        state.entities.get_mut("chalice").unwrap().attrs.insert(
            "container".to_string(),
            AttrValue::Str("ana".to_string()), // now carried, not shelved
        );
        let mut protos = IndexMap::new();
        protos.insert("ASHES".to_string(), proto("item"));
        let c = decay_content(&dd, "REPLACE OBJECT WITH ASHES", protos);
        let r = run_with(&c, &dd, state, "0:20 @ ANA DECAY CHALICE", 1);
        assert_eq!(r.rejected, None);
        let ash_id = &r.state.by_name.get("ASHES").unwrap()[0];
        assert_eq!(
            r.state.entities[ash_id].attrs.get("container"),
            Some(&AttrValue::Str("ana".to_string()))
        );
        let _ = (&dict, &content);
    }

    #[test]
    fn a_zone_tagged_card_passes_its_zone_to_the_successor_no_container_card() {
        let (dict, content) = rig();
        let dd = decay_dict();
        let mut base = seed_state();
        base.entities.get_mut("chalice").unwrap().attrs.shift_remove("container");
        base.entities
            .get_mut("chalice")
            .unwrap()
            .attrs
            .insert("zone".to_string(), AttrValue::Str("NIGHTJAR_FLYER".to_string()));
        let mut protos = IndexMap::new();
        protos.insert("ASHES".to_string(), proto("item"));
        let c = decay_content(&dd, "REPLACE OBJECT WITH ASHES", protos);
        let r = run_with(&c, &dd, base, "0:20 @ ANA DECAY CHALICE", 1);
        assert_eq!(r.rejected, None);
        let ash_id = &r.state.by_name.get("ASHES").unwrap()[0];
        let ash = &r.state.entities[ash_id];
        assert_eq!(
            ash.attrs.get("zone"),
            Some(&AttrValue::Str("NIGHTJAR_FLYER".to_string())) // the grouping has no card: the tag passes on
        );
        assert_eq!(ash.attrs.get("container"), None);
        // the zone rides the concrete attrs pair, so the fold replays it (§4.4)
        let played = clone_state(&r.state);
        let fold_base = {
            let mut b = seed_state();
            b.entities.get_mut("chalice").unwrap().attrs.shift_remove("container");
            b.entities
                .get_mut("chalice")
                .unwrap()
                .attrs
                .insert("zone".to_string(), AttrValue::Str("NIGHTJAR_FLYER".to_string()));
            b
        };
        assert_eq!(snap(&fold(&r.records, &fold_base, &dd, &c).unwrap()), snap(&played));
        let _ = (&dict, &content);
    }

    #[test]
    fn replace_of_an_unbound_role_ref_rejects_the_step_nothing_logged() {
        let (dict, content, state) = setup();
        let dd = decay_dict();
        let c = compile_content(
            &ContentRaw {
                verbs: {
                    let mut v = IndexMap::new();
                    v.insert(
                        "DECAY".to_string(),
                        VerbContent {
                            slots: vec![
                                SlotDef {
                                    role: "actors".to_string(),
                                    min: 1,
                                    max: Some(1),
                                    query: Some("kind:character".to_string()),
                                    shown: None,
                                },
                                SlotDef {
                                    role: "object".to_string(),
                                    min: 0,
                                    max: Some(1), // optional, and left unbound
                                    query: None,
                                    shown: None,
                                },
                            ],
                            duration: 0,
                            recipes: vec![RecipeDef {
                                id: None,
                                when: None,
                                shown: None,
                                hint: None,
                                max: None,
                                aspects: Vec::new(),
                                out: vec!["REPLACE OBJECT WITH ASHES".to_string()],
                            }],
                        },
                    );
                    v
                },
                prototypes: Some([("ASHES".to_string(), proto("item"))].into_iter().collect()),
                ..ContentRaw::default()
            },
            &dd,
            &[],
        )
        .unwrap();
        let r = run_with(&c, &dd, state, "0:20 @ ANA DECAY", 1);
        assert_eq!(r.rejected, None);
        assert_refusal(&r, r"unresolvable ref");
        assert_eq!(r.state.entities["chalice"].dead_action, None); // nothing applied
        let _ = (&dict, &content);
    }

    #[test]
    fn the_old_card_soft_destroys_its_name_is_released_and_reusable() {
        let (dict, content, state) = setup();
        let dd = decay_dict();
        let mut protos = IndexMap::new();
        protos.insert("ASHES".to_string(), proto("item"));
        let c = decay_content(&dd, "REPLACE OBJECT WITH ASHES", protos);
        let r = run_with(&c, &dd, state, "0:20 @ ANA DECAY CHALICE", 1);
        assert_eq!(r.rejected, None);
        assert!(r.state.entities["chalice"].dead_action.is_some());
        assert!(!r.state.by_name.contains_key("CHALICE"));
        let _ = (&dict, &content);
    }

    #[test]
    fn a_plain_kind_name_ref_mints_without_a_prototype_everything_clause_less_spawn_accepts() {
        let (dict, content, state) = setup();
        let dd = decay_dict();
        let c = decay_content(&dd, "REPLACE OBJECT WITH item:CRUMBS", IndexMap::new());
        let r = run_with(&c, &dd, state, "0:20 @ ANA DECAY CHALICE", 1);
        assert_eq!(r.rejected, None);
        let crumbs_id = &r.state.by_name.get("CRUMBS").unwrap()[0];
        let crumbs = &r.state.entities[crumbs_id];
        assert_eq!(crumbs.kind, "item");
        assert_eq!(crumbs.attrs.get("container"), Some(&AttrValue::Str("house".to_string())));
        let _ = (&dict, &content);
    }

    #[test]
    fn collision_with_a_living_entity_the_dying_card_included_rejects_the_whole_step() {
        let (dict, content, state) = setup();
        let dd = decay_dict();
        let mut protos = IndexMap::new();
        protos.insert("CHALICE".to_string(), proto("item"));
        let c = decay_content(&dd, "REPLACE OBJECT WITH CHALICE", protos);
        let r = run_with(&c, &dd, state, "0:20 @ ANA DECAY CHALICE", 1); // CHALICE is still alive at spawn time
        assert_eq!(r.rejected, None);
        assert_refusal(&r, r"already in use");
        assert_eq!(r.state.entities["chalice"].dead_action, None); // nothing applied
                                                                   // the ORIGINAL state object still holds the name — the
                                                                   // refusal discards everything, so the returned state carries it too
        assert!(r.state.by_name.contains_key("CHALICE"));
        let _ = (&dict, &content);
    }

    #[test]
    fn authored_replace_round_trips_the_paren_less_form_is_the_only_form() {
        let (dict, _content, _state) = setup();
        let dd = decay_dict();
        let name_form =
            parse_sentence("0:0 @ ANA DECAY CHALICE -> REPLACE CHALICE WITH ASHES", &dd).unwrap();
        assert_eq!(
            format_sentence(&name_form, Some(&dd)).unwrap(),
            "0:0 @ ANA DECAY CHALICE -> REPLACE CHALICE WITH ASHES"
        );
        let role_form =
            parse_sentence("0:0 @ ANA DECAY CHALICE -> REPLACE OBJECT WITH ASHES", &dd).unwrap();
        assert_eq!(
            format_sentence(&role_form, Some(&dd)).unwrap(),
            "0:0 @ ANA DECAY CHALICE -> REPLACE OBJECT WITH ASHES"
        );
        assert_err_msg(
            parse_sentence("0:0 @ ANA WAIT -> REPLACE(CHALICE, ASHES)", &dd),
            r"paren-less",
        );
        assert_err_msg(
            parse_sentence("0:0 @ ANA WAIT -> REPLACE CHALICE", &dd),
            r"REPLACE expects WITH",
        );
        let _ = dict;
    }
}

/// Authored expiry triggers — `on expire of`
/// (§6.6, §6.6). influence decay: G → E → C → (no trigger: built-in destroy)
mod authored_expiry_triggers {
    use super::*;

    fn moth_dict() -> Dictionary {
        core_dictionary()
    }

    /// wait_verb
    fn wait_verb() -> VerbContent {
        VerbContent {
            slots: vec![SlotDef {
                role: "actors".to_string(),
                min: 1,
                max: Some(1),
                query: Some("kind:character".to_string()),
                shown: None,
            }],
            duration: 60,
            recipes: vec![RecipeDef {
                id: None,
                when: None,
                shown: None,
                hint: None,
                max: None,
                aspects: Vec::new(),
                out: vec!["NARRATE(\"{actors} wait.\")".to_string()],
            }],
        }
    }

    /// moth_content
    fn moth_content(dict: &Dictionary) -> CompiledContent {
        compile_content(
            &ContentRaw {
                verbs: [("WAIT".to_string(), wait_verb())].into_iter().collect(),
                triggers: Some(vec![
                    TriggerDef::Expire(ExpireTriggerDef {
                        id: "decay_moth_g".to_string(),
                        on_expire_of: "INFLUENCE_MOTH_G".to_string(),
                        out: vec![
                            "REPLACE INFLUENCE_MOTH_G WITH INFLUENCE_MOTH_E".to_string(),
                            "SET(INFLUENCE_MOTH_E, expires, 1h)".to_string(),
                        ],
                    }),
                    TriggerDef::Expire(ExpireTriggerDef {
                        id: "decay_moth_e".to_string(),
                        on_expire_of: "INFLUENCE_MOTH_E".to_string(),
                        out: vec![
                            "REPLACE INFLUENCE_MOTH_E WITH INFLUENCE_MOTH_C".to_string(),
                            "SET(INFLUENCE_MOTH_C, expires, 1h)".to_string(),
                        ],
                    }),
                ]),
                prototypes: Some(
                    [
                        (
                            "INFLUENCE_MOTH_E".to_string(),
                            PrototypeDef {
                                kind: "idea".to_string(),
                                aspects: Some(num_aspect("influence", 2)),
                                ..PrototypeDef::default()
                            },
                        ),
                        (
                            "INFLUENCE_MOTH_C".to_string(),
                            PrototypeDef {
                                kind: "idea".to_string(),
                                aspects: Some(num_aspect("influence", 1)),
                                ..PrototypeDef::default()
                            },
                        ),
                    ]
                    .into_iter()
                    .collect(),
                ),
                ..ContentRaw::default()
            },
            dict,
            &[],
        )
        .unwrap()
    }

    /// moth_base
    fn moth_base() -> State {
        let mut s = seed_state();
        let mut attrs = container_attrs("house");
        attrs.insert("expiresAt".to_string(), AttrValue::Num(16 * 60));
        plain_card(
            &mut s,
            "moth_g",
            "idea",
            "INFLUENCE_MOTH_G",
            num_aspect("influence", 3),
            attrs,
            false,
        );
        s
    }

    /// run_moth
    fn run_moth(dict: &Dictionary, state: State, src: &str, first_record_id: i64) -> ExecResult {
        let s = parse_sentence(src, dict).unwrap();
        let mut n: i64 = 0;
        let mut id_gen = || {
            n += 1;
            format!("x{first_record_id}_{n}")
        };
        let content = moth_content(dict);
        settle_step(&content, dict, state, step_of(&s), first_record_id, Some(&mut id_gen)).unwrap()
    }

    #[test]
    fn a_decay_chain_runs_across_successive_waits_then_default_destroys() {
        let dict = moth_dict();
        let mut state = moth_base();
        let mut triggers: Vec<String> = Vec::new();
        let mut id = 1;
        let mut all_records: Vec<ActionRecord> = Vec::new();
        let mut e_id: Option<String> = None;
        let mut c_id: Option<String> = None;
        for step in ["0:30 @ ANA WAIT", "0:32 @ ANA WAIT", "0:34 @ ANA WAIT"] {
            let r = run_moth(&dict, state, step, id);
            assert_eq!(r.rejected, None);
            all_records.extend(r.records.clone());
            triggers.extend(
                r.records.iter().filter(|x| x.kind == "event").map(|x| x.trigger.clone().unwrap()),
            );
            state = r.state;
            if e_id.is_none() {
                e_id = state.by_name.get("INFLUENCE_MOTH_E").and_then(|v| v.first().cloned());
            }
            if c_id.is_none() {
                c_id = state.by_name.get("INFLUENCE_MOTH_C").and_then(|v| v.first().cloned());
            }
            id += r.records.len() as i64;
        }
        // G decayed into E, E into C, then C expired with no trigger →
        // built-in destroy
        assert_eq!(triggers, ["decay_moth_g", "decay_moth_e", "expire:INFLUENCE_MOTH_C"]);
        // v0.5: every stamp lands on the completion row that did the work —
        // derive the ids from the log instead of hardcoding them
        let stamp = |trigger: &str| -> i64 {
            all_records
                .iter()
                .find(|x| x.kind == "event" && x.trigger.as_deref() == Some(trigger))
                .unwrap()
                .id
        };
        assert_eq!(state.entities["moth_g"].dead_action, Some(stamp("decay_moth_g")));
        let e = &state.entities[e_id.as_deref().unwrap()];
        assert_eq!(e.aspects, num_aspect("influence", 2)); // plain prototype mint (no derive)
        assert_eq!(
            e.attrs.get("container"),
            Some(&AttrValue::Str("house".to_string())) // inherited container
        );
        assert_eq!(e.dead_action, Some(stamp("decay_moth_e")));
        assert_eq!(
            state.entities[c_id.as_deref().unwrap()].dead_action,
            Some(stamp("expire:INFLUENCE_MOTH_C"))
        );
        assert!(!state.by_name.contains_key("INFLUENCE_MOTH_G"));
        assert!(!state.by_name.contains_key("INFLUENCE_MOTH_E"));
        assert!(!state.by_name.contains_key("INFLUENCE_MOTH_C")); // destroyed: released, no successor
    }

    #[test]
    fn an_authored_time_trigger_and_an_authored_expiry_fire_in_the_same_advance_triggers_first() {
        let (dict, _content, mut state) = setup();
        let c = compile_content(
            &ContentRaw {
                verbs: [("WAIT".to_string(), wait_verb())].into_iter().collect(),
                triggers: Some(vec![
                    TriggerDef::Slot(SlotTriggerDef {
                        id: "dusk_falls".to_string(),
                        slot: 32,
                        once: None,
                        out: vec!["SPAWN DREAD".to_string()],
                    }),
                    TriggerDef::Expire(ExpireTriggerDef {
                        id: "decay_chalice".to_string(),
                        on_expire_of: "CHALICE".to_string(),
                        out: vec!["REPLACE CHALICE WITH RUSTY_CHALICE".to_string()],
                    }),
                ]),
                prototypes: Some(
                    [
                        ("DREAD".to_string(), proto_state()),
                        ("RUSTY_CHALICE".to_string(), proto("item")),
                    ]
                    .into_iter()
                    .collect(),
                ),
                ..ContentRaw::default()
            },
            &dict,
            &[],
        )
        .unwrap();
        state.entities.get_mut("chalice").unwrap().attrs.insert(
            "expiresAt".to_string(),
            AttrValue::Num(16 * 60), // same crossing as dusk (slot 32)
        );
        let r = run_with(&c, &dict, state, "0:28 @ ANA WAIT; ANA WAIT; ANA WAIT", 1);
        assert_eq!(r.rejected, None);
        let events: Vec<&ActionRecord> = r.records.iter().filter(|x| x.kind == "event").collect();
        assert_eq!(
            events.iter().map(|x| x.trigger.as_deref()).collect::<Vec<_>>(),
            [Some("dusk_falls"), Some("decay_chalice")] // triggers first
        );
        assert!(events[1].sentence.contains("EVENT decay_chalice"));
        assert_eq!(r.state.entities["chalice"].dead_action, Some(events[1].id));
        let rusty = &r.state.by_name.get("RUSTY_CHALICE").unwrap()[0];
        assert_eq!(
            r.state.entities[rusty].attrs.get("container"),
            Some(&AttrValue::Str("house".to_string()))
        );
        assert!(r.state.by_name.contains_key("DREAD"));
    }

    fn proto_state() -> PrototypeDef {
        PrototypeDef { kind: "state".to_string(), ..PrototypeDef::default() }
    }

    fn proto(kind: &str) -> PrototypeDef {
        PrototypeDef { kind: kind.to_string(), ..PrototypeDef::default() }
    }

    #[test]
    fn a_colliding_authored_expiry_rejects_the_whole_step_nothing_logged() {
        let (dict, _content, mut state) = setup();
        let c = compile_content(
            &ContentRaw {
                verbs: [("WAIT".to_string(), wait_verb())].into_iter().collect(),
                triggers: Some(vec![TriggerDef::Expire(ExpireTriggerDef {
                    id: "decay_chalice".to_string(),
                    on_expire_of: "CHALICE".to_string(),
                    out: vec!["REPLACE CHALICE WITH CHALICE".to_string()],
                })]),
                prototypes: Some([("CHALICE".to_string(), proto("item"))].into_iter().collect()),
                ..ContentRaw::default()
            },
            &dict,
            &[],
        )
        .unwrap();
        state
            .entities
            .get_mut("chalice")
            .unwrap()
            .attrs
            .insert("expiresAt".to_string(), AttrValue::Num(15 * 60));
        let before = snap(&state);
        let r = run_with(&c, &dict, state, "0:28 @ ANA WAIT; ANA WAIT", 1);
        assert_rejected(&r, r"already in use");
        assert_eq!(r.records.len(), 0);
        assert_eq!(snap(&r.state), before);
    }

    #[test]
    fn play_and_fold_agree_on_fired_for_every_event_row_full_state_fold_equality_incl_jsonl() {
        let dict = moth_dict();
        let mut state = moth_base();
        let mut all: Vec<ActionRecord> = Vec::new();
        let mut id = 1;
        for step in ["0:30 @ ANA WAIT", "0:32 @ ANA WAIT", "0:34 @ ANA WAIT"] {
            let r = run_moth(&dict, state, step, id);
            assert_eq!(r.rejected, None);
            all.extend(r.records.clone());
            state = r.state;
            id += r.records.len() as i64;
        }
        let dir = tmp_log_dir("moth-jsonl");
        let path = dir.join("log.jsonl");
        save_log(&path, &all).unwrap();
        let replayed =
            fold(&load_log(&path).unwrap(), &moth_base(), &dict, &moth_content(&dict)).unwrap();
        assert_eq!(snap(&replayed), snap(&state)); // snap includes fired (§6.6)
                                                   // one fired entry per EVENT row's trigger — authored ids and
                                                   // expire:<NAME> alike
        let mut fired: Vec<&String> = replayed.fired.iter().collect();
        fired.sort();
        assert_eq!(
            fired.iter().map(|s| s.as_str()).collect::<Vec<_>>(),
            ["decay_moth_e", "decay_moth_g", "expire:INFLUENCE_MOTH_C"]
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}

/// Trigger outcome guards (§6.7, §6.6)
mod trigger_outcome_guards {
    use super::*;

    #[test]
    fn progress_is_a_load_time_error_in_both_trigger_forms() {
        let (dict, _c, _s) = setup();
        let raw = |trigger: TriggerDef| ContentRaw {
            verbs: IndexMap::new(),
            triggers: Some(vec![trigger]),
            ..ContentRaw::default()
        };
        assert_err_msg(
            compile_content(
                &raw(TriggerDef::Slot(SlotTriggerDef {
                    id: "t".to_string(),
                    slot: 1,
                    once: None,
                    out: vec!["PROGRESS(30m, \"tick\")".to_string()],
                })),
                &dict,
                &[],
            ),
            r"PROGRESS belongs",
        );
        assert_err_msg(
            compile_content(
                &raw(TriggerDef::Expire(ExpireTriggerDef {
                    id: "t".to_string(),
                    on_expire_of: "X".to_string(),
                    out: vec!["PROGRESS(30m, \"tick\")".to_string()],
                })),
                &dict,
                &[],
            ),
            r"PROGRESS belongs",
        );
    }

    #[test]
    fn refuse_is_a_load_time_error_in_both_trigger_forms_events_have_no_step_to_refuse() {
        let (dict, _c, _s) = setup();
        let raw = |trigger: TriggerDef| ContentRaw {
            verbs: IndexMap::new(),
            triggers: Some(vec![trigger]),
            ..ContentRaw::default()
        };
        assert_err_msg(
            compile_content(
                &raw(TriggerDef::Slot(SlotTriggerDef {
                    id: "t".to_string(),
                    slot: 1,
                    once: None,
                    out: vec!["REFUSE(\"no\")".to_string()],
                })),
                &dict,
                &[],
            ),
            r"REFUSE belongs",
        );
        assert_err_msg(
            compile_content(
                &raw(TriggerDef::Expire(ExpireTriggerDef {
                    id: "t".to_string(),
                    on_expire_of: "X".to_string(),
                    out: vec!["REFUSE(\"no\")".to_string()],
                })),
                &dict,
                &[],
            ),
            r"REFUSE belongs",
        );
    }
}

/// SET expires — every §5.1 duration form
/// materializes expiresAt (§4.2, §4.2)
mod set_expires_duration_forms {
    use super::*;

    fn tune_dict() -> Dictionary {
        let mut d = core_dictionary();
        d.register_verb("TUNE").unwrap(); // not a core verb — the rig from the §4.4 describes
        d
    }

    /// tune_content
    fn tune_content(dict: &Dictionary, out: &str) -> CompiledContent {
        let mut verbs = IndexMap::new();
        verbs.insert(
            "TUNE".to_string(),
            VerbContent {
                slots: vec![SlotDef {
                    role: "actors".to_string(),
                    min: 1,
                    max: Some(1),
                    query: Some("kind:character".to_string()),
                    shown: None,
                }],
                duration: 0,
                recipes: vec![RecipeDef {
                    id: None,
                    when: None,
                    shown: None,
                    hint: None,
                    max: None,
                    aspects: Vec::new(),
                    out: vec![out.to_string()],
                }],
            },
        );
        compile_content(&ContentRaw { verbs, ..ContentRaw::default() }, dict, &[]).unwrap()
    }

    /// 0:20 = absolute minute 600
    fn at(minutes: i64) -> i64 {
        20 * 30 + minutes
    }

    #[test]
    fn quoted_durations_materialize_expires_at() {
        let td = tune_dict();
        for (lit, minutes) in [
            ("\"2halfhour\"", 60),
            ("\"90m\"", 90),
            ("\"1h30m\"", 90),
            ("\"2h\"", 120),
            ("\"1d\"", 1440),
            ("\"0m\"", 0),
        ] {
            let state = seed_state();
            let r = run_with(
                &tune_content(&td, &format!("SET(CHALICE, expires, {lit})")),
                &td,
                state,
                "0:20 @ ANA TUNE",
                1,
            );
            assert_eq!(r.rejected, None, "lit {lit}");
            assert_eq!(
                r.state.entities["chalice"].attrs.get("expiresAt"),
                Some(&AttrValue::Num(at(minutes))),
                "lit {lit}"
            );
            // the log keeps the authored quoted string and round-trips (§9.2)
            assert!(
                done(&r).unwrap().sentence.contains(&format!("-> SET(CHALICE, expires, {lit})")),
                "lit {lit}: {}",
                done(&r).unwrap().sentence
            );
        }
    }

    #[test]
    fn bare_duration_literals_are_unchanged_the_lexer_pre_converts_them() {
        let td = tune_dict();
        let r = run_with(
            &tune_content(&td, "SET(CHALICE, expires, 2halfhour)"),
            &td,
            seed_state(),
            "0:20 @ ANA TUNE",
            1,
        );
        assert_eq!(r.rejected, None);
        assert_eq!(
            r.state.entities["chalice"].attrs.get("expiresAt"),
            Some(&AttrValue::Num(at(60)))
        );
        // the log stores the pre-converted minutes; canonical output prefers
        // the shorter unit (§5.1)
        assert!(done(&r).unwrap().sentence.contains("-> SET(CHALICE, expires, 1h)"));
        // quoted and bare materialize the same engine fact — identical
        // expiresAt. The authored `expires` attr itself keeps its authored
        // form: minutes for bare, the string for quoted
        let rq = run_with(
            &tune_content(&td, "SET(CHALICE, expires, \"2halfhour\")"),
            &td,
            seed_state(),
            "0:20 @ ANA TUNE",
            1,
        );
        assert_eq!(rq.rejected, None);
        assert_eq!(
            rq.state.entities["chalice"].attrs.get("expiresAt"),
            r.state.entities["chalice"].attrs.get("expiresAt")
        );
        assert_eq!(
            rq.state.entities["chalice"].attrs.get("expires"),
            Some(&AttrValue::Str("2halfhour".to_string()))
        );
        assert_eq!(r.state.entities["chalice"].attrs.get("expires"), Some(&AttrValue::Num(60)));
    }

    #[test]
    fn a_bad_unit_refuses_the_window_nothing_logged_as_state() {
        let state = seed_state();
        let td = tune_dict();
        let r = run_with(
            &tune_content(&td, "SET(CHALICE, expires, \"2fortnights\")"),
            &td,
            state,
            "0:20 @ ANA TUNE",
            1,
        );
        assert_eq!(r.rejected, None);
        assert_refusal(&r, r"bad duration '2fortnights'");
        // the ORIGINAL state's chalice — untouched by the refusal
        assert_eq!(r.state.entities["chalice"].attrs.get("expiresAt"), None);
    }

    #[test]
    fn end_to_end_a_quoted_expiry_fires_its_authored_on_expire_trigger_at_the_next_crossing() {
        let state = seed_state();
        let td = tune_dict();
        let c = compile_content(
            &ContentRaw {
                verbs: [
                    (
                        "TUNE".to_string(),
                        VerbContent {
                            slots: vec![SlotDef {
                                role: "actors".to_string(),
                                min: 1,
                                max: Some(1),
                                query: Some("kind:character".to_string()),
                                shown: None,
                            }],
                            duration: 0,
                            recipes: vec![RecipeDef {
                                id: None,
                                when: None,
                                shown: None,
                                hint: None,
                                max: None,
                                aspects: Vec::new(),
                                out: vec!["SET(CHALICE, expires, \"2halfhour\")".to_string()],
                            }],
                        },
                    ),
                    (
                        "WAIT".to_string(),
                        VerbContent {
                            slots: vec![SlotDef {
                                role: "actors".to_string(),
                                min: 1,
                                max: Some(1),
                                query: Some("kind:character".to_string()),
                                shown: None,
                            }],
                            duration: 60,
                            recipes: vec![RecipeDef {
                                id: None,
                                when: None,
                                shown: None,
                                hint: None,
                                max: None,
                                aspects: Vec::new(),
                                out: vec!["NARRATE(\"{actors} wait.\")".to_string()],
                            }],
                        },
                    ),
                ]
                .into_iter()
                .collect(),
                triggers: Some(vec![TriggerDef::Expire(ExpireTriggerDef {
                    id: "melt_chalice".to_string(),
                    on_expire_of: "CHALICE".to_string(),
                    out: vec!["REPLACE CHALICE WITH RUSTY_CHALICE".to_string()],
                })]),
                prototypes: Some(
                    [(
                        "RUSTY_CHALICE".to_string(),
                        PrototypeDef { kind: "item".to_string(), ..PrototypeDef::default() },
                    )]
                    .into_iter()
                    .collect(),
                ),
                ..ContentRaw::default()
            },
            &td,
            &[],
        )
        .unwrap();
        let r1 = run_with(&c, &td, state, "0:28 @ ANA TUNE", 1); // expiresAt = 28*30 + 60
        assert_eq!(r1.rejected, None);
        // set this pass: resolves at the NEXT advance (§6.6 snapshot)
        assert!(!r1.records.iter().any(|x| x.kind == "event"));
        let r2 = run_with(&c, &td, r1.state, "0:30 @ ANA WAIT", 1 + r1.records.len() as i64); // crosses slot 31 (0:31)
        assert_eq!(r2.rejected, None);
        let event = r2.records.iter().find(|x| x.kind == "event").expect("the melt_chalice event");
        assert_eq!(event.trigger.as_deref(), Some("melt_chalice"));
        assert!(event.sentence.contains("EVENT melt_chalice"));
        assert_eq!(r2.state.entities["chalice"].dead_action, Some(event.id));
        let rusty = &r2.state.by_name.get("RUSTY_CHALICE").unwrap()[0];
        assert_eq!(
            r2.state.entities[rusty].attrs.get("container"),
            Some(&AttrValue::Str("house".to_string()))
        );
        // play and fold agree through the quoted path too (§6.6)
        let mut all = r1.records.clone();
        all.extend(r2.records.clone());
        assert_eq!(snap(&fold(&all, &seed_state(), &td, &c).unwrap()), snap(&r2.state));
    }
}

/// The SPAWN `expires <duration>` modifier (§4.4, §4.2, §9.2; the E12
/// ledger entry): a mint arms its own decay at BIRTH with the SET-expires
/// pair — one shared write, so the same forms, the same boundary behavior,
/// the same loud rejection — anchored to the mint clock (`ctx.now`: the
/// window's START clock inside a completion, §6.7). Each SPAWN arms its own
/// fresh copy, so a pooled name arms per instance where a following
/// `SET DREAD, …` would hit the OLDEST pool member (§4.2). The modifier is
/// authored-sugar only: the pair rides the concrete row's `attrs=`
/// materialization, so the fold replays byte-identically with no new log
/// surface.
mod spawn_expires_modifier {
    use super::*;
    use chorus::content::script::compile_script;
    use chorus::core::clock::parse_clock;
    use chorus::engine::engine::{execute_step, ExecOptions};

    /// The rig: ANA at HOUSE, a stackable DREAD pool prototype, a plain
    /// non-pool prototype, and the caller's DREAM verb body.
    fn rig(recipes: &str, extra_entities: &str, triggers: Option<&str>) -> String {
        let triggers = match triggers {
            Some(t) => format!("\ntriggers:\n{t}"),
            None => String::new(),
        };
        format!(
            "
script: Spawn Expires Rig
player: ANA
start: 0:0
seed: 0

entities:
  ANA is character at HOUSE
  HOUSE is place
{extra_entities}{triggers}
prototypes:
  DREAD is state stackable
  CHALICE is item
verbs:
  DREAM:
    actors: 1 kind:character
    duration: 0m
    recipes:
{recipes}
"
        )
    }

    fn compile(dict: &Dictionary, text: &str) -> (CompiledContent, State) {
        let compiled = compile_script(text, dict).unwrap();
        (compiled.content, compiled.state)
    }

    /// A pure advance (bare clock) with the same id shape.
    fn advance(
        content: &CompiledContent,
        dict: &Dictionary,
        state: State,
        clock: &str,
        first: i64,
    ) -> ExecResult {
        let mut n: i64 = 0;
        let mut id_gen = || {
            n += 1;
            format!("x{first}_{n}")
        };
        execute_step(
            content,
            dict,
            state,
            Step {
                start_min: None,
                clock: Some(parse_clock(clock).unwrap()),
                clauses: vec![],
                t_real: None,
            },
            ExecOptions { first_record_id: first, id_gen: Some(&mut id_gen) },
        )
        .unwrap()
    }

    fn pool(state: &State) -> Vec<String> {
        state.by_name["DREAD"].clone()
    }

    /// Each SPAWN carries its own modifier, so each fresh copy arms with its
    /// OWN expiresAt — the oldest-name trap is gone (a `SET DREAD, …` after
    /// the same list would arm only the pool head, §4.2) — and a plain
    /// unmodified spawn arms not at all. The concrete row's `attrs=` pair
    /// carries the materialization (§9.2).
    #[test]
    fn each_spawn_arms_its_own_fresh_pooled_copy() {
        let dict = core_dictionary();
        let (c, s) = compile(
            &dict,
            &rig(
                "      d when count(actors) >= 1 -> SPAWN DREAD expires 3h, SPAWN DREAD expires 1h, SPAWN DREAD, NARRATE \"done\"",
                "",
                None,
            ),
        );
        let base = clone_state(&s);
        let r = run_with(&c, &dict, s, "0:0 @ ANA DREAM", 1);
        assert_eq!(r.rejected, None);
        let p = pool(&r.state);
        assert_eq!(p.len(), 3, "three fresh copies");
        // birth order: 3h, 1h, unarmed — each copy its own pair
        assert_eq!(
            r.state.entities[p[0].as_str()].attrs.get("expires"),
            Some(&AttrValue::Num(180))
        );
        assert_eq!(
            r.state.entities[p[0].as_str()].attrs.get("expiresAt"),
            Some(&AttrValue::Num(180))
        );
        assert_eq!(r.state.entities[p[1].as_str()].attrs.get("expires"), Some(&AttrValue::Num(60)));
        assert_eq!(
            r.state.entities[p[1].as_str()].attrs.get("expiresAt"),
            Some(&AttrValue::Num(60))
        );
        assert_eq!(
            r.state.entities[p[2].as_str()].attrs.get("expiresAt"),
            None,
            "the plain spawn arms nothing"
        );
        // the concrete SPAWN rows carry the pair inside attrs= (§9.2) —
        // the modifier itself never reaches the log
        let done_rows: Vec<String> = r
            .records
            .iter()
            .filter(|rec| rec.kind == "complete")
            .flat_map(|rec| rec.outcomes.iter().map(|o| format!("{o:?}")))
            .collect();
        let armed_rows = done_rows.iter().filter(|o| o.contains("expiresAt")).count();
        assert_eq!(armed_rows, 2, "the concrete attrs= pairs carry expiresAt: {done_rows:?}");
        assert!(
            done_rows.iter().all(|o| !o.contains("Dur")),
            "no authored modifier arg in the log: {done_rows:?}"
        );
        // the whole turn folds identically — structural (windows/entities)
        // and through the shared snap
        let replayed = fold(&r.records, &base, &dict, &c).unwrap();
        assert_eq!(replayed.entities, r.state.entities, "identical entities");
        assert_eq!(snap(&replayed), snap(&r.state));
    }

    /// The pair is authoritative materialization: JSONL round-trip, then
    /// fold, land the same armed attrs.
    #[test]
    fn the_armed_mint_survives_jsonl_and_refolds_identically() {
        let dict = core_dictionary();
        let (c, s) = compile(
            &dict,
            &rig(
                "      d when count(actors) >= 1 -> SPAWN DREAD expires 90m, NARRATE \"done\"",
                "",
                None,
            ),
        );
        let base = clone_state(&s);
        let r = run_with(&c, &dict, s, "0:0 @ ANA DREAM", 1);
        assert_eq!(r.rejected, None);
        let dir = std::env::temp_dir().join(format!("chorus-spawn-expires-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("log.jsonl");
        save_log(&path, &r.records).unwrap();
        let loaded = load_log(&path).unwrap();
        assert_eq!(loaded, r.records);
        let replayed = fold(&loaded, &base, &dict, &c).unwrap();
        let p = pool(&r.state);
        assert_eq!(
            replayed.entities[p[0].as_str()].attrs.get("expiresAt"),
            Some(&AttrValue::Num(90))
        );
        assert_eq!(snap(&replayed), snap(&r.state));
    }

    /// An authored `on expire of DREAD` trigger fires at the right minute:
    /// `expires 45m` from minute 0 crosses no boundary at :30 and fires at
    /// the :60 crossing (the first halfhour boundary at or after expiresAt
    /// that the clock actually crosses, §6.6/§4.2's zero rule).
    #[test]
    fn expiry_fires_the_on_expire_trigger_at_the_right_minute() {
        let dict = core_dictionary();
        let (c, s) = compile(
            &dict,
            &rig(
                "      d when count(actors) >= 1 -> SPAWN DREAD expires 45m, NARRATE \"done\"",
                "",
                Some("  melt on expire of DREAD -> NARRATE \"the dread melts\""),
            ),
        );
        let base = clone_state(&s);
        let r1 = run_with(&c, &dict, s, "0:0 @ ANA DREAM", 1);
        assert_eq!(r1.rejected, None);
        // set this pass: resolves at the NEXT advance (§6.6 snapshot)
        assert!(!r1.records.iter().any(|x| x.kind == "event"));
        let r2 = advance(&c, &dict, r1.state, "0:2", 100); // crosses :30 and :60
        assert_eq!(r2.rejected, None);
        let events: Vec<&ActionRecord> = r2.records.iter().filter(|x| x.kind == "event").collect();
        assert_eq!(events.len(), 1, "one expiry, at the :60 crossing");
        assert_eq!(events[0].trigger.as_deref(), Some("melt"));
        assert_eq!(events[0].t_game, "0:2", "the :60 crossing, not :30");
        assert!(events[0].sentence.contains("the dread melts"));
        // §6.6: the authored outcomes replace the built-in destroy entirely
        // — the NARRATE-only trigger leaves the copy alive to re-fire
        assert_eq!(
            r2.state.entities[pool(&r2.state)[0].as_str()].dead_action,
            None,
            "the authored expiry replaced the built-in destroy"
        );
        let replayed = fold(&[r1.records, r2.records].concat(), &base, &dict, &c).unwrap();
        assert_eq!(snap(&replayed), snap(&r2.state));
    }

    /// INTO-targeted spawns arm the same way, and an authored `attrs=`
    /// materialization pair merges beside the modifier (§9.2) — with the
    /// modifier applied after the merge, so it is authoritative over a
    /// literal `expiresAt` in the same step's pair.
    #[test]
    fn into_targeted_spawns_arm_and_authored_attrs_pairs_merge() {
        let dict = core_dictionary();
        let (c, s) = compile(
            &dict,
            &rig(
                "      d when count(actors) >= 1 -> SPAWN DREAD expires 30m attrs=\"{\\\"container\\\":\\\"house\\\",\\\"zone\\\":\\\"cellar\\\"}\", SPAWN CHALICE expires 1h attrs=\"{\\\"expiresAt\\\":999}\", NARRATE \"done\"",
                "",
                None,
            ),
        );
        let r = run_with(&c, &dict, s, "0:0 @ ANA DREAM", 1);
        assert_eq!(r.rejected, None);
        let d = &r.state.entities[pool(&r.state)[0].as_str()];
        assert_eq!(d.attrs.get("container"), Some(&AttrValue::Str("house".to_string())));
        assert_eq!(d.attrs.get("zone"), Some(&AttrValue::Str("cellar".to_string())));
        assert_eq!(d.attrs.get("expires"), Some(&AttrValue::Num(30)));
        assert_eq!(d.attrs.get("expiresAt"), Some(&AttrValue::Num(30)));
        // the modifier wins over the pair's literal
        let ch = &r.state.entities[r.state.by_name["CHALICE"][0].as_str()];
        assert_eq!(ch.attrs.get("expiresAt"), Some(&AttrValue::Num(60)));
        assert_eq!(ch.attrs.get("expires"), Some(&AttrValue::Num(60)));
    }

    /// The zero-duration boundary is SET-expires' own (§4.2): expiresAt =
    /// the mint clock, firing at the first halfhour boundary at or after
    /// that minute the clock actually crosses.
    #[test]
    fn zero_duration_materializes_the_mint_clock_like_set() {
        let dict = core_dictionary();
        let (c, s) = compile(
            &dict,
            &rig(
                "      d when count(actors) >= 1 -> SPAWN DREAD expires 0m, NARRATE \"done\"",
                "",
                None,
            ),
        );
        let base = clone_state(&s);
        let r1 = run_with(&c, &dict, s, "0:20 @ ANA DREAM", 1); // absolute minute 600
        assert_eq!(r1.rejected, None);
        assert_eq!(
            r1.state.entities[pool(&r1.state)[0].as_str()].attrs.get("expiresAt"),
            Some(&AttrValue::Num(600))
        );
        // fires at the first crossed boundary at or after 600 — the :30 slot
        // of hour 20 (absolute 630) — during the next advance
        let r2 = advance(&c, &dict, r1.state, "0:22", 100); // crosses 630
        assert_eq!(r2.rejected, None);
        assert!(r2.records.iter().any(|x| x.kind == "event"));
        let replayed = fold(&[r1.records, r2.records].concat(), &base, &dict, &c).unwrap();
        assert_eq!(snap(&replayed), snap(&r2.state));
    }

    /// A non-duration is the same loud rejection SET-expires gives: the bare
    /// form cannot even lex into the modifier slot (parse error), the quoted
    /// form reaches the one duration parser and refuses the window with
    /// nothing logged as state (§4.2's "2fortnights" precedent).
    #[test]
    fn bad_durations_get_the_set_expires_rejections() {
        let dict = core_dictionary();
        // a negative bare duration lexes as punct + duration — the modifier
        // slot rejects it at parse, naming the grammar
        assert_err_msg(
            parse_sentence("0:0 @ ANA DREAM -> SPAWN DREAD expires -1h", &dict),
            "SPAWN expires takes a §5.1 duration \\(e\\.g\\. 2h, 90m, 1halfhour\\), got '-'",
        );
        // a non-duration keyword lands the same way
        assert_err_msg(
            parse_sentence("0:0 @ ANA DREAM -> SPAWN DREAD expires NEVER", &dict),
            "SPAWN expires takes a §5.1 duration \\(e\\.g\\. 2h, 90m, 1halfhour\\), got 'NEVER'",
        );
        // the quoted form is §4.2's string path: a bad unit refuses the
        // window at application, exactly like SET
        let (c, s) = compile(
            &dict,
            &rig(
                "      d when count(actors) >= 1 -> SPAWN DREAD expires \"2fortnights\", NARRATE \"done\"",
                "",
                None,
            ),
        );
        let r = run_with(&c, &dict, s, "0:0 @ ANA DREAM", 1);
        assert_eq!(r.rejected, None);
        assert_refusal(&r, r"bad duration '2fortnights'");
        assert!(!r.state.by_name.contains_key("DREAD"), "the refused mint never landed");
    }

    /// The §5.2 round-trip of the new spelling: the modifier renders
    /// keyword-restored from the arg shape, canonicalizes its duration per
    /// §5.1, and re-parses to the identical outcome; quoted forms re-quote.
    #[test]
    fn the_new_spelling_round_trips() {
        let dict = core_dictionary();
        let s =
            parse_sentence("0:0 @ ANA DREAM -> SPAWN DREAD INTO HOUSE expires 90m", &dict).unwrap();
        let out = format_sentence(&s, Some(&dict)).unwrap();
        // 90m canonicalizes to the exact single unit 3halfhour (§5.1)
        assert_eq!(out, "0:0 @ ANA DREAM -> SPAWN DREAD INTO HOUSE expires 3halfhour");
        let s2 = parse_sentence(&out, &dict).unwrap();
        assert_eq!(s2.outcomes, s.outcomes);
        assert_eq!(format_sentence(&s2, Some(&dict)).unwrap(), out);
        // a compound with no exact single unit decomposes (§5.1) and still
        // re-parses
        let s3 = parse_sentence("0:0 @ ANA DREAM -> SPAWN DREAD expires 100m", &dict).unwrap();
        let out3 = format_sentence(&s3, Some(&dict)).unwrap();
        assert_eq!(out3, "0:0 @ ANA DREAM -> SPAWN DREAD expires 1h40m");
        assert_eq!(parse_sentence(&out3, &dict).unwrap().outcomes, s3.outcomes);
        // the §4.2 quoted form keeps its string and re-quotes
        let s4 = parse_sentence("0:0 @ ANA DREAM -> SPAWN DREAD expires \"2h\"", &dict).unwrap();
        let out4 = format_sentence(&s4, Some(&dict)).unwrap();
        assert_eq!(out4, "0:0 @ ANA DREAM -> SPAWN DREAD expires \"2h\"");
        assert_eq!(parse_sentence(&out4, &dict).unwrap().outcomes, s4.outcomes);
        // the modifier sits after INTO/AS: INTO after the modifier is a
        // parse error (the authored order is pinned)
        assert!(parse_sentence("0:0 @ ANA DREAM -> SPAWN DREAD expires 30m INTO HOUSE", &dict,)
            .is_err());
        // the `key=value` spelling is NOT the modifier (P2-c): the keyword
        // branch fires before the pair loop, so `expires=2h` is a loud parse
        // error naming the modifier's grammar — the pair form can never
        // silently stand in for it
        assert_err_msg(
            parse_sentence("0:0 @ ANA DREAM -> SPAWN DREAD expires=2h", &dict),
            "SPAWN expires takes a §5\\.1 duration \\(e\\.g\\. 2h, 90m, 1halfhour\\), got '='",
        );
    }

    /// The anchor inside a REAL completion — a 1h window begun at 0:0, its
    /// outcomes applying when the settle advance closes it at 1:0 — is the
    /// window START clock (§6.7, the same clock SET-expires anchors to
    /// there): `expires 30m` materializes 0:30, never completion+30 (1:30).
    /// The expiry snapshot predates the mint, so the fire waits for the
    /// next advance's first crossed boundary at or after 0:30 — the :90
    /// slot (§6.6).
    #[test]
    fn the_anchor_inside_a_real_completion_is_the_window_start() {
        let dict = core_dictionary();
        let text = rig(
            "      d when count(actors) >= 1 -> SPAWN DREAD expires 30m, NARRATE \"done\"",
            "",
            None,
        )
        .replace("duration: 0m", "duration: 1h");
        let (c, s) = compile(&dict, &text);
        let base = clone_state(&s);
        // the settle adapter: begin at 0:0, then the closing advance to the
        // window's end (1:0) — the completion mints there
        let r1 = run_with(&c, &dict, s, "0:0 @ ANA DREAM", 1);
        assert_eq!(r1.rejected, None);
        assert!(r1.state.windows.is_empty(), "the settle advance closed the window");
        assert!(
            !r1.records.iter().any(|x| x.kind == "event"),
            "no in-pass fire (the snapshot predates the mint, §6.6)"
        );
        assert_eq!(
            r1.state.entities[pool(&r1.state)[0].as_str()].attrs.get("expiresAt"),
            Some(&AttrValue::Num(30)),
            "window START + 30 (0:30), never completion + 30 (1:30)"
        );
        // the next advance (now = 1:0) crosses the :90 slot → the fire
        let r2 = advance(&c, &dict, r1.state, "1:2", 200);
        assert_eq!(r2.rejected, None);
        let events: Vec<&ActionRecord> = r2.records.iter().filter(|x| x.kind == "event").collect();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].t_game, "0:3", "the :90-minute crossing (halfhour 3) — the first boundary at or after expiresAt that the clock crosses");
        let replayed = fold(&[r1.records, r2.records].concat(), &base, &dict, &c).unwrap();
        assert_eq!(snap(&replayed), snap(&r2.state));
    }

    /// A non-pooled mint arms exactly the same way (the §9.2 tier where
    /// `SPAWN X, SET X, expires, …` was already exact — the modifier makes
    /// it one op).
    #[test]
    fn a_non_pool_mint_arms_the_same_way() {
        let dict = core_dictionary();
        let (c, s) = compile(
            &dict,
            &rig(
                "      d when count(actors) >= 1 -> SPAWN CHALICE expires 2h, NARRATE \"done\"",
                "",
                None,
            ),
        );
        let r = run_with(&c, &dict, s, "0:0 @ ANA DREAM", 1);
        assert_eq!(r.rejected, None);
        let id = &r.state.by_name["CHALICE"][0];
        assert_eq!(
            r.state.entities[id.as_str()].attrs.get("expiresAt"),
            Some(&AttrValue::Num(120))
        );
    }
}

/// Core verbs STUDY and MANSUS (§4.3, §4.3)
mod core_verbs_study_mansus {
    use super::*;

    /// The mirroring the spec declares: STUDY
    /// copies READ's slots, MANSUS copies EXPLORE's — cardinalities and
    /// queries stay content (§6.1)
    fn verbs_content(dict: &Dictionary) -> CompiledContent {
        compile_content(
            &ContentRaw {
                verbs: [
                    (
                        "STUDY".to_string(),
                        VerbContent {
                            slots: vec![
                                SlotDef {
                                    role: "actors".to_string(),
                                    min: 1,
                                    max: Some(2),
                                    query: Some("kind:character".to_string()),
                                    shown: None,
                                },
                                SlotDef {
                                    role: "object".to_string(),
                                    min: 1,
                                    max: Some(1),
                                    query: Some("kind:text".to_string()),
                                    shown: None,
                                },
                                SlotDef {
                                    role: "at".to_string(),
                                    min: 0,
                                    max: Some(1),
                                    query: Some("kind:place".to_string()),
                                    shown: None,
                                },
                            ],
                            duration: 60,
                            recipes: vec![
                                RecipeDef {
                                    id: Some("study.secret".to_string()),
                                    when: Some("object.aspect.secret >= 3".to_string()),
                                    shown: None,
                                    hint: None,
                                    max: None,
                                    aspects: Vec::new(),
                                    out: vec![
                                        "SPAWN CLUE".to_string(),
                                        "NARRATE(\"{actors} tease out a hidden thread.\")"
                                            .to_string(),
                                    ],
                                },
                                RecipeDef {
                                    id: None,
                                    when: None,
                                    shown: None,
                                    hint: None,
                                    max: None,
                                    aspects: Vec::new(),
                                    out: vec!["NARRATE(\"{actors} study {object}.\")".to_string()],
                                },
                            ],
                        },
                    ),
                    (
                        "MANSUS".to_string(),
                        VerbContent {
                            slots: vec![
                                SlotDef {
                                    role: "actors".to_string(),
                                    min: 1,
                                    max: Some(1),
                                    query: Some("kind:character".to_string()),
                                    shown: None,
                                },
                                SlotDef {
                                    role: "object".to_string(),
                                    min: 1,
                                    max: Some(1),
                                    query: Some("kind:place".to_string()),
                                    shown: None,
                                },
                            ],
                            duration: 60,
                            recipes: vec![RecipeDef {
                                id: None,
                                when: None,
                                shown: None,
                                hint: None,
                                max: None,
                                aspects: Vec::new(),
                                out: vec!["NARRATE(\"{actors} walk the Mansus to {object}.\")"
                                    .to_string()],
                            }],
                        },
                    ),
                ]
                .into_iter()
                .collect(),
                prototypes: Some(
                    [(
                        "CLUE".to_string(),
                        PrototypeDef {
                            kind: "idea".to_string(),
                            derive: Some(PrototypeDerive {
                                name_from: Some("object".to_string()),
                                aspects: Some(
                                    [(
                                        "secret".to_string(),
                                        DeriveAspect {
                                            from_object_aspect: "secret".to_string(),
                                            minus: 2,
                                        },
                                    )]
                                    .into_iter()
                                    .collect(),
                                ),
                            }),
                            ..PrototypeDef::default()
                        },
                    )]
                    .into_iter()
                    .collect(),
                ),
                ..ContentRaw::default()
            },
            dict,
            &[],
        )
        .unwrap()
    }

    /// World
    fn world() -> State {
        let mut s = seed_state();
        plain_card(
            &mut s,
            "manifest",
            "text",
            "THE_MANIFEST",
            num_aspect("secret", 4),
            container_attrs("house"),
            false,
        );
        s
    }

    #[test]
    fn the_mirrored_grammar_bites_mansus_takes_a_place_not_a_text() {
        let (dict, _content, _s) = setup();
        let vc = verbs_content(&dict);
        let r = run_with(&vc, &dict, world(), "0:14 @ ANA MANSUS THE_MANIFEST", 1);
        assert_rejected(&r, r"does not fit slot 'object'");
        assert_eq!(r.records.len(), 0);
        let r2 = run_with(&vc, &dict, world(), "0:14 @ ANA MANSUS HOUSE", 1);
        assert_eq!(r2.rejected, None);
        assert_eq!(act(&r2).unwrap().recipes, ["mansus.default"]);
        assert!(act(&r2).unwrap().sentence.contains("ANA MANSUS HOUSE"));
    }

    #[test]
    fn untemplated_content_renders_acceptably_recipe_narrates_speak_no_template_line() {
        let (dict, content, _s) = setup();
        let bare = compile_content(
            &ContentRaw {
                verbs: [(
                    "MANSUS".to_string(),
                    VerbContent {
                        slots: vec![
                            SlotDef {
                                role: "actors".to_string(),
                                min: 1,
                                max: Some(1),
                                query: Some("kind:character".to_string()),
                                shown: None,
                            },
                            SlotDef {
                                role: "object".to_string(),
                                min: 1,
                                max: Some(1),
                                query: Some("kind:place".to_string()),
                                shown: None,
                            },
                        ],
                        duration: 60,
                        recipes: vec![RecipeDef {
                            id: None,
                            when: None,
                            shown: None,
                            hint: None,
                            max: None,
                            aspects: Vec::new(),
                            out: vec![
                                "NARRATE(\"{actors} walk the Mansus to {object}.\")".to_string()
                            ],
                        }],
                    },
                )]
                .into_iter()
                .collect(),
                // no `templates:` section at all
                ..ContentRaw::default()
            },
            &dict,
            &[],
        )
        .unwrap();
        let r = run_with(&bare, &dict, world(), "0:14 @ ANA MANSUS HOUSE", 1);
        assert_eq!(r.rejected, None);
        // v0.5: the NARRATE speaks at the completion, so the prose renders
        // from there
        assert_eq!(
            chorus::session::render::prose(done(&r).unwrap(), &bare, &dict).unwrap(),
            "ANA walk the Mansus to HOUSE."
        );
        let _ = content;
    }

    #[test]
    fn the_seed_verbs_pin_the_mirror_in_code_study_read_mansus_explore() {
        let (_dict, content, _s) = setup();
        // the mirroring the spec declares, asserted against the seed content —
        // not prose: slots are the grammar, cardinalities and queries included
        assert_eq!(
            content.verbs.get("STUDY").unwrap().slots,
            content.verbs.get("READ").unwrap().slots
        );
        assert_eq!(
            content.verbs.get("MANSUS").unwrap().slots,
            content.verbs.get("EXPLORE").unwrap().slots
        );
    }

    #[test]
    fn a_story_that_does_not_author_the_verbs_rejects_at_the_content_gate() {
        let (dict, content, _s) = setup();
        // registration puts STUDY/MANSUS in the dictionary (they parse, they
        // are reserved); content that never declares them refuses cleanly
        let unauthored = compile_content(
            &ContentRaw { verbs: IndexMap::new(), ..ContentRaw::default() },
            &dict,
            &[],
        )
        .unwrap();
        let s1 = world();
        let r = run_with(&unauthored, &dict, s1, "0:14 @ ANA STUDY THE_MANIFEST", 1);
        assert_rejected(&r, r"STUDY is not a content verb");
        assert_eq!(r.records.len(), 0);
        assert_eq!(snap(&r.state), snap(&world())); // same world, untouched
        let s2 = world();
        let r2 = run_with(&unauthored, &dict, s2, "0:14 @ ANA MANSUS HOUSE", 1);
        assert_rejected(&r2, r"MANSUS is not a content verb");
        assert_eq!(r2.records.len(), 0);
        assert_eq!(snap(&r2.state), snap(&world()));
        let _ = content;
    }
}
