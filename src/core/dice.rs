//! Seeded counter RNG — the engine's ONLY randomness (spec §9.41, plan v0.4
//! F4; law 5: no RNG anywhere outside this mechanism). Sampling is
//! counter-based and pure: die i of a roll at counter c is
//!
//!   1 + floor(sides * splitmix64(seed XOR (c+i)) / 2^64)
//!
//! `State.rng` is the shared counter: a roll of m dice consumes exactly m
//! values (c .. c+m-1); the concrete `ROLLED(<total>, <ctrAfter>)` row lets
//! the fold land the counter exactly where play left it without re-deriving
//! anything (law 3, W3).

/// ts: src/core/dice.ts:20-22. u64 wrapping arithmetic replaces the TS
/// BigInt `& MASK64` chain step for step.
const GOLDEN_GAMMA: u64 = 0x9e37_79b9_7f4a_7c15;
const MIX1: u64 = 0xbf58_476d_1ce4_e5b9;
const MIX2: u64 = 0x94d0_49bb_1331_11eb;

/// ts: src/core/dice.ts:26-31 — the splitmix64 finalizer, applied ONCE (not
/// the full splitmix64 sequence). Every intermediate stays an exact 64-bit
/// unsigned value: `wrapping_*` on u64 is bit-identical to the TS
/// `& MASK64` after each add/multiply.
#[must_use]
pub fn splitmix64(x: u64) -> u64 {
    let mut z = x.wrapping_add(GOLDEN_GAMMA); // z = (x + GOLDEN_GAMMA) & MASK64
    z = (z ^ (z >> 30)).wrapping_mul(MIX1); // z = ((z ^ (z >> 30)) * MIX1) & MASK64
    z = (z ^ (z >> 27)).wrapping_mul(MIX2); // z = ((z ^ (z >> 27)) * MIX2) & MASK64
    z ^ (z >> 31) // return (z ^ (z >> 31)) & MASK64
}

/// ts: src/core/dice.ts:38-42 — one die: the value at (seed, counter) over
/// `sides` faces. `seed as u64` IS `BigInt.asUintN(64, BigInt(seed))`:
/// negative seeds/counter get the same two's-complement treatment (the
/// `-1`/`-42` vector rows pin this). ENG-05: the `sides * r` product needs
/// the u128 widening — a pure u64 wrapping multiply would destroy the high
/// bits that the `>> 64` floor-divide-by-2^64 reads; the product of
/// `sides < 2^53` and `r < 2^64` is < 2^117, well inside u128.
#[must_use]
pub fn roll_die(seed: i64, counter: i64, sides: i64) -> i64 {
    let key = (seed as u64) ^ (counter as u64);
    let r = splitmix64(key);
    1 + (((sides as u128) * (r as u128)) >> 64) as i64
}
