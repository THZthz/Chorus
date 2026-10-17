//! Endings (25 tests) — the ENDING outcome, the `endings:` script section, and game-over
//! discipline (spec §4.4): first ENDING in a step wins; the step completes
//! normally; steps on an ended state are rejected with nothing logged; time
//! triggers and authored expiries may ENDING (the CS despair-death shape);
//! undo reopens the game because `ended` derives from the fold.
//!
//! Every test builds a FRESH Dictionary. BOTH game-over texts are
//! pinned here — the UI notice full text and the engine rejected
//! short text — never merged.

use crate::common::harness::{assert_err_msg, assert_rejected};
use crate::common::rig::ops;
use crate::common::rig::settle_line;
use crate::common::seed::seed_state;
use crate::common::snap::snap;
use chorus::engine::settle::{act, done};

use chorus::content::script::{compile_script, parse_script};
use chorus::content::{
    compile_content, CompiledContent, ContentRaw, EndingDef, ExpireTriggerDef, PrototypeDef,
    RecipeDef, SlotDef, SlotTriggerDef, TriggerDef, VerbContent,
};
use chorus::core::dict::{core_dictionary, Dictionary};
use chorus::core::format::format_sentence;
use chorus::core::parse::parse_sentence;
use chorus::core::r#ref::Ref;
use chorus::core::sentence::Arg;
use chorus::core::types::{clone_state, Ending, State};
use chorus::engine::fold::fold;
use chorus::engine::store::{load_log, save_log};
use chorus::engine::{execute_step, ExecOptions, ExecResult, Step};
use chorus::session::game::{Game, TurnKind};
use chorus::session::render::{ending_banner, ending_notice, prose_lines};
use indexmap::IndexMap;

// ---------------------------------------------------------------- rig ------

fn endings_table() -> IndexMap<String, EndingDef> {
    [
        ("DESPAIR_DEATH", EndingDef { title: "No More".into(), flavour: None }),
        (
            "LANTERN_VICTORY",
            EndingDef { title: "The Light in the Ruins".into(), flavour: Some("grand".into()) },
        ),
    ]
    .into_iter()
    .map(|(k, v)| (k.to_string(), v))
    .collect()
}

/// `extra` rides as post-set
/// fields on the returned raw.
fn wait_content_raw(out: &[&str]) -> ContentRaw {
    ContentRaw {
        verbs: [(
            "WAIT".to_string(),
            VerbContent {
                slots: vec![SlotDef {
                    role: "actors".into(),
                    min: 1,
                    max: Some(1),
                    query: Some("kind:character".into()),
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
                    out: out.iter().map(|s| (*s).to_string()).collect(),
                }],
            },
        )]
        .into_iter()
        .collect(),
        ..ContentRaw::default()
    }
}

/// compile wait_content_raw + the endings table (+ anything the caller adds).
fn wait_content(out: &[&str]) -> ContentRaw {
    let mut raw = wait_content_raw(out);
    raw.endings = Some(endings_table());
    raw
}

fn ended_of(state: &State) -> Ending {
    state.ended.clone().expect("the story has ended")
}

// ---------- the endings table (§10) ----------------------------------------

const SCRIPT: &str = "
script: The Long Fall
player: ANA
start: 0:0

entities:
  HOUSE is place
  ANA is character at HOUSE

endings:
  DESPAIR_DEATH: \"No More\"
  LANTERN_VICTORY: \"The Light in the Ruins\" (grand)

verbs:
  WAIT:
    actors: 1 kind:character
    duration: 1h
    recipes:
      default -> NARRATE \"The candle gutters.\", ENDING DESPAIR_DEATH
";

mod the_endings_script_section {
    use super::*;

