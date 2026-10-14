// Randomized differential testing for src/reach.ts — NO vitest here (scripts
// and tests import this alike, explorer-lib precedent).
//
// Per seed: generate a small random scenario (verbs × recipes with when/IF/
// ROLL guards, SPAWN/REPLACE/DRAW/SET/DESTROY/REFUSE, slot triggers + expiry,
// a deck with defaultcard/reset, stackable and derive mints), compile it, run
// the static analyzer over a probe set, then run an EXHAUSTIVE bounded
// forward exploration driven only by real engine steps (settleStep), and
// check the differential properties:
//
//   unobtainable ⇒ the card/demand NEVER appears in any explored play   (hard)
//   obtainable   ⇒ it appears in some explored play                     (hard
//                  when the exploration exhausted its bounds; downgraded to
//                  'inconclusive' when the state cap hit — the ground truth is
//                  an under-approximation, so only the first direction is
//                  sound under arbitrary bounds)
//
// 'unproven' asserts nothing by design. A failure is reproducible from its
// seed: runFuzzSeed(seed) is a pure function of the seed.
import { compileContent, type CompiledContent, type ContentRaw } from '../src/content/content.js'
import { CORE_VERBS, KINDS, coreDictionary, type Dictionary } from '../src/core/dict.js'
import { absMin, clockFromAbsMin, formatClock, nextStepClock } from '../src/core/clock.js'
import { parseSentence } from '../src/core/parse.js'
import { matchQuery, parseQuery } from '../src/core/query.js'
import { analyzeCard } from '../src/reach/reach.js'
import { settleStep } from './settle.js'
import { emptyClause, type Clause, type Role } from '../src/core/sentence.js'
import { addEntity, newState, type Aspects, type Entity, type State } from '../src/core/types.js'
import type { Ref } from '../src/core/ref.js'
import { idRef, nameRef } from '../src/core/ref.js'
import type { Step } from '../src/engine/engine.js'

// ---------- deterministic RNG (explorer-lib's LCG discipline) ----------

export function rng(seed: number): () => number {
  let s = (seed ^ 0x9e3779b9) >>> 0
  return () => ((s = (Math.imul(s, 1664525) + 1013904223) >>> 0) / 0x100000000)
}
type Rand = () => number
const int = (r: Rand, n: number): number => Math.floor(r() * n)
const pick = <T>(r: Rand, arr: T[]): T => arr[int(r, arr.length)]!
function sampleDistinct<T>(r: Rand, arr: T[], n: number): T[] {
  const pool = [...arr]
  const out: T[] = []
  while (out.length < n && pool.length > 0) out.push(pool.splice(int(r, pool.length), 1)[0]!)
  return out
}

// ---------- scenario generator ----------

const NON_WAIT_VERBS = CORE_VERBS.filter(v => v !== 'WAIT')
const ASPECTS = ['a', 'b']
const NAME_POOL = ['LAMP', 'ROPE', 'LEDGER', 'CRATE', 'VIAL']
const KIND_POOL = ['item', 'text', 'idea', 'place', 'resource']
const NON_CHAR_KINDS = KIND_POOL.filter(k => k !== 'place')

export interface Scenario {
  seed: number
  raw: ContentRaw
  initial: State
  /** the initial entities as {name, kind} — compileContent's deck-entry
   *  validation takes them explicitly (compileScript passes the same list) */
  entityKinds: { name: string; kind: string }[]
  probes: { mode: 'name' | 'demand'; q: string }[]
}

const ent = (name: string, kind: string, aspects: Aspects = {}): Entity => ({
  id: `s_${name.toLowerCase()}`,
  kind,
  name,
  aspects,
  attrs: {},
  bornAction: 0,
  deadAction: null,
  stackable: false,
})

