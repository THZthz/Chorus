// Randomized differential testing for the card-reachability analyzer — the
// soundness contract under fire:
//   unobtainable ⇒ never appears in any explored play   (hard — the forward
//                  truth is an under-approximation, sound under any bounds)
//   obtainable   ⇒ appears in some explored play        (hard; downgraded to
//                  inconclusive when the exploration hit its state cap)
// Failures print their seed — runFuzzSeed(seed) reproduces exactly.
// Bounds rationale (2026-10 audit): ~46 ms/seed at the default 8 steps × 500
// states (deep +7/×20 never fires its cap here — 0 inconclusive over 300
// seeds), so 100 seeds cost ~2 s and sample 2.5× the scenarios of the old 40.
// The CLI (npm run fuzz:reach) shakes thousands; failures print their seed.
import { describe, expect, test } from 'vitest'
import { runFuzzSeed } from './reach-fuzz-lib.js'

const SEEDS = 100

describe('reach fuzz — randomized scenarios vs exhaustive forward truth', () => {
  for (let seed = 1; seed <= SEEDS; seed++) {
    test(`seed ${seed}`, { timeout: 30_000 }, () => {
      const out = runFuzzSeed(seed)
      if (out.inconclusive.length > 0) console.warn(`seed ${seed}: ${out.inconclusive.join('; ')}`)
      expect(out.failures, out.failures.join('\n')).toEqual([])
    })
  }
})
