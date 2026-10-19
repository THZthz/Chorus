//! Conditionals (36 tests): IF / ELSE IF / ELSE in
//! outcome lists (spec §6.8). Branch selection runs
//! against the DRAFT (earlier outcomes in the same list are visible to later
//! IFs); first true branch wins; the log records only the taken branch's
//! outcomes, flattened in position — the record never contains an IF op.
//! Asymmetric failure discipline: a condition operand that cannot resolve
//! rejects the step (§9.2) where a `when:` predicate would read false. The
//! trigger guards and the endings check recurse into branch bodies;
//! role-dependent operands in trigger IF conditions are a load-time error.
//!
//! Every test builds a FRESH Dictionary with INSPECT/ATTUNE/STEW registered
//! (per-test isolation matters for reserved-word collisions). Error-message
//! patterns are copied CHARACTER FOR CHARACTER from the frozen literals;
//! the one `/s` (dotall) flag becomes an inline `(?s)` (the harness's
//! registered escape adjustment).

use crate::common::harness::{assert_err_msg, assert_refusal, assert_rejected};
use crate::common::rig::{
    inspect_content, narrated, ops_outcomes, said, settle_line, tmp_log_path, try_inspect_content,
    Extra,
};
use crate::common::seed::seed_state;
use crate::common::snap::snap;
use chorus::engine::settle::{act, done};

use chorus::content::script::compile_script;
use chorus::content::{
    compile_content, CompiledContent, ContentRaw, EndingDef, PrototypeDef, RecipeDef, SlotDef,
    SlotTriggerDef, TriggerDef, VerbContent,
};
use chorus::core::dict::{core_dictionary, Dictionary};
use chorus::core::format::format_sentence;
use chorus::core::parse::{parse_outcomes, parse_sentence};
use chorus::core::types::{add_entity, clone_state, new_state, Attrs, Ending, State};
use chorus::engine::fold::fold;
use chorus::engine::store::{load_log, save_log};
use chorus::engine::ActionRecord;
use indexmap::IndexMap;

// ---------------------------------------------------------------- rigs ------

/// The rig's own verb lexemes (verbs
/// are code-registered, §10). Per-test fresh.
fn cond_dict() -> Dictionary {
    let mut d = core_dictionary();
    d.register_verb("INSPECT").unwrap();
    d.register_verb("ATTUNE").unwrap();
    d.register_verb("STEW").unwrap();
    d
}

/// A one-verb rig
/// with a REQUIRED object role (ATTUNE).
fn attune_content(dict: &Dictionary, out: &[&str], duration: i64) -> CompiledContent {
    compile_content(
        &ContentRaw {
            verbs: [(
                "ATTUNE".to_string(),
                VerbContent {
                    slots: vec![
                        SlotDef {
                            role: "actors".into(),
                            min: 1,
                            max: Some(1),
                            query: Some("kind:character".into()),
                            shown: None,
                        },
                        SlotDef {
                            role: "object".into(),
                            min: 1,
                            max: Some(1),
                            query: None,
                            shown: None,
                        },
                    ],
                    duration,
                    recipes: vec![RecipeDef {
                        id: None,
                        when: None,
                        shown: None,
                        hint: None,
                        max: None,
                        aspects: Vec::new(),
                        dur: None,
                        out: out.iter().map(|s| (*s).to_string()).collect(),
                    }],
                    ambient: false,
                },
            )]
            .into_iter()
            .collect(),
            ..ContentRaw::default()
        },
        dict,
        &[],
    )
    .unwrap()
}

/// The seed world with
/// every lantern stripped — exists(aspect:lantern>=1) is false.
fn dim_world() -> State {
    let mut s = seed_state();
    let ids: Vec<String> = s.entities.keys().cloned().collect();
    for id in ids {
        s.entities.get_mut(&id).unwrap().aspects.shift_remove("lantern");
    }
    s
}

// ---------------------------------------------------------------------------
// IF / ELSE IF / ELSE grammar (§5.1, §6.8)
// ---------------------------------------------------------------------------

#[test]
fn parses_into_arms_plus_fallback_the_else_chain_is_consumed_inside_the_if_node() {
    // the flagship example, verbatim
    let out = parse_outcomes(
        r#"PROGRESS(30m, "You work the crowd."),
   IF count(name:SUSPICION) >= 3 -> (NARRATE "Too many eyes.", SPAWN PARANOIA)
   ELSE IF exists(THE_BRAZEN_COP) -> (NARRATE "The cop watches.")
   ELSE -> (NARRATE "Nobody notices."),
   NARRATE "after""#,
        &cond_dict(),
        None,
    )
    .unwrap();
    assert_eq!(out.len(), 3); // PROGRESS, IF, NARRATE — a stranded ELSE would end the list early
    assert_eq!(out[0].op, "PROGRESS");
    assert_eq!(out[1].op, "IF");
    assert_eq!(out[2].op, "NARRATE");
    let arms = out[1].arms.as_ref().unwrap();
    assert_eq!(arms.len(), 2);
    assert_eq!(ops_outcomes(&arms[0].then), ["NARRATE", "SPAWN"]);
    assert_eq!(ops_outcomes(&arms[1].then), ["NARRATE"]);
    assert_eq!(ops_outcomes(out[1].fallback.as_ref().unwrap()), ["NARRATE"]);
}

#[test]
fn conditions_keep_their_commas_rel_and_every_never_split_the_chain() {
    let out = parse_outcomes(
        "IF count(actors) = 2 AND rel(parent_child, actors) AND every(actors, aspect:occult >= 1) -> (NARRATE \"a\")",
        &cond_dict(),
        None,
    )
    .unwrap();
    assert_eq!(out.len(), 1);
    assert_eq!(out[0].op, "IF");
}

