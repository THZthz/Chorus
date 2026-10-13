// Shared machinery for the nightjar explorer test: the compiled Nightjar
// Flyer content, the candidate generator (sampled for the walk, exhaustive
// for the sweep), and the state sources both passes play against. No vitest
// here — scripts and tests import this alike.
import { readFileSync } from 'node:fs'
import { compileScript } from '../src/script.js'
import { executeStep, type ActionRecord, type Step } from '../src/engine.js'
import { fold } from '../src/fold.js'
import { settleStep } from './settle.js'
import { absMin, clockFromAbsMin, formatClock, nextStepClock } from '../src/clock.js'
import { parseSentence } from '../src/parse.js'
import { matchQuery } from '../src/query.js'
import { parsePlayerInput } from '../src/lenient.js'
import { nameRef, type Ref } from '../src/ref.js'
import { cloneState, type State } from '../src/types.js'
import { emptyClause, ROLES, type Clause, type Role } from '../src/sentence.js'
import type { CompiledContent, CompiledSlot } from '../src/content.js'
import { coreDictionary, type Dictionary } from '../src/dict.js'

export const dict: Dictionary = coreDictionary()
export const scriptText = readFileSync(new URL('../scripts/the-nightjar-flyer.txt', import.meta.url), 'utf8')
export const { content, state: base, meta } = compileScript(scriptText, dict)
export const PLAYER = meta.player ?? 'MARCH'

// ---------- candidate generation ----------

export interface Cand {
  tag: string
  step: Step
}

/** 'sampled' = the bounded space the random walk picks from; 'full' = the sweep's exhaustive space */
export type Depth = 'sampled' | 'full'

const roleRefs = (c: Clause, role: Role): Ref[] | Ref | null =>
  role === 'actors' ? c.actors
  : role === 'object' ? c.objects
  : role === 'with' ? c.with
  : role === 'at' ? c.at
  : role === 'about' ? c.about
  : c.to

const clauseKey = (c: Clause): string =>
  [c.verb, ...ROLES.map(r => JSON.stringify(roleRefs(c, r)))].join('|')

/** every k-subset of arr with min ≤ k ≤ max, in pool order (deterministic) */
function subsets<T>(arr: T[], min: number, max: number): T[][] {
  const out: T[][] = []
  const walk = (start: number, cur: T[]): void => {
    if (cur.length >= min) out.push([...cur])
    if (cur.length >= max) return
    for (let i = start; i < arr.length; i++) walk(i + 1, [...cur, arr[i]]!)
  }
  walk(0, [])
  return out
}

/** roles whose fill is a list (the rest are single-ref and cannot over-fill) */
const LIST_ROLES: Role[] = ['actors', 'object', 'with']

