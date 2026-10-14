// Parallel verb stations (v0.5) — the feature's own acceptance tests:
// windows overlap, stations and cards lock, completions resolve in a
// deterministic order, and the fold rebuilds open windows byte-for-byte.
import { describe, expect, test } from 'vitest'
import { compileContent, type CompiledContent, type ContentRaw, type VerbContent } from '../src/content/content.js'
import { coreDictionary } from '../src/core/dict.js'
import { executeStep, type ActionRecord, type ExecResult } from '../src/engine/engine.js'
import { formatClock, parseClock } from '../src/core/clock.js'
import { fold } from '../src/engine/fold.js'
import { parseSentence } from '../src/core/parse.js'
import { loadLog, saveLog } from '../src/engine/store.js'
import { cloneState, newState, type State } from '../src/core/types.js'
import { seedContent, seedState } from './seed.js'
import { snap } from './snap.js'

const dict = coreDictionary()
const content = compileContent(seedContent(), dict)

function run(state: State, src: string, firstRecordId = 1, theContent: CompiledContent = content): ExecResult {
  // a lone clock token is a pure advance — no sentence to parse
  if (/^\d+:\d+$/.test(src)) {
    return executeStep(theContent, dict, state, { clock: parseClock(src), clauses: [] }, { firstRecordId })
  }
  const s = parseSentence(src, dict)
  return executeStep(theContent, dict, state, { clock: s.clock ?? { day: 0, halfhour: 0 }, clauses: s.clauses }, { firstRecordId })
}

const rigContent = (verbs: Record<string, VerbContent>, extra: Partial<ContentRaw> = {}): CompiledContent => {
  for (const name of Object.keys(verbs)) if (!dict.isVerb(name)) dict.registerVerb(name) // rig verbs are code-registered (§10)
  return compileContent({ verbs, ...extra }, dict)
}

// two stations, two durations, no side effects — the geometry rig
const geo = (aDur: number, bDur: number, extra: Partial<ContentRaw> = {}): CompiledContent =>
  rigContent({
    PACE_A: { slots: [{ role: 'actors', min: 1, max: 1, query: 'kind:character' }], duration: aDur, recipes: [{ out: ['NARRATE "a done."'] }] },
    PACE_B: { slots: [{ role: 'actors', min: 1, max: 1, query: 'kind:character' }], duration: bDur, recipes: [{ out: ['NARRATE "b done."'] }] },
  }, extra)

function seed(): State {
  return seedState()
}

