//! The engine kernel: RNG (dice), clock and durations, the state data model
//! (types), references and relations (ref), roles, the dictionary, query
//! evaluation, predicates, and the sentence pipeline (sentence → lexer →
//! parse → fn_expand → format). `text`/`jsonw` are the frozen
//! whitespace/collation classes and the canonical JSON surface;
//! `regex_util` is the shared regex plumbing; `num_parse` is
//! authored-integer parsing.
//!
//! `r#ref` is spelled with the raw identifier because `ref` is a Rust
//! keyword. `Role`/`ROLES` live in `roles` and are re-exported by
//! `sentence`.

pub mod dice;
pub mod dict;
pub mod r#ref;
pub mod clock;
pub mod query;
pub mod sentence;
pub mod predicate;
pub mod parse;
pub mod fn_expand;
pub mod format;
pub mod lexer;
pub mod roles;
pub mod types;
pub mod text;
pub mod jsonw;
pub mod regex_util;
pub mod num_parse;
