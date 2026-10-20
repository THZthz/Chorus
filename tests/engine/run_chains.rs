//! `RUN` — recipe chaining (spec §6.7): the a→b hand-off with fold/JSONL
//! equality and continue-playing, the `when:`-disambiguated dream router, the
//! silent chain end, the ROLL+IF chance fork, the pinned recipe's top-level
//! REFUSE refusing the PARENT window, the second-RUN refusal, the §6.4
//! max-exhausted candidate skipping, the 1024-window livelock budget, the
//! load checks (same-verb candidates, trigger homes, the quoted-id arg form,
//! IF branches included), the deep chain resolving across passes with
//! `State.executed` counting every begin, and the follow-up clause's
//! PROJECTION onto the candidate verb's declared roles (§6.7: cross-verb
//! follow-ups carry only the roles their verb declares — the dropped cards
//! never lock; the E7 slice shape: the bootstrap with-card stays free through
//! the self-rechaining wheel and the dream restore lands clean).
//!
//! Conventions follow tests/decks/decks.rs + tests/engine/max_executions.rs: a fresh
//! dictionary per test, error patterns copied CHARACTER FOR CHARACTER from
//! the frozen literals (regex-escaped), and play/fold equality asserted
//! through the shared `snap`.

use crate::common::harness::assert_err_msg;
use crate::common::rig::{
    begin_recipes, compile, executed_entries, kind_spine, settle_line, tmp_log_path,
};
use crate::common::snap::snap;
use chorus::engine::settle::{done, refusal};

use chorus::content::script::compile_script;
use chorus::content::{
    compile_content, CompiledContent, ContentRaw, RecipeDef, SlotDef, VerbContent,
};
use chorus::core::clock::parse_clock;
use chorus::core::dice::roll_die;
use chorus::core::dict::{core_dictionary, Dictionary};
use chorus::core::format::format_sentence;
use chorus::core::parse::{parse_outcomes, parse_sentence};
use chorus::core::sentence::Arg;
use chorus::core::types::{clone_state, AttrValue, State};
use chorus::engine::fold::fold;
use chorus::engine::store::{load_log, save_log};
use chorus::engine::{execute_step, ExecOptions, ExecResult, Step};

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
    format!("  DREAM:\n    actors: 1 kind:character\n    duration: 0m\n    recipes:\n{recipes}")
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
        ExecOptions { first_record_id, id_gen: Some(&mut id_gen) },
    )
    .unwrap()
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
    let s = parse_sentence(
        "0:0 @ ANA DREAM -> NARRATE \"a\", RUN \"study.recipe0\", \"study.recipe1\", NARRATE \"z\"",
        &dict,
    )
    .unwrap();
    let [narr_a, run, narr_z] = s.outcomes.as_slice() else {
        panic!("three outcomes");
    };
    assert_eq!(narr_a.op, "NARRATE");
    assert_eq!(run.op, "RUN");
    assert_eq!(run.args.len(), 2, "both quoted candidates belong to the RUN");
    assert_eq!(narr_z.op, "NARRATE");
    // the canonical shell renders re-quoted and re-parses identically
    let out = format_sentence(&s, &dict).unwrap();
    assert_eq!(
        out,
        "0:0 @ ANA DREAM -> NARRATE \"a\", RUN(\"study.recipe0\", \"study.recipe1\"), NARRATE \"z\""
    );
    let s2 = parse_sentence(&out, &dict).unwrap();
    assert_eq!(s2.outcomes, s.outcomes);
    assert_eq!(format_sentence(&s2, &dict).unwrap(), out);
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
    let r = settle_line(&c, &dict, s, "0:0 @ ANA DREAM", 1).unwrap();
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
    // §6.4: the count rides the ONE registration path — both begins counted
    assert_eq!(executed_entries(&r.state), vec![("a".to_string(), 1), ("b".to_string(), 1)]);
    assert!(r.state.windows.is_empty());
    assert_eq!(refusal(&r), None);
    // fold from the log: the play-inserted window and the fold-replayed BEGIN
    // land identical
    let replayed = fold(&r.records, &base, &dict, &c).unwrap();
    assert_eq!(snap(&replayed), snap(&r.state));
    assert_eq!(executed_entries(&replayed), vec![("a".to_string(), 1), ("b".to_string(), 1)]);
    // the records survive JSONL and still fold identically
    let path = tmp_log_path("run-chains-jsonl");
    save_log(&path, &r.records).unwrap();
    let loaded = load_log(&path).unwrap();
    assert_eq!(loaded, r.records);
    let refolded = fold(&loaded, &base, &dict, &c).unwrap();
    assert_eq!(snap(&refolded), snap(&r.state));
    // play continues from the FOLDED state and agrees with play from the
    // play-state (the fold is the state, §7)
    let from_fold = settle_line(&c, &dict, replayed, "0:0 @ ANA DREAM", 100).unwrap();
    let from_play = settle_line(&c, &dict, clone_state(&r.state), "0:0 @ ANA DREAM", 200).unwrap();
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
/// make this inexpressible, §6.7).
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
    let r1 = settle_line(&c, &dict, s, "0:0 @ ANA DREAM", 1).unwrap();
    assert_eq!(r1.rejected, None);
    assert_eq!(begin_recipes(&r1), [vec!["router".to_string()], vec!["r_low".to_string()]]);
    let ops: Vec<&str> = r1.records[3].outcomes.iter().map(|o| o.op.as_str()).collect();
    assert_eq!(ops, ["NARRATE"]);
    assert_eq!(r1.records[3].outcomes[0].args[0], Arg::Str { v: "low".to_string() });
    // completion 2: mood 1→2 — the SITUATION changed, so the MATCH changed
    let r2 = settle_line(&c, &dict, r1.state, "0:0 @ ANA DREAM", 100).unwrap();
    assert_eq!(r2.rejected, None);
    assert_eq!(begin_recipes(&r2), [vec!["router".to_string()], vec!["r_high".to_string()]]);
    assert_eq!(r2.records[3].outcomes[0].args[0], Arg::Str { v: "high".to_string() });
    assert_eq!(
        executed_entries(&r2.state),
        vec![("router".to_string(), 2), ("r_low".to_string(), 1), ("r_high".to_string(), 1),]
    );
}

/// The dream-router shape keyed on the BOUND mood card, end to end (§6.7's
/// walk + the E8 binding merge): a verb slot binds the mood card, the
/// follow-up candidate's `when:` reads that role, and the dispatch follows
/// the mood. The whole turn folds identically — the hand-off bindings are
/// eval-context only, the BEGIN row carries the projected clause alone
/// (play-inserted and fold-replayed windows land structurally equal).
#[test]
fn the_router_dispatches_on_the_bound_mood_card() {
    let dict = run_dict();
    let (c, s) = compile(
        &dict,
        &rig_full(
            "  DREAM:\n    actors: 1 kind:character\n    with: 0..1 kind:resource\n    duration: 0m\n    recipes:\n      router when count(actors) >= 1 -> NARRATE \"route\", RUN \"m_dread\", \"m_rapture\", \"m_plain\"\n      m_dread when total(with, aspect:dread) >= 1 -> NARRATE \"dread night\"\n      m_rapture when total(with, aspect:rapture) >= 1 -> NARRATE \"rapture night\"\n      m_plain when count(actors) >= 1 -> NARRATE \"plain night\"",
            None,
            Some(
                "  DREAD_MARK is resource with dread:1 at ANA\n  RAPTURE_MARK is resource with rapture:1 at ANA",
            ),
        ),
    );
    let base = clone_state(&s);
    // dream on the dread: the walk pins m_dread — the gate read the BOUND
    // with-card through the hand-off bindings
    let r1 = settle_line(&c, &dict, s, "0:0 @ ANA DREAM WITH DREAD_MARK", 1).unwrap();
    assert_eq!(r1.rejected, None);
    assert_eq!(begin_recipes(&r1), [vec!["router".to_string()], vec!["m_dread".to_string()]]);
    assert_eq!(r1.records[3].outcomes[0].args[0], Arg::Str { v: "dread night".to_string() });
    // dream on the rapture: same router, different mood, different dispatch
    let r2 = settle_line(&c, &dict, r1.state, "0:0 @ ANA DREAM WITH RAPTURE_MARK", 100).unwrap();
    assert_eq!(r2.rejected, None);
    assert_eq!(begin_recipes(&r2), [vec!["router".to_string()], vec!["m_rapture".to_string()]]);
    // the follow-up BEGIN row carries the projected clause alone — the
    // gate's merged bindings never reach the log
    let begin = &r2.records[2];
    assert_eq!(begin.actors.len(), 1);
    assert_eq!(begin.instruments.len(), 1, "the with role rides");
    assert_eq!(begin.outcomes.len(), 0, "no PROGRESS, no eval-context leak");
    // both turns fold identically, windows included
    let mut records = r1.records.clone();
    records.extend(r2.records.iter().cloned());
    let replayed = fold(&records, &base, &dict, &c).unwrap();
    assert_eq!(replayed.windows, r2.state.windows, "identical OpenWindows");
    assert_eq!(snap(&replayed), snap(&r2.state));
}

