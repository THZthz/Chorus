//! `RUN` — recipe chaining (spec §9.47): the a→b hand-off with fold/JSONL
//! equality and continue-playing, the `when:`-disambiguated dream router, the
//! silent chain end, the ROLL+IF chance fork, the pinned recipe's top-level
//! REFUSE refusing the PARENT window, the second-RUN refusal, the §9.50
//! max-exhausted candidate skipping, the 1024-window livelock budget, the
//! load checks (same-verb candidates, trigger homes, the quoted-id arg form,
//! IF branches included), the deep chain resolving across passes with
//! `State.executed` counting every begin, and the follow-up clause's
//! PROJECTION onto the candidate verb's declared roles (§9.47: cross-verb
//! follow-ups carry only the roles their verb declares — the dropped cards
//! never lock; the E7 slice shape: the bootstrap with-card stays free through
//! the self-rechaining wheel and the dream restore lands clean).
//!
//! Conventions follow tests/decks.rs + tests/max_executions.rs: a fresh
//! dictionary per test, error patterns copied CHARACTER FOR CHARACTER from
//! the frozen literals (regex-escaped), and play/fold equality asserted
//! through the shared `snap`.

mod common;

use common::harness::assert_err_msg;
use common::settle::{done, refusal, settle_step};
use common::snap::snap;

use chorus::content::content::{compile_content, CompiledContent, ContentRaw, RecipeDef, SlotDef, VerbContent};
use chorus::content::script::compile_script;
use chorus::core::clock::{parse_clock, Clock};
use chorus::core::dice::roll_die;
use chorus::core::dict::{core_dictionary, Dictionary};
use chorus::core::format::format_sentence;
use chorus::core::parse::{parse_outcomes, parse_sentence};
use chorus::core::sentence::Arg;
use chorus::core::types::{clone_state, AttrValue, State};
use chorus::engine::engine::{execute_step, ExecOptions, ExecResult, Step};
use chorus::engine::fold::fold;
use chorus::engine::store::{load_log, save_log};

// ---------------------------------------------------------------- rigs ------

/// DREAM and WAIT are core verbs — the default dictionary carries them; the
/// per-test fresh dictionary is the decks.rs discipline anyway.
fn run_dict() -> Dictionary {
    core_dictionary()
}

/// The rig: ANA at HOUSE and the caller's verbs block (plus optional
/// triggers/entities for the load-error cases).
fn rig(verbs: &str, triggers: Option<&str>) -> String {
    rig_full(verbs, triggers, None)
}

/// The full rig: extra entity lines (the multi-role pins bind an object and
/// an instrument), trigger lines, and the verbs block.
fn rig_full(verbs: &str, triggers: Option<&str>, entities: Option<&str>) -> String {
    let triggers = match triggers {
        Some(t) => format!("\ntriggers:\n{t}"),
        None => String::new(),
    };
    let entities = match entities {
        Some(e) => format!("{e}\n"),
        None => String::new(),
    };
    format!(
        "
script: Run Chains Rig
player: ANA
start: 0:0
seed: 0

entities:
  ANA is character at HOUSE
  HOUSE is place
{entities}{triggers}

prototypes:
  TRINKET is idea stackable

verbs:
{verbs}
"
    )
}

/// The zero-duration one-verb body every play test starts from (recipes
/// appended per test).
fn dream(recipes: &str) -> String {
    format!(
        "  DREAM:\n    actors: 1 kind:character\n    duration: 0m\n    recipes:\n{recipes}"
    )
}

fn compile(dict: &Dictionary, text: &str) -> (CompiledContent, State) {
    let compiled = compile_script(text, dict).unwrap();
    (compiled.content, compiled.state)
}

/// Parse + settle_step with the decks.rs
/// id-generator shape: `x{first_record_id}_{n}`.
fn run(
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
        format!("x{first_record_id}_{n}")
    };
    settle_step(
        content,
        dict,
        state,
        Step {
            start_min: None,
            clock: Some(s.clock.unwrap_or(Clock { day: 0, halfhour: 0 })),
            clauses: s.clauses,
            t_real: None,
        },
        first_record_id,
        Some(&mut id_gen),
    )
    .unwrap()
}

/// A pure advance (the bare-clock step) with the same id-generator shape.
fn advance(
    content: &CompiledContent,
    dict: &Dictionary,
    state: State,
    clock: &str,
    first_record_id: i64,
) -> ExecResult {
    let mut n: i64 = 0;
    let mut id_gen = || {
        n += 1;
        format!("x{first_record_id}_{n}")
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
        ExecOptions {
            first_record_id,
            id_gen: Some(&mut id_gen),
        },
    )
    .unwrap()
}

/// Every action row's commit-time recipe ids, record order — the BEGIN
/// spine of a turn: [parent…, follow-up…].
fn begin_recipes(r: &ExecResult) -> Vec<Vec<String>> {
    r.records
        .iter()
        .filter(|rec| rec.kind == "action")
        .map(|rec| rec.recipes.clone())
        .collect()
}

/// The execution map as entry pairs (insertion order observable).
fn executed_entries(s: &State) -> Vec<(String, i64)> {
    s.executed.iter().map(|(k, v)| (k.clone(), *v)).collect()
}

/// The kind spine of a settled turn.
fn kind_spine(r: &ExecResult) -> Vec<&str> {
    r.records.iter().map(|rec| rec.kind.as_str()).collect()
}

// ------------------------------------------------- parse/format/load walks --

