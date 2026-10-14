// The interpreter — spec §6.6 (validation → selection → application → time →
// events) with step-atomic REFUSE (§6.5) and event-sourced records (§7).
import type { Arg, Clause, Outcome, Role, Sentence } from '../core/sentence.js'
import { ROLES } from '../core/sentence.js'
import type { Ref } from '../core/ref.js'
import { idRef, nameRef, sortRefs } from '../core/ref.js'
import { formatSentence } from '../core/format.js'
import type { Dictionary } from '../core/dict.js'
import type { CompiledContent, CompiledRecipe, CompiledSlot, CompiledVerb, CompiledDeck, DeckCard } from '../content/content.js'
import { drawFrom } from '../content/content.js'
import { evalPred, evalExpr, type PredCtx, type RoleBindings } from '../core/predicate.js'
import { rollDie } from '../core/dice.js'
import { matchQuery } from '../core/query.js'
import { MIN_PER_HALFHOUR, absMin, clockFromAbsMin, formatClock, parseClock, parseDuration, type Clock } from '../core/clock.js'
import { parseSentence } from '../core/parse.js'
import {
  addEntity, aspectEntry, aspectGet, aspectSet, cloneState, getAlive, oldestNamed, relationKey, releaseName,
  type Aspects, type Entity, type OpenWindow, type State,
} from '../core/types.js'

export interface Step {
  /** begin at this absolute minute (v0.5, minute-precise — wins over clock) */
  startMin?: number
  /** quantized begin clock — the input syntax; the default begin is now */
  clock?: Clock | null
  clauses: Clause[]
  tReal?: string
}

export interface ActionRecord {
  id: number
  kind: 'action' | 'event' | 'complete' | 'wait'
  trigger: string | null
  tGame: string          // display clock "day:halfhour" (floored — display only)
  tReal: string | null
  advancedMin: number    // BEGIN: window length; wait: the span; else 0
  /** v0.5 — exact minutes: BEGIN = begin minute, wait = span start. The
   *  quantized tGame cannot carry mid-halfhour minutes, and fold must land
   *  them exactly (expires anchoring, §9.46) */
  startMin: number | null
  /** v0.5 — COMPLETE rows only: the window key they close */
  window: string | null
  actors: Ref[]          // sorted union across clauses
  verb: string           // first clause's verb ('' for events/waits; the verb of a COMPLETE row)
  objects: Ref[]
  instruments: Ref[]
  at: Ref | null
  about: Ref | null
  to: Ref | null
  outcomes: Outcome[]    // concrete: ids minted, sugar resolved (§9.15)
  recipes: string[]      // BEGIN: fired recipe ids, clause order; COMPLETE: the one recipe
  sentence: string       // canonical, lossless
}

export interface ExecResult {
  records: ActionRecord[]
  state: State           // unchanged when rejected or refused
  rejected: string | null
}

export interface ExecOptions {
  firstRecordId: number
  idGen?: () => string
}

// ---------- role binding & slot validation (§6.1) ----------

interface Prepared {
  clause: Clause
  verb: CompiledVerb
  refs: Record<Role, Ref[]>
  bindings: RoleBindings      // resolved against the INPUT state (validation & selection)
  recipe: CompiledRecipe
}

function clauseRefs(c: Clause): Record<Role, Ref[]> {
  return {
    actors: c.actors, object: c.objects, with: c.with,
    at: c.at ? [c.at] : [], about: c.about ? [c.about] : [], to: c.to ? [c.to] : [],
  }
}

function validateSlots(verb: CompiledVerb, refs: Record<Role, Ref[]>, state: State): { bindings: RoleBindings } | { error: string } {
  const bindings: RoleBindings = {}
  for (const role of ROLES) {
    const provided = refs[role]
    const slot: CompiledSlot | undefined = verb.slotByRole.get(role)
    if (!slot) {
      if (provided.length > 0) return { error: `${verb.name} does not accept '${role}'` }
      bindings[role] = []
      continue
    }
    if (provided.length < slot.min || provided.length > slot.max) {
      const range = slot.max === Infinity ? `${slot.min}..` : `${slot.min}..${slot.max}`
      return { error: `${verb.name}: '${role}' takes ${range}, got ${provided.length}` }
    }
    const entities: Entity[] = []
    const placed = new Set<string>()
    for (const ref of provided) {
      const e = resolveRef(state, ref)
      if (!e) return { error: `unresolvable ref ${show(ref)}` }
      if (placed.has(e.id)) return { error: `${show(ref)} is already placed in '${role}'` }
      placed.add(e.id)
      if (slot.query && !matchQuery(e, slot.query)) {
        return { error: `${show(ref)} does not fit slot '${role}' of ${verb.name}` }
      }
      entities.push(e)
    }
    bindings[role] = entities
  }
  return { bindings }
}

export function resolveRef(state: State, ref: Ref): Entity | null {
  if (ref.form === 'id') return getAlive(state, ref.id) ?? null
  // §9.40: expansion removes every parameter before content compiles — a
  // param ref here means an expansion bug, not a resolution failure
  if (ref.form === 'param') throw new Error('internal: a function parameter ref survived expansion')
  // §9.38 pool addressing: a name held by N > 1 living copies addresses the
  // OLDEST copy — the HEAD of the byName pool (insertion order, never id sort)
  const e = oldestNamed(state, ref.name)
  if (!e) return null
  if (ref.kind && e.kind !== ref.kind) return null
  return e
}

function show(r: Ref): string {
  return r.form === 'id' ? `#${r.id}` : r.name
}

// ---------- recipe selection (§6.4): first match wins ----------

function selectRecipe(verb: CompiledVerb, bindings: RoleBindings, state: State): CompiledRecipe | { error: string } {
  const ctx: PredCtx = { b: bindings, state }
  for (const r of verb.recipes) {
    if (r.when === null) return r
    if (evalPred(r.when, ctx)) return r
  }
  return { error: `${verb.name}: no recipe matched` }
}

// ---------- execution (§6.6) ----------