describe('parallel windows — the model', () => {
  test('two verbs run overlapping windows; the clock stays put until an advance', () => {
    let state = seed()
    const r1 = run(state, '0:0 @ ANA PACE_A', 1, geo(60, 30)) // window [0, 60]
    expect(r1.rejected).toBeNull()
    state = r1.state
    expect(state.windows.size).toBe(1)
    expect(state.now).toBe(0) // beginning moves nothing
    const r2 = run(state, '0:0 @ ANA PACE_B', r1.records.length + 1, geo(60, 30)) // window [0, 30] — IN PARALLEL
    expect(r2.rejected).toBeNull()
    state = r2.state
    expect(state.windows.size).toBe(2)
    expect(state.now).toBe(0)
    // the fold rebuilds both windows byte-for-byte from the log alone
    const geoContent = geo(60, 30)
    expect(snap(fold(r1.records.concat(r2.records), cloneState(seed()), dict, geoContent))).toBe(snap(state))
  })

  test('a bare clock advance resolves due completions before the slot events', () => {
    let state = seed()
    const chime = rigContent({
      PACE_A: { slots: [{ role: 'actors', min: 1, max: 1, query: 'kind:character' }], duration: 60, recipes: [{ out: ['NARRATE "a done."'] }] },
    }, { triggers: [{ id: 'the_chime', slot: 2, out: ['NARRATE "the halfhour chime sings."'] }] })
    const r = run(state, '0:0 @ ANA PACE_A', 1, chime) // window [0, 60]; slot 2 = minute 60
    state = r.state
    expect(state.windows.size).toBe(1)
    const adv = run(state, '0:2', r.records.length + 1, chime) // pure advance to minute 60
    expect(adv.rejected).toBeNull()
    // v0.5 order: the completion at minute 60 fires BEFORE slot-60 events
    expect(adv.records.map(x => x.kind)).toEqual(['complete', 'event', 'wait'])
    expect(adv.records[0]!.kind).toBe('complete')
    expect(adv.records[1]!.trigger).toBe('the_chime')
    expect(r.state.now).toBe(0) // the returned state of the BEGIN step — the advance's own state is below
    expect(adv.state.now).toBe(60)
    // the advance is one atomic turn ending with the clock row
    expect(adv.records.at(-1)!.kind).toBe('wait')
  })

  test('an advance pass is chronological: earlier completion first, tie goes to the completion', () => {
    const c = rigContent({
      SLOW: { slots: [{ role: 'actors', min: 1, max: 1, query: 'kind:character' }], duration: 90, recipes: [{ out: ['NARRATE "slow."'] }] },
      FAST: { slots: [{ role: 'actors', min: 1, max: 1, query: 'kind:character' }], duration: 30, recipes: [{ out: ['NARRATE "fast."'] }] },
    }, { triggers: [{ id: 'halfhour_mark', slot: 1, out: ['NARRATE "mark."'] }, { id: 'ninety_mark', slot: 3, out: ['NARRATE "ninety."'] }] })
    let state = seed()
    const r1 = run(state, '0:0 @ ANA SLOW', 1, c)
    const r2 = run(r1.state, '0:0 @ ANA FAST', r1.records.length + 1, c)
    state = r2.state
    const adv = run(state, '0:3', r1.records.length + r2.records.length + 1, c) // minute 90
    expect(adv.rejected).toBeNull()
    const shapes = adv.records.map(x => [x.kind, x.trigger ?? (x.verb || null)])
    // FAST closes at minute 30 (before the halfhour mark), SLOW at 90 (before
    // the ninety mark) — completions lead, same-minute events follow
    expect(shapes).toEqual([
      ['complete', 'FAST'], ['event', 'halfhour_mark'],
      ['complete', 'SLOW'], ['event', 'ninety_mark'],
      ['wait', null],
    ])
    expect(adv.state.now).toBe(90)
  })
})

describe('the locks', () => {
  const lockRig = (extra: Partial<ContentRaw> = {}): CompiledContent =>
    rigContent({
      SEIZE: {
        slots: [
          { role: 'actors', min: 1, max: 1, query: 'kind:character' },
          { role: 'object', min: 0, max: 1 },
        ],
        duration: 60,
        recipes: [{ out: ['NARRATE "seized."'] }],
      },
      TOUCH: { slots: [{ role: 'actors', min: 1, max: 1, query: 'kind:character' }, { role: 'object', min: 0, max: 1 }], duration: 30, recipes: [{ out: ['NARRATE "touched."'] }] },
    }, extra)

  test('station exclusivity: the same verb cannot open a second window', () => {
    let state = seed()
    const r1 = run(state, '0:0 @ ANA SEIZE', 1, lockRig())
    state = r1.state
    const r2 = run(state, '0:0 @ ANA SEIZE', r1.records.length + 1, lockRig())
    expect(r2.rejected).toMatch(/SEIZE is already processing/)
    expect(r2.records).toHaveLength(0)
    expect(r2.state).toBe(state)
  })

  test('committed cards are busy in any role; actors stay free', () => {
    let state = seed()
    const r1 = run(state, '0:0 @ ANA SEIZE MAGICK_BOOK', 1, lockRig())
    state = r1.state
    // the BOOK is locked — as object of another verb
    const r2 = run(state, '0:0 @ ANA TOUCH MAGICK_BOOK', r1.records.length + 1, lockRig())
    expect(r2.rejected).toMatch(/MAGICK BOOK is busy with SEIZE/)
    // …but the ACTOR is never locked: ANA can act with other cards
    const r3 = run(state, '0:0 @ ANA TOUCH', r1.records.length + 1, lockRig())
    expect(r3.rejected).toBeNull()
    expect(r3.state.windows.size).toBe(2)
  })

  test('locks release the moment a pass resolves their window', () => {
    let state = seed()
    const r1 = run(state, '0:0 @ ANA SEIZE MAGICK_BOOK', 1, lockRig())
    state = r1.state
    // an explicit future clock first advances (the lock frees at the begin
    // minute), then begins — the card is usable exactly when free
    const r2 = run(state, '1:0 @ ANA TOUCH MAGICK_BOOK', r1.records.length + 1, lockRig())
    expect(r2.rejected).toBeNull()
    expect(r2.records[0]!.kind).toBe('complete') // the SEIZE window closed en route
  })

  test('events do not respect locks: an expiry destroys a locked card and the completion drops the ref', () => {
    const c = rigContent({
      SEIZE: {
        slots: [
          { role: 'actors', min: 1, max: 1, query: 'kind:character' },
          { role: 'object', min: 0, max: 1 },
        ],
        duration: 60,
        recipes: [{ out: ['SET(OBJECT, aspect:touched, 1), DESTROY OBJECT'] }],
      },
    }, {})
    let state = seed()
    const r1 = run(state, '0:0 @ ANA SEIZE MAGICK_BOOK', 1, c)
    state = r1.state
    // the book expires mid-window — nothing stops the crossing (locks bind
    // steps, not events, §6.7 v0.5)
    state.entities.get(state.byName.get('MAGICK_BOOK')![0])!.attrs['expiresAt'] = 30
    const adv = run(state, '1:0', r1.records.length + 1, c)
    expect(adv.rejected).toBeNull()
    const complete = adv.records.find(x => x.kind === 'complete')!
    // the completion applied with the ref dropped: the SET found no targets
    // and the DESTROY concretized to an empty, harmless row
    expect(complete.outcomes.map(o => o.op)).toEqual(['DESTROY'])
    expect(complete.outcomes[0]!.args).toEqual([])
    expect(adv.state.byName.has('MAGICK_BOOK')).toBe(false) // the expiry took it
  })
})

