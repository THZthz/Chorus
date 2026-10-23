//! The DesignAuditor (story-analyze spec §7.3, §10 M3 tests): planted
//! shadowing with the free-atom UNSAT proof, suppression rules, unreachable
//! content with evidence classes, and reachable runtime faults with
//! replaying witnesses.

mod common;

use std::collections::BTreeSet;

use story_analyze::audit::{audit, Evidence};
use story_analyze::{ExploreConfig, PlayerStep};
use story_core::EndingId;

/// `quick` (priority 5, no when) shadows `grand` (priority 10, any when):
/// wherever grand's `when` holds, quick matches first.
const SHADOWED: &str = r#"
game "Shadow"
  seed host
  turn 10m
  home bench

zones
  bench

legacies
  k: 0

cards
  card tool
    kind gadget  stackable
    start in bench count 1

endings
  ending won "Won" grand

verbs
  verb act committable instances 1
    recipe quick
      craftable
      duration 0
      effects
        legacy k += 1

    recipe grand
      craftable
      max 1
      duration 0
      when legacy.k >= 7
      effects
        ending won
"#;

/// Interval-disjoint `when`s do not shadow; interval-nested ones do.
const INTERVALS: &str = r#"
game "Intervals"
  seed host
  turn 10m
  home bench

zones
  bench

legacies
  k: 0

endings
  ending won "Won" grand

verbs
  verb act committable instances 1
    recipe low
      craftable
      duration 0
      when legacy.k >= 5
      effects
        narrate "low"

    recipe high
      craftable
      max 1
      duration 0
      when legacy.k >= 7
      effects
        narrate "high"

    recipe other
      craftable
      max 1
      duration 0
      when legacy.k <= 2
      effects
        narrate "other"
"#;

/// A decaying consumable: the recipe's `when` reads the count at open,
/// the query consume resolves at completion — the unbound card decays in
/// between (bound cards never decay, so the race needs the unbound one).
const FAULT: &str = r#"
game "Fault"
  seed host
  turn 10m
  home bench

zones
  bench

aspects
  charge: 0

legacies
  k: 0

cards
  card spark
    kind gadget  stackable
    aspects charge 1
    lifetime 20m
    start in bench count 1

endings
  ending won "Won" grand

verbs
  verb use committable instances 1
    recipe drain
      craftable
      duration 30m
      when count(anywhere, aspect:charge) >= 1
      effects
        consume aspect:charge 1
        legacy k += 1

  verb idle committable instances 1
    recipe hum
      craftable
      duration 0
      effects
        narrate "hums"
"#;

/// An autonomous start that passes candidacy (per-slot counts) but fails
/// greedy materialization (two slots compete for one card).
const SKIPPED: &str = r#"
game "Skipped"
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
    start in bench count 1

legacies
  k: 0

verbs
  verb pair autonomous instances 1
    recipe double
      duration 10m
      slots
        left: 1 aspect:charge
        right: 1 aspect:charge
      effects
        legacy k += 1
"#;

fn explore() -> ExploreConfig {
    ExploreConfig {
        max_states: 2_000,
        max_time: Some(120),
        ..Default::default()
    }
}

#[test]
fn a_when_true_recipe_shadows_every_later_recipe() {
    let content = common::compile(SHADOWED);
    let mut a = common::analyzer(content, explore_seed());
    let report = audit(&mut a, &explore());
    let hit = report
        .shadowing
        .iter()
        .find(|s| s.shadowed == "grand" && s.shadower == "quick");
    assert!(hit.is_some(), "{:?}", report.shadowing);
    let hit = hit.unwrap();
    assert_eq!(hit.verb.as_ref(), Some(&story_core::VerbId::from("act")));
    assert!(!hit.conditional, "no once/max suppression applies");
}

fn explore_seed() -> u64 {
    1
}

