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
import { absMin, clockFromAbsMin, crossedHours, formatClock, parseClock, type Clock } from './clock.js'
import {
  addEntity, aspectGet, cloneState, getAlive, relationKey,
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
  tGame: string          // start clock "day:hour"
  tReal: string | null
  advancedMin: number    // minutes this record advanced the clock (0 for refused/events)
  actors: Ref[]          // sorted union across clauses
  verb: string           // first clause's verb ('' for events)
  objects: Ref[]
  instruments: Ref[]
  at: Ref | null
  about: Ref | null
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
  bindings: RoleBindings      // resolved against the INPUT state (validation)
  draftBindings: RoleBindings // resolved against the DRAFT (application)
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
    for (const ref of provided) {
      const e = resolveRef(state, ref)
      if (!e) return { error: `unresolvable ref ${show(ref)}` }
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
    prepared.push({ clause, verb, refs, bindings: slots.bindings, draftBindings: {}, recipe })
  }

  // 3. application into a draft — REFUSE discards the whole draft (§6.5)
  const draft = cloneState(stateIn)
  // re-bind roles against the draft so outcome mutations hit the clones,
  // never the caller's state (bindings from validation point at stateIn)
  for (const p of prepared) {
    const rebound: RoleBindings = {}
    for (const role of ROLES) {
      rebound[role] = p.refs[role]
        .map(ref => resolveRef(draft, ref))
        .filter((e): e is Entity => e !== null)
    }
    p.draftBindings = rebound
  }
  const startMin = absMin(step.clock)
  const collected: Outcome[] = []
  const recipeIds: string[] = []
  const ctx: ApplyCtx = {
    state: draft, dict, content,
    bindings: prepared[0]?.draftBindings ?? ({} as RoleBindings),
    now: startMin,
    recordId: opts.firstRecordId,
    idGen: opts.idGen ?? defaultIdGen,
    extraAdvanceMin: 0,
  }

  let refused = false
  outer:
  for (const p of prepared) {
    ctx.bindings = p.draftBindings
    recipeIds.push(p.recipe.id)
    for (const outcome of p.recipe.out) {
      const res = applyOutcome(outcome, ctx)
      collected.push(res.concrete)
      if (res.refused) { refused = true; break outer }
    }
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
    outcomes: collected,
    recipes: recipeIds,
    sentence: '',
  }
  const sentenceAst: Sentence = { kind: 'action', clock: parseClock(record.tGame), trigger: null, clauses: step.clauses, outcomes: collected }
  record.sentence = formatSentence(sentenceAst, dict)

  if (refused) {
    return { records: [record], state: stateIn, rejected: null } // draft discarded; attempt logged
  }

  // 4+6. time & events — the engine owns the clock (§9.12), events get own rows (§9.16)
  const state = draft
  state.now = startMin + advancedMin
  const records: ActionRecord[] = [record]
  const eventRecords = fireCrossingEvents(content, dict, state, startMin, state.now, opts.firstRecordId + 1, ctx.idGen)
  records.push(...eventRecords)

  return { records, state, rejected: null }
}

function rejected(reason: string, state: State): ExecResult {
  return { records: [], state, rejected: reason }
}

