// Query mini-language (§4.5) and condition predicates (§6.3)
import { describe, expect, test } from 'vitest'
import { parseQuery, matchQuery } from '../src/core/query.js'
import { parsePredicate, evalPred, type PredCtx } from '../src/core/predicate.js'
import { newState, addEntity, type Entity, type State } from '../src/core/types.js'

function ent(partial: Partial<Entity> & { id: string; kind: string; name: string }): Entity {
  return { aspects: {}, attrs: {}, bornAction: null, deadAction: null, stackable: false, ...partial }
}

describe('slot queries (§4.5)', () => {
  const q = parseQuery('kind:text aspect:secret>=3 aspect:language.latin>=2 state:!sealed -aspect:winter')

  test('a matching card passes every term', () => {
    const book = ent({ id: 'b', kind: 'text', name: 'B', aspects: { secret: 3, language: { latin: 2 } } })
    expect(matchQuery(book, q)).toBe(true)
  })

  test('each term can fail the card', () => {
    expect(matchQuery(ent({ id: 'x', kind: 'item', name: 'X' }), q)).toBe(false) // kind
    expect(matchQuery(ent({ id: 'x', kind: 'text', name: 'X', aspects: { secret: 2 } }), q)).toBe(false) // level
    expect(matchQuery(ent({ id: 'x', kind: 'text', name: 'X', aspects: { secret: 3, language: { latin: 2 }, winter: 1 } }), q)).toBe(false) // negation
    const sealed = ent({ id: 'x', kind: 'text', name: 'X', aspects: { secret: 3, language: { latin: 2 } }, attrs: { 'state.sealed': true } })
    expect(matchQuery(sealed, q)).toBe(false)
  })

  test('bare aspect: term means level >= 1', () => {
    const q1 = parseQuery('aspect:lantern')
    expect(matchQuery(ent({ id: 'a', kind: 'character', name: 'A', aspects: { lantern: 1 } }), q1)).toBe(true)
    expect(matchQuery(ent({ id: 'a', kind: 'character', name: 'A', aspects: {} }), q1)).toBe(false)
  })
})

describe('condition predicates (§6.3)', () => {
  const state: State = newState()
  addEntity(state, ent({ id: 'ana', kind: 'character', name: 'ANA', aspects: { occult: 2, lantern: 1 } }))
  addEntity(state, ent({ id: 'bertha', kind: 'character', name: 'BERTHA', aspects: { occult: 0 } }))
  addEntity(state, ent({ id: 'rite', kind: 'rite', name: 'RITE', aspects: { demand: 3 } }))
  state.relations.set('ana|bertha|parent_child', { a: 'ana', b: 'bertha', type: 'parent_child', value: 1, bornAction: 0, deadAction: null })

  const ctx = (b: PredCtx['b']): PredCtx => ({ b, state })
  const A = state.entities.get('ana')!, B = state.entities.get('bertha')!, R = state.entities.get('rite')!

  test('count(actors) = 2', () => {
    const p = parsePredicate('count(actors) = 2')
    expect(evalPred(p, ctx({ actors: [A, B] }))).toBe(true)
    expect(evalPred(p, ctx({ actors: [A] }))).toBe(false)
  })

  test('rel(parent_child, actors) — pairwise existence', () => {
    const p = parsePredicate('rel(parent_child, actors)')
    expect(evalPred(p, ctx({ actors: [A, B] }))).toBe(true)
    expect(evalPred(p, ctx({ actors: [B, A] }))).toBe(true) // symmetric, order-free
    expect(evalPred(p, ctx({ actors: [A, R] }))).toBe(false)
  })

  test('every/any quantifiers', () => {
    const every = parsePredicate('every(actors, aspect:occult >= 2)')
    const any = parsePredicate('any(actors, aspect:occult >= 2)')
    expect(evalPred(every, ctx({ actors: [A] }))).toBe(true)
    expect(evalPred(every, ctx({ actors: [A, B] }))).toBe(false)
    expect(evalPred(any, ctx({ actors: [A, B] }))).toBe(true)
    expect(evalPred(any, ctx({ actors: [B] }))).toBe(false)
  })

  test('aggregate vs path operand: total(actors, …) < object.aspect.demand', () => {
    const p = parsePredicate('total(actors, aspect:lantern) < object.aspect.demand')
    expect(evalPred(p, ctx({ actors: [A], object: [R] }))).toBe(true) // 1 < 3
    expect(evalPred(p, ctx({ actors: [A, B], object: [R] }))).toBe(true) // 1+0 < 3
  })

  test('NOT binds tighter than AND (§6.3)', () => {
    const p = parsePredicate('NOT 1 = 2 AND 2 = 2')
    expect(evalPred(p, ctx({}))).toBe(true)
  })

  test('group gate with NOT any — the magick gate shape', () => {
    const p = parsePredicate('object.aspect.magick >= 1 AND NOT any(actors, aspect:occult >= 2)')
    const magick = ent({ id: 'm', kind: 'text', name: 'M', aspects: { magick: 1 } })
    expect(evalPred(p, ctx({ actors: [B], object: [magick] }))).toBe(true) // nobody knows
    expect(evalPred(p, ctx({ actors: [A, B], object: [magick] }))).toBe(false) // ANA knows
  })
})