/// The cross-verb hand-off (§6.7): the candidate verb declares ACTORS only,
/// so the projection DROPS the parent's with-card — and the gate still reads
/// it, through the parent window's committed binding (the E8 merge fills the
/// dropped roles). This is CS's linked-target-gated-on-the-situation shape.
#[test]
fn a_cross_verb_gate_reads_the_parent_situation() {
    let dict = run_dict();
    let (c, s) = compile(
        &dict,
        &rig_full(
            "  DREAM:\n    actors: 1 kind:character\n    with: 0..1 kind:resource\n    duration: 0m\n    recipes:\n      router aspects hand when count(actors) >= 1 -> NARRATE \"route\"\n  WAIT:\n    actors: 1 kind:character\n    duration: 1h\n    recipes:\n      rest_hot when total(with, aspect:hot) >= 1 -> NARRATE \"hot rest\"\n      rest_plain when count(actors) >= 1 -> NARRATE \"plain rest\"",
            None,
            Some(
                "  EMBER is resource with hot:1 at ANA on complete hand -> (RUN \"rest_hot\", \"rest_plain\")",
            ),
        ),
    );
    let base = clone_state(&s);
    let r = settle_line(&c, &dict, s, "0:0 @ ANA DREAM WITH EMBER", 1).unwrap();
    assert_eq!(r.rejected, None);
    // the first candidate's when reads `with` — a role WAIT does not declare
    // and the projection drops — and still matches on the parent's EMBER
    assert_eq!(begin_recipes(&r), [vec!["router".to_string()], vec!["rest_hot".to_string()]]);
    // the follow-up BEGIN carries only the declared actor role, as ever (E7)
    let begin = &r.records[2];
    assert_eq!(begin.verb, "WAIT");
    assert_eq!(begin.actors.len(), 1);
    assert!(begin.instruments.is_empty(), "with is projected away");
    // the gate's parent-situation read never reached the log; fold agrees
    let replayed = fold(&r.records, &base, &dict, &c).unwrap();
    assert_eq!(replayed.windows, r.state.windows, "identical OpenWindows");
    assert_eq!(snap(&replayed), snap(&r.state));
}

/// Projected-binding precedence (the E8 behavior change): the parent's own
/// outcomes DESTROY the bound with-card and mint a fresh same-named copy;
/// the gate reads the PROJECTED binding — the fresh copy, resolved against
/// the post-outcome draft (what the follow-up will carry) — not the parent
/// window's stale pre-outcome id (dead in the draft, a silent miss before
/// the merge).
#[test]
fn the_gate_reads_the_projected_binding_over_the_parent_stale_one() {
    let dict = run_dict();
    let (c, s) = compile(
        &dict,
        &rig_full(
            // the re-mint is a kind:NAME SPAWN (an entity name has no
            // prototype to mint from) whose authored aspects= pair restores
            // the hot aspect — the fresh copy is the card the follow-up
            // will carry
            "  DREAM:\n    actors: 1 kind:character\n    with: 0..1 kind:resource\n    duration: 0m\n    recipes:\n      router when count(actors) >= 1 -> DESTROY WITH, SPAWN resource:EMBER aspects=\"{\\\"hot\\\":1}\", NARRATE \"route\", RUN \"m_high\", \"m_low\"\n      m_high when total(with, aspect:hot) >= 1 -> NARRATE \"high\"\n      m_low when count(actors) >= 1 -> NARRATE \"low\"",
            None,
            Some("  EMBER is resource with hot:1 at ANA"),
        ),
    );
    let base = clone_state(&s);
    let r = settle_line(&c, &dict, s, "0:0 @ ANA DREAM WITH EMBER", 1).unwrap();
    assert_eq!(r.rejected, None);
    assert_eq!(refusal(&r), None, "the re-mint lands clean");
    // the fresh EMBER carries hot:1 — the projected binding sees it
    assert_eq!(begin_recipes(&r), [vec!["router".to_string()], vec!["m_high".to_string()]]);
    assert_eq!(r.records[3].outcomes[0].args[0], Arg::Str { v: "high".to_string() });
    // the minted EMBER is a NEW card (the parent consumed the old one): the
    // re-minted copy is the only living EMBER
    assert!(
        r.state.entities.values().filter(|e| e.name == "EMBER" && e.dead_action.is_none()).count()
            == 1,
        "exactly the fresh EMBER lives"
    );
    // fold symmetry: identical state (the 0m follow-up completed in-pass;
    // the open-window symmetry is the two pins above)
    let replayed = fold(&r.records, &base, &dict, &c).unwrap();
    assert_eq!(snap(&replayed), snap(&r.state));
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
    let r = settle_line(&c, &dict, s, "0:0 @ ANA DREAM", 1).unwrap();
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

/// The chance fork (§6.7's authoring shape): ROLL + IF with one RUN per arm
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
    let r = settle_line(&c, &dict, s, "0:0 @ ANA DREAM", 1).unwrap();
    assert_eq!(r.rejected, None);
    assert_eq!(refusal(&r), None, "the taken arm's RUN applied cleanly");
    // the parent's COMPLETE carries the ROLLED row; the RUN itself logs nothing
    let fork_ops: Vec<&str> = r.records[1].outcomes.iter().map(|o| o.op.as_str()).collect();
    assert_eq!(fork_ops, ["ROLLED"]);
    assert_eq!(r.records[1].outcomes[0].args[0], Arg::Int { v: total });
    // exactly ONE follow-up — the taken arm's candidate
    assert_eq!(begin_recipes(&r), [vec!["fork".to_string()], vec![expected.to_string()]]);
    assert_eq!(
        executed_entries(&r.state),
        vec![("fork".to_string(), 1), (expected.to_string(), 1),]
    );
    // determinism: the same counters throw the same die from the same state
    let r2 = settle_line(&c, &dict, clone_state(&r.state), "0:0 @ ANA DREAM", 100).unwrap();
    assert_eq!(begin_recipes(&r2), begin_recipes(&r));
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
    let r = settle_line(&c, &dict, s, "0:0 @ ANA DREAM", 1).unwrap();
    assert_eq!(r.rejected, None);
    assert_eq!(
        refusal(&r),
        Some("a second RUN in one completion — a station holds one window (§6.7)".to_string())
    );
    assert_eq!(kind_spine(&r), ["action", "complete", "wait"], "no follow-up opened");
    let ops: Vec<&str> = done(&r).unwrap().outcomes.iter().map(|o| o.op.as_str()).collect();
    assert_eq!(ops, ["REFUSE"]);
    assert_eq!(executed_entries(&r.state), vec![("greedy".to_string(), 1)]);
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
    let r =
        settle_line(&c, &dict, s, "0:0 @ ANA AT HOUSE DREAM MAGICK_BOOK WITH CHARM", 1).unwrap();
    assert_eq!(r.rejected, None);
    assert_eq!(begin_recipes(&r), [vec!["parent".to_string()], vec!["child".to_string()]]);
    // the follow-up BEGIN carries every parent role, authored-form
    let begin_child = &r.records[2];
    assert_eq!(begin_child.objects.len(), 1, "object rides");
    assert_eq!(begin_child.instruments.len(), 1, "with rides");
    assert!(begin_child.at.is_some(), "at rides");
    assert_eq!(begin_child.t_game, "0:6", "beginning at the completion clock");
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
    let r2 = advance(&c, &dict, r.state, "0:12", 100);
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
    let r = settle_line(&c, &dict, s, "0:0 @ ANA DREAM MAGICK_BOOK WITH CHARM", 1).unwrap();
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
    assert_eq!(
        w.refs.get(&chorus::core::roles::Role::With).map(Vec::len),
        Some(1),
        "the authored ref rides"
    );
    assert_eq!(w.locked.len(), 1, "the dead card is not locked");
    assert!(!r.state.entities.values().any(|e| e.name == "CHARM" && e.dead_action.is_none()));
    // play-inserted vs fold-replayed: identical OpenWindows
    let replayed = fold(&r.records, &base, &dict, &c).unwrap();
    assert_eq!(replayed.windows, r.state.windows, "identical OpenWindows");
    assert_eq!(snap(&replayed), snap(&r.state));
}

