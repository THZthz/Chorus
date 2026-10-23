//! Emberwatch acceptance queries (story-analyze spec §10, M1–M5): the
//! suite asserts replay validity and the class of findings — the analyzer
//! is the arbiter of what the kernel actually does, including where the
//! sample's header comments and the kernel's arithmetic diverge (the heat
//! budget: four coal against six hours of furnace cannot hold the beacon
//! until dawn). Concrete numbers are pinned after hand verification.

mod common;

use std::collections::BTreeSet;

use story_analyze::audit::audit;
use story_analyze::classify::{classify, ClassifyConfig};
use story_analyze::{
    Evidence, ExploreConfig, GoalPredicate, PlayerStep, ReachQuery, RngMode, SearchObjective,
};
use story_content::ir::{CmpOp, Expr, QueryTerm, Scope};
use story_core::{CardId, EndingId};
use story_engine::{EffectRegistry, Engine};

fn ember() -> std::sync::Arc<story_content::Content> {
    let src = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../docs/emberwatch.sw"
    ))
    .unwrap();
    common::compile(&src)
}

fn ember_engine() -> Engine {
    let (e, diags) = Engine::boot(ember(), EffectRegistry::new(), 7, None);
    assert!(diags.iter().all(|d| !d.is_error()), "boot errors: {diags:?}");
    e
}

fn heat_of(e: &Engine) -> i64 {
    e.state()
        .entities
        .values()
        .find(|x| x.name == "beacon")
        .and_then(|x| x.aspects.get("heat"))
        .copied()
        .unwrap_or(0)
}

fn watch_of(e: &Engine) -> i64 {
    e.state()
        .entities
        .values()
        .find(|x| x.name == "warden")
        .and_then(|x| x.aspects.get("watch"))
        .copied()
        .unwrap_or(0)
}

/// The beacon must be hot for the dawn tick to mean anything; the passive
/// game at the witching hour is the depleted late-game state of §10.
fn witching_hour() -> Engine {
    let mut e = ember_engine();
    e.advance(300).unwrap();
    assert_eq!(e.state().now, 300);
    e
}

#[test]
fn the_passive_night_ends_gray() {
    let e = witching_hour();
    let mut a = common::analyzer(ember(), 7)
        .without_heuristic()
        .with_start_state(e.state().clone());
    let mut q = ReachQuery::new(GoalPredicate::Ending(Some("gray_dawn".into())));
    // FewestActions: the pure-advance line has zero action cost, so it
    // surfaces immediately (EarliestTime's Dijkstra must drain every
    // clock layer below the dawn first — a horizon-scale job).
    q.objective = SearchObjective::FewestActions;
    q.max_states = 6_000;
    q.max_time = Some(430);
    let out = a.solve(&q);
    assert_eq!(out.witnesses.len(), 1, "the passive line reaches gray dawn");
    assert!(!out.truncated);
    assert_eq!(out.witnesses_dropped_by_the_oracle, 0);
    let w = &out.witnesses[0];
    // Dawn opens at the 6h tick and completes an hour later, heat long dead.
    assert!(w.final_clock() >= 420);
    assert_eq!(heat_of(&engine_of_state(&content(), w.final_state())), 0);
    // The witness is pure advance: doing nothing is a strategy.
    assert_eq!(w.actions, 0);
}

fn content() -> std::sync::Arc<story_content::Content> {
    ember()
}

fn engine_of_state(content: &std::sync::Arc<story_content::Content>, state: &story_core::State) -> Engine {
    let (e, _) = Engine::boot(content.clone(), EffectRegistry::new(), 7, None);
    e.with_state(state.clone())
}

#[test]
fn the_arbiter_finds_a_survived_line_from_boot() {
    let mut a = common::analyzer(ember(), 7).without_heuristic();
    let mut q = ReachQuery::new(GoalPredicate::Ending(Some("survived".into())));
    q.max_states = 6_000;
    q.max_time = Some(430);
    let out = a.solve(&q);
    // The arbiter finding, against the header's implied six-hour stoking
    // grind: one LATE blaze is enough. Waiting through the dark hours
    // keeps the furnace idle (its cool gate reads heat >= 1), the gnaws
    // clamp at a cold beacon, and a +3 burn at 250m suffers exactly one
    // cool drain before the dawn tick — the minimal-action degenerate
    // strategy the spec says the analyzer is the arbiter of. The witness
    // replays (solve runs the oracle); the shape is pinned.
    assert!(!out.witnesses.is_empty(), "a survived line exists");
    assert_eq!(out.witnesses_dropped_by_the_oracle, 0);
    let w = &out.witnesses[0];
    assert_eq!(
        w.final_state().ended.as_ref(),
        Some(&EndingId::from("survived"))
    );
    assert!(w.final_clock() >= 420 && w.final_clock() <= 430);
    assert_eq!(w.actions, 1, "one late blaze carries the night");
    // The burn sits late: the beacon is still cold when it lands.
    let blaze_at = w
        .records
        .iter()
        .find(|r| matches!(r.step, PlayerStep::Commit { .. }))
        .map(|r| r.time_after)
        .unwrap();
    assert!(blaze_at >= 200, "the blaze lands at t={blaze_at}");
}

