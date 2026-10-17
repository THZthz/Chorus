//! Engine acceptance tests — the spec §6.4 READ ladder, REFUSE semantics
//! (§6.5), time ownership (§6.6), events (§6.6), fold determinism (§7).
//!
//! The mod tree groups the behaviors; the two heaviest families split into
//! sibling files of this binary: the counter-writing SET/REPLACE ops live in
//! tests/engine/set_ops.rs and the E12 expiry family in tests/engine/expiry.rs
//! (both reach the shared seed-world rig in tests/engine/rig.rs). Every test
//! builds a FRESH Dictionary (per-test isolation — this choice affects
//! reserved-word collision behavior and must not be shared). Error-message
//! patterns are copied CHARACTER FOR CHARACTER from the frozen regex
//! literals; state equality goes through the shared `snap()` (law 6); the
//! run adapter is `crate::common::rig::settle_line` — parse +
//! `chorus::engine::settle::settle_step` with the id-generator closure
//! `x{first_record_id}_{n}`, which lives in ONE place (the REPLACE sentence
//! assertions regex-match the minted `#x\d+_1` ids, so the id sequence is
//! load-bearing).
//!
//! Reference-identity semantics (`r.state === state`) have no
//! Rust observable: `execute_step` consumes the State and hands the ORIGINAL
//! back untouched on a rejection/refusal, so `snap()` equality IS the
//! semantic content of those checks (noted at each site, never weakened).

use crate::common::harness::{assert_err_msg, assert_refusal, assert_rejected};
use crate::common::rig::{kinds, ops, settle_line, tmp_log_path};
use crate::common::seed::seed_state;
use crate::common::snap::snap;
use crate::rig::{container_attrs, fixture_complete, num_aspect, outcome, plain_card, setup};
use chorus::engine::settle::{act, done};

use chorus::content::{
    compile_content, CompiledContent, ContentRaw, DeriveAspect, PrototypeDef, PrototypeDerive,
    RecipeDef, SlotDef, SlotTriggerDef, TriggerDef, VerbContent,
};
use chorus::core::clock::format_duration;
use chorus::core::dict::{core_dictionary, Dictionary};
use chorus::core::parse::parse_sentence;
use chorus::core::r#ref::{name_ref, Ref};
use chorus::core::sentence::Arg;
use chorus::core::types::{new_state, AspectVal, Aspects, AttrValue, State};
use chorus::engine::fold::{fold, fold_up_to};
use chorus::engine::store::{load_log, save_log, Log};
use chorus::engine::{apply_record, ActionRecord};
use indexmap::IndexMap;

// ---------------------------------------------------------------- rig ------

// ------------------------------------------------------------- ladder ------

/// READ ladder — first match wins, most specific
/// first (§6.4)
mod read_ladder {
    use super::*;

    #[test]
    fn solo_reader_with_knowledge_passes_the_gate_default_narrates() {
        let (dict, content, state) = setup();
        let r = settle_line(&content, &dict, state, "0:28 @ ANA READ MAGICK_BOOK", 1).unwrap();
        assert_eq!(r.rejected, None);
        assert_eq!(act(&r).unwrap().recipes, ["read.default"]);
        assert_eq!(act(&r).unwrap().advanced_min, 60);
        assert_eq!(r.state.now, 14 * 60 + 60);
    }

    #[test]
    fn magick_gate_refuses_an_ignorant_reader_no_state_change_no_time_attempt_logged() {
        let (dict, content, state) = setup();
        let before = snap(&state);
        let r = settle_line(&content, &dict, state, "0:28 @ BERTHA READ MAGICK_BOOK", 1).unwrap();
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
        let r =
            settle_line(&content, &dict, state, "0:28 @ ANA, BERTHA READ FAIRY_TALES", 1).unwrap();
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
        let r = settle_line(&content, &dict, state, "0:28 @ ANA, MY_FOLLOWER READ FAIRY_TALES", 1)
            .unwrap();
        assert_eq!(act(&r).unwrap().recipes, ["read.unrelated_pair"]);
        assert!(done(&r).unwrap().outcomes.iter().all(|o| o.op == "NARRATE"));
    }

    #[test]
    fn prototype_parameterized_spawn_resolves_into_a_concrete_named_card() {
        let (dict, content, state) = setup();
        let r = settle_line(&content, &dict, state, "0:28 @ ANA READ THE_FORGETTING", 1).unwrap();
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
        let r = settle_line(
            &content,
            &dict,
            state,
            "0:28 @ ANA, BERTHA, MY_FOLLOWER READ FAIRY_TALES",
            1,
        )
        .unwrap();
        assert_rejected(&r, r"'actors' takes 1\.\.2");
        assert_eq!(r.records.len(), 0);
    }

