// Endings — the ENDING outcome, the `endings:` script section, and game-over
// discipline (spec §9.35): first ENDING in a step wins; the step completes
// normally; steps on an ended state are rejected with nothing logged; time
// triggers and authored expiries may ENDING (the CS despair-death shape);
// undo reopens the game because `ended` derives from the fold.
import { describe, expect, test } from 'vitest'
import { compileContent, type CompiledContent, type ContentRaw } from '../src/content.js'
import { executeStep, type ActionRecord, type ExecResult } from '../src/engine.js'
import { fold } from '../src/fold.js'
import { formatSentence } from '../src/format.js'
import { parseSentence } from '../src/parse.js'
import { endingBanner, endingNotice, proseLines } from '../src/render.js'
import { seedContent, seedDictionary, seedState } from '../src/seed.js'
import { loadLog, saveLog } from '../src/store.js'
import { Game } from '../src/play.js'
import { compileScript, parseScript } from '../src/script.js'
import { cloneState, type State } from '../src/types.js'
import { mkdtempSync, rmSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'

const dict = seedDictionary()

function snap(s: State): string {
  return JSON.stringify({
    ents: [...s.entities.entries()].sort(([a], [b]) => a < b ? -1 : 1),
    rels: [...s.relations.entries()].sort(([a], [b]) => a < b ? -1 : 1),
    byName: [...s.byName.entries()].sort(([a], [b]) => a < b ? -1 : 1),
    now: s.now,
    fired: [...s.fired].sort(),
    ended: s.ended, // §9.35: fold equality includes the story's ending
  })
}

const waitContent = (out: string[], extra: Partial<ContentRaw> = {}): CompiledContent =>
  compileContent({
    verbs: {
      WAIT: {
        slots: [{ role: 'actors', min: 1, max: 1, query: 'kind:character' }],
        duration: 60,
        recipes: [{ out }],
      },
    },
    ...extra,
  }, dict)

const endingsTable = {
  DESPAIR_DEATH: { title: 'No More' },
  LANTERN_VICTORY: { title: 'The Light in the Ruins', flavour: 'grand' },
}

function run(content: CompiledContent, state: State, src: string, firstRecordId = 1): ExecResult {
  const s = parseSentence(src, dict)
  let n = 0
  return executeStep(content, dict, state, { clock: s.clock ?? { day: 0, halfhour: 0 }, clauses: s.clauses }, {
    firstRecordId,
    idGen: () => `x${firstRecordId}_${++n}`,
  })
}

// ---------- the endings table (§10) ----------

describe('the `endings:` script section (§10)', () => {
  const script = `
script: The Long Fall
player: ANA
start: 0:0

entities:
  HOUSE is place
  ANA is character at HOUSE

endings:
  DESPAIR_DEATH: "No More"
  LANTERN_VICTORY: "The Light in the Ruins" (grand)

verbs:
  WAIT:
    actors: 1 kind:character
    duration: 1h
    recipes:
      default -> NARRATE "The candle gutters.", ENDING DESPAIR_DEATH
`

  test('the table compiles with resolved flavours (default melancholy)', () => {
    const { content } = compileScript(script, dict)
    expect(content.endings.get('DESPAIR_DEATH')).toEqual({ title: 'No More', flavour: 'melancholy' })
    expect(content.endings.get('LANTERN_VICTORY')).toEqual({ title: 'The Light in the Ruins', flavour: 'grand' })
  })

  test('duplicate ending ids fail at load', () => {
    expect(() => parseScript('endings:\n  DESPAIR_DEATH: "No More"\n  DESPAIR_DEATH: "Again"\n', dict))
      .toThrow(/duplicate ending id/)
  })

  test('bad ending lines fail loudly', () => {
    expect(() => parseScript('endings:\n  DESPAIR_DEATH No More\n', dict)).toThrow(/endings expect/)
    expect(() => parseScript('endings:\n  despair_death: "No More"\n', dict)).toThrow(/endings expect/) // UPPER_SNAKE only
    expect(() => parseScript('endings:\n  DESPAIR_DEATH: No More\n', dict)).toThrow(/endings expect/) // title must be quoted
  })

  test('compile failures: ENDING with an undeclared id — recipe and trigger alike', () => {
    expect(() => compileContent({
      verbs: { WAIT: { slots: [{ role: 'actors', min: 1, max: 1 }], duration: 0, recipes: [{ out: ['ENDING NOPE'] }] } },
    }, dict)).toThrow(/recipe wait\.default: ENDING 'NOPE' is not in the endings table/)
    expect(() => compileContent({
      verbs: {},
      triggers: [{ id: 't', slot: 1, out: ['ENDING NOPE'] }],
      endings: endingsTable,
    }, dict)).toThrow(/trigger t: ENDING 'NOPE' is not in the endings table/)
    expect(() => compileContent({
      verbs: {},
      triggers: [{ id: 't', onExpireOf: 'CHALICE', out: ['ENDING NOPE'] }],
      endings: endingsTable,
    }, dict)).toThrow(/trigger t: ENDING 'NOPE' is not in the endings table/) // both trigger forms
  })

  test('raw-content discipline: UPPER_SNAKE ids, titled, lowercase flavour', () => {
    expect(() => compileContent({ verbs: {}, endings: { nope: { title: 'x' } } }, dict)).toThrow(/UPPER_SNAKE/)
    expect(() => compileContent({ verbs: {}, endings: { NOPE: { title: '' } } }, dict)).toThrow(/needs a title/)
    expect(() => compileContent({ verbs: {}, endings: { NOPE: { title: '   ' } } }, dict)).toThrow(/needs a title/)
    expect(() => compileContent({ verbs: {}, endings: { NOPE: { title: 'x', flavour: 'Grand' } } }, dict)).toThrow(/lowercase word/)
  })

  test('two scripts compiled side by side keep separate ending tables (no bleed)', () => {
    const A = compileScript('endings:\n  ONLY_A: "A Title"\n', dict).content
    const B = compileScript('endings:\n  ONLY_B: "B Title"\n', dict).content
    expect(A.endings.has('ONLY_A')).toBe(true)
    expect(A.endings.has('ONLY_B')).toBe(false)
    expect(B.endings.has('ONLY_A')).toBe(false)
    // and a cross-script reference is a load-time error, not a silent typo
    expect(() => compileContent({
      verbs: { WAIT: { slots: [{ role: 'actors', min: 1, max: 1 }], duration: 0, recipes: [{ out: ['ENDING ONLY_B'] }] } },
      endings: { ONLY_A: { title: 'A Title' } },
    }, dict)).toThrow(/ENDING 'ONLY_B' is not in the endings table/)
  })
})

// ---------- the ENDING outcome: parse, format, round-trip ----------

describe('the ENDING outcome — paren-less, round-tripping', () => {
  test('parses as a ref-form id and canonicalizes to itself', () => {
    const s = parseSentence('0:0 @ ANA WAIT -> ENDING DESPAIR_DEATH', dict)
    expect(s.outcomes[0]).toEqual({ op: 'ENDING', args: [{ t: 'ref', ref: { form: 'name', name: 'DESPAIR_DEATH' } }] })
    expect(formatSentence(s, dict)).toBe('0:0 @ ANA WAIT -> ENDING DESPAIR_DEATH')
    // quoted ids round-trip too (an id that collides with a reserved word)
    const q = parseSentence('0:0 @ ANA WAIT -> ENDING "WAIT"', dict)
    expect(formatSentence(q, dict)).toBe('0:0 @ ANA WAIT -> ENDING "WAIT"')
  })

  test('the paren form is a parse error — paren-less only (§9.35)', () => {
    expect(() => parseSentence('0:0 @ ANA WAIT -> ENDING(DESPAIR_DEATH)', dict)).toThrow(/paren-less/)
  })
})

// ---------- a recipe that ends the story (§9.35) ----------

describe('a recipe that ends the story (§9.35)', () => {
  const content = waitContent(
    ['NARRATE("The candle gutters.")', 'ENDING DESPAIR_DEATH'],
    { endings: endingsTable },
  )

  test('the step applies, ends the story, and the clock rides along', () => {
    const state = seedState()
    const r = run(content, state, '0:28 @ ANA WAIT')
    expect(r.rejected).toBeNull()
    expect(r.state.ended).toEqual({ id: 'DESPAIR_DEATH', title: 'No More', flavour: 'melancholy' })
    expect(r.state.now).toBe(14 * 60 + 60) // the ending does not stop the clock
    expect(r.records[0].advancedMin).toBe(60)
    expect(r.records[0].outcomes.map(o => o.op)).toEqual(['NARRATE', 'ENDING'])
  })

  test('the record carries the concrete ENDING and round-trips', () => {
    const state = seedState()
    const r = run(content, state, '0:28 @ ANA WAIT')
    const rec: ActionRecord = r.records[0]
    expect(rec.outcomes[1]).toEqual({ op: 'ENDING', args: [{ t: 'ref', ref: { form: 'name', name: 'DESPAIR_DEATH' } }] })
    expect(rec.sentence).toContain('-> NARRATE "The candle gutters.", ENDING DESPAIR_DEATH')
    expect(formatSentence(parseSentence(rec.sentence, dict), dict)).toBe(rec.sentence)
  })

  test('explicit flavour resolves through; first ENDING in one step wins', () => {
    const twoEndings = waitContent(['ENDING LANTERN_VICTORY', 'ENDING DESPAIR_DEATH'], { endings: endingsTable })
    const r = run(twoEndings, seedState(), '0:28 @ ANA WAIT')
    expect(r.rejected).toBeNull()
    expect(r.state.ended).toEqual({ id: 'LANTERN_VICTORY', title: 'The Light in the Ruins', flavour: 'grand' })
    // both are logged; only the first moved the state
    expect(r.records[0].outcomes.map(o => o.op)).toEqual(['ENDING', 'ENDING'])
  })

  test('further steps are rejected with nothing logged — the state object untouched', () => {
    const state = seedState()
    const r1 = run(content, state, '0:28 @ ANA WAIT')
    const before = snap(r1.state)
    const r2 = run(content, r1.state, '0:30 @ ANA WAIT', 2)
    expect(r2.rejected).toMatch(/the story has ended — No More/)
    expect(r2.records).toHaveLength(0)
    expect(r2.state).toBe(r1.state)
    expect(snap(r2.state)).toBe(before)
  })

  test('a REFUSE after the ENDING discards the ending with the whole draft (§6.5)', () => {
    const c = waitContent(['ENDING DESPAIR_DEATH', 'REFUSE("not yet")'], { endings: endingsTable })
    const state = seedState()
    const r = run(c, state, '0:28 @ ANA WAIT')
    expect(r.records[0].outcomes.map(o => o.op)).toEqual(['ENDING', 'REFUSE'])
    expect(r.state).toBe(state)
    expect(r.state.ended).toBeNull()
    // and the discarded ending is never announced in prose (§6.5)…
    const lines = proseLines(r.records[0], c, dict)
    expect(lines).toContain('not yet') // the refusal itself is spoken (§6.2)
    expect(lines.join('\n')).not.toContain('— No More —')
  })

  test('the game-over guard is first: stale clocks, EVENT sentences, multi-clause steps all bounce', () => {
    const state = seedState()
    const r1 = run(content, state, '0:28 @ ANA WAIT')
    expect(r1.state.ended).not.toBeNull()
    // a stale clock (would be a §9.5 rejection) and a multi-clause step would
    // both get past their own checks — the ended guard must precede them
    const attempts: ExecResult[] = [
      run(content, r1.state, '0:10 @ ANA WAIT', 2),
      run(content, r1.state, '0:30 @ ANA WAIT; ANA WAIT', 2),
    ]
    // an EVENT-typed sentence reaches executeStep with zero clauses (canonical passthrough)
    const ev = parseSentence('0:40 @ EVENT dusk_falls -> ENDING DESPAIR_DEATH', dict)
    attempts.push(executeStep(content, dict, r1.state, { clock: ev.clock ?? { day: 0, halfhour: 0 }, clauses: ev.clauses }, { firstRecordId: 2 }))
    for (const a of attempts) {
      expect(a.rejected).toMatch(/the story has ended — No More/)
      expect(a.records).toHaveLength(0) // nothing logged, never a throw
      expect(a.state).toBe(r1.state) // the state object untouched
    }
  })

  test('fold equality incl. `ended`, through JSONL', () => {
    const state = seedState()
    const r1 = run(content, state, '0:28 @ ANA WAIT')
    const r2 = run(content, r1.state, '0:30 @ ANA WAIT', 2) // rejected; nothing logged
    expect(r2.records).toHaveLength(0)
    const dir = mkdtempSync(join(tmpdir(), 'chorus-log-'))
    const path = join(dir, 'log.jsonl')
    try {
      saveLog(path, r1.records)
      const replayed = fold(loadLog(path), seedState(), dict, content)
      expect(snap(replayed)).toBe(snap(r1.state))
      expect(replayed.ended).toEqual({ id: 'DESPAIR_DEATH', title: 'No More', flavour: 'melancholy' })
    } finally {
      rmSync(dir, { recursive: true, force: true })
    }
  })
})

// ---------- events may end the game (§9.35) ----------

describe('events may end the game (§9.35)', () => {
  test('a time-trigger chain ends the game when the clock crosses (the despair-death shape)', () => {
    const c = waitContent(['NARRATE("{actors} wait.")'], {
      endings: endingsTable,
      triggers: [{ id: 'despair', slot: 32, out: ['SPAWN DREAD', 'ENDING DESPAIR_DEATH'] }], // 16:00
      prototypes: { DREAD: { kind: 'state' } },
    })
    const state = seedState()
    const r = run(c, state, '0:30 @ ANA WAIT') // 15:00 → 16:00, crossing slot 32
    expect(r.rejected).toBeNull()
    expect(r.records.map(x => x.kind)).toEqual(['action', 'event'])
    const event = r.records[1]
    expect(event.trigger).toBe('despair')
    expect(event.outcomes.some(o => o.op === 'ENDING')).toBe(true)
    expect(r.state.ended).toEqual({ id: 'DESPAIR_DEATH', title: 'No More', flavour: 'melancholy' })
    // the fold trusts the EVENT row: replaying alone re-ends the story
    const replayed = fold(r.records, seedState(), dict, c)
    expect(snap(replayed)).toBe(snap(r.state))
  })

  test('an authored-expiry trigger may ENDING — and must consume the card itself (§9.32)', () => {
    const c = waitContent(['NARRATE("{actors} wait.")'], {
      endings: endingsTable,
      // the authored outcomes replace the built-in destroy entirely (§9.32),
      // so the despair-death author destroys the dying card explicitly
      triggers: [{ id: 'the_lights_go_out', onExpireOf: 'CHALICE', out: ['DESTROY CHALICE', 'NARRATE("The candle drowns in its own wax.")', 'ENDING DESPAIR_DEATH'] }],
    })
    const base = seedState()
    base.entities.get('chalice')!.attrs['expiresAt'] = 15 * 60 // 0:30
    const r = run(c, cloneState(base), '0:28 @ ANA WAIT; ANA WAIT') // 14:00 → 16:00, expiry at 15:00
    expect(r.rejected).toBeNull()
    expect(r.records.map(x => x.kind)).toEqual(['action', 'event'])
    expect(r.records[1].trigger).toBe('the_lights_go_out')
    expect(r.state.entities.get('chalice')!.deadAction).toBe(2)
    expect(r.state.ended).toEqual({ id: 'DESPAIR_DEATH', title: 'No More', flavour: 'melancholy' })
    expect(snap(fold(r.records, cloneState(base), dict, c))).toBe(snap(r.state))
  })

  test('an authored-expiry trigger that does NOT destroy its card re-fires (§9.32, §9.35)', () => {
    // the authored outcomes replace the built-in destroy entirely — forget the
    // DESTROY and the card survives, re-firing on every later crossing
    const c = waitContent(['NARRATE("{actors} wait.")'], {
      endings: endingsTable,
      triggers: [{ id: 'flicker', onExpireOf: 'CHALICE', out: ['NARRATE("the candle drowns.")'] }],
    })
    const base = seedState()
    base.entities.get('chalice')!.attrs['expiresAt'] = 15 * 60 // 0:30
    const r1 = run(c, cloneState(base), '0:28 @ ANA WAIT') // 14:00 → 15:00 — the expiry crossing
    expect(r1.records.filter(x => x.kind === 'event' && x.trigger === 'flicker')).toHaveLength(1)
    const r2 = run(c, r1.state, '0:30 @ ANA WAIT', 3) // 15:00 → 16:00 — still past expiresAt
    expect(r2.records.filter(x => x.kind === 'event' && x.trigger === 'flicker').length).toBeGreaterThanOrEqual(1)
    expect(r2.state.entities.get('chalice')!.deadAction).toBeNull() // never consumed
  })

  test('action outranks event on first-wins within one advance', () => {
    const c = waitContent(['ENDING LANTERN_VICTORY'], {
      endings: endingsTable,
      triggers: [{ id: 'despair', slot: 32, out: ['ENDING DESPAIR_DEATH'] }],
    })
    const r = run(c, seedState(), '0:30 @ ANA WAIT') // the step crosses slot 32
    expect(r.rejected).toBeNull()
    expect(r.state.ended?.id).toBe('LANTERN_VICTORY') // the action's ENDING applied first
  })
})

// ---------- the player surface (§11) ----------

const gameScript = `
script: The Long Fall
player: ANA
start: 0:0

entities:
  HOUSE is place
  ANA is character at HOUSE

endings:
  DESPAIR_DEATH: "No More"
  LANTERN_VICTORY: "The Light in the Ruins" (grand)

verbs:
  WAIT:
    actors: 1 kind:character
    duration: 1h
    recipes:
      default -> NARRATE "The candle gutters.", ENDING DESPAIR_DEATH

templates:
  WAIT says "{actors} wait."
`

describe('the player surface (§11, §9.35)', () => {
  test('the ending turn tells the closing line; the journal stays canonical', () => {
    const game = Game.fromScript(gameScript, dict)
    const turn = game.handleLine('wait')
    expect(turn.kind).toBe('action')
    expect(turn.lines.join('\n')).toContain('— No More —')
    expect(game.state.ended).toEqual({ id: 'DESPAIR_DEATH', title: 'No More', flavour: 'melancholy' })
    expect(game.journal().join('\n')).toContain('ENDING DESPAIR_DEATH')
    expect(game.story().join('\n')).toContain('— No More —')
  })

  test('after the ending: the notice instead of execution, and no suggestions', () => {
    const game = Game.fromScript(gameScript, dict)
    game.handleLine('wait')
    const nRecords = game.log.records.length
    const turn = game.handleLine('wait')
    expect(turn.kind).toBe('info')
    expect(turn.lines).toEqual([endingNotice(game.state.ended!)])
    expect(game.log.records.length).toBe(nRecords) // nothing logged
    expect(game.suggestions()).toEqual([])
  })

  test('undo across the ending step reopens the game — ended derives from the fold', () => {
    const game = Game.fromScript(gameScript, dict)
    game.handleLine('wait')
    expect(game.state.ended).not.toBeNull()
    expect(game.undo().kind).toBe('info')
    expect(game.state.ended).toBeNull() // no special-case: re-fold before the ending record
    const turn = game.handleLine('wait')
    expect(turn.kind).toBe('action') // play resumes
    expect(game.state.ended).toEqual({ id: 'DESPAIR_DEATH', title: 'No More', flavour: 'melancholy' })
  })

  test('the banner and the notice render title + flavour', () => {
    expect(endingBanner({ id: 'DESPAIR_DEATH', title: 'No More', flavour: 'melancholy' })).toBe('— NO MORE · melancholy —')
    expect(endingBanner({ id: 'LANTERN_VICTORY', title: 'The Light in the Ruins', flavour: 'grand' })).toBe('— THE LIGHT IN THE RUINS · grand —')
    // flavour is open lowercase vocabulary — any word renders, no crash
    expect(endingBanner({ id: 'QUIET_END', title: 'Hush', flavour: 'quiet' })).toBe('— HUSH · quiet —')
    expect(endingNotice({ id: 'DESPAIR_DEATH', title: 'No More', flavour: 'melancholy' })).toBe('the story has ended — No More (melancholy).')
  })

  test('the prose of the ending step carries the closing line; others do not', () => {
    // outcomes come from the fired recipe (executeStep ignores a sentence's
    // own `->` tail), so author the ending in the recipe
    const content = waitContent(['NARRATE("last light.")', 'ENDING DESPAIR_DEATH'], { endings: endingsTable })
    const r = run(content, seedState(), '0:28 @ ANA WAIT')
    const lines = proseLines(r.records[0], content, dict)
    expect(lines[lines.length - 1]).toBe('— No More —')
    const plain = proseLines(run(waitContent(['NARRATE("just waiting.")'], { endings: endingsTable }), seedState(), '0:28 @ ANA WAIT').records[0], content, dict)
    expect(plain.join('\n')).not.toContain('— No More —')
  })

  test('cloneState copies the ending (undo-safe projections)', () => {
    const game = Game.fromScript(gameScript, dict)
    game.handleLine('wait')
    const c = cloneState(game.state)
    expect(c.ended).toEqual(game.state.ended)
    c.ended!.title = 'mutated'
    expect(game.state.ended!.title).toBe('No More')
  })
})