/// Cross-verb projection (§6.7, the E7 fix): a parent holding object/with/at
/// chains — through the with-card's `on complete` trigger, the one legal
/// cross-verb home (§6.7) — into a candidate whose verb declares ACTORS
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
    let r =
        settle_line(&c, &dict, s, "0:0 @ ANA AT HOUSE DREAM MAGICK_BOOK WITH CHARM", 1).unwrap();
    assert_eq!(r.rejected, None);
    assert_eq!(begin_recipes(&r), [vec!["parent".to_string()], vec!["rest".to_string()]]);
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
    assert_eq!(w.refs.get(&chorus::core::roles::Role::Actors).map(Vec::len), Some(1));
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
    let probe = settle_line(
        &c,
        &dict,
        clone_state(&r.state),
        "0:0 @ ANA AT HOUSE DREAM MAGICK_BOOK WITH CHARM",
        100,
    )
    .unwrap();
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
    let ops: Vec<&str> = r2.records[0].outcomes.iter().map(|o| o.op.as_str()).collect();
    assert_eq!(ops, ["SET", "NARRATE"], "the actor role rode the window");
    let ana = r2.state.by_name["ANA"][0].clone();
    assert_eq!(r2.state.entities[ana.as_str()].attrs.get("silver"), Some(&AttrValue::Num(1)));
}

/// The E7 regression, the slice shape in miniature (§6.7 projection;
/// docs/plans/v0.6-engine-findings.md E7): a work bootstrap whose with-card
/// carries the `on complete job` induce (§6.7) chains a self-rechaining
/// WAIT wheel. Pre-projection every rechained window inherited
/// `with=VIGOR`: the bootstrap card was busy until the chain died, and a
/// restored mint re-locked by name. Post-projection the wheel's windows
/// carry the actor role only: VIGOR is never busy, the wheel drains it, the
/// dream restore mints it back without a §9.2 rejection, and the next
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
    // and chains time.turn's analog cross-verb (§6.7)
    let r1 = settle_line(&c, &dict, s, "0:0 @ ANA WORK WITH VIGOR", 1).unwrap();
    assert_eq!(r1.rejected, None);
    assert_eq!(begin_recipes(&r1), [vec!["work.job".to_string()], vec!["turn".to_string()]]);
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
    let probe1 = settle_line(&c, &dict, r1.state, "0:0 @ ANA WORK WITH VIGOR", 100).unwrap();
    assert_eq!(probe1.rejected, None, "VIGOR is free while the wheel runs");
    assert_eq!(refusal(&probe1), None);
    // the wheel drains the body at 1:0 and rechains
    let r2 = advance(&c, &dict, probe1.state, "1:0", 200);
    assert_eq!(r2.rejected, None);
    assert!(
        !r2.state.entities.values().any(|e| e.name == "VIGOR" && e.dead_action.is_none()),
        "the wheel ate VIGOR"
    );
    assert!(r2
        .state
        .entities
        .values()
        .any(|e| e.name == "THE_WEARY_HOUR" && e.dead_action.is_none()));
    assert_eq!(r2.state.windows.len(), 1, "the wheel rechained");
    assert!(r2.state.windows.values().next().unwrap().locked.is_empty());
    // the restore: dream.rest_vigor mints VIGOR back — no §9.2 rejection,
    // the restored mint never re-locks by name on a later rechain
    let r3 = settle_line(&c, &dict, r2.state, "1:0 @ ANA DREAM WITH THE_WEARY_HOUR", 300).unwrap();
    assert_eq!(r3.rejected, None);
    assert_eq!(refusal(&r3), None, "the restore lands clean");
    assert!(
        r3.state.entities.values().any(|e| e.name == "VIGOR" && e.dead_action.is_none()),
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
    let probe2 = settle_line(&c, &dict, r4.state, "2:0 @ ANA WORK WITH VIGOR", 500).unwrap();
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
    assert_eq!(replayed.windows, probe2.state.windows, "identical OpenWindows");
    assert_eq!(snap(&replayed), snap(&probe2.state));
}

/// The guard trial's scratch never leaks: a gated candidate whose body ROLLs
/// and SPAWNs before its top-level REFUSE consumes the SCRATCH's rng counters
/// and mints into the SCRATCH only — the real draft's counter stays pinned
/// and no entity lands (§6.7's Phase-3-shaped trial).
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
    let r = settle_line(&c, &dict, s, "0:0 @ ANA DREAM", 1).unwrap();
    assert_eq!(r.rejected, None);
    assert_eq!(refusal(&r), Some("the gate says no".to_string()));
    assert_eq!(
        kind_spine(&r),
        ["action", "complete", "wait"],
        "the pinned REFUSE rejects the parent window, no follow-up"
    );
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

/// A max-exhausted candidate is simply non-matching (§6.4): the walk skips
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
    let r1 = settle_line(&c, &dict, s, "0:0 @ ANA DREAM", 1).unwrap();
    assert_eq!(r1.rejected, None);
    assert_eq!(begin_recipes(&r1), [vec!["parent".to_string()], vec!["spent".to_string()]]);
    // play 2: `spent` is exhausted — the walk continues into `fresh`
    let r2 = settle_line(&c, &dict, r1.state, "0:0 @ ANA DREAM", 100).unwrap();
    assert_eq!(r2.rejected, None);
    assert_eq!(begin_recipes(&r2), [vec!["parent".to_string()], vec!["fresh".to_string()]]);
    assert_eq!(
        executed_entries(&r2.state),
        vec![("parent".to_string(), 2), ("spent".to_string(), 1), ("fresh".to_string(), 1),]
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
    let p1 = settle_line(&c3, &dict, s3, "0:0 @ ANA DREAM", 300).unwrap();
    assert_eq!(p1.rejected, None);
    let p2 = settle_line(&c3, &dict, p1.state, "0:0 @ ANA DREAM", 400).unwrap();
    assert_eq!(p2.rejected, None);
    assert_eq!(kind_spine(&p2), ["action", "complete", "wait"]);
    assert_eq!(refusal(&p2), None);
    assert_eq!(
        executed_entries(&p2.state),
        vec![("parent".to_string(), 2), ("once".to_string(), 1)]
    );
}

/// The livelock guard (§6.7/§10): two zero-duration recipes RUN-ing each
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
    let r = settle_line(&c, &dict, s, "0:0 @ ANA DREAM", 1).unwrap();
    let rejected = r
        .rejected
        .clone()
        .unwrap_or_else(|| panic!("expected the livelock rejection, got {:?}", r.rejected));
    assert!(
        rejected.contains("livelock guard")
            && rejected.contains("1024")
            && rejected.contains("§6.7"),
        "{rejected}"
    );
    // the settle advance rolled back whole: nothing half-logged
    assert!(r.records.is_empty(), "the rejected pass logs nothing");
    assert_eq!(snap(&r.state), snap(&original), "the original state rides back");
}

// ----------------------------------------------------------- fall-through ----
// The F-series: the chain slot is earned by a PIN, not by a reach (§6.7) —
// a whiffed RUN list spends nothing, so a later chain op in the same
// completion gets its walk.

/// F-1: fall-through on a whiff — `RUN "gone", RUN "here"` with gone's `when`
/// dead: `here` pins, no refusal, one follow-up; the whole turn folds
/// identically from the log and from JSONL.
#[test]
fn a_whiffed_run_list_falls_through_to_a_later_run() {
    let dict = run_dict();
    let (c, s) = compile(
        &dict,
        &rig(
            &dream(
                "      parent when count(actors) >= 1 -> RUN \"gone\", RUN \"here\"\n      gone when actors.aspect.mood >= 99 -> NARRATE \"gone\"\n      here when count(actors) >= 1 -> NARRATE \"here\"",
            ),
            None,
        ),
    );
    let base = clone_state(&s);
    let r = settle_line(&c, &dict, s, "0:0 @ ANA DREAM", 1).unwrap();
    assert_eq!(r.rejected, None);
    assert_eq!(refusal(&r), None, "a whiffed list spends nothing, refuses nothing");
    assert_eq!(kind_spine(&r), ["action", "complete", "action", "complete", "wait"]);
    assert_eq!(begin_recipes(&r), [vec!["parent".to_string()], vec!["here".to_string()]]);
    let ops: Vec<&str> = r.records[3].outcomes.iter().map(|o| o.op.as_str()).collect();
    assert_eq!(ops, ["NARRATE"]);
    let replayed = fold(&r.records, &base, &dict, &c).unwrap();
    assert_eq!(snap(&replayed), snap(&r.state));
    let path = tmp_log_path("run-chains-fallthrough-jsonl");
    save_log(&path, &r.records).unwrap();
    let loaded = load_log(&path).unwrap();
    assert_eq!(loaded, r.records);
    let refolded = fold(&loaded, &base, &dict, &c).unwrap();
    assert_eq!(snap(&refolded), snap(&r.state));
}