/// The §5.2 round-trip: the paren-less authored form parses, the canonical
/// paren'd shell renders the ids RE-QUOTED (a bare dotted id would re-parse
/// as a SET key), and the rendering re-parses to the identical outcome. RUN
/// also composes in an outcome list: the comma after its last candidate
/// belongs to the list when a non-string outcome follows.
#[test]
fn run_parses_round_trips_and_composes_in_a_list() {
    let dict = run_dict();
    let s = parse_sentence("0:0 @ ANA DREAM -> NARRATE \"a\", RUN \"study.recipe0\", \"study.recipe1\", NARRATE \"z\"", &dict)
        .unwrap();
    let [narr_a, run, narr_z] = s.outcomes.as_slice() else {
        panic!("three outcomes");
    };
    assert_eq!(narr_a.op, "NARRATE");
    assert_eq!(run.op, "RUN");
    assert_eq!(run.args.len(), 2, "both quoted candidates belong to the RUN");
    assert_eq!(narr_z.op, "NARRATE");
    // the canonical shell renders re-quoted and re-parses identically
    let out = format_sentence(&s, Some(&dict)).unwrap();
    assert_eq!(
        out,
        "0:0 @ ANA DREAM -> NARRATE \"a\", RUN(\"study.recipe0\", \"study.recipe1\"), NARRATE \"z\""
    );
    let s2 = parse_sentence(&out, &dict).unwrap();
    assert_eq!(s2.outcomes, s.outcomes);
    assert_eq!(format_sentence(&s2, Some(&dict)).unwrap(), out);
    // the arg-form pins, at the parse layer (the load checks below re-prove
    // them through compile_script)
    let outs = parse_outcomes("RUN \"a.b\"", &dict, None).unwrap();
    assert_eq!(outs[0].op, "RUN");
    assert_eq!(
        parse_outcomes("RUN a.b", &dict, None).unwrap_err().msg,
        "RUN takes quoted-string recipe ids: RUN \"id\" [, \"id\" …] — got a"
    );
    assert_eq!(
        parse_outcomes("RUN", &dict, None).unwrap_err().msg,
        "RUN takes quoted-string recipe ids: RUN \"id\" [, \"id\" …] — got none"
    );
}

// --------------------------------------------------------- the chain itself --

