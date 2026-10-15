//! Parallel verb stations (13 tests, v0.5) — the feature's own acceptance suite:
//! windows overlap, stations and cards lock, completions resolve in a
//! deterministic order, and the fold rebuilds open windows byte-for-byte.
//!
//! Differences from the engine.rs rig: this
//! file's `run` is execute_step ONLY (no settle adapter) and its lone-clock
//! branch is a pure advance. Rig verbs are
//! code-registered into a shared Dictionary; each test
//! owns a fresh Dictionary.

mod common;

use common::seed::{seed_content, seed_state};
use common::snap::snap;

use chorus::content::content::{
    compile_content, CompiledContent, ContentRaw, PrototypeDef, RecipeDef, SlotDef, TriggerDef,
    VerbContent,
};
use chorus::core::clock::parse_clock;
use chorus::core::dict::{core_dictionary, Dictionary};
use chorus::core::parse::parse_sentence;
use chorus::core::sentence::Sentence;
use chorus::core::types::{clone_state, AttrValue, State};
use chorus::engine::engine::{execute_step, ActionRecord, ExecResult, ExecOptions, Step};
use chorus::engine::fold::fold;
use chorus::engine::store::{load_log, save_log};
use indexmap::IndexMap;

// ---------------------------------------------------------------- rig ------

static LONE_CLOCK: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();

fn lone_clock() -> &'static regex::Regex {
    LONE_CLOCK.get_or_init(|| regex::Regex::new(r"^\d+:\d+$").unwrap())
}

/// A lone clock token is a pure advance;
/// otherwise parse and execute. No id generator is passed, so the engine's
/// deterministic default mint is in force here.
fn run(
    content: &CompiledContent,
    dict: &Dictionary,
    state: State,
    src: &str,
    first_record_id: i64,
) -> ExecResult {
    if lone_clock().is_match(src) {
        return execute_step(
            content,
            dict,
            state,
            Step {
                start_min: None,
                clock: Some(parse_clock(src).unwrap()),
                clauses: vec![],
                t_real: None,
            },
            ExecOptions {
                first_record_id,
                id_gen: None,
            },
        )
        .unwrap();
    }
    let s = parse_sentence(src, dict).unwrap();
    execute_step(content, dict, state, step_of(&s), ExecOptions {
        first_record_id,
        id_gen: None,
    })
    .unwrap()
}

fn step_of(s: &Sentence) -> Step {
    Step {
        start_min: None,
        clock: Some(s.clock.unwrap_or(chorus::core::clock::Clock { day: 0, halfhour: 0 })),
        clauses: s.clauses.clone(),
        t_real: None,
    }
}

/// The rig content — rig verbs are code-registered (§10).
fn rig_content(
    dict: &mut Dictionary,
    verbs: Vec<(&str, VerbContent)>,
    extra: impl FnOnce(&mut ContentRaw),
) -> CompiledContent {
    let mut map = IndexMap::new();
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
    compile_content(&raw, dict, &[]).unwrap()
}

fn actor_slot() -> SlotDef {
    SlotDef {
        role: "actors".to_string(),
        min: 1,
        max: Some(1),
        query: Some("kind:character".to_string()),
        shown: None,
    }
}

fn object_slot() -> SlotDef {
    SlotDef {
        role: "object".to_string(),
        min: 0,
        max: Some(1),
        query: None,
        shown: None,
    }
}

fn narr_recipe(out: &str) -> Vec<RecipeDef> {
    vec![RecipeDef {
        id: None,
        when: None,
        shown: None,
        hint: None,
        max: None,
        out: vec![out.to_string()],
    }]
}

/// Two stations, two durations, no side effects
/// — the geometry rig.
fn geo(dict: &mut Dictionary, a_dur: i64, b_dur: i64) -> CompiledContent {
    rig_content(
        dict,
        vec![
            (
                "PACE_A",
                VerbContent {
                    slots: vec![actor_slot()],
                    duration: a_dur,
                    recipes: narr_recipe("NARRATE \"a done.\""),
                },
            ),
            (
                "PACE_B",
                VerbContent {
                    slots: vec![actor_slot()],
                    duration: b_dur,
                    recipes: narr_recipe("NARRATE \"b done.\""),
                },
            ),
        ],
        |_| {},
    )
}