/// F-2: fall-through on exhaustion — `spent max 1` already counted by a prior
/// turn's clean chain; then `RUN "spent", RUN "fresh"`: the whiffed list
/// spends nothing and `fresh` pins (was: parent-window refusal).
#[test]
fn an_exhausted_run_list_falls_through_to_a_later_run() {
    let dict = run_dict();
    let (c, s) = compile(
        &dict,
        &rig(
            &dream(
                "      starter max 1 when count(actors) >= 1 -> RUN \"spent\"\n      parent when count(actors) >= 1 -> RUN \"spent\", RUN \"fresh\"\n      spent max 1 when count(actors) >= 1 -> NARRATE \"spent\"\n      fresh when count(actors) >= 1 -> NARRATE \"fresh\"",
            ),
            None,
        ),
    );
    // play 1: the starter pins `spent` cleanly — its window registers and
    // the §6.4 count lands
    let r1 = settle_line(&c, &dict, s, "0:0 @ ANA DREAM", 1).unwrap();
    assert_eq!(r1.rejected, None);
    assert_eq!(refusal(&r1), None);
    assert_eq!(begin_recipes(&r1), [vec!["starter".to_string()], vec!["spent".to_string()]]);
    // play 2: `spent` is exhausted — the first list whiffs, the slot goes
    // unspent and the second RUN gets its walk (previously a refusal here)
    let r2 = settle_line(&c, &dict, r1.state, "0:0 @ ANA DREAM", 100).unwrap();
    assert_eq!(r2.rejected, None);
    assert_eq!(refusal(&r2), None);
    assert_eq!(begin_recipes(&r2), [vec!["parent".to_string()], vec!["fresh".to_string()]]);
    assert_eq!(
        executed_entries(&r2.state),
        vec![
            ("starter".to_string(), 1),
            ("spent".to_string(), 1),
            ("parent".to_string(), 1),
            ("fresh".to_string(), 1),
        ]
    );
}

/// F-4: fall-through composes with the inline gate — the first RUN whiffs,
/// the second pins a gated candidate whose REFUSE fires: the PARENT window
/// refuses, no follow-up opens, and the trial's rng/mint consumption stays
/// scratch-only.
#[test]
fn a_fallthrough_pins_second_run_whose_gate_refuses_the_parent() {
    let dict = run_dict();
    let (c, s) = compile(
        &dict,
        &rig(
            &dream(
                "      parent when count(actors) >= 1 -> RUN \"gone\", RUN \"gated\"\n      gone when actors.aspect.mood >= 99 -> NARRATE \"gone\"\n      gated when count(actors) >= 1 -> ROLL(2, 6), SPAWN TRINKET, REFUSE(\"the gate says no\")",
            ),
            None,
        ),
    );
    let base = clone_state(&s);
    assert_eq!(s.rng, 0);
    let r = settle_line(&c, &dict, s, "0:0 @ ANA DREAM", 1).unwrap();
    assert_eq!(r.rejected, None);
    assert_eq!(refusal(&r), Some("the gate says no".to_string()));
    assert_eq!(kind_spine(&r), ["action", "complete", "wait"], "no follow-up opened");
    let ops: Vec<&str> = done(&r).unwrap().outcomes.iter().map(|o| o.op.as_str()).collect();
    assert_eq!(ops, ["REFUSE"]);
    assert_eq!(executed_entries(&r.state), vec![("parent".to_string(), 1)]);
    assert!(r.state.windows.is_empty());
    assert_eq!(r.state.rng, 0, "the trial's ROLL never touched the real counter");
    assert!(!r.state.entities.values().any(|e| e.name == "TRINKET"));
    let replayed = fold(&r.records, &base, &dict, &c).unwrap();
    assert_eq!(snap(&replayed), snap(&r.state));
}

/// F-5: deferred order across a fall-through — `RUN "whiff", NARRATE "mid",
/// RUN "here"`: "mid" logs in the first pass, the parked lists replay in
/// order, the whiff spends nothing and `here` pins after it.
#[test]
fn a_whiff_between_narrate_and_a_pinning_run_falls_through_in_order() {
    let dict = run_dict();
    let (c, s) = compile(
        &dict,
        &rig(
            &dream(
                "      parent when count(actors) >= 1 -> RUN \"whiff\", NARRATE \"mid\", RUN \"here\"\n      whiff when actors.aspect.mood >= 99 -> NARRATE \"whiff\"\n      here when count(actors) >= 1 -> NARRATE \"here\"",
            ),
            None,
        ),
    );
    let base = clone_state(&s);
    let r = settle_line(&c, &dict, s, "0:0 @ ANA DREAM", 1).unwrap();
    assert_eq!(r.rejected, None);
    assert_eq!(refusal(&r), None);
    // "mid" is the only concrete outcome on the COMPLETE row (the whiff and
    // the pin log nothing of themselves); the follow-up BEGIN follows it
    let ops: Vec<&str> = r.records[1].outcomes.iter().map(|o| o.op.as_str()).collect();
    assert_eq!(ops, ["NARRATE"]);
    assert_eq!(r.records[1].outcomes[0].args[0], Arg::Str { v: "mid".to_string() });
    assert_eq!(kind_spine(&r), ["action", "complete", "action", "complete", "wait"]);
    assert_eq!(begin_recipes(&r), [vec!["parent".to_string()], vec!["here".to_string()]]);
    let replayed = fold(&r.records, &base, &dict, &c).unwrap();
    assert_eq!(snap(&replayed), snap(&r.state));
}

/// The load checks (§6.7): candidates must name compiled recipes through the
/// GLOBAL table — cross-verb ids legal (an unknown id is the load error), IF
/// branches included; chain ops are legal in slot and expire triggers (§6.6,
/// the event-chain channel); the bare/dotted id and the empty list are parse
/// errors.
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
        "recipe a: RUN 'nope' does not name a compiled recipe — candidates resolve against the global recipe table \\(§6\\.7\\)",
    );
    // cross-verb id: wait.default belongs to WAIT — the global table owns it,
    // so the chain loads
    compile(&dict, &dream_verb("      a when count(actors) >= 1 -> RUN \"wait.default\""));
    // IF branches included — an untaken branch hides nothing at load
    assert_err_msg(
        compile_script(
            &dream_verb(
                "      a when count(actors) >= 1 -> IF count(actors) >= 1 -> (RUN \"nope\")",
            ),
            &dict,
        ),
        "recipe a: RUN 'nope' does not name a compiled recipe",
    );
    // a legal chain loads (the same-verb candidate resolves)
    compile(
        &dict,
        &dream_verb("      a when count(actors) >= 1 -> RUN \"b\"\n      b when count(actors) >= 1 -> NARRATE \"b\""),
    );
    // slot + expire trigger bodies chain too (§6.6): the trigger's own
    // candidate resolves through the global table
    compile(
        &dict,
        &rig(
            &dream("      a when count(actors) >= 1 -> NARRATE \"a\""),
            Some("  tick at hour 6 -> RUN \"a\"\n  boom on expire of THING -> FORK \"a\""),
        ),
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
                    greedy: false,
                    consuming: false,
                }],
                duration: 0,
                recipes: vec![RecipeDef {
                    id: Some("a".into()),
                    when: Some("count(actors) >= 1".into()),
                    shown: None,
                    hint: None,
                    max: None,
                    aspects: Vec::new(),
                    dur: None,
                    out: vec!["RUN \"ghost\"".to_string()],
                }],
                ambient: false,
            },
        )]
        .into_iter()
        .collect(),
        ..ContentRaw::default()
    };
    assert_err_msg(
        compile_content(&raw, &dict, &[]),
        "recipe a: RUN 'ghost' does not name a compiled recipe",
    );
}

