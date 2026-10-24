//! Kernel behavior tests over small fixtures: completion order (T12), the
//! per-unit failure policy (T4), instant recipes (inline open+complete, the
//! recursion budget, the autonomous pulse), commit atomicity (T3), total
//! locks (T8), context-free expiry books (T9), and the chain failure
//! taxonomy (T10).

mod common;

use common::{boot, compile, count_named, narration, recipe_runs, situations, state_bytes};
use story_content::runtime::DiagKind;
use story_core::EntityId;
use story_engine::{CommitAction, EffectRegistry, Engine, Event, StartSkipReason};

#[path = "kernel/order.rs"]
mod order;
#[path = "kernel/interior_books.rs"]
mod interior_books;
#[path = "kernel/failure.rs"]
mod failure;
#[path = "kernel/instant.rs"]
mod instant;
#[path = "kernel/atomicity.rs"]
mod atomicity;
#[path = "kernel/expiry.rs"]
mod expiry;
#[path = "kernel/chains.rs"]
mod chains;
#[path = "kernel/handoff.rs"]
mod handoff;
#[path = "kernel/recognition.rs"]
mod recognition;
#[path = "kernel/relations.rs"]
mod relations;

fn game(src: &str) -> Engine {
    boot(compile(src), 1)
}