/// The shared seed state.
fn seed() -> State {
    seed_state()
}

/// A temp-file JSONL round trip.
fn save_log_tmp(tag: &str, records: &[ActionRecord]) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("chorus-parallel-{}-{tag}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("log.jsonl");
    save_log(&path, records).unwrap();
    path
}

fn assert_rejected(r: &ExecResult, pattern: &str) {
    let msg = r
        .rejected
        .as_deref()
        .unwrap_or_else(|| panic!("expected a rejected step matching /{pattern}/, got None"));
    let re = regex::Regex::new(pattern).unwrap();
    assert!(re.is_match(msg), "rejected {msg:?} does not match /{pattern}/");
}

// ------------------------------------------ parallel windows — the model ------

/// Parallel windows — the model: overlapping windows, stations, locks, and
/// deterministic completion order.
mod parallel_windows_the_model {
    use super::*;

    #[test]
    fn two_verbs_run_overlapping_windows_the_clock_stays_put_until_an_advance() {
        let mut dict = core_dictionary();
        let mut state = seed();
        let r1 = run(&geo(&mut dict, 60, 30), &dict, state, "0:0 @ ANA PACE_A", 1); // window [0, 60]
        assert_eq!(r1.rejected, None);
        state = r1.state;
        assert_eq!(state.windows.len(), 1);
        assert_eq!(state.now, 0); // beginning moves nothing
        let geo6030 = geo(&mut dict, 60, 30);
        let r2 = run(
            &geo6030,
            &dict,
            state,
            "0:0 @ ANA PACE_B",
            r1.records.len() as i64 + 1,
        ); // window [0, 30] — IN PARALLEL
        assert_eq!(r2.rejected, None);
        state = r2.state;
        assert_eq!(state.windows.len(), 2);
        assert_eq!(state.now, 0);
        // the fold rebuilds both windows byte-for-byte from the log alone
        let geo_content = geo6030;
        let mut all = r1.records.clone();
        all.extend(r2.records.clone());
        assert_eq!(
            snap(&fold(&all, &clone_state(&seed()), &dict, &geo_content).unwrap()),
            snap(&state)
        );
    }

    #[test]
    fn a_bare_clock_advance_resolves_due_completions_before_the_slot_events() {
        let mut dict = core_dictionary();
        let mut state = seed();
        let chime = rig_content(
            &mut dict,
            vec![(
                "PACE_A",
                VerbContent {
                    slots: vec![actor_slot()],
                    duration: 60,
                    recipes: narr_recipe("NARRATE \"a done.\""),
                },
            )],
            |raw| {
                raw.triggers = Some(vec![TriggerDef::Slot(chorus::content::content::SlotTriggerDef {
                    id: "the_chime".to_string(),
                    slot: 2,
                    once: None,
                    out: vec!["NARRATE \"the halfhour chime sings.\"".to_string()],
                })]);
            },
        );
        let r = run(&chime, &dict, state, "0:0 @ ANA PACE_A", 1); // window [0, 60]; slot 2 = minute 60
        let begin_step_now = r.state.now;
        state = r.state;
        assert_eq!(state.windows.len(), 1);
        let adv = run(&chime, &dict, state, "0:2", r.records.len() as i64 + 1); // pure advance to minute 60
        assert_eq!(adv.rejected, None);
        // v0.5 order: the completion at minute 60 fires BEFORE slot-60 events
        assert_eq!(
            adv.records.iter().map(|x| x.kind.as_str()).collect::<Vec<_>>(),
            ["complete", "event", "wait"]
        );
        assert_eq!(adv.records[0].kind, "complete");
        assert_eq!(adv.records[1].trigger.as_deref(), Some("the_chime"));
        assert_eq!(begin_step_now, 0); // the returned state of the BEGIN step — the advance's own state is below
        assert_eq!(adv.state.now, 60);
        // the advance is one atomic turn ending with the clock row
        assert_eq!(adv.records.last().unwrap().kind, "wait");
    }

