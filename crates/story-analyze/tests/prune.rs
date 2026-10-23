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
    assert!(on.pruned > 0, "the relaxation gate cut the dead branches");
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
    assert_eq!(
        off.witnesses.len(),
        on.witnesses.len(),
        "same witness count"
    );
    assert!(on.pruned > 0, "dead branches are pruned");
    assert!(on.expansions <= off.expansions);
    if let (Some(a), Some(b)) = (off.witnesses.first(), on.witnesses.first()) {
        assert_eq!(a.final_state().ended, b.final_state().ended);
        assert_eq!(a.records.len(), b.records.len());
        assert_eq!(a.final_clock(), b.final_clock());
    }
}

/// The scarcity shape: the door wants a charge of 3, the battery stock is
/// two cells, and each charge-up burns one — the arithmetic is dead, not
/// just hard. A third cell makes it reachable.
fn charged_cell_fixture(cells: u32) -> String {
    format!(
        r#"
game "Cells"
  seed 3
  turn 30m
  home bench

zones
  bench

aspects
  charge: 0
  juice: 0

cards
  card lamp
    kind tool  unique
    aspects charge 0
    start in bench
  card cell
    kind matter  stackable
    aspects juice 1
    start in bench count {cells}

legacies
  luck: 0

endings
  ending lit "Lit" grand
    text "The lamp holds."
  ending dark "Dark" somber
    text "Cold filament."

verbs
  verb charge committable instances 1
    recipe top_up
      craftable
      duration 0
      when count(anywhere, aspect:juice) >= 1
      effects
        consume aspect:juice 1
        set lamp.charge += 1
        narrate "One cell in."
    recipe hand_crank
      max 1
      craftable
      duration 0
      effects
        set lamp.charge += 1
  verb light committable instances 1
    recipe strike
      craftable
      duration 0
      when clock >= 30m
      effects
        if lamp.charge >= 4
          ending lit
        else
          ending dark
"#
    )
}

fn lit_query(prune: bool) -> ReachQuery {
    let mut q = ReachQuery::new(GoalPredicate::Ending(Some("lit".into())));
    q.prune = prune;
    q.max_time = Some(600);
    q
}

#[test]
fn a_dead_scarcity_budget_prunes_and_degrades_honestly() {
    // Two cells plus one max-1 hand crank against a four-charge door: every
    // branch is arithmetically dead, exhaustively or heuristically depending
    // on the flag, and the MDP solver degrades `complete` when pruning fires.
    let content = common::compile(&charged_cell_fixture(2));

    let off = common::analyzer(content.clone(), 3).solve(&lit_query(false));
    assert!(off.witnesses.is_empty());
    assert_eq!(off.pruned, 0);
    assert!(off.conclusive(), "exhaustive runs are conclusive");

    let on = common::analyzer(content.clone(), 3).solve(&lit_query(true));
    assert!(on.witnesses.is_empty());
    assert!(on.pruned > 0, "the scarcity gate fired");
    assert!(!on.conclusive(), "a pruned empty answer is not exhaustive");

    let mut mdp_cfg = story_analyze::mdp::MdpConfig::new(GoalPredicate::Ending(Some("lit".into())));
    mdp_cfg.max_time = Some(600);
    mdp_cfg.prune = true;
    let mdp_on = story_analyze::mdp::solve(&common::analyzer(content, 3), &mdp_cfg);
    assert_eq!(mdp_on.start.v_max, 0.0);
    assert!(!mdp_on.complete, "pruned MDP runs are not exhaustive");
}

#[test]
fn a_live_scarcity_budget_and_else_branch_ending_are_never_pruned() {
    // Three cells plus one hand crank reaches `lit` (charge >= 4), while the
    // `else` branch ending `dark` (charge < 4) stays reachable from boot
    // without raising `lamp.charge`.
    let content = common::compile(&charged_cell_fixture(3));
    for prune in [false, true] {
        let out = common::analyzer(content.clone(), 3).solve(&lit_query(prune));
        assert_eq!(out.witnesses.len(), 1, "prune={prune} reaches lit");

        let mut dark = ReachQuery::new(GoalPredicate::Ending(Some("dark".into())));
        dark.prune = prune;
        dark.max_time = Some(600);
        let dark_out = common::analyzer(content.clone(), 3).solve(&dark);
        assert_eq!(dark_out.witnesses.len(), 1, "prune={prune} reaches dark");
    }
}
