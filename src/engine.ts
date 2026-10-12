// The interpreter — spec §6.6 (validation → selection → application → time →
// events) with step-atomic REFUSE (§6.5) and event-sourced records (§7).
import type { Arg, Clause, Outcome, Role, Sentence } from './sentence.js'
import { ROLES } from './sentence.js'
import type { Ref } from './ref.js'
import { idRef, nameRef, sortRefs } from './ref.js'
import { formatSentence } from './format.js'
import type { Dictionary } from './dict.js'
import type { CompiledContent, CompiledRecipe, CompiledSlot, CompiledVerb } from './content.js'
import { evalPred, evalExpr, type PredCtx, type RoleBindings } from './predicate.js'
import { rollDie } from './dice.js'
import { matchQuery } from './query.js'
import { absMin, clockFromAbsMin, crossedSlots, formatClock, parseClock, parseDuration, type Clock } from './clock.js'
import {
  addEntity, aspectEntry, aspectGet, aspectSet, cloneState, getAlive, oldestNamed, relationKey, releaseName,
  type Aspects, type Entity, type State,
} from './types.js'

export interface Step {
  clock: Clock
  clauses: Clause[]
  tReal?: string
}

export interface ActionRecord {
  id: number
  kind: 'action' | 'event'
  trigger: string | null
  tGame: string          // start clock "day:halfhour"
  tReal: string | null
  advancedMin: number    // minutes this record advanced the clock (0 for refused/events)
  actors: Ref[]          // sorted union across clauses
  verb: string           // first clause's verb ('' for events)
  objects: Ref[]
  instruments: Ref[]
  at: Ref | null
  about: Ref | null
  to: Ref | null
  outcomes: Outcome[]    // concrete: ids minted, sugar resolved (§9.15)
  recipes: string[]      // fired recipe ids, clause order
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
  // monotonic mechanics time (§9.5): a step may never rewind the clock
  if (absMin(step.clock) < stateIn.now) {
    const now = clockFromAbsMin(stateIn.now)
    return rejected(`clock ${formatClock(step.clock)} is before state.now (${now.day}:${now.halfhour})`, stateIn)
  }
  // events are fired by the clock (§5.1); a player-typed step needs a clause
  if (step.clauses.length === 0) {
    return rejected('a step needs at least one clause', stateIn)
  }

  // 1+2. validation & selection — nothing applies unless every clause clears
  const prepared: Prepared[] = []
  for (const clause of step.clauses) {
    const verb = content.verbs.get(clause.verb)
    if (!verb) return rejected(`${clause.verb} is not a content verb`, stateIn)
    const refs = clauseRefs(clause)
    const slots = validateSlots(verb, refs, stateIn)
    if ('error' in slots) return rejected(`${clause.verb}: ${slots.error}`, stateIn)
    const recipe = selectRecipe(verb, slots.bindings, stateIn)
    if ('error' in recipe) return rejected(recipe.error, stateIn)
    prepared.push({ clause, verb, refs, bindings: slots.bindings, recipe })
  }

  // 3. application into a draft — REFUSE discards the whole draft (§6.5)
  const draft = cloneState(stateIn)
  const startMin = absMin(step.clock)
  let mint = 0
  const ctx: ApplyCtx = {
    state: draft, dict, content,
    bindings: {} as RoleBindings,
    now: startMin,
    recordId: opts.firstRecordId,
    idGen: opts.idGen ?? (() => `e${opts.firstRecordId}_${++mint}`), // deterministic default (§9.3)
    extraAdvanceMin: 0,
  }