    #[test]
    fn the_table_compiles_with_resolved_flavours_default_melancholy() {
        let dict = core_dictionary();
        let game = compile_script(SCRIPT, &dict).unwrap();
        assert_eq!(
            game.content.endings.get("DESPAIR_DEATH"),
            Some(&chorus::content::CompiledEnding {
                title: "No More".into(),
                flavour: "melancholy".into(),
            })
        );
        assert_eq!(
            game.content.endings.get("LANTERN_VICTORY"),
            Some(&chorus::content::CompiledEnding {
                title: "The Light in the Ruins".into(),
                flavour: "grand".into(),
            })
        );
    }

    #[test]
    fn duplicate_ending_ids_fail_at_load() {
        let dict = core_dictionary();
        assert_err_msg(
            parse_script(
                "endings:\n  DESPAIR_DEATH: \"No More\"\n  DESPAIR_DEATH: \"Again\"\n",
                &dict,
            ),
            r"duplicate ending id",
        );
    }

    #[test]
    fn bad_ending_lines_fail_loudly() {
        let dict = core_dictionary();
        assert_err_msg(
            parse_script("endings:\n  DESPAIR_DEATH No More\n", &dict),
            r"endings expect",
        );
        // UPPER_SNAKE only
        assert_err_msg(
            parse_script("endings:\n  despair_death: \"No More\"\n", &dict),
            r"endings expect",
        );
        // title must be quoted
        assert_err_msg(
            parse_script("endings:\n  DESPAIR_DEATH: No More\n", &dict),
            r"endings expect",
        );
    }

    #[test]
    fn compile_failures_ending_with_an_undeclared_id_recipe_and_trigger_alike() {
        let dict = core_dictionary();
        assert_err_msg(
            compile_content(
                &ContentRaw {
                    verbs: [(
                        "WAIT".to_string(),
                        VerbContent {
                            slots: vec![SlotDef {
                                role: "actors".into(),
                                min: 1,
                                max: Some(1),
                                query: None,
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
                                out: vec!["ENDING NOPE".to_string()],
                            }],
                        },
                    )]
                    .into_iter()
                    .collect(),
                    ..ContentRaw::default()
                },
                &dict,
                &[],
            ),
            r"recipe wait\.default: ENDING 'NOPE' is not in the endings table",
        );
        assert_err_msg(
            compile_content(
                &ContentRaw {
                    triggers: Some(vec![TriggerDef::Slot(SlotTriggerDef {
                        id: "t".into(),
                        slot: 1,
                        once: None,
                        out: vec!["ENDING NOPE".to_string()],
                    })]),
                    endings: Some(endings_table()),
                    ..ContentRaw::default()
                },
                &dict,
                &[],
            ),
            r"trigger t: ENDING 'NOPE' is not in the endings table",
        );
        // both trigger forms
        assert_err_msg(
            compile_content(
                &ContentRaw {
                    triggers: Some(vec![TriggerDef::Expire(ExpireTriggerDef {
                        id: "t".into(),
                        on_expire_of: "CHALICE".into(),
                        out: vec!["ENDING NOPE".to_string()],
                    })]),
                    endings: Some(endings_table()),
                    ..ContentRaw::default()
                },
                &dict,
                &[],
            ),
            r"trigger t: ENDING 'NOPE' is not in the endings table",
        );
    }

    #[test]
    fn raw_content_discipline_upper_snake_ids_titled_lowercase_flavour() {
        let dict = core_dictionary();
        let ending = |id: &str, title: &str, flavour: Option<&str>| {
            let mut endings = IndexMap::new();
            endings.insert(
                id.to_string(),
                EndingDef { title: title.to_string(), flavour: flavour.map(str::to_string) },
            );
            ContentRaw { endings: Some(endings), ..ContentRaw::default() }
        };
        assert_err_msg(compile_content(&ending("nope", "x", None), &dict, &[]), r"UPPER_SNAKE");
        assert_err_msg(compile_content(&ending("NOPE", "", None), &dict, &[]), r"needs a title");
        assert_err_msg(compile_content(&ending("NOPE", "   ", None), &dict, &[]), r"needs a title");
        assert_err_msg(
            compile_content(&ending("NOPE", "x", Some("Grand")), &dict, &[]),
            r"lowercase word",
        );
    }