#[test]
fn a_nested_else_binds_to_the_nearest_if_greedy() {
    let dict = cond_dict();
    let inner = parse_outcomes(
        "IF exists(A) -> (IF exists(B) -> (NARRATE \"in\") ELSE -> (NARRATE \"inner else\"))",
        &dict,
        None,
    )
    .unwrap();
    assert_eq!(inner[0].fallback, None);
    let deep = inner[0].arms.as_ref().unwrap()[0].then[0].fallback.as_ref().unwrap();
    assert_eq!(narrated(deep), ["inner else"]);
    let outer = parse_outcomes(
        "IF exists(A) -> (IF exists(B) -> (NARRATE \"in\")) ELSE -> (NARRATE \"outer else\")",
        &dict,
        None,
    )
    .unwrap();
    assert_eq!(outer[0].arms.as_ref().unwrap()[0].then[0].fallback, None);
    assert_eq!(narrated(outer[0].fallback.as_ref().unwrap()), ["outer else"]);
}

#[test]
fn if_and_else_are_reserved_words_quoted_names_still_parse() {
    let dict = cond_dict();
    assert!(dict.reserved.contains("IF"));
    assert!(dict.reserved.contains("ELSE"));
    assert_err_msg(parse_sentence("0:0 @ ANA WAIT -> SPAWN IF", &dict), "reserved");
    assert_eq!(parse_outcomes("SPAWN \"IF\"", &dict, None).unwrap().len(), 1); // the quote is the escape hatch
}

#[test]
fn parse_errors_missing_branch_parens_else_without_if_dangling_else_if_condition_without_arrow() {
    let dict = cond_dict();
    assert_err_msg(parse_outcomes("IF exists(X) -> SPAWN Y", &dict, None), r"expected '\('"); // bodies require parens
    assert_err_msg(
        parse_sentence("0:0 @ ANA WAIT -> SPAWN X ELSE -> (NARRATE \"y\")", &dict),
        "ELSE",
    );
    assert_err_msg(parse_outcomes("IF exists(X) -> (NARRATE \"a\") ELSE IF", &dict, None), "->");
    assert_err_msg(parse_outcomes("IF exists(X)", &dict, None), "->");
    assert_err_msg(parse_outcomes("IF exists(X), NARRATE \"y\"", &dict, None), "->"); // a comma never splits a condition
    assert_err_msg(parse_outcomes("IF exists(X) ELSE -> (NARRATE \"y\")", &dict, None), "->");
}

#[test]
fn authored_outcome_strings_must_parse_completely_trailing_tokens_are_load_errors_too() {
    let dict = cond_dict();
    // content compile goes through parse_outcomes; a second ELSE, junk after a
    // complete chain, or a forgotten IF keyword must not vanish silently there
    assert_err_msg(
        parse_outcomes(
            "IF exists(X) -> (NARRATE \"a\") ELSE -> (NARRATE \"b\") ELSE -> (NARRATE \"c\")",
            &dict,
            None,
        ),
        r"trailing token 'ELSE'",
    );
    assert_err_msg(
        parse_outcomes("IF exists(X) -> (NARRATE \"a\") GARBAGE", &dict, None),
        r"trailing token 'GARBAGE'",
    );
    // v0.3.3: a name followed by '(' in outcome position is now a function-call
    // dispatch — an undeclared one is a compile error listing the vocabulary,
    // with a forgotten-IF hint for the predicate helpers (§10)
    assert_err_msg(
        parse_outcomes("exists(X) -> (NARRATE \"a\")", &dict, None),
        // the frozen pattern carries the /s (dotall) flag — inline (?s) here
        r"(?s)unknown 'exists\(\.\.\.\)' in outcome position.*IF keyword",
    ); // the IF keyword is not optional
    assert_err_msg(
        parse_outcomes("SPAWN X ELSE -> (NARRATE \"y\")", &dict, None),
        r"trailing token 'ELSE'",
    );
    assert_eq!(
        parse_outcomes("IF exists(X) -> (NARRATE \"a\") ELSE -> (NARRATE \"b\")", &dict, None)
            .unwrap()
            .len(),
        1 // a complete chain still parses
    );
}

// ---------------------------------------------------------------------------
// branch selection against the draft (§6.8)
// ---------------------------------------------------------------------------

#[test]
fn first_true_branch_wins_later_arms_never_apply() {
    let dict = cond_dict();
    let c = inspect_content(
        &dict,
        &["IF count(actors) >= 1 -> (NARRATE \"first\") ELSE IF exists(HOUSE) -> (NARRATE \"second\") ELSE -> (NARRATE \"third\")"],
        Extra::default(),
    );
    let r = settle_line(&c, &dict, seed_state(), "0:0 @ ANA INSPECT CHALICE", 1).unwrap();
    assert_eq!(r.rejected, None);
    assert_eq!(said(&r), ["first"]);
}

#[test]
fn an_else_if_chain_picks_the_first_true_arm_skipping_false_ones() {
    let dict = cond_dict();
    let c = inspect_content(
        &dict,
        &["IF exists(NOSUCH_CARD) -> (NARRATE \"one\") ELSE IF exists(HOUSE) -> (NARRATE \"two\") ELSE -> (NARRATE \"three\")"],
        Extra::default(),
    );
    let r = settle_line(&c, &dict, seed_state(), "0:0 @ ANA INSPECT CHALICE", 1).unwrap();
    assert_eq!(r.rejected, None);
    assert_eq!(said(&r), ["two"]);
}

#[test]
fn all_false_with_an_else_takes_the_else_branch() {
    let dict = cond_dict();
    let c = inspect_content(
        &dict,
        &["IF exists(NOSUCH_CARD) -> (NARRATE \"a\") ELSE -> (NARRATE \"b\")"],
        Extra::default(),
    );
    let r = settle_line(&c, &dict, seed_state(), "0:0 @ ANA INSPECT CHALICE", 1).unwrap();
    assert_eq!(r.rejected, None);
    assert_eq!(said(&r), ["b"]);
}