// ----------------------------------------------------- cross-verb chains -----
// The C-series: a recipe body may RUN any compiled recipe, same verb or not —
// the global `recipe_verbs` table is the ONE resolution scope (§6.7), and the
// busy check applies in recipe bodies too (the parent's own window excluded).

/// C-3: a recipe-body cross-verb hand-off — DREAM completes, WAIT's recipe
/// begins at the completion clock; the whole turn folds identically from the
/// log directly and from JSONL.
#[test]
fn a_recipe_body_chains_across_verbs() {
    let dict = run_dict();
    let (c, s) = compile(
        &dict,
        &rig(
            "  DREAM:\n    actors: 1 kind:character\n    duration: 0m\n    recipes:\n      a when count(actors) >= 1 -> NARRATE \"a done\", RUN \"wait.rest\"\n  WAIT:\n    actors: 1 kind:character\n    duration: 0m\n    recipes:\n      wait.rest when count(actors) >= 1 -> NARRATE \"waited\"",
            None,
        ),
    );
    let base = clone_state(&s);
    let r = settle_line(&c, &dict, s, "0:0 @ ANA DREAM", 1).unwrap();
    assert_eq!(r.rejected, None);
    assert_eq!(kind_spine(&r), ["action", "complete", "action", "complete", "wait"]);
    assert_eq!(begin_recipes(&r), [vec!["a".to_string()], vec!["wait.rest".to_string()]]);
    // the follow-up begins AT THE COMPLETION CLOCK on the CANDIDATE's verb
    let begin_rest = &r.records[2];
    assert_eq!(begin_rest.verb, "WAIT");
    assert_eq!(begin_rest.start_min, Some(0));
    assert_eq!(begin_rest.t_game, "0:0");
    assert_eq!(begin_rest.actors.len(), 1, "the parent's bindings ride along");
    let ops: Vec<&str> = r.records[3].outcomes.iter().map(|o| o.op.as_str()).collect();
    assert_eq!(ops, ["NARRATE"]);
    assert_eq!(
        executed_entries(&r.state),
        vec![("a".to_string(), 1), ("wait.rest".to_string(), 1)]
    );
    assert!(r.state.windows.is_empty());
    assert_eq!(refusal(&r), None);
    // fold from the log, and from JSONL: identical state both ways
    let replayed = fold(&r.records, &base, &dict, &c).unwrap();
    assert_eq!(replayed.windows, r.state.windows, "identical OpenWindows");
    assert_eq!(snap(&replayed), snap(&r.state));
    let path = tmp_log_path("run-chains-cross-verb-jsonl");
    save_log(&path, &r.records).unwrap();
    let loaded = load_log(&path).unwrap();
    assert_eq!(loaded, r.records);
    let refolded = fold(&loaded, &base, &dict, &c).unwrap();
    assert_eq!(snap(&refolded), snap(&r.state));
}

/// C-4: a cross-verb RECIPE-BODY candidate whose `when:` reads a role the
/// candidate verb does not declare — the parent-situation merge (E8) fires
/// from a recipe body: the gate reads the parent's bound with-card through
/// the committed binding, while the synthesized BEGIN carries the projected
/// clause alone (the dropped card never locks).
#[test]
fn a_cross_verb_recipe_body_gate_reads_the_parent_situation() {
    let dict = run_dict();
    let (c, s) = compile(
        &dict,
        &rig_full(
            "  DREAM:\n    actors: 1 kind:character\n    with: 0..1 kind:resource\n    duration: 0m\n    recipes:\n      router when count(actors) >= 1 -> NARRATE \"route\", RUN \"rest_hot\", \"rest_plain\"\n  WAIT:\n    actors: 1 kind:character\n    duration: 1h\n    recipes:\n      rest_hot when total(with, aspect:hot) >= 1 -> NARRATE \"hot rest\"\n      rest_plain when count(actors) >= 1 -> NARRATE \"plain rest\"",
            None,
            Some("  EMBER is resource with hot:1 at ANA"),
        ),
    );
    let base = clone_state(&s);
    let r = settle_line(&c, &dict, s, "0:0 @ ANA DREAM WITH EMBER", 1).unwrap();
    assert_eq!(r.rejected, None);
    // rest_hot's when reads `with` — a role WAIT does not declare and the
    // projection drops — and still matches on the parent's EMBER
    assert_eq!(begin_recipes(&r), [vec!["router".to_string()], vec!["rest_hot".to_string()]]);
    // the BEGIN carries the projected clause alone: the actor role only
    let begin = &r.records[2];
    assert_eq!(begin.verb, "WAIT");
    assert_eq!(begin.actors.len(), 1);
    assert!(begin.instruments.is_empty(), "with is projected away");
    // the 1h follow-up is still open: play-inserted vs fold-replayed windows
    // land identical
    assert_eq!(r.state.windows.len(), 1);
    let w = r.state.windows.values().next().unwrap();
    assert_eq!(w.recipe, "rest_hot");
    assert!(w.locked.is_empty(), "the dropped card is not locked");
    let replayed = fold(&r.records, &base, &dict, &c).unwrap();
    assert_eq!(replayed.windows, r.state.windows, "identical OpenWindows");
    assert_eq!(snap(&replayed), snap(&r.state));
}

/// C-4's §6.3 scope face — the reach-P1 repro: the candidate verb declares
/// only `actors`, and its `when` reads the window sum `total(bound,
/// aspect:a)`. The parent's committed object stands in the MERGED bindings
/// (a role the candidate does not declare), so the gate pins on the
/// parent's charged card although the candidate's own declared-slot window
/// has ceiling 0 for the aspect — and the pinned follow-up mints. The whiff
/// control commits an aspect-free card: the gate misses and the chain ends
/// silently.
#[test]
fn a_bound_sum_gate_on_an_actors_only_candidate_reads_the_parents_object() {
    let dict = run_dict();
    let rig_text = || {
        rig_full(
            "  WORK:\n    actors: 1 kind:character\n    object: 1 kind:resource\n    duration: 0m\n    recipes:\n      go when count(actors) >= 1 -> NARRATE \"charged\", RUN \"explore.recipe0\"\n  EXPLORE:\n    actors: 1 kind:character\n    duration: 0m\n    recipes:\n      explore.recipe0 when total(bound, aspect:a) >= 5 -> SPAWN TRINKET",
            None,
            Some("  POT is resource with a:5\n  NOPE is resource"),
        )
    };
    let trinkets = |s: &State| {
        s.entities.values().filter(|e| e.name == "TRINKET" && e.dead_action.is_none()).count()
    };
    // the charged card rides the merged bindings: the pin fires, the
    // follow-up begins and completes, the mint lands
    let (c, s) = compile(&dict, &rig_text());
    let r = settle_line(&c, &dict, s, "0:0 @ ANA WORK POT", 1).unwrap();
    assert_eq!(r.rejected, None);
    assert_eq!(begin_recipes(&r), [vec!["go".to_string()], vec!["explore.recipe0".to_string()]]);
    assert_eq!(kind_spine(&r), ["action", "complete", "action", "complete", "wait"]);
    assert_eq!(trinkets(&r.state), 1);
    // the whiff control: no aspect-bearing card in the merged bindings, so
    // the gate misses and no follow-up opens
    let (c2, s2) = compile(&dict, &rig_text());
    let r2 = settle_line(&c2, &dict, s2, "0:0 @ ANA WORK NOPE", 1).unwrap();
    assert_eq!(r2.rejected, None);
    assert_eq!(begin_recipes(&r2), [vec!["go".to_string()]]);
    assert_eq!(trinkets(&r2.state), 0);
}

