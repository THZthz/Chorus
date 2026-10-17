//! Expiry (§4.4, §4.2, §6.6, §9.2) — the four E12-family sections moved out of
//! engine_core:
//!
//! - `authored_expiry_triggers` — `on expire of` influence decay: G → E → C,
//!   and the built-in destroy when no trigger is declared;
//! - `trigger_outcome_guards` — trigger outcome guards (§6.7, §6.6);
//! - `set_expires_duration_forms` — every §5.1 duration form materializing
//!   the `expiresAt` attr (§4.2);
//! - `spawn_expires_modifier` — the SPAWN `expires <duration>` modifier: a
//!   mint arms its own decay at BIRTH with the SET-expires pair (one shared
//!   write), anchored to the mint clock.
//!
//! Conventions follow tests/engine/engine_core.rs: a fresh dictionary per
//! test, error patterns copied CHARACTER FOR CHARACTER from the frozen
//! literals (regex-escaped), and play/fold equality asserted through the
//! shared `snap`. The seed-world rig lives in tests/engine/rig.rs; the run
//! adapter is `crate::common::rig::settle_line` with the load-bearing
//! `x{first_record_id}_{n}` id-generator closure.

use crate::common::harness::{assert_err_msg, assert_refusal, assert_rejected};
use crate::common::rig::{settle_line, tmp_log_path};
use crate::common::seed::seed_state;
use crate::common::snap::snap;
use crate::rig::{container_attrs, num_aspect, plain_card, setup, step_of};

use chorus::content::{
    compile_content, CompiledContent, ContentRaw, ExpireTriggerDef, PrototypeDef, RecipeDef,
    SlotDef, SlotTriggerDef, TriggerDef, VerbContent,
};
use chorus::core::dict::{core_dictionary, Dictionary};
use chorus::core::format::format_sentence;
use chorus::core::parse::parse_sentence;
use chorus::core::types::{clone_state, AttrValue, State};
use chorus::engine::fold::fold;
use chorus::engine::settle::{done, settle_step};
use chorus::engine::store::{load_log, save_log};
use chorus::engine::{ActionRecord, ExecResult, Step};
use indexmap::IndexMap;

/// Authored expiry triggers — `on expire of`
/// (§6.6). influence decay: G → E → C → (no trigger: built-in destroy)
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
        let r = settle_line(&c, &dict, state, "0:28 @ ANA WAIT; ANA WAIT; ANA WAIT", 1).unwrap();
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
        let r = settle_line(&c, &dict, state, "0:28 @ ANA WAIT; ANA WAIT", 1).unwrap();
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
        let path = tmp_log_path("engine-moth-jsonl");
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
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
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
/// materializes expiresAt (§4.2)
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
            let r = settle_line(
                &tune_content(&td, &format!("SET(CHALICE, expires, {lit})")),
                &td,
                state,
                "0:20 @ ANA TUNE",
                1,
            )
            .unwrap();
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
        let r = settle_line(
            &tune_content(&td, "SET(CHALICE, expires, 2halfhour)"),
            &td,
            seed_state(),
            "0:20 @ ANA TUNE",
            1,
        )
        .unwrap();
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
        let rq = settle_line(
            &tune_content(&td, "SET(CHALICE, expires, \"2halfhour\")"),
            &td,
            seed_state(),
            "0:20 @ ANA TUNE",
            1,
        )
        .unwrap();
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
        let r = settle_line(
            &tune_content(&td, "SET(CHALICE, expires, \"2fortnights\")"),
            &td,
            state,
            "0:20 @ ANA TUNE",
            1,
        )
        .unwrap();
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
        let r1 = settle_line(&c, &td, state, "0:28 @ ANA TUNE", 1).unwrap(); // expiresAt = 28*30 + 60
        assert_eq!(r1.rejected, None);
        // set this pass: resolves at the NEXT advance (§6.6 snapshot)
        assert!(!r1.records.iter().any(|x| x.kind == "event"));
        let r2 =
            settle_line(&c, &td, r1.state, "0:30 @ ANA WAIT", 1 + r1.records.len() as i64).unwrap(); // crosses slot 31 (0:31)
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
    use chorus::engine::{execute_step, ExecOptions};

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
        let r = settle_line(&c, &dict, s, "0:0 @ ANA DREAM", 1).unwrap();
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
        let r = settle_line(&c, &dict, s, "0:0 @ ANA DREAM", 1).unwrap();
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
        let r1 = settle_line(&c, &dict, s, "0:0 @ ANA DREAM", 1).unwrap();
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
        let r = settle_line(&c, &dict, s, "0:0 @ ANA DREAM", 1).unwrap();
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
        let r1 = settle_line(&c, &dict, s, "0:20 @ ANA DREAM", 1).unwrap(); // absolute minute 600
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
        let r = settle_line(&c, &dict, s, "0:0 @ ANA DREAM", 1).unwrap();
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
        let out = format_sentence(&s, &dict).unwrap();
        // 90m canonicalizes to the exact single unit 3halfhour (§5.1)
        assert_eq!(out, "0:0 @ ANA DREAM -> SPAWN DREAD INTO HOUSE expires 3halfhour");
        let s2 = parse_sentence(&out, &dict).unwrap();
        assert_eq!(s2.outcomes, s.outcomes);
        assert_eq!(format_sentence(&s2, &dict).unwrap(), out);
        // a compound with no exact single unit decomposes (§5.1) and still
        // re-parses
        let s3 = parse_sentence("0:0 @ ANA DREAM -> SPAWN DREAD expires 100m", &dict).unwrap();
        let out3 = format_sentence(&s3, &dict).unwrap();
        assert_eq!(out3, "0:0 @ ANA DREAM -> SPAWN DREAD expires 1h40m");
        assert_eq!(parse_sentence(&out3, &dict).unwrap().outcomes, s3.outcomes);
        // the §4.2 quoted form keeps its string and re-quotes
        let s4 = parse_sentence("0:0 @ ANA DREAM -> SPAWN DREAD expires \"2h\"", &dict).unwrap();
        let out4 = format_sentence(&s4, &dict).unwrap();
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
        let r1 = settle_line(&c, &dict, s, "0:0 @ ANA DREAM", 1).unwrap();
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
        let r = settle_line(&c, &dict, s, "0:0 @ ANA DREAM", 1).unwrap();
        assert_eq!(r.rejected, None);
        let id = &r.state.by_name["CHALICE"][0];
        assert_eq!(
            r.state.entities[id.as_str()].attrs.get("expiresAt"),
            Some(&AttrValue::Num(120))
        );
    }
}
