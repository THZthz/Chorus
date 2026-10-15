//! vitest → cargo assertion helpers, finalized in W1 (plan §5.2 "harness.rs
//! 一次定型，W0/W1 落地"). Every ported test file mounts this through
//! `mod common;` so assertion discipline cannot drift between files.
//!
//! **W3 扩展**：`snap()`（full-state snapshot, test/snap.ts）、`run()` settle
//! 适配器（tests/common/settle.rs）、seed fixtures 与 explorer_lib 落位于
//! `tests/common/`（§5.2.1）——它们不属于本文件；本文件只承载 W1 的三个
//! 测试文件需要的断言基建。

// each integration test crate compiles this module separately and not every
// helper is used by every file (e.g. fn_expand_direct uses assert_err_msg
// only) — the helpers are the shared discipline, not per-file dead code
#![allow(dead_code)]

use chorus::core::dict::Dictionary;
use chorus::core::format::format_sentence;
use chorus::core::parse::parse_sentence;
use chorus::js::ChorusError;

/// ts: plan §5.2 — `expect(f).toThrow(/re/)`: the ChorusError message must
/// match `pattern` (the JS regex literal copied CHARACTER FOR CHARACTER —
/// TEST-05 verified none of the repo's patterns need lookaround/backrefs).
/// If a pattern ever needs a Rust-regex escaping adjustment, register the
/// adjustment HERE in one place (plan §2.5); none are needed so far.
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

/// ts: plan §5.2 — `expect(a).toEqual(b)`: deep equality over
/// `serde_json::Value`. Test data is written with INTEGER literals only
/// (plan §5.2: avoid the `Number(1)` vs `1.0` false positive; the record
/// number surface is all-integer per PR2-04).
pub fn assert_json_eq(a: serde_json::Value, b: serde_json::Value) {
    assert_eq!(a, b, "assert_json_eq mismatch:\n  left  = {a}\n  right = {b}");
}

/// ts: test/sentence.test.ts:11-17 (identical helper in
/// test/clock.test.ts:12-18) — parse → format → RE-PARSE → re-format must
/// return the same text (law 2's per-sentence form: the canonical text
/// re-parses and re-formats to itself); the canonical form is returned.
/// The dict is a parameter because Rust tests build a FRESH Dictionary per
/// test (plan §5.2, TEST-10).
///
/// NOTE: this is the canonical-SENTENCE helper only — NOT `snap()` (the
/// full-state fold-equality snapshot, test/snap.ts → W3's
/// tests/common/snap.rs).
pub fn canonical(src: &str, dict: &Dictionary) -> String {
    let s1 = parse_sentence(src, dict).unwrap();
    let c = format_sentence(&s1, Some(dict)).unwrap();
    let s2 = parse_sentence(&c, dict).unwrap(); // canonical text must re-parse
    assert_eq!(format_sentence(&s2, Some(dict)).unwrap(), c); // and re-format to itself
    c
}