/// C-5: the busy check in a recipe body — two clauses in one step (1h each,
/// same end); the earlier completion's RUN targets the other clause's still
/// open window: the candidate skips SILENTLY (no follow-up, no refusal),
/// exactly like a completion-trigger hand-off onto a busy station.
#[test]
fn a_recipe_body_run_onto_a_busy_station_skips_silently() {
    let dict = run_dict();
    let (c, s) = compile(
        &dict,
        &rig(
            "  DREAM:\n    actors: 1 kind:character\n    duration: 1h\n    recipes:\n      a when count(actors) >= 1 -> NARRATE \"a\", RUN \"study.s\"\n  STUDY:\n    actors: 1 kind:character\n    duration: 1h\n    recipes:\n      study.s when count(actors) >= 1 -> NARRATE \"s\"",
            None,
        ),
    );
    let base = clone_state(&s);
    let r = settle_line(&c, &dict, s, "0:0 @ ANA DREAM ; ANA STUDY", 1).unwrap();
    assert_eq!(r.rejected, None);
    assert_eq!(refusal(&r), None, "a busy station skips, it does not refuse");
    // both windows complete; the DREAM completion (begin_id 1) fires first,
    // finds STUDY's window open and skips the candidate
    assert_eq!(kind_spine(&r), ["action", "complete", "complete", "wait"]);
    assert_eq!(begin_recipes(&r), [vec!["a".to_string(), "study.s".to_string()]]);
    assert_eq!(r.state.executed.get("study.s"), Some(&1), "no follow-up registered");
    let replayed = fold(&r.records, &base, &dict, &c).unwrap();
    assert_eq!(snap(&replayed), snap(&r.state));
}

/// C-6: the cross-verb livelock — a zero-duration cycle ACROSS verbs (DREAM
/// RUNs WAIT's recipe, WAIT RUNs DREAM's) spins inside one pass exactly like
/// a same-verb cycle: the pass fails loud at the 1024 budget, the step is
/// rejected with nothing logged, the original state rides back.
#[test]
fn a_cross_verb_zero_duration_run_cycle_trips_the_livelock_budget() {
    let dict = run_dict();
    let (c, s) = compile(
        &dict,
        &rig(
            "  DREAM:\n    actors: 1 kind:character\n    duration: 0m\n    recipes:\n      dream.ping when count(actors) >= 1 -> NARRATE \"ping\", RUN \"wait.pong\"\n  WAIT:\n    actors: 1 kind:character\n    duration: 0m\n    recipes:\n      wait.pong when count(actors) >= 1 -> NARRATE \"pong\", RUN \"dream.ping\"",
            None,
        ),
    );
    let original = clone_state(&s);
    let r = settle_line(&c, &dict, s, "0:0 @ ANA DREAM", 1).unwrap();
    let rejected = r
        .rejected
        .clone()
        .unwrap_or_else(|| panic!("expected the livelock rejection, got {:?}", r.rejected));
    assert!(
        rejected.contains("livelock guard")
            && rejected.contains("1024")
            && rejected.contains("§6.7"),
        "{rejected}"
    );
    assert!(r.records.is_empty(), "the rejected pass logs nothing");
    assert_eq!(snap(&r.state), snap(&original), "the original state rides back");
}

/// The deep chain a→b→c — RUN inside the follow-up — resolves ACROSS passes
/// (each window opens at its parent's completion clock), and §6.4 counts
/// every begin on both paths.
#[test]
fn the_deep_chain_resolves_across_passes_and_counts_every_begin() {
    let dict = run_dict();
    let verbs = "  DREAM:\n    actors: 1 kind:character\n    duration: 1h\n    recipes:\n      a when count(actors) >= 1 -> NARRATE \"a\", RUN \"b\"\n      b when count(actors) >= 1 -> NARRATE \"b\", RUN \"c\"\n      c when count(actors) >= 1 -> NARRATE \"c\"";
    let (c, s) = compile(&dict, &rig(verbs, None));
    let base = clone_state(&s);
    // stage 1: a runs [0,60], completes, opens b at the completion clock
    let r1 = settle_line(&c, &dict, s, "0:0 @ ANA DREAM", 1).unwrap();
    assert_eq!(r1.rejected, None);
    assert_eq!(begin_recipes(&r1), [vec!["a".to_string()], vec!["b".to_string()]]);
    assert_eq!(r1.records[2].start_min, Some(60), "b begins at a's completion clock");
    assert_eq!(r1.records[2].t_game, "0:6");
    assert_eq!(r1.state.now, 60);
    assert_eq!(r1.state.windows.len(), 1, "b is still processing");
    assert_eq!(executed_entries(&r1.state), vec![("a".to_string(), 1), ("b".to_string(), 1)]);
    // stage 2: b completes at 120 and opens c
    let r2 = advance(&c, &dict, r1.state, "0:12", 100);
    assert_eq!(r2.rejected, None);
    assert_eq!(kind_spine(&r2), ["complete", "action", "wait"]);
    assert_eq!(begin_recipes(&r2), [vec!["c".to_string()]]);
    assert_eq!(r2.records[1].start_min, Some(120));
    assert_eq!(
        executed_entries(&r2.state),
        vec![("a".to_string(), 1), ("b".to_string(), 1), ("c".to_string(), 1)]
    );
    // stage 3: c completes plainly — the chain ends
    let r3 = advance(&c, &dict, r2.state, "0:18", 200);
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

// -------------------------------------------------------------------- FORK ---
// The F2-series: FORK "id", … opens ONE follow-up window for EVERY matching
// candidate, in list order — the fan-out beside RUN's first-match walk (§6.7).

/// F2-1: parse/round-trip — `FORK "a", "b"` parses to two candidates, the
/// canonical shell `FORK("a", "b")` re-parses identically, the op composes
/// in an outcome list, and the arg-form errors are byte-pinned.
#[test]
fn fork_parses_round_trips_and_composes_in_a_list() {
    let dict = run_dict();
    let s = parse_sentence(
        "0:0 @ ANA DREAM -> NARRATE \"a\", FORK \"study.recipe0\", \"study.recipe1\", NARRATE \"z\"",
        &dict,
    )
    .unwrap();
    let [narr_a, fork, narr_z] = s.outcomes.as_slice() else {
        panic!("three outcomes");
    };
    assert_eq!(narr_a.op, "NARRATE");
    assert_eq!(fork.op, "FORK");
    assert_eq!(fork.args.len(), 2, "both quoted candidates belong to the FORK");
    assert_eq!(narr_z.op, "NARRATE");
    // the canonical shell renders re-quoted and re-parses identically
    let out = format_sentence(&s, &dict).unwrap();
    assert_eq!(
        out,
        "0:0 @ ANA DREAM -> NARRATE \"a\", FORK(\"study.recipe0\", \"study.recipe1\"), NARRATE \"z\""
    );
    let s2 = parse_sentence(&out, &dict).unwrap();
    assert_eq!(s2.outcomes, s.outcomes);
    assert_eq!(format_sentence(&s2, &dict).unwrap(), out);
    // the arg-form pins, at the parse layer
    let outs = parse_outcomes("FORK \"a.b\"", &dict, None).unwrap();
    assert_eq!(outs[0].op, "FORK");
    assert_eq!(
        parse_outcomes("FORK a.b", &dict, None).unwrap_err().msg,
        "FORK takes quoted-string recipe ids: FORK \"id\" [, \"id\" …] — got a"
    );
    assert_eq!(
        parse_outcomes("FORK", &dict, None).unwrap_err().msg,
        "FORK takes quoted-string recipe ids: FORK \"id\" [, \"id\" …] — got none"
    );
    assert_eq!(
        parse_outcomes("FORK(\"a\",)", &dict, None).unwrap_err().msg,
        "FORK takes quoted-string recipe ids: FORK \"id\" [, \"id\" …] — got )"
    );
}

/// F2-2: the load checks — an unknown id names the shared global-table
/// message (IF branches included), the hand-built raw face errors just as
/// loud, and FORK is a load error in a slot trigger body (events have no
/// window to chain from).
#[test]
fn the_fork_load_checks_pin_homes_and_arg_form() {
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
        compile_script(&dream_verb("      a when count(actors) >= 1 -> FORK \"nope\""), &dict),
        "recipe a: FORK 'nope' does not name a compiled recipe — candidates resolve against the global recipe table \\(§6\\.7\\)",
    );
    // cross-verb id: the global table owns it, the chain loads
    compile(&dict, &dream_verb("      a when count(actors) >= 1 -> FORK \"wait.default\""));
    // IF branches included — an untaken branch hides nothing at load
    assert_err_msg(
        compile_script(
            &dream_verb(
                "      a when count(actors) >= 1 -> IF count(actors) >= 1 -> (FORK \"nope\")",
            ),
            &dict,
        ),
        "recipe a: FORK 'nope' does not name a compiled recipe",
    );
    // a legal fork loads
    compile(
        &dict,
        &dream_verb("      a when count(actors) >= 1 -> FORK \"b\"\n      b when count(actors) >= 1 -> NARRATE \"b\""),
    );
    // slot trigger: the event-chain channel owns FORK too (§6.6)
    compile(
        &dict,
        &rig(
            &dream("      a when count(actors) >= 1 -> NARRATE \"a\""),
            Some("  tick at hour 6 -> FORK \"a\""),
        ),
    );
    // the hand-built content face fails just as loud
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
                    greedy: false,
                    consuming: false,
                }],
                duration: 0,
                recipes: vec![RecipeDef {
                    id: Some("a".into()),
                    when: Some("count(actors) >= 1".into()),
                    shown: None,
                    hint: None,
                    max: None,
                    aspects: Vec::new(),
                    dur: None,
                    out: vec!["FORK \"ghost\"".to_string()],
                }],
                ambient: false,
            },
        )]
        .into_iter()
        .collect(),
        ..ContentRaw::default()
    };
    assert_err_msg(
        compile_content(&raw, &dict, &[]),
        "recipe a: FORK 'ghost' does not name a compiled recipe",
    );
}

