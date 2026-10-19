//! Dice — seeded dice, the ROLL op, the `roll`
//! binding, tiers via IF (spec §4.4). Includes the pinned
//! RNG vectors (3 raw splitmix64 values + the 27-row (seed, counter, sides)
//! → die table, all computed independently in python3) and
//! the die range / d2 coverage probe, plus determinism
//! through the script compiler, ROLL arg grammar + runtime refusals, the per-clause
//! roll binding and the may-bind lint, {roll} prose interpolation, ROLLED
//! round-trips and fold replay, undo reconstruction, triggers and functions
//! with dice, the JSONL byte-identity gate (same seed twice → identical
//! bytes).
//!
//! Every test builds a FRESH Dictionary with INSPECT/ATTUNE registered
//! (per-test isolation matters for reserved-word collisions). Error-message
//! patterns are copied CHARACTER FOR CHARACTER from the frozen literals.

use crate::common::harness::{assert_err_msg, assert_refusal, assert_rejected};
use crate::common::rig::{
    inspect_content, said_record, settle_line, tmp_log_path, try_inspect_content, Extra,
};
use crate::common::seed::seed_state;
use crate::common::snap::snap;
use chorus::engine::settle::done;

use chorus::content::script::compile_script;
use chorus::content::{
    compile_content, CompiledContent, ContentRaw, FunctionDef, PrototypeDef, RecipeDef, SlotDef,
    SlotTriggerDef, TriggerDef, VerbContent,
};
use chorus::core::dice::{roll_die, splitmix64};
use chorus::core::dict::{core_dictionary, Dictionary};
use chorus::core::format::format_sentence;
use chorus::core::parse::{parse_outcomes, parse_sentence};
use chorus::core::sentence::{Arg, Outcome};
use chorus::core::types::{clone_state, new_state, AspectVal, AttrValue, State};
use chorus::engine::fold::{fold, fold_up_to};
use chorus::engine::store::{load_log, save_log};
use chorus::engine::ActionRecord;
use chorus::session::game::{Game, TurnKind};

// ---------------------------------------------------------------- rigs ------

/// The file-level dict registers the rigs' own
/// verb lexemes (verbs are code-registered, §10). Per-test fresh.
fn dice_dict() -> Dictionary {
    let mut d = core_dictionary();
    d.register_verb("INSPECT").unwrap();
    d.register_verb("ATTUNE").unwrap();
    d
}

/// The record's first ROLLED row
/// as (total, counter-after), or None.
fn rolled_row(rec: &ActionRecord) -> Option<(i64, i64)> {
    let o = rec.outcomes.iter().find(|o| o.op == "ROLLED")?;
    match (o.args.first(), o.args.get(1)) {
        (Some(Arg::Int { v: a }), Some(Arg::Int { v: b })) => Some((*a, *b)),
        _ => None,
    }
}

/// Every ROLLED row of a record,
/// in order.
fn rolled_rows(rec: &ActionRecord) -> Vec<(i64, i64)> {
    let mut rows = Vec::new();
    for o in &rec.outcomes {
        if o.op != "ROLLED" {
            continue;
        }
        if let (Some(Arg::Int { v: a }), Some(Arg::Int { v: b })) = (o.args.first(), o.args.get(1))
        {
            rows.push((*a, *b));
        }
    }
    rows
}

/// The standard seed world plus
/// a `nerve:3` aspect on ANA — the total() cases roll 3 dice at counters
/// 0..2 = 6+4+4 = 14.
fn seed_world() -> State {
    let mut s = seed_state();
    s.entities.get_mut("ana").unwrap().aspects.insert("nerve".to_string(), AspectVal::Num(3));
    s
}

fn dice_script(seed: i64) -> String {
    format!(
        r#"
script: Dice Rig
player: ANA
start: 0:0
seed: {seed}

entities:
  ANA is character

verbs:
  WAIT:
    actors: 1 kind:character
    duration: 0m
    recipes:
      default -> ROLL(3, 6), NARRATE "the bones say {{roll}}."
"#
    )
}

