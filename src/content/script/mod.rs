//! The script layer: a game script is a piece of
//! text — meta lines, section blocks, and sentence-like declarations
//! (spec: "sentences + small blocks"). [`parse_script`] is the
//! indentation-sensitive line-segment parser; [`compile_script`] turns a
//! script into runnable game content + the initial State; [`lint_script`] is
//! the §10 warn-loudly sibling (it never throws for a script that compiles).
//!
//! Regex-domain freeze: every `(.+)`-style pattern in
//! this module runs on `split('\n')` +
//! [`dsl_trim`](crate::core::text::dsl_trim)-trimmed SINGLE-LINE
//! text, so `.` never sees `\r`/U+2028/U+2029 (line terminators are split
//! away or trimmed by the frozen whitespace set); these patterns must NOT
//! be reused on multi-line input.
//!
//! Whitespace discipline (Q8): every `\s`/`\S` in these patterns is the
//! explicit frozen class
//! ([`DSL_WS_CLASS`](crate::core::text::DSL_WS_CLASS) /
//! [`DSL_NON_WS_CLASS`](crate::core::text::DSL_NON_WS_CLASS)) and every
//! `trim()`/`trimStart()` is
//! [`dsl_trim`](crate::core::text::dsl_trim) / an
//! [`is_dsl_ws_char`](crate::core::text::is_dsl_ws_char) take-while —
//! the frozen set and `char::is_whitespace` disagree on U+0085/U+FEFF
//! exactly reversely.

mod complete;
mod decls;
mod lint;
mod load;
mod parse;
mod types;
mod verbblock;
mod when_dead;

pub use lint::{lint_script, lint_script_findings, LintFinding, LintSeverity};
pub use load::compile_script;
pub use parse::parse_script;
pub use types::{CompiledScript, EntitySpec, RelationSpec, Script, ScriptMeta};