export function genScenario(seed: number): Scenario {
  const r = rng(seed)
  const verbs = sampleDistinct(r, NON_WAIT_VERBS, 2 + int(r, 3))   // 2–4
  const aspectVal = () => int(r, 4)                                // 0..3

  // initial entities — two characters (ANA the player + maybe BERTHA), 1–3
  // pool cards, maybe a place: enough live cards that object/with slots and
  // cross-role distinctness actually bite
  const entities: Entity[] = [ent('ANA', 'character')]
  const others = sampleDistinct(r, NAME_POOL, 1 + int(r, 3))
  for (const n of others) entities.push(ent(n, pick(r, NON_CHAR_KINDS), { a: aspectVal(), b: int(r, 3) }))
  const hasBertha = r() < 0.5
  if (hasBertha) entities.push(ent('BERTHA', 'character'))
  const hasHouse = r() < 0.6
  if (hasHouse) entities.push(ent('HOUSE', 'place'))
  const charNames = ['ANA', ...(hasBertha ? ['BERTHA'] : [])]
  const placeNames = hasHouse ? ['HOUSE'] : []

  // prototypes — static aspects, stackable, derive mints (up to one)
  const protoNames = ['P1', 'P2', 'P3'].slice(0, 1 + int(r, 3))
  const prototypes: Record<string, Record<string, unknown>> = {}
  let deriveDone = false
  for (const pn of protoNames) {
    const p: Record<string, unknown> = { kind: pick(r, NON_CHAR_KINDS) }
    if (r() < 0.6) p.aspects = { [pick(r, ASPECTS)]: int(r, 3) }
    if (r() < 0.25) p.stackable = true
    if (!deriveDone && r() < 0.25) { p.derive = { nameFrom: 'object' }; deriveDone = true }
    prototypes[pn] = p
  }

  // deck — repeats weight the spec; defaultcard/reset combinations
  const deckNames = [...protoNames, ...others]
  const deck = r() < 0.6 && deckNames.length >= 2
    ? {
        id: 'D1',
        spec: Array.from({ length: 2 + int(r, 2) }, () => pick(r, deckNames)),
        ...(r() < 0.4 ? { defaultcard: pick(r, deckNames) } : {}),
        ...(r() < 0.3 ? { reset: true } : {}),
      }
    : undefined

  // outcome ops (strings the parser accepts; parseOutcomeString splits lists)
  const op = (allowRefuse: boolean): string => {
    const roll = r()
    if (roll < 0.22) {
      const pn = pick(r, protoNames)
      const kindOverride = r() < 0.2 ? `${pick(r, NON_CHAR_KINDS)}:` : ''
      const into = hasHouse && r() < 0.15 ? ' INTO HOUSE' : ''
      return `SPAWN ${kindOverride}${pn}${into}`
    }
    if (roll < 0.32 && others.length > 0) return `REPLACE ${pick(r, others)} WITH ${pick(r, protoNames)}`
    if (roll < 0.42 && deck) return 'DRAW D1'
    if (roll < 0.56) {
      const tgt = pick(r, ['ACTORS', 'OBJECT', 'ANA', 'BERTHA'])
      const key = pick(r, ['aspect:a', 'aspect:b', 'coin', 'state.sealed', ...(r() < 0.1 ? ['expires'] : [])])
      const val = key === 'expires' ? pick(r, ['30m', '1halfhour']) : r() < 0.6 ? `+${1 + int(r, 2)}` : String(int(r, 4))
      return `SET(${tgt}, ${key}, ${val})`
    }
    if (roll < 0.62 && others.length > 0) return `DESTROY ${pick(r, others)}`
    if (roll < 0.70 && hasHouse) return 'MOVE ANA INTO HOUSE'
    if (roll < 0.78) {
      const x = pick(r, [...others, ...protoNames, 'ANA', 'GHOST'])
      const inner = r() < 0.3 && protoNames.length >= 2
        ? `IF exists(${pick(r, protoNames)}) -> (SPAWN ${pick(r, protoNames)}) ELSE -> (NARRATE "nested")`
        : `SPAWN ${pick(r, protoNames)}`
      return `IF exists(${x}) -> (${inner})`
    }
    if (roll < 0.90) {
      const dice = r() < 0.3 ? 2 : 1
      return `ROLL(${dice}, 6), IF roll >= ${2 + int(r, 4 * dice)} -> (SPAWN ${pick(r, protoNames)}) ELSE -> (NARRATE "low")`
    }
    if (roll < 0.94 && allowRefuse) return 'REFUSE("no")'
    return 'NARRATE "beat"'
  }

  const verbsRaw: NonNullable<ContentRaw['verbs']> = {
    WAIT: { slots: [{ role: 'actors', min: 1, max: 1, query: 'kind:character' }], duration: 30, recipes: [{ out: [] }] },
  }
  for (const v of verbs) {
    const slots: NonNullable<ContentRaw['verbs']>[string]['slots'] = [
      { role: 'actors', min: 1, max: 1, query: 'kind:character' },
    ]
    for (let s = 0, nS = int(r, 3); s < nS; s++) {          // 0–2 non-actor slots
      const role = pick(r, ['object', 'with', 'with'] as const)
      const kindGate = r() < 0.5 ? ` kind:${pick(r, NON_CHAR_KINDS)}` : ''
      const aspectGate = r() < 0.4 ? ` aspect:${pick(r, ASPECTS)}>=${1 + int(r, 3)}` : ''
      const query = (kindGate + aspectGate).trim()
      const double = role === 'with' && r() < 0.15           // min:2 — distinctness bites
      slots.push({ role, min: double ? 2 : 1, max: double ? 2 : 1, ...(query ? { query } : {}) })
    }
    const recipes: { id?: string; when?: string; out: string[] }[] = []
    for (let i = 0, n = 1 + int(r, 3); i < n; i++) {
      const when = r() < 0.4
        ? pick(r, [
            `actors.aspect.a >= ${1 + int(r, 3)}`,
            `exists(${pick(r, [...others, ...protoNames, 'ANA', 'GHOST'])})`,
            `count(name:ANA) >= 1`,
            `count(name:${pick(r, protoNames.length ? protoNames : ['P1'])}) >= 1`,
          ])
        : undefined
      const outs: string[] = []
      for (let j = 0, m = 1 + int(r, 4); j < m; j++) outs.push(op(true))
      recipes.push({ id: `${v.toLowerCase()}.r${i}`, ...(when ? { when } : {}), out: outs })
    }
    verbsRaw[v] = { slots, duration: pick(r, [0, 30, 60]), recipes }
  }

  const triggers: (Record<string, unknown> & { id: string })[] = []
  for (let i = 0, n = 1 + int(r, 3); i < n; i++) {
    const out = Array.from({ length: 1 + int(r, 2) }, () => op(false))   // events cannot REFUSE (§6.7)
    triggers.push(r() < 0.25
      ? { id: `t${i}`, onExpireOf: pick(r, ['ANA', ...others]), out }
      : { id: `t${i}`, slot: int(r, 48), once: r() < 0.4, out })
  }

  const raw: ContentRaw = {
    seed: int(r, 1000),
    verbs: verbsRaw,
    prototypes: prototypes as unknown as ContentRaw['prototypes'],
    ...(deck ? { decks: [deck] } : {}),
    ...(triggers.length ? { triggers: triggers as unknown as ContentRaw['triggers'] } : {}),
  }
  const initial = newState()
  for (const e of entities) addEntity(initial, e)

  // probes — every name that could ever exist, derived shapes, and demands
  const probes: Scenario['probes'] = []
  for (const n of [...new Set(['ANA', 'BERTHA', ...others, ...protoNames, ...(deck ? deck.spec : []), ...(deck?.defaultcard ? [deck.defaultcard] : [])])]) {
    probes.push({ mode: 'name', q: n })
  }
  const deriveProto = Object.entries(prototypes).find(([, p]) => (p as { derive?: unknown }).derive)?.[0]
  if (deriveProto) {
    // every object half the derive can bind, not just the first pool card
    for (const n of others) probes.push({ mode: 'name', q: `${deriveProto}_${n}` })
  }
  // named in guards only (IF exists(GHOST)) — no producer anywhere, so the
  // sound direction (unobtainable ⇒ never appears) is asserted for free
  probes.push({ mode: 'name', q: 'GHOST' })
  probes.push({ mode: 'demand', q: `aspect:a>=${1 + int(r, 3)}` })
  probes.push({ mode: 'demand', q: `aspect:b>=${1 + int(r, 2)}` })
  probes.push({ mode: 'demand', q: `kind:${pick(r, KIND_POOL)}` })
  probes.push({ mode: 'demand', q: `name:${pick(r, [...others, ...protoNames, 'ANA'])}` })
  probes.push({ mode: 'demand', q: `kind:${pick(r, NON_CHAR_KINDS)} aspect:a>=${1 + int(r, 2)}` })

  return { seed, raw, initial, entityKinds: entities.map(e => ({ name: e.name, kind: e.kind })), probes }
}

