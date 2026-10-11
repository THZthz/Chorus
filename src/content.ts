// Content model — verbs, slots, recipes, prototypes, triggers (spec §6).
// Authored as plain data with string queries/predicates/outcomes; `compileContent`
// parses them once and fails loudly at load time.
import type { Outcome } from './sentence.js'
import type { Pred } from './predicate.js'
import { parsePredicate } from './predicate.js'
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

export interface TriggerDef {
  id: string
  hour: number           // hour of day at which the trigger fires on crossing
  once?: boolean         // fires on its first crossing only (§9.29)
  out: string[]
}

/** Prototype-parameterized spawn, e.g. CLUE(secret-2) — resolved at execution
 *  into a concrete SPAWN ... AS #id (§6.4, §9.15). The registry key is the name. */
export interface PrototypeDef {
  kind: string
  aspects?: Aspects
  attrs?: Attrs
  derive?: {
    nameFrom?: 'object'                       // minted name: `${name}_${object.name}`
    aspects?: Record<string, { fromObjectAspect: string; minus?: number }>
  }
}

export interface ContentRaw {
  verbs: Record<string, VerbContent>
  triggers?: TriggerDef[]
  prototypes?: Record<string, PrototypeDef>
  templates?: Record<string, string>  // prose templates per verb (§5.3)
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

export interface CompiledContent {
  verbs: Map<string, CompiledVerb>
  triggers: { id: string; hour: number; once: boolean; out: Outcome[] }[]
  prototypes: Map<string, PrototypeDef>
  templates: Map<string, string>
}

export function compileContent(raw: ContentRaw, dict: Dictionary): CompiledContent {
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
    verbs.set(name, { name, slots, slotByRole, duration: v.duration, recipes })
  }
  const triggers = (raw.triggers ?? []).map(t => {
    const out = t.out.flatMap(o => parseOutcomeString(o, dict))
    // §6.7: PROGRESS narrates a processing window; events have none
    if (out.some(o => o.op === 'PROGRESS')) throw new Error(`trigger ${t.id}: PROGRESS belongs to verb recipes, not events`)
    return { id: t.id, hour: t.hour, once: t.once ?? false, out }
  })
  const prototypes = new Map(Object.entries(raw.prototypes ?? {}))
  const templates = new Map(Object.entries(raw.templates ?? {}))
  return { verbs, triggers, prototypes, templates }
}

function parseOutcomeString(src: string, dict: Dictionary): Outcome[] {
  // allow several outcomes in one string; usually one per array element
  return parseOutcomes(src, dict)
}
