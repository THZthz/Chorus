//! The §6.6/§6.7 chain-model faces the Wave-3 fixes closed (v0.9.7) — one
//! divergent-shape pin per fix, each repro a shape the reach-fuzz campaign
//! surfaced:
//!
//! - R1 (the selection shadow, seed 70's class): a when-less recipe never
//!   witnesses a completion trigger while an earlier sibling's `when` is
//!   decidably true — §6.4 first-match would select the sibling, and the
//!   fix's hoisted rule demands EVERY earlier `when` decidably False;
//! - R2 (the chain-slot refusal, seeds 103/170's class): a pin-capable
//!   RUN/FORK followed by another chain op refuses the parent window, so
//!   the whole list's mints demote at the new ChainPinRefusal rung;
//! - R3 (the chain-pinned dead verb, seed 84's class): a slot-dead verb
//!   some FORK/RUN targets completes over EMPTY bindings (the chain path
//!   synthesizes the follow-up and skips validate_slots), so the verb is
//!   exempt from the dead-verb demotion and its mints stay candidates;
//! - R4 (the mint marks manifested): a proven in-play SPAWN of a unique
//!   name marks it manifested — the once draw's serve skips the dead
//!   position at no cost and serves the next position.
//!
//! The Wave-4 cross-list faces (v0.9.8) — one pin per channel of the
//! census's interference map, each repro a shape the campaign window
//! surfaced:
//!
//! - W4A (seed 291's shape, channel a): a pinned candidate whose list
//!   carries a top-level REFUSE refuses the PARENT window — the inline gate
//!   (`run_inline_gate`) trials it on the completion draft — so the
//!   completing list's mints die with the scratch (ChainSlotInterference);
//! - W4G (channel c, no current seed): two sibling `on complete` bodies on
//!   the same home share the completion's ONE chain slot — the second pin
//!   refuses the parent window (ChainSlotInterference);
//! - W4E (seed 851's shape): the ambient gate's window-bound sum reads
//!   EXACTLY 0 over the §6.9 sweep's empty bindings — not Unknown through a
//!   chain-pinned verb's nulled window model — so the gated recipe never
//!   opens (WhenNotStaticTrue).

use chorus::content::script::compile_script;
use chorus::content::CompiledContent;
use chorus::core::clock::parse_clock;
use chorus::core::dict::core_dictionary;
use chorus::core::sentence::Arg;
use chorus::core::types::State;
use chorus::engine::{execute_step, ExecOptions, Step};
use chorus::reach::fixpoint::{CardBlocker, SiteOp};
use chorus::reach::{analyze_card, Analyzer, CardOptions, CardVerdict, Existence};

use crate::common::rig::settle_line;

/// Compile a script fixture against a fresh core dictionary.
fn compile(text: &str) -> (CompiledContent, State) {
    let dict = core_dictionary();
    let compiled = compile_script(text, &dict).expect("the fixture compiles");
    (compiled.content, compiled.state)
}

fn analyze(content: &CompiledContent, state: &State, target: &str) -> chorus::reach::CardReport {
    let dict = core_dictionary();
    analyze_card(content, state, &dict, target, CardOptions::default()).unwrap()
}

// ---------------------------------------------------------------- R1: seed 70's class ----

