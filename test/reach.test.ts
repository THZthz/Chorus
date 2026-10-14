// Static card-reachability (src/reach.ts): both semantics — (a) existence and
// (b) usability — over hand-built ContentRaw fixtures. The conservative
// direction is the contract: 'unobtainable' only on structural absence;
// anything guarded demotes to 'unproven'.
import { describe, expect, test } from 'vitest'
import { compileContent, type ContentRaw } from '../src/content/content.js'
import { coreDictionary } from '../src/core/dict.js'
import { analyzeCard } from '../src/reach/reach.js'
import { addEntity, newState, type Entity, type State } from '../src/core/types.js'

const dict = coreDictionary()

const ent = (name: string, kind: string, aspects: Entity['aspects'] = {}): Entity => ({
  id: `s_${name.toLowerCase()}`,
  kind,
  name,
  aspects,
  attrs: {},
  bornAction: 0,
  deadAction: null,
  stackable: false,
})

const stateWith = (...es: Entity[]): State => {
  const s = newState()
  for (const e of es) addEntity(s, e)
  return s
}

const compile = (raw: ContentRaw) => compileContent(raw, dict)

const ANA = () => ent('ANA', 'character')
const BOOK = () => ent('THE_BOOK', 'text', { secret: 2 })

// ---------- (a) existence ----------