#[test]
fn all_false_without_an_else_applies_nothing_no_error_step_completes() {
    let dict = cond_dict();
    let c = inspect_content(
        &dict,
        &["NARRATE(\"before\"), IF exists(NOSUCH_CARD) -> (NARRATE \"a\") ELSE IF exists(NOSUCH_OTHER) -> (NARRATE \"b\"), NARRATE(\"after\")"],
        Extra::default(),
    );
    let r = settle_line(&c, &dict, seed_state(), "0:0 @ ANA INSPECT CHALICE", 1).unwrap();
    assert_eq!(r.rejected, None);
    assert_eq!(ops_outcomes(&done(&r).unwrap().outcomes), ["NARRATE", "NARRATE"]);
    assert_eq!(said(&r), ["before", "after"]);
}

#[test]
fn draft_visibility_set_then_if_reads_the_new_level_not_the_input_state() {
    let dict = cond_dict();
    let c = attune_content(
        &dict,
        &[
            "SET(OBJECT, aspect:level, +2)",
            "IF object.aspect.level >= 2 -> (NARRATE \"then — the draft\") ELSE -> (NARRATE \"else — the input\")",
        ],
        0,
    );
    let r = settle_line(&c, &dict, seed_state(), "0:0 @ ANA ATTUNE CHALICE", 1).unwrap(); // CHALICE carries no level aspect
    assert_eq!(r.rejected, None);
    assert_eq!(said(&r), ["then — the draft"]);
    // the delta logged its concrete absolute (§4.4), taken branch only
    assert_eq!(ops_outcomes(&done(&r).unwrap().outcomes), ["SET", "NARRATE"]);
    assert_eq!(
        r.state.entities.get("chalice").unwrap().aspects.get("level"),
        Some(&chorus::core::types::AspectVal::Num(2))
    );
}

#[test]
fn world_form_conditions_too_spawn_then_if_count_name_x_fires_in_the_same_list() {
    let dict = cond_dict();
    let c = inspect_content(
        &dict,
        &["SPAWN WHISPER, IF count(name:WHISPER) >= 1 -> (NARRATE \"heard\") ELSE -> (NARRATE \"silence\")"],
        Extra {
            prototypes: Some(
                [("WHISPER", "idea")]
                    .into_iter()
                    .map(|(n, k)| (n.to_string(), PrototypeDef { kind: k.into(), ..Default::default() }))
                    .collect(),
            ),
            ..Extra::default()
        },
    );
    let r = settle_line(&c, &dict, seed_state(), "0:0 @ ANA INSPECT CHALICE", 1).unwrap();
    assert_eq!(r.rejected, None);
    assert_eq!(said(&r), ["heard"]);
}

#[test]
fn draft_visibility_crosses_the_branch_boundary_a_branch_spawn_is_visible_to_the_next_if() {
    let dict = cond_dict();
    let c = inspect_content(
        &dict,
        &["IF exists(HOUSE) -> (SPAWN PARANOIA), IF count(name:PARANOIA) >= 1 -> (NARRATE \"saw it\") ELSE -> (NARRATE \"blind\")"],
        Extra {
            prototypes: Some(
                [("PARANOIA", "state")]
                    .into_iter()
                    .map(|(n, k)| (n.to_string(), PrototypeDef { kind: k.into(), ..Default::default() }))
                    .collect(),
            ),
            ..Extra::default()
        },
    );
    let r = settle_line(&c, &dict, seed_state(), "0:0 @ ANA INSPECT CHALICE", 1).unwrap();
    assert_eq!(r.rejected, None);
    assert_eq!(ops_outcomes(&done(&r).unwrap().outcomes), ["SPAWN", "NARRATE"]); // taken branch flattened in position
    assert_eq!(said(&r), ["saw it"]);
}

#[test]
fn an_if_after_a_replace_reads_the_successor_the_expansion_minted() {
    let dict = cond_dict();
    let c = inspect_content(
        &dict,
        &[
            "REPLACE OBJECT WITH GHOST",
            "IF exists(GHOST) -> (NARRATE \"successor here\") ELSE -> (NARRATE \"no successor\")",
        ],
        Extra {
            prototypes: Some(
                [("GHOST", "idea")]
                    .into_iter()
                    .map(|(n, k)| {
                        (n.to_string(), PrototypeDef { kind: k.into(), ..Default::default() })
                    })
                    .collect(),
            ),
            ..Extra::default()
        },
    );
    let base = seed_state();
    let r = settle_line(&c, &dict, clone_state(&base), "0:0 @ ANA INSPECT RITE_OF_ASCENSION", 1)
        .unwrap();
    assert_eq!(r.rejected, None);
    // the expansion's concrete SPAWN+DESTROY pair, then the taken branch — in position
    assert_eq!(ops_outcomes(&done(&r).unwrap().outcomes), ["SPAWN", "DESTROY", "NARRATE"]);
    assert_eq!(said(&r), ["successor here"]);
    let replayed = fold(&r.records, &base, &dict, &c).unwrap();
    assert_eq!(snap(&replayed), snap(&r.state));
}

#[test]
fn multi_clause_steps_each_clause_carries_its_own_bindings_ifs_in_both_apply() {
    let dict = cond_dict();
    let c = compile_content(
        &ContentRaw {
            verbs: [
                (
                    "INSPECT".to_string(),
                    VerbContent {
                        slots: vec![
                            SlotDef {
                                role: "actors".into(),
                                min: 1,
                                max: Some(1),
                                query: Some("kind:character".into()),
                                shown: None,
                            },
                            SlotDef { role: "object".into(), min: 1, max: Some(1), query: None, shown: None },
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
                            out: vec![
                                "IF exists(HOUSE) -> (NARRATE \"clause one\") ELSE -> (NARRATE \"clause one void\")"
                                    .to_string(),
                            ],
                        }],
                    ambient: false,
                    },
                ),
                (
                    "ATTUNE".to_string(),
                    VerbContent {
                        slots: vec![
                            SlotDef {
                                role: "actors".into(),
                                min: 1,
                                max: Some(1),
                                query: Some("kind:character".into()),
                                shown: None,
                            },
                            SlotDef { role: "object".into(), min: 1, max: Some(1), query: None, shown: None },
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
                            out: vec![
                                "IF exists(NOSUCH_CARD) -> (NARRATE \"clause two bad\") ELSE -> (NARRATE \"clause two else\")"
                                    .to_string(),
                            ],
                        }],
                    ambient: false,
                    },
                ),
            ]
            .into_iter()
            .collect(),
            ..ContentRaw::default()
        },
        &dict,
        &[],
    )
    .unwrap();
    let base = seed_state();
    let r = settle_line(
        &c,
        &dict,
        clone_state(&base),
        "0:0 @ ANA INSPECT CHALICE; ANA ATTUNE RITE_OF_ASCENSION",
        1,
    )
    .unwrap();
    assert_eq!(r.rejected, None);
    assert_eq!(said(&r), ["clause one", "clause two else"]);
    let replayed = fold(&r.records, &base, &dict, &c).unwrap();
    assert_eq!(snap(&replayed), snap(&r.state));
}