    #[test]
    fn two_scripts_compiled_side_by_side_keep_separate_ending_tables_no_bleed() {
        let dict = core_dictionary();
        let a = compile_script("endings:\n  ONLY_A: \"A Title\"\n", &dict).unwrap().content;
        let b = compile_script("endings:\n  ONLY_B: \"B Title\"\n", &dict).unwrap().content;
        assert!(a.endings.contains_key("ONLY_A"));
        assert!(!a.endings.contains_key("ONLY_B"));
        assert!(!b.endings.contains_key("ONLY_A"));
        // and a cross-script reference is a load-time error, not a silent typo
        assert_err_msg(
            compile_content(
                &ContentRaw {
                    verbs: [(
                        "WAIT".to_string(),
                        VerbContent {
                            slots: vec![SlotDef {
                                role: "actors".into(),
                                min: 1,
                                max: Some(1),
                                query: None,
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
                                out: vec!["ENDING ONLY_B".to_string()],
                            }],
                        },
                    )]
                    .into_iter()
                    .collect(),
                    endings: Some(
                        [(
                            "ONLY_A".to_string(),
                            EndingDef { title: "A Title".into(), flavour: None },
                        )]
                        .into_iter()
                        .collect(),
                    ),
                    ..ContentRaw::default()
                },
                &dict,
                &[],
            ),
            r"ENDING 'ONLY_B' is not in the endings table",
        );
    }
}

// ---------- the ENDING outcome: parse, format, round-trip ------------------

mod the_ending_outcome_paren_less_round_tripping {
    use super::*;

    #[test]
    fn parses_as_a_ref_form_id_and_canonicalizes_to_itself() {
        let dict = core_dictionary();
        let s = parse_sentence("0:0 @ ANA WAIT -> ENDING DESPAIR_DEATH", &dict).unwrap();
        assert_eq!(
            s.outcomes[0],
            chorus::core::sentence::Outcome {
                op: "ENDING".into(),
                args: vec![Arg::Ref {
                    r#ref: Ref::Name { kind: None, name: "DESPAIR_DEATH".into() }
                }],
                arms: None,
                fallback: None,
                call: None,
            }
        );
        assert_eq!(format_sentence(&s, &dict).unwrap(), "0:0 @ ANA WAIT -> ENDING DESPAIR_DEATH");
        // quoted ids round-trip too (an id that collides with a reserved word)
        let q = parse_sentence("0:0 @ ANA WAIT -> ENDING \"WAIT\"", &dict).unwrap();
        assert_eq!(format_sentence(&q, &dict).unwrap(), "0:0 @ ANA WAIT -> ENDING \"WAIT\"");
    }

    #[test]
    fn the_paren_form_is_a_parse_error_paren_less_only() {
        // (§4.4)
        let dict = core_dictionary();
        assert_err_msg(
            parse_sentence("0:0 @ ANA WAIT -> ENDING(DESPAIR_DEATH)", &dict),
            r"paren-less",
        );
    }
}

// ---------- a recipe that ends the story (§4.4) ---------------------------

/// The section-level content
/// (`['NARRATE("The candle gutters.")', 'ENDING DESPAIR_DEATH']`).
fn guttering(dict: &Dictionary) -> CompiledContent {
    compile_content(
        &wait_content(&["NARRATE(\"The candle gutters.\")", "ENDING DESPAIR_DEATH"]),
        dict,
        &[],
    )
    .unwrap()
}

mod a_recipe_that_ends_the_story {
    use super::*;