/// F2-3: the fan-out — `FORK "left", "right"` on distinct zero-duration verbs
/// opens BOTH follow-ups: spine `action, complete, action, action, complete,
/// complete, wait`, sequential BEGIN ids, both §6.4 counts ride, the whole
/// turn folds identically (from the log and from JSONL), and a second run of
/// the same turn from the same state is byte-identical.
#[test]
fn a_fork_fans_out_one_window_per_branch() {
    let dict = run_dict();
    let (c, s) = compile(
        &dict,
        &rig(
            "  DREAM:\n    actors: 1 kind:character\n    duration: 0m\n    recipes:\n      parent when count(actors) >= 1 -> NARRATE \"parent\", FORK \"wait.left\", \"study.right\"\n  WAIT:\n    actors: 1 kind:character\n    duration: 0m\n    recipes:\n      wait.left when count(actors) >= 1 -> NARRATE \"left\"\n  STUDY:\n    actors: 1 kind:character\n    duration: 0m\n    recipes:\n      study.right when count(actors) >= 1 -> NARRATE \"right\"",
            None,
        ),
    );
    let base = clone_state(&s);
    let r = settle_line(&c, &dict, s, "0:0 @ ANA DREAM", 1).unwrap();
    assert_eq!(r.rejected, None);
    assert_eq!(refusal(&r), None);
    assert_eq!(
        kind_spine(&r),
        ["action", "complete", "action", "action", "complete", "complete", "wait"]
    );
    assert_eq!(
        begin_recipes(&r),
        [
            vec!["parent".to_string()],
            vec!["wait.left".to_string()],
            vec!["study.right".to_string()]
        ]
    );
    // sequential BEGIN ids: the two branches own complete_id+1 and +2
    assert_eq!(r.records[2].id, r.records[1].id + 1);
    assert_eq!(r.records[3].id, r.records[1].id + 2);
    // the branch BEGINs carry their own verbs and the parent's bindings
    assert_eq!(r.records[2].verb, "WAIT");
    assert_eq!(r.records[3].verb, "STUDY");
    assert_eq!(r.records[2].actors.len(), 1);
    assert_eq!(r.records[3].actors.len(), 1);
    assert_eq!(
        executed_entries(&r.state),
        vec![
            ("parent".to_string(), 1),
            ("wait.left".to_string(), 1),
            ("study.right".to_string(), 1),
        ]
    );
    // fold from the log, and from JSONL: identical state both ways
    let replayed = fold(&r.records, &base, &dict, &c).unwrap();
    assert_eq!(replayed.windows, r.state.windows, "identical OpenWindows");
    assert_eq!(snap(&replayed), snap(&r.state));
    let path = tmp_log_path("fork-fanout-jsonl");
    save_log(&path, &r.records).unwrap();
    let loaded = load_log(&path).unwrap();
    assert_eq!(loaded, r.records);
    let refolded = fold(&loaded, &base, &dict, &c).unwrap();
    assert_eq!(snap(&refolded), snap(&r.state));
    // determinism: the same turn from the same state is byte-identical
    let r2 = settle_line(&c, &dict, clone_state(&base), "0:0 @ ANA DREAM", 1).unwrap();
    let path2 = tmp_log_path("fork-fanout-jsonl-2");
    save_log(&path2, &r2.records).unwrap();
    let a = std::fs::read_to_string(&path).unwrap();
    let b = std::fs::read_to_string(&path2).unwrap();
    assert_eq!(a, b, "byte-identical double run");
    assert_eq!(snap(&r2.state), snap(&r.state));
}

/// F2-4: one branch per station — `FORK "a", "b"` with same-verb candidates:
/// only the first opens (list order decides), one window.
#[test]
fn a_fork_opens_one_branch_per_station() {
    let dict = run_dict();
    let (c, s) = compile(
        &dict,
        &rig(
            &dream(
                "      parent when count(actors) >= 1 -> FORK \"a\", \"b\"\n      a when count(actors) >= 1 -> NARRATE \"a\"\n      b when count(actors) >= 1 -> NARRATE \"b\"",
            ),
            None,
        ),
    );
    let base = clone_state(&s);
    let r = settle_line(&c, &dict, s, "0:0 @ ANA DREAM", 1).unwrap();
    assert_eq!(r.rejected, None);
    assert_eq!(refusal(&r), None, "the second branch skips silently");
    assert_eq!(kind_spine(&r), ["action", "complete", "action", "complete", "wait"]);
    assert_eq!(begin_recipes(&r), [vec!["parent".to_string()], vec!["a".to_string()]]);
    assert_eq!(executed_entries(&r.state), vec![("parent".to_string(), 1), ("a".to_string(), 1)]);
    let replayed = fold(&r.records, &base, &dict, &c).unwrap();
    assert_eq!(snap(&replayed), snap(&r.state));
}

/// F2-5: duplicate ids — `FORK "a", "a"` opens one window.
#[test]
fn a_fork_with_duplicate_ids_opens_one_window() {
    let dict = run_dict();
    let (c, s) = compile(
        &dict,
        &rig(
            &dream(
                "      parent when count(actors) >= 1 -> FORK \"a\", \"a\"\n      a when count(actors) >= 1 -> NARRATE \"a\"",
            ),
            None,
        ),
    );
    let base = clone_state(&s);
    let r = settle_line(&c, &dict, s, "0:0 @ ANA DREAM", 1).unwrap();
    assert_eq!(r.rejected, None);
    assert_eq!(refusal(&r), None);
    assert_eq!(kind_spine(&r), ["action", "complete", "action", "complete", "wait"]);
    assert_eq!(begin_recipes(&r), [vec!["parent".to_string()], vec!["a".to_string()]]);
    assert_eq!(r.records[1].id + 1, r.records[2].id, "exactly one BEGIN follows");
    let replayed = fold(&r.records, &base, &dict, &c).unwrap();
    assert_eq!(snap(&replayed), snap(&r.state));
}

/// F2-6: the busy check per branch — one branch's verb holds a foreign open
/// window (committed in the same step's other clause): that branch skips,
/// the other opens.
#[test]
fn a_fork_skips_a_busy_branch_and_opens_the_free_one() {
    let dict = run_dict();
    let (c, s) = compile(
        &dict,
        &rig_full(
            "  DREAM:\n    actors: 1 kind:character\n    duration: 0m\n    recipes:\n      parent when count(actors) >= 1 -> FORK \"study.busy\", \"wait.free\"\n  STUDY:\n    actors: 1 kind:character\n    duration: 1h\n    recipes:\n      study.busy when count(actors) >= 1 -> NARRATE \"busy\"\n  WAIT:\n    actors: 1 kind:character\n    duration: 1h\n    recipes:\n      wait.free when count(actors) >= 1 -> NARRATE \"free\"",
            None,
            Some("  BERTHA is character at HOUSE"),
        ),
    );
    let base = clone_state(&s);
    // ANA cannot fill both clauses; BERTHA holds STUDY while ANA dreams —
    // the STUDY branch skips (foreign window), the WAIT branch opens
    let r = settle_line(&c, &dict, s, "0:0 @ ANA DREAM ; BERTHA STUDY", 1).unwrap();
    assert_eq!(r.rejected, None);
    assert_eq!(refusal(&r), None);
    assert_eq!(
        begin_recipes(&r),
        [vec!["parent".to_string(), "study.busy".to_string()], vec!["wait.free".to_string()]]
    );
    assert_eq!(r.state.executed.get("study.busy"), Some(&1), "no second STUDY window");
    assert_eq!(r.state.executed.get("wait.free"), Some(&1));
    let replayed = fold(&r.records, &base, &dict, &c).unwrap();
    assert_eq!(snap(&replayed), snap(&r.state));
}