#[test]
fn the_dead_stamp_rule_destroy_object_then_if_reads_the_dead_card_as_absent_silent_false() {
    let dict = cond_dict();
    let c = attune_content(
        &dict,
        &[
            "DESTROY OBJECT",
            "IF object.aspect.demand >= 1 -> (NARRATE \"still there\") ELSE -> (NARRATE \"gone\")",
        ],
        0,
    );
    // RITE_OF_ASCENSION carries demand:3 — destroyed earlier in the SAME list,
    // so the condition reads a dead-stamped entity: false, silently (§6.3)
    let r = settle_line(&c, &dict, seed_state(), "0:0 @ ANA ATTUNE RITE_OF_ASCENSION", 1).unwrap();
    assert_eq!(r.rejected, None);
    assert_eq!(said(&r), ["gone"]);
}

// ---------------------------------------------------------------------------
// concretization — the record never contains an IF op (§6.8)
// ---------------------------------------------------------------------------

/// The section-level content (PARANOIA
/// prototype added).
fn concretization_content(dict: &Dictionary) -> CompiledContent {
    inspect_content(
        dict,
        &["NARRATE(\"before\"), IF exists(HOUSE) -> (PROGRESS(30m, \"tick\"), SPAWN PARANOIA) ELSE -> (NARRATE \"untaken\"), NARRATE(\"after\")"],
        Extra {
            prototypes: Some(
                [("PARANOIA", "state")]
                    .into_iter()
                    .map(|(n, k)| (n.to_string(), PrototypeDef { kind: k.into(), ..Default::default() }))
                    .collect(),
            ),
            ..Extra::default()
        },
    )
}

