//! The engine binary's seed-world fixture rig — shared by engine_core,
//! expiry and set_ops. The spec §6.4 flagship examples as data
//! (`crate::common::seed`), compiled fresh per test, plus the record fixtures
//! the regression literals hand-build.

// every engine file uses a different subset of the rig
#![allow(dead_code)]

use crate::common::seed::{seed_content, seed_state};

use chorus::content::{compile_content, CompiledContent};
use chorus::core::clock::Clock;
use chorus::core::dict::{core_dictionary, Dictionary};
use chorus::core::sentence::{Arg, Outcome, Sentence};
use chorus::core::types::{add_entity, AspectVal, Aspects, AttrValue, Attrs, Entity, State};
use chorus::engine::{ActionRecord, Step};

/// The file-level fixture: a fresh `dict`/`content` pair per test,
/// semantically identical because both are immutable.
pub fn rig() -> (Dictionary, CompiledContent) {
    let dict = core_dictionary();
    let content = compile_content(&seed_content(), &dict, &[]).unwrap();
    (dict, content)
}

pub fn setup() -> (Dictionary, CompiledContent, State) {
    let (dict, content) = rig();
    (dict, content, seed_state())
}

/// the step clock defaults to day 0 / tick 0 when the sentence omits one.
pub fn step_of(s: &Sentence) -> Step {
    Step {
        start_min: None,
        clock: Some(s.clock.unwrap_or(Clock { day: 0, tick: 0 })),
        clauses: s.clauses.clone(),
        t_real: None,
    }
}

/// Plain entity fixture shape.
#[allow(clippy::too_many_arguments)]
pub fn plain_card(
    s: &mut State,
    id: &str,
    kind: &str,
    name: &str,
    aspects: Aspects,
    attrs: Attrs,
    stackable: bool,
) {
    add_entity(
        s,
        Entity {
            id: id.to_string(),
            kind: kind.to_string(),
            name: name.to_string(),
            aspects,
            attrs,
            born_action: None,
            dead_action: None,
            stackable,
        },
    )
    .unwrap();
}

pub fn num_aspect(k: &str, v: i64) -> Aspects {
    [(k.to_string(), AspectVal::Num(v))].into_iter().collect()
}

pub fn container_attrs(container: &str) -> Attrs {
    [("container".to_string(), AttrValue::Str(container.to_string()))].into_iter().collect()
}

/// A hand-built COMPLETE record like the regression literals (id, kind
/// "complete", trigger None, t_game set, t_real/start_min None, window
/// "fixture", advanced_min 0).
pub fn fixture_complete(id: i64, t_game: &str, outcomes: Vec<Outcome>) -> ActionRecord {
    ActionRecord {
        id,
        kind: "complete".to_string(),
        trigger: None,
        t_game: t_game.to_string(),
        t_real: None,
        start_min: None,
        window: Some("fixture".to_string()),
        advanced_min: 0,
        actors: vec![],
        verb: String::new(),
        objects: vec![],
        instruments: vec![],
        at: None,
        about: None,
        to: None,
        held: None,
        outcomes,
        recipes: vec![],
        sentence: String::new(),
    }
}

pub fn outcome(op: &str, args: Vec<Arg>) -> Outcome {
    Outcome { op: op.to_string(), args, arms: None, fallback: None, call: None }
}
