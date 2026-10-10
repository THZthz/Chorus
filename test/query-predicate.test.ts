// Query mini-language (§4.5) and condition predicates (§6.3)
import { describe, expect, test } from 'vitest'
import { parseQuery, matchQuery } from '../src/query.js'
import { parsePredicate, evalPred, type PredCtx } from '../src/predicate.js'
import { newState, addEntity, type Entity, type State } from '../src/types.js'

function ent(partial: Partial<Entity> & { id: string; kind: string; name: string }): Entity {
  return { aspects: {}, attrs: {}, bornAction: null, deadAction: null, ...partial }
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