    #[test]
    fn an_advance_pass_is_chronological_earlier_completion_first_tie_goes_to_the_completion() {
        let mut dict = core_dictionary();
        let c = rig_content(
            &mut dict,
            vec![
                (
                    "SLOW",
                    VerbContent {
                        slots: vec![actor_slot()],
                        duration: 90,
                        recipes: narr_recipe("NARRATE \"slow.\""),
                    },
                ),
                (
                    "FAST",
                    VerbContent {
                        slots: vec![actor_slot()],
                        duration: 30,
                        recipes: narr_recipe("NARRATE \"fast.\""),
                    },
                ),
            ],
            |raw| {
                raw.triggers = Some(vec![
                    TriggerDef::Slot(chorus::content::content::SlotTriggerDef {
                        id: "halfhour_mark".to_string(),
                        slot: 1,
                        once: None,
                        out: vec!["NARRATE \"mark.\"".to_string()],
                    }),
                    TriggerDef::Slot(chorus::content::content::SlotTriggerDef {
                        id: "ninety_mark".to_string(),
                        slot: 3,
                        once: None,
                        out: vec!["NARRATE \"ninety.\"".to_string()],
                    }),
                ]);
            },
        );
        let mut state = seed();
        let r1 = run(&c, &dict, state, "0:0 @ ANA SLOW", 1);
        let r2 = run(&c, &dict, r1.state, "0:0 @ ANA FAST", r1.records.len() as i64 + 1);
        state = r2.state;
        let adv = run(
            &c,
            &dict,
            state,
            "0:3",
            (r1.records.len() + r2.records.len()) as i64 + 1,
        ); // minute 90
        assert_eq!(adv.rejected, None);
        // FAST closes at minute 30 (before the halfhour mark), SLOW at 90
        // (before the ninety mark) — completions lead, same-minute events follow
        let shapes: Vec<(String, Option<String>)> = adv
            .records
            .iter()
            .map(|x| {
                (
                    x.kind.clone(),
                    x.trigger
                        .clone()
                        .or_else(|| if x.verb.is_empty() { None } else { Some(x.verb.clone()) }),
                )
            })
            .collect();
        assert_eq!(
            shapes,
            vec![
                ("complete".to_string(), Some("FAST".to_string())),
                ("event".to_string(), Some("halfhour_mark".to_string())),
                ("complete".to_string(), Some("SLOW".to_string())),
                ("event".to_string(), Some("ninety_mark".to_string())),
                ("wait".to_string(), None),
            ]
        );
        assert_eq!(adv.state.now, 90);
    }
}

/// The locks: a committed card is busy in any role.
mod the_locks {
    use super::*;

    /// The lock rig content.
    fn lock_rig(dict: &mut Dictionary) -> CompiledContent {
        rig_content(
            dict,
            vec![
                (
                    "SEIZE",
                    VerbContent {
                        slots: vec![actor_slot(), object_slot()],
                        duration: 60,
                        recipes: narr_recipe("NARRATE \"seized.\""),
                    },
                ),
                (
                    "TOUCH",
                    VerbContent {
                        slots: vec![actor_slot(), object_slot()],
                        duration: 30,
                        recipes: narr_recipe("NARRATE \"touched.\""),
                    },
                ),
            ],
            |_| {},
        )
    }

    #[test]
    fn station_exclusivity_the_same_verb_cannot_open_a_second_window() {
        let mut dict = core_dictionary();
        let mut state = seed();
        let c1 = lock_rig(&mut dict);
        let r1 = run(&c1, &dict, state, "0:0 @ ANA SEIZE", 1);
        state = r1.state;
        let c2 = lock_rig(&mut dict);
        let r2 = run(&c2, &dict, clone_state(&state), "0:0 @ ANA SEIZE", r1.records.len() as i64 + 1);
        assert_rejected(&r2, r"SEIZE is already processing");
        assert_eq!(r2.records.len(), 0);
        // `r2.state === state` — the untouched original; the
        // Rust observable is value equality through snap
        assert_eq!(snap(&r2.state), snap(&state));
    }