describe('commit-time gates vs completion-time refusals', () => {
  test('a top-level REFUSE still rejects AT THE TABLE (zero time, attempt logged)', () => {
    const c = rigContent({
      GATE: { slots: [{ role: 'actors', min: 1, max: 1, query: 'kind:character' }], duration: 60, recipes: [{ out: ['REFUSE("not without a lantern.")'] }] },
    })
    const state = seed()
    const before = snap(state)
    const r = run(state, '0:0 @ ANA GATE', 1, c)
    expect(r.rejected).toBeNull()
    expect(r.records).toHaveLength(1)
    expect(r.records[0]!.outcomes.some(o => o.op === 'REFUSE')).toBe(true)
    expect(r.records[0]!.advancedMin).toBe(0)
    expect(r.state).toBe(state)
    expect(snap(state)).toBe(before)
  })

  test('an IF-branch REFUSE defers: the window opens, the completion refuses, effects discarded', () => {
    const c = rigContent({
      LATE: {
        slots: [{ role: 'actors', min: 1, max: 1, query: 'kind:character' }],
        duration: 30,
        recipes: [{ out: ['IF exists(HOUSE) -> (SPAWN PARANOIA, REFUSE("too late.")) ELSE -> (NARRATE "calm.")'] }],
      },
    }, { prototypes: { PARANOIA: { kind: 'state' } } })
    let state = seed()
    const r = run(state, '0:0 @ ANA LATE', 1, c)
    expect(r.rejected).toBeNull()
    state = r.state
    expect(state.windows.size).toBe(1) // the window DID open
    const adv = run(state, '0:31', r.records.length + 1, c)
    expect(adv.rejected).toBeNull()
    const complete = adv.records.find(x => x.kind === 'complete')!
    expect(complete.outcomes.some(o => o.op === 'REFUSE')).toBe(true)
    expect(adv.state.byName.has('PARANOIA')).toBe(false) // the scratch died with the refusal
    expect(adv.state.windows.size).toBe(0) // …and the window still closed
    // the fold agrees: the REFUSE row is a state no-op that closes its window
    expect(snap(fold(adv.records, cloneState(r.state), dict, c))).toBe(snap(adv.state))
  })
})