#[test]
fn the_taken_branch_flattens_in_position_the_untaken_branch_is_gone() {
    let dict = cond_dict();
    let c = concretization_content(&dict);
    let r = settle_line(&c, &dict, seed_state(), "0:0 @ ANA INSPECT CHALICE", 1).unwrap();
    assert_eq!(r.rejected, None);
    assert_eq!(
        ops_outcomes(&done(&r).unwrap().outcomes),
        ["NARRATE", "PROGRESS", "SPAWN", "NARRATE"]
    );
    let all = serde_json::to_string(&r.records).unwrap();
    assert!(!all.contains(r#""IF""#));
    assert!(!all.contains("ELSE"));
    assert!(!all.contains("untaken"));
    // v0.5: a branch-nested PROGRESS defers with the branch — the completion
    // row carries the flattened, taken branch verbatim
    let sentence = &done(&r).unwrap().sentence;
    assert!(sentence.contains("PROGRESS(30m, \"tick\")"), "{sentence}"); // canonical duration form
    assert!(!sentence.contains("IF "));
}

#[test]
fn the_recorded_sentence_round_trips() {
    let dict = cond_dict();
    let c = concretization_content(&dict);
    let r = settle_line(&c, &dict, seed_state(), "0:0 @ ANA INSPECT CHALICE", 1).unwrap();
    let s = done(&r).unwrap().sentence.clone();
    let reparsed = parse_sentence(&s, &dict).unwrap();
    assert_eq!(format_sentence(&reparsed, &dict).unwrap(), s);
}

#[test]
fn fold_and_jsonl_equality_replaying_the_flattened_rows_reproduces_play_exactly() {
    let dict = cond_dict();
    let c = concretization_content(&dict);
    let state = seed_state();
    let base = clone_state(&state);
    let r1 = settle_line(&c, &dict, state, "0:0 @ ANA INSPECT CHALICE", 1).unwrap();
    let quiet =
        inspect_content(&dict, &["IF exists(NOSUCH_CARD) -> (NARRATE \"no\")"], Extra::default());
    let r2 = settle_line(&quiet, &dict, r1.state, "0:0 @ ANA INSPECT CHALICE", 2).unwrap(); // no ELSE: nothing to replay
    assert_eq!(r2.rejected, None);
    assert_eq!(done(&r2).unwrap().outcomes.len(), 0);
    let path = tmp_log_path("cond-flatten");
    let both: Vec<ActionRecord> = r1.records.iter().chain(r2.records.iter()).cloned().collect();
    save_log(&path, &both).unwrap();
    let replayed = fold(&load_log(&path).unwrap(), &base, &dict, &c).unwrap();
    assert_eq!(snap(&replayed), snap(&r2.state));
    let _ = std::fs::remove_file(&path);
}

// ---------------------------------------------------------------------------
// REFUSE inside a taken branch refuses the window (§6.5, v0.5 completion)
// ---------------------------------------------------------------------------

#[test]
fn refuse_inside_a_taken_branch_attempt_logged_effects_discarded_state_untouched_fold_no_op() {
    let dict = cond_dict();
    let c = attune_content(
        &dict,
        &["IF exists(HOUSE) -> (DESTROY OBJECT, REFUSE(\"not yet\")) ELSE -> (NARRATE \"calm\")"],
        60, // a duration the refusal cancels — the window opens, then collapses
    );
    let state = seed_state();
    let before = snap(&state);
    let base_now = state.now;
    let r = settle_line(&c, &dict, clone_state(&state), "0:0 @ ANA ATTUNE CHALICE", 1).unwrap();
    assert_eq!(r.rejected, None); // v0.5: the commit is clean; the REFUSE decides at completion
    assert_eq!(ops_outcomes(&done(&r).unwrap().outcomes), ["DESTROY", "REFUSE"]);
    assert_eq!(act(&r).unwrap().advanced_min, 60); // the window DID open…
    assert_eq!(r.state.now, base_now + 60); // …and the clock crossed it
                                            // `r.state !== state`: the step state genuinely moved (now advanced)
    assert_eq!(snap(&state), before); // the scratch died with the refusal
    let replayed = fold(&r.records, &state, &dict, &c).unwrap(); // replay agrees, effects skipped
    assert_eq!(snap(&replayed), snap(&r.state));
    assert_eq!(said(&r), Vec::<String>::new()); // the untaken else never spoke
}

// ---------------------------------------------------------------------------
// condition failure refuses the window; the same grammar reads false in when:
// ---------------------------------------------------------------------------

#[test]
fn an_if_condition_operand_with_no_bound_card_the_window_refuses_at_completion() {
    let dict = cond_dict();
    let c = inspect_content(
        &dict,
        &["IF object.aspect.demand >= 1 -> (NARRATE \"needs\")"],
        Extra::default(),
    );
    let state = seed_state();
    let r = settle_line(&c, &dict, clone_state(&state), "0:0 @ ANA INSPECT", 1).unwrap(); // no object bound: the operand cannot resolve
    assert_eq!(r.rejected, None); // v0.5: strict conditions decide at the completion
    assert_refusal(&r, r"IF condition: operand 'object\.aspect\.demand' cannot resolve");
    // the effects are discarded and the window closes — but §6.4: it
    // registered at begin, so a window that later refuses still counted
    // (the one deliberate state delta; everything else is untouched)
    assert_eq!(r.state.executed.get("inspect.default"), Some(&1));
    assert_eq!(r.state.windows.len(), 0);
    assert_eq!(r.state.now, state.now);
    // full-state strength (the dice.rs refusal shape): the fold re-derives
    // exactly that state from the log alone — the REFUSE complete row is a
    // state no-op — so any unlogged play-side drift fails loud here
    let replayed = fold(&r.records, &state, &dict, &c).unwrap();
    assert_eq!(snap(&replayed), snap(&r.state));
}

#[test]
fn the_same_predicate_in_a_when_position_reads_false_and_selects_the_default() {
    let dict = cond_dict();
    let c = compile_content(
        &ContentRaw {
            verbs: [(
                "INSPECT".to_string(),
                VerbContent {
                    slots: vec![
                        SlotDef {
                            role: "actors".into(),
                            min: 1,
                            max: Some(1),
                            query: Some("kind:character".into()),
                            shown: None,
                        },
                        SlotDef {
                            role: "object".into(),
                            min: 0,
                            max: Some(1),
                            query: None,
                            shown: None,
                        },
                    ],
                    duration: 0,
                    recipes: vec![
                        RecipeDef {
                            id: Some("inspect.gate".into()),
                            when: Some("object.aspect.demand >= 1".into()),
                            shown: None,
                            hint: None,
                            max: None,
                            aspects: Vec::new(),
                            dur: None,
                            out: vec!["NARRATE(\"gate\")".to_string()],
                        },
                        RecipeDef {
                            id: Some("inspect.default".into()),
                            when: None,
                            shown: None,
                            hint: None,
                            max: None,
                            aspects: Vec::new(),
                            dur: None,
                            out: vec!["NARRATE(\"default\")".to_string()],
                        },
                    ],
                    ambient: false,
                },
            )]
            .into_iter()
            .collect(),
            ..ContentRaw::default()
        },
        &dict,
        &[],
    )
    .unwrap();
    let r = settle_line(&c, &dict, seed_state(), "0:0 @ ANA INSPECT", 1).unwrap();
    assert_eq!(r.rejected, None);
    assert_eq!(act(&r).unwrap().recipes, ["inspect.default"]); // false, not a rejection — the asymmetry
}

#[test]
fn a_missing_aspect_on_a_bound_card_still_reads_false_inside_an_if() {
    let dict = cond_dict();
    let c = inspect_content(
        &dict,
        &["IF object.aspect.demand >= 1 -> (NARRATE \"needs\") ELSE -> (NARRATE \"harmless\")"],
        Extra::default(),
    );
    let r = settle_line(&c, &dict, seed_state(), "0:0 @ ANA INSPECT CHALICE", 1).unwrap(); // bound, but no demand aspect
    assert_eq!(r.rejected, None);
    assert_eq!(said(&r), ["harmless"]);
}

#[test]
fn count_and_total_over_an_empty_role_read_0_branch_selection_never_a_rejection() {
    let dict = cond_dict();
    let count = inspect_content(
        &dict,
        &["IF count(object) >= 1 -> (NARRATE \"has\") ELSE -> (NARRATE \"none\")"],
        Extra::default(),
    );
    assert_eq!(
        said(&settle_line(&count, &dict, seed_state(), "0:0 @ ANA INSPECT", 1).unwrap()),
        ["none"]
    ); // object slot unbound
    let total = inspect_content(
        &dict,
        &["IF total(object, aspect:demand) >= 1 -> (NARRATE \"has\") ELSE -> (NARRATE \"none\")"],
        Extra::default(),
    );
    assert_eq!(
        said(&settle_line(&total, &dict, seed_state(), "0:0 @ ANA INSPECT", 1).unwrap()),
        ["none"]
    );
}

#[test]
fn the_documented_guard_for_an_optional_role_count_and_never_rejects() {
    let dict = cond_dict();
    let c = inspect_content(
        &dict,
        &["IF count(object) >= 1 AND object.aspect.demand >= 1 -> (NARRATE \"demands\") ELSE -> (NARRATE \"no demand\")"],
        Extra::default(),
    );
    // AND short-circuits before the strict path reads
    assert_eq!(
        said(&settle_line(&c, &dict, seed_state(), "0:0 @ ANA INSPECT", 1).unwrap()),
        ["no demand"]
    );
    assert_eq!(
        said(
            &settle_line(&c, &dict, seed_state(), "0:0 @ ANA INSPECT RITE_OF_ASCENSION", 1)
                .unwrap()
        ),
        ["demands"]
    );
}

// ---------------------------------------------------------------------------
// IF conditions in recipes read the step bindings (§6.8)
// ---------------------------------------------------------------------------

/// The section-level content.
fn bindings_content(dict: &Dictionary) -> CompiledContent {
    inspect_content(
        dict,
        &["IF count(actors) = 2 AND rel(parent_child, actors) -> (NARRATE \"kin\")\n         ELSE IF exists(aspect:lantern >= 1) -> (NARRATE \"lantern light\")\n         ELSE -> (NARRATE \"plain\")"],
        Extra::default(),
    )
}

#[test]
fn count_actors_rel_and_exists_query_bind_against_the_step_and_the_world() {
    let dict = cond_dict();
    let c = bindings_content(&dict);
    assert_eq!(
        said(&settle_line(&c, &dict, seed_state(), "0:0 @ ANA, BERTHA INSPECT", 1).unwrap()),
        ["kin"]
    );
    assert_eq!(
        said(&settle_line(&c, &dict, dim_world(), "0:0 @ ANA INSPECT", 1).unwrap()),
        ["plain"]
    );
    // the world form can be false
}

#[test]
fn object_aspect_reads_the_bound_card_the_count_dread_typo_lint_covers_if_conditions_too() {
    let dict = cond_dict();
    let d = inspect_content(
        &dict,
        &["IF object.aspect.demand >= 2 -> (NARRATE \"demanding\") ELSE -> (NARRATE \"modest\")"],
        Extra::default(),
    );
    // demand:3
    assert_eq!(
        said(
            &settle_line(&d, &dict, seed_state(), "0:0 @ ANA INSPECT RITE_OF_ASCENSION", 1)
                .unwrap()
        ),
        ["demanding"]
    );
    assert_eq!(
        said(&settle_line(&d, &dict, seed_state(), "0:0 @ ANA INSPECT CHALICE", 1).unwrap()),
        ["modest"]
    );
    assert_err_msg(
        try_inspect_content(&dict, &["IF count(DREAD) >= 3 -> (NARRATE \"x\")"], Extra::default()),
        r"count\(\) argument 'DREAD' is neither a role",
    );
    let _ = try_inspect_content(
        &dict,
        &["IF count(name:DREAD) >= 3 -> (NARRATE \"x\")"],
        Extra::default(),
    );
    // compiles clean
}

// ---------------------------------------------------------------------------
// role-dependent operands in trigger IF conditions are load errors (§6.3)
// ---------------------------------------------------------------------------

fn slot_triggers(out: &[&str]) -> Vec<TriggerDef> {
    vec![TriggerDef::Slot(SlotTriggerDef {
        id: "t".into(),
        slot: 3,
        once: None,
        out: out.iter().map(|s| (*s).to_string()).collect(),
    })]
}

fn expiry_trigger(out: &[&str]) -> Vec<TriggerDef> {
    vec![TriggerDef::Expire(chorus::content::ExpireTriggerDef {
        id: "t".into(),
        on_expire_of: "CHALICE".into(),
        out: out.iter().map(|s| (*s).to_string()).collect(),
    })]
}

#[test]
fn path_count_total_rel_every_any_each_named_by_the_error() {
    let dict = cond_dict();
    let raw =
        |out: &[&str]| ContentRaw { triggers: Some(slot_triggers(out)), ..ContentRaw::default() };
    assert_err_msg(
        compile_content(&raw(&["IF object.aspect.duty >= 1 -> (NARRATE \"x\")"]), &dict, &[]),
        r"IF operand object\.aspect\.duty reads a step role",
    );
    assert_err_msg(
        compile_content(&raw(&["IF object.attr.seats >= 1 -> (NARRATE \"x\")"]), &dict, &[]),
        r"IF operand object\.attr\.seats reads a step role",
    );
    assert_err_msg(
        compile_content(&raw(&["IF count(actors) >= 1 -> (NARRATE \"x\")"]), &dict, &[]),
        r"IF operand count\(actors\) reads a step role",
    );
    assert_err_msg(
        compile_content(
            &raw(&["IF total(actors, aspect:nerve) >= 2 -> (NARRATE \"x\")"]),
            &dict,
            &[],
        ),
        r"IF operand total\(actors, aspect:nerve\) reads a step role",
    );
    assert_err_msg(
        compile_content(&raw(&["IF rel(parent_child, actors) -> (NARRATE \"x\")"]), &dict, &[]),
        r"IF operand rel\(parent_child, actors\) reads a step role",
    );
    assert_err_msg(
        compile_content(
            &raw(&["IF any(actors, aspect:occult >= 1) -> (NARRATE \"x\")"]),
            &dict,
            &[],
        ),
        r"IF operand any\(actors, …\) reads a step role",
    );
}

#[test]
fn the_check_recurses_into_branch_bodies_and_world_forms_pass() {
    let dict = cond_dict();
    let raw =
        |out: &[&str]| ContentRaw { triggers: Some(slot_triggers(out)), ..ContentRaw::default() };
    assert_err_msg(
        compile_content(
            &raw(&["IF exists(X) -> (IF count(actors) >= 1 -> (NARRATE \"deep\"))"]),
            &dict,
            &[],
        ),
        r"trigger t: IF operand count\(actors\) reads a step role",
    );
    // two IF levels down — the walk must not stop at the first branch
    assert_err_msg(
        compile_content(
            &raw(&[
                "IF exists(X) -> (IF exists(Y) -> (IF count(actors) >= 1 -> (NARRATE \"deeper\")))",
            ]),
            &dict,
            &[],
        ),
        r"trigger t: IF operand count\(actors\) reads a step role",
    );
    let _ = compile_content(
        &raw(&[
            "IF exists(X) -> (NARRATE \"ok\") ELSE IF count(name:X) >= 3 -> (NARRATE \"ok too\")",
        ]),
        &dict,
        &[],
    ); // compiles clean
}

#[test]
fn expiry_triggers_lint_their_if_conditions_too_same_empty_bindings_argument() {
    let dict = cond_dict();
    let raw =
        |out: &[&str]| ContentRaw { triggers: Some(expiry_trigger(out)), ..ContentRaw::default() };
    assert_err_msg(
        compile_content(&raw(&["IF object.aspect.duty >= 1 -> (NARRATE \"x\")"]), &dict, &[]),
        r"trigger t: IF operand object\.aspect\.duty reads a step role",
    );
    assert_err_msg(
        compile_content(
            &raw(&["IF exists(X) -> (IF total(actors, aspect:nerve) >= 1 -> (NARRATE \"x\"))"]),
            &dict,
            &[],
        ),
        r"trigger t: IF operand total\(actors, aspect:nerve\) reads a step role",
    );
    let _ = compile_content(&raw(&["IF count(name:CHALICE) >= 1 -> (NARRATE \"ok\")"]), &dict, &[]);
    // compiles clean
}

#[test]
fn the_recursive_guards_catch_progress_refuse_unknown_ending_nested_in_if_branches() {
    let dict = cond_dict();
    let raw = |out: &[&str], endings: Option<IndexMap<String, EndingDef>>| ContentRaw {
        triggers: Some(slot_triggers(out)),
        endings,
        ..ContentRaw::default()
    };
    assert_err_msg(
        compile_content(&raw(&["IF exists(X) -> (PROGRESS(30m, \"tick\"))"], None), &dict, &[]),
        "trigger t: PROGRESS belongs to verb recipes, not events",
    );
    assert_err_msg(
        compile_content(&raw(&["IF exists(X) -> (REFUSE(\"no\"))"], None), &dict, &[]),
        "trigger t: REFUSE belongs to verb recipes — events have no step to refuse",
    );
    // §6.6: ADVANCE stretches the pass a completion rides; the event kinds
    // own no pass, so ADVANCE there is the same load-error family as
    // PROGRESS/REFUSE/RUN, nested arms included
    assert_err_msg(
        compile_content(&raw(&["IF exists(X) -> (ADVANCE(1h))"], None), &dict, &[]),
        "trigger t: ADVANCE belongs to verb recipes — events own no advance pass to stretch",
    );
    assert_err_msg(
        compile_content(&raw(&["ADVANCE(1h)"], None), &dict, &[]),
        "trigger t: ADVANCE belongs to verb recipes — events own no advance pass to stretch",
    );
    let endings = Some(
        [("REAL".to_string(), EndingDef { title: "Real".into(), flavour: None })]
            .into_iter()
            .collect(),
    );
    assert_err_msg(
        compile_content(&raw(&["IF exists(X) -> (ENDING NOPE)"], endings.clone()), &dict, &[]),
        r"trigger t: ENDING 'NOPE' is not in the endings table",
    );
    // deep nesting: two IF levels down
    assert_err_msg(
        compile_content(
            &raw(&["IF exists(X) -> (IF exists(Y) -> (ENDING NOPE))"], endings),
            &dict,
            &[],
        ),
        r"ENDING 'NOPE' is not in the endings table",
    );
}

// ---------------------------------------------------------------------------
// IF inside working triggers (§6.8, world-form conditions)
// ---------------------------------------------------------------------------

fn trigger_wait(dict: &Dictionary, triggers: Vec<TriggerDef>) -> CompiledContent {
    compile_content(
        &ContentRaw {
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
                        dur: None,
                        out: vec!["NARRATE(\"{actors} wait.\")".to_string()],
                    }],
                    ambient: false,
                },
            )]
            .into_iter()
            .collect(),
            triggers: Some(triggers),
            prototypes: Some(
                [("PARANOIA", "state")]
                    .into_iter()
                    .map(|(n, k)| {
                        (n.to_string(), PrototypeDef { kind: k.into(), ..Default::default() })
                    })
                    .collect(),
            ),
            ..ContentRaw::default()
        },
        dict,
        &[],
    )
    .unwrap()
}

