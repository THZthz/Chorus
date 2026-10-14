// Deep runs for the reach fuzz — the vitest file keeps seeds few for CI; this
// CLI shakes thousands.
//
//   npm run fuzz:reach -- --seeds 500
//   npm run fuzz:reach -- --seeds 1000 --steps 9 --states 800 --from 300
import { runFuzzSeed } from '../test/reach-fuzz-lib.js'

const args = process.argv.slice(2)
const flag = (name: string, dflt: number): number => {
  const i = args.indexOf(name)
  return i >= 0 ? Number(args[i + 1]) : dflt
}
const seeds = flag('--seeds', 200)
const steps = flag('--steps', 7)
const states = flag('--states', 400)
const from = flag('--from', 1)

let failures = 0
let inconclusive = 0
let worst = 0
let dupPushes = 0
const t0 = Date.now()
for (let seed = from; seed < from + seeds; seed++) {
  const out = runFuzzSeed(seed, steps, states)
  worst = Math.max(worst, out.statesSeen)
  dupPushes += out.deduped
  for (const f of out.failures) { console.error(`FAIL ${f}`); failures++ }
  for (const i of out.inconclusive) { console.warn(`soft ${i}`); inconclusive++ }
  if ((seed - from + 1) % 50 === 0) console.log(`… ${seed - from + 1}/${seeds} seeds, ${failures} failures`)
}
console.log(`\n${seeds} seeds × ≤${steps} steps × ≤${states} states: ${failures} failure(s), ${inconclusive} inconclusive, busiest world ${worst} states, ${dupPushes} duplicate push(es) deduped, ${((Date.now() - t0) / 1000).toFixed(1)}s`)
process.exit(failures > 0 ? 1 : 0)
