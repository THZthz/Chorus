// Exploratory story test — plays Murder on the Nightjar Flyer the way no
// human would. Three passes, all deterministic:
//
//   A. sweep      — at every state of the win path, execute EVERY verb
//                   × reachable-card combination (plus deliberately
//                   malformed fills) and check the engine invariants on
//                   each outcome: accepted / refused / rejected all behave
//                   per spec (§6.5, §6.6, §6.7).
//   B. walk       — a seeded random walk of hundreds of steps (with
//                   periodic undo), asserting after every step that the log
//                   alone reproduces the state (state = fold(log), §7),
//                   sentences round-trip losslessly, and the world keeps
//                   its shape (unique living names, resolvable containers,
//                   living relations). Ends with a JSONL round-trip.
//   C. surface    — the typed front door: slash commands, garbage, and the
//                   player-typed EVENT guard.
//
// Anything here failing means an engine bug, not a content bug: the checks
// are invariants, not plot.
import { describe, expect, test } from 'vitest'
import { readFileSync, mkdtempSync, rmSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { compileScript } from '../src/script.js'
import { executeStep, type ActionRecord, type ExecResult, type Step } from '../src/engine.js'
import { fold } from '../src/fold.js'
import { absMin, clockFromAbsMin, formatClock, nextStepClock } from '../src/clock.js'
import { parseSentence } from '../src/parse.js'
import { formatSentence } from '../src/format.js'
import { matchQuery } from '../src/query.js'
import { parsePlayerInput } from '../src/lenient.js'
import { proseLines } from '../src/render.js'
import { saveLog, loadLog } from '../src/store.js'
import { seedDictionary } from '../src/seed.js'
import { sortRefs, nameRef, type Ref } from '../src/ref.js'
import { cloneState, type State } from '../src/types.js'
import { emptyClause, ROLES, type Clause, type Role } from '../src/sentence.js'
import type { CompiledContent, CompiledSlot } from '../src/content.js'
import type { Dictionary } from '../src/dict.js'
import { Game } from '../src/play.js'

const dict = seedDictionary()
const scriptText = readFileSync(new URL('../scripts/the-nightjar-flyer.txt', import.meta.url), 'utf8')
const { content, state: base, meta } = compileScript(scriptText, dict)
const PLAYER = meta.player ?? 'MARCH'

// ---------- shared invariants ----------

function snap(s: State): string {
  return JSON.stringify({
    ents: [...s.entities.entries()].sort(([a], [b]) => a < b ? -1 : 1),
    rels: [...s.relations.entries()].sort(([a], [b]) => a < b ? -1 : 1),
    byName: [...s.byName.entries()].sort(([a], [b]) => a < b ? -1 : 1),
    now: s.now,
    fired: [...s.fired].sort(),
    ended: s.ended, // §9.35: fold equality covers the story's ending too
  })
}

function checkStateShape(s: State): void {
  const seen = new Set<string>()
  for (const e of s.entities.values()) {
    if (e.deadAction !== null) continue
    expect(seen.has(e.name), `duplicate living name ${e.name}`).toBe(false)
    seen.add(e.name)
    expect(s.byName.get(e.name)).toBe(e.id)
    const container = e.attrs['container']
    if (typeof container === 'string') expect(s.entities.has(container), `${e.name} sits nowhere`).toBe(true)
  }
  for (const r of s.relations.values()) {
    if (r.deadAction !== null) continue
    expect(s.entities.has(r.a) && s.entities.has(r.b), `dangling relation ${r.type}`).toBe(true)
  }
}

function checkRecords(records: ActionRecord[]): void {
  for (const rec of records) {
    // losslessness: the canonical sentence re-parses and re-formats to itself
    const s = parseSentence(rec.sentence, dict)
    expect(formatSentence(s, dict)).toBe(rec.sentence)
    expect(() => proseLines(rec, content, dict)).not.toThrow()
    expect(rec.actors).toEqual(sortRefs(rec.actors))
    expect(rec.advancedMin).toBeGreaterThanOrEqual(0)
    if (rec.kind === 'event') {
      expect(rec.advancedMin).toBe(0)
      expect(rec.trigger).not.toBeNull()
      expect(rec.verb).toBe('')
    }
  }
}

/** full spec-conformance check of one executed step against its pre-state */
function checkStep(pre: State, r: ExecResult, step: Step): 'refused' | 'applied' {
  expect(r.rejected).toBeNull()
  expect(r.records.length).toBeGreaterThan(0)
  checkRecords(r.records)
  const first = r.records[0]
  expect(first.tGame).toBe(formatClock(step.clock))

  const refused = first.outcomes.some(o => o.op === 'REFUSE')
  if (refused) {
    // §6.5: the attempt is logged, the draft discarded, no time passes;
    // clauses after the refusal never fire, so recipes ≤ clauses
    expect(first.advancedMin).toBe(0)
    expect(first.recipes.length).toBeLessThanOrEqual(step.clauses.length)
    expect(r.state).toBe(pre)
    expect(snap(r.state)).toBe(snap(pre))
    return 'refused'
  }
  expect(first.recipes.length).toBe(step.clauses.length)
  // §6.6: time = sum of clause durations + authored ADVANCE extras
  const sumDur = step.clauses.reduce((a, c) => a + (content.verbs.get(c.verb)?.duration ?? 0), 0)
  const advance = first.outcomes.reduce((a, o) => {
    if (o.op !== 'ADVANCE') return a
    const d = o.args[0]
    return a + (d && d.t === 'dur' ? d.v : 0)
  }, 0)
  expect(first.advancedMin).toBe(sumDur + advance)
  expect(r.state.now).toBe(absMin(step.clock) + first.advancedMin)
  // state = fold(log): the records alone reproduce the played state (§7)
  expect(snap(fold(r.records, cloneState(pre), dict, content))).toBe(snap(r.state))
  checkStateShape(r.state)
  return 'applied'
}

// ---------- candidate generation ----------

interface Cand {
  tag: string
  step: Step
}

const roleRefs = (c: Clause, role: Role): Ref[] | Ref | null =>
  role === 'actors' ? c.actors
  : role === 'object' ? c.objects
  : role === 'with' ? c.with
  : role === 'at' ? c.at
  : role === 'about' ? c.about
  : c.to

const clauseKey = (c: Clause): string =>
  [c.verb, ...ROLES.map(r => JSON.stringify(roleRefs(c, r)))].join('|')

function candidatesFor(content: CompiledContent, state: State, player: string): Cand[] {
  const clock = nextStepClock(state.now) // the front door's rule (§9.28)
  const alive = [...state.entities.values()].filter(e => e.deadAction === null)
  const chars = alive.filter(e => e.kind === 'character')
  const pool = (slot: CompiledSlot): typeof alive =>
    slot.query ? alive.filter(e => matchQuery(e, slot.query!)) : alive

  const out = new Map<string, Cand>()
  const push = (tag: string, clauses: Clause[]): void => {
    const key = clauses.map(clauseKey).join(';')
    if (!out.has(key)) out.set(key, { tag, step: { clock, clauses } })
  }

  for (const verb of content.verbs.values()) {
    // options per declared role: empty (when allowed), singles, and shaped extras
    const fills = new Map<Role, Ref[][]>()
    for (const slot of verb.slots) {
      const opts: Ref[][] = []
      if (slot.min === 0) opts.push([])
      const candidates = slot.role === 'actors' && chars.length > 0 ? chars : pool(slot)
      for (const e of candidates.slice(0, 4)) opts.push([nameRef(e.name)])
      const actorsSlot = verb.slotByRole.get('actors')
      if (slot.role === 'actors' && actorsSlot && actorsSlot.max >= 2 && chars.length >= 2) {
        opts.push(chars.slice(0, 2).map(e => nameRef(e.name)))
      }
      if (slot.role === 'with' && candidates.length >= 2) {
        opts.push(candidates.slice(0, Math.min(3, candidates.length)).map(e => nameRef(e.name)))
      }
      fills.set(slot.role, opts)
    }
    // baseline valid fill: the first option covering each slot's minimum
    const minFill = new Map<Role, Ref[]>()
    let viable = true
    for (const role of ROLES) {
      const slot = verb.slotByRole.get(role)
      if (!slot) { minFill.set(role, []); continue }
      const cover = (fills.get(role) ?? []).filter(o => o.length >= slot.min)
      if (cover.length === 0) { viable = false; break }
      minFill.set(role, cover[0])
    }
    if (!viable) continue
    const mk = (over: Map<Role, Ref[]>, tag: string): void => {
      const c = emptyClause([])
      c.verb = verb.name
      c.actors = over.get('actors') ?? []
      c.objects = over.get('object') ?? []
      c.with = over.get('with') ?? []
      c.at = (over.get('at') ?? [])[0] ?? null
      c.about = (over.get('about') ?? [])[0] ?? null
      c.to = (over.get('to') ?? [])[0] ?? null
      push(tag, [c])
    }
    mk(minFill, `${verb.name} min-fill`)
    for (const role of ROLES) {
      const slot = verb.slotByRole.get(role)
      if (!slot) continue
      for (const opt of fills.get(role) ?? []) {
        const over = new Map(minFill)
        over.set(role, opt)
        mk(over, `${verb.name} ${role}=${opt.length}`)
      }
      // deliberate slot-mismatch probe: a card that must not fit
      if (slot.query) {
        const wrong = alive.find(e => !matchQuery(e, slot.query!))
        if (wrong) {
          const over = new Map(minFill)
          over.set(role, [nameRef(wrong.name)])
          mk(over, `${verb.name} ${role}=wrong-kind`)
        }
      }
    }
    // witness × topic matrix for verbs that interrogate (to × about)
    const toOpts = fills.get('to') ?? []
    const aboutOpts = fills.get('about') ?? []
    if (toOpts.length > 0 && aboutOpts.length > 0) {
      for (const t of toOpts.slice(0, 4)) {
        for (const a of aboutOpts.slice(0, 4)) {
          const over = new Map(minFill)
          over.set('to', t)
          over.set('about', a)
          mk(over, `${verb.name} to×about`)
        }
      }
    }
  }

  // multi-clause and clock-discipline probes
  push('multi-clause', parseSentence(`${formatClock(clock)} @ ${PLAYER} WAIT; ${PLAYER} WAIT`, dict).clauses)
  if (state.now >= 60) {
    const past = clockFromAbsMin(absMin(clock) - 60)
    push('clock rewind', parseSentence(`${formatClock(past)} @ ${PLAYER} WAIT`, dict).clauses)
  }
  return [...out.values()]
}

// ---------- the win path, as sweep states ----------

const WIN_PATH = [
  'wait',
  'explore the first cabin',
  'read the solicitor letter',
  'explore the firebox',
  'read the vane ledger',
  'talk with the spiritualist', // meet Seraphine — the name is learned by talking
  'seraphine read the wax cylinder',
  'seraphine read the telegram',
  'use on the inquest with the embezzlement the stolen voice the vienna warning',
]

function parsePlayerish(line: string, state: State): Step {
  const parsed = parsePlayerInput(line, { dict, content, state, player: PLAYER })
  if (!parsed.ok) throw new Error(`win path line not parseable: '${line}': ${parsed.error}`)
  return parsed.step
}

/** play the win path on primitives, snapshotting a fresh clone after each step */
function winPathStates(): { states: { label: string; state: State }[]; records: ActionRecord[] } {
  let state = cloneState(base)
  const states = [{ label: 'start', state: cloneState(state) }]
  const records: ActionRecord[] = []
  let id = 1
  for (const line of WIN_PATH) {
    const r = executeStep(content, dict, state, parsePlayerish(line, state), { firstRecordId: id })
    if (r.rejected) throw new Error(`win path broke at '${line}': ${r.rejected}`)
    records.push(...r.records)
    id += r.records.length
    state = r.state
    states.push({ label: `after ${line}`, state: cloneState(state) })
  }
  return { states, records }
}

// ---------- pass A: exhaustive sweep ----------

describe('nightjar explorer — sweep', () => {
  test('every verb × card combination at every win-path state keeps the invariants', () => {
    const { states } = winPathStates()
    let total = 0
    let applied = 0
    let refused = 0
    let rejected = 0
    const verbsSeen = new Set<string>()
    for (const { label, state } of states) {
      for (const cand of candidatesFor(content, state, PLAYER)) {
        total++
        const verbName = cand.tag.split(' ')[0]
        if (content.verbs.has(verbName)) verbsSeen.add(verbName)
        let r: ExecResult
        try {
          r = executeStep(content, dict, state, cand.step, { firstRecordId: 1 })
        } catch (err) {
          throw new Error(`engine threw at ${label} on [${cand.tag}]: ${(err as Error).message}`)
        }
        if (r.rejected !== null) {
          rejected++
          expect(r.records).toHaveLength(0)
          expect(r.state).toBe(state)
          continue
        }
        const outcome = checkStep(state, r, cand.step)
        if (outcome === 'refused') refused++
        else applied++
      }
      checkStateShape(state)
    }
    // coverage floors: the sweep must actually be sweeping
    expect(states.length).toBe(WIN_PATH.length + 1)
    expect(total).toBeGreaterThan(400)
    expect(applied).toBeGreaterThan(100)
    expect(refused).toBeGreaterThan(5)
    expect(rejected).toBeGreaterThan(50)
    expect(verbsSeen.size).toBe(content.verbs.size)
  })
})

// ---------- pass B: seeded random walk with undo ----------

describe('nightjar explorer — random walk', () => {
  test('hundreds of arbitrary steps (and undos) keep state = fold(log)', () => {
    // deterministic LCG so failures reproduce
    let seed = 0x4e494748 // 'NIGH'
    const rand = (): number => ((seed = (Math.imul(seed, 1664525) + 1013904223) >>> 0) / 0x100000000)

    let state = cloneState(base)
    const records: ActionRecord[] = []
    let id = 1
    let executed = 0
    let walkRejects = 0
    const snapshots = new Map<number, string>()

    for (let i = 0; i < 300; i++) {
      const cands = candidatesFor(content, state, PLAYER)
      // mostly walk the well-formed space; keep a 20% budget for error probes
      const benign = cands.filter(c => !c.tag.includes('wrong-kind') && c.tag !== 'clock rewind')
      const pickFrom = rand() < 0.2 || benign.length === 0 ? cands : benign
      const cand = pickFrom[Math.floor(rand() * pickFrom.length) % pickFrom.length]
      const r = executeStep(content, dict, state, cand.step, { firstRecordId: id })
      if (r.rejected !== null) { walkRejects++; continue }
      executed++
      records.push(...r.records)
      id += r.records.length
      state = r.state
      snapshots.set(records.length, snap(state))

      // the log alone must always reproduce the state (full re-fold, every 10th step)
      if (i % 10 === 9) {
        expect(snap(fold(records, cloneState(base), dict, content))).toBe(snap(state))
      }
      // §7 undo: drop trailing events + the last action, re-fold, compare
      if (i % 37 === 36 && records.length > 1) {
        while (records.length > 0 && records[records.length - 1].kind === 'event') records.pop()
        records.pop()
        state = fold(records, cloneState(base), dict, content)
        const expected = snapshots.get(records.length)
        expect(expected, `undo to length ${records.length} left no snapshot`).toBeDefined()
        expect(snap(state)).toBe(expected)
      }
    }
    expect(executed).toBeGreaterThan(150)
    expect(walkRejects).toBeGreaterThan(0)

    // the whole walked log survives a JSONL round-trip (§7)
    const dir = mkdtempSync(join(tmpdir(), 'chorus-explorer-'))
    try {
      const path = join(dir, 'log.jsonl')
      saveLog(path, records)
      expect(snap(fold(loadLog(path), cloneState(base), dict, content))).toBe(snap(state))
    } finally {
      rmSync(dir, { recursive: true, force: true })
    }
  })
})

// ---------- pass C: the typed surface ----------

describe('nightjar explorer — typed surface', () => {
  test('slash commands, garbage, and the event guard never throw', () => {
    const game = Game.fromScript(scriptText, dict)
    game.handleLine('wait')

    const cases = [
      '', '   ', 'xyzzy', 'read', 'read the', 'talk to', 'the the the',
      'wait at nowhere', 'talk to seraphine about the lost chord',
      'use on the inquest with the aether seal the aether seal', // duplicate card
      '0:0 @ MARCH WAIT',                                        // rewinds the clock
      '0:0 @ EVENT body_found',                                  // events are not typed (§5.1)
      'MARCH WAIT',                                              // canonical passthrough, valid
    ]
    for (const line of cases) {
      let kind: string | undefined
      expect(() => { kind = game.handleLine(line).kind }).not.toThrow()
      expect(['info', 'error', 'action']).toContain(kind)
    }
    expect(game.handleLine('0:0 @ EVENT body_found').kind).toBe('error')

    expect(game.look().length).toBeGreaterThan(0)
    expect(game.cards().length).toBeGreaterThan(0)
    expect(game.journal().length).toBeGreaterThan(0)
    expect(game.story().length).toBeGreaterThan(0)
    expect(game.handleLine('bogus').kind).toBe('error')
    expect(game.undo().kind).toBe('info')
  })

  test('the guard: a player-typed EVENT sentence is rejected, nothing logged', () => {
    const game = Game.fromScript(scriptText, dict)
    const before = game.log.records.length
    const turn = game.handleLine('0:0 @ EVENT body_found -> SPAWN DREAD')
    expect(turn.kind).toBe('error')
    expect(game.log.records.length).toBe(before)
  })
})
