//! The content layer: `script/` is the line-segment script parser +
//! load-time lint; `model.rs` is the §6 content model (authored + compiled
//! types); `compile.rs` is the compiler; `deck.rs` hosts the §4.4 deck
//! machinery; `functions.rs` hosts the §10 function table.

mod compile;
mod deck;
mod functions;
mod model;
pub mod script;

// the crate-facing content API, one flat namespace: every authored and
// compiled type from `model`, the deck draw rule from `deck`, the compiler
// entry points from `compile`.
pub use compile::{compile_content, for_each_authored_outcome, walk_outcomes};
pub use deck::{draw_from, permute_positions};
pub use model::*;

pub(crate) use compile::{collect_stackable, for_each_authored_outcome_infallible, mint_recipe_id};
