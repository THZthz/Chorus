//! Tests for modular slicing and assume-guarantee reasoning (story-analyze spec).

mod common;

use story_analyze::causal::{CausalGraph, Fact};
use story_analyze::slice::{
    apply_assumption, compose_witnesses, verify_composition, Assumption, InterfaceBoundary,
    SliceContract, SliceTarget, SlicedAnalyzer,
};
use story_analyze::transitions::WitnessPath;
use story_analyze::GoalPredicate;
use story_content::ir::{CmpOp, Expr};
use story_core::{CardId, EndingId, State, VerbId};

const QUEST_SRC: &str = r#"
game "ModularQuest"
  seed host
  turn 10m
  home workshop

zones
  workshop

aspects
  sharpness: 0

legacies
  glory: 0

endings
  ending crowned "Crowned" grand

cards
  card ore
    kind matter  unique
  card blade
    kind matter  unique
    aspects sharpness 1
  card sword
    kind matter  unique

verbs
  verb gather committable instances 1
    recipe mine
      priority 10  craftable
      duration 0
      effects
        spawn ore into workshop

  verb forge committable instances 1
    recipe hammer
      priority 10  craftable
      duration 0
      when alive(ore) == 1
      effects
        consume ore
        spawn blade into workshop

  verb quench committable instances 1
    recipe temper
      priority 10  craftable
      duration 0
      when alive(blade) == 1
      effects
        consume blade
        spawn sword into workshop
        legacy glory += 10

  verb reign committable instances 1
    recipe ascend
      priority 10  craftable
      duration 0
      effects
        consume sword
        ending crowned
"#;

#[test]
fn interface_boundary_partitions_slice_facts() {
    let content = common::compile(QUEST_SRC);
    let graph = CausalGraph::build(&content);

    // Slice: forge
    let target = SliceTarget::single(VerbId::from("forge"));
    let boundary = InterfaceBoundary::extract(&graph, &target);

    // Forge requires `ore` (which is not produced at boot or by forge itself).
    assert!(
        boundary.inputs.contains(&Fact::CardLive(CardId::from("ore"))),
        "forge must require external ore"
    );

    // Forge produces `blade` which is consumed outside by quench.
    assert!(
        boundary
            .outputs
            .contains(&Fact::CardLive(CardId::from("blade"))),
        "blade is an output consumed by quench"
    );

    // Slice: reign
    let target_reign = SliceTarget::single(VerbId::from("reign"));
    let boundary_reign = InterfaceBoundary::extract(&graph, &target_reign);

    assert!(
        boundary_reign
            .inputs
            .contains(&Fact::CardLive(CardId::from("sword"))),
        "reign requires sword forged externally"
    );
    assert!(
        boundary_reign
            .outputs
            .contains(&Fact::EndingReached(EndingId::from("crowned"))),
        "reign outputs crowned ending"
    );
}

#[test]
fn arc_slice_combines_verbs() {
    let content = common::compile(QUEST_SRC);
    let graph = CausalGraph::build(&content);

    // Arc of forge + quench together
    let target = SliceTarget::arc([VerbId::from("forge"), VerbId::from("quench")]);
    let boundary = InterfaceBoundary::extract(&graph, &target);

    // Input to arc is ore
    assert!(boundary.inputs.contains(&Fact::CardLive(CardId::from("ore"))));
    // Output of arc is sword (consumed by external reign)
    assert!(boundary
        .outputs
        .contains(&Fact::CardLive(CardId::from("sword"))));
    // Internal to arc is blade (produced by forge, consumed by quench, not external)
    assert!(boundary
        .internal
        .contains(&Fact::CardLive(CardId::from("blade"))));
}

#[test]
fn apply_assumption_seeds_starting_state() {
    let content = common::compile(QUEST_SRC);
    let base = common::analyzer(content.clone(), 1);
    let mut state = base.start().clone();

    // Rejecting >1 copies of unique cards
    let invalid_assump = Assumption::Card {
        id: CardId::from("blade"),
        count: 2,
    };
    assert!(apply_assumption(&mut state, &content, &invalid_assump).is_err());

    let card_assump = Assumption::Card {
        id: CardId::from("blade"),
        count: 1,
    };
    apply_assumption(&mut state, &content, &card_assump).unwrap();

    let blades: Vec<_> = state
        .entities
        .values()
        .filter(|e| e.name == "blade")
        .collect();
    assert_eq!(blades.len(), 1);

    let legacy_assump = Assumption::Legacy {
        key: "glory".to_string(),
        value: 42,
    };
    apply_assumption(&mut state, &content, &legacy_assump).unwrap();
    assert_eq!(state.legacy.get("glory"), Some(&42));

    let aspect_assump = Assumption::Aspect {
        card: CardId::from("blade"),
        key: "sharpness".to_string(),
        value: 5,
    };
    apply_assumption(&mut state, &content, &aspect_assump).unwrap();
    let first_blade = state
        .entities
        .values()
        .find(|e| e.name == "blade")
        .unwrap();
    assert_eq!(first_blade.aspects.get("sharpness"), Some(&5));
}

