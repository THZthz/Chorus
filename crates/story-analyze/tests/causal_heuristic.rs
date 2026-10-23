//! Tier 1 search support (story-analyze spec §4 outputs 2–4) and the
//! Tier1/Tier2 agreement (§10 M2): plausibility pre-checks, the `h_add`
//! heuristic, dead-mechanism pruning, and the agreement law — everything
//! Tier 2 reaches must be statically plausible, and every statically
//! impossible goal explores empty within bounds.

mod common;

use std::collections::BTreeSet;

use story_analyze::causal::{goal_facts, Fact, Plausibility};
use story_analyze::{GoalPredicate, ReachQuery, SearchObjective};
use story_content::ir::Expr;

const SHOP: &str = r#"
game "Shop"
  seed host
  turn 10m
  home bench

zones
  bench

aspects
  charge: 0

cards
  card coin
    kind matter  stackable
    start in bench count 2
  card gem
    kind treasure  stackable

legacies
  luck: 0

endings
  ending won "Won" grand

verbs
  verb buy committable instances 1
    recipe trade
      craftable
      max 1
      duration 0
      slots
        pay: 1 consuming aspect:charge
      effects
        spawn gem into bench

  verb pray committable instances 3
    recipe hope
      craftable
      max 1
      duration 0
      effects
        narrate "nothing happens"
"#;

#[test]
fn plausibility_pre_checks_the_query() {
    let content = common::compile(SHOP);
    let a = common::analyzer(content, 1);
    let causal = a.causal();

    // A boot card is grounded.
    assert_eq!(
        causal.plausibility(&Fact::CardLive("coin".into())),
        Plausibility::Plausible
    );
    // The gem is producible through the trade.
    assert_eq!(
        causal.plausibility(&Fact::CardLive("gem".into())),
        Plausibility::Plausible
    );
    // Nothing can ever produce the relic.
    assert_eq!(
        causal.plausibility(&Fact::CardLive("relic".into())),
        Plausibility::StaticallyImpossible
    );
    // Provenance chains the gem through the trade.
    let prov = causal
        .explain_how_to_get(&Fact::CardLive("gem".into()))
        .expect("a producer cone exists");
    assert!(matches!(prov.fact, Fact::CardLive(c) if c.as_str() == "gem"));
}

#[test]
fn statically_impossible_goals_explore_empty() {
    let content = common::compile(SHOP);
    let mut a = common::analyzer(content, 1).without_heuristic();
    let mut q = ReachQuery::new(GoalPredicate::Expr(
        // `relic.charge >= 1` reads a card that no producer can mint.
        Expr::Cmp {
            op: story_content::ir::CmpOp::Ge,
            left: Box::new(Expr::NamePath {
                card: "relic".into(),
                key: "charge".into(),
            }),
            right: Box::new(Expr::Lit(1)),
        },
    ));
    q.max_states = 2_000;
    q.max_time = Some(60);
    let out = a.solve(&q);
    assert!(out.witnesses.is_empty());
    // Within the bounds the whole space was explored: the empty answer is
    // conclusive (the §10 agreement law, Tier 2 side).
    assert!(out.conclusive());
}

#[test]
fn everything_tier2_reaches_is_statically_plausible() {
    let content = common::compile(SHOP);
    let mut a = common::analyzer(content, 1).without_heuristic();
    let mut q = ReachQuery::new(GoalPredicate::Expr(Expr::Legacy("luck".into())));
    q.objective = SearchObjective::DiverseWays { max_ways: 3 };
    q.max_states = 2_000;
    q.max_time = Some(60);
    let out = a.solve(&q);
    // The solver's own oracle verifies the witnesses; the Tier-1 side
    // grounds every fact the witnesses engage.
    let causal = a.causal();
    for w in &out.witnesses {
        for r in w.recipe_trace().into_iter().flatten() {
            assert_eq!(
                causal.plausibility(&Fact::RecipeCanOpen(r)),
                Plausibility::Plausible,
                "an opened recipe must be statically plausible"
            );
        }
    }
}

#[test]
fn h_add_orders_the_queue_without_pruning() {
    let content = common::compile(SHOP);
    let a = common::analyzer(content.clone(), 1);
    let causal = a.causal();

    // The gem needs one causal step (the trade) from boot; the boot card
    // needs none.
    let engine = common::boot(content.clone(), 1);
    let state = engine.state().clone();
    let h_gem = causal.h_add(&state, &engine, &BTreeSet::from([Fact::CardLive("gem".into())]));
    let h_coin = causal.h_add(&state, &engine, &BTreeSet::from([Fact::CardLive("coin".into())]));
    assert_eq!(h_gem, 1, "one trade edge");
    assert_eq!(h_coin, 0, "boot cards hold");

    // Guided solving still answers (the heuristic only orders).
    let mut guided = common::analyzer(content.clone(), 1);
    let mut q = ReachQuery::new(GoalPredicate::Expr(Expr::Cmp {
        op: story_content::ir::CmpOp::Ge,
        left: Box::new(Expr::NamePath {
            card: "gem".into(),
            key: "charge".into(),
        }),
        right: Box::new(Expr::Lit(0)),
    }));
    q.max_states = 2_000;
    q.max_time = Some(60);
    let out = guided.solve(&q);
    assert!(!out.witnesses.is_empty());
    assert_eq!(out.witnesses_dropped_by_the_oracle, 0);
}

#[test]
fn dead_mechanisms_are_deprioritized_not_pruned() {
    let content = common::compile(SHOP);
    let a = common::analyzer(content.clone(), 1);
    let causal = a.causal();

    // The goal cone: a live gem.
    let goals = BTreeSet::from([Fact::CardLive("gem".into())]);
    let cone = causal.backward_cone(&goals);
    let relevant = causal.relevant_verbs(&content, &cone);
    assert!(
        relevant.contains(&story_core::VerbId::from("buy")),
        "the producing verb is relevant"
    );
    assert!(
        !relevant.contains(&story_core::VerbId::from("pray")),
        "the narrate-only verb cannot influence the cone"
    );

    // Deprioritization never prunes: the search with the goal `luck`
    // still completes through the pray verb's states.
    let facts = goal_facts(&GoalPredicate::Expr(Expr::Legacy("luck".into())));
    assert_eq!(
        facts,
        BTreeSet::from([Fact::LegacyCanChange("luck".into())]),
        "a legacy read grounds its write-capability fact"
    );
    let mut guided = common::analyzer(content.clone(), 1);
    let mut q = ReachQuery::new(GoalPredicate::Ending(Some("won".into())));
    q.max_states = 2_000;
    q.max_time = Some(60);
    let out = guided.solve(&q);
    assert!(out.witnesses.is_empty());
    assert!(out.conclusive(), "the shop cannot end");
}
