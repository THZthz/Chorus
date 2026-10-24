//! The relations primitive (kernel-api.md §10 entries 35-44, spec.md §4.9):
//! edge writes and reads, symmetry, exclusivity, enumeration, boot edges,
//! and the destroy/move/snapshot interactions.

mod common;

use common::{boot, compile, narration};
use story_core::{EntityId, Relations};
use story_engine::{Engine, Event};

#[path = "relations/edges.rs"]
mod edges;
#[path = "relations/queries.rs"]
mod queries;
#[path = "relations/boot.rs"]
mod boot;
#[path = "relations/persistence.rs"]
mod persistence;

fn game(src: &str) -> Engine {
    boot(compile(src), 1)
}

fn ids_of(engine: &Engine, name: &str) -> Vec<EntityId> {
    engine
        .state()
        .entities
        .values()
        .filter(|e| e.name == name)
        .map(|e| e.id)
        .collect()
}

fn edge_count(relations: &Relations) -> usize {
    relations
        .values()
        .map(|kinds| kinds.values().map(|o| o.len()).sum::<usize>())
        .sum()
}

fn related_of(e: &Engine, subject: EntityId, kind: &str) -> Vec<EntityId> {
    e.state()
        .relations
        .get(&subject)
        .and_then(|kinds| kinds.get(kind))
        .map(|o| o.iter().copied().collect())
        .unwrap_or_default()
}

/// One effect-runner verb: a 0-duration recipe `go` whose effects are spliced
/// in, so a commit named `act` applies them and returns the events.
fn runner(effects: &str) -> String {
    format!(
        r#"
game "Rel"
  seed host
  turn 1m
  home hall

zones
  hall
  vault

aspects
  worn: 0
  weight: 0
  spark: 1

relations
  carries exclusive
  married symmetric exclusive
  knows
  north_of

cards
  card alice
    unique
    aspects spark 1
    start in hall

  card bob
    unique
    start in hall

  card sword
    unique
    aspects weight 2
    start in hall

  card coin
    stackable
    aspects weight 1
    start in hall count 2

verbs
  verb act committable instances 1
    recipe go
      duration 0
      effects
{effects}

  verb probe committable instances any
    recipe look
      duration 0
      effects
        narrate "looked"
"#
    )
}
