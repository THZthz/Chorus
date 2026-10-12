// Core data model — spec §4.2, §7

export interface Clock {
  day: number
  /** halfhours since day start (0..47); one halfhour = 30 minutes */
  halfhour: number
}

/** Weighted principle tags with optional one-level dot sub-keys: {secret:4, language:{latin:2}} */
export type Aspects = Record<string, number | Record<string, number>>

export type AttrValue = string | number | boolean
export type Attrs = Record<string, AttrValue>

export interface Entity {
  id: string
  kind: string
  name: string
  aspects: Aspects
  attrs: Attrs
  bornAction: number | null
  deadAction: number | null
  /** minted from a `stackable` declaration (§9.38): the name may be held by
   *  several living copies. Every mint site computes it from the compiled
   *  stackable sets; the fold recomputes it identically from the logged ref. */
  stackable: boolean
}

export interface Relation {
  a: string
  b: string
  type: string
  value: number
  bornAction: number
  deadAction: number | null
}

/** a story's ending (§9.35): set by the first ENDING outcome to apply. The
 *  resolved title/flavour ride in the state so renderers need no content
 *  lookup; the log itself carries only the concrete `ENDING <id>` op, and
 *  both play and fold resolve through the same applyOutcome code path. */
export interface Ending {
  id: string
  title: string
  flavour: string
}

export interface State {
  entities: Map<string, Entity>
  /** name -> living ids in birth order (head = OLDEST copy, §9.38 pool
   *  addressing); unique-name law for non-stackable names still holds (§9.14) */
  byName: Map<string, string[]>
  /** key: `${a}|${b}|${type}` with a<=b for symmetric types */
  relations: Map<string, Relation>
  /** engine clock, absolute minutes since day 0, tick 0 */
  now: number
  /** once-triggers that have fired (spec §9.29); rebuilt by fold from EVENT rows */
  fired: Set<string>
  /** the ending once told (spec §9.35); null until an ENDING outcome applies,
   *  cleared naturally when the fold no longer reaches its record (undo) */
  ended: Ending | null
  /** §9.41: the seeded-RNG counter — how many die values the whole history has
   *  consumed. Play consumes counters ON THE DRAFT inside the ROLL outcome;
   *  the fold lands the same spot from the concrete `ROLLED(<total>, <ctrAfter>)`
   *  row — replay never re-derives a die (law 3), and undo reconstructs the
   *  counter from the folded log. */
  rng: number
  /** §9.42: deck cursors — for each deck id, how many cards it has served.
   *  Play advances the cursor ON THE DRAFT inside the DRAW outcome; the fold
   *  lands the same spot from the concrete `DRAWN(<deck>, <name>)` row, which
   *  also asserts the name against the compiled permutation (content drift
   *  fails loud). The permutation itself is pure in (seed, deck id) and lives
   *  on CompiledContent — never here (law 3: replay re-derives only through
   *  the recorded row, and the row is what makes drift visible). */
  decks: Map<string, number>
}

export function newState(now = 0): State {
  return { entities: new Map(), byName: new Map(), relations: new Map(), now, fired: new Set(), ended: null, rng: 0, decks: new Map() }
}

export function cloneState(s: State): State {
  const entities = new Map<string, Entity>()
  for (const [id, e] of s.entities) {
    entities.set(id, { ...e, aspects: cloneAspects(e.aspects), attrs: { ...e.attrs } })
  }
  const relations = new Map<string, Relation>()
  for (const [k, r] of s.relations) relations.set(k, { ...r })
  // law 6 (§9.38): the pool ARRAYS are deep-copied — a shallow `new Map` would
  // share arrays, so a discarded draft's pool destroy would mutate real state
  const byName = new Map<string, string[]>()
  for (const [k, v] of s.byName) byName.set(k, [...v])
  // law 6 (§9.42): cursors are copied — a discarded draft's draws must not
  // move the real deck (values are numbers, so a fresh Map is a deep copy)
  const decks = new Map(s.decks)
  return { entities, byName, relations, now: s.now, fired: new Set(s.fired), ended: s.ended ? { ...s.ended } : null, rng: s.rng, decks }
}