function defaultIdGen(): string {
  return 'e' + Math.random().toString(36).slice(2, 8)
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
  concrete: Outcome
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
      // materialized aspects/attrs as JSON pairs — prefer them over derive
      const aspectsArg = args.find(a => a.t === 'pair' && a.k === 'aspects')
      const attrsArg = args.find(a => a.t === 'pair' && a.k === 'attrs')
      if (aspectsArg && aspectsArg.t === 'pair' && typeof aspectsArg.v === 'string') {
        aspects = JSON.parse(aspectsArg.v) as Aspects
      }
      if (attrsArg && attrsArg.t === 'pair' && typeof attrsArg.v === 'string') {
        Object.assign(attrs, JSON.parse(attrsArg.v) as Record<string, string | number | boolean>)
      }
      addEntity(ctx.state, { id, kind, name, aspects, attrs, bornAction: ctx.recordId, deadAction: null })
      // concrete outcome: everything a fold needs, nothing from the step context
      const concrete: Arg[] = [{ t: 'ref', ref: nameRef(name, kind) }]
      if (container) concrete.push({ t: 'into', ref: idRef(container) })
      concrete.push({ t: 'asId', id })
      if (Object.keys(aspects).length > 0) concrete.push({ t: 'pair', k: 'aspects', v: JSON.stringify(aspects) })
      if (Object.keys(attrs).length > 0) concrete.push({ t: 'pair', k: 'attrs', v: JSON.stringify(attrs) })
      return { concrete: { op: 'SPAWN', args: concrete }, refused: false }
    }
    case 'DESTROY': {
      for (const e of expand(args, ctx)) e.deadAction = ctx.recordId
      return { concrete: o, refused: false }
    }
    case 'SET': {
      const key = args.find((a): a is Extract<Arg, { t: 'str' }> => a.t === 'str')
      const val = args.find((a): a is Extract<Arg, { t: 'int' | 'str' | 'bool' }> => a.t === 'int' || a.t === 'str' || a.t === 'bool')
      if (!key || !val) throw new Error('SET needs (ref, key, value)')
      for (const e of expand(args, ctx)) {
        e.attrs[key.v] = val.v
        if (key.v === 'expires') e.attrs['expiresAt'] = ctx.now + toMinutes(val.v)
      }
      return { concrete: o, refused: false }
    }
    case 'MOVE': {
      const into = args.find((a): a is Extract<Arg, { t: 'into' }> => a.t === 'into')
      if (!into) throw new Error('MOVE needs INTO')
      const dest = 'role' in into.ref ? ctx.bindings[into.ref.role]?.[0]?.id : resolveRef(ctx.state, into.ref)?.id
      if (!dest) throw new Error('MOVE: unresolvable destination')
      for (const e of expand(args, ctx)) e.attrs['container'] = dest
      return { concrete: o, refused: false }
    }
    case 'REVEAL': {
      for (const e of expand(args, ctx)) e.attrs['state.hidden'] = false
      return { concrete: o, refused: false }
    }
    case 'ADVANCE': {
      const d = args[0]
      if (!d || d.t !== 'dur') throw new Error('ADVANCE needs a duration')
      ctx.extraAdvanceMin += d.v
      return { concrete: o, refused: false }
    }
    case 'NARRATE':
      return { concrete: o, refused: false }
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
      return { concrete: o, refused: false }
    }
    case 'UNRELATE': {
      const refs = args.filter((a): a is Extract<Arg, { t: 'ref' }> => a.t === 'ref')
      if (refs.length < 3) throw new Error('UNRELATE needs (ref, ref, type)')
      const a = resolveRef(ctx.state, refs[0].ref), b = resolveRef(ctx.state, refs[1].ref)
      const type = refs[2].ref.form === 'name' ? refs[2].ref.name : null
      if (a && b && type) {
        const def = ctx.dict.relationDef(type)
        ctx.state.relations.delete(relationKey(a.id, b.id, type, def.symmetric))
      }
      return { concrete: o, refused: false }
    }
    case 'REFUSE': {
      return { concrete: o, refused: true }
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
  state.now = absMin(parseClock(record.tGame))
  // rebuild role bindings from the record itself so role-ref outcomes
  // (MOVE ACTORS INTO OBJECT …) replay without the original step context
  const one = (ref: Ref | null): Entity[] => {
    if (!ref) return []
    const e = resolveRef(state, ref)
    return e ? [e] : []
  }
  const idCounter = { n: 0 }
  const ctx: ApplyCtx = {
    state, dict, content,
    bindings: {
      actors: record.actors.flatMap(a => one(a)),
      object: record.objects.flatMap(a => one(a)),
      with: record.instruments.flatMap(a => one(a)),
      at: one(record.at),
      about: one(record.about),
      to: [],
    },
    now: absMin(parseClock(record.tGame)),
    recordId: record.id,
    idGen: () => `r${record.id}s${idCounter.n++}`, // deterministic fallback for AS-less SPAWNs
    extraAdvanceMin: 0,
  }
  for (const o of record.outcomes) applyOutcome(o, ctx)
  state.now += record.advancedMin
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
  for (const absHour of crossedHours(fromMin, toMin)) {
    // authored triggers first, then expiries — deterministic order (§9.16)
    for (const trig of content.triggers) {
      if (trig.hour !== absHour % 24) continue
      const ctx = makeEventCtx(state, dict, content, absHour, firstEventId + records.length, idGen)
      const outcomes: Outcome[] = []
      for (const o of trig.out) outcomes.push(applyOutcome(o, ctx).concrete)
      records.push(makeEventRecord(firstEventId + records.length, trig.id, absHour, outcomes, dict))
    }
    for (const e of [...state.entities.values()]) {
      if (e.deadAction !== null) continue
      const expAt = e.attrs['expiresAt']
      if (typeof expAt !== 'number' || expAt > absHour * 60) continue
      const ctx = makeEventCtx(state, dict, content, absHour, firstEventId + records.length, idGen)
      const destroy: Outcome = { op: 'DESTROY', args: [{ t: 'ref', ref: nameRef(e.name) }] }
      const outcomes = [applyOutcome(destroy, ctx).concrete]
      records.push(makeEventRecord(firstEventId + records.length, `expire:${e.name}`, absHour, outcomes, dict))
    }
  }
  return records
}

function makeEventCtx(state: State, dict: Dictionary, content: CompiledContent, absHour: number, recordId: number, idGen: () => string): ApplyCtx {
  return {
    state, dict, content,
    bindings: {} as RoleBindings,
    now: absHour * 60,
    recordId,
    idGen,
    extraAdvanceMin: 0,
  }
}

function makeEventRecord(id: number, trigger: string, absHour: number, outcomes: Outcome[], dict: Dictionary): ActionRecord {
  const clock = clockFromAbsMin(absHour * 60)
  const rec: ActionRecord = {
    id, kind: 'event', trigger,
    tGame: formatClock(clock), tReal: null, advancedMin: 0,
    actors: [], verb: '', objects: [], instruments: [], at: null, about: null,
    outcomes, recipes: [],
    sentence: '',
  }
  rec.sentence = formatSentence({ kind: 'event', clock, trigger, clauses: [], outcomes }, dict)
  return rec
}