#[test]
fn verify_composition_validates_contract_chains() {
    let content = common::compile(QUEST_SRC);

    let contracts = vec![
        SliceContract {
            name: "gather_arc".to_string(),
            target: SliceTarget::single(VerbId::from("gather")),
            assumptions: vec![],
            guarantee: GoalPredicate::Expr(Expr::Alive(CardId::from("ore"))),
        },
        SliceContract {
            name: "forge_arc".to_string(),
            target: SliceTarget::single(VerbId::from("forge")),
            assumptions: vec![Assumption::Card {
                id: CardId::from("ore"),
                count: 1,
            }],
            guarantee: GoalPredicate::Expr(Expr::Alive(CardId::from("blade"))),
        },
        SliceContract {
            name: "quench_arc".to_string(),
            target: SliceTarget::single(VerbId::from("quench")),
            assumptions: vec![Assumption::Card {
                id: CardId::from("blade"),
                count: 1,
            }],
            guarantee: GoalPredicate::Expr(Expr::Cmp {
                op: CmpOp::Ge,
                left: Box::new(Expr::Legacy("glory".to_string())),
                right: Box::new(Expr::Lit(10)),
            }),
        },
        SliceContract {
            name: "reign_arc".to_string(),
            target: SliceTarget::single(VerbId::from("reign")),
            assumptions: vec![Assumption::Legacy {
                key: "glory".to_string(),
                value: 10,
            }],
            guarantee: GoalPredicate::Ending(Some(EndingId::from("crowned"))),
        },
    ];

    let report = verify_composition(&content, &contracts);
    assert!(report.satisfied, "contract chain should be fully satisfied");
    assert!(report.violations.is_empty());
    assert!(!report.discharged.is_empty());

    // Omitting the forge contract leaves quench with an unsatisfied assumption (blade).
    let broken = vec![contracts[0].clone(), contracts[2].clone()];
    let broken_report = verify_composition(&content, &broken);
    assert!(!broken_report.satisfied);
    assert_eq!(broken_report.violations.len(), 1);
    assert_eq!(broken_report.violations[0].contract_name, "quench_arc");
    assert_eq!(
        broken_report.violations[0].missing_assumption,
        "CardLive(blade)"
    );
}

#[test]
fn sliced_analyzer_solves_under_assumptions() {
    let content = common::compile(QUEST_SRC);
    let base = common::analyzer(content, 1);

    // Quench slice under assumed blade
    let target = SliceTarget::single(VerbId::from("quench"));
    let assumptions = vec![Assumption::Card {
        id: CardId::from("blade"),
        count: 1,
    }];
    let mut sliced = SlicedAnalyzer::new(&base, target, &assumptions).unwrap();

    let mut query = story_analyze::ReachQuery::new(GoalPredicate::Expr(Expr::Alive(CardId::from("sword"))));
    let outcome = sliced.solve(&mut query);

    assert!(
        !outcome.witnesses.is_empty(),
        "quench should reach sword when blade assumption is provided"
    );
    let witness = &outcome.witnesses[0];
    assert_eq!(witness.actions, 1);
}

#[test]
fn witness_splicing_combines_paths() {
    let w1 = WitnessPath {
        start: State::default(),
        records: vec![],
        depth: 1,
        actions: 2,
        probability: Some(0.8),
    };
    let w2 = WitnessPath {
        start: State::default(),
        records: vec![],
        depth: 2,
        actions: 3,
        probability: Some(0.5),
    };

    let composite = compose_witnesses(&w1, &w2);
    assert_eq!(composite.actions, 5);
    assert_eq!(composite.depth, 0);
    assert!((composite.probability.unwrap() - 0.4).abs() < 1e-6);
}

#[test]
fn unreachable_goal_terminates_immediately_without_advance_hang() {
    let content = common::compile(QUEST_SRC);
    let base = common::analyzer(content, 1);

    // Quench slice without blade assumption: temper cannot commit
    let target = SliceTarget::single(VerbId::from("quench"));
    let mut sliced = SlicedAnalyzer::new(&base, target, &[]).unwrap();

    let mut query = story_analyze::ReachQuery::new(GoalPredicate::Expr(Expr::Alive(CardId::from("sword"))));
    let start = std::time::Instant::now();
    let outcome = sliced.solve(&mut query);
    assert!(outcome.witnesses.is_empty());
    // Quiescence pruning suppresses advance on idle state; search halts in 1 expansion
    assert_eq!(outcome.expansions, 1);
    assert!(start.elapsed() < std::time::Duration::from_secs(1));
}