#[test]
fn a_when_less_completion_witness_is_never_selected_while_an_earlier_recipe_gates_static_true() {
    // LEDGER holds b:2, so talk.r0's `total(table, aspect:b) >= 2` gate is
    // static-true over every world where the completion locks LEDGER — §6.4
    // first-match selects talk.r0 on EVERY completion and the when-less
    // talk.r1 never runs. The pre-fix model offered (TALK, talk.r1) as the
    // trigger's witness (a when-less witness recipe skipped the
    // earlier-sibling test entirely) and proved P3 obtainable; at play
    // talk.r0 completes, its SPAWN P2 lands on the draft, and the on
    // complete body's SPAWN P2 collides with it — the window REFUSES the
    // whole scratch, so P2 AND P3 never land. The hoisted rule demotes
    // talk.r1's pair; talk.r0's own pair dies at the draft rung (the
    // completion-contamination and per-pair draft rungs were already
    // sound).
    let text = "\
script: R1 Selection Shadow
player: ANA
start: 0:0
seed: 0

entities:
  ANA is character
  LEDGER is text with b:2 on complete -> (SPAWN P2, SPAWN P3)

prototypes:
  P2 is text
  P3 is idea

verbs:
  TALK:
    actors: 1 kind:character
    with: 1 aspect:b>=1
    duration: 0m
    recipes:
      talk.r0 when total(table, aspect:b) >= 2 -> SPAWN P2
      talk.r1 -> NARRATE \"r1\"
";
    let (content, state) = compile(text);
    // P3: never obtainable — the trigger site stays live and unproven, so
    // the honest tier is candidate
    let p3 = analyze(&content, &state, "P3");
    assert_eq!(p3.verdict, CardVerdict::Unproven);
    assert_eq!(p3.existence, Existence::Candidate);
    // P2 never lands at play either (the refused scratch) — the analyzer
    // agrees through the completion-contamination rung on talk.r0's own
    // mint and the draft rung on the trigger's
    let p2 = analyze(&content, &state, "P2");
    assert_ne!(p2.verdict, CardVerdict::Obtainable, "the window refuses: P2 never lands");
    // the trigger-site blocker is on the --why table
    let dict = core_dictionary();
    let analyzer = Analyzer::new(&content, &state, &dict);
    let rows = analyzer.card_blockers("P3");
    assert_eq!(rows.len(), 1, "exactly the trigger's SPAWN P3 site serves P3");
    assert_eq!(rows[0].owner, "trigger complete:LEDGER");
    assert_eq!(rows[0].op, SiteOp::Spawn);
    assert_eq!(rows[0].name, "P3");
    assert!(
        matches!(rows[0].blocker, CardBlocker::CompleteDraftCollision { ref name } if name == "P2"),
        "the sole surviving witness pair (talk.r0) is draft-dirty: {}",
        rows[0].blocker.as_str()
    );
}

// ------------------------------------------------------------ R2: seeds 103/170's class ----

#[test]
fn a_second_chain_op_after_a_pinning_one_refuses_the_parent_window_and_demotes_the_list() {
    // work.r0's list is `SPAWN P2, FORK "work.r1", RUN "work.r1"`: the FORK
    // pins (the default candidate is when-less), the RUN then hits the
    // taken chain slot FIRST and refuses the PARENT window (§6.7 — a
    // station holds one window), so the SPAWN P2 dies with the whole
    // completion scratch and P2 never exists. The census records the
    // list-level refusal; the ladder's ChainPinRefusal rung (after
    // ThrowOps) demotes the list's every live site.
    let text = "\
script: R2 Chain Pin Refusal
player: ANA
start: 0:0
seed: 0

entities:
  ANA is character

prototypes:
  P2 is text

verbs:
  WORK:
    actors: 1 kind:character
    duration: 0m
    recipes:
      work.r0 -> SPAWN P2, FORK \"work.r1\", RUN \"work.r1\"
      work.r1 -> NARRATE \"branch\"
";
    let (content, state) = compile(text);
    let p2 = analyze(&content, &state, "P2");
    assert_eq!(p2.verdict, CardVerdict::Unproven);
    assert_eq!(p2.existence, Existence::Candidate);
    let dict = core_dictionary();
    let analyzer = Analyzer::new(&content, &state, &dict);
    let rows = analyzer.card_blockers("P2");
    assert_eq!(rows.len(), 1, "exactly work.r0's SPAWN site serves P2");
    assert_eq!(rows[0].owner, "recipe work.r0 of WORK");
    assert_eq!(rows[0].op, SiteOp::Spawn);
    assert_eq!(rows[0].name, "P2");
    assert_eq!(rows[0].blocker, CardBlocker::ChainPinRefusal);
}

// ------------------------------------------------------------ R3: seed 84's class ----

