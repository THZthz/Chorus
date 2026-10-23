//! The `lang` feature (story-analyze spec §1.1): query goals compiled by
//! the real story-lang pipeline — `.sw` condition syntax plus inline
//! declared predicates.

#![cfg(feature = "lang")]

use story_analyze::lang::{parse_cond, parse_goal};
use story_content::ir::Expr;

/// The declarations the sample conditions read.
const DECLS: &[&str] = &[
    "aspects\n  heat: 0\n  hunger: 0\n  restore: 0",
    "cards\n  card beacon\n    kind fixture  unique\n  card warden\n    kind person  unique",
    "legacies\n  dawns_seen: 0",
];

#[test]
fn conditions_lower_to_engine_expressions() {
    let e = parse_cond(DECLS, &[], "beacon.heat >= 6").expect("compiles");
    assert!(matches!(e, Expr::Cmp { .. }));

    // Aggregates, arithmetic, logic, and negation lower too.
    let e = parse_cond(DECLS, &[], "clock >= 30 and not beacon.heat >= 1").ok();
    assert!(e.is_some());
}

#[test]
fn declared_predicates_inline_into_the_goal() {
    let e = parse_cond(
        DECLS,
        &["predicate starving(p) = p.hunger >= 3 and p.restore < 1"],
        "starving(warden) and legacy.dawns_seen >= 1",
    )
    .expect("the predicate inlines");
    // The lowered tree carries no predicate calls — everything inlined.
    let rendered = format!("{e:?}");
    assert!(!rendered.contains("starving"));
}

#[test]
fn syntax_errors_come_back_as_errors() {
    assert!(parse_cond(DECLS, &[], "beacon.heat >=").is_err());
    assert!(parse_cond(&[], &[], "unknown_aspect(1").is_err());
}

#[test]
fn parsed_goals_drive_queries() {
    let goal = parse_goal(DECLS, &[], "clock >= 30").expect("compiles");
    let mut q = story_analyze::ReachQuery::new(goal);
    q.max_states = 100;
    // The parsed goal's thresholds join Tcrit (§5.4).
    assert_eq!(q.clock_thresholds(), std::collections::BTreeSet::from([30]));
}