describe('existence semantics (a)', () => {
  test('an initial entity is proven obtainable; a name with no source is structurally unobtainable', () => {
    const content = compile({ verbs: {} })
    const state = stateWith(ANA(), BOOK())
    const book = analyzeCard(content, state, 'THE_BOOK')
    expect(book.verdict).toBe('obtainable')
    expect(book.existence).toBe('proven')
    expect(book.chain[0]).toContain('initial entities')

    expect(analyzeCard(content, state, 'GHOST_CARD').verdict).toBe('unobtainable')
  })

  test('a deck hit behind a slot-trigger DRAW is proven — the order is seed-fixed at load', () => {
    const content = compile({
      prototypes: { A_CARD: { kind: 'item' }, B_CARD: { kind: 'item' } },
      decks: [{ id: 'BOX', spec: ['A_CARD', 'B_CARD'] }],
      triggers: [{ id: 'deal', slot: 2, out: ['DRAW BOX'] }],
      verbs: {},
    })
    const state = stateWith(ANA())
    const b = analyzeCard(content, state, 'B_CARD')
    expect(b.verdict).toBe('obtainable')
    expect(b.chain[0]).toContain("deck BOX serves 'B_CARD'")
    expect(b.chain.join('\n')).toContain('trigger deal fires on slot 2')

    expect(analyzeCard(content, state, 'C_CARD').verdict).toBe('unobtainable')
  })

  test('deck capacity: one once-trigger draw serves only order[0] — deeper positions are never served', () => {
    const content = compile({
      prototypes: { A_CARD: { kind: 'item' }, B_CARD: { kind: 'item' } },
      decks: [{ id: 'BOX', spec: ['A_CARD', 'B_CARD'] }],
      triggers: [{ id: 'deal', slot: 2, once: true, out: ['DRAW BOX'] }],
      verbs: {},
    })
    const order = content.decks.get('BOX')!.order
    expect(analyzeCard(content, stateWith(ANA()), order[0]!.name).verdict).toBe('obtainable')
    // nothing else can draw, so the deeper position is structurally unserved
    expect(analyzeCard(content, stateWith(ANA()), order[1]!.name).verdict).toBe('unobtainable')
  })

  test('a duplicate non-stackable mint in one list refuses the draft — every site in it demotes (fuzz seed 13)', () => {
    const mk = (dup: boolean) => compile({
      prototypes: { P1: { kind: 'item' }, P2: { kind: 'idea' } },
      decks: [{ id: 'D1', spec: ['P1'] }],
      verbs: {
        MANSUS: {
          slots: [
            { role: 'actors', min: 1, max: 1, query: 'kind:character' },
            { role: 'object', min: 1, max: 1, query: 'kind:text' },
          ],
          duration: 30,
          recipes: [{ out: [dup ? 'DRAW D1, SPAWN P1' : 'DRAW D1, SPAWN P2'] }],
        },
      },
    })
    // the DRAW deterministically mints P1 (order[0] of a single-card deck) →
    // the trailing SPAWN P1 collides → completion refusal → P1 never survives
    const risky = analyzeCard(mk(true), stateWith(ANA(), BOOK()), 'P1')
    expect(risky.verdict).toBe('unproven')
    expect(risky.notes.join(' ')).toContain('collide')
    // control: the SPAWN mints a DIFFERENT name — the draw site stays proven
    expect(analyzeCard(mk(false), stateWith(ANA(), BOOK()), 'P1').verdict).toBe('obtainable')
  })

  test('a deck nobody draws is not a source', () => {
    const content = compile({
      prototypes: { A_CARD: { kind: 'item' } },
      decks: [{ id: 'BOX', spec: ['A_CARD'] }],
      verbs: {},
    })
    expect(analyzeCard(content, stateWith(ANA()), 'A_CARD').verdict).toBe('unobtainable')
  })

  test('SPAWN via a default recipe on a committable verb — the chain names the slot supports', () => {
    const content = compile({
      prototypes: { PILLAR: { kind: 'item' } },
      verbs: {
        EXPLORE: {
          slots: [{ role: 'actors', min: 1, max: 1, query: 'kind:character' }],
          duration: 30,
          recipes: [{ out: ['SPAWN PILLAR'] }],
        },
      },
    })
    const r = analyzeCard(content, stateWith(ANA()), 'PILLAR')
    expect(r.verdict).toBe('obtainable')
    expect(r.chain[0]).toContain('SPAWN mints')
    expect(r.chain.join('\n')).toContain('EXPLORE committable: actors ×1 ← initial entity ANA')
  })

  test('a role-dependent `when` demotes to unproven — never a false unobtainable', () => {
    const content = compile({
      prototypes: { PILLAR: { kind: 'item' } },
      verbs: {
        EXPLORE: {
          slots: [{ role: 'actors', min: 1, max: 1, query: 'kind:character' }],
          duration: 30,
          recipes: [{ when: 'actors.aspect.occult >= 1', out: ['SPAWN PILLAR'] }],
        },
      },
    })
    const r = analyzeCard(content, stateWith(ANA()), 'PILLAR')
    expect(r.verdict).toBe('unproven')
    expect(r.existence).toBe('candidate')
    expect(r.notes.join(' ')).toContain('guarded')
  })

  test('a world-static `when` that already holds stays proven', () => {
    const content = compile({
      prototypes: { PILLAR: { kind: 'item' } },
      verbs: {
        EXPLORE: {
          slots: [{ role: 'actors', min: 1, max: 1, query: 'kind:character' }],
          duration: 30,
          recipes: [{ when: 'exists(THE_BOOK)', out: ['SPAWN PILLAR'] }],
        },
      },
    })
    const r = analyzeCard(content, stateWith(ANA(), BOOK()), 'PILLAR')
    expect(r.verdict).toBe('obtainable')
  })

  test('an uncommittable verb is a dead producer — structural absence', () => {
    const content = compile({
      prototypes: { PILLAR: { kind: 'item' } },
      verbs: {
        EXPLORE: {
          slots: [
            { role: 'actors', min: 1, max: 1, query: 'kind:character' },
            { role: 'object', min: 1, max: 1, query: 'kind:rite' },
          ],
          duration: 30,
          recipes: [{ out: ['SPAWN PILLAR'] }],
        },
      },
    })
    expect(analyzeCard(content, stateWith(ANA()), 'PILLAR').verdict).toBe('unobtainable')
  })

  test('the fixpoint chains mints through slots: deck card → slot → second-stage mint', () => {
    const content = compile({
      prototypes: {
        KEY_CARD: { kind: 'item' },
        PILLAR: { kind: 'item' },
      },
      decks: [{ id: 'BOX', spec: ['KEY_CARD'] }],
      triggers: [{ id: 'deal', slot: 2, out: ['DRAW BOX'] }],
      verbs: {
        EXPLORE: {
          slots: [
            { role: 'actors', min: 1, max: 1, query: 'kind:character' },
            { role: 'object', min: 1, max: 1, query: 'kind:item' },
          ],
          duration: 30,
          recipes: [{ out: ['SPAWN PILLAR'] }],
        },
      },
    })
    const r = analyzeCard(content, stateWith(ANA()), 'PILLAR')
    expect(r.verdict).toBe('obtainable')
    expect(r.chain.join('\n')).toContain('KEY_CARD')
  })

  test('an authored REFUSE upstream kills the site (the apply loop stops there)', () => {
    const content = compile({
      prototypes: { PILLAR: { kind: 'item' } },
      verbs: {
        EXPLORE: {
          slots: [{ role: 'actors', min: 1, max: 1, query: 'kind:character' }],
          duration: 30,
          recipes: [{ out: ['REFUSE("not yet"), SPAWN PILLAR'] }],
        },
      },
    })
    expect(analyzeCard(content, stateWith(ANA()), 'PILLAR').verdict).toBe('unobtainable')
  })

  test('a derived-mint shape is never structurally absent', () => {
    const content = compile({
      prototypes: { RUMOUR: { kind: 'idea', derive: { nameFrom: 'object' } } },
      verbs: {},
    })
    const r = analyzeCard(content, stateWith(ANA()), 'RUMOUR_HOUSE')
    expect(r.verdict).toBe('unproven')
    expect(r.notes.join(' ')).toContain('derived-mint')
  })
})