    #[test]
    fn the_step_applies_ends_the_story_and_the_clock_rides_along() {
        let dict = core_dictionary();
        let content = guttering(&dict);
        let state = seed_state();
        let r = settle_line(&content, &dict, state, "0:28 @ ANA WAIT", 1).unwrap();
        assert_eq!(r.rejected, None);
        assert_eq!(
            ended_of(&r.state),
            Ending {
                id: "DESPAIR_DEATH".into(),
                title: "No More".into(),
                flavour: "melancholy".into(),
            }
        );
        assert_eq!(r.state.now, 14 * 60 + 60); // the ending does not stop the clock
        assert_eq!(act(&r).unwrap().advanced_min, 60);
        assert_eq!(ops(done(&r).unwrap()), ["NARRATE", "ENDING"]);
    }

    #[test]
    fn the_record_carries_the_concrete_ending_and_round_trips() {
        let dict = core_dictionary();
        let content = guttering(&dict);
        let state = seed_state();
        let r = settle_line(&content, &dict, state, "0:28 @ ANA WAIT", 1).unwrap();
        let rec = done(&r).unwrap();
        assert!(rec.outcomes.iter().any(|o| o.op == "ENDING"));
        assert!(rec.sentence.contains("-> NARRATE \"The candle gutters.\", ENDING DESPAIR_DEATH"));
        assert_eq!(
            format_sentence(&parse_sentence(&rec.sentence, &dict).unwrap(), &dict).unwrap(),
            rec.sentence
        );
    }

    #[test]
    fn explicit_flavour_resolves_through_first_ending_in_one_step_wins() {
        let dict = core_dictionary();
        let two_endings = compile_content(
            &wait_content(&["ENDING LANTERN_VICTORY", "ENDING DESPAIR_DEATH"]),
            &dict,
            &[],
        )
        .unwrap();
        let r = settle_line(&two_endings, &dict, seed_state(), "0:28 @ ANA WAIT", 1).unwrap();
        assert_eq!(r.rejected, None);
        assert_eq!(
            ended_of(&r.state),
            Ending {
                id: "LANTERN_VICTORY".into(),
                title: "The Light in the Ruins".into(),
                flavour: "grand".into(),
            }
        );
        // both are logged; only the first moved the state
        assert_eq!(ops(done(&r).unwrap()), ["ENDING", "ENDING"]);
    }

    #[test]
    fn further_steps_are_rejected_with_nothing_logged_the_state_object_untouched() {
        let dict = core_dictionary();
        let content = guttering(&dict);
        let state = seed_state();
        let r1 = settle_line(&content, &dict, state, "0:28 @ ANA WAIT", 1).unwrap();
        let before = snap(&r1.state);
        // reference identity `r2.state === r1.state` — execute_step hands
        // the ORIGINAL back untouched on a rejection, so snap equality IS the
        // semantic content of the check
        let r2 =
            settle_line(&content, &dict, clone_state(&r1.state), "0:30 @ ANA WAIT", 2).unwrap();
        assert_rejected(&r2, r"the story has ended — No More");
        assert_eq!(r2.records.len(), 0);
        assert_eq!(snap(&r2.state), before);
    }

    #[test]
    fn a_refuse_after_the_ending_discards_the_ending_with_the_whole_draft() {
        // (§6.5)
        let dict = core_dictionary();
        let c = compile_content(
            &wait_content(&["ENDING DESPAIR_DEATH", "REFUSE(\"not yet\")"]),
            &dict,
            &[],
        )
        .unwrap();
        let state = seed_state();
        let before = snap(&state);
        let r = settle_line(&c, &dict, state, "0:28 @ ANA WAIT", 1).unwrap();
        assert_eq!(ops(done(&r).unwrap()), ["ENDING", "REFUSE"]);
        // reference identity `r.state === state` — the refused turn returns the
        // untouched input state; snap equality is its Rust observable
        assert_eq!(snap(&r.state), before);
        assert!(r.state.ended.is_none());
        // and the discarded ending is never announced in prose (§6.5)…
        let lines = prose_lines(act(&r).unwrap(), &c, &dict).unwrap();
        assert!(lines.iter().any(|l| l.contains("not yet"))); // the refusal itself is spoken (§6.2)
        assert!(!lines.join("\n").contains("— No More —"));
    }

