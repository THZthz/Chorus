// F5 — decks: seed-fixed permutation draws + the DRAW op (spec §9.42, plan
// v0.4 F5). The fixed draw order is a PERMUTATION OVER POSITIONS of the deck's
// spec (multiset preserved — repeats weight the draw), derived purely from
// (seed, deck id) at load with dice.ts's splitmix64 mixer: identical at play
// and fold, consuming NO rng counter (the deck streams and the dice stream are
// independent key spaces). State.decks holds one cursor per deck; play
// advances it on the draft, the fold lands it from the engine-written
// DRAWN(<deck>, <name>) row — which also asserts the recorded name against
// the compiled permutation, so content drift between play and fold fails loud.
import { describe, expect, test } from 'vitest'
import { readdirSync, readFileSync, mkdtempSync, rmSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { compileContent, drawFrom, permutePositions, type CompiledContent, type ContentRaw } from '../src/content.js'
import { executeStep, type ActionRecord, type ExecResult } from '../src/engine.js'
import { fold } from '../src/fold.js'
import { formatSentence } from '../src/format.js'
import { Game } from '../src/play.js'
import { parseOutcomes, parseSentence } from '../src/parse.js'
import { compileScript } from '../src/script.js'
import { seedDictionary, seedState } from '../src/seed.js'
import { loadLog, saveLog } from '../src/store.js'
import { cloneState, type State } from '../src/types.js'
import { snap } from './snap.js'

const dict = seedDictionary()
// the rig's second verb lexeme (verbs are code-registered, §10) — an
// actors+at shape for the placement-default tests
dict.registerVerb('SEEK')

function run(content: CompiledContent, state: State, src: string, firstRecordId = 1): ExecResult {
  const s = parseSentence(src, dict)
  let n = 0
  return executeStep(content, dict, state, { clock: s.clock ?? { day: 0, halfhour: 0 }, clauses: s.clauses }, {
    firstRecordId,
    idGen: () => `x${firstRecordId}_${++n}`,
  })
}

/** the drawn names of a record, in order (the DRAWN row's second arg) */
const drawnNames = (r: ExecResult): string[] =>
  r.records.flatMap(rec => rec.outcomes.filter(o => o.op === 'DRAWN').map(o => {
    const a = o.args[1]
    return a && a.t === 'ref' && a.ref.form === 'name' ? a.ref.name : '(?)'
  }))

// ---------- the rigs ----------

interface RigOpts {
  seed?: number
  entities?: string
  prototypes?: string
  decks?: string
  triggers?: string
  functions?: string
  verbs?: string
  /** which deck the default WAIT verb draws (default RUMOURS) — the DRAW
   *  check pins every verb's deck at load, so custom decks name it here */
  drawDeck?: string
}

/** the deck rig: ANA at HOUSE, KITCHEN inside HOUSE, stackable gossip
 *  prototypes by default, and a `default -> DRAW RUMOURS` WAIT verb */
const rig = (o: RigOpts = {}): string => `
script: Deck Rig
player: ANA
start: 0:0
seed: ${o.seed ?? 0}

entities:
  ANA is character at HOUSE
  HOUSE is place
  KITCHEN is place in HOUSE
${o.entities ?? ''}

prototypes:
${o.prototypes ?? `  RUMOUR is idea stackable
  HEARSAY is idea stackable
  SCANDAL is idea stackable`}
${o.decks ? `
decks:
${o.decks}` : ''}
${o.triggers ? `
triggers:
${o.triggers}` : ''}
${o.functions ? `
functions:
${o.functions}` : ''}

verbs:
${o.verbs ?? `  WAIT:
    actors: 1 kind:character
    duration: 0m
    recipes:
      default -> DRAW ${o.drawDeck ?? 'RUMOURS'}`}
`

// the plan's §5 example deck, on the default rig (seed 0 order: RUMOUR,
// HEARSAY, RUMOUR, SCANDAL — pinned below via permutePositions)
const PLAN_DECK = 'RUMOURS spec RUMOUR, RUMOUR, HEARSAY, SCANDAL defaultcard RUMOUR reset'

function compile(text: string): { content: CompiledContent; state: State } {
  const { content, state } = compileScript(text, dict)
  return { content, state }
}

/** play `n` WAITs, returning the drawn names, the records, and the final state */
function playDraws(content: CompiledContent, state: State, n: number): { names: string[]; records: ActionRecord[]; state: State } {
  const names: string[] = []
  const records: ActionRecord[] = []
  let id = 1
  for (let i = 0; i < n; i++) {
    const r = run(content, state, '0:0 @ ANA WAIT', id)
    if (r.rejected) throw new Error(`unexpected rejection: ${r.rejected}`)
    names.push(...drawnNames(r))
    records.push(...r.records)
    id += r.records.length
    state = r.state
  }
  return { names, records, state }
}

// ---------- permutation (§9.42): seed-fixed, position-based, multiset-safe ----------

describe('the permutation (§9.42): pure in (seed, deck id), positions shuffled', () => {
  test('same seed + deck id -> the identical draw sequence across two fresh compiles', () => {
    const a = compile(rig({ seed: 0, decks: PLAN_DECK }))
    const b = compile(rig({ seed: 0, decks: PLAN_DECK }))
    const ra = playDraws(a.content, a.state, 6)
    const rb = playDraws(b.content, b.state, 6)
    expect(ra.names).toEqual(rb.names)
    expect(ra.names.slice(0, 4)).toEqual(['RUMOUR', 'HEARSAY', 'RUMOUR', 'SCANDAL']) // seed 0 order, pinned
  })

  test('different seed -> different order (seed pair 0 vs 42 provably differs)', () => {
    // probability note: the shuffle keys are pinned splitmix64 values, so this
    // is not a probabilistic assert — permutePositions(4, 0, 'RUMOURS') =
    // [0,2,1,3] and permutePositions(4, 42, 'RUMOURS') = [3,0,1,2] provably
    // differ for this (seed, deck id) pair. The n=7 and negative-seed vectors
    // were computed independently (python3, arbitrary-precision integers)
    // against the same algorithm, like dice.ts's vector table.
    expect(permutePositions(4, 0, 'RUMOURS')).toEqual([0, 2, 1, 3])
    expect(permutePositions(4, 42, 'RUMOURS')).toEqual([3, 0, 1, 2])
    expect(permutePositions(7, 0, 'SEVEN')).toEqual([0, 6, 3, 1, 4, 5, 2])
    expect(permutePositions(7, -3, 'NEG')).toEqual([4, 0, 2, 6, 3, 1, 5])
    const a = compile(rig({ seed: 0, decks: PLAN_DECK }))
    const b = compile(rig({ seed: 42, decks: PLAN_DECK }))
    const ra = playDraws(a.content, a.state, 4)
    const rb = playDraws(b.content, b.state, 4)
    expect(ra.names).toEqual(['RUMOUR', 'HEARSAY', 'RUMOUR', 'SCANDAL']) // seed 0
    expect(rb.names).toEqual(['SCANDAL', 'RUMOUR', 'RUMOUR', 'HEARSAY']) // seed 42
    expect(ra.names).not.toEqual(rb.names)
  })

  test('multiset weighting intact: A, A, B serves A twice per period', () => {
    const c = compile(rig({ decks: 'WEIGHTED spec RUMOUR, RUMOUR, SCANDAL defaultcard SCANDAL reset', drawDeck: 'WEIGHTED' }))
    const { names } = playDraws(c.content, c.state, 3)
    expect([...names].sort()).toEqual(['RUMOUR', 'RUMOUR', 'SCANDAL']) // 2:1 weighting preserved
  })

  test('the permutation lives on CompiledContent, not State (decks start empty; no rng consumed)', () => {
    const { content, state } = compile(rig({ decks: PLAN_DECK }))
    expect(content.decks.get('RUMOURS')!.order.map(e => e.name)).toEqual(['RUMOUR', 'HEARSAY', 'RUMOUR', 'SCANDAL'])
    expect(state.decks.size).toBe(0) // cursors only, and none yet
    expect(state.rng).toBe(0)
    const after = playDraws(content, state, 4)
    expect(after.state.rng).toBe(0) // decks never touch the dice counter (§9.42)
  })

  test('drawFrom: below the period serves order[cursor]; a single-entry deck always serves it', () => {
    const { content } = compile(rig({ decks: PLAN_DECK }))
    const deck = content.decks.get('RUMOURS')!
    expect(drawFrom(deck, 0).name).toBe('RUMOUR')
    expect(drawFrom(deck, 3).name).toBe('SCANDAL')
    expect(drawFrom(deck, 4).name).toBe('RUMOUR') // reset wraps: period 4
    expect(drawFrom(deck, 9).name).toBe('HEARSAY') // 9 % 4 = 1
    const one = compile(rig({ decks: 'SOLO spec HEARSAY reset', drawDeck: 'SOLO' })).content.decks.get('SOLO')!
    expect(drawFrom(one, 17).name).toBe('HEARSAY')
  })
})

// ---------- the DRAW op: placement ----------

describe('the DRAW op (§9.42): INTO placement and the SPAWN defaults', () => {
  test('DRAW INTO a place lands the card there', () => {
    const c = compile(rig({ decks: PLAN_DECK, verbs: `  WAIT:
    actors: 1 kind:character
    duration: 0m
    recipes:
      default -> DRAW RUMOURS INTO KITCHEN` }))
    const r = run(c.content, c.state, '0:0 @ ANA WAIT')
    expect(r.rejected).toBeNull()
    const rumour = [...r.state.entities.values()].find(e => e.name === 'RUMOUR')!
    expect(rumour.attrs['container']).toBe('s_kitchen')
  })

  test('no INTO: the at-card default wins over the actor container', () => {
    const c = compile(rig({ decks: PLAN_DECK, verbs: `  SEEK:
    actors: 1 kind:character
    at: 0..1 kind:place
    duration: 0m
    recipes:
      default -> DRAW RUMOURS` }))
    const atCard = run(c.content, c.state, '0:0 @ ANA AT KITCHEN SEEK')
    expect(atCard.rejected).toBeNull()
    const placed = [...atCard.state.entities.values()].find(e => e.name === 'RUMOUR')!
    expect(placed.attrs['container']).toBe('s_kitchen')
    // without AT, the mint goes to the actor's container (ANA is at HOUSE);
    // cursor 1 serves HEARSAY at seed 0 — look up whatever was drawn
    const carried = run(c.content, atCard.state, '0:0 @ ANA SEEK', 2)
    expect(carried.rejected).toBeNull()
    const drawn = drawnNames(carried)[0]!
    const second = [...carried.state.entities.values()].find(e => e.name === drawn)!
    expect(second.attrs['container']).toBe('s_house')
    expect(second.attrs['container']).not.toBe(placed.attrs['container'])
  })

  test('a trigger DRAW without INTO mints containerless (event bindings are empty — documented)', () => {
    const c = compile(rig({
      decks: PLAN_DECK,
      triggers: 'rumour_hour at hour 1 -> DRAW RUMOURS',
      verbs: `  WAIT:
    actors: 1 kind:character
    duration: 1h
    recipes:
      default -> NARRATE "{actors} wait."`,
    }))
    const r = run(c.content, c.state, '0:0 @ ANA WAIT') // 1h crosses hour 1 (slot 2)
    expect(r.rejected).toBeNull()
    const event = r.records.find(x => x.kind === 'event')!
    expect(event.outcomes.map(o => o.op)).toEqual(['DRAWN', 'SPAWN'])
    const rumour = [...r.state.entities.values()].find(e => e.name === 'RUMOUR')!
    expect(rumour.attrs['container']).toBeUndefined() // the §9.42 empty-bindings note
    expect(snap(fold(r.records, cloneState(c.state), dict, c.content))).toBe(snap(r.state))
  })

  test('a trigger DRAW with an explicit INTO lands in the named place', () => {
    const c = compile(rig({
      decks: PLAN_DECK,
      triggers: 'rumour_hour at hour 1 -> DRAW RUMOURS INTO HOUSE',
      verbs: `  WAIT:
    actors: 1 kind:character
    duration: 1h
    recipes:
      default -> NARRATE "{actors} wait."`,
    }))
    const r = run(c.content, c.state, '0:0 @ ANA WAIT')
    expect(r.rejected).toBeNull()
    const rumour = [...r.state.entities.values()].find(e => e.name === 'RUMOUR')!
    expect(rumour.attrs['container']).toBe('s_house')
    expect(snap(fold(r.records, cloneState(c.state), dict, c.content))).toBe(snap(r.state))
  })

  test('an INTO that cannot resolve rejects the step (§9.19: nothing logged, cursor unmoved)', () => {
    const c = compile(rig({ decks: PLAN_DECK, verbs: `  WAIT:
    actors: 1 kind:character
    duration: 0m
    recipes:
      default -> DRAW RUMOURS INTO OBJECT` }))
    const state = c.state
    const before = snap(state)
    const r = run(c.content, state, '0:0 @ ANA WAIT') // no object bound
    expect(r.rejected).toMatch(/INTO target of 'RUMOURS' cannot resolve/)
    expect(r.records).toHaveLength(0)
    expect(state.decks.get('RUMOURS')).toBeUndefined() // the draft died with the rejection
    expect(snap(state)).toBe(before)
    expect(fold([], cloneState(state), dict, c.content).decks.size).toBe(0)
  })
})

// ---------- DRAWN+SPAWN concretization, parse+format, authoring guards ----------

describe('DRAWN+SPAWN concretization (§9.42): two rows, round-trip, guards', () => {
  test('the record carries DRAWN(deck, name) then the concrete SPAWN; the row round-trips', () => {
    const c = compile(rig({ decks: PLAN_DECK }))
    const r = run(c.content, c.state, '0:0 @ ANA WAIT')
    expect(r.rejected).toBeNull()
    const rec = r.records[0]!
    expect(rec.outcomes.map(o => o.op)).toEqual(['DRAWN', 'SPAWN'])
    const drawn = rec.outcomes[0]!
    expect(drawn.args.map(a => a.t)).toEqual(['ref', 'ref'])
    const deckArg = drawn.args[0]!, nameArg = drawn.args[1]!
    if (deckArg.t !== 'ref' || deckArg.ref.form !== 'name' || nameArg.t !== 'ref' || nameArg.ref.form !== 'name') throw new Error('unreachable')
    expect(deckArg.ref.name).toBe('RUMOURS')
    expect(nameArg.ref.name).toBe('RUMOUR') // seed 0 serves RUMOUR first
    expect(rec.sentence).toContain('DRAWN(RUMOURS, RUMOUR)')
    expect(rec.sentence).toContain('SPAWN RUMOUR')
    expect(formatSentence(parseSentence(rec.sentence, dict), dict)).toBe(rec.sentence)
  })

  test('the row round-trips for an UNSAFE card name too — the quoted spelling re-parses', () => {
    // a name failing SAFE_NAME formats re-quoted through formatRef; the DRAWN
    // paren path must accept ref's quoted-string form or the recorded row
    // would be unparseable (law 2 — the script parser accepts such names in
    // entity lines, so this shape is reachable)
    const c = compile(rig({
      entities: '  X-Y is item stackable at HOUSE', // stackable: the living declaration shares its name with the mint
      decks: 'PAIR spec X-Y defaultcard X-Y',
      drawDeck: 'PAIR',
    }))
    const r = run(c.content, c.state, '0:0 @ ANA WAIT')
    expect(r.rejected).toBeNull()
    const sentence = r.records[0]!.sentence
    expect(sentence).toContain('DRAWN(PAIR, "X-Y")')
    expect(formatSentence(parseSentence(sentence, dict), dict)).toBe(sentence)
    // and the DRAWN assertion reads the same name back at fold
    expect(fold(r.records, cloneState(c.state), dict, c.content).decks.get('PAIR')).toBe(1)
  })

  test('the DRAWN parse+format entry: parses, pins its shape, formats bare', () => {
    const parsed = parseOutcomes('DRAWN(RUMOURS, HEARSAY)', dict)
    expect(parsed.map(o => o.op)).toEqual(['DRAWN'])
    expect(parsed[0]!.args.map(a => a.t)).toEqual(['ref', 'ref'])
    expect(formatSentence(parseSentence('0:0 @ ANA WAIT -> DRAWN(RUMOURS, HEARSAY)', dict), dict))
      .toBe('0:0 @ ANA WAIT -> DRAWN(RUMOURS, HEARSAY)')
    expect(() => parseOutcomes('DRAWN(RUMOURS)', dict)).toThrow(/DRAWN takes exactly two name refs/)
    expect(() => parseOutcomes('DRAWN(RUMOURS, HEARSAY, SCANDAL)', dict)).toThrow(/DRAWN takes exactly two name refs/)
    expect(() => parseOutcomes('DRAWN(1, 2)', dict)).toThrow(/DRAWN takes exactly two name refs/)
    expect(() => parseOutcomes('DRAWN RUMOURS, HEARSAY', dict)).toThrow(/requires parentheses/)
  })

  test('authoring DRAWN is a load error (engine-written only, like ROLLED)', () => {
    expect(() => compileContent({
      verbs: { WAIT: { slots: [{ role: 'actors', min: 1, max: 1 }], duration: 0, recipes: [{ out: ['DRAWN(RUMOURS, RUMOUR)'] }] } },
      decks: [{ id: 'RUMOURS', spec: ['RUMOUR'] }],
      prototypes: { RUMOUR: { kind: 'idea' } },
    }, dict)).toThrow(/recipe wait\.default: DRAWN is engine-written/)
    expect(() => compileContent({
      verbs: {},
      triggers: [{ id: 't', slot: 1, out: ['DRAWN(RUMOURS, RUMOUR)'] }],
      decks: [{ id: 'RUMOURS', spec: ['RUMOUR'] }],
      prototypes: { RUMOUR: { kind: 'idea' } },
    }, dict)).toThrow(/trigger t: DRAWN is engine-written/)
    // inside a function body: expansion runs before the lint, so the call
    // site's compile names the recipe
    expect(() => compileContent({
      verbs: { WAIT: { slots: [{ role: 'actors', min: 1, max: 1 }], duration: 0, recipes: [{ out: ['helper()'] }] } },
      functions: [{ name: 'helper', params: [], body: 'DRAWN(RUMOURS, RUMOUR)' }],
      decks: [{ id: 'RUMOURS', spec: ['RUMOUR'] }],
      prototypes: { RUMOUR: { kind: 'idea' } },
    }, dict)).toThrow(/recipe wait\.default: DRAWN is engine-written/)
    // and a DRAWN hidden in an untaken IF branch is still a load error
    expect(() => compileContent({
      verbs: { WAIT: { slots: [{ role: 'actors', min: 1, max: 1 }], duration: 0, recipes: [{ out: ['IF exists(NOSUCH) -> (DRAWN(RUMOURS, RUMOUR))'] }] } },
      decks: [{ id: 'RUMOURS', spec: ['RUMOUR'] }],
      prototypes: { RUMOUR: { kind: 'idea' } },
    }, dict)).toThrow(/DRAWN is engine-written/)
  })

  test('DRAW without a deck id, with parens, or naming an unknown deck is a load error', () => {
    const base = (out: string[]): ContentRaw => ({
      verbs: { WAIT: { slots: [{ role: 'actors', min: 1, max: 1 }], duration: 0, recipes: [{ out }] } },
      prototypes: { RUMOUR: { kind: 'idea' } },
      decks: [{ id: 'RUMOURS', spec: ['RUMOUR'] }],
    })
    expect(() => compileContent(base(['DRAW']), dict)).toThrow(/DRAW needs a deck id/)
    expect(() => parseOutcomes('DRAW(RUMOURS)', dict)).toThrow(/paren-less form/)
    expect(() => compileContent(base(['DRAW NOSUCH']), dict))
      .toThrow(/recipe wait\.default: DRAW 'NOSUCH' is not a declared deck/)
    expect(() => compileContent(base(['IF exists(HOUSE) -> (DRAW NOSUCH)']), dict))
      .toThrow(/DRAW 'NOSUCH' is not a declared deck/) // recurses into branches like checkEndingRefs
    expect(() => compileContent({
      verbs: {}, triggers: [{ id: 't', slot: 1, out: ['DRAW NOSUCH'] }],
    }, dict)).toThrow(/trigger t: DRAW 'NOSUCH' is not a declared deck/)
  })

  test('DRAW and DRAWN join the reserved words (§5.1); a function cannot take their names', () => {
    expect(dict.reserved.has('DRAW')).toBe(true)
    expect(dict.reserved.has('DRAWN')).toBe(true)
    expect(() => parseSentence('0:0 @ ANA WAIT DRAW', dict)).toThrow(/reserved/) // quote it instead
    expect(parseOutcomes('SPAWN "DRAW"', dict)).toHaveLength(1) // the quote is the escape hatch
    expect(() => compileContent({
      functions: [{ name: 'DRAW', params: [], body: 'NARRATE "x"' }],
      verbs: {},
    }, dict)).toThrow(/collides with an outcome op/)
    expect(() => compileContent({
      functions: [{ name: 'DRAWN', params: [], body: 'NARRATE "x"' }],
      verbs: {},
    }, dict)).toThrow(/collides with an outcome op/)
  })
})

// ---------- exhaustion: reset wraps, defaultcard forever, neither rejects ----------

describe('exhaustion (§9.42): reset wraps, defaultcard forever, neither rejects', () => {
  test('reset wraps: the sequence is periodic with period len(spec)', () => {
    const c = compile(rig({ decks: PLAN_DECK }))
    const { names } = playDraws(c.content, c.state, 5)
    expect(names).toEqual(['RUMOUR', 'HEARSAY', 'RUMOUR', 'SCANDAL', 'RUMOUR']) // draw 5 == draw 1
  })

  test('defaultcard forever: past the spec every draw serves the default', () => {
    const c = compile(rig({ decks: 'GOSSIP spec RUMOUR, HEARSAY, SCANDAL defaultcard HEARSAY', drawDeck: 'GOSSIP' }))
    const { names } = playDraws(c.content, c.state, 5)
    expect(names).toEqual(['RUMOUR', 'HEARSAY', 'SCANDAL', 'HEARSAY', 'HEARSAY'])
  })

  test('neither reset nor defaultcard: the draw fails, nothing logged, cursor unmoved', () => {
    const c = compile(rig({ decks: 'SINGLES spec HEARSAY, SCANDAL', drawDeck: 'SINGLES' }))
    let state = c.state
    let id = 1
    for (let i = 0; i < 2; i++) {
      const ok = run(c.content, state, '0:0 @ ANA WAIT', id)
      expect(ok.rejected).toBeNull()
      id += ok.records.length
      state = ok.state
    }
    expect(state.decks.get('SINGLES')).toBe(2)
    const before = snap(state)
    const r = run(c.content, state, '0:0 @ ANA WAIT', id)
    expect(r.rejected).toMatch(/deck 'SINGLES' is exhausted — no 'reset' and no 'defaultcard'/)
    expect(r.records).toHaveLength(0)
    expect(state.decks.get('SINGLES')).toBe(2) // cursor unmoved (§9.19)
    expect(snap(state)).toBe(before)
    expect(fold([], cloneState(state), dict, c.content).decks.get('SINGLES')).toBe(2)
  })
})

// ---------- cursor reconstruction (law 1): the fold rebuilds decks ----------

describe('cursor reconstruction (law 1): fold === play, decks included', () => {
  test('play several draws -> JSONL -> fold(loadLog) === played state (snap incl. decks)', () => {
    const game = Game.fromScript(rig({ decks: PLAN_DECK }), dict)
    for (const _ of [0, 1, 2]) expect(game.handleLine('wait').kind).toBe('action')
    expect(game.state.decks.get('RUMOURS')).toBe(3)
    const dir = mkdtempSync(join(tmpdir(), 'chorus-decks-'))
    const path = join(dir, 'log.jsonl')
    try {
      saveLog(path, game.log.records)
      const replayed = fold(loadLog(path), cloneState(game.log.initialState), dict, game.content)
      expect(snap(replayed)).toBe(snap(game.state))
      expect([...replayed.decks.entries()]).toEqual([['RUMOURS', 3]])
    } finally {
      rmSync(dir, { recursive: true, force: true })
    }
  })

  test('a DRAW inside an UNTAKEN IF branch leaves the cursor untouched', () => {
    const c = compile(rig({
      decks: PLAN_DECK,
      verbs: `  WAIT:
    actors: 1 kind:character
    duration: 0m
    recipes:
      default -> IF exists(NOSUCH) -> (DRAW RUMOURS) ELSE -> (NARRATE "quiet")`,
    }))
    const state = c.state
    const before = snap(state)
    const r = run(c.content, state, '0:0 @ ANA WAIT')
    expect(r.rejected).toBeNull()
    expect(r.records[0]!.outcomes.map(o => o.op)).toEqual(['NARRATE']) // the taken branch only
    expect(state.decks.size).toBe(0)
    expect(snap(state)).toBe(before)
  })

  test('a failure AFTER a successful DRAW rejects the whole step — draw and cursor discarded (§9.19)', () => {
    // the cursor advance happened on the draft inside DRAW; the later
    // outcome's throw discards the DRAFT, so neither the mint nor the cursor
    // survives — play and fold agree on the empty decks map
    const c = compile(rig({
      decks: PLAN_DECK,
      verbs: `  WAIT:
    actors: 1 kind:character
    duration: 0m
    recipes:
      default -> DRAW RUMOURS, MOVE ANA INTO NOSUCH`,
    }))
    const state = c.state
    const before = snap(state)
    const r = run(c.content, state, '0:0 @ ANA WAIT')
    expect(r.rejected).toMatch(/MOVE: unresolvable destination/)
    expect(r.records).toHaveLength(0)
    expect(state.decks.size).toBe(0)
    expect(snap(state)).toBe(before)
    expect(fold([], cloneState(state), dict, c.content).decks.size).toBe(0)
  })

  test('undo past a draw rebuilds the cursor from the fold', () => {
    const game = Game.fromScript(rig({ decks: PLAN_DECK }), dict)
    game.handleLine('wait')
    game.handleLine('wait')
    expect(game.state.decks.get('RUMOURS')).toBe(2)
    const beforeUndo = game.log.records[1]!.sentence // draw #2 (seed 0: HEARSAY)
    game.undo()
    expect(game.state.decks.get('RUMOURS')).toBe(1) // folded, not remembered
    expect(game.journal().join('\n').match(/DRAWN/g)).toHaveLength(1)
    // drawing again replays the SAME card: the cursor was folded back to 1,
    // so draw #2 serves the identical permutation entry, byte for byte
    game.handleLine('wait')
    expect(game.state.decks.get('RUMOURS')).toBe(2)
    expect(game.log.records[1]!.sentence).toBe(beforeUndo)
  })

  test('two identical compiles agree play-to-fold even mid-deck (control for the drift tests)', () => {
    const a = compile(rig({ seed: 7, decks: PLAN_DECK }))
    const b = compile(rig({ seed: 7, decks: PLAN_DECK }))
    const { records, state } = playDraws(a.content, a.state, 3)
    expect(snap(fold(records, cloneState(a.state), dict, a.content))).toBe(snap(state))
    // the same log under a second compile of the SAME content: replays identically
    expect(snap(fold(records, cloneState(b.state), dict, b.content))).toBe(snap(state))
  })

  test('JSONL fold equality with the deck WRAPPED and with defaultcard draws PAST the period', () => {
    // wrapped: the PLAN deck resets — 9 draws land the cursor at 9 (two full
    // periods of 4 plus one), so the fold must rebuild a past-the-period cursor
    const wrapped = Game.fromScript(rig({ decks: PLAN_DECK }), dict)
    for (let i = 0; i < 9; i++) expect(wrapped.handleLine('wait').kind).toBe('action')
    expect(wrapped.state.decks.get('RUMOURS')).toBe(9)
    // exhausted+defaultcard: a period-3 deck with no reset — 5 draws land 2
    // past the end, both served by the defaultcard
    const defaulted = Game.fromScript(rig({ decks: 'GOSSIP spec RUMOUR, HEARSAY, SCANDAL defaultcard HEARSAY', drawDeck: 'GOSSIP' }), dict)
    for (let i = 0; i < 5; i++) expect(defaulted.handleLine('wait').kind).toBe('action')
    expect(defaulted.state.decks.get('GOSSIP')).toBe(5)
    for (const game of [wrapped, defaulted]) {
      const dir = mkdtempSync(join(tmpdir(), 'chorus-decks-'))
      const path = join(dir, 'log.jsonl')
      try {
        saveLog(path, game.log.records)
        const replayed = fold(loadLog(path), cloneState(game.log.initialState), dict, game.content)
        expect(snap(replayed)).toBe(snap(game.state))
      } finally {
        rmSync(dir, { recursive: true, force: true })
      }
    }
  })
})

// ---------- decks × pools (§9.38 x §9.42) ----------

describe('decks compose with stackable pools (§9.38); the livelock rule (§9.42)', () => {
  test('stackable deck entries pool: two RUMOUR draws -> a pool of two living copies', () => {
    const c = compile(rig({ decks: PLAN_DECK }))
    const { state } = playDraws(c.content, c.state, 3) // seed 0: RUMOUR, HEARSAY, RUMOUR
    expect(state.byName.get('RUMOUR')).toHaveLength(2)
    for (const id of state.byName.get('RUMOUR')!) {
      expect(state.entities.get(id)!.deadAction).toBeNull()
      expect(state.entities.get(id)!.stackable).toBe(true)
    }
    expect(state.byName.get('HEARSAY')).toHaveLength(1)
  })

  test('non-stackable livelock: a redraw of a living card rejects identically forever (documented authoring rule)', () => {
    // the authoring rule (§9.42): deck entries should be STACKABLE, or be
    // consumed/renamed on the same step — a non-stackable entry whose previous
    // copy still lives makes every subsequent serve of that entry fail
    // identically (cursor never advances on rejection), pinning the deck shut
    const c = compile(rig({
      prototypes: `  WHISPER is idea`,
      decks: 'PAIR spec WHISPER, WHISPER reset',
      drawDeck: 'PAIR',
    }))
    let state = c.state
    const first = run(c.content, state, '0:0 @ ANA WAIT')
    expect(first.rejected).toBeNull()
    state = first.state
    expect(state.decks.get('PAIR')).toBe(1)
    const before = snap(state)
    for (let i = 0; i < 3; i++) {
      const r = run(c.content, state, '0:0 @ ANA WAIT', 2)
      expect(r.rejected).toBe('SPAWN: name \'WHISPER\' already in use') // the documented stuck-state message
      expect(r.records).toHaveLength(0)
      expect(state.decks.get('PAIR')).toBe(1) // stuck forever
      expect(snap(state)).toBe(before)
    }
  })

  test('the documented escape works: consuming the drawn copy on the same step lets every later serve through', () => {
    // the livelock rule's other half — a NON-stackable entry is fine when the
    // same step destroys/renames the previous copy; draw #2 must succeed on a
    // freed name and the deck must keep cycling through draws 3 and 4
    const c = compile(rig({
      prototypes: `  WHISPER is idea`,
      decks: 'PAIR spec WHISPER, WHISPER reset',
      drawDeck: 'PAIR',
      verbs: `  WAIT:
    actors: 1 kind:character
    duration: 0m
    recipes:
      default -> DRAW PAIR, DESTROY WHISPER`,
    }))
    let state = c.state
    let id = 1
    const records: ActionRecord[] = []
    for (let i = 0; i < 4; i++) {
      const r = run(c.content, state, '0:0 @ ANA WAIT', id)
      expect(r.rejected).toBeNull() // no livelock: the name frees every period
      records.push(...r.records)
      id += r.records.length
      state = r.state
    }
    expect(state.decks.get('PAIR')).toBe(4) // the cursor advanced every draw
    expect([...state.entities.values()].filter(e => e.name === 'WHISPER' && e.deadAction === null)).toHaveLength(0)
    expect(snap(fold(records, cloneState(c.state), dict, c.content))).toBe(snap(state))
  })
})

// ---------- load-time deck validation (§10) ----------

describe('load-time deck validation (§10, §9.42)', () => {
  test('an unknown spec entry is a load error naming the deck and the entry', () => {
    expect(() => compile(rig({ decks: 'RUMOURS spec RUMOUR, NOSUCH' })))
      .toThrow(/deck RUMOURS: spec entry 'NOSUCH' is neither a declared prototype nor an entity/)
    // declared entity lines count as resolutions too (compileScript passes them)
    expect(() => compile(rig({ entities: '  OLD_MAP is item at HOUSE', decks: 'RELICS spec OLD_MAP', drawDeck: 'RELICS' }))).not.toThrow()
  })

  test('an unknown defaultcard is a load error naming the deck and the entry', () => {
    expect(() => compile(rig({ decks: 'RUMOURS spec RUMOUR defaultcard NOSUCH' })))
      .toThrow(/deck RUMOURS: defaultcard 'NOSUCH' is neither a declared prototype nor an entity/)
  })

  test('an empty spec is a load error (at parse and at compile)', () => {
    expect(() => compile(rig({ decks: 'EMPTY spec' }))).toThrow(/deck EMPTY: the spec is empty/)
    expect(() => compileContent({ verbs: {}, decks: [{ id: 'EMPTY', spec: [] }] }, dict))
      .toThrow(/deck EMPTY: the spec is empty/)
  })

  test('deck ids must be UPPER_SNAKE, unreserved, and unique', () => {
    expect(() => compile(rig({ decks: 'gossip spec RUMOUR' }))).toThrow(/deck 'gossip' must be UPPER_SNAKE/)
    expect(() => compile(rig({ decks: 'DRAW spec RUMOUR' }))).toThrow(/'DRAW' is reserved and cannot be a deck id/)
    expect(() => compile(rig({ decks: 'RUMOURS spec RUMOUR\nRUMOURS spec HEARSAY' })))
      .toThrow(/duplicate deck 'RUMOURS'/)
  })
})

// ---------- DRAW inside functions (§9.40 x §9.42) ----------

describe('DRAW inside function bodies (§9.40 x §9.42)', () => {
  test('a param as the INTO target expands from a recipe call', () => {
    const c = compile(rig({
      decks: PLAN_DECK,
      functions: 'drop(target) -> DRAW RUMOURS INTO target',
      verbs: `  WAIT:
    actors: 1 kind:character
    duration: 0m
    recipes:
      default -> drop(KITCHEN)`,
    }))
    const r = run(c.content, c.state, '0:0 @ ANA WAIT')
    expect(r.rejected).toBeNull()
    const rumour = [...r.state.entities.values()].find(e => e.name === 'RUMOUR')!
    expect(rumour.attrs['container']).toBe('s_kitchen')
    expect(r.records[0]!.outcomes.map(o => o.op)).toEqual(['DRAWN', 'SPAWN'])
  })

  test('the same function from a trigger (explicit INTO) lands and replays', () => {
    const c = compile(rig({
      decks: PLAN_DECK,
      functions: 'drop(target) -> DRAW RUMOURS INTO target',
      triggers: 'rumour_fn at hour 1 -> drop(HOUSE)',
      verbs: `  WAIT:
    actors: 1 kind:character
    duration: 1h
    recipes:
      default -> NARRATE "{actors} wait."`,
    }))
    const r = run(c.content, c.state, '0:0 @ ANA WAIT')
    expect(r.rejected).toBeNull()
    const rumour = [...r.state.entities.values()].find(e => e.name === 'RUMOUR')!
    expect(rumour.attrs['container']).toBe('s_house')
    expect(snap(fold(r.records, cloneState(c.state), dict, c.content))).toBe(snap(r.state))
  })

  test('a param as the DECK id expands and passes the deck check at the call site', () => {
    const c = compile(rig({
      decks: `${PLAN_DECK}\n  QUIET spec HEARSAY`,
      functions: 'pull(deck) -> DRAW deck',
      verbs: `  WAIT:
    actors: 1 kind:character
    duration: 0m
    recipes:
      default -> pull(QUIET)`,
    }))
    const r = run(c.content, c.state, '0:0 @ ANA WAIT')
    expect(r.rejected).toBeNull()
    expect(drawnNames(r)).toEqual(['HEARSAY'])
    expect(r.state.decks.get('QUIET')).toBe(1)
  })

  test('a role INTO expanded into a TRIGGER rejects at runtime (event bindings are empty)', () => {
    // guards stay load-time for the DECK id only; the INTO role resolves at
    // runtime like every ref position — and in a trigger nothing binds, so
    // the draw refuses to materialize nowhere (§9.42's stricter INTO)
    const c = compile(rig({
      decks: PLAN_DECK,
      functions: 'drop(target) -> DRAW RUMOURS INTO target',
      triggers: 'rumour_fn at hour 1 -> drop(OBJECT)',
      verbs: `  WAIT:
    actors: 1 kind:character
    duration: 1h
    recipes:
      default -> NARRATE "{actors} wait."`,
    }))
    const state = c.state
    const before = snap(state)
    const r = run(c.content, state, '0:0 @ ANA WAIT')
    expect(r.rejected).toMatch(/INTO target of 'RUMOURS' cannot resolve/)
    expect(r.records).toHaveLength(0) // §9.19: the whole step rejects
    expect(snap(state)).toBe(before)
  })

  test('a function body DRAWing with PROGRESS after it still trips the PROGRESS guard (expansion before guards)', () => {
    // the trigger PROGRESS/REFUSE guards walk the EXPANDED list — a DRAW in
    // the body cannot hide a later PROGRESS from them
    expect(() => compile(rig({
      decks: PLAN_DECK,
      functions: 'sneaky() -> DRAW RUMOURS, PROGRESS(30m, "tick")',
      triggers: 'sneaky_hour at hour 1 -> sneaky()',
      verbs: `  WAIT:
    actors: 1 kind:character
    duration: 1h
    recipes:
      default -> NARRATE "{actors} wait."`,
    }))).toThrow(/trigger sneaky_hour: PROGRESS belongs to verb recipes/)
  })
})

// ---------- content drift between play and fold fails loud (§9.42) ----------

describe('content drift (§9.42): the DRAWN assertion fires instead of silent divergence', () => {
  const driftRaw = (seed: number, spec: string[]): ContentRaw => ({
    seed,
    verbs: { WAIT: { slots: [{ role: 'actors', min: 1, max: 1, query: 'kind:character' }], duration: 0, recipes: [{ out: ['DRAW RUMOURS'] }] } },
    prototypes: {
      RUMOUR: { kind: 'idea', stackable: true },
      HEARSAY: { kind: 'idea', stackable: true },
      SCANDAL: { kind: 'idea', stackable: true },
    },
    decks: [{ id: 'RUMOURS', spec, defaultcard: 'RUMOUR', reset: true }],
  })
  const driftWorld = (): State => seedState() // ANA carries container 'house' from the seed world

  test('a seed change under an old log: the assertion names the deck, cursor, both names', () => {
    // §9.30 asymmetry, decks edition: a seed change shifts every FUTURE draw —
    // and where the dice replay past rows from the log alone, a deck's cursor
    // row must re-derive its expectation, so the drifted PAST fails loud
    const played = compileContent(driftRaw(0, ['RUMOUR', 'RUMOUR', 'HEARSAY', 'SCANDAL']), dict)
    const drifted = compileContent(driftRaw(42, ['RUMOUR', 'RUMOUR', 'HEARSAY', 'SCANDAL']), dict)
    expect(played.decks.get('RUMOURS')!.order.map(e => e.name)).not.toEqual(drifted.decks.get('RUMOURS')!.order.map(e => e.name))
    const r = run(played, driftWorld(), '0:0 @ ANA WAIT')
    expect(r.rejected).toBeNull()
    expect(() => fold(r.records, driftWorld(), dict, drifted))
      .toThrow(/DRAWN: deck 'RUMOURS' draw #0 recorded 'RUMOUR' but the compiled deck serves/)
  })

  test('a changed spec under the same seed fires too; a removed deck says so', () => {
    const played = compileContent(driftRaw(0, ['RUMOUR', 'RUMOUR', 'HEARSAY', 'SCANDAL']), dict)
    const respecced = compileContent(driftRaw(0, ['SCANDAL', 'HEARSAY', 'RUMOUR', 'RUMOUR']), dict)
    const r = run(played, driftWorld(), '0:0 @ ANA WAIT')
    expect(() => fold(r.records, driftWorld(), dict, respecced)).toThrow(/content drift between play and fold/)
    // a deck that vanished from the compiled content: unreachable through
    // compileContent (the DRAW check pins the id at load), so simulate the
    // drift on the compiled table
    const deckless = compileContent(driftRaw(0, ['RUMOUR', 'RUMOUR', 'HEARSAY', 'SCANDAL']), dict)
    deckless.decks.delete('RUMOURS')
    expect(() => fold(r.records, driftWorld(), dict, deckless)).toThrow(/deck 'RUMOURS' is not in the compiled decks/)
  })

  test('drift in the DEFAULTCARD alone fires too — past the period the assertion still re-derives', () => {
    // a spec of one, no reset: draw #0 is the spec entry, draws #1.. carry the
    // defaultcard — changing ONLY the defaultcard must trip draw #1's assertion
    const raw = (defaultcard: string): ContentRaw => ({
      seed: 0,
      verbs: { WAIT: { slots: [{ role: 'actors', min: 1, max: 1, query: 'kind:character' }], duration: 0, recipes: [{ out: ['DRAW SMALL'] }] } },
      prototypes: {
        RUMOUR: { kind: 'idea', stackable: true },
        HEARSAY: { kind: 'idea', stackable: true },
        SCANDAL: { kind: 'idea', stackable: true },
      },
      decks: [{ id: 'SMALL', spec: ['RUMOUR'], defaultcard }],
    })
    const played = compileContent(raw('RUMOUR'), dict)
    const drifted = compileContent(raw('SCANDAL'), dict)
    let records: ActionRecord[] = []
    let state = driftWorld()
    let id = 1
    for (let i = 0; i < 3; i++) {
      const r = run(played, state, '0:0 @ ANA WAIT', id)
      expect(r.rejected).toBeNull()
      records = [...records, ...r.records]
      id += r.records.length
      state = r.state
    }
    // the identical compile replays clean (control — a non-drifted fold never trips)
    expect(() => fold(records, driftWorld(), dict, compileContent(raw('RUMOUR'), dict))).not.toThrow()
    // only the defaultcard changed: draw #1 (past the period) is the first to fire
    expect(() => fold(records, driftWorld(), dict, drifted))
      .toThrow(/DRAWN: deck 'SMALL' draw #1 recorded 'RUMOUR' but the compiled deck serves 'SCANDAL'/)
  })

  test('REFUSE after a draw: the draft dies, the cursor never moves, the fold skips the rows', () => {
    const c = compile(rig({
      decks: PLAN_DECK,
      verbs: `  WAIT:
    actors: 1 kind:character
    duration: 0m
    recipes:
      default -> DRAW RUMOURS, REFUSE("not this one")`,
    }))
    const state = c.state
    const before = snap(state)
    const r = run(c.content, state, '0:0 @ ANA WAIT')
    expect(r.rejected).toBeNull()
    expect(r.records[0]!.outcomes.map(o => o.op)).toEqual(['DRAWN', 'SPAWN', 'REFUSE'])
    expect(r.records[0]!.advancedMin).toBe(0)
    expect(r.state).toBe(state) // draft discarded
    expect(state.decks.get('RUMOURS')).toBeUndefined()
    expect(snap(state)).toBe(before)
    // the fold treats the refused record as a no-op — the cursor never moves there either
    const replayed = fold(r.records, cloneState(state), dict, c.content)
    expect(replayed.decks.get('RUMOURS')).toBeUndefined()
    expect(snap(replayed)).toBe(before)
  })
})

// ---------- hygiene ----------

describe('hygiene (§9.42)', () => {
  test('grep gate: no Math.random anywhere in src — decks draw from splitmix64 keys, not from chance', () => {
    const srcDir = new URL('../src', import.meta.url).pathname
    const offenders: string[] = []
    for (const f of readdirSync(srcDir)) {
      if (!f.endsWith('.ts') && !f.endsWith('.tsx')) continue
      if (readFileSync(join(srcDir, f), 'utf8').includes('Math.random')) offenders.push(f)
    }
    expect(offenders).toEqual([])
  })

  test('cloneState copies the deck cursors — a discarded draft moves nothing (law 6)', () => {
    const game = Game.fromScript(rig({ decks: PLAN_DECK }), dict)
    game.handleLine('wait')
    const real = game.state.decks.get('RUMOURS')!
    const draft = cloneState(game.state)
    draft.decks.set('RUMOURS', 99)
    expect(game.state.decks.get('RUMOURS')).toBe(real)
  })
})
