//! Invariants T1-T14 (kernel-api.md §9), each with an executable assertion here or a forwarding comment naming the owning test.

use super::*;

// t1 — totality: absent declared keys, dead unique cards, empty scopes,
// unbound slot scopes, and runtime zero division all read 0. (The
// unbound-roll clause is authoring-unreachable: a roll reference outside
// a binding list is the E244 refusal pinned by matrix_27_roll_scope_.)
#[test]
fn t1_totality_reads_are_total() {
    let src = r#"
game "Total"
  seed host
  turn 1m
  home hall

zones
  hall

aspects
  ghost: 0
  charge: 0

cards
  card beacon
    unique
    start in hall

  card slain
    unique
    start in hall

  card coin
    stackable
    start in hall

verbs
  verb prober committable instances 1
    recipe probe
      duration 0
      slots
        never: 0..1 aspect:ghost
      effects
        destroy slain
        narrate "a{slain.ghost}"
        narrate "b{beacon.ghost}"
        if count(interior, aspect:ghost) == 0
          narrate "c0"
        if count(slot:never, aspect:ghost) == 0
          narrate "d0"
        set beacon.charge += 5 / count(world, aspect:ghost)
        narrate "f{beacon.charge}"
        if alive(slain) == 0
          narrate "g0"
"#;
    let mut e = game(src);
    let events = common::commit_named(&mut e, "prober", &[]).expect("probes");
    assert_eq!(
        narration(&events),
        vec![
            "a{slain.ghost}",
            "b{beacon.ghost}",
            "c0",
            "d0",
            "f{beacon.charge}",
            "g0"
        ],
        "the cues are recorded for the presentation layer"
    );
    assert_eq!(aspect(&e, common::named(&e, "beacon"), "charge"), 0);
}

// t2 — constant zero divisor refused at compile time [E242]. Compile-only
// face; a constant zero divisor cannot reach the kernel. Owned by
// story-lang: features::compile_time_invariant_t2_zero_divisor_refused,
// diagnostics::e242_constant_zero_divisor, and
// diagnostics::e242_constant_zero_divisor_folded. The runtime face of
// division totality (x / 0 == 0) is asserted by t1 above.

// t3 — commit atomicity: any refusal leaves the state bit-identical and
// spends zero time. Owned by kernel::atomicity::commit_effect_failure_
// restores_bit_for_bit and emberwatch::determinism_same_seed_same_ops_
// identical_state; the GuardRefused face is asserted by
// matrix_16_guard_refused_pos; DeckExhausted by matrix_10_fallback_neg;
// NameInUse by matrix_07_destroy_swap_neg and matrix_11_unique_mint_skip_neg.

// t4 — per-unit atomicity on the advance path: the unit checkpoint
// includes the RNG counter and deck cursors.
#[test]
fn t4_advance_rollback_restores_rng_and_deck_cursors() {
    let src = r#"
game "Cursed"
  seed host
  turn 1m
  home hall

zones
  hall

aspects
  spark: 0
  ghost: 0

cards
  card coin
    stackable
    aspects spark 1

decks
  deck pouch
    entries coin
    wrap

verbs
  verb risky committable instances 1
    recipe gamble
      duration 10m
      effects
        draw from pouch
        roll 1d6 as fate
        consume aspect:ghost 1
"#;
    let mut e = game(src);
    common::commit_named(&mut e, "risky", &[]).expect("opens");
    let (cursor, rng) = (
        *e.state().decks.values().next().expect("the deck exists"),
        e.state().rng_counter,
    );
    assert_eq!((cursor, rng), (0, 0));
    let events = e
        .advance(10)
        .expect("the unit fails; the advance continues");
    assert!(events
        .iter()
        .any(|ev| matches!(ev, Event::ResolutionFailed { .. })));
    assert_eq!(
        *e.state().decks.values().next().expect("the deck exists"),
        cursor,
        "deck cursor"
    );
    assert_eq!(e.state().rng_counter, rng, "rng counter");
    assert_eq!(count_named(&e, "coin"), 0, "the drawn card rolled back");
    assert_eq!(situations(&e).len(), 0, "the situation aborted");
    assert_eq!(e.state().now, 10, "the loop moved on to the target");
}