function cloneAspects(a: Aspects): Aspects {
  const out: Aspects = {}
  for (const [k, v] of Object.entries(a)) out[k] = typeof v === 'number' ? v : { ...v }
  return out
}

/** §9.14 + §9.38: names are unique among the living — except stackable mints,
 *  which append to the pool. A stackable mint may not collide with a DISTINCT
 *  non-stackable living name (shared namespace): under the mint rules a mixed
 *  pool is impossible (a non-stackable mint demands an empty pool), so checking
 *  the existing members for stackability is exact. */
export function addEntity(s: State, e: Entity): void {
  if (e.deadAction === null) {
    const pool = s.byName.get(e.name)
    if (pool && pool.length > 0) {
      const existingStackable = pool.every(id => s.entities.get(id)?.stackable === true)
      if (!e.stackable || !existingStackable) {
        throw new Error(`duplicate name '${e.name}' among alive entities`)
      }
    }
    if (pool) pool.push(e.id)
    else s.byName.set(e.name, [e.id])
  }
  s.entities.set(e.id, e)
}

/** §9.14/§9.38 name release on death: remove the id from the name's pool;
 *  the name only frees when the LAST copy dies. */
export function releaseName(s: State, e: Entity): void {
  const pool = s.byName.get(e.name)
  if (!pool) return
  const i = pool.indexOf(e.id)
  if (i !== -1) pool.splice(i, 1)
  if (pool.length === 0) s.byName.delete(e.name)
}

/** oldest living copy bearing a name (§9.38 pool addressing: a name ref with
 *  N > 1 living copies addresses the HEAD of its pool — never id-string order) */
export function oldestNamed(s: State, name: string): Entity | undefined {
  for (const id of s.byName.get(name) ?? []) {
    const e = s.entities.get(id)
    if (e && e.deadAction === null) return e
  }
  return undefined
}

export function getEntity(s: State, id: string): Entity | undefined {
  return s.entities.get(id)
}

export function getAlive(s: State, id: string): Entity | undefined {
  const e = s.entities.get(id)
  return e && e.deadAction === null ? e : undefined
}

/** raw aspect entry by dotted path: 'secret' | 'language.latin' (missing → undefined).
 *  Unlike aspectGet this surfaces sub-maps, so a delta can refuse instead of
 *  silently clobbering one (§9.31). */
export function aspectEntry(e: Entity, path: string): number | Aspects | undefined {
  const dot = path.indexOf('.')
  if (dot === -1) return e.aspects[path]
  const sub = e.aspects[path.slice(0, dot)]
  if (typeof sub === 'number') return undefined
  return sub?.[path.slice(dot + 1)]
}

/** aspect lookup by dotted path: 'secret' | 'language.latin' (missing → undefined) */
export function aspectGet(e: Entity, path: string): number | undefined {
  const v = aspectEntry(e, path)
  return typeof v === 'number' ? v : undefined
}

/** aspect write by dotted path — creates the sub-object when missing (mirrors aspectGet) */
export function aspectSet(e: Entity, path: string, v: number): void {
  const dot = path.indexOf('.')
  if (dot === -1) {
    e.aspects[path] = v
    return
  }
  const head = path.slice(0, dot)
  const sub = e.aspects[head]
  const obj = typeof sub === 'object' ? sub : (e.aspects[head] = {})
  obj[path.slice(dot + 1)] = v
}

export function attrGet(e: Entity, key: string): AttrValue | undefined {
  return e.attrs[key]
}

export function relationKey(a: string, b: string, type: string, symmetric: boolean): string {
  // symmetric edges get one canonical key (sorted); directed edges keep the
  // stored direction — swapping directed endpoints loses the direction
  if (symmetric && a > b) [a, b] = [b, a]
  return `${a}|${b}|${type}`
}

export function relationAlive(s: State, idA: string, idB: string, type: string): boolean {
  // relations are stored under one canonical key; check both orders so this
  // stays correct regardless of symmetric/directed storage
  for (const k of [`${idA}|${idB}|${type}`, `${idB}|${idA}|${type}`]) {
    const r = s.relations.get(k)
    if (r && r.deadAction === null) return true
  }
  return false
}
