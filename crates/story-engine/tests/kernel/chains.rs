//! Chain starts: `when` gates, materialization skips (T10), and per-slot candidacy.

use super::*;

/// A `start` whose `when` never holds ends the chain silently.
#[test]
fn start_with_failing_when_ends_the_chain_silently() {
    let src = r#"
game "Chains"
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
    start in hall count 1

verbs
  verb caller committable instances 1
    recipe go
      duration 5m
      effects
        start shy

  verb shy uncommittable instances 1
    recipe unreachable
      chain-only
      duration 5m
      when count(world, aspect:spark) >= 99
"#;
    let mut e = game(src);
    common::commit_named(&mut e, "caller", &[]).expect("opens");
    let events = e.advance(5).expect("the completion");
    assert_eq!(
        events
            .iter()
            .filter(|ev| matches!(ev, Event::ChainEnded { verb } if verb.as_str() == "shy"))
            .count(),
        1
    );
}

/// T10: a recipe that matches but materializes short of a slot minimum —
/// two slots competing for one card — yields
/// `StartSkipped { MaterializationFailed }` with the parent untouched.
#[test]
fn matched_but_materialization_short_skips_the_open() {
    let src = r#"
game "Chains"
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
    start in hall count 1

verbs
  verb caller committable instances 1
    recipe go
      duration 5m
      effects
        start greedy

  verb greedy uncommittable instances 1
    recipe compete
      chain-only
      duration 5m
      when count(world, aspect:spark) >= 1
      slots
        left: 1..1 aspect:spark
        right: 1..1 aspect:spark
"#;
    let mut e = game(src);
    common::commit_named(&mut e, "caller", &[]).expect("opens");
    let events = e.advance(5).expect("the completion");
    assert!(events.iter().any(|ev| matches!(
        ev,
        Event::StartSkipped { verb, reason: StartSkipReason::MaterializationFailed }
            if verb.as_str() == "greedy"
    )));
    assert!(events
        .iter()
        .any(|ev| matches!(ev, Event::SituationCompleted { .. })));
}

/// Candidacy counts each slot's minimum separately against the
/// state-qualifying candidates: a `2..2` slot with one candidate is not a
/// candidate at all — the chain ends silently — while two candidates fill
/// it and the recipe opens.
#[test]
fn per_slot_minimum_gates_engine_source_candidacy() {
    let one_coin = r#"
game "MinFill"
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
    start in hall count 1

verbs
  verb caller committable instances 1
    recipe go
      duration 5m
      effects
        start pairer

  verb pairer uncommittable instances 1
    recipe pair
      chain-only
      duration 5m
      when count(world, aspect:spark) >= 1
      slots
        left: 2..2 aspect:spark
"#;
    let mut e = game(one_coin);
    common::commit_named(&mut e, "caller", &[]).expect("opens");
    let events = e.advance(5).expect("the completion");
    assert!(events.iter().any(|ev| matches!(
        ev,
        Event::ChainEnded { verb } if verb.as_str() == "pairer"
    )));
    assert!(
        !events.iter().any(
            |ev| matches!(ev, Event::StartSkipped { verb, .. } if verb.as_str() == "pairer")
        ),
        "an unfillable minimum is not a materialization skip"
    );

    let two_coins = one_coin.replace("start in hall count 1", "start in hall count 2");
    let mut e = game(&two_coins);
    common::commit_named(&mut e, "caller", &[]).expect("opens");
    let events = e.advance(5).expect("the completion");
    let live: Vec<String> = situations(&e)
        .into_iter()
        .map(|s| e.state().situations[&s].recipe.to_string())
        .collect();
    assert_eq!(
        live,
        vec!["pairer.pair".to_string()],
        "the pair opens with two candidates"
    );
    assert!(narration(&events).is_empty(), "no narration, just the open");
}
