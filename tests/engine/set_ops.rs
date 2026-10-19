//! SET and REPLACE (§4.4) — the two counter-writing outcome ops, moved out of
//! engine_core:
//!
//! - `set_deltas_and_aspect_keys` — signed and absolute SET forms over attrs
//!   and aspects, missing-entry-as-zero, dot-key aspects, the record holding
//!   the concrete absolute, and the JSONL round trip;
//! - `replace_decay` — REPLACE's decay into a successor.
//!
//! Conventions follow tests/engine/engine_core.rs: a fresh dictionary per
//! test, error patterns copied CHARACTER FOR CHARACTER from the frozen
//! literals (regex-escaped), and play/fold equality asserted through the
//! shared `snap`. The seed-world rig lives in tests/engine/rig.rs; the run
//! adapter is `crate::common::rig::settle_line` with the load-bearing
//! `x{first_record_id}_{n}` id-generator closure.

use crate::common::harness::{assert_err_msg, assert_refusal};
use crate::common::rig::{ops, settle_line, tmp_log_path};
use crate::common::seed::seed_state;
use crate::common::snap::snap;
use crate::rig::{fixture_complete, num_aspect, rig, setup};

use chorus::content::{
    compile_content, CompiledContent, ContentRaw, PrototypeDef, RecipeDef, SlotDef, VerbContent,
};
use chorus::core::dict::{core_dictionary, Dictionary};
use chorus::core::format::format_sentence;
use chorus::core::parse::parse_sentence;
use chorus::core::types::{clone_state, AspectVal, AttrValue};
use chorus::engine::fold::fold;
use chorus::engine::settle::{act, done};
use chorus::engine::store::{load_log, save_log};
use chorus::engine::{apply_record, ActionRecord};
use indexmap::IndexMap;

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
                ambient: false,
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
            dur: None,
            out: out.iter().map(|s| s.to_string()).collect(),
        }
    }

    /// the round-trip probe: re-formatting a parsed sentence reproduces it
    /// exactly (`format_sentence(parse_sentence(s, dict), dict) == s`).
    fn assert_round_trips(sentence: &str, dict: &Dictionary) {
        let parsed = parse_sentence(sentence, dict).unwrap();
        let formatted = format_sentence(&parsed, dict).unwrap();
        assert_eq!(formatted, sentence, "sentence must round-trip: {sentence:?}");
    }

    #[test]
    fn a_signed_value_adds_to_the_attr_missing_is_zero_and_the_log_carries_the_absolute() {
        let (dict, content, state) = setup();
        let td = tune_dict();
        let c = counter_content(&td, vec![recipe(&["SET(ANA, funds, +2)"])]);
        let r = settle_line(&c, &td, state, "0:60 @ ANA TUNE", 1).unwrap();
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
        let r2 = settle_line(&c, &td, r.state, "0:63 @ ANA TUNE", 2).unwrap();
        assert_eq!(r2.state.entities["ana"].attrs.get("funds"), Some(&AttrValue::Num(4)));
        let _ = (&dict, &content);
    }

    #[test]
    fn decrements_may_go_negative_no_clamping() {
        let (dict, content, state) = setup();
        let td = tune_dict();
        let c = counter_content(&td, vec![recipe(&["SET(ACTORS, funds, -3)"])]);
        let r = settle_line(&c, &td, state, "0:60 @ ANA TUNE", 1).unwrap();
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
        let mut r = settle_line(&c, &td, state, "0:60 @ ANA TUNE", 1).unwrap();
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
        let r = settle_line(&c, &td, state, "0:60 @ ANA TUNE", 1).unwrap();
        assert_eq!(r.state.entities["ana"].attrs.get("funds"), Some(&AttrValue::Num(5)));
        let r2 = settle_line(&c, &td, r.state, "0:63 @ ANA TUNE", 2).unwrap();
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
                            ambient: false,
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
        let r = settle_line(
            &c,
            &td,
            state,
            "0:60 @ ANA TUNE THE_FORGETTING; BERTHA TUNE FAIRY_TALES",
            1,
        )
        .unwrap();
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
                dur: None,
                out: vec!["SET(ACTORS, aspect:dread, +1)".to_string()],
            }],
        );
        let r = settle_line(&c, &td, state, "0:60 @ ANA, BERTHA TUNE", 1).unwrap();
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
        for step in ["0:60 @ ANA, BERTHA TUNE", "0:63 @ ANA TUNE", "0:66 @ BERTHA TUNE"] {
            let r = settle_line(&c, &td, state, step, id).unwrap();
            assert_eq!(r.rejected, None);
            all.extend(r.records.clone());
            state = r.state;
            id += r.records.len() as i64;
        }
        assert_eq!(state.entities["ana"].attrs.get("funds"), Some(&AttrValue::Num(4)));
        assert_eq!(state.entities["ana"].aspects.get("dread"), Some(&AspectVal::Num(2)));
        assert_eq!(state.entities["bertha"].attrs.get("funds"), Some(&AttrValue::Num(14)));
        assert_eq!(state.entities["bertha"].aspects.get("dread"), Some(&AspectVal::Num(9)));
        let path = tmp_log_path("engine-delta-jsonl");
        save_log(&path, &all).unwrap();
        let replayed = fold(&load_log(&path).unwrap(), &seed_state(), &td, &c).unwrap();
        assert_eq!(snap(&replayed), snap(&state));
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
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
        let r = settle_line(&c, &td, state, "0:60 @ ANA TUNE", 1).unwrap();
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
        let r = settle_line(&c, &td, state, "0:60 @ ANA TUNE", 1).unwrap();
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
        let r = settle_line(&c, &td, state, "0:60 @ ANA TUNE", 1).unwrap();
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
        let r = settle_line(&c, &td, state, "0:60 @ ANA TUNE", 1).unwrap();
        assert_eq!(r.rejected, None);
        assert_eq!(r.state.entities["ana"].attrs.get("about"), Some(&AttrValue::Num(5)));
        assert!(done(&r).unwrap().sentence.contains("SET(#ana, \"about\", 5)"));
        // the record must round-trip: unquoted it would re-parse as the about ROLE
        assert_round_trips(&done(&r).unwrap().sentence.clone(), &td);
        let _ = (&dict, &content);
    }
}