    #[test]
    fn committed_cards_are_busy_in_any_role_actors_stay_free() {
        let mut dict = core_dictionary();
        let mut state = seed();
        let c1 = lock_rig(&mut dict);
        let r1 = run(&c1, &dict, state, "0:0 @ ANA SEIZE MAGICK_BOOK", 1);
        state = r1.state;
        // the BOOK is locked — as object of another verb
        let c2 = lock_rig(&mut dict);
        let r2 = run(
            &c2,
            &dict,
            clone_state(&state),
            "0:0 @ ANA TOUCH MAGICK_BOOK",
            r1.records.len() as i64 + 1,
        );
        assert_rejected(&r2, r"MAGICK BOOK is busy with SEIZE");
        // …but the ACTOR is never locked: ANA can act with other cards
        let c3 = lock_rig(&mut dict);
        let r3 = run(&c3, &dict, state, "0:0 @ ANA TOUCH", r1.records.len() as i64 + 1);
        assert_eq!(r3.rejected, None);
        assert_eq!(r3.state.windows.len(), 2);
    }

    #[test]
    fn locks_release_the_moment_a_pass_resolves_their_window() {
        let mut dict = core_dictionary();
        let mut state = seed();
        let c1 = lock_rig(&mut dict);
        let r1 = run(&c1, &dict, state, "0:0 @ ANA SEIZE MAGICK_BOOK", 1);
        state = r1.state;
        // an explicit future clock first advances (the lock frees at the
        // begin minute), then begins — the card is usable exactly when free
        let c2 = lock_rig(&mut dict);
        let r2 = run(
            &c2,
            &dict,
            state,
            "1:0 @ ANA TOUCH MAGICK_BOOK",
            r1.records.len() as i64 + 1,
        );
        assert_eq!(r2.rejected, None);
        assert_eq!(r2.records[0].kind, "complete"); // the SEIZE window closed en route
    }

    #[test]
    fn events_do_not_respect_locks_an_expiry_destroys_a_locked_card_and_the_completion_drops_the_ref() {
        let mut dict = core_dictionary();
        let c = rig_content(
            &mut dict,
            vec![(
                "SEIZE",
                VerbContent {
                    slots: vec![actor_slot(), object_slot()],
                    duration: 60,
                    recipes: vec![RecipeDef {
                        id: None,
                        when: None,
                        shown: None,
                        hint: None,
                        max: None,
                        out: vec!["SET(OBJECT, aspect:touched, 1), DESTROY OBJECT".to_string()],
                    }],
                },
            )],
            |_| {},
        );
        let mut state = seed();
        let r1 = run(&c, &dict, state, "0:0 @ ANA SEIZE MAGICK_BOOK", 1);
        state = r1.state;
        // the book expires mid-window — nothing stops the crossing (locks
        // bind steps, not events, §6.7 v0.5)
        let book_id = state.by_name.get("MAGICK_BOOK").unwrap()[0].clone();
        state
            .entities
            .get_mut(&book_id)
            .unwrap()
            .attrs
            .insert("expiresAt".to_string(), AttrValue::Num(30));
        let adv = run(&c, &dict, state, "1:0", r1.records.len() as i64 + 1);
        assert_eq!(adv.rejected, None);
        let complete = adv
            .records
            .iter()
            .find(|x| x.kind == "complete")
            .expect("the completion row");
        // the completion applied with the ref dropped: the SET found no
        // targets and the DESTROY concretized to an empty, harmless row
        assert_eq!(
            complete.outcomes.iter().map(|o| o.op.as_str()).collect::<Vec<_>>(),
            ["DESTROY"]
        );
        assert!(complete.outcomes[0].args.is_empty());
        assert!(!adv.state.by_name.contains_key("MAGICK_BOOK")); // the expiry took it
    }
}

/// Commit-time gates vs completion-time refusals.
mod commit_time_gates_vs_completion_time_refusals {
    use super::*;