#[test]
fn the_heat_invariant_fails_at_boot() {
    let mut a = common::analyzer(ember(), 7).without_heuristic();
    let mut q = ReachQuery::new(GoalPredicate::Ending(Some("survived".into())));
    q.invariant = Some(GoalPredicate::Expr(Expr::Cmp {
        op: CmpOp::Ge,
        left: Box::new(Expr::NamePath {
            card: "beacon".into(),
            key: "heat".into(),
        }),
        right: Box::new(Expr::Lit(1)),
    }));
    q.max_states = 1_000;
    q.max_time = Some(430);
    let out = a.solve(&q);
    // The beacon boots cold: the invariant is violated on the start state
    // itself, so the query is conclusively empty (§5.6's invariant gate).
    assert!(out.witnesses.is_empty());
    assert!(out.conclusive());
}

#[test]
fn a_token_answers_one_flash_under_luck() {
    let mut a = common::analyzer(ember(), 7).without_heuristic();
    let mut q = ReachQuery::new(GoalPredicate::CardMatch {
        scope: Scope::Anywhere,
        prototype: Some(CardId::from("token")),
        kind: None,
        terms: vec![QueryTerm::Name(CardId::from("token"))],
        min_count: 1,
        min_ttl: None,
    });
    q.rng_mode = RngMode::OptimisticBranching;
    q.max_states = 3_000;
    q.max_time = Some(300);
    let out = a.solve(&q);
    assert_eq!(out.witnesses.len(), 1);
    assert_eq!(out.witnesses_dropped_by_the_oracle, 0);
    let w = &out.witnesses[0];
    // One seaward commit: `reply = 1d6 + watch(1) >= 5` wins on a 4+.
    assert_eq!(w.actions, 1);
    assert!((w.probability.unwrap() - 0.5).abs() < 1e-9);
}

#[test]
fn watch_three_via_dream_and_portent() {
    let mut a = common::analyzer(ember(), 7).without_heuristic();
    let mut q = ReachQuery::new(GoalPredicate::Expr(Expr::Cmp {
        op: CmpOp::Ge,
        left: Box::new(Expr::NamePath {
            card: "warden".into(),
            key: "watch".into(),
        }),
        right: Box::new(Expr::Lit(3)),
    }));
    q.rng_mode = RngMode::OptimisticBranching;
    q.max_states = 3_000;
    q.max_time = Some(300);
    let out = a.solve(&q);
    assert!(!out.witnesses.is_empty(), "dream + read_portent lift the watch");
    assert_eq!(out.witnesses_dropped_by_the_oracle, 0);
    let w = &out.witnesses[0];
    // One dream commit; the two completions ride the advances.
    assert_eq!(w.actions, 1);
    let e = engine_of_state(&content(), w.final_state());
    assert_eq!(watch_of(&e), 3);
}

#[test]
fn the_depleted_night_is_not_winnable_but_never_stuck() {
    let e = witching_hour();
    let mut a = common::analyzer(ember(), 7).with_start_state(e.state().clone());
    let cfg = ClassifyConfig {
        winning_endings: BTreeSet::from([EndingId::from("survived")]),
        explore: ExploreConfig {
            max_states: 8_000,
            max_time: Some(430),
            ..Default::default()
        },
    };
    let c = classify(&mut a, &cfg);
    // The four coal still sit on the table at the witching hour: the late
    // blaze keeps the win reachable from the start.
    assert!(c.winnable.contains(&c.graph.start));
    // The walking-dead frontier is populated and well-formed: burning the
    // coal early (or one blaze too late) crosses into states the dawn
    // finds cold, while they can still reach the gray dawn.
    assert!(!c.point_of_no_return_edges.is_empty());
    for (u, v) in &c.point_of_no_return_edges {
        assert!(c.winnable.contains(u));
        assert!(!c.winnable.contains(v));
        assert_eq!(c.graph.nodes[*v].state.ended, None);
    }
    // Late states whose remaining horizon cannot complete the dawn are
    // bounds-conditional softlocks; the start is not among them.
    assert!(!c.softlock.contains(&c.graph.start));
    // Endings sit within the explored space.
    assert!(c
        .graph
        .nodes
        .iter()
        .any(|n| n.state.ended.is_some()));
}