#[test]
fn a_chain_pinned_slot_dead_verb_completes_over_empty_bindings_so_its_mints_stay_candidates() {
    // DREAM's `with` slot (`kind:item aspect:b>=1`) can never match any
    // descriptor (no item-kind card exists or is mintable), so the
    // player-commit path would call DREAM dead — but the hour-1 pulse FORKs
    // dream.r1: the event-channel pin skips validate_slots entirely and the
    // follow-up completes over EMPTY bindings, landing SPAWN P2. The
    // dead-verb rule models the player-commit path only, so chain-pinned
    // verbs are exempt: P2 stays a candidate, never unobtainable — while
    // play proves the mint (the engine face below).
    let text = "\
script: R3 Chain Fed Dead Verb
player: ANA
start: 0:0
seed: 0

entities:
  ANA is character
  LEDGER is resource with b:2

prototypes:
  P2 is text

triggers:
  pulse at hour 1 -> FORK \"dream.r1\"

verbs:
  WAIT:
    actors: 1 kind:character
    duration: 0m
    recipes:
      idle -> NARRATE \"beat\"
  DREAM:
    actors: 1 kind:character
    with: 1 kind:item aspect:b>=1
    duration: 30m
    recipes:
      dream.r1 when count(name:ANA) >= 1 -> IF exists(LEDGER) -> (SPAWN P2)
";
    let (content, state) = compile(text);
    let p2 = analyze(&content, &state, "P2");
    assert_eq!(p2.verdict, CardVerdict::Unproven);
    assert_eq!(p2.existence, Existence::Candidate);
    // the --why table keeps the site visible with its honest blocker
    // (DREAM is uncommittable on the player path — the chain path is the
    // exempted one)
    let dict = core_dictionary();
    let analyzer = Analyzer::new(&content, &state, &dict);
    let rows = analyzer.card_blockers("P2");
    assert_eq!(rows.len(), 1, "exactly dream.r1's SPAWN site serves P2");
    assert_eq!(rows[0].owner, "recipe dream.r1 of DREAM");
    assert_eq!(rows[0].blocker, CardBlocker::VerbNotCommittable { verb: "DREAM".to_string() });

    // the engine's face: the 1:0 crossing FORKs dream.r1 over empty
    // bindings and the 30m follow-up completes, landing P2
    let dict = core_dictionary();
    let r1 = settle_line(&content, &dict, state, "0:0 @ ANA WAIT", 1).unwrap();
    assert_eq!(r1.rejected, None);
    let r2 = execute_step(
        &content,
        &dict,
        r1.state,
        Step {
            start_min: None,
            clock: Some(parse_clock("1:0").unwrap()),
            clauses: vec![],
            t_real: None,
        },
        ExecOptions { first_record_id: r1.records.len() as i64 + 1, id_gen: None },
    )
    .unwrap();
    assert_eq!(r2.rejected, None);
    let r3 = execute_step(
        &content,
        &dict,
        r2.state,
        Step {
            start_min: None,
            clock: Some(parse_clock("1:30").unwrap()),
            clauses: vec![],
            t_real: None,
        },
        ExecOptions {
            first_record_id: (r1.records.len() + r2.records.len()) as i64 + 1,
            id_gen: None,
        },
    )
    .unwrap();
    assert_eq!(r3.rejected, None);
    assert_eq!(
        r3.state.by_name.get("P2").map(Vec::len),
        Some(1),
        "the chain-fed completion lands P2 over empty bindings"
    );
}

#[test]
fn without_the_chain_feed_the_slot_dead_verb_stays_dead_and_its_mint_stays_unobtainable() {
    // the control: delete the trigger and DREAM is chain-pinned by nobody —
    // the dead-verb demotion applies again (the player path is the only
    // path, and it validates slots), so P2 is structurally absent.
    let text = "\
script: R3 Control
player: ANA
start: 0:0
seed: 0

entities:
  ANA is character
  LEDGER is resource with b:2

prototypes:
  P2 is text

verbs:
  WAIT:
    actors: 1 kind:character
    duration: 0m
    recipes:
      idle -> NARRATE \"beat\"
  DREAM:
    actors: 1 kind:character
    with: 1 kind:item aspect:b>=1
    duration: 30m
    recipes:
      dream.r1 when count(name:ANA) >= 1 -> IF exists(LEDGER) -> (SPAWN P2)
";
    let (content, state) = compile(text);
    let p2 = analyze(&content, &state, "P2");
    assert_eq!(p2.verdict, CardVerdict::Unobtainable);
    assert_eq!(p2.existence, Existence::None);
}

// ---------------------------------------------------------------- R4: the mint marks manifested ----

#[test]
fn a_proven_in_play_mint_marks_its_unique_manifested_so_the_once_draw_serves_the_next_position() {
    // the composition behind the rewritten split pin (reach_unique.rs): the
    // WAIT mint proves and marks U manifested (the engine's mint face
    // records the name permanently), so the once trigger's single serve
    // skips U's dead position 0 at no serve cost and serves B — the
    // analyzer agrees with play on BOTH faces (the player-mint face; the
    // ambient face rides the count-clamp fixture in reach_unique.rs).
    let text = "\
script: R4 Mint Marks Manifested
player: ANA
start: 0:0
seed: 0

entities:
  ANA is character

prototypes:
  U is idea unique
  B is idea stackable

decks:
  QUEST spec U, B

triggers:
  grant once at hour 1 -> DRAW QUEST

verbs:
  WAIT:
    actors: 1 kind:character
    duration: 0m
    recipes:
      mint -> SPAWN U
";
    let (content, state) = compile(text);
    let u = analyze(&content, &state, "U");
    assert_eq!(u.verdict, CardVerdict::Obtainable);
    assert_eq!(u.existence, Existence::Proven);
    let b = analyze(&content, &state, "B");
    assert_eq!(
        b.verdict,
        CardVerdict::Obtainable,
        "the minted-unique position is dead — the once draw's serve skips it and serves B"
    );
    assert_eq!(b.existence, Existence::Proven);
}

