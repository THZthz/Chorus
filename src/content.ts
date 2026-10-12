// Content model — verbs, slots, recipes, prototypes, triggers (spec §6).
// Authored as plain data with string queries/predicates/outcomes; `compileContent`
// parses them once and fails loudly at load time.
import type { Outcome } from './sentence.js'
import type { Pred } from './predicate.js'
import { lintPred, parsePredicate } from './predicate.js'
import { parseQuery, type Query } from './query.js'
import { parseOutcomes } from './parse.js'
import type { Dictionary } from './dict.js'
import type { Aspects, Attrs } from './types.js'

export type SlotRole = 'actors' | 'object' | 'with' | 'at' | 'about' | 'to'

export interface SlotDef {
  role: SlotRole
  min: number
  max: number            // Infinity allowed
  query?: string
  shown?: boolean        // default true (§6.2)
}

export interface RecipeDef {
  id?: string
  when?: string          // undefined/'' → default recipe
  shown?: boolean        // gates default hidden (§6.2)
  hint?: string
  out: string[]          // outcome strings, parsed at load
}

export interface VerbContent {
  slots: SlotDef[]
  duration: number       // minutes, consumed automatically on success (§9.12)
  recipes: RecipeDef[]   // first match wins; a `when`-less recipe is the default
}

/** A clock trigger: fires on every crossing of its halfhour-of-day slot
 *  (0..47; `at hour N` in the script is slot 2N), or on the first crossing
 *  only when `once` (§9.29). */
export interface SlotTriggerDef {
  id: string
  slot: number
  once?: boolean
  out: string[]
}

/** An expiry trigger (`on expire of NAME`, §6.6): fires when an entity with
 *  this NAME expires (its `expiresAt` crosses), replacing the built-in
 *  destroy. No compile-time existence check — the entity may be spawned at
 *  runtime; names are unique among the living (§9.14). */
export interface ExpireTriggerDef {
  id: string
  onExpireOf: string
  out: string[]
}

export type TriggerDef = SlotTriggerDef | ExpireTriggerDef

/** Prototype-parameterized spawn, e.g. CLUE(secret-2) — resolved at execution
 *  into a concrete SPAWN ... AS #id (§6.4, §9.15). The registry key is the name. */
export interface PrototypeDef {
  kind: string
  /** §9.38: mints from this prototype may share their name with living copies */
  stackable?: boolean
  aspects?: Aspects
  attrs?: Attrs
  derive?: {
    nameFrom?: 'object'                       // minted name: `${name}_${object.name}`
    aspects?: Record<string, { fromObjectAspect: string; minus?: number }>
  }
}

/** A story ending (§9.35, §10): authored in the `endings:` section as
 *  `ID: "Title" (flavour)`. The flavour is a display word ('melancholy'
 *  default, 'grand' the other canonical value); it is open vocabulary —
 *  any lowercase word — so authors are not fenced. */
export interface EndingDef {
  title: string
  flavour?: string
}

export interface ContentRaw {
  verbs: Record<string, VerbContent>
  triggers?: TriggerDef[]
  prototypes?: Record<string, PrototypeDef>
  templates?: Record<string, string>  // prose templates per verb (§5.3)
  endings?: Record<string, EndingDef> // the ending table (§9.35)
}

export interface CompiledRecipe {
  id: string
  when: Pred | null                  // null → default
  shown: boolean
  hint?: string
  out: Outcome[]
}

export interface CompiledSlot {
  role: SlotRole
  min: number
  max: number
  shown: boolean
  query?: Query
}

export interface CompiledVerb {
  name: string
  slots: CompiledSlot[]
  slotByRole: Map<SlotRole, CompiledSlot>
  duration: number
  recipes: CompiledRecipe[]
}

export interface CompiledEnding {
  title: string
  flavour: string       // resolved: defaults to 'melancholy' (§10)
}

export interface CompiledContent {
  verbs: Map<string, CompiledVerb>
  triggers: (
    | { id: string; slot: number; once: boolean; out: Outcome[] }
    | { id: string; onExpireOf: string; out: Outcome[] }
  )[]
  prototypes: Map<string, PrototypeDef>
  templates: Map<string, string>
  endings: Map<string, CompiledEnding>
  /** §9.38 stackability, derived from `stackable` declarations at load:
   *  stackableNames = declared stackable prototype/entity names;
   *  stackableKinds = the KINDS of those declarations (a `kind:K` mint is
   *  stackable when its kind carries a stackable declaration). */
  stackableNames: Set<string>
  stackableKinds: Set<string>
}