    #[test]
    fn a_top_level_refuse_still_rejects_at_the_table_zero_time_attempt_logged() {
        let mut dict = core_dictionary();
        let c = rig_content(
            &mut dict,
            vec![(
                "GATE",
                VerbContent {
                    slots: vec![actor_slot()],
                    duration: 60,
                    recipes: narr_recipe("REFUSE(\"not without a lantern.\")"),
                },
            )],
            |_| {},
        );
        let state = seed();
        let before = snap(&state);
        let r = run(&c, &dict, state, "0:0 @ ANA GATE", 1);
        assert_eq!(r.rejected, None);
        assert_eq!(r.records.len(), 1);
        assert!(r.records[0].outcomes.iter().any(|o| o.op == "REFUSE"));
        assert_eq!(r.records[0].advanced_min, 0);
        // `r.state === state` — untouched; snap equality
        assert_eq!(snap(&r.state), before);
    }

    #[test]
    fn an_if_branch_refuse_defers_the_window_opens_the_completion_refuses_effects_discarded() {
        let mut dict = core_dictionary();
        let c = rig_content(
            &mut dict,
            vec![(
                "LATE",
                VerbContent {
                    slots: vec![actor_slot()],
                    duration: 30,
                    recipes: vec![RecipeDef {
                        id: None,
                        when: None,
                        shown: None,
                        hint: None,
                        max: None,
                        out: vec![
                            "IF exists(HOUSE) -> (SPAWN PARANOIA, REFUSE(\"too late.\")) ELSE -> (NARRATE \"calm.\")"
                                .to_string(),
                        ],
                    }],
                },
            )],
            |raw| {
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
            },
        );
        let mut state = seed();
        let r = run(&c, &dict, state, "0:0 @ ANA LATE", 1);
        assert_eq!(r.rejected, None);
        let r_state = clone_state(&r.state);
        state = r.state;
        assert_eq!(state.windows.len(), 1); // the window DID open
        let adv = run(&c, &dict, state, "0:31", r.records.len() as i64 + 1);
        assert_eq!(adv.rejected, None);
        let complete = adv
            .records
            .iter()
            .find(|x| x.kind == "complete")
            .expect("the completion row");
        assert!(complete.outcomes.iter().any(|o| o.op == "REFUSE"));
        assert!(!adv.state.by_name.contains_key("PARANOIA")); // the scratch died with the refusal
        assert_eq!(adv.state.windows.len(), 0); // …and the window still closed
        // the fold agrees: the REFUSE row is a state no-op that closes its window
        assert_eq!(
            snap(&fold(&adv.records, &r_state, &dict, &c).unwrap()),
            snap(&adv.state)
        );
    }
}

/// Completion-time failures refuse
/// the window — never a livelock.
mod completion_time_failures_never_a_livelock {
    use super::*;

    #[test]
    fn a_completion_that_cannot_apply_closes_the_window_and_logs_the_attempt() {
        let mut dict = core_dictionary();
        let c = rig_content(
            &mut dict,
            vec![(
                "BAD",
                VerbContent {
                    slots: vec![actor_slot()],
                    duration: 30,
                    recipes: narr_recipe("SPAWN NOSUCH_PROTOTYPE"),
                },
            )],
            |_| {},
        );
        let mut state = seed();
        let r = run(&c, &dict, state, "0:0 @ ANA BAD", 1);
        assert_eq!(r.rejected, None);
        state = r.state;
        let adv = run(&c, &dict, state, "0:1", r.records.len() as i64 + 1);
        assert_eq!(adv.rejected, None); // the pass did NOT roll back
        let complete = adv
            .records
            .iter()
            .find(|x| x.kind == "complete")
            .expect("the completion row");
        assert!(complete.outcomes.iter().any(|o| o.op == "REFUSE"));
        assert_eq!(adv.state.windows.len(), 0); // the window is gone — no retry loop
        assert_eq!(adv.state.now, 30);
        // a SECOND advance works: the pass is healthy again
        let adv2 = run(
            &c,
            &dict,
            adv.state,
            "0:2",
            (r.records.len() + adv.records.len()) as i64 + 1,
        );
        assert_eq!(adv2.rejected, None);
    }
}

/// Undo with open windows.
mod undo_with_open_windows {
    use super::*;

