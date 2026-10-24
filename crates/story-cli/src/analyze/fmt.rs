//! Number formatting for the report values: exact small rationals,
//! decimals, and probability phrasing.

use story_analyze::query::RngMode;

// ---------------------------------------------------------------------------
// Number formatting
// ---------------------------------------------------------------------------

pub(super) fn rng_name(rng: RngMode) -> &'static str {
    match rng {
        RngMode::Deterministic => "deterministic",
        RngMode::OptimisticBranching => "optimistic",
        RngMode::Adversarial => "adversarial",
    }
}

/// Exact fraction when the double is a small rational (the chance-point
/// class probabilities are), else a decimal. The scan runs on the absolute
/// value so negative rationals reduce too, with the sign reapplied; a
/// magnitude below the six-decimal resolution prints as `≈0` rather than a
/// misleading `±0.000000`.
pub(super) fn fraction_or_decimal(x: f64) -> String {
    if !x.is_finite() {
        return format!("{x}");
    }
    if x == 0.0 {
        return "0".to_string();
    }
    let sign = if x < 0.0 { "-" } else { "" };
    let a = x.abs();
    if a < 1e-6 {
        return "≈0".to_string();
    }
    for q in 1..=1000u64 {
        let scaled = a * q as f64;
        let nearest = scaled.round();
        if nearest > 0.0 && (scaled - nearest).abs() < 1e-9 {
            let (mut n, mut d) = (nearest as u64, q);
            let g = gcd(n, d);
            n /= g;
            d /= g;
            return if d == 1 {
                format!("{sign}{n}")
            } else {
                format!("{sign}{n}/{d}")
            };
        }
    }
    format!("{sign}{a:.6}")
}

fn gcd(mut a: u64, mut b: u64) -> u64 {
    while b != 0 {
        (a, b) = (b, a % b);
    }
    a
}

pub(super) fn probability_text(p: Option<f64>) -> String {
    match p {
        None => "probability n/a (adversarial witnesses carry none)".to_string(),
        Some(x) => format!("probability {}", fraction_or_decimal(x)),
    }
}