// ---------- exhaustive bounded forward exploration (the ground truth) ----------

export interface Forward {
  everNames: Set<string>
  everDesc: Entity[]
  statesSeen: number
  /** pushes dropped because an equal-signature state was already queued —
   *  pure diagnostics (budget tuning) */
  deduped: number
  capped: boolean
}

const canon = (o: Record<string, unknown>): string =>
  '{' + Object.keys(o).sort().map(k => {
    const v = o[k]
    return `${k}=${v !== null && typeof v === 'object' ? canon(v as Record<string, unknown>) : JSON.stringify(v)}`
  }).join(',') + '}'

const scrub = (a: Record<string, unknown>): Record<string, unknown> =>
  Object.fromEntries(Object.entries(a).filter(([k]) => k !== 'container'))

const sig = (s: State): string => {
  const nameOf = new Map([...s.entities.values()].map(e => [e.id, e.name] as const))
  return JSON.stringify({
    now: s.now,
    // §9.41: the dice counter is State — two worlds one ROLL apart read the
    // same here but diverge on every later die (IF roll >= k arms), so
    // collapsing them would silently drop a subtree
    g: s.rng,
    ended: s.ended?.id ?? null,
    e: [...s.entities.values()]
      .filter(e => e.deadAction === null)
      .map(e => [e.name, e.kind, canon(e.aspects as Record<string, unknown>), canon(scrub(e.attrs))])
      .sort(),
    // relations are ids — map through names like locked windows (a generated
    // scenario never RELATEs today, but a signature that ignores a State
    // field is a latent subtree loss)
    r: [...s.relations.values()]
      .filter(rel => rel.deadAction === null)
      .map(rel => [nameOf.get(rel.a) ?? rel.a, nameOf.get(rel.b) ?? rel.b, rel.type, rel.value])
      .sort(),
    f: [...s.fired].sort(),
    d: [...s.decks.entries()].map(([k, v]) => `${k}:${v}`).sort(),
    // a window's completion applies ITS recipe's outcomes anchored to w.start
    // (SET expires!) over ITS role bindings — verb+end+locked alone would
    // collapse worlds whose completions mint different cards. beginId/clause
    // stay OUT: they are exploration-order artifacts, not world facts.
    w: [...s.windows.values()]
      .map(w => [
        w.verb, w.start, w.end, w.recipe,
        ...Object.entries(w.refs)
          .flatMap(([role, refs]) => refs.map(ref => `${role}:${ref.form === 'name' ? ref.name : ref.form === 'id' ? nameOf.get(ref.id) ?? ref.id : `param:${ref.name}`}`))
          .sort(),
        ...w.locked.map(id => nameOf.get(id) ?? id).sort(),
      ])
      .sort(),
  })
}