/// A hand-built COMPLETE record like the regression literals in this file
/// (id, kind "complete", trigger None, t_game "0:0", t_real None,
/// start_min None, window "fixture", advanced_min 0).
fn fixture_complete(id: i64, outcomes: Vec<Outcome>) -> ActionRecord {
    ActionRecord {
        id,
        kind: "complete".to_string(),
        trigger: None,
        t_game: "0:0".to_string(),
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

fn rolled_outcome(total: i64, ctr_after: i64) -> Outcome {
    Outcome {
        op: "ROLLED".to_string(),
        args: vec![Arg::Int { v: total }, Arg::Int { v: ctr_after }],
        arms: None,
        fallback: None,
        call: None,
    }
}

// ---------------------------------------------------------------------------
// the pinned vectors (computed in python3)
// ---------------------------------------------------------------------------

#[test]
fn splitmix64_raw_values() {
    // Raw finalizer pins: catch ANY drift in
    // constants or shift structure.
    assert_eq!(splitmix64(0), 16294208416658607535);
    assert_eq!(splitmix64(1), 10451216379200822465);
    assert_eq!(splitmix64(42), 13679457532755275413);
}

#[test]
fn the_seed_counter_sides_value_table() {
    // Computed independently in python3 with
    // arbitrary-precision integers; the two's-complement XOR (asUintN) and
    // the u128 widening multiply must reproduce every row exactly.
    const VECTORS: &[(i64, i64, i64, i64)] = &[
        // seed, counter, sides, die
        (0, 0, 6, 6),
        (0, 1, 6, 4),
        (0, 2, 6, 4),
        (0, 3, 6, 1),
        (0, 4, 6, 3),
        (1, 0, 6, 4),
        (1, 1, 6, 6),
        (1, 2, 6, 1),
        (42, 0, 6, 5),
        (42, 1, 6, 5),
        (42, 2, 6, 2),
        (7, 5, 20, 12),
        (0, 100, 6, 1),
        (0, 101, 6, 5),
        (0, 102, 6, 1),
        (9007199254740991, 3, 6, 5), // 2^53-1: exercises the full BigInt path
        (0, 0, 2, 2),
        (123456789, 12, 10, 10),
        (2147483648, 7, 4, 1),
        (65536, 999, 8, 4),
        (0, 999999, 6, 3),
        // negative seed: the key is the seed's 64-bit TWO'S COMPLEMENT
        // (asUintN) XORed with the counter — pinned so the treatment can
        // never drift
        (-1, 0, 6, 6),
        (-1, 1, 6, 6),
        (-42, 7, 20, 13),
        (9007199254740991, 0, 6, 1), // 2^53-1 at counter 0
        (0, 1099511627776, 6, 1),    // counter 2^40 — the full 64-bit key path
        (42, 1099511627783, 20, 7),  // 2^40+7
    ];
    for &(seed, counter, sides, want) in VECTORS {
        assert_eq!(roll_die(seed, counter, sides), want, "roll_die({seed}, {counter}, {sides})");
    }
}

#[test]
fn die_values_stay_in_1_to_sides_and_every_value_of_a_d2_occurs() {
    for c in 0..200 {
        let d6 = roll_die(9, c, 6);
        assert!((1..=6).contains(&d6), "d6 at counter {c} gave {d6}");
    }
    let mut faces: Vec<i64> = (0..64).map(|c| roll_die(3, c, 2)).collect();
    faces.sort_unstable();
    faces.dedup();
    assert_eq!(faces, vec![1, 2]);
}

// ---------------------------------------------------------------------------
// determinism (§4.4): the dice are a pure function of (seed, counter)
// ---------------------------------------------------------------------------

#[test]
fn different_seed_different_sequence() {
    let dict = dice_dict();
    let rows: Vec<Option<(i64, i64)>> = [0, 1, 42]
        .map(|seed| {
            let g = compile_script(&dice_script(seed), &dict).unwrap();
            let r = settle_line(&g.content, &dict, g.state, "0:0 @ ANA WAIT", 1).unwrap();
            rolled_row(done(&r).unwrap())
        })
        .to_vec();
    assert_eq!(rows[0], Some((14, 3)));
    assert_eq!(rows[1], Some((11, 3))); // 4+6+1
    assert_eq!(rows[2], Some((12, 3))); // 5+5+2
    assert_ne!(rows[0], rows[1]);
    assert_ne!(rows[0], rows[2]);
}

#[test]
fn the_meta_seed_lands_in_content_default_is_0_a_bad_seed_is_a_load_error() {
    let dict = dice_dict();
    assert_eq!(compile_script(&dice_script(42), &dict).unwrap().content.seed, 42);
    assert_eq!(compile_script(&dice_script(0), &dict).unwrap().content.seed, 0);
    assert_eq!(compile_content(&ContentRaw::default(), &dict, &[]).unwrap().seed, 0);
    assert_err_msg(
        compile_script(&dice_script(0).replace("seed: 0", "seed: six"), &dict),
        "seed must be an integer",
    );
}

// ---------------------------------------------------------------------------
// the ROLL op: expression args and tier selection (§4.4)
// ---------------------------------------------------------------------------

#[test]
fn roll_parses_into_vexpr_args_arity_is_pinned_parens_required() {
    let dict = dice_dict();
    let out = parse_outcomes(
        "ROLL(total(actors, aspect:nerve), 6, 2), ROLL(count(name:X), object.aspect.dc)",
        &dict,
        None,
    )
    .unwrap();
    let ops: Vec<&str> = out.iter().map(|o| o.op.as_str()).collect();
    assert_eq!(ops, ["ROLL", "ROLL"]);
    let tags: Vec<&str> = out[0].args.iter().map(|a| a.tag()).collect();
    assert_eq!(tags, ["vexpr", "vexpr", "vexpr"]);
    assert_err_msg(parse_outcomes("ROLL(2, 6, 0, 0)", &dict, None), "ROLL takes");
    assert_err_msg(parse_outcomes("ROLL(2)", &dict, None), "ROLL takes");
    assert_err_msg(parse_outcomes("ROLL 2, 6", &dict, None), "paren form");
    assert_err_msg(parse_outcomes("ROLL()", &dict, None), "ROLL takes");
    // a comma inside total(...) never splits the args (depth-aware scan)
    assert_eq!(out[0].args.len(), 3);
}

const TIERS: &str = "ROLL(total(actors, aspect:nerve), 6), \
                     IF roll >= 15 -> (NARRATE \"high\") ELSE IF roll >= 8 -> (NARRATE \"mid\") ELSE -> (NARRATE \"low\")";

#[test]
fn tier_selection_roll_with_if_else_if_else_fires_the_mid_tier() {
    // ANA carries nerve:3 in the seed world -> 3 dice at counters 0,1,2 = 6+4+4 = 14
    let dict = dice_dict();
    let c = inspect_content(&dict, &[TIERS], Extra::default());
    let r = settle_line(&c, &dict, seed_world(), "0:0 @ ANA INSPECT", 1).unwrap();
    assert_eq!(r.rejected, None);
    assert_eq!(rolled_row(done(&r).unwrap()), Some((14, 3)));
    assert_eq!(said_record(done(&r).unwrap()), ["mid"]);
    assert_eq!(r.state.rng, 3);
}

#[test]
fn the_same_tiers_fire_low_from_a_forced_counter() {
    // force the counter to 100 by folding a recorded roll; counters 100..102 = 1+5+1 = 7
    let dict = dice_dict();
    let c = inspect_content(&dict, &[TIERS], Extra::default());
    let forced =
        fold(&[fixture_complete(1, vec![rolled_outcome(0, 100)])], &seed_world(), &dict, &c)
            .unwrap();
    assert_eq!(forced.rng, 100);
    let r = settle_line(&c, &dict, forced, "0:0 @ ANA INSPECT", 2).unwrap();
    assert_eq!(r.rejected, None);
    assert_eq!(rolled_row(done(&r).unwrap()), Some((7, 103)));
    assert_eq!(said_record(done(&r).unwrap()), ["low"]);
}

#[test]
fn bonus_expression_adds_without_consuming_counters_count_0_rolls_no_dice() {
    let dict = dice_dict();
    // 5 + 6 + 4 = 15 at counters 0,1 — two consumed
    let bonus =
        inspect_content(&dict, &["ROLL(2, 6, 5), NARRATE \"got {roll}\""], Extra::default());
    let rb = settle_line(&bonus, &dict, seed_world(), "0:0 @ ANA INSPECT", 1).unwrap();
    assert_eq!(rb.rejected, None);
    assert_eq!(rolled_row(done(&rb).unwrap()), Some((15, 2)));
    assert_eq!(said_record(done(&rb).unwrap()), ["got 15"]);
    // a computed bonus too: object.aspect.demand = 3 (RITE_OF_ASCENSION)
    let bonus_expr = inspect_content(
        &dict,
        &["ROLL(1, 6, object.aspect.demand), NARRATE \"got {roll}\""],
        Extra::default(),
    );
    let re =
        settle_line(&bonus_expr, &dict, seed_world(), "0:0 @ ANA INSPECT RITE_OF_ASCENSION", 1)
            .unwrap();
    assert_eq!(rolled_row(done(&re).unwrap()), Some((9, 1))); // 6 (die at c0) + 3
                                                              // count 0: total is the bonus, counter unmoved, step still fine
    let zero = inspect_content(&dict, &["ROLL(0, 6, 2)"], Extra::default());
    let rz = settle_line(&zero, &dict, seed_world(), "0:0 @ ANA INSPECT", 1).unwrap();
    assert_eq!(rz.rejected, None);
    assert_eq!(rolled_row(done(&rz).unwrap()), Some((2, 0)));
    assert_eq!(rz.state.rng, 0);
}

#[test]
fn count_name_and_total_arg_forms_evaluate_0_count_is_legal() {
    let dict = dice_dict();
    // the count(name:…) arg form: the seed world holds one RITE_OF_ASCENSION
    let c = inspect_content(&dict, &["ROLL(count(name:RITE_OF_ASCENSION), 6)"], Extra::default());
    let r = settle_line(&c, &dict, seed_world(), "0:0 @ ANA INSPECT", 1).unwrap();
    assert_eq!(rolled_row(done(&r).unwrap()), Some((6, 1))); // one die, value 6
                                                             // a computed count of 0 rolls no dice: total over a missing aspect reads 0
    let zero = inspect_content(
        &dict,
        &["ROLL(total(actors, aspect:nonesuch), 6), NARRATE \"after\""],
        Extra::default(),
    );
    let rz = settle_line(&zero, &dict, seed_world(), "0:0 @ ANA INSPECT", 1).unwrap();
    assert_eq!(rz.rejected, None);
    assert_eq!(rolled_row(done(&rz).unwrap()), Some((0, 0)));
    let ops: Vec<&str> = done(&rz).unwrap().outcomes.iter().map(|o| o.op.as_str()).collect();
    assert_eq!(ops, ["ROLLED", "NARRATE"]);
    assert_eq!(rz.state.rng, 0);
}

#[test]
fn an_operand_that_cannot_resolve_missing_attr_refuses_the_window() {
    let dict = dice_dict();
    let attr = inspect_content(
        &dict,
        &["ROLL(actors.attr.nonesuch, 6), NARRATE \"never\""],
        Extra::default(),
    );
    let ra = settle_line(&attr, &dict, seed_world(), "0:0 @ ANA INSPECT", 1).unwrap();
    // v0.5: the error defers to the completion, where the window refuses —
    // the attempt is logged, the scratch (and its counters) discarded
    assert_eq!(ra.rejected, None);
    assert_refusal(&ra, "ROLL: dice count must be an integer >= 0");
    assert_eq!(ra.state.rng, 0);
}

#[test]
fn roll_arguments_get_the_count_dread_typo_lint_roll_and_rolled_are_reserved() {
    let dict = dice_dict();
    assert_err_msg(
        try_inspect_content(&dict, &["ROLL(count(DREAD), 6)"], Extra::default()),
        r"count\(\) argument 'DREAD' is neither a role",
    );
    assert_err_msg(
        try_inspect_content(&dict, &["ROLL(total(DREAD, aspect:nerve), 6)"], Extra::default()),
        r"total\(\) first argument 'DREAD' is not a declared role",
    );
    let _ = inspect_content(&dict, &["ROLL(count(name:DREAD), 6)"], Extra::default()); // compiles clean
                                                                                       // reserved words: an entity named ROLL needs quotes; a function cannot take the op's name
    assert!(dict.reserved.contains("ROLL"));
    assert!(dict.reserved.contains("ROLLED"));
    assert_err_msg(parse_sentence("0:0 @ ANA INSPECT ROLL", &dict), "reserved");
    assert_eq!(parse_outcomes("SPAWN \"ROLLED\"", &dict, None).unwrap().len(), 1); // the quote is the escape hatch
    let raw = ContentRaw {
        functions: Some(vec![FunctionDef {
            name: "ROLL".into(),
            params: vec![],
            body: "NARRATE \"x\"".into(),
        }]),
        ..ContentRaw::default()
    };
    assert_err_msg(compile_content(&raw, &dict, &[]), "collides with an outcome op");
}

#[test]
fn a_roll_inside_an_untaken_branch_consumes_nothing() {
    // apply_if only applies the taken arm — the untaken branch's ROLL never
    // runs, so no counters burn and no ROLLED row exists
    let dict = dice_dict();
    let c = inspect_content(
        &dict,
        &["IF exists(NOSUCH) -> (ROLL(2, 6)) ELSE -> (NARRATE \"quiet\")"],
        Extra::default(),
    );
    let state = seed_world();
    let base = clone_for_snap(&state);
    let r = settle_line(&c, &dict, state, "0:0 @ ANA INSPECT", 1).unwrap();
    assert_eq!(r.rejected, None);
    let ops: Vec<&str> = done(&r).unwrap().outcomes.iter().map(|o| o.op.as_str()).collect();
    assert_eq!(ops, ["NARRATE"]);
    assert_eq!(r.state.rng, 0);
    // §6.4: the window completed — the step counted (the one snap delta;
    // the untaken branch's ROLL never ran)
    assert_eq!(r.state.executed.get("inspect.default"), Some(&1));
    let replayed = fold(&r.records, &base, &dict, &c).unwrap();
    assert_eq!(snap(&replayed), snap(&r.state));
}

#[test]
fn a_failing_later_outcome_after_a_successful_roll_refuses_the_window() {
    // the first ROLL succeeds and consumes counters ON THE SCRATCH; the
    // second ROLL's sides < 2 then refuses the window — the attempt is
    // logged, the real counter unmoved (the scratch died with the refusal,
    // exactly like a REFUSE's)
    let dict = dice_dict();
    let c = inspect_content(
        &dict,
        &["ROLL(2, 6), NARRATE \"mid {roll}\", ROLL(0, 1)"],
        Extra::default(),
    );
    let state = seed_world();
    let r = settle_line(&c, &dict, clone_for_snap(&state), "0:0 @ ANA INSPECT", 1).unwrap();
    assert_eq!(r.rejected, None);
    assert_refusal(&r, "sides must be an integer >= 2");
    assert_eq!(state.rng, 0);
    // §6.4: the window registered at begin — the completion-time refusal
    // still counted (the one delta; the scratch counter died with it)
    assert_eq!(r.state.executed.get("inspect.default"), Some(&1));
    // fold agreement: a REFUSE row is a state no-op, so the counter stays put
    let replayed = fold(&r.records, &state, &dict, &c).unwrap();
    assert_eq!(replayed.rng, 0);
    assert_eq!(snap(&replayed), snap(&r.state));
}

/// test-side clone helper.
fn clone_for_snap(s: &State) -> State {
    clone_state(s)
}

#[test]
fn sides_below_2_and_count_below_0_refuse_the_window_nothing_moved() {
    let dict = dice_dict();
    let state = seed_world();
    let bad_sides = inspect_content(&dict, &["ROLL(1, 1), NARRATE \"never\""], Extra::default());
    let rs =
        settle_line(&bad_sides, &dict, clone_for_snap(&state), "0:0 @ ANA INSPECT", 1).unwrap();
    assert_eq!(rs.rejected, None);
    assert_refusal(&rs, "sides must be an integer >= 2");
    assert_eq!(state.rng, 0);
    // §6.4: the window registered at begin — the completion-time refusal
    // still counted (the one delta)
    assert_eq!(rs.state.executed.get("inspect.default"), Some(&1));
    let base = clone_for_snap(&state);
    let replayed = fold(&rs.records, &base, &dict, &bad_sides).unwrap();
    assert_eq!(snap(&replayed), snap(&rs.state));
    // negative count via a negative aspect (entity lines allow -n)
    let neg = compile_content(
        &ContentRaw {
            verbs: [(
                "INSPECT".to_string(),
                VerbContent {
                    slots: vec![SlotDef {
                        role: "actors".into(),
                        min: 1,
                        max: Some(1),
                        query: Some("kind:character".into()),
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
                        dur: None,
                        out: vec!["ROLL(actors.aspect.dread, 6), NARRATE \"never\"".to_string()],
                    }],
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
    let mut s2 = seed_world();
    s2.entities.get_mut("ana").unwrap().aspects.insert("dread".to_string(), AspectVal::Num(-3));
    let rn = settle_line(&neg, &dict, clone_for_snap(&s2), "0:0 @ ANA INSPECT", 1).unwrap();
    assert_eq!(rn.rejected, None);
    assert_refusal(&rn, "dice count must be an integer >= 0");
    // §6.4: registered at begin — the refusal still counted
    assert_eq!(rn.state.executed.get("inspect.default"), Some(&1));
    let replayed_neg = fold(&rn.records, &s2, &dict, &neg).unwrap();
    assert_eq!(snap(&replayed_neg), snap(&rn.state));
    // and a fractional/missing operand is likewise a rejection, never a silent roll
    let missing = inspect_content(&dict, &["ROLL(object.aspect.nonesuch, 6)"], Extra::default());
    let rm = settle_line(&missing, &dict, seed_world(), "0:0 @ ANA INSPECT", 1).unwrap(); // no object bound
    assert_refusal(&rm, "cannot resolve");
}

// ---------------------------------------------------------------------------
// the roll binding (§4.4): last-wins, per-clause, unbound = rejection
// ---------------------------------------------------------------------------

#[test]
fn last_wins_a_second_roll_rebinds_both_consumed_counters_show_in_the_rows() {
    let dict = dice_dict();
    let c = inspect_content(
        &dict,
        &["ROLL(1, 6), ROLL(1, 6), NARRATE \"final {roll}\""],
        Extra::default(),
    );
    let r = settle_line(&c, &dict, seed_world(), "0:0 @ ANA INSPECT", 1).unwrap();
    assert_eq!(r.rejected, None);
    assert_eq!(rolled_rows(done(&r).unwrap()), vec![(6, 1), (4, 2)]);
    assert_eq!(said_record(done(&r).unwrap()), ["final 4"]); // die(0,1,6) = 4, not the first roll's 6
    assert_eq!(r.state.rng, 2);
}

#[test]
fn a_roll_does_not_cross_clauses_clause_two_read_is_a_load_error() {
    // clause one rolls; clause two's list provably has no ROLL of its own —
    // with the per-clause reset that read is dead on arrival, and the may-bind
    // lint proves it: the recipe cannot even compile
    let dict = dice_dict();
    let raw = ContentRaw {
        verbs: [
            (
                "INSPECT".to_string(),
                VerbContent {
                    slots: vec![actors_slot()],
                    duration: 0,
                    recipes: vec![default_recipe(&["ROLL(1, 6), NARRATE \"clause one {roll}\""])],
                },
            ),
            (
                "ATTUNE".to_string(),
                VerbContent {
                    slots: vec![actors_slot()],
                    duration: 0,
                    recipes: vec![default_recipe(&[
                        "IF roll >= 0 -> (NARRATE \"clause two {roll}\")",
                    ])],
                },
            ),
        ]
        .into_iter()
        .collect(),
        ..ContentRaw::default()
    };
    assert_err_msg(
        compile_content(&raw, &dict, &[]),
        r"recipe attune\.default: 'roll' is read but no ROLL can have run before it",
    );
}

fn actors_slot() -> SlotDef {
    SlotDef {
        role: "actors".into(),
        min: 1,
        max: Some(1),
        query: Some("kind:character".into()),
        shown: None,
    }
}

fn default_recipe(out: &[&str]) -> RecipeDef {
    RecipeDef {
        id: None,
        when: None,
        shown: None,
        hint: None,
        max: None,
        aspects: Vec::new(),
        dur: None,
        out: out.iter().map(|s| (*s).to_string()).collect(),
    }
}

#[test]
fn each_clause_binds_its_own_dice_the_counter_chain_continues_across_clauses() {
    let dict = dice_dict();
    let c = compile_content(
        &ContentRaw {
            verbs: [
                (
                    "INSPECT".to_string(),
                    VerbContent {
                        slots: vec![actors_slot()],
                        duration: 0,
                        recipes: vec![default_recipe(&[
                            "ROLL(1, 6), NARRATE \"clause one {roll}\"",
                        ])],
                    },
                ),
                (
                    "ATTUNE".to_string(),
                    VerbContent {
                        slots: vec![actors_slot()],
                        duration: 0,
                        recipes: vec![default_recipe(&[
                            "ROLL(1, 6), NARRATE \"clause two {roll}\"",
                        ])],
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
    let r = settle_line(&c, &dict, seed_world(), "0:0 @ ANA INSPECT; ANA ATTUNE", 1).unwrap();
    assert_eq!(r.rejected, None);
    // clause one rolls die(0,0,6)=6; clause two rolls die(0,1,6)=4 at the
    // CONTINUED counter — shared counter, independent bindings; each clause's
    // window closes with its own COMPLETE row
    let completes: Vec<&ActionRecord> = r.records.iter().filter(|x| x.kind == "complete").collect();
    let narrated: Vec<String> = completes.iter().flat_map(|x| said_record(x)).collect();
    assert_eq!(narrated, ["clause one 6", "clause two 4"]);
    let rows: Vec<(i64, i64)> = completes.iter().flat_map(|x| rolled_rows(x)).collect();
    assert_eq!(rows, vec![(6, 1), (4, 2)]);
}

#[test]
fn unbound_roll_in_a_taken_branch_refuses_the_window_at_runtime() {
    // branch one may bind (so the lint passes); the ELSE path never rolls
    let dict = dice_dict();
    let c = inspect_content(
        &dict,
        &["IF exists(HOUSE) -> (ROLL(1, 6)) ELSE -> (NARRATE \"no house\"), IF roll >= 0 -> (NARRATE \"read {roll}\")"],
        Extra::default(),
    );
    let with_house = settle_line(&c, &dict, seed_world(), "0:0 @ ANA INSPECT", 1).unwrap();
    assert_eq!(with_house.rejected, None);
    assert_eq!(said_record(done(&with_house).unwrap()), ["read 6"]); // rolled, so `roll` reads
    let without_house = inspect_content(
        &dict,
        &["IF exists(NOSUCH) -> (ROLL(1, 6)) ELSE -> (NARRATE \"skip\"), IF roll >= 0 -> (NARRATE \"read {roll}\")"],
        Extra::default(),
    );
    let r = settle_line(&without_house, &dict, seed_world(), "0:0 @ ANA INSPECT", 1).unwrap();
    assert_eq!(r.rejected, None);
    assert_refusal(&r, "'roll' is unbound");
}

#[test]
fn roll_as_a_roll_argument_with_no_prior_roll_refuses_the_window_at_runtime() {
    // the may-bind lint scans IF conditions only — a ROLL argument reading
    // `roll` loads fine and is caught by the runtime refusal (false
    // negatives are allowed; the refusal is the backstop)
    let dict = dice_dict();
    let c = inspect_content(&dict, &["ROLL(roll, 6)"], Extra::default());
    let r = settle_line(&c, &dict, seed_world(), "0:0 @ ANA INSPECT", 1).unwrap();
    assert_eq!(r.rejected, None);
    assert_refusal(&r, "'roll' is unbound");
}

#[test]
fn a_narrate_in_a_later_clause_keeps_the_roll_literal_the_binding_resets_per_clause() {
    // clause one rolls and its NARRATE rewrites; clause two's NARRATE runs
    // after the per-clause reset with no ROLL of its own — the token stays
    // literal text in the journal (documented §4.4 ruling)
    let dict = dice_dict();
    let c = compile_content(
        &ContentRaw {
            verbs: [
                (
                    "INSPECT".to_string(),
                    VerbContent {
                        slots: vec![actors_slot()],
                        duration: 0,
                        recipes: vec![default_recipe(&[
                            "ROLL(1, 6), NARRATE \"clause one {roll}\"",
                        ])],
                    },
                ),
                (
                    "ATTUNE".to_string(),
                    VerbContent {
                        slots: vec![actors_slot()],
                        duration: 0,
                        recipes: vec![default_recipe(&["NARRATE \"clause two {roll}\""])],
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
    let r = settle_line(&c, &dict, seed_world(), "0:0 @ ANA INSPECT; ANA ATTUNE", 1).unwrap();
    assert_eq!(r.rejected, None);
    let completes: Vec<&ActionRecord> = r.records.iter().filter(|x| x.kind == "complete").collect();
    let narrated: Vec<String> = completes.iter().flat_map(|x| said_record(x)).collect();
    assert_eq!(narrated, ["clause one 6", "clause two {roll}"]);
}

#[test]
fn when_predicates_cannot_read_roll_if_conditions_can() {
    let dict = dice_dict();
    // a recipe IF with no possible prior ROLL: the may-bind error
    assert_err_msg(
        try_inspect_content(&dict, &["IF roll >= 3 -> (NARRATE \"x\")"], Extra::default()),
        r"recipe inspect\.default: 'roll' is read but no ROLL can have run before it",
    );
    // the same operand in a when: line: the string front-end refuses it outright
    let raw = ContentRaw {
        verbs: [(
            "INSPECT".to_string(),
            VerbContent {
                slots: vec![actors_slot()],
                duration: 0,
                recipes: vec![
                    RecipeDef {
                        id: Some("inspect.gate".into()),
                        when: Some("roll >= 3".into()),
                        shown: None,
                        hint: None,
                        max: None,
                        aspects: Vec::new(),
                        dur: None,
                        out: vec!["NARRATE(\"gate\")".to_string()],
                    },
                    default_recipe(&["NARRATE(\"default\")"]),
                ],
            },
        )]
        .into_iter()
        .collect(),
        ..ContentRaw::default()
    };
    assert_err_msg(
        compile_content(&raw, &dict, &[]),
        "'roll' is not available in 'when:' predicates",
    );
}

// ---------------------------------------------------------------------------
// compile lint: roll without a provable prior ROLL is a load error (§4.4)
// ---------------------------------------------------------------------------

#[test]
fn provable_lint_cases_name_the_recipe_a_preceding_roll_clears_them() {
    let dict = dice_dict();
    assert_err_msg(
        try_inspect_content(&dict, &["IF roll < 3 -> (NARRATE \"low\")"], Extra::default()),
        r"recipe inspect\.default: 'roll' is read but no ROLL can have run before it",
    );
    let _ = inspect_content(
        &dict,
        &["ROLL(1, 6), IF roll < 3 -> (NARRATE \"low\") ELSE -> (NARRATE \"high\")"],
        Extra::default(),
    ); // compiles clean
       // nested in a branch with no ROLL before it, inside the branch
    assert_err_msg(
        try_inspect_content(
            &dict,
            &["IF exists(HOUSE) -> (IF roll < 3 -> (NARRATE \"deep\"))"],
            Extra::default(),
        ),
        "no ROLL can have run before it",
    );
    // a ROLL before the nested IF clears the inner reference
    let _ = inspect_content(
        &dict,
        &["ROLL(1, 6), IF exists(HOUSE) -> (IF roll < 3 -> (NARRATE \"deep\"))"],
        Extra::default(),
    ); // compiles clean
}

#[test]
fn no_false_positive_for_a_branch_hidden_prior_roll() {
    // the first IF's branch MIGHT roll — the lint cannot prove otherwise
    let dict = dice_dict();
    let _ = inspect_content(
        &dict,
        &["IF exists(HOUSE) -> (ROLL(1, 6)) ELSE -> (NARRATE \"skip\"), IF roll >= 3 -> (NARRATE \"big\") ELSE -> (NARRATE \"small\")"],
        Extra::default(),
    ); // compiles clean
}

// ---------------------------------------------------------------------------
// {roll} interpolation (§4.4): rewritten at execution, load-fixed elsewhere
// ---------------------------------------------------------------------------

#[test]
fn the_concrete_journal_row_carries_the_number() {
    let dict = dice_dict();
    let c = inspect_content(
        &dict,
        &["ROLL(3, 6), NARRATE \"the bones say {roll}.\""],
        Extra::default(),
    );
    let r = settle_line(&c, &dict, seed_world(), "0:0 @ ANA INSPECT", 1).unwrap();
    assert_eq!(r.rejected, None);
    assert_eq!(said_record(done(&r).unwrap()), ["the bones say 14."]);
    // the recorded SENTENCE carries the number — the fold never rewrites prose
    let sentence = &done(&r).unwrap().sentence;
    assert!(sentence.contains("NARRATE \"the bones say 14.\""), "{sentence}");
    let reparsed = parse_sentence(sentence, &dict).unwrap();
    assert_eq!(format_sentence(&reparsed, &dict).unwrap(), *sentence);
}

#[test]
fn a_narrate_before_the_roll_keeps_the_literal_token_after_a_second_roll_it_rebinds() {
    let dict = dice_dict();
    let c = inspect_content(
        &dict,
        &["NARRATE \"early {roll}\", ROLL(1, 6), NARRATE \"late {roll}\""],
        Extra::default(),
    );
    let r = settle_line(&c, &dict, seed_world(), "0:0 @ ANA INSPECT", 1).unwrap();
    assert_eq!(said_record(done(&r).unwrap()), ["early {roll}", "late 6"]);
}

#[test]
fn roll_in_progress_or_refuse_strings_is_a_load_error() {
    let dict = dice_dict();
    assert_err_msg(
        try_inspect_content(
            &dict,
            &["ROLL(1, 6), PROGRESS(30m, \"ticking {roll}\")"],
            Extra::default(),
        ),
        r"recipe inspect\.default: \{roll\} is only available in NARRATE strings — PROGRESS prose is load-fixed",
    );
    assert_err_msg(
        try_inspect_content(
            &dict,
            &["ROLL(1, 6), REFUSE(\"the dice said {roll}\")"],
            Extra::default(),
        ),
        r"recipe inspect\.default: \{roll\} is only available in NARRATE strings — REFUSE prose is load-fixed",
    );
    let _ = inspect_content(&dict, &["ROLL(1, 6), NARRATE \"ok {roll}\""], Extra::default()); // compiles clean
                                                                                              // a function body's PROGRESS {roll} is caught too: expansion runs before
                                                                                              // the lint, so the RECIPE's compile names the offender
    assert_err_msg(
        try_inspect_content(
            &dict,
            &["helper()"],
            Extra {
                functions: Some(vec![FunctionDef {
                    name: "helper".into(),
                    params: vec![],
                    body: "PROGRESS(30m, \"ticking {roll}\")".into(),
                }]),
                ..Extra::default()
            },
        ),
        r"recipe inspect\.default: \{roll\} is only available in NARRATE strings",
    );
}

// ---------------------------------------------------------------------------
// ROLLED (§4.4): engine-written, parse+format round-trip, fold sets rng
// ---------------------------------------------------------------------------

#[test]
fn the_concrete_rolled_row_round_trips_through_parse_and_format() {
    let dict = dice_dict();
    let c = inspect_content(&dict, &["ROLL(3, 6)"], Extra::default());
    let r = settle_line(&c, &dict, seed_world(), "0:0 @ ANA INSPECT", 1).unwrap();
    let row = done(&r).unwrap().sentence.clone();
    assert!(row.contains("ROLLED(14, 3)"), "{row}");
    let reparsed = parse_sentence(&row, &dict).unwrap();
    assert_eq!(format_sentence(&reparsed, &dict).unwrap(), row);
    // and the parse entry pins the arity/shape
    assert_err_msg(parse_outcomes("ROLLED(1)", &dict, None), "ROLLED takes exactly two integers");
    assert_err_msg(
        parse_outcomes("ROLLED(1, 2, 3)", &dict, None),
        "ROLLED takes exactly two integers",
    );
    // non-int args cannot even get through the generic arg grammar
    assert!(parse_outcomes("ROLLED(total(actors, aspect:nerve), 3)", &dict, None).is_err());
}

#[test]
fn authoring_rolled_in_content_is_a_load_error_engine_written_only() {
    let dict = dice_dict();
    assert_err_msg(
        try_inspect_content(&dict, &["ROLLED(14, 3)"], Extra::default()),
        "ROLLED is engine-written",
    );
    assert_err_msg(
        try_inspect_content(&dict, &["IF exists(HOUSE) -> (ROLLED(1, 2))"], Extra::default()),
        "ROLLED is engine-written",
    );
    let trigger_raw = ContentRaw {
        triggers: Some(vec![TriggerDef::Slot(SlotTriggerDef {
            id: "t".into(),
            slot: 3,
            once: None,
            out: vec!["ROLLED(1, 2)".to_string()],
        })]),
        ..ContentRaw::default()
    };
    assert_err_msg(
        compile_content(&trigger_raw, &dict, &[]),
        "trigger t: ROLLED is engine-written",
    );
    // and inside a FUNCTION body: expansion runs before the lint, so the call
    // site's compile names the recipe, not the function
    assert_err_msg(
        try_inspect_content(
            &dict,
            &["helper()"],
            Extra {
                functions: Some(vec![FunctionDef {
                    name: "helper".into(),
                    params: vec![],
                    body: "ROLLED(1, 2)".into(),
                }]),
                ..Extra::default()
            },
        ),
        "recipe inspect\\.default: ROLLED is engine-written",
    );
}

#[test]
fn fold_of_a_bare_rolled_row_moves_only_the_counter_snap_probe() {
    // hand-feeding one ROLLED row over a fresh world must equal the same
    // world with rng set by hand — no other field may move
    let dict = dice_dict();
    let c = inspect_content(&dict, &["NARRATE \"x\""], Extra::default());
    let base = seed_world();
    let forced =
        fold(&[fixture_complete(1, vec![rolled_outcome(11, 55)])], &clone_state(&base), &dict, &c)
            .unwrap();
    let mut expected = clone_state(&base);
    expected.rng = 55;
    assert_eq!(snap(&forced), snap(&expected));
}

#[test]
fn fold_sets_state_rng_from_the_row_and_nothing_else() {
    let dict = dice_dict();
    let c = inspect_content(&dict, &["ROLL(3, 6)"], Extra::default());
    let r = settle_line(&c, &dict, seed_world(), "0:0 @ ANA INSPECT", 1).unwrap();
    let replayed = fold(&r.records, &seed_world(), &dict, &c).unwrap();
    assert_eq!(replayed.rng, 3);
    assert_eq!(snap(&replayed), snap(&r.state));
    // a hand-written ROLLED row lands the counter wherever its (total,
    // counter-after) args say
    let forced =
        fold(&[fixture_complete(1, vec![rolled_outcome(11, 55)])], &seed_world(), &dict, &c)
            .unwrap();
    assert_eq!(forced.rng, 55);
}

// ---------------------------------------------------------------------------
// REFUSE after ROLL (§4.4): play and fold agree the counter is unmoved
// ---------------------------------------------------------------------------

#[test]
fn refuse_after_roll_zero_time_attempt_logged_counter_unmoved_in_play_and_fold() {
    let dict = dice_dict();
    let c = inspect_content(
        &dict,
        &["ROLL(2, 6), NARRATE \"considering {roll}...\", REFUSE(\"not yet\")"],
        Extra::default(),
    );
    let state = seed_world();
    let before = snap(&state);
    let r = settle_line(&c, &dict, clone_for_snap(&state), "0:0 @ ANA INSPECT", 1).unwrap();
    assert_eq!(r.rejected, None);
    assert_eq!(r.records.len(), 1);
    let ops: Vec<&str> = done(&r).unwrap().outcomes.iter().map(|o| o.op.as_str()).collect();
    assert_eq!(ops, ["ROLLED", "NARRATE", "REFUSE"]);
    assert_eq!(rolled_row(done(&r).unwrap()), Some((10, 2))); // the dice WERE thrown (6+4)
    assert_eq!(said_record(done(&r).unwrap()), ["considering 10..."]); // journal-visible
    assert_eq!(done(&r).unwrap().advanced_min, 0);
    // `r.state === state`: the draft died with the refusal — the input
    // state's value rides back untouched (identity → snap equality)
    assert_eq!(r.state.rng, 0); // so the counter never moved in play
    assert_eq!(snap(&r.state), before);
    // and the fold treats the refused record as a no-op: counter unmoved there too
    let replayed = fold(&r.records, &state, &dict, &c).unwrap();
    assert_eq!(replayed.rng, 0);
    assert_eq!(snap(&replayed), before);
}

// ---------------------------------------------------------------------------
// undo reconstruction (§4.4): the counter rebuilds from the folded log
// ---------------------------------------------------------------------------

fn undo_script() -> String {
    r#"
script: Undo Rig
player: ANA
start: 0:0
seed: 0

entities:
  ANA is character

verbs:
  WAIT:
    actors: 1 kind:character
    duration: 0m
    recipes:
      default -> ROLL(3, 6), NARRATE "waited on {roll}."
  DREAM:
    actors: 1 kind:character
    duration: 0m
    recipes:
      default -> ROLL(1, 6), NARRATE "dreamed of {roll}."
"#
    .to_string()
}

#[test]
fn play_undo_past_the_roll_different_action_save_then_fold_load_log_equals_state() {
    let mut game = Game::from_script(&undo_script(), dice_dict()).unwrap();
    // step 1: WAIT rolls 3 dice (counters 0..2, total 14) — this is step k.
    // v0.5: the roll lands when the window completes, so settle it
    let t1 = game.handle_line("wait").unwrap();
    assert_eq!(t1.kind, TurnKind::Action);
    game.wait().unwrap();
    assert_eq!(game.state.rng, 3);
    // undo past k: the reconstructed state's counter is folded, not remembered.
    // Two undos — the settle turn and the begin turn — leave no stale window
    game.undo().unwrap();
    game.undo().unwrap();
    assert_eq!(game.state.rng, 0);
    assert_eq!(game.log.records.len(), 0);
    // a DIFFERENT action with a roll: DREAM rolls 1 die — at counter 0 again,
    // i.e. the reconstructed counter, not the discarded history's
    let t2 = game.handle_line("dream").unwrap();
    assert_eq!(t2.kind, TurnKind::Action);
    game.wait().unwrap();
    assert_eq!(game.state.rng, 1);
    // the die equals the vector value at (seed 0, counter 0, d6) — the same a
    // fresh run's FIRST roll would produce: reconstruction, not drift
    assert_eq!(roll_die(0, 0, 6), 6);
    assert!(game.journal().join("\n").contains("ROLLED(6, 1)"));
    // save the whole log and fold it back: byte-identical state, rng included
    let path = tmp_log_path("dice-undo");
    save_log(&path, &game.log.records).unwrap();
    let replayed = fold(
        &load_log(&path).unwrap(),
        &clone_state(&game.log.initial_state),
        &game.dict,
        &game.content,
    )
    .unwrap();
    assert_eq!(snap(&replayed), snap(&game.state));
    let _ = std::fs::remove_file(&path);
}

#[test]
fn fold_up_to_rebuilds_the_counter_mid_log_without_the_tail() {
    let dict = dice_dict();
    let compiled = compile_script(&undo_script(), &dict).unwrap();
    let base = clone_state(&compiled.state);
    let r1 =
        settle_line(&compiled.content, &dict, clone_state(&compiled.state), "0:0 @ ANA WAIT", 1)
            .unwrap(); // rng 0 -> 3
    let r2 = settle_line(
        &compiled.content,
        &dict,
        r1.state,
        "0:0 @ ANA DREAM",
        1 + r1.records.len() as i64,
    )
    .unwrap(); // rng 3 -> 4
    assert_eq!(r2.state.rng, 4);
    let all: Vec<ActionRecord> = r1.records.iter().chain(r2.records.iter()).cloned().collect();
    let mid =
        fold_up_to(&all, &base, &dict, &compiled.content, 1 + r1.records.len() as i64).unwrap();
    assert_eq!(mid.rng, 3); // exactly where the folded history left it
    let folded_all = fold_up_to(&all, &base, &dict, &compiled.content, 99).unwrap();
    assert_eq!(folded_all.rng, 4);
}

#[test]
fn undo_to_mid_log_keep_playing_the_counter_resumes_from_the_fold() {
    let mut game = Game::from_script(&undo_script(), dice_dict()).unwrap();
    game.handle_line("wait").unwrap(); // rng 0 -> 3
    game.wait().unwrap();
    game.handle_line("dream").unwrap(); // rng 3 -> 4
    game.wait().unwrap();
    assert!(game.journal().join("\n").contains("ROLLED(1, 4)"));
    // drop the DREAM turn (its settle + its begin) — the ROLLED row goes WITH
    // it (no re-roll anywhere), and no stale window is left open
    game.undo().unwrap();
    game.undo().unwrap();
    assert_eq!(game.state.rng, 3);
    assert!(!game.journal().join("\n").contains("DREAM"));
    assert!(!game.journal().join("\n").contains("ROLLED(1, 4)"));
    // a THIRD roll continues from the FOLDED counter: counters 3..5 = 1+5+1 = 7
    game.handle_line("wait").unwrap();
    game.wait().unwrap();
    assert_eq!(game.state.rng, 6);
    assert!(game.journal().join("\n").contains("ROLLED(7, 6)"));
    // the undone-and-continued path is indistinguishable from a straight-line
    // run of the same surviving history — state AND journal
    let mut straight = Game::from_script(&undo_script(), dice_dict()).unwrap();
    straight.handle_line("wait").unwrap();
    straight.wait().unwrap();
    straight.handle_line("wait").unwrap();
    straight.wait().unwrap();
    assert_eq!(snap(&straight.state), snap(&game.state));
    assert_eq!(straight.journal(), game.journal());
    // the save/fold law still holds on the post-undo log
    let path = tmp_log_path("dice-undo-mid");
    save_log(&path, &game.log.records).unwrap();
    let replayed = fold(
        &load_log(&path).unwrap(),
        &clone_state(&game.log.initial_state),
        &game.dict,
        &game.content,
    )
    .unwrap();
    assert_eq!(snap(&replayed), snap(&game.state));
    let _ = std::fs::remove_file(&path);
}

// ---------------------------------------------------------------------------
// triggers roll (§4.4): slot and expiry events may ROLL and tier on roll
// ---------------------------------------------------------------------------

/// Returns the compile `Result`
/// (the compiler throws through, and several tests assert that throw).
fn wait_content(
    dict: &Dictionary,
    triggers: Vec<TriggerDef>,
) -> chorus::error::Result<CompiledContent> {
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
                    recipes: vec![default_recipe(&["NARRATE(\"{actors} wait.\")"])],
                },
            )]
            .into_iter()
            .collect(),
            triggers: Some(triggers),
            prototypes: Some(
                [("OMEN", "idea"), ("DUST", "state")]
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
}

#[test]
fn a_slot_trigger_rolls_tiers_on_roll_and_the_event_row_carries_the_result() {
    // 2 dice at counters 0,1 = 6+4 = 10 -> the high tier
    let dict = dice_dict();
    let c = wait_content(
        &dict,
        vec![TriggerDef::Slot(SlotTriggerDef {
            id: "patrol".into(),
            slot: 6,
            once: Some(true),
            out: vec![
                "ROLL(2, 6), IF roll >= 8 -> (SPAWN OMEN, NARRATE \"an omen stirs ({roll}).\") ELSE -> (NARRATE \"all quiet.\")"
                    .to_string(),
            ],
        })],
    )
    .unwrap();
    let state = seed_world();
    let r = settle_line(&c, &dict, state, "0:0 @ ANA WAIT", 1).unwrap(); // 1h crosses slot 6
    assert_eq!(r.rejected, None);
    let event = r.records.iter().find(|x| x.kind == "event").unwrap();
    assert_eq!(event.trigger.as_deref(), Some("patrol"));
    let ops: Vec<&str> = event.outcomes.iter().map(|o| o.op.as_str()).collect();
    assert_eq!(ops, ["ROLLED", "SPAWN", "NARRATE"]);
    assert_eq!(rolled_row(event), Some((10, 2)));
    assert_eq!(said_record(event), ["an omen stirs (10)."]);
    assert_eq!(r.state.rng, 2);
    // the fold replays the EVENT row's ROLLED exactly
    let replayed = fold(&r.records, &seed_world(), &dict, &c).unwrap();
    assert_eq!(snap(&replayed), snap(&r.state));
}

#[test]
fn an_expiry_trigger_rolls_too_the_card_goes_with_its_authored_outcomes() {
    let dict = dice_dict();
    let c = wait_content(
        &dict,
        vec![TriggerDef::Expire(chorus::content::ExpireTriggerDef {
            id: "chalice_dust".into(),
            on_expire_of: "CHALICE".into(),
            out: vec![
                "ROLL(1, 6), IF roll >= 4 -> (DESTROY CHALICE, SPAWN DUST, NARRATE \"it crumbles ({roll}).\") ELSE -> (NARRATE \"it holds ({roll}).\")"
                    .to_string(),
            ],
        })],
    )
    .unwrap();
    let mut state = seed_world();
    state
        .entities
        .get_mut("chalice")
        .unwrap()
        .attrs
        .insert("expiresAt".to_string(), AttrValue::Num(30)); // crosses slot 1
    let base = clone_state(&state);
    let r = settle_line(&c, &dict, state, "0:0 @ ANA WAIT", 1).unwrap();
    assert_eq!(r.rejected, None);
    let event = r.records.iter().find(|x| x.kind == "event").unwrap();
    assert_eq!(event.trigger.as_deref(), Some("chalice_dust"));
    let ops: Vec<&str> = event.outcomes.iter().map(|o| o.op.as_str()).collect();
    assert_eq!(ops, ["ROLLED", "DESTROY", "SPAWN", "NARRATE"]);
    assert_eq!(rolled_row(event), Some((6, 1))); // die(0,0,6) = 6 -> the crumble tier
    assert_eq!(said_record(event), ["it crumbles (6)."]);
    let replayed = fold(&r.records, &base, &dict, &c).unwrap();
    assert_eq!(snap(&replayed), snap(&r.state));
}

#[test]
fn trigger_roll_arguments_must_be_role_free_load_error_naming_the_argument() {
    let dict = dice_dict();
    let slot_trigger = |out: &str| {
        vec![TriggerDef::Slot(SlotTriggerDef {
            id: "t".into(),
            slot: 3,
            once: None,
            out: vec![out.to_string()],
        })]
    };
    // (the error-path call sites pass wait_content's Result straight to
    // assert_err_msg)
    assert_err_msg(
        wait_content(&dict, slot_trigger("ROLL(count(actors), 6)")),
        r"trigger t: ROLL argument count\(actors\) reads a step role",
    );
    assert_err_msg(
        wait_content(&dict, slot_trigger("ROLL(total(actors, aspect:nerve), 6)")),
        r"trigger t: ROLL argument total\(actors, aspect:nerve\) reads a step role",
    );
    let expiry_trigger = vec![TriggerDef::Expire(chorus::content::ExpireTriggerDef {
        id: "t".into(),
        on_expire_of: "CHALICE".into(),
        out: vec!["ROLL(object.attr.seats, 6)".to_string()],
    })];
    assert_err_msg(
        wait_content(&dict, expiry_trigger),
        r"trigger t: ROLL argument object\.attr\.seats reads a step role",
    );
    // world forms are the trigger vocabulary (compiles clean)
    let _ = wait_content(&dict, slot_trigger("ROLL(count(name:HOUSE), 6)"));
}

#[test]
fn a_trigger_reading_roll_with_no_possible_roll_is_a_load_error() {
    let dict = dice_dict();
    assert_err_msg(
        wait_content(
            &dict,
            vec![TriggerDef::Slot(SlotTriggerDef {
                id: "t".into(),
                slot: 3,
                once: None,
                out: vec!["IF roll >= 4 -> (NARRATE \"x\")".to_string()],
            })],
        ),
        "trigger t: 'roll' is read but no ROLL can have run before it",
    );
}

#[test]
fn a_failing_later_event_after_a_trigger_roll_rejects_the_whole_step() {
    // the first trigger rolls (counters consumed ON THE DRAFT); the second
    // trigger's sides < 2 then throws mid-events — the whole step is rejected:
    // nothing logged (not even the action row), the real counter unmoved,
    // and the fold (which would see no records at all) agrees trivially
    let dict = dice_dict();
    let c = wait_content(
        &dict,
        vec![
            TriggerDef::Slot(SlotTriggerDef {
                id: "roller".into(),
                slot: 6,
                once: Some(true),
                out: vec!["ROLL(2, 6), NARRATE \"rolled.\"".to_string()],
            }),
            TriggerDef::Slot(SlotTriggerDef {
                id: "breaker".into(),
                slot: 6,
                once: Some(true),
                out: vec!["ROLL(1, 1)".to_string()],
            }),
        ],
    )
    .unwrap();
    let state = seed_world();
    let before = snap(&state);
    let r = settle_line(&c, &dict, state, "0:0 @ ANA WAIT", 1).unwrap();
    assert_rejected(&r, "sides must be an integer >= 2");
    assert_eq!(r.records.len(), 0);
    assert_eq!(r.state.rng, 0); // the real counter unmoved
                                // the fold sees no records at all — it agrees trivially; the input state's
                                // value rides back on a rejection (identity → snap equality)
    let replayed = fold(&[], &r.state, &dict, &c).unwrap();
    assert_eq!(replayed.rng, 0);
    assert_eq!(snap(&r.state), before);
}

#[test]
fn trigger_rolls_replay_through_saved_jsonl_rows() {
    let dict = dice_dict();
    let c = wait_content(
        &dict,
        vec![TriggerDef::Slot(SlotTriggerDef {
            id: "patrol".into(),
            slot: 6,
            once: Some(true),
            out: vec![
                "ROLL(2, 6), IF roll >= 8 -> (SPAWN OMEN) ELSE -> (NARRATE \"quiet.\")".to_string()
            ],
        })],
    )
    .unwrap();
    let state = seed_world();
    let r = settle_line(&c, &dict, state, "0:0 @ ANA WAIT", 7).unwrap();
    let path = tmp_log_path("dice-trig");
    save_log(&path, &r.records).unwrap();
    let replayed = fold(&load_log(&path).unwrap(), &seed_world(), &dict, &c).unwrap();
    assert_eq!(snap(&replayed), snap(&r.state));
    assert_eq!(replayed.rng, 2);
    let _ = std::fs::remove_file(&path);
}

// ---------------------------------------------------------------------------
// functions with dice (§4.4 x §10): ROLL and roll-tiers inside bodies
// ---------------------------------------------------------------------------

fn fn_script(extra: &str) -> String {
    let duration = if extra.contains("cross") { "1h" } else { "0m" };
    // the cross marker only sets the duration; it is stripped from the
    // function names themselves
    let tail = extra.replace(" cross", "");
    format!(
        r#"
script: Fn Dice Rig
player: ANA
start: 0:0
seed: 0

entities:
  ANA is character
  HOUSE is place

prototypes:
  OMEN is idea

triggers:
  patrol at hour 1 -> check(8)

functions:
  check(dc) -> ROLL(2, 6), IF roll >= dc -> (SPAWN OMEN, NARRATE "passed {{roll}} vs {{dc}}.") ELSE -> (NARRATE "failed {{roll}} vs {{dc}}.")
  pile(n) -> ROLL(n, 6)

verbs:
  WAIT:
    actors: 1 kind:character
    duration: {duration}
    recipes:
      default -> check(8){tail}
"#
    )
}

#[test]
fn a_body_roll_with_roll_tiers_expands_into_a_recipe_and_plays() {
    let dict = dice_dict();
    let compiled = compile_script(&fn_script(""), &dict).unwrap();
    let base = clone_state(&compiled.state);
    let r = settle_line(&compiled.content, &dict, compiled.state, "0:0 @ ANA WAIT", 1).unwrap();
    assert_eq!(r.rejected, None);
    // 2 dice at counters 0,1 = 10 >= 8 -> the pass tier, param interpolated
    assert_eq!(rolled_row(done(&r).unwrap()), Some((10, 2)));
    assert_eq!(said_record(done(&r).unwrap()), ["passed 10 vs 8."]);
    let replayed = fold(&r.records, &base, &dict, &compiled.content).unwrap();
    assert_eq!(snap(&replayed), snap(&r.state));
}

#[test]
fn a_param_as_the_roll_count_expands_from_the_call_site() {
    let dict = dice_dict();
    let compiled = compile_script(&fn_script(", pile(3)"), &dict).unwrap();
    let r = settle_line(&compiled.content, &dict, compiled.state, "0:0 @ ANA WAIT", 1).unwrap();
    assert_eq!(r.rejected, None);
    let rows = rolled_rows(done(&r).unwrap());
    assert_eq!(rows, vec![(10, 2), (8, 5)]); // second call: 3 dice at counters 2..4 = 4+1+3 = 8
}

#[test]
fn the_same_function_called_from_a_trigger_rolls_and_replays_through_the_event_row() {
    let dict = dice_dict();
    // WAIT lasts 1h, crossing hour 1 (slot 2)
    let compiled = compile_script(&fn_script(" cross"), &dict).unwrap();
    let base = clone_state(&compiled.state);
    let r = settle_line(&compiled.content, &dict, compiled.state, "0:0 @ ANA WAIT", 1).unwrap();
    assert_eq!(r.rejected, None);
    // the recipe's own check(8) consumed counters 0,1 (10, passed); the event's
    // call continues at counters 2,3 = 4+1 = 5 < 8 -> the fail tier
    let event = r.records.iter().find(|x| x.kind == "event").unwrap();
    assert_eq!(event.trigger.as_deref(), Some("patrol"));
    assert_eq!(rolled_row(event), Some((5, 4)));
    assert_eq!(said_record(event), ["failed 5 vs 8."]);
    let replayed = fold(&r.records, &base, &dict, &compiled.content).unwrap();
    assert_eq!(snap(&replayed), snap(&r.state));
}

// ---------------------------------------------------------------------------
// seed drift and hygiene (§9.2, §4.4)
// ---------------------------------------------------------------------------

#[test]
fn a_different_seed_shifts_future_dice_but_never_past_rows_fold_is_row_driven() {
    let dict = dice_dict();
    let row = |seed: i64| -> (i64, i64) {
        let g = compile_script(&dice_script(seed), &dict).unwrap();
        let r = settle_line(&g.content, &dict, g.state, "0:0 @ ANA WAIT", 1).unwrap();
        rolled_row(done(&r).unwrap()).unwrap()
    };
    assert_ne!(row(0), row(42));
    // the recorded row is what the fold replays: hand-feeding the SAME row to
    // differently-seeded content lands the identical counter
    let c0 = compile_script(&dice_script(0), &dict).unwrap().content;
    let c42 = compile_script(&dice_script(42), &dict).unwrap().content;
    let record = fixture_complete(1, vec![rolled_outcome(14, 3)]);
    assert_eq!(fold(std::slice::from_ref(&record), &new_state(0), &dict, &c0).unwrap().rng, 3);
    assert_eq!(fold(std::slice::from_ref(&record), &new_state(0), &dict, &c42).unwrap().rng, 3);
}

/// Two fresh same-seed runs write byte-identical
/// JSONL logs; a different seed diverges. The comparison is over the SAVED
/// BYTES of store::save_log (read back from disk), compared as raw bytes.
#[test]
fn two_fresh_same_seed_runs_write_byte_identical_jsonl_logs_a_different_seed_diverges() {
    fn play(seed: i64) -> String {
        let mut game = Game::from_script(&dice_script(seed), dice_dict()).unwrap();
        assert_eq!(game.handle_line("wait").unwrap().kind, TurnKind::Action);
        game.wait().unwrap(); // v0.5: the roll lands on the completion — the row must be in the log
        let path = tmp_log_path(&format!("dice-det-{seed}"));
        save_log(&path, &game.log.records).unwrap();
        let bytes = std::fs::read(&path).unwrap();
        let _ = std::fs::remove_file(&path);
        String::from_utf8(bytes).unwrap()
    }
    assert_eq!(play(7), play(7));
    assert_ne!(play(7), play(8));
}
