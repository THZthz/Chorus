//! The engine kernel: RNG (dice), clock and durations, the state data model
//! (types), references and relations (ref), roles, the dictionary, query
//! evaluation, predicates (`predicate` is the AST + evaluator, with its
//! `pred_parse`/`pred_lint` seams re-exported through it), and the sentence
//! pipeline (sentence → lexer → parse → fn_expand → format). `text`/`jsonw`
//! are the frozen whitespace/collation classes and the canonical JSON
//! surface; `regex_util` is the shared regex plumbing; `num_parse` is
//! authored-integer parsing.
//!
//! `r#ref` is spelled with the raw identifier because `ref` is a Rust
//! keyword. `Role`/`ROLES` live in `roles` and are re-exported by
//! `sentence`.

pub mod clock;
pub mod dice;
pub mod dict;
pub mod fn_expand;
pub mod format;
pub mod jsonw;
pub mod lexer;
pub mod num_parse;
pub mod parse;
mod pred_lint;
mod pred_parse;
pub mod predicate;
pub mod query;
pub mod r#ref;
pub mod regex_util;
pub mod roles;
pub mod sentence;
pub mod text;
pub mod types;