  let refused = false
  const collected: Outcome[] = []
  const recipeIds: string[] = []
  try {
    outer:
    for (const p of prepared) {
      // §9.41: the dice binding is PER CLAUSE — a roll in clause one is not
      // visible to clause two's conditions
      ctx.roll = undefined
      // re-resolve this clause's roles against the draft right before applying,
      // so entities an earlier clause destroyed can't be used stale (§6.6)
      const rebound: RoleBindings = {}
      for (const role of ROLES) {
        rebound[role] = p.refs[role]
          .map(ref => resolveRef(draft, ref))
          .filter((e): e is Entity => e !== null)
      }
      ctx.bindings = rebound
      recipeIds.push(p.recipe.id)
      for (const outcome of p.recipe.out) {
        const res = applyOutcome(outcome, ctx)
        collected.push(...res.concrete)
        if (res.refused) { refused = true; break outer }
      }
    }
  } catch (err) {
    return rejected(err instanceof Error ? err.message : String(err), stateIn)
  }
  const advancedMin = refused ? 0 : prepared.reduce((s, p) => s + p.verb.duration, 0) + ctx.extraAdvanceMin
  const first = prepared[0]
  const seen = new Set<string>()
  const record: ActionRecord = {
    id: opts.firstRecordId,
    kind: 'action',
    trigger: null,
    tGame: formatClock(step.clock),
    tReal: step.tReal ?? null,
    advancedMin,
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
    outcomes: collected,
    recipes: recipeIds,
    sentence: '',
  }
  const sentenceAst: Sentence = { kind: 'action', clock: parseClock(record.tGame), trigger: null, clauses: step.clauses, outcomes: collected }
  record.sentence = formatSentence(sentenceAst, dict)

  if (refused) {
    return { records: [record], state: stateIn, rejected: null } // draft discarded; attempt logged
  }

  // 4+6. time & events — the engine owns the clock (§9.12), events get own rows (§9.16).
  // Events span the whole elapsed span (§9.28): from the state's previous clock,
  // so quantized dead air before the step cannot swallow a crossing.
  const state = draft
  state.now = startMin + advancedMin
  const records: ActionRecord[] = [record]
  try {
    const eventRecords = fireCrossingEvents(content, dict, state, stateIn.now, state.now, opts.firstRecordId + 1, ctx.idGen)
    records.push(...eventRecords)
  } catch (err) {
    // §9.19: an event that cannot apply rejects the whole step — nothing logged
    return rejected(err instanceof Error ? err.message : String(err), stateIn)
  }

  return { records, state, rejected: null }
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
      for (const e of expand([refArg], ctx)) write(e, value)
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
  if (record.outcomes.some(o => o.op === 'REFUSE')) return // refused: state no-op, clock untouched (§6.5)
  // events don't own the clock — only action records advance/rebase it (§7)
  if (record.kind === 'event') state.fired.add(record.trigger ?? '') // once-triggers replay as fired (§9.29)
  if (record.kind === 'action') state.now = absMin(parseClock(record.tGame))
  const startMin = absMin(parseClock(record.tGame))
  // rebuild role bindings from the record itself so role-ref outcomes
  // replay without the original step context (concrete outcomes make this rare)
  const one = (ref: Ref | null): Entity[] => {
    if (!ref) return []
    const e = resolveRef(state, ref)
    return e ? [e] : []
  }
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
  if (record.kind === 'action') state.now = startMin + record.advancedMin
}

// ---------- events: triggers & expiries (§6.6, §9.16) ----------

function fireCrossingEvents(
  content: CompiledContent,
  dict: Dictionary,
  state: State,
  fromMin: number,
  toMin: number,
  firstEventId: number,
  idGen: () => string,
): ActionRecord[] {
  const records: ActionRecord[] = []
  // snapshot expiries before the pass — §6.6: no cascades within one advance;
  // expiries created by this pass's outcomes resolve at the NEXT advance
  const expiring = [...state.entities.values()]
    .filter(e => e.deadAction === null && typeof e.attrs['expiresAt'] === 'number')
    .map(e => ({ id: e.id, name: e.name, at: e.attrs['expiresAt'] as number }))
  for (const absSlot of crossedSlots(fromMin, toMin)) {
    // authored time triggers first, then expiries — deterministic order (§9.16)
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
    actors: [], verb: '', objects: [], instruments: [], at: null, about: null, to: null,
    outcomes, recipes: [],
    sentence: '',
  }
  rec.sentence = formatSentence({ kind: 'event', clock, trigger, clauses: [], outcomes }, dict)
  return rec
}
