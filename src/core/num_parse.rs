//! Authored-integer parsing: the f64 middle step and its saturation tail
//! (B-22 tracks the middle step itself).

/// Authored integer literals parse through a correctly-rounded f64 middle
/// step and only then land in the integer domain — a frozen engine choice.
/// Rust's `str::parse::<f64>` rounds to nearest, ties to even, so a literal
/// past 2^53 ("9007199254740993") lands on the nearest representable value
/// (…992) rather than the exact integer (…993, which a direct
/// `str::parse::<i64>()` would produce). The final f64→i64 `as` conversion
/// saturates at the i64 bounds; no log pins that region.
///
/// NaN can only occur for inputs the call sites' regexes already rejected;
/// it maps to 0 rather than panicking.
#[must_use]
pub fn parse_i64_like_f64(s: &str) -> i64 {
    let f: f64 = s.parse().unwrap_or(f64::NAN);
    if f.is_nan() {
        0
    } else {
        f as i64
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_i64_like_f64_rounds_like_f64_not_direct_i64() {
        // direct i64 parse gives …993, the f64 middle step …992
        assert_eq!(parse_i64_like_f64("9007199254740993"), 9007199254740992);
        assert_eq!(parse_i64_like_f64("0"), 0);
        assert_eq!(parse_i64_like_f64("47"), 47);
        assert_eq!(parse_i64_like_f64("-5"), -5);
        // > i64 range: saturates at the i64 bounds (no log pins that region)
        assert_eq!(parse_i64_like_f64("99999999999999999999"), i64::MAX);
    }
}