    #[test]
    fn slot_discipline_undeclared_roles_are_rejected() {
        let (dict, content, state) = setup();
        let r = settle_line(&content, &dict, state, "0:28 @ ANA READ FAIRY_TALES WITH CHALICE", 1)
            .unwrap();
        assert_rejected(&r, r"does not accept 'with'");
        assert_eq!(r.records.len(), 0);
    }

    #[test]
    fn slot_discipline_one_card_cannot_fill_a_role_twice() {
        let (dict, content, state) = setup();
        let r = settle_line(&content, &dict, state, "0:28 @ ANA, ANA READ FAIRY_TALES", 1).unwrap();
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
        let r = settle_line(
            &content,
            &dict,
            state,
            "0:28 @ ANA, BERTHA AT DINING_HALL TALK ABOUT CLUE_SECRET_HIDEOUT",
            1,
        )
        .unwrap();
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
        let r = settle_line(
            &content,
            &dict,
            state,
            "0:28 @ ANA TRAVEL_TO THE_STUDY; ANA AT THE_STUDY READ THE_FORGETTING",
            1,
        )
        .unwrap();
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
        let r =
            settle_line(&content, &dict, state, "0:28 @ ANA AT HOUSE USE_ON RITE_OF_ASCENSION", 1)
                .unwrap();
        assert_eq!(ops(done(&r).unwrap()), ["REFUSE"]);
        assert_eq!(r.state.now, 14 * 60);
        assert_eq!(snap(&r.state), before);
    }