    #[test]
    fn undo_drops_a_whole_turn_a_log_may_end_with_open_windows_and_folds_to_them() {
        let mut dict = core_dictionary();
        let mut state = seed();
        let g1 = geo(&mut dict, 60, 30);
        let r1 = run(&g1, &dict, state, "0:0 @ ANA PACE_A", 1);
        let r1_by_name_len = r1.state.by_name.len();
        state = r1.state;
        let g2 = geo(&mut dict, 60, 30);
        let r2 = run(&g2, &dict, state, "0:0 @ ANA PACE_B", r1.records.len() as i64 + 1);
        let mut all = r1.records.clone();
        all.extend(r2.records.clone());
        let gf = geo(&mut dict, 60, 30);
        // JSONL round-trip with both windows open: the fold rebuilds them
        let path = save_log_tmp("undo-open-windows", &all);
        let loaded = load_log(&path).unwrap();
        let replayed = fold(&loaded, &clone_state(&seed()), &dict, &gf).unwrap();
        assert_eq!(snap(&replayed), snap(&r2.state));
        assert_eq!(replayed.windows.len(), 2);
        // undo turn 2: keep r1's records only
        let undone = fold(
            &all.iter().filter(|x| x.id < r1.records.len() as i64 + 1).cloned().collect::<Vec<_>>(),
            &clone_state(&seed()),
            &dict,
            &gf,
        )
        .unwrap();
        assert_eq!(undone.windows.len(), 1);
        assert_eq!(undone.by_name.len(), r1_by_name_len);
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }
}

/// The input surface.
mod input_surface {
    use super::*;

    #[test]
    fn a_lone_clock_advances_the_same_minute_with_nothing_due_is_rejected() {
        let mut dict = core_dictionary();
        let content = compile_content(&seed_content(), &dict, &[]).unwrap();
        let mut state = seed();
        let idle = run(&content, &dict, clone_state(&state), "0:0", 1);
        assert_rejected(&idle, r"nothing to do");
        let g = geo(&mut dict, 60, 30);
        let r = run(&g, &dict, state, "0:0 @ ANA PACE_A", 1);
        state = r.state;
        let adv = run(&g, &dict, state, "0:1", r.records.len() as i64 + 1);
        assert_eq!(adv.rejected, None);
        assert_eq!(
            adv.records.iter().map(|x| x.kind.as_str()).collect::<Vec<_>>(),
            ["wait"] // the PACE_A window is not due yet
        );
        assert_eq!(adv.state.now, 30);
        let adv2 = run(
            &g,
            &dict,
            adv.state,
            "0:2",
            (r.records.len() + adv.records.len()) as i64 + 1,
        );
        assert_eq!(
            adv2.records.iter().map(|x| x.kind.as_str()).collect::<Vec<_>>(),
            ["complete", "wait"] // due at minute 60
        );
        assert_eq!(adv2.state.windows.len(), 0);
    }

    #[test]
    fn beginning_at_a_mid_halfhour_minute_is_representable_and_folds_exactly() {
        let mut dict = core_dictionary();
        let c = rig_content(
            &mut dict,
            vec![(
                "NAP",
                VerbContent {
                    slots: vec![actor_slot()],
                    duration: 20,
                    recipes: narr_recipe("NARRATE \"twenty minutes.\""),
                },
            )],
            |_| {},
        );
        let mut state = seed();
        let r1 = run(&c, &dict, state, "0:0 @ ANA NAP", 1); // [0, 20] — mid-halfhour end
        state = r1.state;
        let adv = run(&c, &dict, state, "0:1", r1.records.len() as i64 + 1);
        assert_eq!(adv.state.now, 30);
        let adv_state = clone_state(&adv.state);
        // a second NAP begins at minute 30 exactly (now, quantized input 0:1 = 30)
        let r2 = run(
            &c,
            &dict,
            adv.state,
            "0:1 @ ANA NAP",
            (r1.records.len() + adv.records.len()) as i64 + 1,
        );
        assert_eq!(r2.rejected, None);
        let w = r2.state.windows.values().next().expect("one open window");
        assert_eq!(w.start, 30);
        assert_eq!(w.end, 50);
        assert_eq!(
            snap(&fold(&r2.records, &adv_state, &dict, &c).unwrap()),
            snap(&r2.state)
        );
    }
}