#[test]
fn interval_implication_proves_and_refutes() {
    let content = common::compile(INTERVALS);
    let mut a = common::analyzer(content, explore_seed());
    let report = audit(&mut a, &explore());
    // `high` (k >= 7) is shadowed by `low` (k >= 5): the obligation
    // `k >= 7 ∧ ¬(k >= 5)` is interval-empty.
    assert!(report
        .shadowing
        .iter()
        .any(|s| s.shadower == "low" && s.shadowed == "high"));
    // `other` (k <= 2) never implies nor is implied by `low`: disjoint.
    assert!(!report
        .shadowing
        .iter()
        .any(|s| s.shadower == "low" && s.shadowed == "other"));
    assert!(!report
        .shadowing
        .iter()
        .any(|s| s.shadower == "other" && s.shadowed == "low"));
}

#[test]
fn max_caps_and_allow_shadow_suppress() {
    // `allow shadow` on the shadowed recipe suppresses the finding.
    let src = SHADOWED.replace(
        "recipe grand\n      craftable",
        "recipe grand\n      craftable\n      allow shadow",
    );
    let content = common::compile(&src);
    let mut a = common::analyzer(content, explore_seed());
    let report = audit(&mut a, &explore());
    assert!(
        !report.shadowing.iter().any(|s| s.shadowed == "grand"),
        "allow shadow suppresses the finding"
    );

    // A `max` cap on the shadower suppresses too: the cap spends, and the
    // shadow is not permanent.
    let src2 = SHADOWED.replace(
        "recipe quick\n      craftable\n      duration 0",
        "recipe quick\n      craftable\n      max 1\n      duration 0",
    );
    let content = common::compile(&src2);
    let mut a = common::analyzer(content, explore_seed());
    let report = audit(&mut a, &explore());
    assert!(
        !report.shadowing.iter().any(|s| s.shadowed == "grand"),
        "a capped shadower is spent after its firing"
    );
}

#[test]
fn a_cap_forced_past_by_the_shadowed_when_proves_shadowing() {
    // §7.3's eligibility atoms: R1 carries `max 1`, and R2's `when` forces
    // the run counter past the cap wherever it holds — R1 is spent in R2's
    // whole region, so the refusal obligation `counter >= 1 ∧ counter < 1`
    // is interval-empty and the pair is provably shadowed. The DSL has no
    // counter operand, so the obligation is planted directly in the IR.
    let mut content = (*common::compile(SHADOWED)).clone();
    for rec in content.verbs.get_mut("act").unwrap().recipes.values_mut() {
        if rec.id == story_core::RecipeId::from("act.quick") {
            rec.max = Some(1);
        }
        if rec.id == story_core::RecipeId::from("act.grand") {
            rec.when = Some(story_content::ir::Expr::Cmp {
                op: story_content::ir::CmpOp::Ge,
                left: Box::new(story_content::ir::Expr::Counter),
                right: Box::new(story_content::ir::Expr::Lit(1)),
            });
        }
    }
    let content = std::sync::Arc::new(content);
    let mut a = common::analyzer(content, explore_seed());
    let report = audit(&mut a, &explore());
    let hit = report
        .shadowing
        .iter()
        .find(|s| s.shadower == "quick" && s.shadowed == "grand");
    assert!(hit.is_some(), "{:?}", report.shadowing);
    assert!(!hit.unwrap().conditional, "the cap is spent, not spending");
}

#[test]
fn once_rules_report_conditional_shadowing() {
    let src = r#"
game "Once"
  seed host
  turn 10m
  home bench

zones
  bench

cards
  card tool
    kind gadget  stackable
    start in bench count 1
    on complete
      rule first priority 1 once
        effects
          narrate "first"
      rule second priority 2
        effects
          narrate "second"
"#;
    let content = common::compile(src);
    let mut a = common::analyzer(content, explore_seed());
    let report = audit(&mut a, &explore());
    let hit = report
        .shadowing
        .iter()
        .find(|s| s.shadowed.ends_with("second") && s.shadower.ends_with("first"));
    assert!(hit.is_some(), "{:?}", report.shadowing);
    assert!(hit.unwrap().conditional, "a once shadower is spent after firing");
}