#[test]
fn a_slot_trigger_takes_its_if_branch_exists_form_and_the_record_flattens_it() {
    let dict = cond_dict();
    let c = trigger_wait(
        &dict,
        vec![TriggerDef::Slot(SlotTriggerDef {
            id: "patrol".into(),
            slot: 6,
            once: Some(true),
            out: vec![
                "IF exists(HOUSE) -> (SPAWN PARANOIA) ELSE -> (NARRATE \"quiet.\")".to_string()
            ],
        })],
    );
    let state = seed_state();
    let base = clone_state(&state);
    let r = settle_line(&c, &dict, state, "0:0 @ ANA WAIT", 1).unwrap(); // 1h crosses slot 2 (1:00)
    assert_eq!(r.rejected, None);
    // v0.5 order: the WAIT window completes, then the crossed slot's events
    let kinds: Vec<&str> = r.records.iter().map(|x| x.kind.as_str()).collect();
    assert_eq!(kinds, ["action", "complete", "event", "wait"]);
    let event = r.records.iter().find(|x| x.kind == "event").unwrap();
    assert_eq!(event.trigger.as_deref(), Some("patrol"));
    assert_eq!(ops_outcomes(&event.outcomes), ["SPAWN"]); // the taken branch, no IF op, no else NARRATE
    assert!(r.state.by_name.contains_key("PARANOIA"));
    let replayed = fold(&r.records, &base, &dict, &c).unwrap();
    assert_eq!(snap(&replayed), snap(&r.state));
}