function kSubsets<T>(arr: T[], k: number): T[][] {
  const out: T[][] = []
  const walk = (start: number, cur: T[]): void => {
    if (cur.length === k) { out.push([...cur]); return }
    for (let i = start; i < arr.length; i++) walk(i + 1, [...cur, arr[i]!])
  }
  walk(0, [])
  return out
}

function candidates(content: CompiledContent, dict: Dictionary, s: State): Step[] {
  const out: Step[] = []
  const minFills: Clause[] = []
  const alive = [...s.entities.values()].filter(e => e.deadAction === null)
  const clock = nextStepClock(s.now)
  // waits — advance the clock so slot triggers, expiries and completions
  // strike. The 1hh/2hh hops isolate a single boundary (two same-slot
  // triggers minting the same name reject any step that crosses both — fuzz
  // seed 33); the 48hh jump reaches a second daily crossing fast.
  for (const ahead of [0, 1, 2, 12, 24, 48]) {
    const begin = ahead === 0 ? clock : clockFromAbsMin(absMin(clock) + ahead * 30)
    const parsed = parseSentence(`${formatClock(begin)} @ ANA WAIT`, dict)
    out.push({ clock: parsed.clock ?? begin, clauses: parsed.clauses })
  }
  const waitClause = parseSentence(`${formatClock(clock)} @ ANA WAIT`, dict).clauses
  const idOf = (name: string): string => alive.find(e => e.name === name)?.id ?? name
  for (const verb of content.verbs.values()) {
    if (verb.name === 'WAIT') continue
    // the engine validates ONE slot per role: validateSlots iterates ROLES
    // and reads slotByRole, whose LAST entry per role wins (content.ts §6).
    // Duplicate role defs in verb.slots are inert at commit time — building
    // perRole from the raw array imposed cross-entry distinctness the engine
    // never checks, and could yield ZERO combos for a committable verb
    // (fuzz seed 12606: WORK actors + with×2 + with×2 mints P2 in one step —
    // engine-confirmed via settleStep — but the explorer demanded five
    // distinct live cards and never generated a single WORK clause).
    const effSlots: typeof verb.slots = []
    {
      const byRole = new Map<string, (typeof verb.slots)[number]>()
      for (const slot of verb.slots) byRole.set(slot.role, slot)   // last def wins
      for (const slot of verb.slots) {
        if (byRole.get(slot.role) === slot) effSlots.push(slot)
      }
    }
    const perRole: { role: Role; opts: Ref[][] }[] = []
    let viable = true
    for (const slot of effSlots) {
      const pool = alive.filter(e => (slot.query ? matchQuery(e, slot.query) : true))
      const maxK = Math.min(slot.max === Infinity ? 2 : slot.max, 2)
      const opts: Ref[][] = []
      for (let k = slot.min; k <= maxK; k++) {
        for (const set of kSubsets(pool, k)) {
          // a binding that takes TWO copies of one stackable name must pin
          // them by #id: a name ref resolves to the OLDEST pool copy (§9.38),
          // so two name refs collapse onto one entity and the placed-set
          // check rejects the clause ("already placed in 'role'") — the
          // commit the engine accepts was invisible to the explorer (fuzz
          // seed 60447: two TALK mints stack P3; `ANA WORK WITH #p3a, #p3b`
          // commits (engine-confirmed via settleStep) and work.r0 mints the
          // derive-card P2 bare, since WORK has no object slot). Distinct
          // names keep the name-ref form.
          const dup = new Set(set.map(e => e.name)).size !== set.length
          opts.push(set.map(e => (dup ? idRef(e.id) : nameRef(e.name))))
        }
      }
      if (slot.min > 0 && opts.length === 0) { viable = false; break }
      perRole.push({ role: slot.role, opts: opts.length > 0 ? opts : [[]] })
    }
    if (!viable) continue
    // cross-role identity: id refs pin their copy directly; name refs
    // collapse to the OLDEST living copy (§9.38) — the same key comparison
    // the engine's per-role placed-set effectively performs
    const refKey = (r: Ref): string => (r.form === 'id' ? r.id : idOf(r.name))
    let combos: Map<Role, Ref[]>[] = [new Map()]
    for (const { role, opts } of perRole) {
      const next: Map<Role, Ref[]>[] = []
      for (const base of combos) {
        for (const opt of opts.slice(0, 6)) {
          const used = [...base.values()].flat().map(refKey)
          if (opt.map(refKey).some(id => used.includes(id))) continue
          const m = new Map(base)
          m.set(role, opt)
          next.push(m)
          if (next.length >= 8) break
        }
        if (next.length >= 8) break
      }
      combos = next
    }
    for (const combo of combos) {
      const clause: Clause = emptyClause([])
      clause.verb = verb.name
      for (const [role, refs] of combo) {
        if (role === 'actors') clause.actors = refs
        else if (role === 'object') clause.objects = refs
        else if (role === 'with') clause.with = refs
        else if (role === 'at') clause.at = refs[0] ?? null
        else if (role === 'about') clause.about = refs[0] ?? null
        else if (role === 'to') clause.to = refs[0] ?? null
      }
      out.push({ clock, clauses: [clause] })
      minFills.push(clause)
    }
  }
  // multi-clause steps — per-clause recipe provenance, summed durations, and
  // cross-clause locks (§6.6/§6.7): verb+WAIT and verb+verb pairs
  for (let i = 0; i < minFills.length && i < 3; i++) {
    out.push({ clock, clauses: [minFills[i]!, ...waitClause] })
    for (let j = i + 1; j < minFills.length && j < 3; j++) {
      out.push({ clock, clauses: [minFills[i]!, minFills[j]!] })
    }
  }
  return out
}