export function executeStep(
  content: CompiledContent,
  dict: Dictionary,
  stateIn: State,
  step: Step,
  opts: ExecOptions,
): ExecResult {
  // game-over (§9.35): once the story has ended nothing further executes, and
  // nothing is logged — undo past the ending record reopens the game (it
  // derives from the fold, so no special-case is needed)
  if (stateIn.ended) {
    return rejected(`the story has ended — ${stateIn.ended.title}`, stateIn)
  }
  // v0.5: the begin minute — startMin wins (a mid-halfhour begin is
  // representable), the quantized clock field is the input syntax
  const startMin = step.startMin ?? absMin(step.clock ?? clockFromAbsMin(stateIn.now))
  // monotonic mechanics time (§9.5): a step may never begin before now
  if (startMin < stateIn.now) {
    const now = clockFromAbsMin(stateIn.now)
    return rejected(`clock ${formatClock(clockFromAbsMin(startMin))} is before state.now (${now.day}:${now.halfhour})`, stateIn)
  }
  if (step.clauses.length === 0) {
    const dueNow = [...stateIn.windows.values()].some(w => w.end <= startMin)
    if (startMin === stateIn.now && !dueNow) {
      const now = clockFromAbsMin(stateIn.now)
      return rejected(`nothing to do — the clock already reads ${now.day}:${now.halfhour} (commit an action, or name a later minute to wait)`, stateIn)
    }
  }

  // Phase 1 — the advance (v0.5): everything due at or before the begin
  // minute resolves first — due windows in (end, beginId, clause) order, slot
  // events at each crossed halfhour, completions before same-minute events.
  // Atomic with the whole step: any rejection below discards the pass —
  // nothing logged, clock unmoved (§9.19).
  let mint = 0
  const idGen = opts.idGen ?? (() => `e${opts.firstRecordId}_${++mint}`) // deterministic default (§9.3)
  let draft = cloneState(stateIn)
  let advance: ActionRecord[] = []
  try {
    const pass = resolveDue(content, dict, draft, startMin, opts.firstRecordId, idGen)
    draft = pass.state
    advance = pass.records
  } catch (err) {
    return rejected(err instanceof Error ? err.message : String(err), stateIn)
  }
  const nextId = opts.firstRecordId + advance.length

  // a bare clock step is a pure advance — its wait row already moved the clock
  if (step.clauses.length === 0) {
    return { records: advance, state: draft, rejected: null }
  }

  // Phase 2 — validation, busy checks, selection, all against the
  // POST-ADVANCE draft: locks release when their windows close during the
  // pass, so a card freeing exactly at the begin minute is usable (§6.1/§6.4)
  const prepared: Prepared[] = []
  for (const clause of step.clauses) {
    const verb = content.verbs.get(clause.verb)
    if (!verb) return rejected(`${clause.verb} is not a content verb`, stateIn)
    const refs = clauseRefs(clause)
    const slots = validateSlots(verb, refs, draft)
    if ('error' in slots) return rejected(`${clause.verb}: ${slots.error}`, stateIn)
    const busy = busyReason(draft, clause.verb, refs)
    if (busy) return rejected(busy, stateIn)
    const recipe = selectRecipe(verb, slots.bindings, draft)
    if ('error' in recipe) return rejected(recipe.error, stateIn)
    prepared.push({ clause, verb, refs, bindings: slots.bindings, recipe })
  }

  // Phase 3 — the top-level REFUSE pre-pass (v0.5): authored gates reject AT
  // THE TABLE, not after the window. The trial runs on a scratch draft; only
  // a REFUSE is decisive here — an application ERROR defers to completion
  // (the state will have changed by then; the window refuses there, the
  // completion analog of §9.19). The trial's dice and spawns die with it.
  const scratch = cloneState(draft)
  let smint = 0
  const sctx: ApplyCtx = {
    state: scratch, dict, content,
    bindings: {} as RoleBindings,
    now: startMin,
    recordId: nextId,
    idGen: () => `s${nextId}_${++smint}`,
    extraAdvanceMin: 0,
  }
  let refused = false
  const collected: Outcome[] = []
  try {
    outer:
    for (const p of prepared) {
      // §9.41: the dice binding is PER CLAUSE — a roll in clause one is not
      // visible to clause two's conditions
      sctx.roll = undefined
      const rebound: RoleBindings = {}
      for (const role of ROLES) {
        rebound[role] = p.refs[role]
          .map(ref => resolveRef(scratch, ref))
          .filter((e): e is Entity => e !== null)
      }
      sctx.bindings = rebound
      for (const outcome of p.recipe.out) {
        // an IF is opaque to the gate check: its branches — including any
        // REFUSE inside them — belong to the completion, where the world the
        // window imagined actually exists (§9.39 v0.5)
        if (outcome.op === 'IF') continue
        const res = applyOutcome(outcome, sctx)
        collected.push(...res.concrete)
        if (res.refused) { refused = true; break outer }
      }
    }
  } catch {
    refused = false // an error is not a gate — it defers to completion time
    collected.length = 0
  }
  if (refused) {
    // refused BEGIN: attempt logged, zero time, the advance discarded with it
    const record = makeBeginRecord(opts.firstRecordId, startMin, step, prepared, collected, dict)
    record.advancedMin = 0 // §6.5: a refusal collapses the cycle — no window elapses
    return { records: [record], state: stateIn, rejected: null }
  }

  // Phase 4 — BEGIN (v0.5): one row per step, one window per clause. No
  // outcome applies here: the row carries the PROGRESS ticks (journal), the
  // commit-time recipe ids, and the window table does the locking. The
  // recipe's real outcomes land on the COMPLETE row at the window's close.
  const begin = makeBeginRecord(nextId, startMin, step, prepared, prepared.flatMap(p => p.recipe.out).filter(o => o.op === 'PROGRESS'), dict)
  registerWindows(begin, draft, dict, content)
  return { records: [...advance, begin], state: draft, rejected: null }
}

/** roles a card is COMMITTED to — the actors stay free (the player is never
 *  locked, or no two verbs could ever run in parallel) */
const COMMITTED_ROLES: Role[] = ['object', 'with', 'at', 'about', 'to']

/** §6.7 v0.5 lock checks, against the post-advance draft: one window per
 *  verb station; committed cards busy while any open window holds them */
function busyReason(state: State, verb: string, refs: Record<Role, Ref[]>): string | null {
  for (const w of state.windows.values()) {
    if (w.verb === verb) {
      const end = clockFromAbsMin(w.end)
      return `${verb} is already processing — its station frees at ${end.day}:${end.halfhour}`
    }
  }
  for (const role of COMMITTED_ROLES) {
    for (const ref of refs[role]) {
      const e = resolveRef(state, ref)
      if (!e) continue
      for (const w of state.windows.values()) {
        if (w.locked.includes(e.id)) {
          const end = clockFromAbsMin(w.end)
          return `${e.name.replace(/_/g, ' ')} is busy with ${w.verb} — it frees at ${end.day}:${end.halfhour}`
        }
      }
    }
  }
  return null
}

function makeBeginRecord(
  id: number,
  startMin: number,
  step: Step,
  prepared: Prepared[],
  outcomes: Outcome[],
  dict: Dictionary,
): ActionRecord {
  const advancedMin = prepared.reduce((s, p) => s + p.verb.duration, 0)
  const first = prepared[0]
  const seen = new Set<string>()
  const record: ActionRecord = {
    id,
    kind: 'action',
    trigger: null,
    tGame: formatClock(clockFromAbsMin(startMin)),
    tReal: step.tReal ?? null,
    advancedMin,               // the window length — NOT a clock advance (v0.5)
    startMin,                  // exact begin minute (fold lands it exactly)
    window: null,
    actors: sortRefs(prepared.flatMap(p => p.clause.actors)).filter(r => {
      const k = r.form === 'id' ? `#${r.id}` : r.name
      if (seen.has(k)) return false
      seen.add(k)
      return true
    }),
    verb: first?.clause.verb ?? '',
    objects: first?.clause.objects ?? [],
    instruments: first?.clause.with ?? [],
    at: first?.clause.at ?? null,
    about: first?.clause.about ?? null,
    to: first?.clause.to ?? null,
    outcomes,
    recipes: prepared.map(p => p.recipe.id),
    sentence: '',
  }
  record.sentence = formatSentence(
    { kind: 'action', clock: clockFromAbsMin(startMin), trigger: null, clauses: step.clauses, outcomes },
    dict,
  )
  return record
}

/** register one window per BEGIN clause — the SAME code path play and fold
 *  run (the divergence guard): clauses come from the row's canonical sentence
 *  (law 2 makes that lossless), durations and recipe ids from content */
function registerWindows(record: ActionRecord, state: State, dict: Dictionary, content: CompiledContent): void {
  // a BEGIN row's clauses ride its canonical sentence (law 2 makes that
  // lossless); a degenerate row with no parseable clauses opens no window
  let clauses: Clause[] = []
  try { clauses = parseSentence(record.sentence, dict).clauses } catch { clauses = [] }
  const startMin = record.startMin ?? absMin(parseClock(record.tGame))
  let offset = 0
  clauses.forEach((clause, ci) => {
    const verb = content.verbs.get(clause.verb)
    if (!verb) throw new Error(`begin row ${record.id}: verb '${clause.verb}' is not in the compiled content — drift`)
    const refs = clauseRefs(clause)
    const locked: string[] = []
    for (const role of COMMITTED_ROLES) {
      for (const ref of refs[role]) {
        const e = resolveRef(state, ref)
        if (e) locked.push(e.id)
      }
    }
    state.windows.set(`${record.id}:${ci}`, {
      key: `${record.id}:${ci}`,
      beginId: record.id,
      clause: ci,
      verb: clause.verb,
      start: startMin + offset,
      end: startMin + offset + verb.duration,
      recipe: record.recipes[ci] ?? '',
      refs,
      locked,
    })
    offset += verb.duration
  })
}

