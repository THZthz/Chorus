//! ts: src/core/* — one Rust module per TS file (plan §2.2).
//! W0: dice; W1 adds roles/clock/types/ref/dict/query/predicate/
//! sentence/lexer/parse/fn_expand/format. (W0's `deck_rng` staging module
//! folded into `content/content.rs` in W2, per plan §2.2.)
//!
//! `ref.ts` lands as `core::r#ref` (`ref` is a Rust keyword); the file name
//! keeps the TS mapping. `Role/ROLES` are sunk into `roles` (CORE-02) and
//! re-exported by `sentence` to mirror the TS surface.

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