export function explore(content: CompiledContent, dict: Dictionary, initial: State, maxSteps: number, maxStates: number): Forward {
  const everNames = new Set<string>()
  const everDesc = new Map<string, Entity>()
  const visited = new Set<string>()
  let statesSeen = 0
  let deduped = 0
  let capped = false
  let idCounter = 0
  const record = (s: State): void => {
    for (const e of s.entities.values()) {
      if (e.deadAction !== null) continue
      everNames.add(e.name)
      // attrs belong in the key: a later SET(state.sealed/expires) on the same
      // living card is a NEW demand-descriptor, not a re-sight of the old one
      const key = `${e.name}|${e.kind}|${canon(e.aspects as Record<string, unknown>)}|${canon(scrub(e.attrs))}`
      if (!everDesc.has(key)) {
        everDesc.set(key, { ...e, attrs: scrub(e.attrs) as Entity['attrs'] })
      }
    }
  }
  // dedupe at PUSH time: the old pop-time check counted every duplicate pop
  // against maxStates AND let the queue grow with them — the budget bought
  // re-sights instead of new worlds. BFS pop order makes the first (shallowest)
  // push the keeper; equal-signature states record identical descriptors.
  const queue: { s: State; steps: number }[] = []
  const enqueue = (s: State, steps: number): void => {
    const key = sig(s)
    if (visited.has(key)) { deduped++; return }
    visited.add(key)
    record(s)
    queue.push({ s, steps })
  }
  enqueue(initial, 0)
  while (queue.length > 0) {
    if (statesSeen >= maxStates) { capped = true; break }
    const { s, steps } = queue.shift()!
    statesSeen++
    if (steps >= maxSteps || s.ended) continue
    for (const step of candidates(content, dict, s)) {
      const res = settleStep(content, dict, s, step, 1 + idCounter, () => `f${idCounter++}`)
      if (res.rejected !== null) continue
      enqueue(res.state, steps + 1)
    }
  }
  return { everNames, everDesc: [...everDesc.values()], statesSeen, deduped, capped }
}