function rejected(reason: string, state: State): ExecResult {
  return { records: [], state, rejected: reason }
}

/** role args → concrete id refs captured from the draft bindings (§9.15: the log never contains sugar) */
function concretize(args: Arg[], ctx: ApplyCtx): Arg[] {
  const out: Arg[] = []
  for (const a of args) {
    if (a.t === 'role') {
      for (const e of ctx.bindings[a.role] ?? []) out.push({ t: 'ref', ref: idRef(e.id) })
    } else {
      out.push(a)
    }
  }
  return out
}

/** §9.38: the mint's stackability from its RESOLVED name+kind — a NAME declared
 *  stackable, or a KIND carrying a stackable declaration. Concrete SPAWN rows
 *  always carry the resolved `kind:K`, so the fold recomputes from the same
 *  (name, kind) pair: play must decide from the resolved kind too, or a plain
 *  spawn of a non-stackable prototype in a stackable KIND would mint
 *  non-stackable at play but stackable at fold (snap would diverge). */
function mintIsStackable(content: CompiledContent, ref: { kind?: string; name: string }): boolean {
  return content.stackableNames.has(ref.name) || (ref.kind !== undefined && content.stackableKinds.has(ref.kind))
}

function idOrSelf(e: Entity | undefined, fallback: Ref | { role: Role }): Ref | { role: Role } {
  return e ? idRef(e.id) : fallback
}

// ---------- outcome application (§4.4) ----------

interface ApplyCtx {
  state: State
  dict: Dictionary
  content: CompiledContent
  bindings: RoleBindings
  now: number             // absolute minutes at application time
  recordId: number
  idGen: () => string
  extraAdvanceMin: number
  /** §9.41: the per-clause dice binding — the last ROLL of THIS clause's
   *  outcome list (last-wins, rebinds on a second ROLL). Lives on the apply
   *  context, never on State: a REFUSE discards the draft along with it, so
   *  a refused step's dice never moved the real counter. Reset per clause of
   *  a multi-clause step; a fresh event context starts unbound. Function
   *  expansion is compile-time, so expanded bodies read the caller's list
   *  binding naturally. */
  roll?: number
}

interface ApplyResult {
  concrete: Outcome[]    // what the log stores: ids minted, sugar resolved, deltas computed (§9.15, §9.31)
  refused: boolean
}

/**
 * §9.41 {roll} interpolation: a NARRATE string's `{roll}` token is rewritten
 * AT EXECUTION, so the concrete row carries the number — the journal never
 * re-derives dice. Runs inside applyOutcome, which places it exactly "after a
 * ROLL, for subsequent NARRATE args in the same list": the binding is
 * undefined until this list's ROLL applies, and last-wins on a second ROLL.
 * A NARRATE before the list's (only) ROLL keeps the literal token — nothing
 * has been rolled yet; a NARRATE in a REFUSED step is journal-visible with
 * its number even though the counter never moved (the draft died — the
 * documented, deterministic re-roll oddity). At fold the concrete rows no
 * longer contain `{roll}`, so the rewrite is a no-op there.
 */
function rewriteRollProse(o: Outcome, ctx: ApplyCtx): Outcome {
  const s = o.args[0]
  if (!s || s.t !== 'str' || ctx.roll === undefined || !s.v.includes('{roll}')) return o
  return { op: o.op, args: [{ t: 'str', v: s.v.replaceAll('{roll}', String(ctx.roll)) }] }
}