#[test]
fn the_auditor_maps_the_night_dead_content_and_the_decay_race() {
    let mut a = common::analyzer(ember(), 7);
    let report = audit(
        &mut a,
        &ExploreConfig {
            max_states: 60_000,
            max_time: Some(430),
            ..Default::default()
        },
    );
    // Three semantic shadowing pairs, one per shape: the event rule whose
    // priority gate hides the once-rule, the continue target behind its
    // chain's entry, and the feed/polish pair `allow shadow` silences at
    // compile time but the scan still orders.
    assert_eq!(report.shadowing.len(), 3, "{:?}", report.shadowing);
    let event_pair = report
        .shadowing
        .iter()
        .find(|s| s.shadower.contains("toll_panic"))
        .expect("the bell's panic rule shadows the once-rule");
    assert!(event_pair.shadowed.contains("toll_once"));
    assert!(report
        .shadowing
        .iter()
        .any(|s| s.verb.as_ref().map(|v| v.as_str()) == Some("log") && s.shadower == "write" && s.shadowed == "close"));
    assert!(report
        .shadowing
        .iter()
        .any(|s| s.verb.as_ref().map(|v| v.as_str()) == Some("tend") && s.shadower == "feed" && s.shadowed == "polish"));

    // Two authored-dead recipes: the handbook (its dawn gate is only written
    // past the last dawn) and routine (polish wins the equal-priority scan).
    for dead in ["tend.handbook", "tend.routine"] {
        let u = report
            .unreachable
            .iter()
            .find(|u| u.kind == "recipe" && u.id == dead)
            .unwrap_or_else(|| panic!("{dead} is dead content"));
        assert_eq!(u.evidence, Evidence::NotWithinBounds);
    }
    // The despair ending is genuinely reachable (teeth failures and the
    // unread-portent dread stack), so it must never be reported statically
    // dead; within a truncated exploration it may only appear
    // bounds-conditional.
    assert!(!report.unreachable.iter().any(
        |u| u.kind == "ending" && u.id == "despair" && u.evidence == Evidence::StaticallyProved
    ));

    // The gnaw-vs-decay race: the bread absorbs the early gnaw ticks, so the
    // race sits deep in the night — one fault within the bounds, its witness
    // ending on the ResolutionFailed.
    assert_eq!(
        report.faults.len(),
        1,
        "{:?}",
        report.faults.iter().map(|f| format!("{:?}", f.event)).collect::<Vec<_>>()
    );
    let last = report.faults[0].witness.records.last().unwrap();
    assert!(last.events.iter().any(|e| matches!(
        e,
        story_engine::Event::ResolutionFailed { .. }
    )));
    assert!(report.faults[0]
        .witness
        .records
        .iter()
        .any(|r| matches!(r.step, PlayerStep::Commit { .. } | PlayerStep::Advance { .. })));
}

#[test]
fn diverse_ways_counts_distinct_survived_recipe_sequences() {
    // Spec Â§10: the `DiverseWays { 5 }` query for `survived`, run from
    // the Â§7.1 depleted late-game state. From the boot state the
    // signature-keyed search degenerates over the 43-poll night (every
    // commit order is its own sequence, unpruned by design) and the bounds
    // answer "empty, truncated"; 12 polls from the depleted state fit the
    // budget and the cap saturates: five distinct, replay-verified recipe
    // sequences carry the night.
    let e = witching_hour();
    let mut a = common::analyzer(ember(), 7).with_start_state(e.state().clone());
    let mut q = ReachQuery::new(GoalPredicate::Ending(Some("survived".into())));
    q.objective = SearchObjective::DiverseWays { max_ways: 5 };
    q.max_states = 60_000;
    q.max_time = Some(430);
    let out = a.solve(&q);
    assert_eq!(out.witnesses.len(), 5, "the cap saturates");
    // One witness per recipe sequence, all replayed (solve runs the
    // oracle) and all ending `survived`.
    let mut sigs: Vec<Vec<Option<story_core::RecipeId>>> = out
        .witnesses
        .iter()
        .map(|w| w.recipe_trace())
        .collect();
    sigs.sort();
    sigs.dedup();
    assert_eq!(sigs.len(), out.witnesses.len(), "one witness per sequence");
    for w in &out.witnesses {
        assert_eq!(
            w.final_state().ended.as_ref(),
            Some(&EndingId::from("survived"))
        );
        // The sequences genuinely differ: a blaze line and a feed line
        // both carry the night.
    }
    let heads: BTreeSet<story_core::RecipeId> = out
        .witnesses
        .iter()
        .filter_map(|w| w.recipe_trace().into_iter().flatten().next())
        .collect();
    assert!(
        heads.contains(&story_core::RecipeId::from("tend.blaze"))
            && heads.contains(&story_core::RecipeId::from("tend.feed")),
        "distinct opening strategies: {heads:?}"
    );
}