export function compileContent(raw: ContentRaw, dict: Dictionary): CompiledContent {
  // §9.38: two derived sets every mint site reads — declared stackable names,
  // and the kinds of the stackable declarations (kind-qualified spawns)
  const stackableNames = new Set<string>()
  const stackableKinds = new Set<string>()
  for (const [name, p] of Object.entries(raw.prototypes ?? {})) {
    if (p.stackable) { stackableNames.add(name); stackableKinds.add(p.kind) }
  }
  const endings = new Map<string, CompiledEnding>()
  for (const [id, e] of Object.entries(raw.endings ?? {})) {
    if (!/^[A-Z][A-Z0-9_]*$/.test(id)) throw new Error(`ending '${id}' must be UPPER_SNAKE`)
    if (!e.title || !e.title.trim()) throw new Error(`ending ${id}: needs a title`)
    const flavour = e.flavour ?? 'melancholy'
    if (!/^[a-z]+$/.test(flavour)) throw new Error(`ending ${id}: flavour must be a lowercase word, got '${flavour}'`)
    endings.set(id, { title: e.title, flavour })
  }
  const verbs = new Map<string, CompiledVerb>()
  for (const [name, v] of Object.entries(raw.verbs)) {
    if (!dict.isVerb(name)) throw new Error(`verb '${name}' is not registered in the dictionary`)
    const slotByRole = new Map<SlotRole, CompiledSlot>()
    const slots: CompiledSlot[] = v.slots.map(s => {
      if (s.max < s.min) throw new Error(`verb ${name}: slot ${s.role} has max < min`)
      const compiled: CompiledSlot = { role: s.role, min: s.min, max: s.max, shown: s.shown ?? true, query: s.query ? parseQuery(s.query) : undefined }
      slotByRole.set(s.role, compiled)
      return compiled
    })
    const recipes: CompiledRecipe[] = v.recipes.map((r, i) => ({
      id: r.id ?? (r.when ? `${name.toLowerCase()}.recipe${i}` : `${name.toLowerCase()}.default`),
      when: r.when ? parsePredicate(r.when) : null,
      shown: r.shown ?? false,
      hint: r.hint,
      out: r.out.flatMap(o => parseOutcomeString(o, dict)),
    }))
    // §9.38 load-time lint: count()/total() args must be declared roles or queries
    for (const r of recipes) if (r.when) lintPred(`recipe ${r.id}`, r.when)
    verbs.set(name, { name, slots, slotByRole, duration: v.duration, recipes })
  }
  const triggers = (raw.triggers ?? []).map(t => {
    const out = t.out.flatMap(o => parseOutcomeString(o, dict))
    // §6.7: PROGRESS narrates a processing window; events have none
    if (out.some(o => o.op === 'PROGRESS')) throw new Error(`trigger ${t.id}: PROGRESS belongs to verb recipes, not events`)
    // §9.34: REFUSE aborts a step (§6.5); events have no step to refuse — and
    // the old silent behavior diverged play from fold
    if (out.some(o => o.op === 'REFUSE')) throw new Error(`trigger ${t.id}: REFUSE belongs to verb recipes — events have no step to refuse`)
    if ('onExpireOf' in t) return { id: t.id, onExpireOf: t.onExpireOf, out }
    return { id: t.id, slot: t.slot, once: t.once ?? false, out }
  })
  const prototypes = new Map(Object.entries(raw.prototypes ?? {}))
  const templates = new Map(Object.entries(raw.templates ?? {}))
  // §9.35: an ENDING outcome must name a declared ending — recipe or trigger,
  // checked once at load so a typo cannot wait for playtime
  for (const v of verbs.values()) {
    for (const r of v.recipes) checkEndingRefs(`recipe ${r.id}`, r.out, endings)
  }
  for (const t of triggers) checkEndingRefs(`trigger ${t.id}`, t.out, endings)
  return { verbs, triggers, prototypes, templates, endings, stackableNames, stackableKinds }
}

function checkEndingRefs(where: string, out: Outcome[], endings: Map<string, CompiledEnding>): void {
  for (const o of out) {
    if (o.op !== 'ENDING') continue
    const a = o.args[0]
    const id = a && a.t === 'ref' && a.ref.form === 'name' ? a.ref.name : null
    if (!id || !endings.has(id)) throw new Error(`${where}: ENDING '${id ?? '?'}' is not in the endings table`)
  }
}

function parseOutcomeString(src: string, dict: Dictionary): Outcome[] {
  // allow several outcomes in one string; usually one per array element
  return parseOutcomes(src, dict)
}