    #[test]
    fn the_game_over_guard_is_first_stale_clocks_event_sentences_multi_clause_steps_all_bounce() {
        let dict = core_dictionary();
        let content = guttering(&dict);
        let state = seed_state();
        let r1 = settle_line(&content, &dict, state, "0:28 @ ANA WAIT", 1).unwrap();
        assert!(r1.state.ended.is_some());
        // a stale clock (would be a §9.2 rejection) and a multi-clause step
        // would both get past their own checks — the ended guard must precede
        // them
        let mut attempts: Vec<ExecResult> = vec![
            settle_line(&content, &dict, clone_state(&r1.state), "0:10 @ ANA WAIT", 2).unwrap(),
            settle_line(&content, &dict, clone_state(&r1.state), "0:30 @ ANA WAIT; ANA WAIT", 2)
                .unwrap(),
        ];
        // an EVENT-typed sentence reaches execute_step with zero clauses
        // (canonical passthrough)
        let ev = parse_sentence("0:40 @ EVENT dusk_falls -> ENDING DESPAIR_DEATH", &dict).unwrap();
        attempts.push(
            execute_step(
                &content,
                &dict,
                clone_state(&r1.state),
                Step {
                    start_min: None,
                    clock: Some(
                        ev.clock.unwrap_or(chorus::core::clock::Clock { day: 0, halfhour: 0 }),
                    ),
                    clauses: ev.clauses,
                    t_real: None,
                },
                ExecOptions { first_record_id: 2, id_gen: None },
            )
            .unwrap(),
        );
        for a in &attempts {
            assert_rejected(a, r"the story has ended — No More");
            assert_eq!(a.records.len(), 0); // nothing logged, never a throw
                                            // reference identity `a.state === r1.state` — snap equality (untouched)
            assert_eq!(snap(&a.state), snap(&r1.state));
        }
    }

