// Content model — verbs, slots, recipes, prototypes, triggers (spec §6).
// Authored as plain data with string queries/predicates/outcomes; `compileContent`
// parses them once and fails loudly at load time.
import type { Outcome } from './sentence.js'
import type { Pred } from './predicate.js'
import { lintPred, lintRollMayBind, lintTriggerPred, lintTriggerValExpr, lintValExpr, parsePredicate } from './predicate.js'
import { parseQuery, type Query } from './query.js'
import { MAX_FN_DEPTH, collectFnCalls, expandFnCalls, parseFunctionBody, parseOutcomes, type FnTable } from './parse.js'
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

/** A function declaration (§9.40, §10): `NAME(params…) -> OUTCOMES` in the
 *  `functions:` section — a parametric outcome MACRO. Compiled to a template
 *  before verbs and triggers parse; call sites expand at load time, so the
 *  function table itself never reaches CompiledContent. */
export interface FunctionDef {
  name: string
  params: string[]     // [] for a parameterless function
  body: string         // raw outcome-list source after `->`
}

export interface ContentRaw {
  /** §9.41: the dice seed — the whole script's random sequence is a pure
   *  function of (seed, counter); default 0. Meta line `seed: N` (§10). */
  seed?: number
  verbs: Record<string, VerbContent>
  triggers?: TriggerDef[]
  prototypes?: Record<string, PrototypeDef>
  templates?: Record<string, string>  // prose templates per verb (§5.3)
  endings?: Record<string, EndingDef> // the ending table (§9.35)
  functions?: FunctionDef[]           // parametric outcome macros (§9.40) — declaration order matters for error reports only
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
  /** §9.41: the dice seed — every die is `f(seed, counter)`; the run's dice
   *  sequence is fixed at load and replay is counter-based (no re-derivation) */
  seed: number
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
  // §9.40 FIRST, before any verb/trigger body parses: build the function
  // table (declaration checks, body templates, recursion/depth validation) —
  // call sites in verb and trigger bodies dispatch against it and expand
  // immediately after their parse, so the guards below walk expanded lists.
  const fns = buildFnTable(raw.functions ?? [], dict)
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
      out: r.out.flatMap(o => parseOutcomeString(o, dict, fns)),
    }))
    // §9.38 load-time lint: count()/total() args must be declared roles or queries —
    // over every `when` AND over every IF condition authored in the recipe bodies
    for (const r of recipes) {
      if (r.when) lintPred(`recipe ${r.id}`, r.when)
      forEachIfCondition(r.out, w => lintPred(`recipe ${r.id}`, w))
      lintDice(`recipe ${r.id}`, r.out)
    }
    verbs.set(name, { name, slots, slotByRole, duration: v.duration, recipes })
  }
  const triggers = (raw.triggers ?? []).map(t => {
    // §9.40: expansion (inside parseOutcomeString) has already run — these
    // guards therefore see function-expanded bodies exactly like literal ones
    const out = t.out.flatMap(o => parseOutcomeString(o, dict, fns))
    // §6.7/§9.34 guards — recursive over IF branch bodies (§9.39), so nothing
    // escapes by nesting. F3's function expansion runs BEFORE these guards.
    forEachAuthoredOutcome(out, o => {
      // §6.7: PROGRESS narrates a processing window; events have none
      if (o.op === 'PROGRESS') throw new Error(`trigger ${t.id}: PROGRESS belongs to verb recipes, not events`)
      // §9.34: REFUSE aborts a step (§6.5); events have no step to refuse — and
      // the old silent behavior diverged play from fold
      if (o.op === 'REFUSE') throw new Error(`trigger ${t.id}: REFUSE belongs to verb recipes — events have no step to refuse`)
    })
    // §9.39: event bindings are empty, so a role-dependent operand in a trigger
    // IF condition would silently read false/0 — load-time error naming it
    forEachIfCondition(out, w => {
      lintPred(`trigger ${t.id}`, w) // the count(DREAD)-typo lint applies here too
      lintTriggerPred(`trigger ${t.id}`, w)
    })
    // §9.41: trigger ROLL arguments must be role-FREE — same empty-bindings
    // argument as the IF lint, applied to the dice expression itself
    forEachAuthoredOutcome(out, o => {
      if (o.op === 'ROLL') {
        for (const a of o.args) if (a.t === 'vexpr') lintTriggerValExpr(`trigger ${t.id}`, a.v, 'ROLL argument')
      }
    })
    lintDice(`trigger ${t.id}`, out)
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
  return { seed: raw.seed ?? 0, verbs, triggers, prototypes, templates, endings, stackableNames, stackableKinds }
}

/**
 * Visit every outcome in an authored list, RECURSING into IF branch bodies
 * (§9.39) — the single entry point for the load-time guards (trigger
 * PROGRESS/REFUSE, checkEndingRefs, the IF-condition lints), so nothing
 * escapes by nesting. Ordering with F3: function expansion happens BEFORE
 * these guards run, so expanded bodies walk like literal ones.
 */
export function forEachAuthoredOutcome(out: Outcome[], visit: (o: Outcome) => void): void {
  for (const o of out) {
    visit(o)
    if (o.op === 'IF') {
      for (const arm of o.arms ?? []) forEachAuthoredOutcome(arm.then, visit)
      if (o.fallback) forEachAuthoredOutcome(o.fallback, visit)
    }
  }
}

/** visit every IF condition in an authored list (branch bodies included) */
function forEachIfCondition(out: Outcome[], visit: (when: Pred) => void): void {
  forEachAuthoredOutcome(out, o => {
    if (o.op === 'IF') for (const arm of o.arms ?? []) visit(arm.when)
  })
}

/**
 * §9.41 dice lints, shared by recipe and trigger outcome lists — all run
 * AFTER function expansion, like every load guard (expanded bodies are
 * guarded exactly like literal ones):
 *  - `ROLLED` is engine-written; an authored one forges dice history
 *    (paren-form-error precedent: the parse entry exists so concrete rows
 *    re-parse, and this lint is what keeps it out of scripts).
 *  - `{roll}` in PROGRESS/REFUSE strings is a load error: those strings are
 *    stored raw and rendered without an execution pass, so the token would
 *    bake into the journal as literal text. NARRATE is the one legal home —
 *    the engine rewrites it at execution.
 *  - ROLL arguments get the recipe-context role lint (count(DREAD)-style
 *    typos would silently roll 0 dice); the trigger-context role-FREE lint
 *    is the caller's (it needs the trigger's stricter rule).
 *  - the `roll` may-bind analysis over the whole list (best-effort,
 *    false negatives allowed — the runtime rejects whatever it misses).
 */
function lintDice(where: string, out: Outcome[]): void {
  forEachAuthoredOutcome(out, o => {
    if (o.op === 'ROLLED') {
      throw new Error(`${where}: ROLLED is engine-written — author ROLL and the log records the concrete result (§9.41)`)
    }
    if (o.op === 'PROGRESS' || o.op === 'REFUSE') {
      const s = o.args.find(a => a.t === 'str') // PROGRESS carries (dur, string); REFUSE (string)
      if (s && s.t === 'str' && s.v.includes('{roll}')) {
        throw new Error(`${where}: {roll} is only available in NARRATE strings — ${o.op} prose is load-fixed (§9.41)`)
      }
    }
    if (o.op === 'ROLL') {
      for (const a of o.args) if (a.t === 'vexpr') lintValExpr(`${where}`, a.v)
    }
  })
  lintRollMayBind(where, out)
}

function checkEndingRefs(where: string, out: Outcome[], endings: Map<string, CompiledEnding>): void {
  // §9.39: recurses into IF branch bodies — an unknown ending hidden in an
  // untaken branch is still a load error
  forEachAuthoredOutcome(out, o => {
    if (o.op !== 'ENDING') return
    const a = o.args[0]
    const id = a && a.t === 'ref' && a.ref.form === 'name' ? a.ref.name : null
    if (!id || !endings.has(id)) throw new Error(`${where}: ENDING '${id ?? '?'}' is not in the endings table`)
  })
}

function parseOutcomeString(src: string, dict: Dictionary, fns: FnTable): Outcome[] {
  // allow several outcomes in one string; usually one per array element.
  // §9.40: with a function table, parse dispatches NAME( calls and the
  // expansion pass runs in the same step — verb/trigger bodies are compiled
  // fully expanded, so every guard below (and the engine) sees plain outcomes.
  const out = parseOutcomes(src, dict, fns)
  return fns.byName.size > 0 ? expandFnCalls(out, fns) : out
}

// ---------- functions: the compile-time macro table (§9.40) ----------

const FN_IDENT = /^[A-Za-z_][A-Za-z0-9_]*$/ // function names and parameters

/**
 * Build the function table — functions parse BEFORE any verb/trigger body
 * (§9.40), in three ordered passes:
 *  1. declarations: name/param shape, op-name collisions (`dict.isOp` — a
 *     function and an outcome op share the caller's vocabulary, so a
 *     collision is a load error), IF/ELSE (undispatchable at the top of
 *     `outcome()`), duplicates. No entity/verb name constraint: calls only
 *     ever parse in outcome position, so a function may share a name with an
 *     entity or a verb.
 *  2. bodies: each body parses to a TEMPLATE against the table's names —
 *     nested calls become FN nodes (forward references fine), bare params
 *     become placeholders. Arity of every call is checked here, a load error.
 *  3. the call graph: colored DFS rejects recursion naming the cycle; the
 *     same graph's depth (1 + deepest callee chain) rejects nesting beyond
 *     MAX_FN_DEPTH. Both are load errors even for never-called functions.
 */
function buildFnTable(defs: FunctionDef[], dict: Dictionary): FnTable {
  const table: FnTable = { byName: new Map() }
  // pass 1 — declarations
  for (const def of defs) {
    if (!FN_IDENT.test(def.name)) {
      throw new Error(`function name '${def.name}' must be an identifier (letters, digits, _)`)
    }
    if (dict.isOp(def.name)) {
      throw new Error(
        `function '${def.name}' collides with an outcome op — functions and ops share one ` +
        `caller vocabulary; rename the function`,
      )
    }
    if (def.name === 'IF' || def.name === 'ELSE') {
      throw new Error(`function '${def.name}' cannot be named a conditional keyword — IF/ELSE dispatch before functions in outcome position`)
    }
    if (table.byName.has(def.name)) throw new Error(`duplicate function '${def.name}'`)
    for (const p of def.params) {
      if (!FN_IDENT.test(p)) throw new Error(`function '${def.name}': parameter '${p}' must be an identifier`)
    }
    const dups = def.params.filter((p, i) => def.params.indexOf(p) !== i)
    if (dups.length > 0) throw new Error(`function '${def.name}': duplicate parameter(s) ${[...new Set(dups)].join(', ')}`)
    table.byName.set(def.name, { name: def.name, params: def.params, body: [] })
  }
  // pass 2 — bodies (nested-call arity checks against the declared params)
  for (const def of defs) {
    const tpl = table.byName.get(def.name)!
    try {
      tpl.body = parseFunctionBody(def.body, dict, table, def.params)
    } catch (err) {
      throw new Error(`function '${def.name}': ${err instanceof Error ? err.message : String(err)}`)
    }
  }
  // pass 3 — the call graph: recursion cycles, then the depth cap
  const callsOf = new Map<string, Set<string>>()
  for (const tpl of table.byName.values()) {
    const calls = new Set<string>()
    collectFnCalls(tpl.body, calls)
    for (const c of calls) {
      if (!table.byName.has(c)) throw new Error(`function '${tpl.name}': call to undeclared function '${c}'`)
    }
    callsOf.set(tpl.name, calls)
  }
  const WHITE = 0, GRAY = 1, BLACK = 2
  const color = new Map<string, number>()
  const stack: string[] = []
  const visit = (name: string): void => {
    color.set(name, GRAY)
    stack.push(name)
    for (const c of callsOf.get(name)!) {
      const col = color.get(c) ?? WHITE
      if (col === GRAY) {
        const cycle = [...stack.slice(stack.indexOf(c)), c]
        throw new Error(
          `functions recurse: ${cycle.join(' -> ')} — calls expand at load time, ` +
          `so a recursion cycle never terminates; rewrite without the cycle`,
        )
      }
      if (col === WHITE) visit(c)
    }
    stack.pop()
    color.set(name, BLACK)
  }
  for (const name of table.byName.keys()) {
    if ((color.get(name) ?? WHITE) === WHITE) visit(name)
  }
  const depth = new Map<string, number>()
  const depthOf = (name: string): number => {
    const memo = depth.get(name)
    if (memo !== undefined) return memo
    depth.set(name, 1) // safe: the graph is acyclic after the cycle check
    let deepest = 0
    for (const c of callsOf.get(name)!) deepest = Math.max(deepest, depthOf(c))
    const v = 1 + deepest
    depth.set(name, v)
    return v
  }
  for (const name of table.byName.keys()) {
    const d = depthOf(name)
    if (d > MAX_FN_DEPTH) {
      throw new Error(`function '${name}' sits ${d} calls deep — the nesting cap is ${MAX_FN_DEPTH}`)
    }
  }
  return table
}
