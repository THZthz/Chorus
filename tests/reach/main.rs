// the shared test infrastructure, mounted ONCE per binary — every module
// reaches it as `crate::common::…`
#[path = "../common/mod.rs"]
mod common;

mod reach;
mod reach_adversarial;
mod reach_fuzz;
mod reach_scale;
