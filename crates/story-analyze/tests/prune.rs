//! The `--prune` feasibility gate: witnesses are unaffected (they replay
//! either way), goal cards extinct with no recipe producer prune their
//! branches, and a pruned empty answer degrades honestly — it is never
//! reported as exhaustive unreachability.

mod common;

use story_analyze::{GoalPredicate, ReachQuery};
use story_content::ir::{QueryTerm, Scope};
use story_core::CardId;

const EMBERWATCH: &str = include_str!("../../../docs/emberwatch.sw");

/// The key exists only as a mint template: no boot instance, no recipe
/// spawns it, and the door demands it. Every branch past the start is dead,
/// honestly or heuristically depending on the flag.
const VANISH: &str = r#"
game "Vanish"
  seed 3
  turn 30m
  home bench

zones
  bench

aspects
  charge: 0

cards
  card key
    kind matter  unique
    aspects charge 0
    desc "The only key, and it is nowhere."
  card dust
    kind matter  stackable
    start in bench count 2

endings
  ending open "Open" grand
    text "The door knows its key."

verbs
  verb seal committable instances 1
    recipe unseal
      craftable
      duration 0
      when key.charge >= 1
      effects
        ending open
"#;

fn key_query(prune: bool) -> ReachQuery {
    let mut q = ReachQuery::new(GoalPredicate::CardMatch {
        scope: Scope::Anywhere,
        prototype: Some(CardId::from("key")),
        kind: None,
        terms: vec![QueryTerm::Name(CardId::from("key"))],
        min_count: 1,
        min_ttl: None,
    });
    q.prune = prune;
    q.max_time = Some(600);
    q
}

#[test]
fn an_extinct_goal_card_prunes_and_degrades_honestly() {
    let content = common::compile(VANISH);

    // Exhaustive: the space is tiny, the answer is a real UNREACHABLE.
    let off = common::analyzer(content.clone(), 3).solve(&key_query(false));
    assert!(off.witnesses.is_empty());
    assert_eq!(off.pruned, 0);
    assert!(off.conclusive(), "exhaustive runs are conclusive");

    // Pruned: the gate fires on every successor, and the empty answer
    // loses its conclusiveness — the caller must not read it as
    // UNREACHABLE.
    let on = common::analyzer(content, 3).solve(&key_query(true));
    assert!(on.witnesses.is_empty());
    assert!(on.pruned > 0, "the extinction gate fired");
    assert!(
        !on.conclusive(),
        "a pruned empty answer is not exhaustive unreachability"
    );
}

#[test]
fn the_door_goal_prunes_through_the_relaxation_gate() {
    // The ending's recipe `when` reads the key, so from key-less states the
    // delete relaxation cannot ground the ending fact — the same gate, via
    // the h path instead of the extinction path.
    let content = common::compile(VANISH);
    let run = |prune| {
        let mut q = ReachQuery::new(GoalPredicate::Ending(Some("open".into())));
        q.prune = prune;
        q.max_time = Some(600);
        common::analyzer(content.clone(), 3).solve(&q)
    };
    let off = run(false);
    assert!(off.witnesses.is_empty() && off.conclusive());
    let on = run(true);
    assert!(on.witnesses.is_empty());
    assert!(
        on.pruned > 0,
        "the relaxation gate cut the dead branches"
    );
}

#[test]
fn witnesses_survive_the_gate() {
    // The survived line exists with and without pruning, identical to the
    // replay oracle's eyes.
    let content = common::compile(EMBERWATCH);
    let run = |prune| {
        let mut q = ReachQuery::new(GoalPredicate::Ending(Some("survived".into())));
        q.prune = prune;
        q.max_states = 60_000;
        q.max_time = Some(430);
        common::analyzer(content.clone(), 7).solve(&q)
    };
    let off = run(false);
    let on = run(true);
    assert_eq!(off.witnesses.len(), on.witnesses.len(), "same witness count");
    assert_eq!(on.pruned, 0, "the survived line prunes nothing yet");
    if let (Some(a), Some(b)) = (off.witnesses.first(), on.witnesses.first()) {
        assert_eq!(a.final_state().ended, b.final_state().ended);
        assert_eq!(a.records.len(), b.records.len());
        assert_eq!(a.final_clock(), b.final_clock());
    }
}