/// REPLACE — decay into a successor (§4.4)
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
                    dur: None,
                    out: vec![out.to_string()],
                }],
                ambient: false,
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
        let r = settle_line(&c, &dd, state, "0:60 @ ANA DECAY CHALICE", 1).unwrap();
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
        let r = settle_line(&c, &dd, state, "0:60 @ ANA DECAY CHALICE", 1).unwrap();
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
        let r = settle_line(&c, &dd, base, "0:60 @ ANA DECAY CHALICE", 1).unwrap();
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
                                dur: None,
                                out: vec!["REPLACE OBJECT WITH ASHES".to_string()],
                            }],
                            ambient: false,
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
        let r = settle_line(&c, &dd, state, "0:60 @ ANA DECAY", 1).unwrap();
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
        let r = settle_line(&c, &dd, state, "0:60 @ ANA DECAY CHALICE", 1).unwrap();
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
        let r = settle_line(&c, &dd, state, "0:60 @ ANA DECAY CHALICE", 1).unwrap();
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
        let r = settle_line(&c, &dd, state, "0:60 @ ANA DECAY CHALICE", 1).unwrap(); // CHALICE is still alive at spawn time
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
            format_sentence(&name_form, &dd).unwrap(),
            "0:0 @ ANA DECAY CHALICE -> REPLACE CHALICE WITH ASHES"
        );
        let role_form =
            parse_sentence("0:0 @ ANA DECAY CHALICE -> REPLACE OBJECT WITH ASHES", &dd).unwrap();
        assert_eq!(
            format_sentence(&role_form, &dd).unwrap(),
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