/// Chain a→b: completing a's window begins b's window on the same verb with
/// the parent's bindings; b's PROGRESS rides its synthesized BEGIN row; the
/// whole turn folds identically — from the log directly and from JSONL —
/// and play continues from the folded state.
#[test]
fn chain_a_to_b_folds_identically_and_play_continues() {
    let dict = run_dict();
    let (c, s) = compile(
        &dict,
        &rig(
            &dream(
                "      a when count(actors) >= 1 -> NARRATE \"a done\", RUN \"b\"\n      b when count(actors) >= 1 -> PROGRESS(30m, \"halfway\"), NARRATE \"b done\"",
            ),
            None,
        ),
    );
    let base = clone_state(&s);
    let r = run(&c, &dict, s, "0:0 @ ANA DREAM", 1);
    assert_eq!(r.rejected, None);
    // the spine: begin a · complete a · begin b (the synthesized follow-up) ·
    // complete b · wait
    assert_eq!(kind_spine(&r), ["action", "complete", "action", "complete", "wait"]);
    assert_eq!(begin_recipes(&r), [vec!["a".to_string()], vec!["b".to_string()]]);
    // the follow-up begins AT THE COMPLETION CLOCK (zero-duration verb: 0:0)
    let begin_b = &r.records[2];
    assert_eq!(begin_b.start_min, Some(0));
    assert_eq!(begin_b.t_game, "0:0");
    assert_eq!(begin_b.verb, "DREAM");
    assert_eq!(begin_b.actors.len(), 1, "the parent's bindings ride along");
    // b's PROGRESS ticks ride its BEGIN row, like any player-begun window
    assert_eq!(begin_b.outcomes.len(), 1);
    assert_eq!(begin_b.outcomes[0].op, "PROGRESS");
    let done_b = &r.records[3];
    let ops: Vec<&str> = done_b.outcomes.iter().map(|o| o.op.as_str()).collect();
    assert_eq!(ops, ["NARRATE"], "PROGRESS is journal-only at completion");
    // §9.50: the count rides the ONE registration path — both begins counted
    assert_eq!(
        executed_entries(&r.state),
        vec![("a".to_string(), 1), ("b".to_string(), 1)]
    );
    assert!(r.state.windows.is_empty());
    assert_eq!(refusal(&r), None);
    // fold from the log: the play-inserted window and the fold-replayed BEGIN
    // land identical
    let replayed = fold(&r.records, &base, &dict, &c).unwrap();
    assert_eq!(snap(&replayed), snap(&r.state));
    assert_eq!(
        executed_entries(&replayed),
        vec![("a".to_string(), 1), ("b".to_string(), 1)]
    );
    // the records survive JSONL and still fold identically
    let dir = std::env::temp_dir().join(format!("chorus-run-chains-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("log.jsonl");
    save_log(&path, &r.records).unwrap();
    let loaded = load_log(&path).unwrap();
    assert_eq!(loaded, r.records);
    let refolded = fold(&loaded, &base, &dict, &c).unwrap();
    assert_eq!(snap(&refolded), snap(&r.state));
    // play continues from the FOLDED state and agrees with play from the
    // play-state (the fold is the state, §7)
    let from_fold = run(&c, &dict, replayed, "0:0 @ ANA DREAM", 100);
    let from_play = run(&c, &dict, clone_state(&r.state), "0:0 @ ANA DREAM", 200);
    assert_eq!(from_fold.rejected, None);
    assert_eq!(begin_recipes(&from_fold), begin_recipes(&from_play));
    assert_eq!(snap(&from_fold.state), snap(&from_play.state));
    assert_eq!(
        executed_entries(&from_fold.state),
        vec![("a".to_string(), 2), ("b".to_string(), 2)]
    );
}

/// The dream-router shape: one RUN with `when:`-disambiguated candidates —
/// the walk pins the FIRST match against the post-outcome draft, so the
/// match changes when the situation changes (a pinned-id-no-check rule would
/// make this inexpressible, §9.47).
#[test]
fn the_router_walk_pins_the_first_matching_candidate() {
    let dict = run_dict();
    let (c, s) = compile(
        &dict,
        &rig(
            &dream(
                "      router when count(actors) >= 1 -> SET(actors, aspect:mood, +1), RUN \"r_high\", \"r_low\", \"r_never\"\n      r_high when actors.aspect.mood >= 2 -> NARRATE \"high\"\n      r_low when actors.aspect.mood >= 1 -> NARRATE \"low\"\n      r_never when actors.aspect.mood >= 99 -> NARRATE \"never\"",
            ),
            None,
        ),
    );
    // completion 1: mood 0→1 — r_high misses, r_low matches
    let r1 = run(&c, &dict, s, "0:0 @ ANA DREAM", 1);
    assert_eq!(r1.rejected, None);
    assert_eq!(begin_recipes(&r1), [vec!["router".to_string()], vec!["r_low".to_string()]]);
    let ops: Vec<&str> = r1.records[3].outcomes.iter().map(|o| o.op.as_str()).collect();
    assert_eq!(ops, ["NARRATE"]);
    assert_eq!(r1.records[3].outcomes[0].args[0], Arg::Str { v: "low".to_string() });
    // completion 2: mood 1→2 — the SITUATION changed, so the MATCH changed
    let r2 = run(&c, &dict, r1.state, "0:0 @ ANA DREAM", 100);
    assert_eq!(r2.rejected, None);
    assert_eq!(begin_recipes(&r2), [vec!["router".to_string()], vec!["r_high".to_string()]]);
    assert_eq!(r2.records[3].outcomes[0].args[0], Arg::Str { v: "high".to_string() });
    assert_eq!(
        executed_entries(&r2.state),
        vec![
            ("router".to_string(), 2),
            ("r_low".to_string(), 1),
            ("r_high".to_string(), 1),
        ]
    );
}

/// No candidate matches: the chain ends SILENTLY — no follow-up window, no
/// refusal, no log beyond the parent's own completion.
#[test]
fn a_chain_with_no_matching_candidate_ends_silently() {
    let dict = run_dict();
    let (c, s) = compile(
        &dict,
        &rig(
            &dream(
                "      parent when count(actors) >= 1 -> NARRATE \"alone\", RUN \"nope1\", \"nope2\"\n      nope1 when actors.aspect.mood >= 50 -> NARRATE \"n1\"\n      nope2 when actors.aspect.mood >= 60 -> NARRATE \"n2\"",
            ),
            None,
        ),
    );
    let base = clone_state(&s);
    let r = run(&c, &dict, s, "0:0 @ ANA DREAM", 1);
    assert_eq!(r.rejected, None);
    assert_eq!(kind_spine(&r), ["action", "complete", "wait"], "no follow-up");
    assert_eq!(refusal(&r), None, "a dead chain is not a refusal");
    let ops: Vec<&str> = done(&r).unwrap().outcomes.iter().map(|o| o.op.as_str()).collect();
    assert_eq!(ops, ["NARRATE"], "the COMPLETE row carries no REFUSE marker");
    assert_eq!(executed_entries(&r.state), vec![("parent".to_string(), 1)]);
    assert!(r.state.windows.is_empty());
    // fold agrees
    let replayed = fold(&r.records, &base, &dict, &c).unwrap();
    assert_eq!(snap(&replayed), snap(&r.state));
}

/// The chance fork (§9.47's authoring shape): ROLL + IF with one RUN per arm
/// — identical-requirements candidates route through the IF form, since the
/// list walk would always pin the first. Exactly one arm evaluates, one RUN
/// applies, deterministically from the seed.
#[test]
fn the_chance_fork_routes_through_its_if_form() {
    let dict = run_dict();
    let (c, s) = compile(
        &dict,
        &rig(
            &dream(
                "      fork when count(actors) >= 1 -> ROLL(1, 2), IF roll <= 1 -> (RUN \"heads\") ELSE -> (RUN \"tails\")\n      heads when count(actors) >= 1 -> NARRATE \"heads\"\n      tails when count(actors) >= 1 -> NARRATE \"tails\"",
            ),
            None,
        ),
    );
    let total = roll_die(0, 0, 2);
    let expected = if total <= 1 { "heads" } else { "tails" };
    let r = run(&c, &dict, s, "0:0 @ ANA DREAM", 1);
    assert_eq!(r.rejected, None);
    assert_eq!(refusal(&r), None, "the taken arm's RUN applied cleanly");
    // the parent's COMPLETE carries the ROLLED row; the RUN itself logs nothing
    let fork_ops: Vec<&str> = r.records[1].outcomes.iter().map(|o| o.op.as_str()).collect();
    assert_eq!(fork_ops, ["ROLLED"]);
    assert_eq!(r.records[1].outcomes[0].args[0], Arg::Int { v: total });
    // exactly ONE follow-up — the taken arm's candidate
    assert_eq!(
        begin_recipes(&r),
        [vec!["fork".to_string()], vec![expected.to_string()]]
    );
    assert_eq!(
        executed_entries(&r.state),
        vec![
            ("fork".to_string(), 1),
            (expected.to_string(), 1),
        ]
    );
    // determinism: the same counters throw the same die from the same state
    let r2 = run(&c, &dict, clone_state(&r.state), "0:0 @ ANA DREAM", 100);
    assert_eq!(begin_recipes(&r2), begin_recipes(&r));
}

/// The guard survives: the pinned recipe's TOP-LEVEL REFUSE gates inline at
/// the parent's completion — the refusal rejects the PARENT window (§6.7's
/// window-refuse: the COMPLETE row carries the marker, partial effects
/// discarded, window closed, no follow-up). Fold agrees.
#[test]
fn the_pinned_refuse_gate_rejects_the_parent_window() {
    let dict = run_dict();
    let text = rig(
        &dream(
            "      parent when count(actors) >= 1 -> NARRATE \"parent\", RUN \"gated\"\n          gated when count(actors) >= 1 -> REFUSE(\"the gate says no\")",
        ),
        None,
    );
    let (c, s) = compile(&dict, &text);
    let base = clone_state(&s);
    let r = run(&c, &dict, s, "0:0 @ ANA DREAM", 1);
    assert_eq!(r.rejected, None);
    assert_eq!(refusal(&r), Some("the gate says no".to_string()));
    // the parent's COMPLETE row carries the NARRATE it managed plus the
    // REFUSE marker; there is no follow-up
    assert_eq!(kind_spine(&r), ["action", "complete", "wait"]);
    let ops: Vec<&str> = done(&r).unwrap().outcomes.iter().map(|o| o.op.as_str()).collect();
    assert_eq!(ops, ["NARRATE", "REFUSE"]);
    // the parent counted at begin; the gated follow-up never registered
    assert_eq!(executed_entries(&r.state), vec![("parent".to_string(), 1)]);
    assert!(r.state.windows.is_empty(), "the parent window still closes");
    // fold agrees (a refused completion is a fold no-op; no follow-up BEGIN)
    let replayed = fold(&r.records, &base, &dict, &c).unwrap();
    assert_eq!(snap(&replayed), snap(&r.state));
}

/// A SECOND RUN op reached in one completion — two bare RUNs — refuses the
/// parent window at that outcome: one chain slot per completion (a station
/// holds one window, §6.7); the first RUN's follow-up never opens.
#[test]
fn a_second_run_op_refuses_the_parent_window() {
    let dict = run_dict();
    let (c, s) = compile(
        &dict,
        &rig(
            &dream(
                "      greedy when count(actors) >= 1 -> RUN \"x\", RUN \"y\"\n      x when count(actors) >= 1 -> NARRATE \"x\"\n      y when count(actors) >= 1 -> NARRATE \"y\"",
            ),
            None,
        ),
    );
    let base = clone_state(&s);
    let r = run(&c, &dict, s, "0:0 @ ANA DREAM", 1);
    assert_eq!(r.rejected, None);
    assert_eq!(
        refusal(&r),
        Some("a second RUN in one completion — a station holds one window (§6.7, §9.47)".to_string())
    );
    assert_eq!(kind_spine(&r), ["action", "complete", "wait"], "no follow-up opened");
    let ops: Vec<&str> = done(&r).unwrap().outcomes.iter().map(|o| o.op.as_str()).collect();
    assert_eq!(ops, ["REFUSE"]);
    assert_eq!(executed_entries(&r.state), vec![("greedy".to_string(), 1)]);
    assert!(r.state.windows.is_empty());
    let replayed = fold(&r.records, &base, &dict, &c).unwrap();
    assert_eq!(snap(&replayed), snap(&r.state));
}

/// The one-chain-slot rule counts REACHED ops, not pinned ones: a silent
/// no-match FIRST RUN still spends the completion's chain slot, so the
/// SECOND RUN op refuses the parent window (§9.47).
#[test]
fn a_second_run_refuses_even_when_the_first_pinned_nothing() {
    let dict = run_dict();
    let (c, s) = compile(
        &dict,
        &rig(
            &dream(
                "      parent when count(actors) >= 1 -> RUN \"never\", RUN \"x\"\n      never when actors.aspect.mood >= 99 -> NARRATE \"never\"\n      x when count(actors) >= 1 -> NARRATE \"x\"",
            ),
            None,
        ),
    );
    let base = clone_state(&s);
    let r = run(&c, &dict, s, "0:0 @ ANA DREAM", 1);
    assert_eq!(r.rejected, None);
    assert_eq!(
        refusal(&r),
        Some("a second RUN in one completion — a station holds one window (§6.7, §9.47)".to_string())
    );
    assert_eq!(kind_spine(&r), ["action", "complete", "wait"], "no follow-up opened");
    assert_eq!(executed_entries(&r.state), vec![("parent".to_string(), 1)]);
    assert!(r.state.windows.is_empty());
    let replayed = fold(&r.records, &base, &dict, &c).unwrap();
    assert_eq!(snap(&replayed), snap(&r.state));
}

/// The multi-role pin — the riskiest fold-symmetry shape: object AND with
/// AND at all bound, the follow-up BEGIN carries them all, and the
/// play-inserted follow-up window and the fold-replayed one land identical
/// `OpenWindow`s (structural equality, not just snap).
#[test]
fn the_followup_window_lands_identical_with_multi_role_refs() {
    let dict = run_dict();
    let (c, s) = compile(
        &dict,
        &rig_full(
            "  DREAM:\n    actors: 1 kind:character\n    object: 1 kind:text\n    with: 1 kind:item\n    at: 1 kind:place\n    duration: 1h\n    recipes:\n      parent when count(actors) >= 1 -> NARRATE \"parent\", RUN \"child\"\n      child when count(actors) >= 1 AND count(object) >= 1 -> NARRATE \"child\"",
            None,
            Some("  MAGICK_BOOK is text at HOUSE\n  CHARM is item at HOUSE"),
        ),
    );
    let base = clone_state(&s);
    let r = run(&c, &dict, s, "0:0 @ ANA AT HOUSE DREAM MAGICK_BOOK WITH CHARM", 1);
    assert_eq!(r.rejected, None);
    assert_eq!(begin_recipes(&r), [vec!["parent".to_string()], vec!["child".to_string()]]);
    // the follow-up BEGIN carries every parent role, authored-form
    let begin_child = &r.records[2];
    assert_eq!(begin_child.objects.len(), 1, "object rides");
    assert_eq!(begin_child.instruments.len(), 1, "with rides");
    assert!(begin_child.at.is_some(), "at rides");
    assert_eq!(begin_child.t_game, "0:2", "beginning at the completion clock");
    // the child is STILL OPEN at turn's end (1h duration) — compare the
    // open windows STRUCTURALLY: play-inserted vs fold-replayed
    assert_eq!(r.state.windows.len(), 1);
    let w = r.state.windows.values().next().unwrap();
    assert_eq!(w.recipe, "child");
    assert_eq!(w.refs.get(&chorus::core::roles::Role::Object).map(Vec::len), Some(1));
    assert_eq!(w.refs.get(&chorus::core::roles::Role::With).map(Vec::len), Some(1));
    assert_eq!(w.refs.get(&chorus::core::roles::Role::At).map(Vec::len), Some(1));
    assert_eq!(w.locked.len(), 3, "object, with and at are committed");
    let replayed = fold(&r.records, &base, &dict, &c).unwrap();
    assert_eq!(replayed.windows, r.state.windows, "identical OpenWindows");
    assert_eq!(snap(&replayed), snap(&r.state));
    // …and the follow-up's own completion re-resolves every role: the child
    // narrates from its bound object
    let r2 = advance(&c, &dict, r.state, "0:4", 100);
    assert_eq!(r2.rejected, None);
    let ops: Vec<&str> = r2.records[0].outcomes.iter().map(|o| o.op.as_str()).collect();
    assert_eq!(ops, ["NARRATE"]);
}

/// …and the inverted shape: a bound card that DIES mid-parent-window (events
/// do not respect locks — a slot trigger destroys the with-card at minute 60
/// of a 2h window). The follow-up's refs keep the authored name while its
/// lock list drops the dead id, and play vs fold still land identical
/// `OpenWindow`s.
#[test]
fn a_ref_that_dies_mid_window_still_folds_identically() {
    let dict = run_dict();
    let (c, s) = compile(
        &dict,
        &rig_full(
            "  DREAM:\n    actors: 1 kind:character\n    object: 1 kind:text\n    with: 1 kind:item\n    duration: 2h\n    recipes:\n      parent when count(actors) >= 1 -> NARRATE \"parent\", RUN \"child\"\n      child when count(actors) >= 1 -> NARRATE \"child\"",
            Some("  curse at hour 1 -> DESTROY CHARM"),
            Some("  MAGICK_BOOK is text at HOUSE\n  CHARM is item at HOUSE"),
        ),
    );
    let base = clone_state(&s);
    let r = run(&c, &dict, s, "0:0 @ ANA DREAM MAGICK_BOOK WITH CHARM", 1);
    assert_eq!(r.rejected, None);
    // the spine: begin · EVENT (the mid-window destroy) · complete · the
    // follow-up BEGIN · wait
    assert_eq!(kind_spine(&r), ["action", "event", "complete", "action", "wait"]);
    assert_eq!(begin_recipes(&r), [vec!["parent".to_string()], vec!["child".to_string()]]);
    // the child window is open: refs keep the AUTHORED with-ref, the lock
    // list dropped the dead id
    assert_eq!(r.state.windows.len(), 1);
    let w = r.state.windows.values().next().unwrap();
    assert_eq!(w.recipe, "child");
    assert_eq!(w.refs.get(&chorus::core::roles::Role::With).map(Vec::len), Some(1), "the authored ref rides");
    assert_eq!(w.locked.len(), 1, "the dead card is not locked");
    assert!(!r.state.entities.values().any(|e| e.name == "CHARM" && e.dead_action.is_none()));
    // play-inserted vs fold-replayed: identical OpenWindows
    let replayed = fold(&r.records, &base, &dict, &c).unwrap();
    assert_eq!(replayed.windows, r.state.windows, "identical OpenWindows");
    assert_eq!(snap(&replayed), snap(&r.state));
}

/// Cross-verb projection (§9.47, the E7 fix): a parent holding object/with/at
/// chains — through the with-card's `on complete` trigger, the one legal
/// cross-verb home (§9.48) — into a candidate whose verb declares ACTORS
/// ONLY. The follow-up clause inherits the actor role alone: a player step
/// cannot bind an undeclared role, so the wholesale shape was never
/// player-producible. The dropped cards are NOT locked (the same step
/// re-binds them while the follow-up window is open), the surviving actor
/// binding re-resolves at the follow-up's own completion, and the
/// play-inserted and fold-replayed windows land identical. A same-verb
/// candidate declares every role the parent validated against — the
/// multi-role pin above proves the projection is the identity there.
#[test]
fn a_cross_verb_followup_carries_only_the_roles_the_candidate_declares() {
    let dict = run_dict();
    let (c, s) = compile(
        &dict,
        &rig_full(
            "  DREAM:\n    actors: 1 kind:character\n    object: 1 kind:text\n    with: 1 kind:item\n    at: 1 kind:place\n    duration: 0m\n    recipes:\n      parent aspects hand when count(actors) >= 1 -> NARRATE \"parent\"\n  WAIT:\n    actors: 1 kind:character\n    duration: 1h\n    recipes:\n      rest when count(actors) >= 1 -> SET(ACTORS, silver, +1), NARRATE \"waited\"",
            None,
            Some(
                "  MAGICK_BOOK is text at HOUSE\n  CHARM is item at HOUSE on complete hand -> (RUN \"rest\")",
            ),
        ),
    );
    let base = clone_state(&s);
    let r = run(
        &c,
        &dict,
        s,
        "0:0 @ ANA AT HOUSE DREAM MAGICK_BOOK WITH CHARM",
        1,
    );
    assert_eq!(r.rejected, None);
    assert_eq!(
        begin_recipes(&r),
        [vec!["parent".to_string()], vec!["rest".to_string()]]
    );
    // the follow-up BEGIN carries ONLY the actor role — object/with/at are
    // projected away (the candidate verb declares actors only)
    let begin_rest = &r.records[2];
    assert_eq!(begin_rest.verb, "WAIT");
    assert_eq!(begin_rest.actors.len(), 1, "the actor role survives");
    assert!(begin_rest.objects.is_empty(), "object dropped");
    assert!(begin_rest.instruments.is_empty(), "with dropped");
    assert!(begin_rest.at.is_none(), "at dropped");
    // the open window holds the actor ref and locks NOTHING (actors are
    // never committed) — the dropped cards are not locked
    assert_eq!(r.state.windows.len(), 1);
    let w = r.state.windows.values().next().unwrap();
    assert_eq!(w.recipe, "rest");
    assert_eq!(
        w.refs.get(&chorus::core::roles::Role::Actors).map(Vec::len),
        Some(1)
    );
    for role in [
        chorus::core::roles::Role::Object,
        chorus::core::roles::Role::With,
        chorus::core::roles::Role::At,
    ] {
        assert_eq!(
            w.refs.get(&role).map(Vec::len),
            Some(0),
            "{role:?} dropped from the window refs"
        );
    }
    assert!(w.locked.is_empty(), "the dropped cards are not locked");
    // the busy probe: the same step re-binds every dropped card while the
    // follow-up window is open (wholesale inheritance refused here: the
    // with-card was busy with WAIT)
    let probe = run(
        &c,
        &dict,
        clone_state(&r.state),
        "0:0 @ ANA AT HOUSE DREAM MAGICK_BOOK WITH CHARM",
        100,
    );
    assert_eq!(probe.rejected, None, "the dropped cards are free");
    assert_eq!(refusal(&probe), None);
    assert_eq!(begin_recipes(&probe), [vec!["parent".to_string()]]);
    // fold: identical windows and state
    let replayed = fold(&r.records, &base, &dict, &c).unwrap();
    assert_eq!(replayed.windows, r.state.windows, "identical OpenWindows");
    assert_eq!(snap(&replayed), snap(&r.state));
    // …and the surviving actor binding re-resolves at the follow-up's own
    // completion (the time.turn-reads-actors shape)
    let r2 = advance(&c, &dict, r.state, "1:0", 200);
    assert_eq!(r2.rejected, None);
    let ops: Vec<&str> = r2.records[0]
        .outcomes
        .iter()
        .map(|o| o.op.as_str())
        .collect();
    assert_eq!(ops, ["SET", "NARRATE"], "the actor role rode the window");
    let ana = r2.state.by_name["ANA"][0].clone();
    assert_eq!(
        r2.state.entities[ana.as_str()].attrs.get("silver"),
        Some(&AttrValue::Num(1))
    );
}

/// The E7 regression, the slice shape in miniature (§9.47 projection;
/// docs/plans/v0.6-engine-findings.md E7): a work bootstrap whose with-card
/// carries the `on complete job` induce (§9.48) chains a self-rechaining
/// WAIT wheel. Pre-projection every rechained window inherited
/// `with=VIGOR`: the bootstrap card was busy until the chain died, and a
/// restored mint re-locked by name. Post-projection the wheel's windows
/// carry the actor role only: VIGOR is never busy, the wheel drains it, the
/// dream restore mints it back without a §9.19 rejection, and the next
/// rechain does not re-lock it.
#[test]
fn the_slice_shaped_bootstrap_chain_never_locks_its_with_card() {
    let dict = run_dict();
    let text = r#"
script: Run Chains Rig
player: ANA
start: 0:0
seed: 0

entities:
  ANA is character at HOUSE
  HOUSE is place
  VIGOR is resource with vigor:1 at ANA on complete job -> (IF NOT exists(name:THE_TURNING) -> (SPAWN THE_TURNING, NARRATE "the wheel begins"), RUN "turn")

prototypes:
  THE_TURNING is state
  THE_WEARY_HOUR is resource with weary:1
  THE_HOLLOW_DAYS is state stackable

verbs:
  WORK:
    actors: 1 kind:character
    with: 0..1 kind:resource
    duration: 0m
    recipes:
      work.job aspects job when total(with, aspect:vigor) >= 1 -> NARRATE "worked"
  WAIT:
    actors: 1 kind:character
    duration: 1h
    recipes:
      turn when exists(name:THE_TURNING) -> IF count(name:THE_HOLLOW_DAYS) < 1 -> (DESTROY VIGOR, SPAWN THE_WEARY_HOUR, SPAWN THE_HOLLOW_DAYS, NARRATE "eaten") ELSE -> (NARRATE "fasting"), RUN "turn"
  DREAM:
    actors: 1 kind:character
    with: 0..2 kind:resource
    duration: 0m
    recipes:
      dream.rest_vigor when count(with) = 1 AND total(with, aspect:weary) >= 1 -> DESTROY WITH, SPAWN resource:VIGOR, SET(VIGOR, aspect:vigor, 1), NARRATE "restored"
      dream.default -> NARRATE "sleep"
"#;
    let (c, s) = compile(&dict, text);
    let base = clone_state(&s);
    // the bootstrap: work with the vigor — its completion spawns the wheel
    // and chains time.turn's analog cross-verb (§9.48)
    let r1 = run(&c, &dict, s, "0:0 @ ANA WORK WITH VIGOR", 1);
    assert_eq!(r1.rejected, None);
    assert_eq!(
        begin_recipes(&r1),
        [vec!["work.job".to_string()], vec!["turn".to_string()]]
    );
    // the wheel's BEGIN carries the actor role only — the with-card is
    // projected away at synthesis
    let begin_turn = &r1.records[2];
    assert_eq!(begin_turn.verb, "WAIT");
    assert_eq!(begin_turn.actors.len(), 1);
    assert!(begin_turn.instruments.is_empty(), "with dropped");
    assert_eq!(r1.state.windows.len(), 1);
    let w1 = r1.state.windows.values().next().unwrap();
    assert_eq!(w1.recipe, "turn");
    assert!(w1.locked.is_empty(), "VIGOR is not locked by the wheel");
    // VIGOR is never busy: the same work re-binds it while the wheel window
    // is open (pre-projection: "VIGOR is busy with WAIT — it frees at 1:0")
    let probe1 = run(&c, &dict, r1.state, "0:0 @ ANA WORK WITH VIGOR", 100);
    assert_eq!(probe1.rejected, None, "VIGOR is free while the wheel runs");
    assert_eq!(refusal(&probe1), None);
    // the wheel drains the body at 1:0 and rechains
    let r2 = advance(&c, &dict, probe1.state, "1:0", 200);
    assert_eq!(r2.rejected, None);
    assert!(
        !r2.state
            .entities
            .values()
            .any(|e| e.name == "VIGOR" && e.dead_action.is_none()),
        "the wheel ate VIGOR"
    );
    assert!(r2
        .state
        .entities
        .values()
        .any(|e| e.name == "THE_WEARY_HOUR" && e.dead_action.is_none()));
    assert_eq!(r2.state.windows.len(), 1, "the wheel rechained");
    assert!(r2.state.windows.values().next().unwrap().locked.is_empty());
    // the restore: dream.rest_vigor mints VIGOR back — no §9.19 rejection
    // (a restored mint used to re-lock by name on the next rechain)
    let r3 = run(
        &c,
        &dict,
        r2.state,
        "1:0 @ ANA DREAM WITH THE_WEARY_HOUR",
        300,
    );
    assert_eq!(r3.rejected, None);
    assert_eq!(refusal(&r3), None, "the restore lands clean");
    assert!(
        r3.state
            .entities
            .values()
            .any(|e| e.name == "VIGOR" && e.dead_action.is_none()),
        "VIGOR is restored"
    );
    // the next rechain must not re-lock the restored mint: the wheel runs on
    // and VIGOR binds again immediately
    let r4 = advance(&c, &dict, r3.state, "2:0", 400);
    assert_eq!(r4.rejected, None);
    assert_eq!(r4.state.windows.len(), 1, "still rechaining");
    assert!(
        r4.state.windows.values().next().unwrap().locked.is_empty(),
        "the restored VIGOR is not re-locked by name"
    );
    let probe2 = run(&c, &dict, r4.state, "2:0 @ ANA WORK WITH VIGOR", 500);
    assert_eq!(probe2.rejected, None, "the restored VIGOR is never busy");
    assert_eq!(refusal(&probe2), None);
    // the whole walk folds identically, windows included
    let mut records = r1.records.clone();
    records.extend(probe1.records.iter().cloned());
    records.extend(r2.records.iter().cloned());
    records.extend(r3.records.iter().cloned());
    records.extend(r4.records.iter().cloned());
    records.extend(probe2.records.iter().cloned());
    let replayed = fold(&records, &base, &dict, &c).unwrap();
    assert_eq!(
        replayed.windows, probe2.state.windows,
        "identical OpenWindows"
    );
    assert_eq!(snap(&replayed), snap(&probe2.state));
}

/// The guard trial's scratch never leaks: a gated candidate whose body ROLLs
/// and SPAWNs before its top-level REFUSE consumes the SCRATCH's rng counters
/// and mints into the SCRATCH only — the real draft's counter stays pinned
/// and no entity lands (§9.47's Phase-3-shaped trial).
#[test]
fn the_guard_trials_rng_and_id_consumption_stay_scratch_only() {
    let dict = run_dict();
    let (c, s) = compile(
        &dict,
        &rig(
            &dream(
                "      parent when count(actors) >= 1 -> NARRATE \"parent\", RUN \"gated\"\n      gated when count(actors) >= 1 -> ROLL(2, 6), SPAWN TRINKET, REFUSE(\"the gate says no\")",
            ),
            None,
        ),
    );
    let base = clone_state(&s);
    assert_eq!(s.rng, 0);
    let r = run(&c, &dict, s, "0:0 @ ANA DREAM", 1);
    assert_eq!(r.rejected, None);
    assert_eq!(refusal(&r), Some("the gate says no".to_string()));
    // the parent window refused: COMPLETE carries the marker, no follow-up
    let ops: Vec<&str> = done(&r).unwrap().outcomes.iter().map(|o| o.op.as_str()).collect();
    assert_eq!(ops, ["NARRATE", "REFUSE"]);
    assert_eq!(executed_entries(&r.state), vec![("parent".to_string(), 1)]);
    assert!(r.state.windows.is_empty());
    // the trial's dice and mint died with the scratch: the counter is
    // UNMOVED and no TRINKET exists (no ids minted into the real state)
    assert_eq!(r.state.rng, 0, "the trial's ROLL never touched the real counter");
    assert!(!r.state.entities.values().any(|e| e.name == "TRINKET"));
    // fold agrees
    let replayed = fold(&r.records, &base, &dict, &c).unwrap();
    assert_eq!(snap(&replayed), snap(&r.state));
    assert_eq!(replayed.rng, 0);
}

/// A max-exhausted candidate is simply non-matching (§9.50): the walk skips
/// it to the next candidate — and to NONE when nothing is left.
#[test]
fn a_max_exhausted_candidate_is_skipped_by_the_walk() {
    let dict = run_dict();
    let (c, s) = compile(
        &dict,
        &rig(
            &dream(
                "      parent when count(actors) >= 1 -> RUN \"spent\", \"fresh\"\n      spent max 1 when count(actors) >= 1 -> NARRATE \"spent\"\n      fresh when count(actors) >= 1 -> NARRATE \"fresh\"",
            ),
            None,
        ),
    );
    // play 1: `spent` is unspent — the walk pins it
    let r1 = run(&c, &dict, s, "0:0 @ ANA DREAM", 1);
    assert_eq!(r1.rejected, None);
    assert_eq!(begin_recipes(&r1), [vec!["parent".to_string()], vec!["spent".to_string()]]);
    // play 2: `spent` is exhausted — the walk continues into `fresh`
    let r2 = run(&c, &dict, r1.state, "0:0 @ ANA DREAM", 100);
    assert_eq!(r2.rejected, None);
    assert_eq!(begin_recipes(&r2), [vec!["parent".to_string()], vec!["fresh".to_string()]]);
    assert_eq!(
        executed_entries(&r2.state),
        vec![
            ("parent".to_string(), 2),
            ("spent".to_string(), 1),
            ("fresh".to_string(), 1),
        ]
    );
    // …and a walk whose ONLY candidate is exhausted pins nothing at all
    let (c3, s3) = compile(
        &dict,
        &rig(
            &dream(
                "      parent when count(actors) >= 1 -> RUN \"once\"\n      once max 1 when count(actors) >= 1 -> NARRATE \"once\"",
            ),
            None,
        ),
    );
    let p1 = run(&c3, &dict, s3, "0:0 @ ANA DREAM", 300);
    assert_eq!(p1.rejected, None);
    let p2 = run(&c3, &dict, p1.state, "0:0 @ ANA DREAM", 400);
    assert_eq!(p2.rejected, None);
    assert_eq!(kind_spine(&p2), ["action", "complete", "wait"]);
    assert_eq!(refusal(&p2), None);
    assert_eq!(executed_entries(&p2.state), vec![("parent".to_string(), 2), ("once".to_string(), 1)]);
}

/// The livelock guard (§9.47/§9.44): two zero-duration recipes RUN-ing each
/// other generate unbounded due work inside one pass — the pass fails LOUD
/// at the 1024 budget, and the throw rejects the whole step (nothing logged,
/// the original state rides back untouched).
#[test]
fn a_zero_duration_run_cycle_trips_the_livelock_budget() {
    let dict = run_dict();
    let (c, s) = compile(
        &dict,
        &rig(
            &dream(
                "      ping when count(actors) >= 1 -> NARRATE \"ping\", RUN \"pong\"\n      pong when count(actors) >= 1 -> NARRATE \"pong\", RUN \"ping\"",
            ),
            None,
        ),
    );
    let original = clone_state(&s);
    let r = run(&c, &dict, s, "0:0 @ ANA DREAM", 1);
    let rejected = r.rejected.clone().unwrap_or_else(|| panic!("expected the livelock rejection, got {:?}", r.rejected));
    assert!(
        rejected.contains("livelock guard")
            && rejected.contains("1024")
            && rejected.contains("§9.47"),
        "{rejected}"
    );
    // the settle advance rolled back whole: nothing half-logged
    assert!(r.records.is_empty(), "the rejected pass logs nothing");
    assert_eq!(snap(&r.state), snap(&original), "the original state rides back");
}

/// The load checks (§9.47): candidates must name recipes of the PARENT verb
/// (unknown and cross-verb ids are load errors), IF branches included; RUN
/// is a load error inside slot and expire triggers (events have no window);
/// the bare/dotted id and the empty list are parse errors.
#[test]
fn the_load_checks_pin_homes_candidates_and_arg_form() {
    let dict = run_dict();
    let dream_verb = |recipes: &str| {
        rig(
            &format!(
                "  DREAM:\n    actors: 1 kind:character\n    duration: 0m\n    recipes:\n{recipes}\n  WAIT:\n    actors: 1 kind:character\n    duration: 0m\n    recipes:\n      default -> NARRATE \"w\""
            ),
            None,
        )
    };
    // unknown id, top level
    assert_err_msg(
        compile_script(&dream_verb("      a when count(actors) >= 1 -> RUN \"nope\""), &dict),
        "recipe a: RUN 'nope' does not name a recipe of verb DREAM — unknown and cross-verb ids are load errors \\(§9\\.47\\)",
    );
    // cross-verb id: wait.default belongs to WAIT, not DREAM
    assert_err_msg(
        compile_script(&dream_verb("      a when count(actors) >= 1 -> RUN \"wait.default\""), &dict),
        "recipe a: RUN 'wait.default' does not name a recipe of verb DREAM — unknown and cross-verb ids are load errors \\(§9\\.47\\)",
    );
    // IF branches included — an untaken branch hides nothing at load
    assert_err_msg(
        compile_script(
            &dream_verb("      a when count(actors) >= 1 -> IF count(actors) >= 1 -> (RUN \"nope\")"),
            &dict,
        ),
        "recipe a: RUN 'nope' does not name a recipe of verb DREAM",
    );
    // a legal chain loads (the same-verb candidate resolves)
    compile(
        &dict,
        &dream_verb("      a when count(actors) >= 1 -> RUN \"b\"\n      b when count(actors) >= 1 -> NARRATE \"b\""),
    );
    // slot trigger: events have no window to chain from
    assert_err_msg(
        compile_script(
            &rig(&dream("      a when count(actors) >= 1 -> NARRATE \"a\""), Some("  tick at hour 6 -> RUN \"a\"")),
            &dict,
        ),
        "trigger tick: RUN belongs to verb recipes — events have no window to chain from",
    );
    // expire trigger: the same home rule
    assert_err_msg(
        compile_script(
            &rig(
                &dream("      a when count(actors) >= 1 -> NARRATE \"a\""),
                Some("  boom on expire of THING -> RUN \"a\""),
            ),
            &dict,
        ),
        "trigger boom: RUN belongs to verb recipes — events have no window to chain from",
    );
    // the bare (dotted) id is unlexable — the parse error names the syntax
    assert_err_msg(
        compile_script(&dream_verb("      a when count(actors) >= 1 -> RUN a"), &dict),
        "RUN takes quoted-string recipe ids: RUN \"id\" \\[, \"id\" …\\] — got a",
    );
    // the empty list is a parse error too
    assert_err_msg(
        compile_script(&dream_verb("      a when count(actors) >= 1 -> RUN"), &dict),
        "RUN takes quoted-string recipe ids: RUN \"id\" \\[, \"id\" …\\] — got none",
    );
    // the hand-built content face fails just as loud (the slot max<min precedent)
    let raw = ContentRaw {
        verbs: [(
            "DREAM".to_string(),
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
                    id: Some("a".into()),
                    when: Some("count(actors) >= 1".into()),
                    shown: None,
                    hint: None,
                    max: None,
                    aspects: Vec::new(),
                    out: vec!["RUN \"ghost\"".to_string()],
                }],
            },
        )]
        .into_iter()
        .collect(),
        ..ContentRaw::default()
    };
    assert_err_msg(
        compile_content(&raw, &dict, &[]),
        "recipe a: RUN 'ghost' does not name a recipe of verb DREAM",
    );
}

