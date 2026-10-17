// the shared test infrastructure, mounted ONCE per binary — every module
// reaches it as `crate::common::…`
#[path = "../common/mod.rs"]
mod common;

mod clock;
mod conditionals;
mod fn_expand_direct;
mod functions;
mod gate;
mod kinds;
mod lint;
mod ops;
mod query_predicate;
mod sentence;
mod verbs;