// t5 — livelock: either budget breach rolls the whole call back to its
// entry checkpoint. The commit-path face is owned by
// kernel::instant::instant_chain_over_budget_livelocks_and_rolls_back
// (depth cap) and matrix_22_recursion_budget_neg (unit cap); this is the
// advance-path face.
#[test]
fn t5_livelock_rolls_back_the_whole_advance() {
    let src = r#"
game "Swarm"
  seed host
  turn 1m
  home hall

zones
  hall

verbs
  verb caller committable instances 1
    recipe go
      duration 10m
      effects
        start swarm

  verb swarm uncommittable instances any
    recipe split
      chain-only
      duration 0
      effects
        start swarm count 2
"#;
    let mut e = game(src);
    common::commit_named(&mut e, "caller", &[]).expect("opens");
    let before = state_bytes(&e);
    let now = e.state().now;
    assert_eq!(err_kind(e.advance(10)), DiagKind::Livelock);
    assert_eq!(state_bytes(&e), before, "the whole advance rolled back");
    assert_eq!(e.state().now, now);
    assert_eq!(situations(&e).len(), 1, "the caller's instance survived");
}

// t6 — determinism. Owned by
// emberwatch::determinism_same_seed_same_ops_identical_state (same
// content + seed + ops across advance splits, and the bit-restored retry
// after a refusal).

// t7 — endings discard the queue, refuse every entry point, and undo
// resumes. Owned by emberwatch::ending_closes_the_run_until_restored and
// emberwatch::scripted_session_reaches_survived.

// t8 — locks are total. Owned by
// kernel::atomicity::locks_are_total_across_situations; the mid-run bind
// face (EntityLocked through `bind`) is asserted by
// matrix_31_bind_happy_path_neg.

// t9 — context-free scopes: expiry books and event books read `bound`,
// `interior`, and `slot:` as empty. The expiry face is owned by
// kernel::expiry::*; this is the event-book face, fired from inside a
// live situation with locked bindings.
#[test]
fn t9_event_books_are_context_free() {
    let src = r#"
game "Probe"
  seed host
  turn 1m
  home hall

zones
  hall

aspects
  spark: 0

cards
  card coin
    stackable
    aspects spark 1
    start in hall

verbs
  verb holder committable instances 1
    recipe hold
      duration 10m
      slots
        held: 1..1 aspect:spark
      effects
        signal probe

on event probe
  rule check when total(bound, aspect:spark) == 0 and count(interior, aspect:spark) == 0 and count(slot:held, aspect:spark) == 0
    effects
      narrate "context-free"
"#;
    let mut e = game(src);
    common::commit_named(&mut e, "holder", &[("held", "coin")]).expect("binds");
    let events = e.advance(10).expect("completes and signals");
    assert_eq!(
        narration(&events),
        vec!["context-free"],
        "the book read empty scopes while the situation's bindings were locked"
    );
}

// t10 — start candidacy. Owned by matrix_30_* (post-fix semantics) and
// kernel::chains::{per_slot_minimum_gates_engine_source_candidacy,
// matched_but_materialization_short_skips_the_open}.

// t11 — roll validation. Owned by matrix_27_dynamic_rolls_pos (count 0
// leaves the RNG counter unmoved) and matrix_27_dynamic_rolls_neg
// (invalid operands roll the unit back).

// t12 — card books. Owned by
// kernel::order::completion_order_recipe_book_then_card_books_in_bind_
// order (order, dedup, declares) and matrix_25_* (declares and the
// inline-completion face).

// t13 — handle scope. Owned by matrix_26_handles_pos (pool behavior
// within its list) and matrix_26_handles_neg ([E238] is the static face
// of "empty outside its list").

// t14 — per-binding writes: a slot-target write lands once per bound
// card, and the same card in two slots accumulates both writes.
#[test]
fn t14_per_binding_writes() {
    let src = r#"
game "Stamp"
  seed host
  turn 1m
  home hall

zones
  hall

aspects
  spark: 0
  stamp: 0

cards
  card coin
    stackable
    aspects spark 1
    start in hall

  card other
    stackable
    aspects spark 1
    start in hall

verbs
  verb stamper committable instances 1
    recipe mark
      duration 0
      slots
        a: 1..1 aspect:spark
        b: 1..1 aspect:spark
      effects
        set slot:a aspect:stamp += 1
        set slot:b aspect:stamp += 1
"#;
    let mut e = game(src);
    common::commit_named(&mut e, "stamper", &[("a", "coin"), ("b", "coin")]).expect("stamps");
    let coin = common::named(&e, "coin");
    assert_eq!(aspect(&e, coin, "stamp"), 2, "one write per binding");
    assert_eq!(
        aspect(&e, common::named(&e, "other"), "stamp"),
        0,
        "the other card is untouched"
    );
}
