//! The `table` scope (spec.md §4.7): every card in a public zone plus every
//! situation's non-greedy slot bindings; greedy bindings and situation
//! interiors are excluded.

mod common;

use common::{boot, compile, narration};
use story_content::runtime::DiagKind;

const SRC: &str = r#"
game "TableScope"
  seed host
  turn 1m
  home hall

zones
  hall

aspects
  spark: 0

cards
  card loose
    stackable
    aspects spark 1
    start in hall count 1
  card morsel
    stackable
    aspects spark 1
    start in hall count 1
  card fuel
    stackable
    aspects spark 1
    start in hall count 1

verbs
  verb holder committable instances 1
    recipe keep
      duration 100m
      slots
        kept: 1..1 aspect:spark
  verb snapper committable instances 1
    recipe grab
      duration 100m
      slots
        snag: 1..1 greedy aspect:spark
  verb probe2 committable instances any
    recipe two
      duration 0
      when count(table, aspect:spark) >= 2
      effects
        narrate "table-holds-two"
  verb probe3 committable instances any
    recipe three
      duration 0
      when count(table, aspect:spark) >= 3
      effects
        narrate "table-holds-three"
"#;

/// `table` counts world cards plus another situation's non-greedy bindings,
/// and excludes greedy bindings and interiors: with one world card, one
/// non-greedy binding, and one greedy binding, the count reads exactly 2 —
/// high enough for the 2-probe, too low for the 3-probe (which would pass if
/// either the greedy binding or an interior card leaked in).
#[test]
fn table_scope_counts_world_plus_non_greedy_bindings_only() {
    let mut e = boot(compile(SRC), 1);
    common::commit_named(&mut e, "holder", &[("kept", "morsel")]).expect("morsel bound");
    common::commit_named(&mut e, "snapper", &[("snag", "fuel")]).expect("fuel bound greedy");

    // Two: the world card (loose) and the non-greedy binding (morsel).
    let events = common::commit_named(&mut e, "probe2", &[]).expect("count(table) >= 2");
    assert_eq!(narration(&events), vec!["table-holds-two"]);

    // Three fails: the greedy binding (fuel) and both interiors are excluded.
    let err = common::commit_named(&mut e, "probe3", &[]).expect_err("count(table) < 3");
    assert_eq!(err.kind, DiagKind::NoRecipe);
}

/// `table` is live: a card released into a situation's interior storage stops
/// counting, and the interior leftovers returning home re-enters the count.
#[test]
fn table_scope_tracks_bindings_as_they_move() {
    let src = r#"
game "TableMove"
  seed host
  turn 1m
  home hall

zones
  hall

aspects
  spark: 0

cards
  card token
    stackable
    aspects spark 1
    start in hall count 1

verbs
  verb keeper committable instances 1
    recipe hold
      duration 10m
      slots
        kept: 1..1 aspect:spark
  verb probe1 committable instances any
    recipe one
      duration 0
      when count(table, aspect:spark) >= 1
      effects
        narrate "table-holds-one"
"#;
    let mut e = boot(compile(src), 1);
    // While bound (non-greedy), the token counts on the table.
    common::commit_named(&mut e, "keeper", &[("kept", "token")]).expect("bound");
    let events = common::commit_named(&mut e, "probe1", &[]).expect("counts while bound");
    assert_eq!(narration(&events), vec!["table-holds-one"]);

    // Released home at completion, it counts again — as a world card.
    e.advance(10).expect("the keeper completes");
    let events = common::commit_named(&mut e, "probe1", &[]).expect("counts after release");
    assert_eq!(narration(&events), vec!["table-holds-one"]);
}
