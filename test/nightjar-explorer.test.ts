// Exploratory story test — plays Murder on the Nightjar Flyer the way no
// human would. Three passes, all deterministic:
//
//   A. sweep      — at every state of the win path (plus states harvested
//                   from a deterministic sampled walk: pools grown, decks
//                   drawn, events crossed), execute the EXHAUSTIVE verb ×
//                   role-fill space — every k-subset within each slot's
//                   declared cardinality, every mismatching card, cardinality
//                   and distinct-card boundary probes, small verbs' full
//                   cartesian products, multi-clause pairs, and adaptive
//                   refuser pairings — and check the engine invariants on
//                   each outcome: accepted / refused / rejected all behave
//                   per spec (§6.5, §6.6, §6.7).
//   B. walk       — a seeded random walk of hundreds of steps (with
//                   periodic undo), asserting after every step that the log
//                   alone reproduces the state (state = fold(log), §7),
//                   sentences round-trip losslessly, and the world keeps
//                   its shape (duplicate living names only where the content
//                   declares them stackable, resolvable containers, living
//                   relations; ending-aware — the patrol's escalation is
//                   undone through when it fires). Ends with a JSONL
//                   round-trip.
//   C. surface    — the typed front door: slash commands, garbage, and the
//                   player-typed EVENT guard.
//
// Anything here failing means an engine bug, not a content bug: the checks
// are invariants, not plot.
import { describe, expect, test } from 'vitest'
import { mkdtempSync, rmSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { executeStep, type ActionRecord, type ExecResult, type Step } from '../src/engine.js'
import { fold } from '../src/fold.js'
import { absMin, formatClock, nextStepClock } from '../src/clock.js'
import { parseSentence } from '../src/parse.js'
import { formatSentence } from '../src/format.js'
import { proseLines } from '../src/render.js'
import { saveLog, loadLog } from '../src/store.js'
import { sortRefs } from '../src/ref.js'
import { cloneState, type State } from '../src/types.js'
import type { Clause } from '../src/sentence.js'
import type { CompiledContent } from '../src/content.js'
import { Game } from '../src/play.js'
import { snap } from './snap.js'
import { settleStep } from './settle.js'
import {
  candidatesFor, dict, scriptText, content, base, PLAYER,
  winPathStates, sampledWalkStates, WIN_PATH,
} from './explorer-lib.js'

// ---------- shared invariants ----------

function checkStateShape(s: State, content: CompiledContent): void {
  const seen = new Set<string>()
  for (const e of s.entities.values()) {
    if (e.deadAction !== null) continue
    // §9.38: a name the content declares stackable is a pool — duplicate
    // living copies (SUSPICION, the RUMOURS deck's gossip) are legal; the
    // card's own flag must agree with the declaration, or the mint lied
    const declared = content.stackableNames.has(e.name) || content.stackableKinds.has(e.kind)
    expect(e.stackable, `${e.name}: stackable flag disagrees with the declaration`).toBe(declared)
    if (!declared) {
      expect(seen.has(e.name), `duplicate living name ${e.name}`).toBe(false)
    }
    seen.add(e.name)
    expect(s.byName.get(e.name)).toContain(e.id) // §9.38: the id is in the name's pool
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
  const last = r.records[r.records.length - 1]!

  // §6.5 v0.5: a refused BEGIN logs the attempt alone and discards the whole
  // turn — its advance phase (if any) goes with it, nothing moves
  if (last.kind === 'action' && last.outcomes.some(o => o.op === 'REFUSE')) {
    expect(r.records.length).toBe(1)
    expect(last.advancedMin).toBe(0)
    expect(last.recipes.length).toBeLessThanOrEqual(step.clauses.length)
    expect(r.state).toBe(pre)
    expect(snap(r.state)).toBe(snap(pre))
    return 'refused'
  }

  // a turn ends at its boundary: action turns end with the BEGIN row,
  // pure advances with the wait row (v0.5)
  if (step.clauses.length > 0) expect(last.kind).toBe('action')

  // v0.5 clock rows: at most one wait row per turn (one advance pass), its
  // span partitions time, and only it moves `now`
  const waits = r.records.filter(x => x.kind === 'wait')
  expect(waits.length).toBeLessThanOrEqual(1)
  if (waits.length === 1) {
    expect(r.state.now).toBe((waits[0]!.startMin ?? 0) + waits[0]!.advancedMin)
  } else {
    expect(r.state.now).toBe(pre.now) // beginning at now moves nothing
  }

  // BEGIN semantics: recipes = clauses; advancedMin = the window length —
  // §6.6 v0.5 (authored ADVANCE defers to the completion, where it extends
  // the resolving pass's wait span instead of the BEGIN row)
  if (last.kind === 'action') {
    expect(last.recipes.length).toBe(step.clauses.length)
    const sumDur = step.clauses.reduce((a, c) => a + (content.verbs.get(c.verb)?.duration ?? 0), 0)
    expect(last.advancedMin).toBe(sumDur)
  }
  // state = fold(log): the records alone reproduce the played state (§7) —
  // open windows included
  expect(snap(fold(r.records, cloneState(pre), dict, content))).toBe(snap(r.state))
  checkStateShape(r.state, content)
  return 'applied'
}

// ---------- pass A: exhaustive sweep ----------

describe('nightjar explorer — sweep', () => {
  test('the exhaustive verb × fill space at every sweep state keeps the invariants', () => {
    const states = [...winPathStates().states, ...sampledWalkStates()]
    let total = 0
    let applied = 0
    let refused = 0
    let rejected = 0
    let atomicityProbes = 0
    const verbsSeen = new Set<string>()
    for (const { label, state } of states) {
      // phase 1: everything the generator can name
      const refusers = new Map<string, Clause>()
      const appliers = new Map<string, Clause>()
      for (const cand of candidatesFor(content, state, PLAYER, 'full')) {
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
        if (outcome === 'refused') {
          refused++
          // remember one refuser and one applier per verb for phase 2
          if (cand.step.clauses.length === 1) {
            const c = cand.step.clauses[0]
            if (c && !refusers.has(c.verb)) refusers.set(c.verb, c)
          }
        } else {
          applied++
          if (cand.step.clauses.length === 1) {
            const c = cand.step.clauses[0]
            if (c && !appliers.has(c.verb)) appliers.set(c.verb, c)
          }
        }
      }
      // phase 2: step-atomicity probes (§6.5, §6.6) — pair each refusing
      // clause with WAIT and with a clause that applies, both orders; the
      // REFUSE must collapse the WHOLE step: no time, no effects from the
      // clause that would have succeeded
      const waitClause = parseSentence(`${formatClock(nextStepClock(state.now))} @ ${PLAYER} WAIT`, dict).clauses[0]
      if (!waitClause) continue
      const anyApplier = appliers.values().next().value
      for (const refuser of [...refusers.values()].slice(0, 4)) {
        const partners = [waitClause, appliers.get(refuser.verb) ?? anyApplier]
        for (const partner of partners) {
          if (!partner) continue
          for (const clauses of [[refuser, partner], [partner, refuser]]) {
            total++
            atomicityProbes++
            const step: Step = { clock: nextStepClock(state.now), clauses }
            const r = executeStep(content, dict, state, step, { firstRecordId: 1 })
            expect(r.rejected, `${label}: atomicity probe rejected: ${r.rejected}`).toBeNull()
            const outcome = checkStep(state, r, step)
            if (outcome === 'refused') refused++
            else applied++
          }
        }
      }
      checkStateShape(state, content)
    }
    // coverage floors: the sweep must actually be sweeping
    expect(states.length).toBeGreaterThanOrEqual(WIN_PATH.length + 1 + 4)
    expect(total).toBeGreaterThan(30000)
    expect(applied).toBeGreaterThan(12000)
    expect(refused).toBeGreaterThan(2000)
    expect(rejected).toBeGreaterThan(8000)
    expect(atomicityProbes).toBeGreaterThan(100)
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
    const turnLengths: number[] = [0] // undo truncates to the previous turn's start (v0.5)
    let id = 1
    let executed = 0
    let walkRejects = 0
    let endings = 0
    const snapshots = new Map<number, string>()
    snapshots.set(0, snap(state)) // the walk may need to undo back to the start

    for (let i = 0; i < 800; i++) {
      const cands = candidatesFor(content, state, PLAYER)
      // mostly walk the well-formed space; keep a 20% budget for error probes
      const benign = cands.filter(c => !c.tag.includes('wrong-kind') && c.tag !== 'clock rewind')
      const pickFrom = rand() < 0.2 || benign.length === 0 ? cands : benign
      const cand = pickFrom[Math.floor(rand() * pickFrom.length) % pickFrom.length]
      const r = settleStep(content, dict, state, cand.step, id)
      if (r.rejected !== null) { walkRejects++; continue }
      turnLengths.push(records.length)
      executed++
      records.push(...r.records)
      id += r.records.length
      state = r.state
      snapshots.set(records.length, snap(state))

      // the log alone must always reproduce the state (full re-fold, every 10th step)
      if (i % 10 === 9) {
        expect(snap(fold(records, cloneState(base), dict, content))).toBe(snap(state))
      }
      // §9.35 ending-awareness: a careless walk stacks SUSPICION, and
      // watch(3) — the escalation the conductor's rounds and the
      // searched-again recipe both call — ends the story when the pool
      // reaches three. Undo through the ending step (it derives from the
      // fold) so the walk keeps exercising real play and `executed` stays
      // meaningful.
      if (state.ended) {
        endings++
        records.length = turnLengths.pop()!
        state = fold(records, cloneState(base), dict, content)
        const expected = snapshots.get(records.length)
        expect(expected, `undo through the ending left no snapshot at length ${records.length}`).toBeDefined()
        expect(snap(state)).toBe(expected)
        continue
      }
      // §7 undo: drop the last turn, re-fold, compare
      if (i % 37 === 36 && records.length > 1) {
        records.length = turnLengths.pop()!
        state = fold(records, cloneState(base), dict, content)
        const expected = snapshots.get(records.length)
        expect(expected, `undo to length ${records.length} left no snapshot`).toBeDefined()
        expect(snap(state)).toBe(expected)
      }
    }
    expect(executed).toBeGreaterThan(150)
    expect(walkRejects).toBeGreaterThan(0)
    expect(endings).toBeGreaterThan(0) // the escalation really fires on a careless walk

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
