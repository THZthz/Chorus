// the shared test infrastructure, mounted ONCE per binary — every module
// reaches it as `crate::common::…`
#[path = "../common/mod.rs"]
mod common;

mod ambient;
mod completion_triggers;
mod demo;
mod endings;
mod engine_core;
mod expiry;
mod held;
mod max_executions;
mod ordering;
mod recipe_dur;
mod rig;
mod run_chains;
mod set_ops;
mod trigger_chains;
