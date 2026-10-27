//! Entry 27 — dynamic rolls and the roll binding's scoping.

use super::*;

// matrix 27
/// Roll operands are full expressions evaluated at effect time; the dice
/// advance the RNG counter once each; count 0 applies the bonus only and
/// leaves the counter unmoved.
#[test]
fn matrix_27_dynamic_rolls_pos() {
    let src = r#"
game "Storm"
  seed host
  turn 1m
  home hall

zones
  hall

aspects
  dice: 0
  charge: 0

legacies
  favor: 3

cards
  card die
    stackable
    aspects dice 1
    start in hall count 2

  card beacon
    unique
    start in hall

verbs
  verb roller committable instances 1
    recipe cast
      duration 0
      effects
        roll total(world, aspect:dice)d(2 + 4) + legacy.favor as storm
        set beacon.charge += storm
        narrate "storm={storm}"
        roll 0d6 + 5 as grace
        narrate "grace={grace}"
"#;
    let mut e = game(src);
    let rng_before = e.state().rng_counter;
    let events = common::commit_named(&mut e, "roller", &[]).expect("rolls");
    assert_eq!(
        e.state().rng_counter - rng_before,
        2,
        "two dice, two counter ticks"
    );
    assert_eq!(narration(&events)[0], "storm={storm}");
    let beacon = common::named(&e, "beacon");
    let storm = aspect(&e, beacon, "charge");
    assert!((5..=15).contains(&storm), "3 bonus + 2d6: {storm}");
    assert_eq!(aspect(&e, beacon, "charge"), storm, "the binding flowed");
    assert_eq!(
        narration(&events)[1],
        "grace={grace}",
        "count 0: bonus only"
    );
    assert_eq!(
        e.state().rng_counter - rng_before,
        2,
        "count 0 left the counter unmoved"
    );
}

// matrix 27
/// The F4 face: a roll binding IS visible to a predicate called from an
/// `if` condition — the inlined body reads the roll through a comparison
/// and the taken branch follows the dice (language.md §4.3). Query
/// positions and slot filters stay E244
/// (`matrix_27_roll_scope_compile_refusal`).
#[test]
fn matrix_27_roll_in_if_predicate_pos() {
    let src = r#"
game "Oracle"
  seed host
  turn 1m
  home hall

zones
  hall

predicates
  predicate fate_at_least(n) = fate >= n

cards
  card beacon
    unique
    start in hall

verbs
  verb seer committable instances 1
    recipe gaze
      duration 0
      effects
        roll 1d6 as fate
        narrate "fate={fate}"
        if fate_at_least(4)
          narrate "high"
        else
          narrate "low"
"#;
    let mut e = game(src);
    let events = common::commit_named(&mut e, "seer", &[]).expect("rolls");
    assert_eq!(narration(&events)[0], "fate={fate}");
    assert!(
        narration(&events)[1] == "high" || narration(&events)[1] == "low",
        "the branch follows the roll through the predicate body"
    );
}

// matrix 27
/// Invalid operands are effect failures rolling the unit back: sides < 1
/// and count < 0 both refuse, leaving the RNG counter untouched.
#[test]
fn matrix_27_dynamic_rolls_neg() {
    let bad_sides = r#"
game "Storm"
  seed host
  turn 1m
  home hall

zones
  hall

verbs
  verb roller committable instances 1
    recipe jinx
      duration 0
      effects
        roll 0d0 as jinx
"#;
    let mut e = game(bad_sides);
    let before = state_bytes(&e);
    assert_eq!(
        err_kind(common::commit_named(&mut e, "roller", &[])),
        DiagKind::EffectFailure
    );
    assert_eq!(state_bytes(&e), before);

    let bad_count = bad_sides.replace("roll 0d0 as jinx", "roll (0 - 1)d6 as jinx");
    let mut e = game(&bad_count);
    let before = state_bytes(&e);
    assert_eq!(
        err_kind(common::commit_named(&mut e, "roller", &[])),
        DiagKind::EffectFailure
    );
    assert_eq!(state_bytes(&e), before);
}

// matrix 27
/// The roll binding is scoped to its effect list: slot filters, recipe
/// `when`s, and book rules are compiled without any roll environment, so
/// a roll reference there is an unknown-name compile refusal — candidacy
/// can never see a roll (review guidance for M2, item 7).
#[test]
fn matrix_27_roll_scope_compile_refusal() {
    let filter = r#"
game "Scope"
  seed host
  turn 1m
  home hall

zones
  hall

aspects
  spark: 0

predicates
  predicate fate_touched(p) = p.spark >= fate

cards
  card coin
    stackable
    aspects spark 1
    start in hall

verbs
  verb grabber committable instances 1
    recipe take
      duration 0
      slots
        s: 1..1 fate_touched
      effects
        roll 1d6 as fate
        narrate "took"
"#;
    assert!(
        compile_error_codes(filter).contains(&code::E244),
        "a slot filter can never see a roll binding"
    );
    let when = r#"
game "Scope"
  seed host
  turn 1m
  home hall

zones
  hall

cards
  card coin
    stackable
    start in hall

verbs
  verb grabber committable instances 1
    recipe take
      duration 0
      when fate >= 1
      effects
        roll 1d6 as fate
        narrate "took"
"#;
    assert!(
        compile_error_codes(when).contains(&code::E244),
        "a selection `when` can never see a roll binding"
    );
    // The target-query face pins the same scoping per the spec reading
    // (language.md §4.3 grants the binding to if conditions and prose
    // tokens, not query terms); see the findings list in the M2 report.
    let target_query = r#"
game "Scope"
  seed host
  turn 1m
  home hall

zones
  hall

aspects
  spark: 0

predicates
  predicate fate_touched(p) = p.spark >= fate

cards
  card coin
    stackable
    aspects spark 1
    start in hall

verbs
  verb grabber committable instances 1
    recipe take
      duration 0
      effects
        roll 1d6 as fate
        destroy world fate_touched count 1
"#;
    assert!(
        compile_error_codes(target_query).contains(&code::E244),
        "a target query term can never see a roll binding"
    );
}
