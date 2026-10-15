//! Authored-integer parsing (B-22, fixed): a DIRECT i64 parse of the
//! integer literals the grammars admit — the authored bytes are the value.

/// Authored integer literals parse directly into `i64` — no f64 middle
/// step (B-22, fixed batch 3). Spec §10 says `seed:` is "an integer"; the
/// former detour through `str::parse::<f64>` silently rewrote authored
/// integers past 2^53 onto the nearest representable value
/// ("9007199254740993" landed as …992). Now the authored bytes are the
/// value: `str::parse::<i64>()` is exact through the whole i64 range.
///
/// The overflow tail is DEFINED: a well-formed literal beyond the i64
/// range saturates at its sign's bound (`i64::MAX` / `i64::MIN`) — the
/// engine's documented saturation direction (`abs_min`, `checked_step`).
/// Malformed input maps to 0; every call site's grammar admits only
/// `-?[0-9]+`-shaped text, so that branch is defensive, never reachable.
#[must_use]
pub fn parse_authored_int(s: &str) -> i64 {
    match s.parse::<i64>() {
        Ok(v) => v,
        Err(e) => match e.kind() {
            std::num::IntErrorKind::PosOverflow => i64::MAX,
            std::num::IntErrorKind::NegOverflow => i64::MIN,
            _ => 0,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_authored_int_is_exact_past_2_53_and_saturates() {
        // B-22: past 2^53 the authored bytes ARE the value — the f64
        // middle step (which rounded …993 to …992) is gone
        assert_eq!(parse_authored_int("9007199254740993"), 9007199254740993);
        assert_eq!(parse_authored_int("0"), 0);
        assert_eq!(parse_authored_int("47"), 47);
        assert_eq!(parse_authored_int("-5"), -5);
        // > i64 range: saturates at the sign's bound (the defined tail)
        assert_eq!(parse_authored_int("99999999999999999999"), i64::MAX);
        assert_eq!(parse_authored_int("-99999999999999999999"), i64::MIN);
    }
}
