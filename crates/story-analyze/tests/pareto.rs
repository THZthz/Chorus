//! The ParetoAndCutAnalyzer (story-analyze spec §7.4, §10 M5 tests): the
//! frontier over objective vectors, mandatory sets, and counterfactual
//! cuts verified by re-exploration on neutralized content.

mod common;

use story_analyze::pareto::{analyze, Direction, Objective, ParetoConfig};
use story_analyze::{GoalPredicate, SearchObjective};
use story_content::ir::Expr;

/// Two strategies reach `score >= 1`: `cheap` scores 1 with one action,
/// `rich` scores 3 with one action. The frontier over (maximize score,
/// minimize actions) carries both; cutting either recipe is verified (the
/// other still reaches the goal), cutting both blocks it.
const TRADEOFF: &str = r#"
game "Trade"
  seed host
  turn 10m
  home bench

zones
  bench

aspects
  charge: 0

cards
  card tool
    kind gadget  stackable
    aspects charge 1
    start in bench count 2

legacies
  score: 0

endings
  ending won "Won" grand

verbs
  verb act committable instances 2
    recipe cheap
      craftable
      max 1
      duration 0
      effects
        legacy score += 1

    recipe rich
      craftable
      max 1
      duration 0
      slots
        it: 1 consuming aspect:charge
      effects
        legacy score += 3
"#;

fn goal() -> GoalPredicate {
    GoalPredicate::Expr(Expr::Cmp {
        op: story_content::ir::CmpOp::Ge,
        left: Box::new(Expr::Legacy("score".into())),
        right: Box::new(Expr::Lit(1)),
    })
}

#[test]
fn the_frontier_is_undominated_and_witness_backed() {
    let content = common::compile(TRADEOFF);
    let a = common::analyzer(content, 1);
    let mut cfg = ParetoConfig::new(
        goal(),
        vec![
            Objective {
                // Cheap keeps both tools; rich burns one.
                expr: Expr::Count {
                    scope: story_content::ir::Scope::Anywhere,
                    query: vec![story_content::ir::QueryTerm::Name("tool".into())],
                },
                direction: Direction::Maximize,
            },
            Objective {
                expr: Expr::Legacy("score".into()),
                direction: Direction::Maximize,
            },
        ],
    );
    cfg.explore.max_states = 2_000;
    cfg.explore.max_time = Some(30);
    let report = analyze(&a, &cfg);
    assert!(!report.truncated);
    // The frontier in objective order (tools, score): cheap reaches
    // (2, 1); rich reaches (1, 3) — neither dominates the other; the
    // deeper score-2 combinations are dominated by rich.
    let mut points: Vec<Vec<f64>> = report.frontier.iter().map(|p| p.values.clone()).collect();
    points.sort_by(|a, b| a.partial_cmp(b).unwrap());
    assert_eq!(points, vec![vec![1.0, 3.0], vec![2.0, 1.0]], "{points:?}");
    // Every point's witness replays and satisfies the goal (verified in
    // analyze through the oracle).
    for p in &report.frontier {
        assert!(!p.witness.records.is_empty());
    }
}

#[test]
fn mandatory_sets_intersect_the_witnesses() {
    let content = common::compile(TRADEOFF);
    let a = common::analyzer(content, 1);
    let mut cfg = ParetoConfig::new(
        goal(),
        vec![
            Objective {
                expr: Expr::Count {
                    scope: story_content::ir::Scope::Anywhere,
                    query: vec![story_content::ir::QueryTerm::Name("tool".into())],
                },
                direction: Direction::Maximize,
            },
            Objective {
                expr: Expr::Legacy("score".into()),
                direction: Direction::Maximize,
            },
        ],
    );
    cfg.explore.max_states = 2_000;
    cfg.explore.max_time = Some(30);
    let report = analyze(&a, &cfg);
    // Two independent strategies: nothing is mandatory.
    assert!(report.mandatory_recipes.is_empty(), "{:?}", report.mandatory_recipes);
    // rich binds a tool; cheap does not — the intersection is empty.
    assert!(report.mandatory_cards.is_empty());
}