describe('exists(...) — world-existential conditions (§6.3, CS extantreqs)', () => {
  const world = (): State => {
    const s = newState()
    // the hunter prowls the villa; nothing else in play has hunter/secret
    addEntity(s, ent({ id: 'hunter', kind: 'character', name: 'HUNTER', aspects: { hunter: 2 }, attrs: { container: 'villa' } }))
    addEntity(s, ent({ id: 'rite', kind: 'rite', name: 'RITE_OF_DAWN', aspects: { ritual: 1 }, attrs: { container: 'house' } }))
    return s
  }
  const ctxIn = (s: State) => (b: PredCtx['b']): PredCtx => ({ b, state: s })

  test('name form: true while the named card is alive, false after DESTROY', () => {
    const s = world()
    const p = parsePredicate('exists(HUNTER)')
    expect(evalPred(p, ctxIn(s)({}))).toBe(true) // no roles bound at all
    s.entities.get('hunter')!.deadAction = 1 // DESTROY stamps deadAction…
    s.byName.delete('HUNTER')                // …and releases the name (§9.14)
    expect(evalPred(p, ctxIn(s)({}))).toBe(false)
    expect(evalPred(parsePredicate('NOT exists(HUNTER)'), ctxIn(s)({}))).toBe(true)
  })

  test('name form is exact and never matches the dead or the absent', () => {
    const s = world()
    expect(evalPred(parsePredicate('exists(hunter)'), ctxIn(s)({}))).toBe(false) // case-exact, like byName
    expect(evalPred(parsePredicate('exists(THE_SEALED_DOOR)'), ctxIn(s)({}))).toBe(false)
    const dead = ent({ id: 'ghost', kind: 'idea', name: 'GHOST' })
    dead.deadAction = 3
    addEntity(s, dead) // a dead entity never enters byName
    expect(evalPred(parsePredicate('exists(GHOST)'), ctxIn(s)({}))).toBe(false)
  })

  test('query form: aspect threshold', () => {
    const s = world()
    expect(evalPred(parsePredicate('exists(aspect:hunter>=1)'), ctxIn(s)({}))).toBe(true)
    expect(evalPred(parsePredicate('exists(aspect:hunter >= 3)'), ctxIn(s)({}))).toBe(false) // spaces tighten to >=3
    expect(evalPred(parsePredicate('exists(aspect:secret)'), ctxIn(s)({}))).toBe(false) // bare term = level >= 1
  })

  test('query form: kind+aspect conjunction', () => {
    const s = world()
    expect(evalPred(parsePredicate('exists(kind:rite aspect:ritual>=1)'), ctxIn(s)({}))).toBe(true)
    expect(evalPred(parsePredicate('exists(kind:rite aspect:ritual>=2)'), ctxIn(s)({}))).toBe(false)
    expect(evalPred(parsePredicate('exists(kind:character aspect:ritual>=1)'), ctxIn(s)({}))).toBe(false)
  })

  test('query form: negation term', () => {
    const s = world()
    expect(evalPred(parsePredicate('exists(kind:rite -aspect:ritual)'), ctxIn(s)({}))).toBe(false) // the only rite has ritual
    expect(evalPred(parsePredicate('exists(-aspect:ritual)'), ctxIn(s)({}))).toBe(true) // the hunter lacks it
  })

  test('query form: state: terms read the state.* attr namespace', () => {
    const s = world()
    expect(evalPred(parsePredicate('exists(state:sealed)'), ctxIn(s)({}))).toBe(false)
    expect(evalPred(parsePredicate('exists(-state:sealed)'), ctxIn(s)({}))).toBe(true) // nobody is sealed
    s.entities.get('rite')!.attrs['state.sealed'] = true
    expect(evalPred(parsePredicate('exists(state:sealed)'), ctxIn(s)({}))).toBe(true)
    expect(evalPred(parsePredicate('exists(kind:rite state:sealed)'), ctxIn(s)({}))).toBe(true)
    expect(evalPred(parsePredicate('exists(kind:character state:sealed)'), ctxIn(s)({}))).toBe(false) // hunter unsealed
    expect(evalPred(parsePredicate('exists(-state:sealed)'), ctxIn(s)({}))).toBe(true) // …but still matches the negation
  })

  test('NOT binds tighter than AND around exists (§6.3 precedence)', () => {
    const s = world()
    // HUNTER is alive and nothing has ritual>=5: the spec grouping
    // (NOT exists(HUNTER)) AND exists(aspect:ritual>=5) is false —
    // the wrong reading NOT (exists AND exists) would be true here
    expect(evalPred(parsePredicate('NOT exists(HUNTER) AND exists(aspect:ritual>=5)'), ctxIn(s)({}))).toBe(false)
    // flip an operand and it turns true, proving both sides are read as written
    expect(evalPred(parsePredicate('NOT exists(GHOST) AND exists(aspect:ritual>=1)'), ctxIn(s)({}))).toBe(true)
  })

  test('scope: finds a card bound to no role, sitting in another place than the actors', () => {
    const s = world()
    // actors = the rite at the house; the hunter is in the villa and in no role
    const p = parsePredicate('exists(aspect:hunter>=1) OR exists(HUNTER)')
    expect(evalPred(p, ctxIn(s)({ actors: [s.entities.get('rite')!] }))).toBe(true)
  })

  test('disambiguation: a starter word without a colon is a NAME, not a query', () => {
    const s = world()
    expect(evalPred(parsePredicate('exists(state)'), ctxIn(s)({}))).toBe(false) // no card named 'state'
    expect(evalPred(parsePredicate('exists(kind)'), ctxIn(s)({}))).toBe(false)
  })

  test('composability and parse errors', () => {
    const s = world()
    const yes = parsePredicate('exists(HUNTER) AND NOT exists(aspect:ritual>=5)')
    expect(evalPred(yes, ctxIn(s)({}))).toBe(true)
    expect(evalPred(parsePredicate('NOT exists(HUNTER) OR exists(aspect:ritual>=5)'), ctxIn(s)({}))).toBe(false)
    expect(() => parsePredicate('exists()')).toThrow(/needs a name or a slot query/)
    expect(() => parsePredicate('exists(TWO WORDS)')).toThrow(/takes one name/)
  })
})

