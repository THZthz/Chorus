// Seeded counter RNG — the engine's ONLY randomness (spec §9.41, plan v0.4 F4;
// law 5: no RNG anywhere outside this mechanism). Sampling is counter-based
// and pure: die i of a roll at counter c is
//
//   1 + floor(sides * splitmix64(seed XOR (c+i)) / 2^64)
//
// computed BigInt END-TO-END — the multiply and shift happen on 64-bit
// integers, never through float64 (a Number cannot hold `sides * u64` and
// would silently quantize the low bits away). `State.rng` is the shared
// counter: a roll of m dice consumes exactly m values (c .. c+m-1), and the
// concrete `ROLLED(<total>, <ctrAfter>)` row lets the fold land the counter
// exactly where the played history left it without re-deriving anything
// (law 3). Changing `seed` shifts future dice but never past rows — the
// documented §9.30 seed-drift asymmetry.
//
// The reference vectors in test/dice.test.ts were computed independently
// (python3, arbitrary-precision integers) against this exact algorithm.

const MASK64 = (1n << 64n) - 1n
const GOLDEN_GAMMA = 0x9e3779b97f4a7c15n
const MIX1 = 0xbf58476d1ce4e5b9n
const MIX2 = 0x94d049bb133111ebn

/** the splitmix64 finalizer, all constants and shifts on BigInt so every
 *  intermediate stays an exact 64-bit unsigned value */
export function splitmix64(x: bigint): bigint {
  let z = (x + GOLDEN_GAMMA) & MASK64
  z = ((z ^ (z >> 30n)) * MIX1) & MASK64
  z = ((z ^ (z >> 27n)) * MIX2) & MASK64
  return (z ^ (z >> 31n)) & MASK64
}

/** one die: the value at (seed, counter) over `sides` faces. The seed and the
 *  counter are XORed as 64-bit unsigned (asUintN gives negatives the same
 *  two's-complement treatment python's mask would), the product with `sides`
 *  is a BigInt multiply (exact), and the 64-right-shift IS the floor divide
 *  by 2^64. The result is at most `sides` (< 2^53), so Number() is exact. */
export function rollDie(seed: number, counter: number, sides: number): number {
  const key = BigInt.asUintN(64, BigInt(seed)) ^ BigInt.asUintN(64, BigInt(counter))
  const r = splitmix64(key)
  return 1 + Number((BigInt(sides) * r) >> 64n)
}