// ---------- the differential property ----------

export interface FuzzOutcome {
  seed: number
  failures: string[]
  inconclusive: string[]
  statesSeen: number
  deduped: number
  probes: number
}

export function runFuzzSeed(seed: number, maxSteps = 8, maxStates = 500): FuzzOutcome {
  const sc = genScenario(seed)
  const dict = coreDictionary()
  const content = compileContent(sc.raw, dict, sc.entityKinds)
  const fw = explore(content, dict, sc.initial, maxSteps, maxStates)
  const failures: string[] = []
  const inconclusive: string[] = []
  // at most one deep exploration per seed — every failing probe would
  // otherwise re-run the identical (content, initial, bounds) search
  let deep: Forward | null = null
  const deepRun = (): Forward => (deep ??= explore(content, dict, sc.initial, maxSteps + 7, maxStates * 20))
  for (const p of sc.probes) {
    const rep = p.mode === 'name'
      ? analyzeCard(content, sc.initial, p.q, {})
      : analyzeCard(content, sc.initial, '', { demand: p.q })
    // path-export invariants: a proven verdict MUST come with at least one
    // exported path, and (name mode) every exported root must mint the probed
    // name — the enumeration and the verdict must never disagree
    if (rep.existence === 'proven' && rep.paths.length === 0) {
      failures.push(`seed ${seed}: '${p.q}' proven obtainable but the path export is empty`)
    }
    if (p.mode === 'name') {
      for (const root of rep.paths) {
        if (root.mints !== p.q) failures.push(`seed ${seed}: exported path mints '${root.mints}' but the probe was '${p.q}'`)
      }
    }
    const truth = p.mode === 'name'
      ? fw.everNames.has(p.q)
      : fw.everDesc.some(e => matchQuery(e, parseQuery(p.q)))
    if (rep.verdict === 'unobtainable' && truth) {
      failures.push(`seed ${seed}: '${p.q}' judged UNOBTAINABLE but appears in play (verdict ${rep.verdict}/${rep.existence})`)
    }
    if (rep.verdict === 'obtainable' && !truth) {
      // second phase — a deeper exhaustive oracle. The analyzer's claims are
      // existential over UNBOUNDED play (repeatable verbs, recurring
      // triggers), so a shallow miss can be horizon, not error.
      const deepFw = deepRun()
      const truthDeep = p.mode === 'name'
        ? deepFw.everNames.has(p.q)
        : deepFw.everDesc.some(e => matchQuery(e, parseQuery(p.q)))
      if (truthDeep) {
        // analyzer vindicated at depth — base horizon was the limit
      } else if (deepFw.capped) {
        inconclusive.push(`seed ${seed}: '${p.q}' obtainable but the deep exploration hit its state cap`)
      } else {
        failures.push(`seed ${seed}: '${p.q}' judged OBTAINABLE but never appears in the exhaustive ${maxSteps + 7}-step exploration (chain: ${rep.chain[0] ?? '?'})`)
      }
    }
  }
  return { seed, failures, inconclusive, statesSeen: fw.statesSeen, deduped: fw.deduped, probes: sc.probes.length }
}