#[test]
fn unreachable_content_carries_evidence_classes() {
    let src = r#"
game "Dead"
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
    start in bench count 1
  card relic
    kind treasure  unique

decks
  deck unused
    entries tool
    wrap

endings
  ending won "Won" grand
  ending lost "Lost" somber

verbs
  verb act committable instances 1
    recipe win
      craftable
      max 1
      duration 0
      effects
        ending won
"#;
    let content = common::compile(src);
    let mut a = common::analyzer(content, explore_seed());
    let report = audit(&mut a, &explore());

    // `relic` has no boot, no mint, and the deck that references it is
    // never drawn: statically dead.
    let relic = report
        .unreachable
        .iter()
        .find(|u| u.id == "relic")
        .expect("relic is dead");
    assert_eq!(relic.kind, "card");
    assert_eq!(relic.evidence, Evidence::StaticallyProved);

    // The unused deck is never drawn by any effect: statically dead.
    let deck = report
        .unreachable
        .iter()
        .find(|u| u.id == "unused")
        .expect("the deck is dead");
    assert_eq!(deck.kind, "deck");
    assert_eq!(deck.evidence, Evidence::StaticallyProved);

    // `lost` is never produced by any effect: statically dead.
    let lost = report
        .unreachable
        .iter()
        .find(|u| u.id == "lost")
        .expect("the ending is dead");
    assert_eq!(lost.kind, "ending");
    assert_eq!(lost.evidence, Evidence::StaticallyProved);

    // Everything reachable is absent from the findings.
    assert!(!report.unreachable.iter().any(|u| u.id == "tool"));
    assert!(!report.unreachable.iter().any(|u| u.id == "act.win"));
}

#[test]
fn the_decaying_consume_race_yields_a_replaying_fault_witness() {
    let content = common::compile(FAULT);
    let mut a = common::analyzer(content, explore_seed());
    let report = audit(&mut a, &explore());
    let faults: Vec<_> = report
        .faults
        .iter()
        .filter(|f| matches!(f.event, story_analyze::FaultEvent::ResolutionFailed { .. }))
        .collect();
    assert!(!faults.is_empty(), "the decay-outcompetes-the-drain race");
    for f in &faults {
        // Every reported witness replays (the audit verified through the
        // oracle); the fault event sits on the witness's final step.
        let last = f.witness.records.last().unwrap();
        assert!(last.events.iter().any(|e| matches!(
            e,
            story_engine::Event::ResolutionFailed { .. }
        )));
        // The race: the recipe opens at t=0, the consume fails at its
        // completion after the card decayed.
        assert!(f.witness.records.iter().any(|r| matches!(
            r.step,
            PlayerStep::Commit { .. }
        )));
    }
}

#[test]
fn the_report_carries_its_exploration_context() {
    // INTERVALS has a finite state space (no unbounded legacy writes), so
    // the generous bounds explore it whole.
    let content = common::compile(INTERVALS);
    let mut a = common::analyzer(content, explore_seed());
    let report = audit(&mut a, &explore());
    assert!(report.explored_nodes > 0, "{:?}", report.explored_nodes);
    assert!(!report.truncated, "{:?}", report.explored_nodes);

    // A one-expansion budget truncates: the findings are then explicitly
    // bounds-conditional through the report itself.
    let content = common::compile(SHADOWED);
    let mut a = common::analyzer(content, explore_seed());
    let report = audit(
        &mut a,
        &ExploreConfig {
            max_states: 1,
            ..explore()
        },
    );
    assert!(report.truncated, "{:?}", report.explored_nodes);
    assert!(report.explored_nodes > 0, "{:?}", report.explored_nodes);
}

#[test]
fn start_skipped_materialization_is_reachable() {
    let content = common::compile(SKIPPED);
    let mut a = common::analyzer(content, explore_seed());
    let report = audit(&mut a, &explore());
    let faults: Vec<_> = report
        .faults
        .iter()
        .filter(|f| {
            matches!(
                f.event,
                story_analyze::FaultEvent::StartSkipped { .. }
            )
        })
        .collect();
    assert!(!faults.is_empty(), "the boot checkpoint skips the double");
    // The witness replays and reaches the skip within the first advance.
    let f = &faults[0];
    assert!(f
        .witness
        .records
        .iter()
        .any(|r| r.events.iter().any(|e| matches!(
            e,
            story_engine::Event::StartSkipped { .. }
        ))));
    let _ = EndingId::from("won");
    let _ = BTreeSet::<String>::new();
}
