// Full-scale census CLI for the reachability analyzer over CS-shaped content —
// the scaling sibling of fuzz:reach (CI keeps test/reach-scale.test.ts small;
// this shakes the whole 1,566 × 2,084 world across all cores).
//
//   npm run bench:reach                                  # full CS scale, all cores
//   npm run bench:reach -- --recipes 400 --cores 4       # smaller/faster
import { spawn } from 'node:child_process'
import { writeFileSync, readFileSync, unlinkSync } from 'node:fs'
import { cpus, tmpdir } from 'node:os'
import { join } from 'node:path'
import { buildCsScale, census, summarize, type NameStat } from '../test/reach-scale-lib.js'

const args = process.argv.slice(2)
const flag = (name: string): string | undefined => {
  const i = args.indexOf(name)
  return i >= 0 ? args[i + 1] : undefined
}
const nProtos = Number(flag('--protos') ?? 1566)
const nRecipes = Number(flag('--recipes') ?? 2084)
const cores = Number(flag('--cores') ?? cpus().length)

// worker mode: analyze names idx % k === i of a freshly rebuilt (deterministic)
// world, dump per-name stats as JSON, exit
const w = args.indexOf('--worker')
if (w >= 0) {
  const i = Number(args[w + 1]), k = Number(args[w + 2])
  const world = buildCsScale(Number(args[w + 3]), Number(args[w + 4]))
  const mine = world.names.filter((_, idx) => idx % k === i)
  const stats: NameStat[] = census(world, mine)
  writeFileSync(join(tmpdir(), `reach-scale-${i}.json`), JSON.stringify(stats))
  process.exit(0)
}

const t0 = performance.now()
const world = buildCsScale(nProtos, nRecipes)
const compileMs = performance.now() - t0
console.log(`content: ${world.stats.protos} prototypes, ${world.stats.recipes} verbs × 1 recipe (~${world.stats.ops} ops), ${world.stats.decks} decks, ${world.stats.triggers} triggers`)
console.log(`build+compile: ${compileMs.toFixed(0)} ms`)
console.log(`census over ${world.names.length} names on ${cores} workers…`)

const wallStart = performance.now()
await Promise.all(Array.from({ length: cores }, (_, i) => new Promise<void>((done, fail) => {
  spawn('npx', ['tsx', process.argv[1]!, '--worker', String(i), String(cores), String(nProtos), String(nRecipes)],
    { stdio: ['ignore', 'ignore', 'inherit'] })
    .on('exit', code => code === 0 ? done() : fail(new Error(`worker ${i} exited ${code}`)))
})))
const wall = (performance.now() - wallStart) / 1000

const stats = Array.from({ length: cores }, (_, i) =>
  JSON.parse(readFileSync(join(tmpdir(), `reach-scale-${i}.json`), 'utf8')) as NameStat[]).flat()
for (let i = 0; i < cores; i++) { try { unlinkSync(join(tmpdir(), `reach-scale-${i}.json`)) } catch { /* ignore */ } }
console.log(`\ncensus wall: ${wall.toFixed(1)} s on ${cores} cores`)
console.log(summarize(stats))