function applyOutcome(o: Outcome, ctx: ApplyCtx): ApplyResult {
  const args = o.args
  switch (o.op) {
    case 'IF':
      return applyIf(o, ctx)
    case 'SPAWN': {
      const refArg = args.find((a): a is Extract<Arg, { t: 'ref' }> => a.t === 'ref')
      if (!refArg || refArg.ref.form !== 'name') throw new Error('SPAWN needs a name ref (prototype or kind:NAME)')
      const protoName = refArg.ref.name
      const proto = ctx.content.prototypes.get(protoName)
      const kind = refArg.ref.kind ?? proto?.kind
      if (!kind) throw new Error(`SPAWN ${protoName}: no kind (prototype or kind:NAME required)`)
      let name = protoName
      let aspects: Aspects = proto?.aspects ? cloneAspects(proto.aspects) : {}
      const attrs: Record<string, string | number | boolean> = { ...(proto?.attrs ?? {}) }
      const derive = proto?.derive
      if (derive) {
        const obj = (ctx.bindings.object ?? [])[0]
        if (derive.nameFrom === 'object' && obj) name = `${name}_${obj.name}`
        if (derive.aspects && obj) {
          for (const [key, rule] of Object.entries(derive.aspects)) {
            const base = aspectGet(obj, rule.fromObjectAspect)
            if (base !== undefined) aspects[key] = base - (rule.minus ?? 0)
          }
        }
      }
      // §9.38: the flag comes from the mint's RESOLVED name+kind declaration
      // (not the authored ref's form — the concrete row always carries the
      // resolved kind, and the fold must recompute the same flag) and applies
      // to whatever name the mint ends up carrying (derived names stack when
      // the prototype is declared stackable)
      const isStack = mintIsStackable(ctx.content, { name: protoName, kind })
      // §9.14 + §9.38: a non-stackable mint demands a free name; a stackable
      // mint stacks with its own kind of pool but may not collide with a
      // distinct NON-stackable living name (shared namespace)
      const pool = ctx.state.byName.get(name)
      if (pool && pool.length > 0 && (!isStack || !pool.every(pid => ctx.state.entities.get(pid)?.stackable === true))) {
        throw new Error(`SPAWN: name '${name}' already in use`)
      }
      const asId = args.find((a): a is Extract<Arg, { t: 'asId' }> => a.t === 'asId')
      const id = asId ? asId.id : ctx.idGen()
      const into = args.find((a): a is Extract<Arg, { t: 'into' }> => a.t === 'into')
      let container: string | undefined
      if (into) {
        container = 'role' in into.ref ? ctx.bindings[into.ref.role]?.[0]?.id : resolveRef(ctx.state, into.ref)?.id
      } else {
        container = ctx.bindings.at?.[0]?.id ?? (ctx.bindings.actors?.[0]?.attrs['container'] as string | undefined)
      }
      if (container) attrs['container'] = container
      // fold-time reconstruction: a recorded SPAWN (asId present) may carry
      // materialized aspects/attrs as JSON pairs — prefer them over derive.
      // An explicit attrs= pair is authoritative (§9.18): it already carries
      // the mint's location (container and/or zone), so a mint with no
      // location must not grow one from the replay bindings.
      const aspectsArg = args.find(a => a.t === 'pair' && a.k === 'aspects')
      const attrsArg = args.find(a => a.t === 'pair' && a.k === 'attrs')
      if (aspectsArg && aspectsArg.t === 'pair' && typeof aspectsArg.v === 'string') {
        aspects = JSON.parse(aspectsArg.v) as Aspects
      }
      if (attrsArg && attrsArg.t === 'pair' && typeof attrsArg.v === 'string') {
        const materialized = JSON.parse(attrsArg.v) as Record<string, string | number | boolean>
        Object.assign(attrs, materialized)
        if (!('container' in materialized)) delete attrs['container']
      }
      addEntity(ctx.state, { id, kind, name, aspects, attrs, bornAction: ctx.recordId, deadAction: null, stackable: isStack })
      // concrete outcome: everything a fold needs, nothing from the step context
      const concrete: Arg[] = [{ t: 'ref', ref: nameRef(name, kind) }]
      if (container) concrete.push({ t: 'into', ref: idRef(container) })
      concrete.push({ t: 'asId', id })
      if (Object.keys(aspects).length > 0) concrete.push({ t: 'pair', k: 'aspects', v: JSON.stringify(aspects) })
      if (Object.keys(attrs).length > 0) concrete.push({ t: 'pair', k: 'attrs', v: JSON.stringify(attrs) })
      return { concrete: [{ op: 'SPAWN', args: concrete }], refused: false }
    }
    case 'DESTROY': {
      const concrete = concretize(args, ctx)
      for (const e of expand(concrete, ctx)) {
        e.deadAction = ctx.recordId
        // names are unique among the LIVING (§9.14): the name frees only when
        // the LAST pool copy dies (§9.38 array removal)
        releaseName(ctx.state, e)
      }
      return { concrete: [{ op: o.op, args: concrete }], refused: false }
    }
    case 'REPLACE': {
      // §4.4: REPLACE ref WITH prototype — mint from the prototype into the
      // dying card's CURRENT container, then soft-destroy it. The log carries
      // the concrete SPAWN+DESTROY pair, so the fold replays it exactly (§9.33).
      const targetArg = args[0]
      const protoArg = args[1]
      if (!protoArg || protoArg.t !== 'ref' || protoArg.ref.form !== 'name') {
        throw new Error('REPLACE needs (ref|ROLE) WITH a prototype or kind:NAME ref')
      }
      const protoName = protoArg.ref.name
      // plain prototype mint — REPLACE has no clause, so prototype derivation
      // (`name from object`, §6.4) is unavailable here (§9.33)
      const proto = ctx.content.prototypes.get(protoName)
      const kind = protoArg.ref.kind ?? proto?.kind
      if (!kind) throw new Error(`REPLACE ${protoName}: no kind (prototype or kind:NAME required)`)
      const aspects: Aspects = proto?.aspects ? cloneAspects(proto.aspects) : {}
      // §9.38: same computation as SPAWN — the RESOLVED name+kind, so the
      // fold's recompute from the concrete SPAWN row agrees (a stackable
      // successor may share the dying card's name: REPLACE X WITH X succeeds —
      // spawn appends, destroy pops)
      const isStack = mintIsStackable(ctx.content, { name: protoName, kind })
      const targets = expand([targetArg], ctx)
      if (targets.length === 0) throw new Error(`REPLACE: unresolvable ref (nothing to replace)`)
      const concrete: Outcome[] = []
      for (const dying of targets) {
        // spawn happens while the dying card is still alive, so a non-stackable
        // successor's name collides with any living holder — whole-step
        // rejection (§9.33); a stackable successor may share a stackable pool
        const pool = ctx.state.byName.get(protoName)
        if (pool && pool.length > 0 && (!isStack || !pool.every(pid => ctx.state.entities.get(pid)?.stackable === true))) {
          throw new Error(`REPLACE: name '${protoName}' already in use`)
        }
        const id = ctx.idGen()
        const attrs: Record<string, string | number | boolean> = { ...(proto?.attrs ?? {}) }
        const container = typeof dying.attrs['container'] === 'string' ? dying.attrs['container'] : undefined
        // zone-tagged (§10): the grouping has no card, so the tag passes on —
        // as a plain attr pair, NOT as INTO (the fold's SPAWN replay would
        // materialize a container the mint never had)
        const zone = !container && typeof dying.attrs['zone'] === 'string' ? dying.attrs['zone'] : undefined
        if (container) attrs['container'] = container
        if (zone) attrs['zone'] = zone
        addEntity(ctx.state, { id, kind, name: protoName, aspects: cloneAspects(aspects), attrs, bornAction: ctx.recordId, deadAction: null, stackable: isStack })
        dying.deadAction = ctx.recordId
        releaseName(ctx.state, dying)
        const spawnArgs: Arg[] = [{ t: 'ref', ref: nameRef(protoName, kind) }]
        if (container) spawnArgs.push({ t: 'into', ref: idRef(container) })
        spawnArgs.push({ t: 'asId', id })
        if (Object.keys(aspects).length > 0) spawnArgs.push({ t: 'pair', k: 'aspects', v: JSON.stringify(aspects) })
        // always materialize attrs, even when empty (§9.18): the pair pins the
        // mint's location — with no pair the fold would re-add the bound
        // actor's container to a location-less successor
        spawnArgs.push({ t: 'pair', k: 'attrs', v: JSON.stringify(attrs) })
        concrete.push({ op: 'SPAWN', args: spawnArgs })
        concrete.push({ op: 'DESTROY', args: [{ t: 'ref', ref: idRef(dying.id) }] })
      }
      return { concrete, refused: false }
    }
    case 'SET': {
      // §4.4 arity: SET(ref, key, value) — positional
      const refArg = args[0]
      const keyArg = args[1]
      const valArg = args[2]
      if (!refArg || !keyArg || !valArg) throw new Error('SET needs (ref, key, value)')
      const rawKey = keyArg.t === 'key' || keyArg.t === 'str'
        ? keyArg.v
        : keyArg.t === 'ref' && keyArg.ref.form === 'name' ? keyArg.ref.name : null
      if (!rawKey) throw new Error('SET key must be a name or string')
      // an `aspect:` prefix names the aspects map (§4.2); every other key is an attr (§9.31)
      const aspectPath = rawKey.startsWith('aspect:') ? rawKey.slice('aspect:'.length) : null
      const key = aspectPath ?? rawKey
      // the concrete key keeps its authored form: a quoted key must stay quoted
      // or the record stops round-tripping (§9.15)
      const keyConcrete: Arg = keyArg.t === 'str' || (keyArg.t === 'ref' && !aspectPath)
        ? keyArg
        : { t: 'key', v: rawKey }
      const write = (e: Entity, value: string | number | boolean): void => {
        if (aspectPath) {
          if (typeof value !== 'number') throw new Error(`SET: aspect '${rawKey}' needs a number`)
          aspectSet(e, aspectPath, value)
        } else {
          e.attrs[key] = value
          if (key === 'expires') e.attrs['expiresAt'] = ctx.now + toMinutes(value)
        }
      }
      if (valArg.t === 'delta') {
        // signed value (§9.31): add to the entry's current number (missing = 0,
        // negative results allowed) and log the CONCRETE ABSOLUTE result — the
        // fold replays literals and never redoes arithmetic
        const targets = expand([refArg], ctx)
        return {
          refused: false,
          concrete: targets.map(e => {
            // raw entry, not aspectGet: a sub-map must REFUSE the delta, not
            // count as missing and be clobbered (§9.31 non-numeric rule)
            const cur = aspectPath ? aspectEntry(e, aspectPath) : e.attrs[key]
            if (cur !== undefined && typeof cur !== 'number') throw new Error(`SET: cannot add to non-numeric '${rawKey}'`)
            const next = (cur ?? 0) + valArg.v
            write(e, next)
            // one concrete SET per card, so replay needs no step context (§9.15)
            const refConcrete: Arg = targets.length === 1 && refArg.t !== 'role' ? refArg : { t: 'ref', ref: idRef(e.id) }
            return { op: 'SET', args: [refConcrete, keyConcrete, { t: 'int', v: next }] }
          }),
        }
      }
      let value: string | number | boolean
      if (valArg.t === 'int' || valArg.t === 'str' || valArg.t === 'bool' || valArg.t === 'dur') value = valArg.v
      else throw new Error('SET value must be a string, number, boolean or duration')
      const targets = expand([refArg], ctx)
      for (const e of targets) write(e, value)
      // §9.15: the log never contains sugar — a ROLE arg must not reach the
      // record (the fold rebuilds whole-record bindings, so over a
      // multi-clause step it would attribute the role to the wrong clause).
      // One concrete SET per card, exactly like the delta path above; name
      // and kind refs replay fine and stay as written.
      if (refArg.t === 'role') {
        return {
          refused: false,
          concrete: targets.map(e => ({ op: 'SET', args: [{ t: 'ref', ref: idRef(e.id) }, keyConcrete, valArg] })),
        }
      }
      return { concrete: [{ op: 'SET', args: [refArg, keyConcrete, valArg] }], refused: false }
    }
    case 'MOVE': {
      const into = args.find((a): a is Extract<Arg, { t: 'into' }> => a.t === 'into')
      if (!into) throw new Error('MOVE needs INTO')
      const intoConcrete: Arg = 'role' in into.ref
        ? { t: 'into', ref: idOrSelf(ctx.bindings[into.ref.role]?.[0], into.ref) }
        : into
      const dest = 'role' in intoConcrete.ref
        ? undefined
        : resolveRef(ctx.state, intoConcrete.ref)?.id
      if (!dest || typeof dest !== 'string') throw new Error('MOVE: unresolvable destination')
      const refs = concretize(args.filter(a => a.t !== 'into'), ctx)
      for (const e of expand(refs, ctx)) e.attrs['container'] = dest
      return { concrete: [{ op: o.op, args: [...refs, intoConcrete] }], refused: false }
    }
    case 'REVEAL': {
      const concrete = concretize(args, ctx)
      for (const e of expand(concrete, ctx)) e.attrs['state.hidden'] = false
      return { concrete: [{ op: o.op, args: concrete }], refused: false }
    }
    case 'ADVANCE': {
      const d = args[0]
      if (!d || d.t !== 'dur') throw new Error('ADVANCE needs a duration')
      ctx.extraAdvanceMin += d.v
      return { concrete: [o], refused: false }
    }
    case 'NARRATE':
      return { concrete: [rewriteRollProse(o, ctx)], refused: false }
    case 'PROGRESS': {
      // §6.7: a read-bar tick — journal-only, no state effect, no time effect
      const d = args[0]
      const s = args[1]
      if (!d || d.t !== 'dur' || !s || s.t !== 'str') throw new Error('PROGRESS needs (duration, string)')
      return { concrete: [o], refused: false }
    }
    case 'RELATE': {
      const refs = args.filter((a): a is Extract<Arg, { t: 'ref' }> => a.t === 'ref')
      const valueArg = args.find((a): a is Extract<Arg, { t: 'int' }> => a.t === 'int')
      if (refs.length < 3) throw new Error('RELATE needs (ref, ref, type[, value])')
      const a = resolveRef(ctx.state, refs[0].ref), b = resolveRef(ctx.state, refs[1].ref)
      if (!a || !b) throw new Error('RELATE: unresolvable ref')
      const type = refs[2].ref.form === 'name' ? refs[2].ref.name : null
      if (!type) throw new Error('RELATE: relation type must be a name')
      const def = ctx.dict.relationDef(type)
      const rel = {
        a: a.id, b: b.id, type,
        value: valueArg ? valueArg.v : 1,
        bornAction: ctx.recordId, deadAction: null,
      }
      ctx.state.relations.set(relationKey(a.id, b.id, type, def.symmetric), rel)
      return { concrete: [o], refused: false }
    }
    case 'UNRELATE': {
      const refs = args.filter((a): a is Extract<Arg, { t: 'ref' }> => a.t === 'ref')
      if (refs.length < 3) throw new Error('UNRELATE needs (ref, ref, type)')
      const a = resolveRef(ctx.state, refs[0].ref), b = resolveRef(ctx.state, refs[1].ref)
      const type = refs[2].ref.form === 'name' ? refs[2].ref.name : null
      if (a && b && type) {
        const def = ctx.dict.relationDef(type)
        const rel = ctx.state.relations.get(relationKey(a.id, b.id, type, def.symmetric))
        if (rel) rel.deadAction = ctx.recordId // §7: stamp, don't erase — undo re-folds
      }
      return { concrete: [o], refused: false }
    }
    case 'ENDING': {
      // §9.35: the story's ending. First ENDING to apply in a step wins —
      // later ones are logged no-ops. The step itself completes normally
      // (outcomes apply, the clock advances; the ending rides along), and a
      // REFUSE later in the same step discards the ending with the whole
      // draft. The concrete log form is `ENDING <id>`; title/flavour resolve
      // here from content — the one code path both play and fold share.
      const a = args[0]
      const id = a && a.t === 'ref' && a.ref.form === 'name' ? a.ref.name : null
      if (!id) throw new Error('ENDING needs an id: ENDING ENDING_ID')
      const ending = ctx.content.endings.get(id)
      if (!ending) throw new Error(`ENDING '${id}' is not in the endings table`)
      if (!ctx.state.ended) ctx.state.ended = { id, title: ending.title, flavour: ending.flavour }
      return { concrete: [o], refused: false }
    }
    case 'REFUSE': {
      return { concrete: [o], refused: true }
    }
    case 'ROLL': {
      // §9.41 — the engine's ONLY randomness. The args are §6.3 value
      // expressions evaluated against this clause's bindings and the DRAFT
      // (strictly: a path over an unbound role rejects the step, like an IF
      // condition). A roll of m dice consumes exactly m counter values
      // c .. c+m-1; the concrete row carries the total AND the counter after,
      // so the fold lands the counter without deriving anything (law 3).
      // count = 0 is legal (total = bonus, counter unmoved); sides < 2 or
      // count < 0 reject the step (§9.19 — nothing logged, clock unmoved).
      const countV = args[0]
      const sidesV = args[1]
      const bonusV = args[2]
      if (!countV || countV.t !== 'vexpr' || !sidesV || sidesV.t !== 'vexpr' || (bonusV !== undefined && bonusV.t !== 'vexpr')) {
        throw new Error('internal: ROLL arguments must be value expressions')
      }
      const pctx: PredCtx = { b: ctx.bindings, state: ctx.state, strict: true, roll: ctx.roll }
      const count = evalExpr(countV.v, pctx)
      const sides = evalExpr(sidesV.v, pctx)
      const bonus = bonusV !== undefined ? evalExpr(bonusV.v, pctx) : 0
      // one strict discipline for all three operands (§9.39's rule, applied to
      // dice): an operand that cannot resolve — a missing aspect/attr over its
      // role — rejects the step, never a silent 0-dice roll
      if (count === undefined || !Number.isInteger(count) || count < 0) {
        throw new Error(`ROLL: dice count must be an integer >= 0, got ${count ?? 'nothing'}`)
      }
      if (sides === undefined || !Number.isInteger(sides) || sides < 2) {
        throw new Error(`ROLL: sides must be an integer >= 2, got ${sides ?? 'nothing'}`)
      }
      if (bonus === undefined || !Number.isInteger(bonus)) {
        throw new Error(`ROLL: bonus must be an integer, got ${bonus === undefined ? 'nothing' : bonus}`)
      }
      const c = ctx.state.rng
      let total = bonus
      for (let i = 0; i < count; i++) total += rollDie(ctx.content.seed, c + i, sides)
      ctx.roll = total // the per-clause binding: `roll` and {roll} read this
      ctx.state.rng = c + count // consume the counters ON THE DRAFT — the fold lands the same spot via the ROLLED row (law 1)
      return {
        concrete: [{ op: 'ROLLED', args: [{ t: 'int', v: total }, { t: 'int', v: c + count }] }],
        refused: false,
      }
    }
    case 'ROLLED': {
      // §9.41, fold side: the recorded roll — set the counter, nothing else
      // (the total is journal data; replay never re-derives a die, law 3).
      // Play never applies this op: authored ROLLED is a load error, so the
      // only path here is applyRecord over a logged row.
      const ctrAfter = args[1]
      if (!ctrAfter || ctrAfter.t !== 'int') throw new Error('ROLLED needs (total, ctrAfter)')
      ctx.state.rng = ctrAfter.v
      return { concrete: [o], refused: false }
    }
    case 'DRAW': {
      // §9.42 — draw the deck's next card. The deck table lives on compiled
      // content (the permutation is pure in seed+id, law 5); the cursor lives
      // on State and advances ON THE DRAFT here — a later failure or REFUSE
      // discards it with the whole draft (law 4). Concretizes to TWO rows:
      // the engine-written `DRAWN(<deck>, <name>)` (the fold's cursor row,
      // carrying the SPEC entry name so its assertion reads the permutation)
      // followed by the standard concrete SPAWN — which owns every mint rule
      // (prototype aspects/derive, §9.38 stackability, materialization
      // pairs), so DRAW adds none of its own. Authored DRAW never reaches
      // the log, like ROLL and IF.
      const deckArg = args[0]
      if (!deckArg || deckArg.t !== 'ref' || deckArg.ref.form !== 'name') {
        throw new Error('DRAW needs a deck id: DRAW <DECK> [INTO target]')
      }
      const deckId = deckArg.ref.name
      const deck = ctx.content.decks.get(deckId)
      if (!deck) throw new Error(`DRAW: unknown deck '${deckId}'`)
      const into = args.find((a): a is Extract<Arg, { t: 'into' }> => a.t === 'into')
      // §9.42: DRAW's INTO must RESOLVE — a role over empty event bindings or
      // an unresolvable ref would otherwise materialize the card nowhere,
      // silently. (SPAWN's own INTO stays lenient; a trigger that wants a
      // DRAWN card lands it with an explicit INTO — event bindings are
      // empty, so the no-INTO default below reads nothing in events either.)
      let intoConcrete: Arg | undefined
      if (into) {
        const dest = 'role' in into.ref ? (ctx.bindings[into.ref.role]?.[0] ?? null) : resolveRef(ctx.state, into.ref)
        if (!dest) throw new Error(`DRAW: INTO target of '${deckId}' cannot resolve`)
        intoConcrete = { t: 'into', ref: idRef(dest.id) }
      }
      // serve + advance the cursor; drawFrom throws on an exhausted deck with
      // neither `reset` nor `defaultcard` — §9.19 (nothing logged, cursor
      // unmoved: the mutation below happens on the draft only after it returns)
      const drawn: DeckCard = drawFrom(deck, ctx.state.decks.get(deckId) ?? 0)
      ctx.state.decks.set(deckId, (ctx.state.decks.get(deckId) ?? 0) + 1)
      const spawn: Outcome = {
        op: 'SPAWN',
        args: [
          { t: 'ref', ref: nameRef(drawn.name, drawn.kind) },
          ...(intoConcrete ? [intoConcrete] : []),
          { t: 'asId', id: ctx.idGen() },
        ],
      }
      const spawnConcrete = applyOutcome(spawn, ctx).concrete
      return {
        concrete: [
          { op: 'DRAWN', args: [{ t: 'ref', ref: nameRef(deckId) }, { t: 'ref', ref: nameRef(drawn.name) }] },
          ...spawnConcrete,
        ],
        refused: false,
      }
    }
    case 'DRAWN': {
      // §9.42, fold side: advance the deck cursor, ASSERTING the recorded
      // name is exactly what the compiled permutation serves at that cursor
      // (play and fold share drawFrom, so only content drift can disagree).
      // A seed/spec change between play and fold shifts future draws — and
      // this assertion is what turns the drifted PAST into a loud error
      // instead of a silent divergence (the dice's ROLLED rows never
      // re-derive; a deck's cursor row must, so drift is visible). Play never
      // applies this op: authored DRAWN is a load error, so the only path
      // here is applyRecord over a logged row.
      const deckArg = args[0]
      const nameArg = args[1]
      if (!deckArg || deckArg.t !== 'ref' || deckArg.ref.form !== 'name' || !nameArg || nameArg.t !== 'ref' || nameArg.ref.form !== 'name') {
        throw new Error('DRAWN needs (deck, name)')
      }
      const deck = ctx.content.decks.get(deckArg.ref.name)
      if (!deck) throw new Error(`DRAWN: deck '${deckArg.ref.name}' is not in the compiled decks — content drift between play and fold (§9.42)`)
      const cursor = ctx.state.decks.get(deck.id) ?? 0
      const expected = drawFrom(deck, cursor) // throws loud on a drifted-away exhaustion rule
      if (expected.name !== nameArg.ref.name) {
        throw new Error(
          `DRAWN: deck '${deck.id}' draw #${cursor} recorded '${nameArg.ref.name}' ` +
          `but the compiled deck serves '${expected.name}' — content drift between play and fold (§9.42)`,
        )
      }
      ctx.state.decks.set(deck.id, cursor + 1)
      return { concrete: [o], refused: false }
    }
    default:
      throw new Error(`unknown outcome op '${o.op}'`)
  }
}