#[test]
fn cuts_hit_every_witness_and_verify_by_reexploration() {
    let content = common::compile(TRADEOFF);
    let a = common::analyzer(content, 1);
    let mut cfg = ParetoConfig::new(
        goal(),
        vec![
            Objective {
                expr: Expr::Count {
                    scope: story_content::ir::Scope::Anywhere,
                    query: vec![story_content::ir::QueryTerm::Name("tool".into())],
                },
                direction: Direction::Maximize,
            },
            Objective {
                expr: Expr::Legacy("score".into()),
                direction: Direction::Maximize,
            },
        ],
    );
    cfg.explore.max_states = 2_000;
    cfg.explore.max_time = Some(30);
    let report = analyze(&a, &cfg);
    // The minimal hitting sets: {cheap, rich} hits both witnesses; no
    // single recipe does.
    assert_eq!(report.minimal_recipe_cuts.len(), 1);
    let cut = &report.minimal_recipe_cuts[0];
    assert_eq!(cut.len(), 2);
    assert!(cut.contains(&story_core::RecipeId::from("act.cheap")));
    assert!(cut.contains(&story_core::RecipeId::from("act.rich")));
    // The verified cuts: neutralizing both leaves no alternative path.
    assert_eq!(report.verified_cuts.len(), 1);
    assert_eq!(&report.verified_cuts[0], cut);
    let _ = SearchObjective::FewestActions;
}

/// One plain pair of strategies rides the frontier while a third,
/// host-op strategy reaches the same goal dominated off it. The op is
/// registered on the analyzer's own registry — story-lang never emits host
/// ops, so the effect is planted directly in the IR. The minimal cut covers
/// the two plain witnesses while the op recipe survives neutralization:
/// the honest verdict is "does not verify", which only the registry-carrying
/// neutral boot can see.
#[test]
fn verify_cut_boots_with_the_analyzers_own_registry() {
    let src = TRADEOFF.replace(
        "        legacy score += 3\n",
        concat!(
            "        legacy score += 3\n",
            "\n",
            "    recipe chant\n",
            "      craftable\n",
            "      max 1\n",
            "      duration 0\n",
            "      slots\n",
            "        it: 1 consuming aspect:charge\n",
            "      effects\n",
            "        narrate \"chant\"\n",
        ),
    );
    let mut content = (*common::compile(&src)).clone();
    for rec in content.verbs.get_mut("act").unwrap().recipes.values_mut() {
        if rec.id == story_core::RecipeId::from("act.chant") {
            rec.effects = vec![story_content::ir::EffectNode::Op {
                name: "bless".into(),
                args: Vec::new(),
            }];
        }
    }
    let mut registry = story_engine::EffectRegistry::new();
    registry.register_host("bless", |state, _| {
        if let Some(score) = state.legacy.get_mut("score") {
            *score += 2;
        }
        Vec::new()
    });
    let (a, diags) = story_analyze::Analyzer::boot(std::sync::Arc::new(content), registry, 1, None);
    assert!(diags.iter().all(|d| !d.is_error()), "{diags:?}");
    let mut cfg = ParetoConfig::new(
        goal(),
        vec![
            Objective {
                expr: Expr::Count {
                    scope: story_content::ir::Scope::Anywhere,
                    query: vec![story_content::ir::QueryTerm::Name("tool".into())],
                },
                direction: Direction::Maximize,
            },
            Objective {
                expr: Expr::Legacy("score".into()),
                direction: Direction::Maximize,
            },
        ],
    );
    cfg.explore.max_states = 2_000;
    cfg.explore.max_time = Some(30);
    let report = analyze(&a, &cfg);
    // The frontier stays the two plain strategies; the op recipe is
    // dominated off it (same tool cost as `rich`, lower score).
    let mut points: Vec<Vec<f64>> = report.frontier.iter().map(|p| p.values.clone()).collect();
    points.sort_by(|a, b| a.partial_cmp(b).unwrap());
    assert_eq!(points, vec![vec![1.0, 3.0], vec![2.0, 1.0]], "{points:?}");
    // The minimal cut covers both plain witnesses — yet the cut does NOT
    // verify: the op recipe still reaches the goal under the analyzer's
    // own registry. A registry-blind neutral boot would fail every host-op
    // commit and falsely verify the cut.
    assert_eq!(report.minimal_recipe_cuts.len(), 1, "{:?}", report.minimal_recipe_cuts);
    assert!(
        report.verified_cuts.is_empty(),
        "the op strategy survives the cut: the honest result is unverified, {:?}",
        report.verified_cuts
    );
}
