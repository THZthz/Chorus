//! The deterministic test-side driver RNG (`Lcg`).
//!
//! Deliberately NOT `#[cfg(test)]`-gated: integration-test crates compile
//! the library without `cfg(test)`, so the type must live in the normal
//! public surface. Its sequences are frozen bit-for-bit — the fuzz
//! campaigns and the seed-keyed findings are keyed to them.

/// The deterministic test-side LCG: u32 wrapping arithmetic,
/// `s = s.wrapping_mul(1664525) + 1013904223 (mod 2^32)` — the frozen
/// sequences shared by the reach-fuzz, reach-scale and explorer drivers.
///
/// The three seed entry points, kept one-to-one:
/// - fuzz:  `Lcg::new(seed)` — XOR 0x9e3779b9
/// - scale: `Lcg::new(0xc5)`
/// - explorer: `Lcg::from_raw_state(0x4e494748)` — 'NIGH', seeded as the raw
///   constant with **no XOR**.
#[derive(Debug, Clone)]
pub struct Lcg {
    s: u32,
}

impl Lcg {
    #[must_use]
    pub fn new(seed: u32) -> Self {
        Lcg { s: seed ^ 0x9e37_79b9 }
    }

    #[must_use]
    pub fn from_raw_state(s: u32) -> Self {
        Lcg { s }
    }

    #[must_use]
    pub fn next_u32(&mut self) -> u32 {
        self.s = self.s.wrapping_mul(1664525).wrapping_add(1013904223);
        self.s
    }

    /// The new state scaled into `[0, 1)`: u32→f64 is exact and the division
    /// by 2^32 is a power-of-two scaling, so the f64 result is exact; kept
    /// in f64, never integerized.
    #[must_use]
    pub fn next_f64(&mut self) -> f64 {
        f64::from(self.next_u32()) / 4294967296.0
    }

    /// `floor(r() * n)` — computed in f64, then truncated.
    #[must_use]
    pub fn next_int(&mut self, n: usize) -> usize {
        (self.next_f64() * n as f64).floor() as usize
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lcg_reproduces_the_frozen_sequence_bit_for_bit() {
        // fuzz entry, seed 0 — states cross-checked against an independent
        // implementation, 2026-10-14
        let mut fuzz = Lcg::new(0);
        assert_eq!(fuzz.next_u32(), 1120982980);
        assert_eq!(fuzz.next_u32(), 3911581779);
        assert_eq!(fuzz.next_u32(), 1066994070);
        assert_eq!(fuzz.next_u32(), 1621898237);
        // explorer entry: 'NIGH' as the RAW state, no XOR
        let mut nigh = Lcg::from_raw_state(0x4e49_4748);
        assert_eq!(nigh.next_f64(), 0.3694802538957447);
        assert_eq!(nigh.next_f64(), 0.35568378726020455);
        // scale entry: lcg(0xc5)
        let mut scale = Lcg::new(0xc5);
        assert_eq!(scale.next_f64(), 0.23735849070362747);
        // floor(r() * n) stays in f64
        assert_eq!(scale.next_f64(), 0.3778064283542335);
        assert_eq!(scale.next_int(10), 4);
    }
}