    #[test]
    fn fold_equality_incl_ended_through_jsonl() {
        let dict = core_dictionary();
        let content = guttering(&dict);
        let state = seed_state();
        let r1 = settle_line(&content, &dict, state, "0:28 @ ANA WAIT", 1).unwrap();
        // rejected; nothing logged
        let r2 =
            settle_line(&content, &dict, clone_state(&r1.state), "0:30 @ ANA WAIT", 2).unwrap();
        assert_eq!(r2.records.len(), 0);
        let dir = std::env::temp_dir().join(format!("chorus-endings-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("log.jsonl");
        save_log(&path, &r1.records).unwrap();
        let replayed = fold(&load_log(&path).unwrap(), &seed_state(), &dict, &content).unwrap();
        let _ = std::fs::remove_dir_all(&dir);
        assert_eq!(snap(&replayed), snap(&r1.state));
        assert_eq!(
            replayed.ended,
            Some(Ending {
                id: "DESPAIR_DEATH".into(),
                title: "No More".into(),
                flavour: "melancholy".into(),
            })
        );
    }
}

// ---------- events may end the game (§4.4) --------------------------------

mod events_may_end_the_game {
    use super::*;
    use chorus::core::types::AttrValue;

    #[test]
    fn a_time_trigger_chain_ends_the_game_when_the_clock_crosses_the_despair_death_shape() {
        let dict = core_dictionary();
        let mut raw = wait_content(&["NARRATE(\"{actors} wait.\")"]);
        raw.triggers = Some(vec![TriggerDef::Slot(SlotTriggerDef {
            id: "despair".into(),
            slot: 32, // 16:00
            once: None,
            out: vec!["SPAWN DREAD".to_string(), "ENDING DESPAIR_DEATH".to_string()],
        })]);
        let mut prototypes = IndexMap::new();
        prototypes.insert(
            "DREAD".to_string(),
            PrototypeDef { kind: "state".into(), ..PrototypeDef::default() },
        );
        raw.prototypes = Some(prototypes);
        let c = compile_content(&raw, &dict, &[]).unwrap();
        let state = seed_state();
        // 15:00 → 16:00, crossing slot 32
        let r = settle_line(&c, &dict, state, "0:30 @ ANA WAIT", 1).unwrap();
        assert_eq!(r.rejected, None);
        let event = r.records.iter().find(|x| x.kind == "event").expect("the trigger fired");
        assert_eq!(event.trigger.as_deref(), Some("despair"));
        assert!(event.outcomes.iter().any(|o| o.op == "ENDING"));
        assert_eq!(
            ended_of(&r.state),
            Ending {
                id: "DESPAIR_DEATH".into(),
                title: "No More".into(),
                flavour: "melancholy".into(),
            }
        );
        // the fold trusts the EVENT row: replaying alone re-ends the story
        let replayed = fold(&r.records, &seed_state(), &dict, &c).unwrap();
        assert_eq!(snap(&replayed), snap(&r.state));
    }

    #[test]
    fn an_authored_expiry_trigger_may_ending_and_must_consume_the_card_itself() {
        // (§6.6)
        let dict = core_dictionary();
        let mut raw = wait_content(&["NARRATE(\"{actors} wait.\")"]);
        // the authored outcomes replace the built-in destroy entirely (§6.6),
        // so the despair-death author destroys the dying card explicitly
        raw.triggers = Some(vec![TriggerDef::Expire(ExpireTriggerDef {
            id: "the_lights_go_out".into(),
            on_expire_of: "CHALICE".into(),
            out: vec![
                "DESTROY CHALICE".to_string(),
                "NARRATE(\"The candle drowns in its own wax.\")".to_string(),
                "ENDING DESPAIR_DEATH".to_string(),
            ],
        })]);
        let c = compile_content(&raw, &dict, &[]).unwrap();
        let mut base = seed_state();
        base.entities
            .get_mut("chalice")
            .unwrap()
            .attrs
            .insert("expiresAt".to_string(), AttrValue::Num(15 * 60)); // 0:30
                                                                       // 14:00 → 16:00, expiry at 15:00
        let r = settle_line(&c, &dict, clone_state(&base), "0:28 @ ANA WAIT; ANA WAIT", 1).unwrap();
        assert_eq!(r.rejected, None);
        let event = r.records.iter().find(|x| x.kind == "event").expect("the expiry fired");
        assert_eq!(event.trigger.as_deref(), Some("the_lights_go_out"));
        assert_eq!(r.state.entities.get("chalice").unwrap().dead_action, Some(event.id));
        assert_eq!(
            ended_of(&r.state),
            Ending {
                id: "DESPAIR_DEATH".into(),
                title: "No More".into(),
                flavour: "melancholy".into(),
            }
        );
        assert_eq!(
            snap(&fold(&r.records, &clone_state(&base), &dict, &c).unwrap()),
            snap(&r.state)
        );
    }

    #[test]
    fn an_authored_expiry_trigger_that_does_not_destroy_its_card_re_fires() {
        // (§6.6, §4.4)
        let dict = core_dictionary();
        let mut raw = wait_content(&["NARRATE(\"{actors} wait.\")"]);
        // the authored outcomes replace the built-in destroy entirely — forget
        // the DESTROY and the card survives, re-firing on every later crossing
        raw.triggers = Some(vec![TriggerDef::Expire(ExpireTriggerDef {
            id: "flicker".into(),
            on_expire_of: "CHALICE".into(),
            out: vec!["NARRATE(\"the candle drowns.\")".to_string()],
        })]);
        let c = compile_content(&raw, &dict, &[]).unwrap();
        let mut base = seed_state();
        base.entities
            .get_mut("chalice")
            .unwrap()
            .attrs
            .insert("expiresAt".to_string(), AttrValue::Num(15 * 60)); // 0:30
                                                                       // 14:00 → 15:00 — the expiry crossing
        let r1 = settle_line(&c, &dict, clone_state(&base), "0:28 @ ANA WAIT", 1).unwrap();
        assert_eq!(
            r1.records
                .iter()
                .filter(|x| x.kind == "event" && x.trigger.as_deref() == Some("flicker"))
                .count(),
            1
        );
        // 15:00 → 16:00 — still past expiresAt
        let r2 = settle_line(&c, &dict, r1.state, "0:30 @ ANA WAIT", 3).unwrap();
        assert!(
            r2.records
                .iter()
                .filter(|x| x.kind == "event" && x.trigger.as_deref() == Some("flicker"))
                .count()
                >= 1
        );
        assert_eq!(
            r2.state.entities.get("chalice").unwrap().dead_action,
            None // never consumed
        );
    }

    #[test]
    fn action_outranks_event_on_first_wins_within_one_advance() {
        let dict = core_dictionary();
        let mut raw = wait_content(&["ENDING LANTERN_VICTORY"]);
        raw.triggers = Some(vec![TriggerDef::Slot(SlotTriggerDef {
            id: "despair".into(),
            slot: 32,
            once: None,
            out: vec!["ENDING DESPAIR_DEATH".to_string()],
        })]);
        let c = compile_content(&raw, &dict, &[]).unwrap();
        // the step crosses slot 32
        let r = settle_line(&c, &dict, seed_state(), "0:30 @ ANA WAIT", 1).unwrap();
        assert_eq!(r.rejected, None);
        // the action's ENDING applied first
        assert_eq!(r.state.ended.as_ref().unwrap().id, "LANTERN_VICTORY");
    }
}

// ---------- the player surface (§11) ---------------------------------------

const GAME_SCRIPT: &str = "
script: The Long Fall
player: ANA
start: 0:0

entities:
  HOUSE is place
  ANA is character at HOUSE

endings:
  DESPAIR_DEATH: \"No More\"
  LANTERN_VICTORY: \"The Light in the Ruins\" (grand)

verbs:
  WAIT:
    actors: 1 kind:character
    duration: 1h
    recipes:
      default -> NARRATE \"The candle gutters.\", ENDING DESPAIR_DEATH

templates:
  WAIT says \"{actors} wait.\"
";

mod the_player_surface {
    use super::*;

    #[test]
    fn the_ending_turn_tells_the_closing_line_the_journal_stays_canonical() {
        let dict = core_dictionary();
        let mut game = Game::from_script(GAME_SCRIPT, dict).unwrap();
        let turn = game.handle_line("wait").unwrap();
        assert_eq!(turn.kind, TurnKind::Action);
        // v0.5: the ENDING lands when the window completes
        let settle = game.wait().unwrap();
        assert!(settle.lines.join("\n").contains("— No More —"));
        assert_eq!(
            ended_of(&game.state),
            Ending {
                id: "DESPAIR_DEATH".into(),
                title: "No More".into(),
                flavour: "melancholy".into(),
            }
        );
        assert!(game.journal().join("\n").contains("ENDING DESPAIR_DEATH"));
        assert!(game.story().unwrap().join("\n").contains("— No More —"));
    }

    #[test]
    fn after_the_ending_the_notice_instead_of_execution_and_no_suggestions() {
        let dict = core_dictionary();
        let mut game = Game::from_script(GAME_SCRIPT, dict).unwrap();
        game.handle_line("wait").unwrap();
        game.wait().unwrap();
        let n_records = game.log.records.len();
        let turn = game.handle_line("wait").unwrap();
        assert_eq!(turn.kind, TurnKind::Info);
        assert_eq!(turn.lines, vec![ending_notice(game.state.ended.as_ref().unwrap())]);
        assert_eq!(game.log.records.len(), n_records); // nothing logged
        assert_eq!(game.suggestions().unwrap(), Vec::<String>::new());
    }

    #[test]
    fn undo_across_the_ending_step_reopens_the_game_ended_derives_from_the_fold() {
        let dict = core_dictionary();
        let mut game = Game::from_script(GAME_SCRIPT, dict).unwrap();
        game.handle_line("wait").unwrap();
        game.wait().unwrap();
        assert!(game.state.ended.is_some());
        assert_eq!(game.undo().unwrap().kind, TurnKind::Info);
        // …and its begin — no stale window stays open
        assert_eq!(game.undo().unwrap().kind, TurnKind::Info);
        // no special-case: re-fold before the ending record
        assert!(game.state.ended.is_none());
        let turn = game.handle_line("wait").unwrap();
        assert_eq!(turn.kind, TurnKind::Action); // play resumes
                                                 // v0.5: the ENDING lands when the window completes
        game.wait().unwrap();
        assert_eq!(
            ended_of(&game.state),
            Ending {
                id: "DESPAIR_DEATH".into(),
                title: "No More".into(),
                flavour: "melancholy".into(),
            }
        );
    }

    #[test]
    fn the_banner_and_the_notice_render_title_plus_flavour() {
        assert_eq!(
            ending_banner(&Ending {
                id: "DESPAIR_DEATH".into(),
                title: "No More".into(),
                flavour: "melancholy".into(),
            }),
            "— NO MORE · melancholy —"
        );
        assert_eq!(
            ending_banner(&Ending {
                id: "LANTERN_VICTORY".into(),
                title: "The Light in the Ruins".into(),
                flavour: "grand".into(),
            }),
            "— THE LIGHT IN THE RUINS · grand —"
        );
        // flavour is open lowercase vocabulary — any word renders, no crash
        assert_eq!(
            ending_banner(&Ending {
                id: "QUIET_END".into(),
                title: "Hush".into(),
                flavour: "quiet".into(),
            }),
            "— HUSH · quiet —"
        );
        // the UI notice FULL text (the engine's rejected short text —
        // `the story has ended — No More` without flavour — is pinned above
        // and lives in engine.rs; the two are never merged)
        assert_eq!(
            ending_notice(&Ending {
                id: "DESPAIR_DEATH".into(),
                title: "No More".into(),
                flavour: "melancholy".into(),
            }),
            "the story has ended — No More (melancholy)."
        );
    }

    #[test]
    fn the_prose_of_the_ending_step_carries_the_closing_line_others_do_not() {
        // Outcomes come from the fired recipe
        // (execute_step ignores a sentence's own `->` tail), so author the
        // ending in the recipe
        let dict = core_dictionary();
        let content = compile_content(
            &wait_content(&["NARRATE(\"last light.\")", "ENDING DESPAIR_DEATH"]),
            &dict,
            &[],
        )
        .unwrap();
        let r = settle_line(&content, &dict, seed_state(), "0:28 @ ANA WAIT", 1).unwrap();
        // v0.5: the ending narrates at the completion
        let lines = prose_lines(done(&r).unwrap(), &content, &dict).unwrap();
        assert_eq!(lines[lines.len() - 1], "— No More —");
        let plain_r = settle_line(
            &compile_content(&wait_content(&["NARRATE(\"just waiting.\")"]), &dict, &[]).unwrap(),
            &dict,
            seed_state(),
            "0:28 @ ANA WAIT",
            1,
        )
        .unwrap();
        let plain = prose_lines(&plain_r.records[0], &content, &dict).unwrap();
        assert!(!plain.join("\n").contains("— No More —"));
    }

    #[test]
    fn clone_state_copies_the_ending_undo_safe_projections() {
        let dict = core_dictionary();
        let mut game = Game::from_script(GAME_SCRIPT, dict).unwrap();
        game.handle_line("wait").unwrap();
        game.wait().unwrap();
        let mut c = clone_state(&game.state);
        assert_eq!(c.ended, game.state.ended);
        c.ended.as_mut().unwrap().title = "mutated".to_string();
        assert_eq!(game.state.ended.as_ref().unwrap().title, "No More");
    }
}