// ---------- mixed sources & segment discipline ----------

describe('mixed sources & segment discipline', () => {
  test('mixture: a deck-fed card satisfies an IF exists(...) guard gating a SPAWN — fixpoint re-evaluates path conditions', () => {
    const content = compile({
      prototypes: { KEY_CARD: { kind: 'item' }, PILLAR: { kind: 'item' } },
      decks: [{ id: 'BOX', spec: ['KEY_CARD'] }],
      triggers: [{ id: 'deal', slot: 2, out: ['DRAW BOX'] }],
      verbs: {
        EXPLORE: {
          slots: [{ role: 'actors', min: 1, max: 1, query: 'kind:character' }],
          duration: 30,
          recipes: [{ out: ['IF exists(KEY_CARD) -> (SPAWN PILLAR)'] }],
        },
      },
    })
    const r = analyzeCard(content, stateWith(ANA()), 'PILLAR')
    expect(r.verdict).toBe('obtainable')
    expect(r.chain.join('\n')).toContain('SPAWN mints')
  })

  test('mixture: a slot needing TWO items sums across sources (initial + deck)', () => {
    // without the deck there is only ROPE — one of two — so the verb never
    // commits; the site is not structurally dead either (a KEY_CARD proto
    // exists in the universe), and the honest verdict is unproven
    const base: ContentRaw = {
      prototypes: { KEY_CARD: { kind: 'item' }, PILLAR: { kind: 'item' } },
      verbs: {
        EXPLORE: {
          slots: [
            { role: 'actors', min: 1, max: 1, query: 'kind:character' },
            { role: 'object', min: 2, max: 2, query: 'kind:item' },
          ],
          duration: 30,
          recipes: [{ out: ['SPAWN PILLAR'] }],
        },
      },
    }
    expect(analyzeCard(compile(base), stateWith(ANA(), ent('ROPE', 'item')), 'PILLAR').verdict).toBe('unproven')

    // add the deck: ROPE (initial) + KEY_CARD (drawn) crosses the min —
    // the chain names both sources from DIFFERENT entry paths
    const mixed: ContentRaw = { ...base, decks: [{ id: 'BOX', spec: ['KEY_CARD'] }], triggers: [{ id: 'deal', slot: 2, out: ['DRAW BOX'] }] }
    const full = analyzeCard(compile(mixed), stateWith(ANA(), ent('ROPE', 'item')), 'PILLAR')
    expect(full.verdict).toBe('obtainable')
    expect(full.chain.join('\n')).toContain('ROPE')
    expect(full.chain.join('\n')).toContain('KEY_CARD')
  })

  test('segment scoping: a REFUSE inside an IF arm does NOT kill the outer list', () => {
    const content = compile({
      prototypes: { PILLAR: { kind: 'item' } },
      verbs: {
        EXPLORE: {
          slots: [{ role: 'actors', min: 1, max: 1, query: 'kind:character' }],
          duration: 30,
          recipes: [{ out: ['IF exists(THE_BOOK) -> (REFUSE("blocked")), SPAWN PILLAR'] }],
        },
      },
    })
    // THE_BOOK is absent, so the arm never runs and the outer SPAWN applies
    const r = analyzeCard(content, stateWith(ANA()), 'PILLAR')
    expect(r.verdict).toBe('obtainable')
  })

  test('a top-level REFUSE kills the WHOLE list — earlier SPAWNs included (C3: the draft is discarded)', () => {
    const content = compile({
      prototypes: { PILLAR: { kind: 'item' } },
      verbs: {
        EXPLORE: {
          slots: [{ role: 'actors', min: 1, max: 1, query: 'kind:character' }],
          duration: 30,
          recipes: [{ out: ['SPAWN PILLAR, REFUSE("stop")'] }],
        },
      },
    })
    expect(analyzeCard(content, stateWith(ANA()), 'PILLAR').verdict).toBe('unobtainable')
  })

  test('a throwing op anywhere in the list demotes every site in it — unproven, never a false unobtainable', () => {
    const content = compile({
      prototypes: { PILLAR: { kind: 'item' } },
      verbs: {
        EXPLORE: {
          slots: [{ role: 'actors', min: 1, max: 1, query: 'kind:character' }],
          duration: 30,
          recipes: [{ out: ['SPAWN PILLAR, MOVE ANA INTO NOWHERE'] }],
        },
      },
    })
    const r = analyzeCard(content, stateWith(ANA()), 'PILLAR')
    expect(r.verdict).toBe('unproven')
    expect(r.notes.join(' ')).toContain('throwing op')
  })

  test('REPLACE is a producer of its WITH target', () => {
    const content = compile({
      prototypes: { ASH: { kind: 'item' } },
      verbs: {
        USE_ON: {
          slots: [{ role: 'actors', min: 1, max: 1, query: 'kind:character' }],
          duration: 30,
          recipes: [{ out: ['REPLACE THE_BOOK WITH ASH'] }],
        },
      },
    })
    const r = analyzeCard(content, stateWith(ANA(), BOOK()), 'ASH')
    expect(r.verdict).toBe('obtainable')
    expect(r.chain[0]).toContain('REPLACE mints')
  })
})

