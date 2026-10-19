// the shared test infrastructure, mounted ONCE per binary — every module
// reaches it as `crate::common::…`
#[path = "../common/mod.rs"]
mod common;

mod cli;
mod common_smoke;
mod recipe_dur;
mod repl;
mod script_play;
mod session_smoke;