describe('the name: query term and count(<query>) (§9.38)', () => {
  const world = (): State => {
    const s = newState()
    // a two-copy SUSPICION pool plus an unrelated card
    addEntity(s, ent({ id: 'sus1', kind: 'state', name: 'SUSPICION', aspects: { weight: 1 }, stackable: true }))
    addEntity(s, ent({ id: 'sus2', kind: 'state', name: 'SUSPICION', aspects: { weight: 1 }, attrs: { container: 'sus1' }, stackable: true }))
    addEntity(s, ent({ id: 'rite', kind: 'rite', name: 'RITE', aspects: { ritual: 1 } }))
    return s
  }
  const ctxIn = (s: State) => (b: PredCtx['b']): PredCtx => ({ b, state: s })

  test('name: matches exactly (case-exact, like exists)', () => {
    const s = world()
    expect(matchQuery(s.entities.get('sus1')!, parseQuery('name:SUSPICION'))).toBe(true)
    expect(matchQuery(s.entities.get('rite')!, parseQuery('name:SUSPICION'))).toBe(false)
    expect(matchQuery(s.entities.get('sus1')!, parseQuery('name:suspicion'))).toBe(false) // case-exact
    expect(matchQuery(s.entities.get('rite')!, parseQuery('-name:SUSPICION'))).toBe(true) // negation
  })

  test('name: composes with other terms and negation composes at the term level', () => {
    const s = world()
    expect(evalPred(parsePredicate('exists(kind:state name:SUSPICION)'), ctxIn(s)({}))).toBe(true)
    expect(evalPred(parsePredicate('exists(kind:rite name:SUSPICION)'), ctxIn(s)({}))).toBe(false)
    expect(evalPred(parsePredicate('exists(-name:SUSPICION)'), ctxIn(s)({}))).toBe(true) // the rite
    expect(evalPred(parsePredicate('exists(kind:state -name:SUSPICION)'), ctxIn(s)({}))).toBe(false) // both copies match
  })

  test('count(name:x) counts every living copy; count(-name:x) counts the rest of the world', () => {
    const s = world()
    expect(evalPred(parsePredicate('count(name:SUSPICION) = 2'), ctxIn(s)({}))).toBe(true)
    expect(evalPred(parsePredicate('count(name:SUSPICION) >= 3'), ctxIn(s)({}))).toBe(false)
    expect(evalPred(parsePredicate('count(-name:SUSPICION) = 1'), ctxIn(s)({}))).toBe(true) // the rite only
    expect(evalPred(parsePredicate('count(name:RITE) = 1'), ctxIn(s)({}))).toBe(true)
  })

  test('count(<query>) composes full slot queries; count(role) is unchanged', () => {
    const s = world()
    expect(evalPred(parsePredicate('count(kind:state aspect:weight>=1) = 2'), ctxIn(s)({}))).toBe(true)
    const A = s.entities.get('sus1')!
    expect(evalPred(parsePredicate('count(actors) = 1'), ctxIn(s)({ actors: [A] }))).toBe(true) // role form untouched
  })

  test('count() with a non-role non-query argument is a parse-shaped role — the lint fences it at load', () => {
    const s = world()
    // parseable (it looks like a role), evaluates to 0 — which is why
    // compileContent rejects it (see test/stackable.test.ts lint tests)
    expect(evalPred(parsePredicate('count(DREAD) = 0'), ctxIn(s)({}))).toBe(true)
  })

  test('a comparison inside a name: term is a load error — it would silently bake the NAME (§9.38)', () => {
    // the exists()/count() glue-tightening turns 'SUSPICION >= 1' into the
    // single token 'SUSPICION>=1'; as a name that matches nothing (silent 0)
    expect(() => parseQuery('name:SUSPICION>=1')).toThrow(/bad name term/)
    expect(() => parsePredicate('count(name:SUSPICION >= 1) >= 1')).toThrow(/bad name term/)
    expect(() => parsePredicate('exists(name:SUSPICION >= 1)')).toThrow(/bad name term/)
    // total() never accepted the query form — now it says so instead of
    // failing with a cryptic `expected ','`
    expect(() => parsePredicate('total(name:SUSPICION, aspect:weight) >= 1')).toThrow(/total\(\) pools a bound role/)
  })
})