/// F2-7: the partial fork — `FORK "spent", "fresh"` with `spent max 1` spent
/// (counted by a prior turn): `fresh` opens alone; the all-whiff variant
/// opens nothing, refuses nothing, and the slot stays free (a following RUN
/// pins — fall-through composition).
#[test]
fn a_fork_partially_pins_and_an_all_whiff_fork_composes_with_a_run() {
    let dict = run_dict();
    let (c, s) = compile(
        &dict,
        &rig(
            &dream(
                "      starter max 1 when count(actors) >= 1 -> RUN \"spent\"\n      parent when count(actors) >= 1 -> FORK \"spent\", \"fresh\"\n      spent max 1 when count(actors) >= 1 -> NARRATE \"spent\"\n      fresh when count(actors) >= 1 -> NARRATE \"fresh\"",
            ),
            None,
        ),
    );
    // play 1: the starter pins `spent` cleanly (its count lands)
    let r1 = settle_line(&c, &dict, s, "0:0 @ ANA DREAM", 1).unwrap();
    assert_eq!(r1.rejected, None);
    assert_eq!(begin_recipes(&r1), [vec!["starter".to_string()], vec!["spent".to_string()]]);
    // play 2: `spent` is exhausted — the fork's branch skips, `fresh` opens
    let r2 = settle_line(&c, &dict, r1.state, "0:0 @ ANA DREAM", 100).unwrap();
    assert_eq!(r2.rejected, None);
    assert_eq!(refusal(&r2), None);
    assert_eq!(begin_recipes(&r2), [vec!["parent".to_string()], vec!["fresh".to_string()]]);
    // the all-whiff variant: both candidates gated off — nothing opens, no
    // refusal, and a following RUN composes (the slot stayed free)
    let (c2, s2) = compile(
        &dict,
        &rig(
            &dream(
                "      parent when count(actors) >= 1 -> FORK \"whiff1\", \"whiff2\", RUN \"here\"\n      whiff1 when actors.aspect.mood >= 99 -> NARRATE \"w1\"\n      whiff2 when actors.aspect.mood >= 98 -> NARRATE \"w2\"\n      here when count(actors) >= 1 -> NARRATE \"here\"",
            ),
            None,
        ),
    );
    let base2 = clone_state(&s2);
    let p1 = settle_line(&c2, &dict, s2, "0:0 @ ANA DREAM", 300).unwrap();
    assert_eq!(p1.rejected, None);
    assert_eq!(refusal(&p1), None);
    assert_eq!(kind_spine(&p1), ["action", "complete", "action", "complete", "wait"]);
    assert_eq!(begin_recipes(&p1), [vec!["parent".to_string()], vec!["here".to_string()]]);
    let replayed = fold(&p1.records, &base2, &dict, &c2).unwrap();
    assert_eq!(snap(&replayed), snap(&p1.state));
}

/// F2-8: the atomic inline gate — the FIRST pinned branch's top-level REFUSE
/// rejects the PARENT window and NO branch opens; the trials' rng/mint
/// consumption stays scratch-only.
#[test]
fn a_fork_whose_first_branch_gate_refuses_opens_nothing() {
    let dict = run_dict();
    let (c, s) = compile(
        &dict,
        &rig(
            &dream(
                "      parent when count(actors) >= 1 -> FORK \"gated\", \"innocent\"\n      gated when count(actors) >= 1 -> ROLL(2, 6), SPAWN TRINKET, REFUSE(\"the gate says no\")\n      innocent when count(actors) >= 1 -> NARRATE \"innocent\"",
            ),
            None,
        ),
    );
    let base = clone_state(&s);
    assert_eq!(s.rng, 0);
    let r = settle_line(&c, &dict, s, "0:0 @ ANA DREAM", 1).unwrap();
    assert_eq!(r.rejected, None);
    assert_eq!(refusal(&r), Some("the gate says no".to_string()));
    assert_eq!(kind_spine(&r), ["action", "complete", "wait"], "no branch opened");
    let ops: Vec<&str> = done(&r).unwrap().outcomes.iter().map(|o| o.op.as_str()).collect();
    assert_eq!(ops, ["REFUSE"]);
    assert_eq!(executed_entries(&r.state), vec![("parent".to_string(), 1)]);
    assert!(r.state.windows.is_empty());
    assert_eq!(r.state.rng, 0, "the trial's ROLL never touched the real counter");
    assert!(!r.state.entities.values().any(|e| e.name == "TRINKET"));
    let replayed = fold(&r.records, &base, &dict, &c).unwrap();
    assert_eq!(snap(&replayed), snap(&r.state));
}

/// F2-9: the livelock fan-out — a mutual zero-duration fork cycle multiplies
/// through the 1024 budget exponentially and trips it: the step is rejected
/// loud, nothing logs, the original state rides back.
#[test]
fn a_zero_duration_fork_cycle_trips_the_livelock_budget() {
    let dict = run_dict();
    let (c, s) = compile(
        &dict,
        &rig(
            "  DREAM:\n    actors: 1 kind:character\n    duration: 0m\n    recipes:\n      dream.ping when count(actors) >= 1 -> NARRATE \"ping\", FORK \"wait.pong_a\", \"wait.pong_b\"\n  WAIT:\n    actors: 1 kind:character\n    duration: 0m\n    recipes:\n      wait.pong_a when count(actors) >= 1 -> FORK \"dream.ping\"\n      wait.pong_b when count(actors) >= 1 -> FORK \"dream.ping\"",
            None,
        ),
    );
    let original = clone_state(&s);
    let r = settle_line(&c, &dict, s, "0:0 @ ANA DREAM", 1).unwrap();
    let rejected = r
        .rejected
        .clone()
        .unwrap_or_else(|| panic!("expected the livelock rejection, got {:?}", r.rejected));
    assert!(
        rejected.contains("livelock guard")
            && rejected.contains("1024")
            && rejected.contains("§6.7"),
        "{rejected}"
    );
    assert!(r.records.is_empty(), "the rejected pass logs nothing");
    assert_eq!(snap(&r.state), snap(&original), "the original state rides back");
}

/// F2-11: the multi-role projection per branch — object/with/at bound, both
/// branches carry the projected clause, and the play-inserted and
/// fold-replayed windows land identical `OpenWindow`s.
#[test]
fn a_fork_projects_the_parent_refs_onto_every_branch() {
    let dict = run_dict();
    let (c, s) = compile(
        &dict,
        &rig_full(
            "  DREAM:\n    actors: 1 kind:character\n    object: 1 kind:text\n    with: 1 kind:item\n    at: 1 kind:place\n    duration: 1h\n    recipes:\n      parent when count(actors) >= 1 -> NARRATE \"parent\", FORK \"wait.child\", \"study.child\"\n  WAIT:\n    actors: 1 kind:character\n    object: 0..1 kind:text\n    duration: 2h\n    recipes:\n      wait.child when count(actors) >= 1 AND count(object) >= 1 -> NARRATE \"wait child\"\n  STUDY:\n    actors: 1 kind:character\n    object: 0..1 kind:text\n    duration: 2h\n    recipes:\n      study.child when count(actors) >= 1 AND count(object) >= 1 -> NARRATE \"study child\"",
            None,
            Some("  MAGICK_BOOK is text at HOUSE\n  CHARM is item at HOUSE"),
        ),
    );
    let base = clone_state(&s);
    let r =
        settle_line(&c, &dict, s, "0:0 @ ANA AT HOUSE DREAM MAGICK_BOOK WITH CHARM", 1).unwrap();
    assert_eq!(r.rejected, None);
    assert_eq!(
        begin_recipes(&r),
        [
            vec!["parent".to_string()],
            vec!["wait.child".to_string()],
            vec!["study.child".to_string()]
        ]
    );
    // both branch BEGINs carry the projected object role (and the actor)
    for i in [2usize, 3] {
        assert_eq!(r.records[i].objects.len(), 1, "object rides branch {i}");
        assert_eq!(r.records[i].actors.len(), 1);
        assert!(r.records[i].instruments.is_empty(), "with is projected away");
    }
    // the two branch windows stay open (2h) — structurally equal on fold
    assert_eq!(r.state.windows.len(), 2);
    let replayed = fold(&r.records, &base, &dict, &c).unwrap();
    assert_eq!(replayed.windows, r.state.windows, "identical OpenWindows");
    assert_eq!(snap(&replayed), snap(&r.state));
}