// ------------------------------------------- W4A: seed 291's shape (channel a) ----

#[test]
fn a_pin_whose_candidate_carries_a_top_level_refuse_refuses_the_parent_completion_window() {
    // The ambient USE_ON completion parks its RUN (§6.7's deferral), its own
    // REPLACE/SPAWN land P1 on the draft, and the parked op then pins
    // commune.r1 — whose `when` reads the draft's P1 — and the inline gate
    // trials commune.r1's TOP-LEVEL REFUSE on that draft: the refusal
    // rejects the PARENT window and the whole scratch dies, so P1 never
    // lands (every window, forever). The census's per-list REFUSE model
    // never sees it — the REFUSE lives on ANOTHER list — so the
    // interference map joins it: any pin in the completion's shared chain
    // channel naming a gate-refusing candidate demotes the completing
    // list's mints at the ChainSlotInterference rung.
    let text = "\
script: W4A Pinned Gate Refusal
player: ANA
start: 0:0
seed: 0

entities:
  ANA is character
  LEDGER is text with a:3

prototypes:
  P1 is idea stackable

verbs:
  COMMUNE:
    actors: 1 kind:character
    duration: 30m
    recipes:
      commune.r1 when count(name:P1) >= 1 -> SPAWN P1, REFUSE(\"the gate refuses\")

  USE_ON:
    ambient
    duration: 30m
    recipes:
      use_on.r0 -> RUN \"commune.r1\", REPLACE LEDGER WITH P1, SPAWN P1
";
    let (content, state) = compile(text);
    let p1 = analyze(&content, &state, "P1");
    assert_eq!(p1.verdict, CardVerdict::Unproven);
    assert_eq!(p1.existence, Existence::Candidate);
    let dict = core_dictionary();
    let analyzer = Analyzer::new(&content, &state, &dict);
    let rows = analyzer.card_blockers("P1");
    let replace =
        rows.iter().find(|r| r.op == SiteOp::Replace).expect("the REPLACE site serves P1");
    assert_eq!(replace.owner, "recipe use_on.r0 of USE_ON");
    assert_eq!(replace.blocker, CardBlocker::ChainSlotInterference);

    // the engine's face: every ambient completion journals the gate's
    // recurring REFUSE and P1 never lands
    let r1 = execute_step(
        &content,
        &dict,
        state,
        Step {
            start_min: None,
            clock: Some(parse_clock("1:30").unwrap()),
            clauses: vec![],
            t_real: None,
        },
        ExecOptions { first_record_id: 1, id_gen: None },
    )
    .unwrap();
    assert_eq!(r1.rejected, None);
    assert!(
        r1.state.by_name.get("P1").is_none_or(|pool| pool.is_empty()),
        "the gate refusal kills every window: P1 never lands"
    );
    let gate_refuses = r1
        .records
        .iter()
        .filter(|rec| rec.kind == "complete" && rec.verb == "USE_ON")
        .flat_map(|rec| &rec.outcomes)
        .any(|o| {
            o.op == "REFUSE"
                && matches!(o.args.first(), Some(Arg::Str { v }) if v == "the gate refuses")
        });
    assert!(gate_refuses, "the COMPLETE rows journal the inline gate's REFUSE");
}

// --------------------------------------------- W4G: channel c, sibling bodies ----

#[test]
fn two_sibling_completion_bodies_pin_each_other_so_the_completing_recipe_mints_die() {
    // LEDGER carries TWO `on complete` bodies, each chaining a RUN: the
    // completion's ONE chain slot takes the first body's pin, and the
    // second body's pin hits the taken slot FIRST — the parent window
    // refuses and the completing recipe's SPAWN P1 dies with the draft.
    // The analyzer's per-list chain refusal sees neither list misbehaving
    // (talk.r0 and talk.r1 each carry a single chain op); the interference
    // map's per-completion join counts the pins ACROSS the firing bodies.
    let text = "\
script: W4G Sibling Bodies Pin And Refuse Each Other
player: ANA
start: 0:0
seed: 0

entities:
  ANA is character
  LEDGER is text with a:1 on complete -> (RUN \"talk.r0\") on complete -> (RUN \"talk.r1\")

prototypes:
  P1 is idea

verbs:
  TALK:
    actors: 1 kind:character
    with: 1 aspect:a>=1
    duration: 0m
    recipes:
      talk.r0 -> SPAWN P1
      talk.r1 -> NARRATE \"beat\"
      talk.r2 -> NARRATE \"beat\"
";
    let (content, state) = compile(text);
    let p1 = analyze(&content, &state, "P1");
    assert_eq!(p1.verdict, CardVerdict::Unproven);
    assert_eq!(p1.existence, Existence::Candidate);
    let dict = core_dictionary();
    let analyzer = Analyzer::new(&content, &state, &dict);
    let rows = analyzer.card_blockers("P1");
    assert_eq!(rows.len(), 1, "exactly talk.r0's SPAWN site serves P1");
    assert_eq!(rows[0].owner, "recipe talk.r0 of TALK");
    assert_eq!(rows[0].blocker, CardBlocker::ChainSlotInterference);

    // the engine's face: the window refuses — P1 never lands, and the
    // COMPLETE row journals the second-pin refusal
    let r1 = settle_line(&content, &dict, state, "0:0 @ ANA TALK WITH LEDGER", 1).unwrap();
    assert_eq!(r1.rejected, None);
    assert!(
        r1.state.by_name.get("P1").is_none_or(|pool| pool.is_empty()),
        "the second sibling pin refuses the window: P1 never lands"
    );
    assert!(
        r1.records
            .iter()
            .any(|rec| rec.kind == "complete" && rec.outcomes.iter().any(|o| o.op == "REFUSE")),
        "the COMPLETE row journals the chain-slot refusal"
    );
}

// ----------------------------------------------- W4E: seed 851's shape, fix D ----

#[test]
fn the_ambient_gate_window_bound_sum_reads_exactly_zero_over_the_sweep_empty_bindings() {
    // t0's RUN makes AMB chain-pinned, so its window model is nulled — and
    // the pre-fix ambient gate read `total(bound, aspect:a)` through that
    // nulled model as Unknown, which HOLDS, so amb.r0 stayed selectable and
    // P1 proved. The §6.9 sweep selects over EMPTY bindings where the
    // window-bound sum reads exactly 0: amb.r0 NEVER selects (0 >= 1 is
    // false — the exact runtime face), the deep exploration never sees P1,
    // and the gate's decided-False demotes.
    let text = "\
script: W4E Ambient Window Sum Through The Nulled Window Model
player: ANA
start: 0:0
seed: 0

entities:
  ANA is character

prototypes:
  P1 is idea

verbs:
  AMB:
    ambient
    duration: 30m
    recipes:
      amb.r0 when total(bound, aspect:a) >= 1 -> SPAWN P1

triggers:
  t0 once at hour 1 -> RUN \"amb.r0\"
";
    let (content, state) = compile(text);
    let p1 = analyze(&content, &state, "P1");
    assert_eq!(p1.verdict, CardVerdict::Unproven);
    assert_eq!(p1.existence, Existence::Candidate);
    let dict = core_dictionary();
    let analyzer = Analyzer::new(&content, &state, &dict);
    let rows = analyzer.card_blockers("P1");
    assert_eq!(rows.len(), 1, "exactly amb.r0's SPAWN site serves P1");
    assert_eq!(rows[0].owner, "recipe amb.r0 of AMB");
    assert_eq!(rows[0].blocker, CardBlocker::WhenNotStaticTrue);

    // the engine's face: the sweep never opens AMB (the gate reads 0 >= 1
    // over the empty bindings) and the t0 chain never pins it — P1 never
    // exists
    let r1 = execute_step(
        &content,
        &dict,
        state,
        Step {
            start_min: None,
            clock: Some(parse_clock("2:0").unwrap()),
            clauses: vec![],
            t_real: None,
        },
        ExecOptions { first_record_id: 1, id_gen: None },
    )
    .unwrap();
    assert_eq!(r1.rejected, None);
    assert!(r1.state.by_name.get("P1").is_none_or(|pool| pool.is_empty()));
    assert!(
        !r1.records.iter().any(|rec| rec.kind == "begin" && rec.verb == "AMB"),
        "the sweep never opens the gated ambient recipe"
    );
}
