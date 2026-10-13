// End-to-end demo: play a few steps, print the canonical journal and the prose.
// Run: npx tsx examples/demo.ts   (or add to an npm script)
import { compileContent } from '../src/content.js'
import { coreDictionary } from '../src/dict.js'
import { executeStep } from '../src/engine.js'
import { fold } from '../src/fold.js'
import { clockFromAbsMin } from '../src/clock.js'
import { parseSentence } from '../src/parse.js'
import { journal, prose } from '../src/render.js'
import { Log } from '../src/store.js'
import { seedContent, seedState } from '../test/seed.js'

const dict = coreDictionary()
const content = compileContent(seedContent(), dict)

const steps = [
  '0:28 @ ANA AT HOUSE READ THE_FORGETTING',        // 14:00
  '0:30 @ ANA, BERTHA READ FAIRY_TALES',            // 15:00 — ends 16:00 → dusk_falls fires
  '0:32 @ ANA, BERTHA AT DINING_HALL TALK ABOUT CLUE_SECRET_HIDEOUT',
  '0:34 @ BERTHA READ MAGICK_BOOK',                 // refused — nobody who knows is present
  '0:34 @ ANA TRAVEL_TO THE_STUDY; ANA AT THE_STUDY READ MAGICK_BOOK',
  '0:40 @ ANA WAIT',
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
const clock = clockFromAbsMin(replayed.now)
console.log(`clock now: day ${clock.day}, halfhour ${clock.halfhour}`)
