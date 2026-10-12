// The interpreter — spec §6.6 (validation → selection → application → time →
// events) with step-atomic REFUSE (§6.5) and event-sourced records (§7).
import type { Arg, Clause, Outcome, Role, Sentence } from './sentence.js'
import { ROLES } from './sentence.js'
import type { Ref } from './ref.js'
import { idRef, nameRef, sortRefs } from './ref.js'
import { formatSentence } from './format.js'
import type { Dictionary } from './dict.js'
import type { CompiledContent, CompiledRecipe, CompiledSlot, CompiledVerb } from './content.js'
import { evalPred, type PredCtx, type RoleBindings } from './predicate.js'
import { matchQuery } from './query.js'
import { absMin, clockFromAbsMin, crossedSlots, formatClock, parseClock, type Clock } from './clock.js'
import {
  addEntity, aspectEntry, aspectGet, aspectSet, cloneState, getAlive, relationKey,
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
  const id = state.byName.get(ref.name)
  if (!id) return null
  const e = getAlive(state, id)
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
}

interface ApplyResult {
  concrete: Outcome[]    // what the log stores: ids minted, sugar resolved, deltas computed (§9.15, §9.31)
  refused: boolean
}

function applyOutcome(o: Outcome, ctx: ApplyCtx): ApplyResult {
  const args = o.args
  switch (o.op) {
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
      if (ctx.state.byName.has(name)) throw new Error(`SPAWN: name '${name}' already in use`)
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
      addEntity(ctx.state, { id, kind, name, aspects, attrs, bornAction: ctx.recordId, deadAction: null })
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
        // names are unique among the LIVING (§9.14): release the name on death
        if (ctx.state.byName.get(e.name) === e.id) ctx.state.byName.delete(e.name)
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
      const targets = expand([targetArg], ctx)
      if (targets.length === 0) throw new Error(`REPLACE: unresolvable ref (nothing to replace)`)
      const concrete: Outcome[] = []
      for (const dying of targets) {
        // spawn happens while the dying card is still alive, so replacing a
        // card with its own name collides — whole-step rejection (§9.33)
        if (ctx.state.byName.has(protoName)) throw new Error(`REPLACE: name '${protoName}' already in use`)
        const id = ctx.idGen()
        const attrs: Record<string, string | number | boolean> = { ...(proto?.attrs ?? {}) }
        const container = typeof dying.attrs['container'] === 'string' ? dying.attrs['container'] : undefined
        // zone-tagged (§10): the grouping has no card, so the tag passes on —
        // as a plain attr pair, NOT as INTO (the fold's SPAWN replay would
        // materialize a container the mint never had)
        const zone = !container && typeof dying.attrs['zone'] === 'string' ? dying.attrs['zone'] : undefined
        if (container) attrs['container'] = container
        if (zone) attrs['zone'] = zone
        addEntity(ctx.state, { id, kind, name: protoName, aspects: cloneAspects(aspects), attrs, bornAction: ctx.recordId, deadAction: null })
        dying.deadAction = ctx.recordId
        if (ctx.state.byName.get(dying.name) === dying.id) ctx.state.byName.delete(dying.name)
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
      return { concrete: [o], refused: false }
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
    default:
      throw new Error(`unknown outcome op '${o.op}'`)
  }
}

function toMinutes(v: string | number | boolean): number {
  if (typeof v === 'number') return v
  if (typeof v !== 'string') throw new Error(`bad duration '${v}'`)
  const m = /^(\d+)(h|m|d)$/.exec(v)
  if (!m) throw new Error(`bad duration '${v}'`)
  const n = Number(m[1])
  return m[2] === 'h' ? n * 60 : m[2] === 'd' ? n * 24 * 60 : n
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
      const authored = content.triggers.find(t => 'onExpireOf' in t && t.onExpireOf === e.name)
      let triggerId: string
      let outcomes: Outcome[]
      if (authored) {
        triggerId = authored.id
        outcomes = []
        for (const o of authored.out) outcomes.push(...applyOutcome(o, ctx).concrete)
      } else {
        triggerId = `expire:${e.name}`
        const destroy: Outcome = { op: 'DESTROY', args: [{ t: 'ref', ref: nameRef(e.name) }] }
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