// ---------- (c) demand semantics: attribute thresholds ----------

describe('demand semantics (attribute thresholds, semantics (c))', () => {
  const bookWorld = () => {
    const content = compile({ verbs: {} })
    const state = stateWith(ANA(), BOOK())   // THE_BOOK: text, secret:2
    return { content, state }
  }

  test('a threshold satisfiable by a static descriptor is proven — across comparison ops', () => {
    for (const demand of ['aspect:secret>=2', 'aspect:secret>1', 'aspect:secret=2', 'aspect:secret<=2', 'aspect:secret!=3']) {
      const { content, state } = bookWorld()
      const r = analyzeCard(content, state, '', { demand })
      expect(r.verdict, demand).toBe('obtainable')
      expect(r.chain[0], demand).toContain('THE_BOOK')
    }
  })

  test('a bounded threshold nothing can write is soundly unobtainable', () => {
    const { content, state } = bookWorld()
    const r = analyzeCard(content, state, '', { demand: 'aspect:secret<=1' })
    expect(r.verdict).toBe('unobtainable')
    expect(r.existence).toBe('none')
  })

  test('a SET pump keeps an unmatched threshold from being called impossible', () => {
    const content = compile({
      verbs: {
        USE_ON: {
          slots: [
            { role: 'actors', min: 1, max: 1, query: 'kind:character' },
            { role: 'object', min: 1, max: 1, query: 'kind:item' },
          ],
          duration: 30,
          recipes: [{ out: ['SET(OBJECT, aspect:charged, +3)'] }],
        },
      },
    })
    const state = stateWith(ANA(), ent('LAMP', 'item', { charged: 0 }))
    const r = analyzeCard(content, state, '', { demand: 'aspect:charged>=2' })
    expect(r.verdict).toBe('unproven')
    expect(r.notes.join(' ')).toContain('pumped')
  })

  test('a pumped threshold un-deads a verb slot no static card could fill — and absence stays sound without the pump', () => {
    const mk = (withPump: boolean) => compile({
      prototypes: { PILLAR: { kind: 'item' } },
      verbs: {
        EXPLORE: {
          slots: [
            { role: 'actors', min: 1, max: 1, query: 'kind:character' },
            { role: 'object', min: 1, max: 1, query: 'kind:item aspect:charged>=3' },
          ],
          duration: 30,
          recipes: [{ out: ['SPAWN PILLAR'] }],
        },
        ...(withPump ? {
          USE_ON: {
            slots: [
              { role: 'actors', min: 1, max: 1, query: 'kind:character' },
              { role: 'object', min: 1, max: 1, query: 'kind:item' },
            ],
            duration: 30,
            recipes: [{ out: ['SET(OBJECT, aspect:charged, +3)'] }],
          },
        } : {}),
      },
    })
    const state = stateWith(ANA(), ent('LAMP', 'item', { charged: 0 }))
    // no SET writes aspect:charged anywhere → the threshold is bounded away → dead
    expect(analyzeCard(mk(false), state, 'PILLAR').verdict).toBe('unobtainable')
    // the pump keeps the slot undecided → candidate, never a false unobtainable
    expect(analyzeCard(mk(true), state, 'PILLAR').verdict).toBe('unproven')
  })

  test('a derive-fed threshold cannot be bounded (derive computes from the object at mint)', () => {
    const content = compile({
      prototypes: { RUMOUR: { kind: 'idea', derive: { aspects: { glimpse: { fromObjectAspect: 'secret' } } } } },
      verbs: {},
    })
    const r = analyzeCard(content, stateWith(ANA()), '', { demand: 'aspect:glimpse>=5' })
    expect(r.verdict).toBe('unproven')
    expect(r.notes.join(' ')).toContain('derive')
  })
})

describe('usability semantics (b)', () => {
  const readableBook = () =>
    compile({
      verbs: {
        READ: {
          slots: [
            { role: 'actors', min: 1, max: 1, query: 'kind:character' },
            { role: 'object', min: 1, max: 1, query: 'kind:text' },
          ],
          duration: 30,
          recipes: [{ out: ['NARRATE "read"'] }],
        },
      },
    })

  test('an initial card bindable through a committable verb slot is obtainable under usable', () => {
    const r = analyzeCard(readableBook(), stateWith(ANA(), BOOK()), 'THE_BOOK', { usable: true })
    expect(r.verdict).toBe('obtainable')
    expect(r.usableVia).toContain('READ.object (query matches)')
  })

  test('an existing card no slot can bind is `unusable` under usable — obtainable without the flag', () => {
    const content = readableBook()
    const state = stateWith(ANA(), ent('THE_WALL', 'place'))
    expect(analyzeCard(content, state, 'THE_WALL').verdict).toBe('obtainable')
    const r = analyzeCard(content, state, 'THE_WALL', { usable: true })
    expect(r.verdict).toBe('unusable')
    expect(r.existence).toBe('proven')
  })
})