/**
 * §9.39 — IF / ELSE IF / ELSE in an outcome list. Arms evaluate in order
 * against the DRAFT (this list's earlier outcomes — SET, SPAWN, DESTROY —
 * are visible to later IFs, unlike recipe selection which reads the input
 * state); the first true condition wins; ELSE is the default; no true arm
 * and no ELSE applies nothing, without error. Concretization follows the
 * SET-delta precedent (§9.31): the log records ONLY the taken branch's
 * outcomes, flattened in position — the record never contains an IF op, and
 * the untaken branches are authored discipline the log cannot recover.
 * Conditions evaluate STRICTLY (§9.19): an operand that cannot resolve —
 * a path over a role with no bound card, F4's unbound `roll` — rejects the
 * whole step, where a `when:` predicate would have read false.
 */
function applyIf(o: Outcome, ctx: ApplyCtx): ApplyResult {
  const pctx: PredCtx = { b: ctx.bindings, state: ctx.state, strict: true, roll: ctx.roll }
  for (const arm of o.arms ?? []) {
    if (evalPred(arm.when, pctx)) return applyAll(arm.then, ctx)
  }
  return o.fallback && o.fallback.length > 0 ? applyAll(o.fallback, ctx) : { concrete: [], refused: false }
}

/** apply a branch body in order; a REFUSE inside a taken branch refuses the
 *  whole step as usual (§6.5 — draft discarded, attempt logged) */
