//! Port of test/clock.test.ts (W1, plan §5.2.1) — clock & duration unit
//! tests, spec §5.1 grammar, §9.45 compound durations.
//!
//! Mapping (plan §5.2): one `#[test]` per TS `test`, TS name kept
//! (snake_case); a FRESH Dictionary per test (TEST-10); `toThrow(/re/)` →
//! `assert_err_msg(res, "re")`; `toEqual` → `assert_json_eq`.

mod common;

use chorus::core::clock::{format_clock, format_duration, parse_clock, parse_duration};
use chorus::core::dict::core_dictionary;
use chorus::core::lexer::{lex, Tok};
use chorus::core::types::Clock;

use common::harness::{assert_err_msg, assert_json_eq, canonical};

// ---------- parseClock (ts: clock.test.ts:20-34) ----------

#[test]
fn accepts_day_halfhour() {
    // ts: clock.test.ts:21-24
    let got = parse_clock("0:0").unwrap();
    assert_json_eq(
        serde_json::json!({ "day": got.day, "halfhour": got.halfhour }),
        serde_json::json!({ "day": 0, "halfhour": 0 }),
    );
    let got = parse_clock("3:47").unwrap();
    assert_json_eq(
        serde_json::json!({ "day": got.day, "halfhour": got.halfhour }),
        serde_json::json!({ "day": 3, "halfhour": 47 }),
    );
}

#[test]
fn day_cannot_be_negative_d_cannot_lex_a_sign() {
    // ts: clock.test.ts:25-27 — `\d+` cannot lex a sign
    assert_err_msg(parse_clock("-1:5"), "bad clock");
}

#[test]
fn rejects_halfhour_out_of_range_and_malformed_input() {
    // ts: clock.test.ts:28-33
    assert_err_msg(parse_clock("0:48"), "bad halfhour");
    assert_err_msg(parse_clock("0:-1"), "bad clock");
    assert_err_msg(parse_clock("1:"), "bad clock");
    assert_err_msg(parse_clock("now"), "bad clock");
}

// ---------- parseDuration (ts: clock.test.ts:36-55) ----------

#[test]
fn single_5_1_units() {
    // ts: clock.test.ts:37-43
    assert_eq!(parse_duration("0m").unwrap(), 0);
    assert_eq!(parse_duration("1halfhour").unwrap(), 30);
    assert_eq!(parse_duration("2h").unwrap(), 120);
    assert_eq!(parse_duration("90m").unwrap(), 90);
    assert_eq!(parse_duration("1d").unwrap(), 1440);
}

#[test]
fn compound_literals_sum_their_unit_pairs() {
    // ts: clock.test.ts:44-48 (§9.45)
    assert_eq!(parse_duration("1h30m").unwrap(), 90);
    assert_eq!(parse_duration("2halfhour15m").unwrap(), 75);
    assert_eq!(parse_duration("1d2h30m").unwrap(), 1590);
}

#[test]
fn rejects_unknown_units_bare_ints_and_stray_suffixes() {
    // ts: clock.test.ts:49-54
    assert_err_msg(parse_duration("2fortnights"), "bad duration");
    assert_err_msg(parse_duration("12"), "bad duration");
    assert_err_msg(parse_duration("1h30min"), "bad duration");
    assert_err_msg(parse_duration("h"), "bad duration");
}

// ---------- formatDuration (ts: clock.test.ts:57-75) ----------

#[test]
fn prefers_the_shortest_exact_single_unit() {
    // ts: clock.test.ts:58-65
    assert_eq!(format_duration(0).unwrap(), "0m");
    assert_eq!(format_duration(30).unwrap(), "1halfhour");
    assert_eq!(format_duration(60).unwrap(), "1h");
    assert_eq!(format_duration(90).unwrap(), "3halfhour"); // exact halfhour beats compound
    assert_eq!(format_duration(1440).unwrap(), "1d");
    assert_eq!(format_duration(1470).unwrap(), "49halfhour"); // exact single unit wins
}

#[test]
fn decomposes_only_when_no_single_unit_fits() {
    // ts: clock.test.ts:66-71 (§9.45)
    assert_eq!(format_duration(45).unwrap(), "45m");
    assert_eq!(format_duration(100).unwrap(), "1h40m");
    assert_eq!(format_duration(1441).unwrap(), "1d1m");
    assert_eq!(format_duration(1501).unwrap(), "1d1h1m");
}

#[test]
fn round_trips_through_parse_duration_dense_sweep() {
    // ts: clock.test.ts:72-75
    for m in 0..=720 {
        assert_eq!(
            parse_duration(&format_duration(m).unwrap()).unwrap(),
            m,
            "round-trip broke at {m}m"
        );
    }
    for m in [1441, 1501, 2882, 4321, 59049] {
        assert_eq!(parse_duration(&format_duration(m).unwrap()).unwrap(), m);
    }
}

// ---------- formatClock / parseClock round-trip (ts: clock.test.ts:78-83) ----------

#[test]
fn format_parse_is_the_identity() {
    // ts: clock.test.ts:79-82
    for day in [0, 1, 12] {
        for halfhour in [0, 23, 47] {
            let parsed = parse_clock(&format_clock(Clock { day, halfhour })).unwrap();
            assert_json_eq(
                serde_json::json!({ "day": parsed.day, "halfhour": parsed.halfhour }),
                serde_json::json!({ "day": day, "halfhour": halfhour }),
            );
        }
    }
}

// ---------- compound literals through the lexer and sentence pipeline
// ---------- (§9.45, ts: clock.test.ts:85-96) ----------

/// `lex(src)` tokens as the TS `toEqual` would see them: `[{ t, v }]` —
/// `v` is a NUMBER for int/dur tokens, a string for the rest (the TS Tok
/// union's per-variant shape).
fn toks_json(toks: &[Tok]) -> serde_json::Value {
    serde_json::Value::Array(
        toks.iter()
            .map(|t| match t {
                Tok::Int { v } | Tok::Dur { v } => {
                    serde_json::json!({ "t": t.t(), "v": v })
                }
                _ => serde_json::json!({ "t": t.t(), "v": t.v_str() }),
            })
            .collect(),
    )
}

#[test]
fn lexes_as_one_dur_token_a_dur_re_regression_would_split_it() {
    // ts: clock.test.ts:86-90 — the `halfhour|h|m|d` alternation order is
    // load-bearing (Q9); no unit → still an int
    assert_json_eq(toks_json(&lex("1h30m").unwrap()), serde_json::json!([{ "t": "dur", "v": 90 }]));
    assert_json_eq(
        toks_json(&lex("1d2h30m").unwrap()),
        serde_json::json!([{ "t": "dur", "v": 1590 }]),
    );
    assert_json_eq(toks_json(&lex("12").unwrap()), serde_json::json!([{ "t": "int", "v": 12 }]));
}

#[test]
fn canonicalizes_in_an_outcome_list_and_round_trips() {
    // ts: clock.test.ts:91-95 (§5.2) — formatDuration(90) prefers the exact
    // single unit: 1h30m → 3halfhour
    let dict = core_dictionary();
    assert_eq!(
        canonical("0:16 @ EVENT dusk_falls -> PROGRESS(1h30m, \"tick\")", &dict),
        "0:16 @ EVENT dusk_falls -> PROGRESS(3halfhour, \"tick\")"
    );
}
