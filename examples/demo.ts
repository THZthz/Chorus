// End-to-end demo: play a few steps, print the canonical journal and the prose.
// Run: npx tsx examples/demo.ts   (or add to an npm script)
import { compileContent } from '../src/content.js'
import { executeStep } from '../src/engine.js'
import { fold } from '../src/fold.js'
import { parseSentence } from '../src/parse.js'
import { journal, prose } from '../src/render.js'
import { seedContent, seedDictionary, seedState } from '../src/seed.js'
import { Log } from '../src/store.js'

const dict = seedDictionary()
const content = compileContent(seedContent(), dict)

const steps = [
  '0:14 @ ANA AT HOUSE READ THE_FORGETTING',
  '0:15 @ ANA, BERTHA READ FAIRY_TALES', // ends 0:16 → dusk_falls fires
  '0:16 @ ANA, BERTHA AT DINING_HALL TALK ABOUT CLUE_SECRET_HIDEOUT',
  '0:17 @ BERTHA READ MAGICK_BOOK', // refused — nobody who knows is present
  '0:17 @ ANA TRAVEL_TO STUDY; ANA AT STUDY READ MAGICK_BOOK',
  '0:20 @ ANA WAIT',
]

const log = new Log(seedState())
let state = log.initialState
let id = 1
let n = 0
for (const src of steps) {
  const s = parseSentence(src, dict)
  const r = executeStep(content, dict, state, { clock: s.clock!, clauses: s.clauses }, {
    firstRecordId: id,
    idGen: () => `x${id}_${++n}`,
  })
  if (r.rejected) throw new Error(r.rejected)
  log.appendAll(r.records)
  state = r.state
  id += r.records.length
}

console.log('── journal (canonical sentences) ' + '─'.repeat(28))
console.log(journal(log.records))
console.log()
console.log('── the same log, as prose ' + '─'.repeat(38))
for (const rec of log.records) {
  console.log('· ' + prose(rec, content, dict))
}
console.log()
console.log('── state = fold(log): verify replay ' + '─'.repeat(25))
const replayed = fold(log.records, seedState(), dict, content)
console.log(`entities alive: ${replayed.entities.size === state.entities.size ? 'match' : 'MISMATCH'}`)
console.log(`clock now: day ${Math.floor(replayed.now / 60 / 24)}, hour ${Math.floor(replayed.now / 60) % 24}`)