#[test]
fn an_expiry_trigger_takes_its_if_branch_count_name_form() {
    let dict = cond_dict();
    let c = trigger_wait(
        &dict,
        vec![TriggerDef::Expire(chorus::content::ExpireTriggerDef {
            id: "chalice_dust".into(),
            on_expire_of: "CHALICE".into(),
            out: vec![
                "IF count(name:RITE_OF_ASCENSION) >= 1 -> (DESTROY CHALICE) ELSE -> (NARRATE \"lingers\")"
                    .to_string(),
            ],
        })],
    );
    let mut state = seed_state();
    state
        .entities
        .get_mut("chalice")
        .unwrap()
        .attrs
        .insert("expiresAt".to_string(), chorus::core::types::AttrValue::Num(30)); // crosses slot 1 (0:30)
    let base = clone_state(&state);
    let r = settle_line(&c, &dict, state, "0:0 @ ANA WAIT", 1).unwrap();
    assert_eq!(r.rejected, None);
    let event = r.records.iter().find(|x| x.kind == "event").unwrap();
    assert_eq!(event.trigger.as_deref(), Some("chalice_dust"));
    assert_eq!(ops_outcomes(&event.outcomes), ["DESTROY"]);
    assert!(r.state.entities.get("chalice").unwrap().dead_action.is_some());
    let replayed = fold(&r.records, &base, &dict, &c).unwrap();
    assert_eq!(snap(&replayed), snap(&r.state));
}