function applyAll(outcomes: Outcome[], ctx: ApplyCtx): ApplyResult {
  const concrete: Outcome[] = []
  for (const sub of outcomes) {
    const res = applyOutcome(sub, ctx)
    concrete.push(...res.concrete)
    if (res.refused) return { concrete, refused: true }
  }
  return { concrete, refused: false }
}

/** `expires` materialization (§4.2): numbers are already minutes — the lexer
 *  pre-converts bare duration literals (§5.1) before SET sees them; quoted
 *  strings go through the one duration parser, so every §5.1 form
 *  (`2halfhour`, `2h`, `90m`, `1d`) materializes `expiresAt`. */
function toMinutes(v: string | number | boolean): number {
  if (typeof v === 'number') return v
  if (typeof v !== 'string') throw new Error(`bad duration '${v}'`)
  return parseDuration(v)
}

/** resolve ref/role args to entities (role refs expand across the binding) */
function expand(args: Arg[], ctx: ApplyCtx): Entity[] {
  const out: Entity[] = []
  for (const a of args) {
    if (a.t === 'role') {
      out.push(...(ctx.bindings[a.role] ?? []))
    } else if (a.t === 'ref') {
      const e = resolveRef(ctx.state, a.ref)
      if (e) out.push(e)
    }
  }
  return out
}

function cloneAspects(a: Aspects): Aspects {
  const out: Aspects = {}
  for (const [k, v] of Object.entries(a)) out[k] = typeof v === 'number' ? v : { ...v }
  return out
}

// ---------- fold support (§7: state = fold(log)) ----------

/**
 * Apply one record's concrete outcomes to a state (replay).
 * Refused records are state no-ops: their logged outcomes are journal
 * history, never replayed effects (§6.5).
 */
