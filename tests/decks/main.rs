// the shared test infrastructure, mounted ONCE per binary — every module
// reaches it as `crate::common::…`
#[path = "../common/mod.rs"]
mod common;

mod decks;
mod stackable;
mod unique;
mod uniquegroup;
