//! Seeded counter RNG — the engine's ONLY randomness (spec §4.4; law 5:
//! no RNG anywhere outside this mechanism). Sampling is
//! counter-based and pure: die i of a roll at counter c is
//!
//!   1 + floor(sides * splitmix64(seed XOR (c+i)) / 2^64)
//!
//! `State.rng` is the shared counter: a roll of m dice consumes exactly m
//! values (c .. c+m-1); the concrete `ROLLED(<total>, <ctrAfter>)` row lets
//! the fold land the counter exactly where play left it without re-deriving
//! anything (law 3).

/// u64 wrapping arithmetic: each `wrapping_*` op is exactly "add/multiply,
/// then keep the low 64 bits".
const GOLDEN_GAMMA: u64 = 0x9e37_79b9_7f4a_7c15;
const MIX1: u64 = 0xbf58_476d_1ce4_e5b9;
const MIX2: u64 = 0x94d0_49bb_1331_11eb;

/// The splitmix64 finalizer, applied ONCE (not the full splitmix64
/// sequence). Every intermediate stays an exact 64-bit unsigned value:
/// `wrapping_*` on u64 is the same as masking to 64 bits after each
/// add/multiply.
#[must_use]
pub fn splitmix64(x: u64) -> u64 {
    let mut z = x.wrapping_add(GOLDEN_GAMMA); // z = (x + GOLDEN_GAMMA) & MASK64
    z = (z ^ (z >> 30)).wrapping_mul(MIX1); // z = ((z ^ (z >> 30)) * MIX1) & MASK64
    z = (z ^ (z >> 27)).wrapping_mul(MIX2); // z = ((z ^ (z >> 27)) * MIX2) & MASK64
    z ^ (z >> 31) // return (z ^ (z >> 31)) & MASK64
}

/// One die: the value at (seed, counter) over `sides` faces. `seed as u64`
/// re-interprets the i64 two's-complement, so negative seeds/counters get
/// the same low-64-bit treatment (the `-1`/`-42` frozen vectors pin this).
/// The `sides * r` product needs the u128 widening — a pure u64 wrapping
/// multiply would destroy the high bits that the `>> 64` floor-divide-by-2^64
/// reads; the product of `sides < 2^53` and `r < 2^64` is < 2^117, well
/// inside u128.
#[must_use]
pub fn roll_die(seed: i64, counter: i64, sides: i64) -> i64 {
    let key = (seed as u64) ^ (counter as u64);
    let r = splitmix64(key);
    1 + (((sides as u128) * (r as u128)) >> 64) as i64
}