#[test]
fn a_trigger_if_whose_condition_is_false_takes_the_else_branch() {
    let dict = cond_dict();
    let c = trigger_wait(
        &dict,
        vec![TriggerDef::Slot(SlotTriggerDef {
            id: "patrol".into(),
            slot: 6,
            once: None,
            out: vec![
                "IF exists(NOSUCH_CARD) -> (SPAWN PARANOIA) ELSE -> (NARRATE \"all quiet.\")"
                    .to_string(),
            ],
        })],
    );
    let r = settle_line(&c, &dict, seed_state(), "0:0 @ ANA WAIT", 1).unwrap();
    let event = r.records.iter().find(|x| x.kind == "event").unwrap();
    assert_eq!(ops_outcomes(&event.outcomes), ["NARRATE"]);
    assert_eq!(narrated(&event.outcomes)[0], "all quiet.");
}

// ---------------------------------------------------------------------------
// pooled worlds compose: the arrest case, end to end
// ---------------------------------------------------------------------------

const ARREST_SCRIPT: &str = r#"
script: Arrest
player: ANA
start: 0:0

entities:
  HOUSE is place
  ANA is character at HOUSE

prototypes:
  SUSPICION is state stackable

endings:
  ARRESTED: "Taken"

verbs:
  STEW:
    actors: 1 kind:character
    duration: 0m
    recipes:
      when count(name:SUSPICION) < 3 -> SPAWN SUSPICION, IF count(name:SUSPICION) >= 3 -> (ENDING ARRESTED) ELSE -> (NARRATE "Nobody notices.")
"#;

#[test]
fn selection_reads_the_input_state_the_recipe_reads_its_own_spawn_on_the_draft() {
    let dict = cond_dict();
    let compiled = compile_script(ARREST_SCRIPT, &dict).unwrap();
    let content = &compiled.content;
    let base = clone_state(&compiled.state);
    let mut cur = compiled.state;
    let mut id = 1i64;
    let mut all: Vec<ActionRecord> = Vec::new();
    let mut told: Vec<Vec<String>> = Vec::new();
    for _ in 0..3 {
        let r = settle_line(content, &dict, cur, "0:0 @ ANA STEW", id).unwrap();
        assert_eq!(r.rejected, None);
        assert_eq!(act(&r).unwrap().recipes, ["stew.recipe0"]); // the `when count < 3` recipe keeps firing on input state
        told.push(said(&r));
        id += r.records.len() as i64;
        all.extend(r.records);
        cur = r.state;
    }
    assert_eq!(
        told,
        vec![vec!["Nobody notices.".to_string()], vec!["Nobody notices.".to_string()], vec![]]
    ); // the third SPAWN flipped its own IF
    assert_eq!(cur.by_name.get("SUSPICION").unwrap().len(), 3);
    assert_eq!(
        cur.ended,
        Some(Ending { id: "ARRESTED".into(), title: "Taken".into(), flavour: "melancholy".into() })
    );
    // game over afterwards
    let over = settle_line(content, &dict, cur, "0:0 @ ANA STEW", id).unwrap();
    assert_rejected(&over, "the story has ended — Taken");
    assert_eq!(over.records.len(), 0);
    // and the whole run folds back identically — the flattening is lossless
    let replayed = fold(&all, &base, &dict, content).unwrap();
    assert_eq!(snap(&replayed), snap(&over.state));
}

#[test]
fn a_preset_pool_arrests_on_the_first_stew_count_name_is_world_scoped() {
    let dict = cond_dict();
    let compiled = compile_script(ARREST_SCRIPT, &dict).unwrap();
    // a fresh world where the pool is pre-stocked to 2: the very next stew arrests
    let mut s = new_state(0);
    let mut card = |id: &str, kind: &str, name: &str, attrs: Attrs, stackable: bool| {
        add_entity(
            &mut s,
            chorus::core::types::Entity {
                id: id.to_string(),
                kind: kind.to_string(),
                name: name.to_string(),
                aspects: chorus::core::types::Aspects::new(),
                attrs,
                born_action: None,
                dead_action: None,
                stackable,
                desc: None,
            },
        )
        .unwrap();
    };
    card("s_house", "place", "HOUSE", Attrs::new(), false);
    card(
        "s_ana",
        "character",
        "ANA",
        [("container".to_string(), chorus::core::types::AttrValue::Str("s_house".into()))]
            .into_iter()
            .collect(),
        false,
    );
    card("w1", "state", "SUSPICION", Attrs::new(), true);
    card("w2", "state", "SUSPICION", Attrs::new(), true);
    let r = settle_line(&compiled.content, &dict, s, "0:0 @ ANA STEW", 1).unwrap();
    assert_eq!(r.rejected, None);
    assert_eq!(ops_outcomes(&done(&r).unwrap().outcomes), ["SPAWN", "ENDING"]);
    assert_eq!(r.state.ended.as_ref().unwrap().id, "ARRESTED");
}