export function applyRecord(record: ActionRecord, state: State, dict: Dictionary, content: CompiledContent): void {
  const one = (ref: Ref | null): Entity[] => {
    if (!ref) return []
    const e = resolveRef(state, ref)
    return e ? [e] : []
  }
  // v0.5 — the pure clock row: the ONLY record that moves `now`, so the
  // fold's clock is a function of the wait spans alone (BEGIN/COMPLETE never
  // touch it, which is what makes interleaved windows replay exactly)
  if (record.kind === 'wait') {
    const start = record.startMin ?? absMin(parseClock(record.tGame))
    if (state.now !== start) {
      throw new Error(`wait row ${record.id} starts at ${record.tGame} but the folded clock reads ${state.now} — wait spans must partition time (log drift, or a pre-v0.5 log)`)
    }
    state.now = start + record.advancedMin
    return
  }
  // v0.5 — a window closing: remove it first (a refused completion still
  // closed its window), then apply outcomes anchored to the window's START
  // clock read from the windows table — the same source play used, so SET
  // expires and friends replay identically (the completion clock on tGame
  // never feeds mechanics, §9.46)
  if (record.kind === 'complete') {
    // the window SHOULD be registered by the turn's BEGIN row (fold rebuilds
    // it first); a row closing an unknown window is tolerated with a
    // tGame-anchored fallback so bare-row fold probes and pre-window logs
    // still replay — the START-clock anchoring that matters for `expires` is
    // exercised on the real path, where the window exists
    const w = record.window ? state.windows.get(record.window) : undefined
    if (w) state.windows.delete(record.window!)
    if (record.outcomes.some(o => o.op === 'REFUSE')) return // refused: window closed, effects discarded (§6.5)
    const startMin = w ? w.start : absMin(parseClock(record.tGame))
    let n = 0
    const ctx: ApplyCtx = {
      state, dict, content,
      bindings: {
        actors: record.actors.flatMap(a => one(a)),
        object: record.objects.flatMap(a => one(a)),
        with: record.instruments.flatMap(a => one(a)),
        at: one(record.at),
        about: one(record.about),
        to: one(record.to),
      },
      now: startMin,
      recordId: record.id,
      idGen: () => `r${record.id}s${++n}`, // deterministic fallback for AS-less SPAWNs
      extraAdvanceMin: 0,
    }
    for (const o of record.outcomes) applyOutcome(o, ctx)
    // ADVANCE replayed here only touches the inert extraAdvanceMin — the
    // extension lives in the wait row's span (§9.46 replay safety)
    return
  }
  if (record.outcomes.some(o => o.op === 'REFUSE')) return // refused: state no-op, clock untouched (§6.5)
  // events don't own the clock — only wait rows move it (§7, v0.5)
  if (record.kind === 'event') state.fired.add(record.trigger ?? '') // once-triggers replay as fired (§9.29)
  const startMin = absMin(parseClock(record.tGame))
  if (record.kind === 'event') {
    let n = 0
    const ctx: ApplyCtx = {
      state, dict, content,
      bindings: {
        actors: record.actors.flatMap(a => one(a)),
        object: record.objects.flatMap(a => one(a)),
        with: record.instruments.flatMap(a => one(a)),
        at: one(record.at),
        about: one(record.about),
        to: one(record.to),
      },
      now: startMin,
      recordId: record.id,
      idGen: () => `r${record.id}s${++n}`,
      extraAdvanceMin: 0,
    }
    for (const o of record.outcomes) applyOutcome(o, ctx)
    return
  }
  // action = BEGIN (v0.5): register one window per clause — no clock move, no
  // outcome application (the PROGRESS ticks are journal-only; the recipe's
  // outcomes land on the COMPLETE row at the window's close)
  registerWindows(record, state, dict, content)
}

// ---------- events & completions: the advance pass (§6.6, §6.7, §9.16) ----------

interface PassResult { state: State; records: ActionRecord[] }

/**
 * v0.5 — advance the world from `state.now` to `toMin` in ONE atomic pass.
 * Candidates merge chronologically: due windows close at their `end` (ordered
 * by end, then beginId, then clause — a total order with no ties); authored
 * slot triggers and expiries fire at each crossed halfhour (§9.16 order:
 * triggers in declaration order, then expiries from the hoisted snapshot).
 * A completion at minute m fires BEFORE slot-m events. The pass ends with one
 * WAIT row — the only record that moves the clock — spanning exactly
 * `[state.now, toMin]`, so wait spans partition time (repeating triggers
 * cannot double-fire across windows). `ADVANCE` outcomes land while windows
 * complete and EXTEND the pass target; the extension rides the wait row's
 * span, which is what makes it replay-safe (fold never re-derives it).
 */
function resolveDue(
  content: CompiledContent,
  dict: Dictionary,
  stateIn: State,
  toMin: number,
  firstRecordId: number,
  idGen: () => string,
): PassResult {
  // a zero-span pass is meaningful only when a due window exists (a
  // zero-duration verb completes exactly at its begin minute)
  const hasDue = [...stateIn.windows.values()].some(w => w.end <= toMin)
  if (toMin < stateIn.now || (toMin === stateIn.now && !hasDue)) return { state: stateIn, records: [] }
  const fromMin = stateIn.now
  let state = stateIn
  const records: ActionRecord[] = []
  // §6.6: the expiry snapshot is taken once per pass and hoisted over every
  // completion and slot — no cascades within one advance; expiries created by
  // this pass's outcomes resolve at the NEXT advance
  const expiring = [...state.entities.values()]
    .filter(e => e.deadAction === null && typeof e.attrs['expiresAt'] === 'number')
    .map(e => ({ id: e.id, name: e.name, at: e.attrs['expiresAt'] as number }))
  let cursor = fromMin
  let target = toMin
  let id = firstRecordId
  for (;;) {
    const due = [...state.windows.values()].filter(w => w.end <= target)
      .sort((a, b) => a.end - b.end || a.beginId - b.beginId || a.clause - b.clause)
    const nextW = due[0]
    const slotAbs = nextSlotAfter(cursor, target)
    if (nextW && (slotAbs === null || nextW.end <= slotAbs)) {
      const fired = fireWindow(content, dict, state, nextW, id, idGen)
      state = fired.state
      records.push(fired.record)
      id += 1
      // NOTE: the slot cursor does NOT jump to the window's end — the
      // halfhours the window spanned must still strike (a completion at
      // minute m precedes slot-m events, but slot-(<m) events fire after it
      // in the same pass, §6.6 chronological order)
      if (fired.advanceExtra > 0) target += fired.advanceExtra
      continue
    }
    if (slotAbs !== null) {
      // fireSlot speaks in halfhour INDICES (§9.16: slot = abs % 48, minute =
      // abs * 30); the walk speaks in minutes
      const slotRecords = fireSlot(content, dict, state, slotAbs / MIN_PER_HALFHOUR, expiring, id, idGen)
      records.push(...slotRecords)
      id += slotRecords.length
      cursor = slotAbs
      continue
    }
    break
  }
  records.push(makeWaitRecord(id, fromMin, target - fromMin, dict))
  state.now = target
  return { state, records }
}

/** the first halfhour boundary strictly after `cursor`, or null past `target`
 *  — the (from, to] crossing rule of §9.28, one boundary at a time */
function nextSlotAfter(cursor: number, target: number): number | null {
  const abs = (Math.floor(cursor / MIN_PER_HALFHOUR) + 1) * MIN_PER_HALFHOUR
  return abs <= target ? abs : null
}

/**
 * Close one window: re-resolve its clause bindings (dead refs DROP — events
 * do not respect locks, §6.7 v0.5), apply the commit-time recipe's outcomes
 * anchored to the window's START clock (§6.7: clock-anchored effects use the
 * start clock). A REFUSE that only decides now, or an outcome that cannot
 * apply, refuses the WINDOW: the partial effects are discarded with the
 * scratch, the attempt is logged with a REFUSE marker (the fold's existing
 * no-op rule), and the window still closes — a pass that rolled back would
 * leave the window retrying forever (§9.19's completion analog).
 */