export function candidatesFor(content: CompiledContent, state: State, player: string, depth: Depth = 'sampled'): Cand[] {
  const full = depth === 'full'
  const clock = nextStepClock(state.now) // the front door's rule (§9.28)
  const alive = [...state.entities.values()].filter(e => e.deadAction === null)
  const chars = alive.filter(e => e.kind === 'character')
  const pool = (slot: CompiledSlot): typeof alive =>
    slot.query ? alive.filter(e => matchQuery(e, slot.query!)) : alive

  const out = new Map<string, Cand>()
  const minFillClauses = new Map<string, Clause>()
  const push = (tag: string, clauses: Clause[]): void => {
    const key = clauses.map(clauseKey).join(';')
    if (!out.has(key)) out.set(key, { tag, step: { clock, clauses } })
  }

  for (const verb of content.verbs.values()) {
    // options per declared role: empty (when allowed), singles, and shaped extras
    const fills = new Map<Role, Ref[][]>()
    for (const slot of verb.slots) {
      const opts: Ref[][] = []
      const candidates = slot.role === 'actors' && chars.length > 0 ? chars : pool(slot)
      if (full) {
        // the whole cardinality space (§6.1): every k-subset in [min, max] …
        for (const set of subsets(candidates, slot.min, slot.max)) {
          opts.push(set.map(e => nameRef(e.name)))
        }
        // … then the boundary: one card beyond the declared max
        if (candidates.length > slot.max && slot.max > 0) {
          opts.push(candidates.slice(0, slot.max + 1).map(e => nameRef(e.name)))
        }
      } else {
        if (slot.min === 0) opts.push([])
        for (const e of candidates.slice(0, 4)) opts.push([nameRef(e.name)])
        const actorsSlot = verb.slotByRole.get('actors')
        if (slot.role === 'actors' && actorsSlot && actorsSlot.max >= 2 && chars.length >= 2) {
          opts.push(chars.slice(0, 2).map(e => nameRef(e.name)))
        }
        if (slot.role === 'with' && candidates.length >= 2) {
          opts.push(candidates.slice(0, Math.min(3, candidates.length)).map(e => nameRef(e.name)))
        }
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
    const buildClause = (over: Map<Role, Ref[]>): Clause => {
      const c = emptyClause([])
      c.verb = verb.name
      c.actors = over.get('actors') ?? []
      c.objects = over.get('object') ?? []
      c.with = over.get('with') ?? []
      c.at = (over.get('at') ?? [])[0] ?? null
      c.about = (over.get('about') ?? [])[0] ?? null
      c.to = (over.get('to') ?? [])[0] ?? null
      return c
    }
    const mk = (over: Map<Role, Ref[]>, tag: string): void => {
      push(tag, [buildClause(over)])
    }
    mk(minFill, `${verb.name} min-fill`)
    if (full) minFillClauses.set(verb.name, buildClause(minFill))
    for (const role of ROLES) {
      const slot = verb.slotByRole.get(role)
      if (!slot) continue
      for (const opt of fills.get(role) ?? []) {
        const over = new Map(minFill)
        over.set(role, opt)
        mk(over, `${verb.name} ${role}=${opt.length}`)
      }
      // deliberate slot-mismatch probe(s): card(s) that must not fit
      if (slot.query) {
        const wrong = alive.filter(e => !matchQuery(e, slot.query!))
        for (const w of full ? wrong : wrong.slice(0, 1)) {
          const over = new Map(minFill)
          over.set(role, [nameRef(w.name)])
          mk(over, `${verb.name} ${role}=wrong-kind`)
        }
      }
      if (full && LIST_ROLES.includes(role)) {
        // §6.7: a role takes DISTINCT cards — the same card twice in one role
        const first = (minFill.get(role) ?? [])[0]
        if (first) {
          const over = new Map(minFill)
          over.set(role, [first, first])
          mk(over, `${verb.name} ${role}=dupe`)
        }
      }
    }
    if (full) {
      // §6.7: distinct cards ACROSS roles too — force each pair of roles to
      // share the same card and let the engine rule
      for (let i = 0; i < LIST_ROLES.length; i++) {
        for (let j = i + 1; j < LIST_ROLES.length; j++) {
          const a = LIST_ROLES[i]!
          const b = LIST_ROLES[j]!
          const refA = (minFill.get(a) ?? [])[0]
          if (!refA || !verb.slotByRole.has(b) || !verb.slotByRole.has(a)) continue
          const over = new Map(minFill)
          over.set(b, [refA])
          mk(over, `${verb.name} ${a}/${b}=shared`)
        }
      }
      // small verbs: the full cartesian product of role options
      const rolesWithOpts = ROLES.filter(r => (fills.get(r) ?? []).length > 0)
      let product = 1
      for (const r of rolesWithOpts) product *= (fills.get(r) ?? []).length
      if (product <= 3000 && rolesWithOpts.length >= 2) {
        const idx = new Map<Role, number>()
        const rec = (i: number): void => {
          if (i === rolesWithOpts.length) {
            const over = new Map<Role, Ref[]>()
            for (const r of rolesWithOpts) over.set(r, (fills.get(r) ?? [])[idx.get(r)!] ?? [])
            mk(over, `${verb.name} product`)
            return
          }
          const r = rolesWithOpts[i]!
          const list = fills.get(r) ?? []
          for (let k = 0; k < list.length; k++) { idx.set(r, k); rec(i + 1) }
        }
        rec(0)
      }
    }
    // witness × topic matrix for verbs that interrogate (to × about)
    const toOpts = fills.get('to') ?? []
    const aboutOpts = fills.get('about') ?? []
    if (toOpts.length > 0 && aboutOpts.length > 0) {
      const cap = full ? Infinity : 4
      for (const t of toOpts.slice(0, cap)) {
        for (const a of aboutOpts.slice(0, cap)) {
          const over = new Map(minFill)
          over.set('to', t)
          over.set('about', a)
          mk(over, `${verb.name} to×about`)
        }
      }
    }
  }

  // multi-clause and clock-discipline probes
  const waitClauses = parseSentence(`${formatClock(clock)} @ ${player} WAIT`, dict).clauses
  push('multi-clause', [...waitClauses, ...waitClauses])
  if (full) {
    // every verb against WAIT (both orders) and every verb pair — summed
    // durations (§6.6), per-clause recipe provenance, and role-ref
    // re-resolution against the draft over multi-clause steps
    const names = [...minFillClauses.keys()]
    for (const name of names) {
      const c = minFillClauses.get(name)!
      push(`${name} then wait`, [c, ...waitClauses])
      push(`wait then ${name}`, [...waitClauses, c])
      for (const other of names) {
        if (other === name) continue
        push(`${name} then ${other}`, [c, minFillClauses.get(other)!])
      }
    }
  }
  if (state.now >= 60) {
    const past = clockFromAbsMin(absMin(clock) - 60)
    push('clock rewind', parseSentence(`${formatClock(past)} @ ${player} WAIT`, dict).clauses)
  }
  return [...out.values()]
}

// ---------- state sources ----------

export const WIN_PATH = [
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

/** play the win path on primitives, snapshotting a fresh clone after each step
 *  (each step settles — its window completes, the way the pre-v0.5 engine
 *  committed effects at once) */
export function winPathStates(): { states: { label: string; state: State }[]; records: ActionRecord[] } {
  let state = cloneState(base)
  const states = [{ label: 'start', state: cloneState(state) }]
  const records: ActionRecord[] = []
  let id = 1
  for (const line of WIN_PATH) {
    const r = settleStep(content, dict, state, parsePlayerish(line, state), id)
    if (r.rejected) throw new Error(`win path broke at '${line}': ${r.rejected}`)
    records.push(...r.records)
    id += r.records.length
    state = r.state
    states.push({ label: `after ${line}`, state: cloneState(state) })
  }
  return { states, records }
}

/** diverse states beyond the win path: a short deterministic sampled walk
 *  (no verification — the walk pass owns that) harvested every 20 applied
 *  steps, up to ten states */
export function sampledWalkStates(): { label: string; state: State }[] {
  let seed = 0x4e494748 // 'NIGH' — same LCG discipline as the walk pass
  const rand = (): number => ((seed = (Math.imul(seed, 1664525) + 1013904223) >>> 0) / 0x100000000)
  let state = cloneState(base)
  const records: ActionRecord[] = []
  const states: { label: string; state: State }[] = []
  const turnLengths: number[] = [0] // ending-undo truncates to the previous turn (v0.5)
  let id = 1
  let executed = 0
  for (let i = 0; i < 500 && states.length < 10; i++) {
    const cands = candidatesFor(content, state, PLAYER)
    const benign = cands.filter(c => !c.tag.includes('wrong-kind') && c.tag !== 'clock rewind')
    const pickFrom = rand() < 0.2 || benign.length === 0 ? cands : benign
    const cand = pickFrom[Math.floor(rand() * pickFrom.length) % pickFrom.length]
    const r = settleStep(content, dict, state, cand.step, id)
    if (r.rejected !== null) continue
    turnLengths.push(records.length)
    records.push(...r.records)
    id += r.records.length
    state = r.state
    // §9.35 ending-awareness, as in the walk pass: undo through the ending
    // step so the harvest keeps exercising real play and reaches later hours
    if (state.ended) {
      records.length = turnLengths.pop()!
      state = fold(records, cloneState(base), dict, content)
      continue
    }
    executed++
    if (executed % 20 === 0) states.push({ label: `walk ${executed}`, state: cloneState(state) })
  }
  return states
}
