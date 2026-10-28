//! M2 behavioral matrix (kernel-api.md §10): every mechanism entry carries at
//! least one positive and one refusal path, tagged `// matrix <NN>` so coverage
//! is greppable. The testable invariants T1-T14 (§9) close the file: each
//! carries an executable assertion here or a forwarding comment naming the
//! test that owns it.

mod common;

use common::{aspect, boot, compile, count_named, narration, recipe_runs, situations, state_bytes};
use story_content::code;
use story_content::runtime::DiagKind;
use story_core::{EntityId, SituationId, SlotName};
use story_engine::{Engine, Event, StartSkipReason};

#[path = "matrix/minting.rs"]
mod minting;
#[path = "matrix/slots.rs"]
mod slots;
#[path = "matrix/presence.rs"]
mod presence;
#[path = "matrix/starts.rs"]
mod starts;
#[path = "matrix/instant.rs"]
mod instant;
#[path = "matrix/decay.rs"]
mod decay;
#[path = "matrix/targeting.rs"]
mod targeting;
#[path = "matrix/books.rs"]
mod books;
#[path = "matrix/compile_steps.rs"]
mod compile_steps;
#[path = "matrix/handles.rs"]
mod handles;
#[path = "matrix/rolls.rs"]
mod rolls;
#[path = "matrix/relations_matrix.rs"]
mod relations_matrix;
#[path = "matrix/invariants.rs"]
mod invariants;

fn game(src: &str) -> Engine {
    boot(compile(src), 1)
}

/// All live ids of one card name, mint order.
fn ids_of(engine: &Engine, name: &str) -> Vec<EntityId> {
    engine
        .state()
        .entities
        .values()
        .filter(|e| e.name == name)
        .map(|e| e.id)
        .collect()
}

fn zone_of(engine: &Engine, e: EntityId) -> story_core::ZoneId {
    engine.state().entities[&e].zone
}

/// The live situation running `recipe` (exactly one expected).
fn sit_of(engine: &Engine, recipe: &str) -> SituationId {
    engine
        .state()
        .situations
        .values()
        .find(|s| s.recipe.as_str() == recipe)
        .map(|s| s.id)
        .unwrap_or_else(|| panic!("no live situation of `{recipe}`"))
}

fn err_kind(r: Result<Vec<Event>, story_content::RuntimeDiagnostic>) -> DiagKind {
    r.expect_err("expected a refusal").kind
}

fn completed_count(events: &[Event]) -> usize {
    events
        .iter()
        .filter(|e| matches!(e, Event::SituationCompleted { .. }))
        .count()
}

fn chain_ended(events: &[Event], verb: &str) -> bool {
    events
        .iter()
        .any(|e| matches!(e, Event::ChainEnded { verb: v } if v.as_str() == verb))
}

fn start_skipped(events: &[Event], verb: &str, reason: &StartSkipReason) -> bool {
    events.iter().any(|e| matches!(e, Event::StartSkipped { verb: v, reason: r } if v.as_str() == verb && r == reason))
}

/// Compile refusals, as stable diagnostic numbers.
fn compile_error_codes(src: &str) -> Vec<u32> {
    story_lang::compile_source("matrix", src)
        .diagnostics
        .iter()
        .filter(|d| d.is_error())
        .map(|d| d.number)
        .collect()
}