function fireWindow(
  content: CompiledContent,
  dict: Dictionary,
  stateIn: State,
  w: OpenWindow,
  recordId: number,
  idGen: () => string,
): { state: State; record: ActionRecord; advanceExtra: number } {
  const verb = content.verbs.get(w.verb)
  const recipe = verb?.recipes.find(r => r.id === w.recipe)
  if (!verb || !recipe) {
    throw new Error(
      `completion of ${w.verb} (begin row ${w.beginId}): recipe '${w.recipe}' is not in the ` +
      `compiled content — content drift between commit and completion`,
    )
  }
  const scratch = cloneState(stateIn)
  const bindings: RoleBindings = {}
  for (const role of ROLES) {
    bindings[role] = w.refs[role]
      .map(ref => resolveRef(scratch, ref))
      .filter((e): e is Entity => e !== null)
  }
  let mint = 0
  const ctx: ApplyCtx = {
    state: scratch, dict, content, bindings,
    now: w.start,             // §6.7: expires and friends anchor to the START clock
    recordId,
    idGen: idGen ?? (() => `c${recordId}_${++mint}`),
    extraAdvanceMin: 0,
  }
  const concrete: Outcome[] = []
  let refused = false
  try {
    for (const o of recipe.out) {
      if (o.op === 'PROGRESS') continue // the ticks rode the BEGIN row
      const res = applyOutcome(o, ctx)
      concrete.push(...res.concrete)
      if (res.refused) { refused = true; break }
    }
  } catch (err) {
    refused = true
    concrete.length = 0
    concrete.push({ op: 'REFUSE', args: [{ t: 'str', v: err instanceof Error ? err.message : String(err) }] })
  }
  const out = refused ? stateIn : scratch
  out.windows.delete(w.key)
  return { state: out, record: makeCompleteRecord(recordId, w, concrete, dict), advanceExtra: ctx.extraAdvanceMin }
}

function makeCompleteRecord(id: number, w: OpenWindow, outcomes: Outcome[], dict: Dictionary): ActionRecord {
  const clock = clockFromAbsMin(w.end)
  const rec: ActionRecord = {
    id,
    kind: 'complete',
    trigger: w.verb,
    tGame: formatClock(clock), // the completion clock — display only; mechanics anchor via the window
    tReal: null,
    advancedMin: 0,            // complete rows never move the clock — waits do
    startMin: null,
    window: w.key,
    actors: [...w.refs.actors],
    verb: w.verb,
    objects: [...w.refs.object],
    instruments: [...w.refs.with],
    at: w.refs.at[0] ?? null,
    about: w.refs.about[0] ?? null,
    to: w.refs.to[0] ?? null,
    outcomes,
    recipes: [w.recipe],
    sentence: '',
  }
  rec.sentence = formatSentence({ kind: 'complete', clock, trigger: w.verb, clauses: [], outcomes }, dict)
  return rec
}

function makeWaitRecord(id: number, fromMin: number, spanMin: number, dict: Dictionary): ActionRecord {
  const clock = clockFromAbsMin(fromMin)
  const rec: ActionRecord = {
    id,
    kind: 'wait',
    trigger: null,
    tGame: formatClock(clock),
    tReal: null,
    advancedMin: spanMin,
    startMin: fromMin,         // exact span start — fold asserts continuation
    window: null,
    actors: [], verb: '', objects: [], instruments: [], at: null, about: null, to: null,
    outcomes: [], recipes: [],
    sentence: '',
  }
  rec.sentence = formatSentence({ kind: 'wait', clock, trigger: null, clauses: [], outcomes: [], spanMin }, dict)
  return rec
}

/** fire one crossed halfhour: authored triggers (declaration order) first,
 *  then expiries from the pass's snapshot — §9.16 order, unchanged (§6.6) */
function fireSlot(
  content: CompiledContent,
  dict: Dictionary,
  state: State,
  absSlot: number,
  expiring: { id: string; name: string; at: number }[],
  firstEventId: number,
  idGen: () => string,
): ActionRecord[] {
  const records: ActionRecord[] = []
  for (const trig of content.triggers) {
    if (!('slot' in trig)) continue // expiry triggers ride the expiry path below
    if (trig.slot !== absSlot % 48) continue
    if (trig.once && state.fired.has(trig.id)) continue // §9.29: once means once
    const ctx = makeEventCtx(state, dict, content, absSlot, firstEventId + records.length, idGen)
    const outcomes: Outcome[] = []
    for (const o of trig.out) outcomes.push(...applyOutcome(o, ctx).concrete)
    state.fired.add(trig.id) // on the draft: a later rejection discards it
    records.push(makeEventRecord(firstEventId + records.length, trig.id, absSlot, outcomes, dict))
  }
  for (const exp of expiring) {
    if (exp.at > absSlot * 30) continue
    const e = state.entities.get(exp.id)
    if (!e || e.deadAction !== null) continue
    const ctx = makeEventCtx(state, dict, content, absSlot, firstEventId + records.length, idGen)
    // an authored `on expire of NAME` trigger (§9.32) replaces the built-in
    // destroy; first match in declaration order wins. Its outcomes ARE the
    // event record's outcomes (record.trigger = the authored id). An outcome
    // that cannot apply throws — the whole step is rejected (§9.19).
    // §9.38 pool asymmetry (documented ruling): the trigger matches by NAME,
    // so a crossing fires it no matter WHICH copy carries the expires — but
    // its name-addressed outcomes resolve per the pool rule (the OLDEST
    // copy), which under a pool may not be the crossing card. The built-in
    // path below has no such ambiguity: it addresses by id.
    const authored = content.triggers.find(t => 'onExpireOf' in t && t.onExpireOf === e.name)
    let triggerId: string
    let outcomes: Outcome[]
    if (authored) {
      triggerId = authored.id
      outcomes = []
      for (const o of authored.out) outcomes.push(...applyOutcome(o, ctx).concrete)
    } else {
      triggerId = `expire:${e.name}`
      // §9.38: engine-written destroys are keyed to the EXPIRING entity and
      // concretize to DESTROY #id — under a pool a name-addressed destroy
      // would kill the oldest copy while the real expiring card re-fires
      const destroy: Outcome = { op: 'DESTROY', args: [{ t: 'ref', ref: idRef(exp.id) }] }
      outcomes = [...applyOutcome(destroy, ctx).concrete]
    }
    // play marks fired exactly as the fold's applyRecord does — one entry per
    // EVENT row's trigger (§9.34); on the draft, so a rejection discards it
    state.fired.add(triggerId)
    records.push(makeEventRecord(firstEventId + records.length, triggerId, absSlot, outcomes, dict))
  }
  return records
}

function makeEventCtx(state: State, dict: Dictionary, content: CompiledContent, absSlot: number, recordId: number, idGen: () => string): ApplyCtx {
  return {
    state, dict, content,
    bindings: {} as RoleBindings,
    now: absSlot * 30,
    recordId,
    idGen,
    extraAdvanceMin: 0,
  }
}

function makeEventRecord(id: number, trigger: string, absSlot: number, outcomes: Outcome[], dict: Dictionary): ActionRecord {
  const clock = clockFromAbsMin(absSlot * 30)
  const rec: ActionRecord = {
    id, kind: 'event', trigger,
    tGame: formatClock(clock), tReal: null, advancedMin: 0,
    startMin: null, window: null,
    actors: [], verb: '', objects: [], instruments: [], at: null, about: null, to: null,
    outcomes, recipes: [],
    sentence: '',
  }
  rec.sentence = formatSentence({ kind: 'event', clock, trigger, clauses: [], outcomes }, dict)
  return rec
}
