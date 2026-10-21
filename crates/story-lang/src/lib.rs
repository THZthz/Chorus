//! The `.sw` text DSL compiler: lexer, parser, and the two-pass lowering into
//! a validated `story_content::Content` (spec.md §3, language.md).

pub mod ast;
pub mod includes;
pub mod lexer;
pub mod lower;
pub mod parser;
pub mod span;
pub mod token_check;

pub use lower::{compile_file, compile_source, Output};
