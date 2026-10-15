//! Assertion helpers shared by every integration test. Every test file mounts
//! this through `mod common;` so assertion discipline cannot drift between
//! files: `assert_err_msg` / `assert_json_eq` here; the full-state snapshot
//! (`snap`), the settle adapter and the seed fixtures live in their own
//! `tests/common/` modules.

// each integration test crate compiles this module separately and not every
// helper is used by every file (e.g. fn_expand_direct uses assert_err_msg
// only) — the helpers are the shared discipline, not per-file dead code
#![allow(dead_code)]

use chorus::core::dict::Dictionary;
use chorus::core::format::format_sentence;
use chorus::core::parse::parse_sentence;
use chorus::js::ChorusError;

/// The ChorusError message must
/// match `pattern` (patterns are JS regex literals copied CHARACTER FOR
/// CHARACTER — none of the repo's patterns need lookaround/backrefs).
/// If a pattern ever needs a Rust-regex escaping adjustment, register the
/// adjustment HERE in one place; none are needed so far.
pub fn assert_err_msg<T>(res: chorus::js::Result<T>, pattern: &str) {
    let err: ChorusError = match res {
        Ok(_) => panic!("expected error matching /{pattern}/, got Ok"),
        Err(e) => e,
    };
    let re = regex::Regex::new(pattern)
        .unwrap_or_else(|e| panic!("harness regex /{pattern}/ failed to compile: {e}"));
    assert!(
        re.is_match(&err.msg),
        "error message {:?} does not match /{pattern}/",
        err.msg
    );
}

/// Deep equality over
/// `serde_json::Value`. Test data is written with INTEGER literals only
/// (avoid the `Number(1)` vs `1.0` false positive; the record
/// number surface is all-integer).
pub fn assert_json_eq(a: serde_json::Value, b: serde_json::Value) {
    assert_eq!(a, b, "assert_json_eq mismatch:\n  left  = {a}\n  right = {b}");
}

/// parse → format → RE-PARSE → re-format must
/// return the same text (law 2's per-sentence form: the canonical text
/// re-parses and re-formats to itself); the canonical form is returned.
/// The dict is a parameter because Rust tests build a FRESH Dictionary per
/// test.
///
/// NOTE: this is the canonical-SENTENCE helper only — NOT `snap()` (the
/// full-state fold-equality snapshot, tests/common/snap.rs).
pub fn canonical(src: &str, dict: &Dictionary) -> String {
    let s1 = parse_sentence(src, dict).unwrap();
    let c = format_sentence(&s1, Some(dict)).unwrap();
    let s2 = parse_sentence(&c, dict).unwrap(); // canonical text must re-parse
    assert_eq!(format_sentence(&s2, Some(dict)).unwrap(), c); // and re-format to itself
    c
}
