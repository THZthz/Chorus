//! Internal slots in the transition model: an internal slot is engine
//! machinery, never a player choice — a `min >= 1` internal slot contributes
//! exactly one host-staged fill (the kernel's deterministic candidates minus
//! whatever the set under construction already uses), a `min = 0` internal
//! slot contributes the empty binding, and neither multiplies the explored
//! action space.

mod common;

use std::sync::Arc;

use story_analyze::transitions::{GeneratorConfig, TransitionGenerator};
use story_analyze::{GoalPredicate, PlayerStep};
use story_content::Content;
use story_core::{CardId, EntityId, SlotName};

fn gen(content: &Arc<Content>) -> TransitionGenerator {
    let engine = common::boot(content.clone(), 1);
    let cfg = GeneratorConfig {
        seed: 1,
        legacy_in: None,
        rng_mode: story_analyze::RngMode::Deterministic,
        allow_mid_run_bind: true,
        max_time: None,
        invariant: None,
        query_thresholds: Default::default(),
        allowed_verbs: None,
        quiescence_pruning: false,
    };
    TransitionGenerator::new(content.clone(), engine, cfg)
}

fn costed_src(cost_slot: &str) -> String {
    format!(
        r#"
game "Costly"
  seed host
  turn 10m
  home bench

zones
  bench

aspects
  charge: 0
  fuel: 0

legacies
  luck: 0

cards
  card gear
    kind gadget  stackable
    aspects charge 1
    start in bench count 3
  card battery
    kind gadget  stackable
    aspects fuel 1
    start in bench count 1

verbs
  verb work committable instances 2
    recipe job
      craftable
      duration 0
      slots
        main: 1..1 open aspect:charge
        {cost_slot}
      effects
        legacy luck += 1
"#
    )
}

fn commits_of(g: &mut TransitionGenerator) -> Vec<Vec<(SlotName, EntityId)>> {
    let state = g.state().clone();
    g.steps(&state)
        .into_iter()
        .filter_map(|s| match s {
            PlayerStep::Commit { bindings, .. } => Some(bindings),
            _ => None,
        })
        .collect()
}

fn prototype_of(g: &TransitionGenerator, e: EntityId) -> CardId {
    g.state().entities[&e].prototype.clone()
}

// internal 01
/// A `min >= 1` internal slot is not a player choice: the single commit
/// stages the deterministic fill (the boot battery) alongside the explicit
/// `main` binding.
#[test]
fn an_internal_cost_slot_stages_one_deterministic_fill() {
    let content = common::compile(costed_src("cost: 1..1 consuming internal aspect:fuel").as_str());
    let mut g = gen(&content);
    let commits = commits_of(&mut g);
    assert_eq!(commits.len(), 1, "the internal slot adds no branching");
    assert_eq!(commits[0].len(), 2, "main and the staged cost fill");
    let cost = commits[0]
        .iter()
        .find(|(s, _)| s == &SlotName::from("cost"))
        .map(|(_, e)| *e)
        .expect("the fill is part of the commit");
    assert_eq!(
        prototype_of(&g, cost),
        CardId::from("battery"),
        "the fill is the kernel's deterministic candidate"
    );
}

// internal 02
/// The internal slot does not multiply the explored action space: boot
/// step counts with the `min >= 1` internal slot match an equivalent
/// non-internal `min = 0` slot (both contribute exactly one choice).
#[test]
fn the_internal_slot_does_not_multiply_the_action_space() {
    let internal = common::compile(costed_src("cost: 1..1 consuming internal aspect:fuel").as_str());
    let plain = common::compile(costed_src("cost: 0..0 aspect:fuel").as_str());
    let mut gi = gen(&internal);
    let mut gp = gen(&plain);
    let internal_steps = gi.steps(&gi.state().clone());
    let plain_steps = gp.steps(&gp.state().clone());
    assert_eq!(internal_steps.len(), plain_steps.len());
    let internal_commits = internal_steps
        .iter()
        .filter(|s| matches!(s, PlayerStep::Commit { .. }))
        .count();
    let plain_commits = plain_steps
        .iter()
        .filter(|s| matches!(s, PlayerStep::Commit { .. }))
        .count();
    assert_eq!(internal_commits, plain_commits);
    assert_eq!(internal_commits, 1);
}

// internal 03
/// The auto-staged fill skips what the set under construction already uses:
/// a card eligible for both slots is never staged twice, and a starved
/// branch yields no sets at all.
#[test]
fn the_auto_fill_dedupes_across_slots_and_starves_without_candidates() {
    let src = |count: u32| {
        format!(
            r#"
game "Dual"
  seed host
  turn 10m
  home bench

zones
  bench

aspects
  charge: 0
  fuel: 0

cards
  card pack
    kind gadget  stackable
    aspects charge 1, fuel 1
    start in bench count {count}

verbs
  verb work committable instances 1
    recipe job
      craftable
      duration 0
      slots
        main: 1..1 open aspect:charge
        cost: 1..1 consuming internal aspect:fuel
      effects
        narrate "done"
"#
        )
    };
    let two = common::compile(src(2).as_str());
    let mut g = gen(&two);
    let commits = commits_of(&mut g);
    assert_eq!(commits.len(), 1);
    assert_eq!(commits[0].len(), 2);
    let main = commits[0].iter().find(|(s, _)| s == &SlotName::from("main"));
    let cost = commits[0].iter().find(|(s, _)| s == &SlotName::from("cost"));
    assert_ne!(main.map(|(_, e)| e), cost.map(|(_, e)| e));
    let one = common::compile(src(1).as_str());
    let mut g = gen(&one);
    assert!(
        commits_of(&mut g).is_empty(),
        "a branch with no unused candidate yields no sets"
    );
}

// internal 04
/// Reachability: the costed recipe's commit is still found end to end —
/// the auto-staged internal fill makes it a valid kernel commit.
#[test]
fn the_costed_recipe_stays_reachable() {
    let content = common::compile(costed_src("cost: 1..1 consuming internal aspect:fuel").as_str());
    let mut a = common::analyzer(content, 1);
    let q = common::dijkstra_query(GoalPredicate::Expr(story_content::ir::Expr::Legacy(
        "luck".into(),
    )));
    let out = a.solve(&q);
    assert!(
        !out.witnesses.is_empty(),
        "the internal cost slot does not block the recipe's commit"
    );
}