/// The deep chain a→b→c — RUN inside the follow-up — resolves ACROSS passes
/// (each window opens at its parent's completion clock), and §9.50 counts
/// every begin on both paths.
#[test]
fn the_deep_chain_resolves_across_passes_and_counts_every_begin() {
    let dict = run_dict();
    let verbs = "  DREAM:\n    actors: 1 kind:character\n    duration: 1h\n    recipes:\n      a when count(actors) >= 1 -> NARRATE \"a\", RUN \"b\"\n      b when count(actors) >= 1 -> NARRATE \"b\", RUN \"c\"\n      c when count(actors) >= 1 -> NARRATE \"c\"";
    let (c, s) = compile(&dict, &rig(verbs, None));
    let base = clone_state(&s);
    // stage 1: a runs [0,60], completes, opens b at the completion clock
    let r1 = run(&c, &dict, s, "0:0 @ ANA DREAM", 1);
    assert_eq!(r1.rejected, None);
    assert_eq!(begin_recipes(&r1), [vec!["a".to_string()], vec!["b".to_string()]]);
    assert_eq!(r1.records[2].start_min, Some(60), "b begins at a's completion clock");
    assert_eq!(r1.records[2].t_game, "0:2");
    assert_eq!(r1.state.now, 60);
    assert_eq!(r1.state.windows.len(), 1, "b is still processing");
    assert_eq!(executed_entries(&r1.state), vec![("a".to_string(), 1), ("b".to_string(), 1)]);
    // stage 2: b completes at 120 and opens c
    let r2 = advance(&c, &dict, r1.state, "0:4", 100);
    assert_eq!(r2.rejected, None);
    assert_eq!(kind_spine(&r2), ["complete", "action", "wait"]);
    assert_eq!(begin_recipes(&r2), [vec!["c".to_string()]]);
    assert_eq!(r2.records[1].start_min, Some(120));
    assert_eq!(executed_entries(&r2.state), vec![("a".to_string(), 1), ("b".to_string(), 1), ("c".to_string(), 1)]);
    // stage 3: c completes plainly — the chain ends
    let r3 = advance(&c, &dict, r2.state, "0:6", 200);
    assert_eq!(r3.rejected, None);
    assert_eq!(kind_spine(&r3), ["complete", "wait"]);
    assert_eq!(r3.state.windows.len(), 0);
    assert_eq!(r3.state.now, 180);
    // the whole chain folds identically, executed map included
    let mut records = r1.records.clone();
    records.extend(r2.records.iter().cloned());
    records.extend(r3.records.iter().cloned());
    let replayed = fold(&records, &base, &dict, &c).unwrap();
    assert_eq!(snap(&replayed), snap(&r3.state));
    assert_eq!(
        executed_entries(&replayed),
        vec![("a".to_string(), 1), ("b".to_string(), 1), ("c".to_string(), 1)]
    );
}
