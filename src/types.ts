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
}

export interface Relation {
  a: string
  b: string
  type: string
  value: number
  bornAction: number
  deadAction: number | null
}

export interface State {
  entities: Map<string, Entity>
  /** name -> id, alive entities only (names are unique among the living, spec §9.14) */
  byName: Map<string, string>
  /** key: `${a}|${b}|${type}` with a<=b for symmetric types */
  relations: Map<string, Relation>
  /** engine clock, absolute minutes since day 0, tick 0 */
  now: number
  /** once-triggers that have fired (spec §9.29); rebuilt by fold from EVENT rows */
  fired: Set<string>
}

export function newState(now = 0): State {
  return { entities: new Map(), byName: new Map(), relations: new Map(), now, fired: new Set() }
}

export function cloneState(s: State): State {
  const entities = new Map<string, Entity>()
  for (const [id, e] of s.entities) {
    entities.set(id, { ...e, aspects: cloneAspects(e.aspects), attrs: { ...e.attrs } })
  }
  const relations = new Map<string, Relation>()
  for (const [k, r] of s.relations) relations.set(k, { ...r })
  return { entities, byName: new Map(s.byName), relations, now: s.now, fired: new Set(s.fired) }
}

function cloneAspects(a: Aspects): Aspects {
  const out: Aspects = {}
  for (const [k, v] of Object.entries(a)) out[k] = typeof v === 'number' ? v : { ...v }
  return out
}

export function addEntity(s: State, e: Entity): void {
  if (e.deadAction === null) {
    if (s.byName.has(e.name)) throw new Error(`duplicate name '${e.name}' among alive entities`)
    s.byName.set(e.name, e.id)
  }
  s.entities.set(e.id, e)
}

export function getEntity(s: State, id: string): Entity | undefined {
  return s.entities.get(id)
}

export function getAlive(s: State, id: string): Entity | undefined {
  const e = s.entities.get(id)
  return e && e.deadAction === null ? e : undefined
}

/** aspect lookup by dotted path: 'secret' | 'language.latin' (missing → undefined) */
export function aspectGet(e: Entity, path: string): number | undefined {
  const dot = path.indexOf('.')
  if (dot === -1) {
    const v = e.aspects[path]
    return typeof v === 'number' ? v : undefined
  }
  const sub = e.aspects[path.slice(0, dot)]
  if (typeof sub === 'number') return undefined
  return sub?.[path.slice(dot + 1)]
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
