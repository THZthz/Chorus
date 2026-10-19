//! Clock & durations (spec §5.1, §9.2). (day,
//! tick) is raw mechanics time: the clock ticks in 10-minute ticks (0..143
//! per day) while the engine tracks absolute minutes, so 20m offsets stay
//! exact.

use std::sync::OnceLock;

use regex::Regex;

use crate::core::num_parse::parse_authored_int;
use crate::core::regex_util::static_regex;
use crate::error::{ChorusError, Result};

/// The mechanics clock: `day` + `tick`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Clock {
    pub day: i64,
    /// ticks since day start (0..143); one tick = 10 minutes
    pub tick: i64,
}

/// Minutes per clock tick.
pub const MIN_PER_TICK: i64 = 10;
/// Clock ticks per day.
pub const TICKS_PER_DAY: i64 = 144;

/// Absolute minutes elapsed since day 0:00 for a clock.
#[must_use]
pub fn abs_min(c: Clock) -> i64 {
    // saturating i64: a clock day parsed from an absurd literal
    // (> 2^63/1440) saturates here instead of wrapping; no test
    // reaches that region.
    (c.day).saturating_mul(TICKS_PER_DAY).saturating_add(c.tick).saturating_mul(MIN_PER_TICK)
}

/// Floor division via div_euclid/rem_euclid, on the non-negative inputs
/// the engine constructs (callers self-certify non-negativity). Negative
/// input is a debug-time contract violation, never runtime behavior.
#[must_use]
pub fn clock_from_abs_min(m: i64) -> Clock {
    debug_assert!(m >= 0, "clock_from_abs_min: negative minute {m} (clocks are non-negative)");
    let total = m.div_euclid(MIN_PER_TICK);
    Clock { day: total.div_euclid(TICKS_PER_DAY), tick: total.rem_euclid(TICKS_PER_DAY) }
}

/// The earliest startable step clock at or
/// after absolute minute m (§6.6): sub-tick actions leave the rest of
/// the tick as dead air. `%` is the truncated remainder (same sign as the
/// dividend).
#[must_use]
pub fn next_step_clock(m: i64) -> Clock {
    debug_assert!(m >= 0, "next_step_clock: negative minute {m} (clocks are non-negative)");
    let rem = m % MIN_PER_TICK;
    clock_from_abs_min(if rem == 0 { m } else { m + MIN_PER_TICK - rem })
}

/// The `day:tick` rendering of a clock.
#[must_use]
pub fn format_clock(c: Clock) -> String {
    format!("{}:{}", c.day, c.tick)
}

/// (clippy's `manual_range_contains` lint is silenced — the explicit bound
/// check is deliberate)
#[allow(clippy::manual_range_contains)]
pub fn parse_clock(s: &str) -> Result<Clock> {
    // the digit class is ASCII: the regex uses explicit `[0-9]` classes.
    static RE: OnceLock<Regex> = OnceLock::new();
    let re = static_regex(&RE, || Regex::new(r"^([0-9]+):([0-9]+)$"));
    let Some(m) = re.captures(s) else {
        return Err(ChorusError::new(format!("bad clock '{s}' (expected \"day:tick\")")));
    };
    // authored integers parse DIRECTLY (B-22) — no f64 middle step
    let tick = parse_authored_int(&m[2]);
    // the 143 bound written as TICKS_PER_DAY - 1 (same value,
    // self-documenting)
    if tick < 0 || tick > TICKS_PER_DAY - 1 {
        return Err(ChorusError::new(format!("bad tick '{s}' (0..143)")));
    }
    // day needs no check: `\d+` cannot lex a sign, so day >= 0 always holds
    Ok(Clock { day: parse_authored_int(&m[1]), tick })
}

/// Minutes per §5.1 duration unit.
fn unit_minutes(unit: &str) -> i64 {
    match unit {
        "h" => 60,
        "m" => 1,
        "d" => 24 * 60,
        _ => 0, // unreachable: parse_duration's unit alternation only yields the three units above
    }
}

/// Duration literal grammar: one or more
/// `int ("h" | "m" | "d")` pairs (§5.1); compounds like `1h30m`
/// sum their pairs (§5.1).
pub fn parse_duration(s: &str) -> Result<i64> {
    static RE: OnceLock<Regex> = OnceLock::new();
    let re = static_regex(&RE, || Regex::new(r"([0-9]+)(h|m|d)"));
    let mut joined = String::with_capacity(s.len());
    // sums saturate on overflow — only absurd literals reach that region,
    // and no log pins their values (the f64 sums there are lossy anyway).
    let mut min: i64 = 0;
    for c in re.captures_iter(s) {
        let Some(m0) = c.get(0) else {
            unreachable!("regex group 0 is always present"); // impossible by construction
        };
        let whole = m0.as_str();
        joined.push_str(whole);
        let n = parse_authored_int(&c[1]);
        min = min.saturating_add(n.saturating_mul(unit_minutes(&c[2])));
    }
    // any gap between matches (an unmatched span) leaves joined != s →
    // the whole input is rejected
    if joined.is_empty() || joined != s {
        return Err(ChorusError::new(format!(
            "bad duration '{s}' (expected e.g. 30m, 2h, 90m, 1d, 1h30m)"
        )));
    }
    Ok(min)
}

