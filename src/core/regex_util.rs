//! Shared regex plumbing: the `OnceLock` literal cache and the
//! capture-group accessors. A literal failing to compile or a guaranteed
//! group missing is a programmer error with no runtime behavior to define,
//! hence `unreachable!` (engine paths use no `unwrap`/`expect`).

use std::sync::OnceLock;

use regex::Regex;

/// Cache for the module-level regex literals. A literal failing to compile
/// is a programmer error with no runtime
/// behavior to define, hence `unreachable!` (engine
/// paths use no `unwrap`/`expect`).
#[must_use]
pub fn static_regex(
    cell: &OnceLock<Regex>,
    build: impl FnOnce() -> std::result::Result<Regex, regex::Error>,
) -> &Regex {
    cell.get_or_init(|| match build() {
        Ok(re) => re,
        Err(e) => unreachable!("static regex literal failed to compile: {e}"),
    })
}

/// The text of a capture group that ALWAYS participates in its pattern
/// (a non-participation would be a programmer error, never runtime
/// behavior). Only optional groups go through [`grp_opt`].
#[must_use]
pub fn grp_text<'t>(c: &regex::Captures<'t>, i: usize) -> &'t str {
    c.get(i)
        .map(|m| m.as_str())
        .unwrap_or_else(|| unreachable!("capture group {i} always participates in this pattern"))
}

/// Group 0's start offset (bytes; byte offsets are monotonic with
/// the UTF-16 unit offsets the `.index` compare/slice decisions use).
#[must_use]
pub fn grp0_start(c: &regex::Captures) -> usize {
    c.get(0).map(|m| m.start()).unwrap_or_else(|| unreachable!("regex group 0 is always present"))
}

/// A group's end offset (bytes) — same always-participates guarantee as
/// [`grp_text`]; only optional groups go through [`grp_opt`].
#[must_use]
pub fn grp_end(c: &regex::Captures, i: usize) -> usize {
    c.get(i)
        .map(|m| m.end())
        .unwrap_or_else(|| unreachable!("capture group {i} always participates in this pattern"))
}

/// An OPTIONAL capture group's text (`(?:…)?` — may legitimately not
/// participate; unmatched reads as `None`).
#[must_use]
pub fn grp_opt<'t>(c: &regex::Captures<'t>, i: usize) -> Option<&'t str> {
    c.get(i).map(|m| m.as_str())
}
