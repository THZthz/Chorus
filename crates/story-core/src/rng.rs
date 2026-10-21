//! The seeded RNG step (kernel-api.md §4): one die = one call, the counter
//! lives in `State` and is incremented once per die.

/// splitmix64 finalizer; deterministic and side-effect free.
pub fn splitmix64(x: u64) -> u64 {
    let mut z = x;
    z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    z ^ (z >> 31)
}

#[cfg(test)]
mod tests {
    use super::splitmix64;

    #[test]
    fn deterministic_and_zero_fixed() {
        assert_eq!(splitmix64(0), 0);
        let a = splitmix64(7);
        assert_eq!(a, splitmix64(7));
        assert_ne!(a, splitmix64(8));
    }
}