/// Canonical duration rendering: prefers the
/// shorter exact single unit (d, then h, then compound minutes);
/// compound decomposition only when no single unit fits, so every rendering
/// re-parses (§5.2 round-trip).
pub fn format_duration(minutes: i64) -> Result<String> {
    if minutes < 0 {
        return Err(ChorusError::new(format!("negative duration {minutes}")));
    }
    if minutes == 0 {
        return Ok("0m".to_string());
    }
    if minutes % (24 * 60) == 0 {
        return Ok(format!("{}d", minutes / (24 * 60)));
    }
    if minutes % 60 == 0 {
        return Ok(format!("{}h", minutes / 60));
    }
    // no exact single unit: decompose — minutes finish the d/h remainder.
    // Math.floor on non-negative values is plain division.
    let d = minutes / (24 * 60);
    let h = (minutes % (24 * 60)) / 60;
    let m = minutes % 60;
    let mut out = String::new();
    if d > 0 {
        out.push_str(&format!("{d}d"));
    }
    if h > 0 {
        out.push_str(&format!("{h}h"));
    }
    if m > 0 {
        out.push_str(&format!("{m}m"));
    }
    Ok(out)
}

// NOTE: a slot-crossing helper is deliberately absent —
// dead code that was never part of the engine's behavior.

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clock_round_trips_and_quantizes() {
        assert_eq!(abs_min(Clock { day: 1, tick: 2 }), (144 + 2) * 10);
        assert_eq!(
            clock_from_abs_min(abs_min(Clock { day: 0, tick: 143 })),
            Clock { day: 0, tick: 143 }
        );
        assert_eq!(clock_from_abs_min(840 + 20), Clock { day: 0, tick: 86 });
        // `next_step_clock`: sub-tick dead air resumes on the next tick
        assert_eq!(next_step_clock(0), Clock { day: 0, tick: 0 });
        assert_eq!(next_step_clock(1), Clock { day: 0, tick: 1 });
        assert_eq!(next_step_clock(10), Clock { day: 0, tick: 1 });
        assert_eq!(format_clock(Clock { day: 3, tick: 41 }), "3:41");
    }

    #[test]
    fn day_rollover_is_exact_at_the_tick_count() {
        // day 0's last tick, then the fold into day 1
        assert_eq!(clock_from_abs_min(1439), Clock { day: 0, tick: 143 });
        assert_eq!(clock_from_abs_min(1440), Clock { day: 1, tick: 0 });
        // the step-clock quantization crosses the same boundary
        assert_eq!(next_step_clock(1439), Clock { day: 1, tick: 0 });
        // and the inverse map closes the round trip
        assert_eq!(abs_min(Clock { day: 1, tick: 0 }), 1440);
    }

    #[test]
    fn parse_clock_messages_are_byte_exact() {
        assert_eq!(parse_clock("2:17").unwrap(), Clock { day: 2, tick: 17 });
        let e = parse_clock("2:144").unwrap_err();
        assert_eq!(e.msg, "bad tick '2:144' (0..143)");
        // a mid-grid unit value parses (the bound is only at 144)
        assert_eq!(parse_clock("2:48").unwrap(), Clock { day: 2, tick: 48 });
        let e = parse_clock("2x").unwrap_err();
        assert_eq!(e.msg, "bad clock '2x' (expected \"day:tick\")");
        // B-22: authored integers parse EXACTLY — the day is the authored
        // bytes, no f64 middle step rounding it to …992
        assert_eq!(parse_clock("9007199254740993:0").unwrap().day, 9007199254740993);
    }

    #[test]
    fn duration_grammar_and_canonical_rendering() {
        assert_eq!(parse_duration("30m").unwrap(), 30);
        assert_eq!(parse_duration("1h30m").unwrap(), 90);
        assert_eq!(parse_duration("1d").unwrap(), 1440);
        assert_eq!(parse_duration("1h").unwrap(), 60);
        assert!(parse_duration("1hx").is_err()); // gap: joined != s
        assert!(parse_duration("90m1x").is_err());
        assert!(parse_duration("x1h").is_err());
        assert_eq!(
            parse_duration("90m1x").unwrap_err().msg,
            "bad duration '90m1x' (expected e.g. 30m, 2h, 90m, 1d, 1h30m)"
        );
        assert_eq!(format_duration(0).unwrap(), "0m");
        assert_eq!(format_duration(1440).unwrap(), "1d");
        assert_eq!(format_duration(90).unwrap(), "1h30m");
        assert_eq!(format_duration(60).unwrap(), "1h");
        assert_eq!(format_duration(30).unwrap(), "30m");
        assert_eq!(format_duration(1470).unwrap(), "1d30m");
        assert_eq!(format_duration(1770).unwrap(), "1d5h30m");
        assert_eq!(format_duration(100).unwrap(), "1h40m");
        assert_eq!(format_duration(-1).unwrap_err().msg, "negative duration -1");
    }

    #[test]
    fn duration_ladder_round_trips() {
        for minutes in [30, 60, 90, 1470, 1770, 0, 1440, 100] {
            let rendered = format_duration(minutes).unwrap();
            assert_eq!(parse_duration(&rendered).unwrap(), minutes, "{rendered} re-parses");
        }
    }
}