    #[test]
    fn pooled_lantern_meets_demand_rite_consumed_instruments_destroyed_mark_spawned_into_place() {
        let (dict, content, state) = setup();
        let r = settle_line(
            &content,
            &dict,
            state,
            "0:28 @ ANA, MY_FOLLOWER AT HOUSE USE_ON RITE_OF_ASCENSION WITH CHALICE",
            1,
        )
        .unwrap();
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

/// Events — authored triggers and expiries (§6.6)
mod events {
    use super::*;

    #[test]
    fn crossing_hour_16_fires_dusk_falls_as_its_own_event_row() {
        let (dict, content, state) = setup();
        let r1 = settle_line(&content, &dict, state, "0:28 @ ANA WAIT", 1).unwrap(); // crosses 15 only
        let mut kinds1 = kinds(&r1.records);
        if kinds1.first() == Some(&"wait") {
            kinds1.remove(0); // the typed clock bridges from now=0
        }
        assert_eq!(kinds1, ["action", "complete", "wait"]); // v0.5 turn shape
        let r2 = settle_line(&content, &dict, r1.state, "0:30 @ ANA WAIT", 1).unwrap(); // crosses 16 → dusk
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
        let r = settle_line(
            &content,
            &dict,
            state,
            "0:28 @ ANA WAIT; ANA WAIT; ANA WAIT", // three staggered windows, 14 → 17
            1,
        )
        .unwrap();
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
            let r = settle_line(&content, &dict, state, step, id).unwrap();
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
        let r = settle_line(&content, &dict, state, "0:28 @ BERTHA READ MAGICK_BOOK", 1).unwrap();
        let replayed = fold(&r.records, &seed_state(), &dict, &content).unwrap();
        assert_eq!(snap(&replayed), before);
    }

    #[test]
    fn jsonl_persistence_round_trips_the_log() {
        let (dict, content, state) = setup();
        let r = settle_line(&content, &dict, state, "0:28 @ ANA READ THE_FORGETTING", 1).unwrap();
        let path = tmp_log_path("engine-jsonl-roundtrip");
        save_log(&path, &r.records).unwrap();
        let loaded = load_log(&path).unwrap();
        let replayed = fold(&loaded, &seed_state(), &dict, &content).unwrap();
        assert_eq!(snap(&replayed), snap(&r.state));
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn undo_re_fold_without_the_tail() {
        let (dict, content, state) = setup();
        let r1 = settle_line(&content, &dict, state, "0:28 @ ANA READ THE_FORGETTING", 1).unwrap();
        let snapshot_after_one = snap(&r1.state);
        let r2 = settle_line(
            &content,
            &dict,
            r1.state,
            "0:32 @ ANA READ FAIRY_TALES",
            r1.records.len() as i64 + 1,
        )
        .unwrap();
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
        let mut r = settle_line(
            &content,
            &dict,
            state,
            "0:28 @ ANA, MY_FOLLOWER AT HOUSE USE_ON RITE_OF_ASCENSION WITH CHALICE",
            1,
        )
        .unwrap();
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
        let r = settle_line(
            &content,
            &dict,
            state,
            "0:28 @ ANA WAIT; ANA WAIT; ANA WAIT", // 14→17, events at 15/16
            1,
        )
        .unwrap();
        assert!(r.records.iter().filter(|x| x.kind == "event").count() >= 2);
        let replayed = fold(&r.records, &seed_state(), &dict, &content).unwrap();
        assert_eq!(replayed.now, r.state.now); // events must not drag the clock back
        assert_eq!(replayed.now, 17 * 60);
    }

    #[test]
    fn role_sugar_is_concretized_in_the_log_replay_matches_play_exactly() {
        let (dict, content, state) = setup();
        let r = settle_line(
            &content,
            &dict,
            state,
            "0:28 @ ANA, BERTHA TRAVEL_TO VILLA; MY_FOLLOWER WAIT",
            1,
        )
        .unwrap();
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
        let r = settle_line(&content, &dict, state, "0:6 @ ANA WAIT", 1).unwrap();
        assert_rejected(&r, r"before state\.now");
        assert_eq!(r.records.len(), 0);
    }

    #[test]
    fn application_failures_refuse_the_window_instead_of_throwing() {
        let (dict, content, state) = setup();
        let r =
            settle_line(&content, &dict, state, "0:28 @ ANA, BERTHA READ FAIRY_TALES", 1).unwrap();
        assert_eq!(r.rejected, None);
        let r2 = settle_line(
            &content,
            &dict,
            r.state,
            "0:32 @ ANA, BERTHA READ FAIRY_TALES", // duplicate mint name
            r.records.len() as i64 + 1,
        )
        .unwrap();
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
        let r = settle_line(&c, &pd, state, "0:28 @ ANA PROBE", 1).unwrap();
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
        let r = settle_line(&gate(&pd), &pd, state, "0:28 @ ANA PROBE", 1).unwrap();
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
        let r = settle_line(&gate(&pd), &pd, state, "0:28 @ ANA PROBE", 1).unwrap();
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
        let r = settle_line(&c, &pd, state, "0:28 @ ANA PROBE", 1).unwrap();
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
        let r = settle_line(&c, &pd, state, "0:28 @ ANA, BERTHA PROBE", 1).unwrap();
        assert_eq!(act(&r).unwrap().recipes, ["probe.bonded"]);
        // no parent_child edge — a fresh seed for the second probe
        let r2 = settle_line(&c, &pd, seed_state(), "0:30 @ ANA, MY_FOLLOWER PROBE", 2).unwrap();
        assert_eq!(act(&r2).unwrap().recipes, ["probe.default"]);
    }
}

/// Progress ticks — the processing read-bar (§6.7)
mod progress_ticks {
    use super::*;

    #[test]
    fn ticks_record_in_emission_order_touch_no_state_cost_no_extra_time() {
        let (dict, content, state) = setup();
        let r = settle_line(
            &content,
            &dict,
            state,
            "0:28 @ ANA, MY_FOLLOWER AT HOUSE USE_ON RITE_OF_ASCENSION WITH CHALICE",
            1,
        )
        .unwrap();
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
        let r1 = settle_line(&content20, &dict, s, "0:28 @ ANA WAIT", 1).unwrap(); // 14:00 → 14:20
        assert_eq!(r1.rejected, None);
        assert_eq!(r1.state.now, 28 * 30 + 20);
        // the next step resumes on the halfhour tick (14:30); the chime
        // crosses inside the quantized gap, so it fires even though no window
        // spans 14:30
        let r2 = settle_line(&content20, &dict, r1.state, "0:29 @ ANA WAIT", 1).unwrap();
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
        let r = settle_line(&content, &dict, state, "0:44 @ ANA WORK AT HOUSE", 1).unwrap(); // 2h → day 1, 0:00
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
        let r = settle_line(
            &content,
            &dict,
            state,
            "0:28 @ BERTHA, ANA AT DINING_HALL TALK ABOUT CLUE_SECRET_HIDEOUT",
            1,
        )
        .unwrap();
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

/// Core verbs STUDY and MANSUS (§4.3)
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
        let r = settle_line(&vc, &dict, world(), "0:14 @ ANA MANSUS THE_MANIFEST", 1).unwrap();
        assert_rejected(&r, r"does not fit slot 'object'");
        assert_eq!(r.records.len(), 0);
        let r2 = settle_line(&vc, &dict, world(), "0:14 @ ANA MANSUS HOUSE", 1).unwrap();
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
        let r = settle_line(&bare, &dict, world(), "0:14 @ ANA MANSUS HOUSE", 1).unwrap();
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
        let r = settle_line(&unauthored, &dict, s1, "0:14 @ ANA STUDY THE_MANIFEST", 1).unwrap();
        assert_rejected(&r, r"STUDY is not a content verb");
        assert_eq!(r.records.len(), 0);
        assert_eq!(snap(&r.state), snap(&world())); // same world, untouched
        let s2 = world();
        let r2 = settle_line(&unauthored, &dict, s2, "0:14 @ ANA MANSUS HOUSE", 1).unwrap();
        assert_rejected(&r2, r"MANSUS is not a content verb");
        assert_eq!(r2.records.len(), 0);
        assert_eq!(snap(&r2.state), snap(&world()));
        let _ = content;
    }
}