describe('completion-time failures refuse the window — never a livelock', () => {
  test('a completion that cannot apply closes the window and logs the attempt', () => {
    const c = rigContent({
      BAD: { slots: [{ role: 'actors', min: 1, max: 1, query: 'kind:character' }], duration: 30, recipes: [{ out: ['SPAWN NOSUCH_PROTOTYPE'] }] },
    })
    let state = seed()
    const r = run(state, '0:0 @ ANA BAD', 1, c)
    expect(r.rejected).toBeNull()
    state = r.state
    const adv = run(state, '0:1', r.records.length + 1, c)
    expect(adv.rejected).toBeNull() // the pass did NOT roll back
    const complete = adv.records.find(x => x.kind === 'complete')!
    expect(complete.outcomes.some(o => o.op === 'REFUSE')).toBe(true)
    expect(adv.state.windows.size).toBe(0) // the window is gone — no retry loop
    expect(adv.state.now).toBe(30)
    // a SECOND advance works: the pass is healthy again
    const adv2 = run(adv.state, '0:2', r.records.length + adv.records.length + 1, c)
    expect(adv2.rejected).toBeNull()
  })
})

describe('undo with open windows', () => {
  test('undo drops a whole turn; a log may end with open windows and folds to them', () => {
    let state = seed()
    const r1 = run(state, '0:0 @ ANA PACE_A', 1, geo(60, 30))
    state = r1.state
    const r2 = run(state, '0:0 @ ANA PACE_B', r1.records.length + 1, geo(60, 30))
    const all = [...r1.records, ...r2.records]
    // JSONL round-trip with both windows open: the fold rebuilds them
    const replayed = fold(loadLog(saveLogTmp(all)), cloneState(seed()), dict, geo(60, 30))
    expect(snap(replayed)).toBe(snap(r2.state))
    expect(replayed.windows.size).toBe(2)
    // undo turn 2: keep r1's records only
    const undone = fold(all.filter(x => x.id < r1.records.length + 1), cloneState(seed()), dict, geo(60, 30))
    expect(undone.windows.size).toBe(1)
    expect(undone.byName.size).toBe(r1.state.byName.size)
  })
})

// temp-file helper without importing node:fs at the top (kept local)
import { mkdtempSync, rmSync, writeFileSync, readFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'

function saveLogTmp(records: ActionRecord[]): string {
  const dir = mkdtempSync(join(tmpdir(), 'chorus-parallel-'))
  const path = join(dir, 'log.jsonl')
  try {
    saveLog(path, records)
    writeFileSync(path, readFileSync(path)) // touch: keep the file alive for loadLog below
    return path
  } finally {
    setTimeout(() => rmSync(dir, { recursive: true, force: true }), 0)
  }
}

describe('input surface', () => {
  test('a lone clock advances; the same minute with nothing due is rejected', () => {
    let state = seed()
    const idle = run(state, '0:0', 1)
    expect(idle.rejected).toMatch(/nothing to do/)
    const r = run(state, '0:0 @ ANA PACE_A', 1, geo(60, 30))
    state = r.state
    const adv = run(state, '0:1', r.records.length + 1, geo(60, 30))
    expect(adv.rejected).toBeNull()
    expect(adv.records.map(x => x.kind)).toEqual(['wait']) // the PACE_A window is not due yet
    expect(adv.state.now).toBe(30)
    const adv2 = run(adv.state, '0:2', r.records.length + adv.records.length + 1, geo(60, 30))
    expect(adv2.records.map(x => x.kind)).toEqual(['complete', 'wait']) // due at minute 60
    expect(adv2.state.windows.size).toBe(0)
  })

  test('beginning at a mid-halfhour minute is representable and folds exactly', () => {
    const c = rigContent({
      NAP: { slots: [{ role: 'actors', min: 1, max: 1, query: 'kind:character' }], duration: 20, recipes: [{ out: ['NARRATE "twenty minutes."'] }] },
    })
    let state = seed()
    const r1 = run(state, '0:0 @ ANA NAP', 1, c) // [0, 20] — mid-halfhour end
    state = r1.state
    const adv = run(state, '0:1', r1.records.length + 1, c)
    expect(adv.state.now).toBe(30)
    // a second NAP begins at minute 30 exactly (now, quantized input 0:1 = 30)
    const r2 = run(adv.state, '0:1 @ ANA NAP', r1.records.length + adv.records.length + 1, c)
    expect(r2.rejected).toBeNull()
    const w = [...r2.state.windows.values()][0]!
    expect(w.start).toBe(30)
    expect(w.end).toBe(50)
    expect(snap(fold(r2.records, cloneState(adv.state), dict, c))).toBe(snap(r2.state))
  })
})
