// F1 — stackable cards: multiple same-name instances (spec §9.38, plan v0.4 F1).
// Declared with `stackable` on entity/prototype lines; the name becomes a POOL
// of living copies addressed oldest-first; engine-written expiry destroys go
// by id while authored name-addressed outcomes hit the oldest.
import { describe, expect, test } from 'vitest'
import { compileContent, type CompiledContent } from '../src/content/content.js'
import { coreDictionary } from '../src/core/dict.js'
import { executeStep, type ActionRecord, type ExecResult } from '../src/engine/engine.js'
import { fold } from '../src/engine/fold.js'
import { parseSentence } from '../src/core/parse.js'
import { parsePlayerInput } from '../src/ui/lenient.js'
import { compileScript, parseScript } from '../src/content/script.js'
import { saveLog, loadLog } from '../src/engine/store.js'
import { addEntity, newState, type State } from '../src/core/types.js'
import { parsePredicate, evalPred } from '../src/core/predicate.js'
import { seedState } from './seed.js'
import { snap } from './snap.js'
import { settleStep, act, done, refusal } from './settle.js'
import { Game } from '../src/ui/play.js'
import { mkdtempSync, rmSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'

const dict = coreDictionary()
// the rig dictionary: core verbs + the rig's own lexemes, so every parse below works
const poolDict = coreDictionary()
poolDict.registerVerb('STEW')
poolDict.registerVerb('PROBE')
poolDict.registerVerb('CALM')
poolDict.registerVerb('DECAY')

// the rig script exercises the §10 `stackable` keyword end to end: a stackable
// prototype (SPAWN SUSPICION pools), a destroy-by-name verb, and a probe that
// binds up to two same-named cards into its object role
const poolScript = `
script: Pools
player: ANA
start: 0:0

entities:
  HOUSE is place
  ANA is character at HOUSE

prototypes:
  SUSPICION is state stackable with weight:1

verbs:
  STEW:
    actors: 1 kind:character
    duration: 0m
    recipes:
      default -> SPAWN SUSPICION, NARRATE "suspicion stirs."
  PROBE:
    actors: 1 kind:character
    object: 0..2 kind:state
    duration: 0m
    recipes:
      probe.clear when count(object) >= 1 -> SET(OBJECT, aspect:noted, +1), DESTROY OBJECT
      default -> NARRATE "{actors} probe."
  CALM:
    actors: 1 kind:character
    duration: 0m
    recipes:
      default -> DESTROY SUSPICION
  WAIT:
    actors: 1 kind:character
    duration: 1h
    recipes:
      default -> NARRATE "{actors} wait."
  ACQUIRE:
    actors: 1 kind:character
    object: 0..1
    duration: 0m
    recipes:
      default -> NARRATE "{actors} acquires."

templates:
  STEW says "{actors} stews."
  PROBE says "{actors} probe."
  CALM says "{actors} calms."
  WAIT says "{actors} wait."
`

function rig() {
  return compileScript(poolScript, poolDict)
}

function run(content: CompiledContent, state: State, src: string, firstRecordId = 1): ExecResult {
  const s = parseSentence(src, poolDict)
  let n = 0
  return settleStep(content, poolDict, state, { clock: s.clock ?? { day: 0, halfhour: 0 }, clauses: s.clauses }, firstRecordId, () => `x${firstRecordId}_${++n}`)
}

function stew(content: CompiledContent, times: number, state: State): State {
  let id = 1
  for (let i = 0; i < times; i++) {
    const r = run(content, state, '0:0 @ ANA STEW', id)
    expect(r.rejected).toBeNull()
    id += r.records.length
    state = r.state
  }
  return state
}

// ---------- the declaration (§10) ----------

describe('the stackable keyword (§9.38, §10)', () => {
  test('parses on entity and prototype lines and lands in the compiled sets', () => {
    const parsed = parseScript(`
entities:
  SUSPICION is state stackable with weight:1
  WALL is place stackable in HOUSE
  HOUSE is place
  PLAIN is idea

prototypes:
  RUMOUR is idea stackable deriving name from object
  QUIET is idea
`, dict)
    expect(parsed.entities.find(e => e.name === 'SUSPICION')!.stackable).toBe(true)
    expect(parsed.entities.find(e => e.name === 'WALL')!.stackable).toBe(true)
    expect(parsed.entities.find(e => e.name === 'PLAIN')!.stackable).toBe(false)
    const protos = parsed.content.prototypes ?? {}
    expect(protos['RUMOUR']!.stackable).toBe(true)
    expect(protos['QUIET']!.stackable).toBeUndefined() // absent = not stackable

    const { content, state } = compileScript(`
entities:
  SUSPICION is state stackable with weight:1
  WALL is place stackable
  HOUSE is place

prototypes:
  RUMOUR is idea stackable deriving name from object
  QUIET is idea
`, dict)
    expect(content.stackableNames.has('SUSPICION')).toBe(true)
    expect(content.stackableNames.has('RUMOUR')).toBe(true)
    expect(content.stackableNames.has('QUIET')).toBe(false)
    expect(content.stackableKinds.has('state')).toBe(true) // kinds of stackable declarations
    expect(content.stackableKinds.has('idea')).toBe(true)
    // entity-line cards mint stackable per their declaration
    expect(state.entities.get('s_suspicion')!.stackable).toBe(true)
    expect(state.entities.get('s_wall')!.stackable).toBe(true)
    expect(state.entities.get('s_house')!.stackable).toBe(false)
  })

  test('the keyword must sit before with/deriving — anything else fails the line', () => {
    expect(() => parseScript('entities:\n  X is place with a:1 stackable\n', dict)).toThrow()
    expect(() => parseScript('prototypes:\n  X is idea deriving name from object stackable\n', dict)).toThrow()
  })
})

// ---------- the pool (§9.38) ----------

describe('spawn pools and oldest-copy addressing (§9.38)', () => {
  test('three spawns stack: the pool holds three ids in birth order; exists() sees the pool', () => {
    const { content, state } = rig()
    const r3state = stew(content, 3, state)
    const pool = r3state.byName.get('SUSPICION')!
    expect(pool).toHaveLength(3)
    // insertion order = birth order; v0.5 mints stamp the completion rows
    const borns = pool.map(id => r3state.entities.get(id)!.bornAction!)
    expect(borns[0]!).toBeLessThan(borns[1]!)
    expect(borns[1]!).toBeLessThan(borns[2]!)
    expect(evalPred(parsePredicate('exists(SUSPICION)'), { b: {}, state: r3state })).toBe(true)
  })

  test('the player surface lists every copy; inspect reads the OLDEST (entered play first)', () => {
    const game = Game.fromScript(poolScript, poolDict)
    for (let i = 0; i < 3; i++) {
      game.handleLine('stew')
      game.wait() // v0.5: the spawn lands at the completion
    }
    expect(game.cards().filter(l => l.startsWith('SUSPICION'))).toHaveLength(3) // cosmetic duplicates OK
    const face = game.inspect('suspicion')
    expect(face[0]).toBe('SUSPICION  [state]')
    const oldestBorn = game.state.entities.get(game.state.byName.get('SUSPICION')![0])!.bornAction
    expect(face.join('\n')).toContain(`entered play at action ${oldestBorn}`) // first-visible = oldest
  })

  test('DESTROY by name takes the oldest, then the next-oldest; the last release frees the name', () => {
    const { content, state } = rig()
    let cur = stew(content, 3, state)
    let id = 4
    for (const expected of [2, 1, 0]) {
      const r = run(content, cur, '0:0 @ ANA CALM', id)
      expect(r.rejected).toBeNull()
      // the authored name-addressed destroy LOGS THE NAME (oldest resolved at apply)
      expect(done(r).outcomes[0].args[0]).toEqual({ t: 'ref', ref: { form: 'name', name: 'SUSPICION' } })
      expect(r.state.byName.get('SUSPICION') ?? []).toHaveLength(expected) // undefined once the name frees
      cur = r.state
      id += r.records.length
    }
    expect(cur.byName.has('SUSPICION')).toBe(false)
    // the freed name accepts a non-stackable mint again (§9.14, among the living)
    const plain = compileContent({
      verbs: { ACQUIRE: { slots: [{ role: 'actors', min: 1, max: 1, query: 'kind:character' }], duration: 0, recipes: [{ out: ['SPAWN SUSPICION'] }] } },
      prototypes: { SUSPICION: { kind: 'state' } }, // NOT stackable
    }, dict)
    expect(run(plain, cur, '0:0 @ ANA ACQUIRE', id).rejected).toBeNull()
  })

  test('a stackable spawn onto a DISTINCT non-stackable living name rejects — nothing logged', () => {
    const poke = compileContent({
      verbs: { ACQUIRE: { slots: [{ role: 'actors', min: 1, max: 1, query: 'kind:character' }], duration: 0, recipes: [{ out: ['SPAWN CHALICE'] }] } },
      prototypes: { CHALICE: { kind: 'item', stackable: true } }, // stackable, but CHALICE lives non-stackable
    }, dict)
    const state = seedState()
    const before = snap(state)
    const r = run(poke, state, '0:0 @ ANA ACQUIRE')
    expect(r.rejected).toBeNull()
    expect(refusal(r)).toMatch(/already in use/)
    expect(snap(state)).toBe(before)
    // once the non-stackable holder dies the name accepts the stackable mint
    state.entities.get('chalice')!.deadAction = 99
    state.byName.delete('CHALICE')
    expect(run(poke, state, '0:0 @ ANA ACQUIRE', 2).rejected).toBeNull()
  })

  test('a non-stackable spawn onto an existing pool name rejects (the shared namespace)', () => {
    const { content, state } = rig()
    const pooled = stew(content, 2, state)
    const plain = compileContent({
      verbs: { ACQUIRE: { slots: [{ role: 'actors', min: 1, max: 1, query: 'kind:character' }], duration: 0, recipes: [{ out: ['SPAWN SUSPICION'] }] } },
      prototypes: { SUSPICION: { kind: 'state' } }, // NOT stackable
    }, dict)
    const before = snap(pooled)
    const r = run(plain, pooled, '0:0 @ ANA ACQUIRE')
    expect(r.rejected).toBeNull()
    expect(refusal(r)).toMatch(/already in use/)
    expect(r.state.entities.get(pooled.byName.get('SUSPICION')![0])!.deadAction).toBeNull() // nothing applied
  })

  test('addEntity enforces the law itself: non-stackable duplicates throw, stackables append', () => {
    const s = newState()
    const mint = (id: string, stackable: boolean): void =>
      addEntity(s, { id, kind: 'state', name: 'X', aspects: {}, attrs: {}, bornAction: 1, deadAction: null, stackable })
    mint('a', false)
    expect(() => mint('b', false)).toThrow(/duplicate name 'X'/)
    expect(() => mint('b', true)).toThrow(/duplicate name 'X'/) // a non-stackable holder blocks stackers too
    const s2 = newState()
    const mint2 = (id: string, stackable: boolean): void =>
      addEntity(s2, { id, kind: 'state', name: 'X', aspects: {}, attrs: {}, bornAction: 1, deadAction: null, stackable })
    mint2('a', true)
    mint2('b', true)
    expect(s2.byName.get('X')).toEqual(['a', 'b'])
    expect(() => mint2('c', false)).toThrow(/duplicate name 'X'/)
  })

  test('a stackable prototype with `derive name from object` stacks under the derived name', () => {
    const spy = compileContent({
      verbs: {
        ACQUIRE: {
          slots: [
            { role: 'actors', min: 1, max: 1, query: 'kind:character' },
            { role: 'object', min: 1, max: 1 },
          ],
          duration: 0,
          recipes: [{ out: ['SPAWN RUMOUR'] }],
        },
      },
      prototypes: { RUMOUR: { kind: 'idea', stackable: true, derive: { nameFrom: 'object' } } },
    }, dict)
    let state: State = seedState()
    let id = 1
    for (let i = 0; i < 2; i++) {
      const r = run(spy, state, '0:0 @ ANA ACQUIRE CHALICE', id)
      expect(r.rejected).toBeNull()
      id += r.records.length
      state = r.state
    }
    expect(state.byName.get('RUMOUR_CHALICE')).toHaveLength(2)
  })

  test('kind-level stackability: a plain spawn of a NON-stackable prototype in a stackable KIND mints stackable — and play/fold agree (§9.38)', () => {
    // the authored ref is a bare name (no kind); the concrete row always
    // carries the resolved kind — so the stackable flag must be computed from
    // the resolved KIND at play too, or the fold's recompute diverges
    const c = compileContent({
      verbs: {
        ACQUIRE: {
          slots: [{ role: 'actors', min: 1, max: 1, query: 'kind:character' }],
          duration: 0,
          recipes: [{ out: ['SPAWN HEARSAY'] }],
        },
        DECAY: {
          slots: [
            { role: 'actors', min: 1, max: 1, query: 'kind:character' },
            { role: 'object', min: 1, max: 1 },
          ],
          duration: 0,
          recipes: [{ out: ['REPLACE OBJECT WITH HEARSAY'] }],
        },
      },
      prototypes: {
        RUMOUR: { kind: 'idea', stackable: true }, // makes 'idea' a stackable KIND
        HEARSAY: { kind: 'idea' },                 // NOT stackable by name
      },
    }, poolDict)
    const base = seedState()
    const r1 = run(c, base, '0:0 @ ANA ACQUIRE')
    expect(r1.rejected).toBeNull()
    const h1 = r1.state.byName.get('HEARSAY')![0]
    expect(r1.state.entities.get(h1)!.stackable).toBe(true)
    // the REPLACE successor computes the same way
    const r2 = run(c, r1.state, '0:0 @ ANA DECAY CHALICE', r1.records.length + 1)
    expect(r2.rejected).toBeNull()
    const h2 = r2.state.byName.get('HEARSAY')![0]
    expect(r2.state.entities.get(h2)!.stackable).toBe(true)
    // replay agreement: snap serializes every entity (flag included), so any
    // play/fold divergence in the recompute shows here — and the fold replays
    // BOTH steps' concrete rows, so x1_1's flag is recomputed from its log too
    expect(snap(fold([...r1.records, ...r2.records], base, poolDict, c))).toBe(snap(r2.state))
  })
})

// ---------- slot bindings and role outcomes under pools (§9.38) ----------

describe('slot bindings hold same-named copies individually (§9.38)', () => {
  test('two copies bind by #id; SET(OBJECT)/DESTROY OBJECT address each bound card', () => {
    const { content, state } = rig()
    const pooled = stew(content, 2, state)
    const [elder, younger] = pooled.byName.get('SUSPICION')!
    const r = run(content, pooled, `0:0 @ ANA PROBE #${elder}, #${younger}`, 3)
    expect(r.rejected).toBeNull()
    expect(act(r).recipes).toEqual(['probe.clear'])
    // one concrete SET per bound card, then the destroy carries both ids
    expect(done(r).sentence).toContain(
      `-> SET(#${elder}, aspect:noted, 1), SET(#${younger}, aspect:noted, 1), DESTROY(#${elder}, #${younger})`,
    )
    expect(r.state.entities.get(elder)!.aspects['noted']).toBe(1)
    expect(r.state.entities.get(younger)!.aspects['noted']).toBe(1)
    expect(r.state.entities.get(elder)!.deadAction).not.toBeNull()
    expect(r.state.entities.get(younger)!.deadAction).not.toBeNull()
    expect(r.state.byName.has('SUSPICION')).toBe(false) // the last copy died: name freed
  })

  test('lenient input resolves a pool name to its OLDEST copy', () => {
    const game = Game.fromScript(poolScript, poolDict)
    game.handleLine('stew')
    game.wait() // v0.5: the STEW window must complete for SUSPICION to mint
    game.handleLine('stew')
    game.wait()
    const [eldest, younger] = game.state.byName.get('SUSPICION')!
    const parsed = parsePlayerInput('probe the suspicion', { dict: poolDict, content: game.content, state: game.state, player: 'ANA' })
    expect(parsed.ok).toBe(true)
    if (parsed.ok) {
      const r = settleStep(game.content, poolDict, game.state, parsed.step, game.log.nextRecordId)
      expect(r.rejected).toBeNull()
      expect(r.state.entities.get(eldest)!.deadAction).not.toBeNull() // the oldest went
      expect(r.state.byName.get('SUSPICION')).toEqual([younger])
    }
  })
})

// ---------- expiry under a pool (§9.32, §9.38) ----------

describe('expiry under a pool (§9.38)', () => {
  test('engine expiry addresses by ID: the YOUNGER expires and dies; no re-fire on later crossings', () => {
    const { content, state } = rig()
    const pooled = stew(content, 2, state)
    const [eldest, younger] = pooled.byName.get('SUSPICION')!
    pooled.entities.get(younger)!.attrs['expiresAt'] = 30 // crosses slot 1 (0:30)
    const r = run(content, pooled, '0:0 @ ANA WAIT') // 1h: crosses slots 1 and 2
    expect(r.rejected).toBeNull()
    const events = r.records.filter(x => x.kind === 'event')
    expect(events).toHaveLength(1)
    expect(events[0].trigger).toBe('expire:SUSPICION')
    expect(events[0].tGame).toBe('0:1')
    // the engine-written destroy is keyed to the EXPIRING copy: DESTROY #id
    const a0 = events[0].outcomes[0].args[0]
    expect(a0.t).toBe('ref')
    if (a0.t === 'ref') expect(a0.ref).toEqual({ form: 'id', id: younger })
    expect(r.state.entities.get(younger)!.deadAction).not.toBeNull()
    expect(r.state.entities.get(eldest)!.deadAction).toBeNull() // the elder survives
    expect(r.state.byName.get('SUSPICION')).toEqual([eldest])
    // later crossings never re-fire: the survivor carries no expires
    const r2 = run(content, r.state, '0:2 @ ANA WAIT', r.records.length + 1)
    expect(r2.rejected).toBeNull()
    expect(r2.records.filter(x => x.kind === 'event')).toHaveLength(0)
  })

  test('three copies past their expires cross one two-slot advance: oldest dies first, all by #id; the fold replays identically (§9.38)', () => {
    const { content, state } = rig()
    const pooled = stew(content, 3, state)
    const pool = [...pooled.byName.get('SUSPICION')!] // birth order
    for (const pid of pool) pooled.entities.get(pid)!.attrs['expiresAt'] = 10 // past BOTH crossed slots
    const r = run(content, pooled, '0:0 @ ANA WAIT') // 1h: crosses slots 1 and 2
    expect(r.rejected).toBeNull()
    const events = r.records.filter(x => x.kind === 'event')
    // the expiry snapshot iterates entities in birth order: all three fire at
    // the FIRST crossed slot (0:1), oldest first; slot 2 finds nobody alive
    expect(events.map(e => e.tGame)).toEqual(['0:1', '0:1', '0:1'])
    const deadIds = events.map(e => {
      const a = e.outcomes[0]!.args[0]
      return a.t === 'ref' && a.ref.form === 'id' ? a.ref.id : '(not an id ref)'
    })
    expect(deadIds).toEqual(pool)
    expect(r.state.byName.has('SUSPICION')).toBe(false)
    // every row is id-keyed, so the fold replays the triple destroy exactly
    expect(snap(fold(r.records, pooled, poolDict, content))).toBe(snap(r.state))
  })

  test('authored expiry asymmetry (§9.38 ruling): fires per crossing copy; its NAME outcomes hit the OLDEST', () => {
    const asym = compileContent({
      verbs: {
        STEW: {
          slots: [{ role: 'actors', min: 1, max: 1, query: 'kind:character' }],
          duration: 0,
          recipes: [{ out: ['SPAWN SUSPICION'] }],
        },
        WAIT: {
          slots: [{ role: 'actors', min: 1, max: 1, query: 'kind:character' }],
          duration: 30, // one slot per wait: each advance crosses exactly one expiry crossing
          recipes: [{ out: ['NARRATE("{actors} wait.")'] }],
        },
      },
      triggers: [{ id: 'sus_burns', onExpireOf: 'SUSPICION', out: ['DESTROY SUSPICION', 'NARRATE("a suspicion burns away.")'] }],
      prototypes: { SUSPICION: { kind: 'state', stackable: true } },
    }, poolDict)
    const { state } = rig()
    const pooled = stew(asym, 2, state)
    const [elder, younger] = pooled.byName.get('SUSPICION')!
    // the YOUNGER carries the expires. The stews cost no time, so the wait's
    // event span reaches back to 0:0 (§9.28 dead air): expiresAt 60 (slot 2)
    // crosses exactly once during the 0:1 wait
    pooled.entities.get(younger)!.attrs['expiresAt'] = 60
    const r = run(asym, pooled, '0:1 @ ANA WAIT', 3)
    expect(r.rejected).toBeNull()
    const events = r.records.filter(x => x.kind === 'event')
    expect(events.map(e => e.trigger)).toEqual(['sus_burns'])
    // …but its name-addressed DESTROY resolved to the OLDEST copy — the asymmetry:
    // the card that died is the ELDER, not the copy whose expires crossed
    expect(r.state.entities.get(elder)!.deadAction).not.toBeNull()
    expect(r.state.entities.get(younger)!.deadAction).toBeNull() // the crossing copy still stands
    expect(r.state.byName.get('SUSPICION')).toEqual([younger])
    // the crossing copy is past its expires: it re-fires (§9.32) on the next
    // crossing — and being now the pool's head, the second DESTROY takes IT
    const r2 = run(asym, r.state, '0:2 @ ANA WAIT', r.records.length + 3)
    expect(r2.records.filter(x => x.kind === 'event').map(e => e.trigger)).toEqual(['sus_burns'])
    expect(r2.state.entities.get(younger)!.deadAction).not.toBeNull()
    expect(r2.state.byName.has('SUSPICION')).toBe(false)
  })
})

// ---------- REPLACE a stackable with its own prototype (§9.33 revised) ----------

describe('REPLACE X WITH X succeeds for stackables (§9.38)', () => {
  const decay = (): CompiledContent => compileContent({
    verbs: {
      DECAY: {
        slots: [
          { role: 'actors', min: 1, max: 1, query: 'kind:character' },
          { role: 'object', min: 1, max: 1 },
        ],
        duration: 0,
        recipes: [{ out: ['REPLACE OBJECT WITH SUSPICION'] }],
      },
    },
    prototypes: { SUSPICION: { kind: 'state', stackable: true } },
  }, poolDict)

  test('spawn appends while the dying card lives, then the destroy pops it', () => {
    const c = decay()
    const { content, state } = rig()
    const pooled = stew(content, 1, state) // one copy alive, container s_house
    const elder = pooled.byName.get('SUSPICION')![0]
    const r = run(c, pooled, `0:0 @ ANA DECAY #${elder}`, 2)
    expect(r.rejected).toBeNull()
    expect(done(r).outcomes.map(o => o.op)).toEqual(['SPAWN', 'DESTROY'])
    const successor = r.state.byName.get('SUSPICION')!
    expect(successor).toHaveLength(1)
    expect(successor[0]).not.toBe(elder)
    expect(r.state.entities.get(elder)!.deadAction).not.toBeNull()
    expect(r.state.entities.get(successor[0])!.attrs['container']).toBe('s_house') // place inherited
    expect(r.state.entities.get(successor[0])!.stackable).toBe(true)
    // the fold replays the concrete SPAWN+DESTROY pair exactly (§9.33)
    expect(snap(fold(r.records, pooled, poolDict, c))).toBe(snap(r.state))
    // and the next-oldest (now the only) replaces again just the same
    const r2 = run(c, r.state, `0:0 @ ANA DECAY #${successor[0]}`, r.records.length + 2)
    expect(r2.rejected).toBeNull()
    expect(r2.state.byName.get('SUSPICION')).toHaveLength(1)
  })

  test('a NON-stackable REPLACE onto a stackable pool still rejects (§9.33 unchanged)', () => {
    const c = compileContent({
      verbs: {
        DECAY: {
          slots: [
            { role: 'actors', min: 1, max: 1, query: 'kind:character' },
            { role: 'object', min: 1, max: 1 },
          ],
          duration: 0,
          recipes: [{ out: ['REPLACE OBJECT WITH SUSPICION'] }],
        },
      },
      prototypes: { SUSPICION: { kind: 'state' } }, // NOT stackable
    }, poolDict)
    const { content, state } = rig()
    const pooled = stew(content, 2, state)
    const elder = pooled.byName.get('SUSPICION')![0]
    const before = snap(pooled)
    const r = run(c, pooled, `0:0 @ ANA DECAY #${elder}`, 3) // the successor's name collides with the pool
    expect(r.rejected).toBeNull()
    expect(refusal(r)).toMatch(/already in use/)
    expect(r.state.entities.get(elder)!.deadAction).toBeNull() // nothing applied
  })
})

// ---------- draft discipline over the pool arrays (law 6) ----------

describe('REFUSE after a pool destroy leaves the real pool untouched (§6.5, law 6)', () => {
  test('the draft spliced a CLONE of the pool array — the real state keeps both copies', () => {
    const refuse = compileContent({
      verbs: {
        CALM: {
          slots: [{ role: 'actors', min: 1, max: 1, query: 'kind:character' }],
          duration: 0,
          recipes: [{ out: ['DESTROY SUSPICION', 'REFUSE("not yet")'] }],
        },
      },
      prototypes: { SUSPICION: { kind: 'state', stackable: true } },
    }, poolDict)
    const { content, state } = rig()
    const pooled = stew(content, 2, state)
    const before = snap(pooled)
    const [eldest, younger] = pooled.byName.get('SUSPICION')!
    const r = run(refuse, pooled, '0:0 @ ANA CALM', 3)
    expect(r.rejected).toBeNull()
    expect(done(r).outcomes.map(o => o.op)).toEqual(['DESTROY', 'REFUSE'])
    expect(act(r).advancedMin).toBe(0)
    expect(r.state).toBe(pooled) // same object, untouched — the destroy ran on the discarded draft
    expect(snap(r.state)).toBe(before)
    // a shallow cloneState would have shared the array: the draft's splice
    // would have dropped the elder from the REAL pool too
    expect(pooled.byName.get('SUSPICION')).toEqual([eldest, younger])
    expect(pooled.entities.get(eldest)!.deadAction).toBeNull()
    expect(pooled.entities.get(younger)!.deadAction).toBeNull()
  })
})

// ---------- fold / JSONL equality incl. the multimap (§7) ----------

describe('fold and JSONL equality with pools (§7)', () => {
  test('the log alone reproduces the pooled state — byName arrays included', () => {
    const { content, state } = rig()
    // the expiry enters THROUGH the log (a concrete SET materializes expiresAt),
    // so the fold sees exactly what play saw
    const expireSet = compileContent({
      verbs: {
        ACQUIRE: {
          slots: [
            { role: 'actors', min: 1, max: 1, query: 'kind:character' },
            { role: 'object', min: 0, max: 1 },
          ],
          duration: 0,
          recipes: [{ out: ['SET(OBJECT, expires, 30m)'] }],
        },
      },
    }, poolDict)
    let cur = state
    let id = 1
    const all: ActionRecord[] = []
    const mints: string[] = [] // the pool copies in birth order, ids from the log
    const mintOf = (r: { records: ActionRecord[] }): string => {
      const sp = r.records.find(x => x.kind === 'complete')?.outcomes.find(o => o.op === 'SPAWN')
      const as = sp?.args.find(a => a.t === 'asId')
      if (!as || as.t !== 'asId') throw new Error('no mint in the completion')
      return as.id
    }
    const steps: [string, string][] = [
      ['STEW', '0:0 @ ANA STEW'],        // mint 1
      ['STEW', '0:0 @ ANA STEW'],        // mint 2
      ['STEW', '0:0 @ ANA STEW'],        // mint 3
      ['EXPIRE', ''],                    // logs SET(#third, expires, 30m) → expiresAt 30
      ['STEW', '0:0 @ ANA WAIT'],        // crosses slot 1: expire:SUSPICION DESTROY the third (by id)
      ['STEW', '0:2 @ ANA CALM'],        // authored destroy of the then-oldest (by name)
      ['STEW', '0:2 @ ANA PROBE'],       // slot-bound SET+DESTROY of the last copy
    ]
    for (const [verb, step] of steps) {
      const src = verb === 'EXPIRE'
        ? `0:0 @ ANA ACQUIRE #${mints[2]}`
        : verb === 'STEW' && step.includes('PROBE')
          ? `0:2 @ ANA PROBE #${mints[1]}`
          : step
      const r = run(verb === 'STEW' ? content : expireSet, cur, src, id)
      expect(r.rejected).toBeNull()
      all.push(...r.records)
      id += r.records.length
      cur = r.state
      if (verb === 'STEW' && src.includes('STEW')) mints.push(mintOf(r))
    }
    expect(cur.byName.has('SUSPICION')).toBe(false) // the pool drained
    const dir = mkdtempSync(join(tmpdir(), 'chorus-pool-'))
    const path = join(dir, 'log.jsonl')
    try {
      saveLog(path, all)
      const loaded = loadLog(path)
      const base2 = rig()
      const replayed = fold(loaded, base2.state, poolDict, base2.content)
      expect(snap(replayed)).toBe(snap(cur)) // snap includes the byName multimap (shared helper)
    } finally {
      rmSync(dir, { recursive: true, force: true })
    }
  })
})

// ---------- counting the pool: count(<query>) (§9.38) ----------

describe('count(<query>) thresholds select recipes (§9.38)', () => {
  const gate = (): CompiledContent => compileContent({
    verbs: {
      STEW: {
        slots: [{ role: 'actors', min: 1, max: 1, query: 'kind:character' }],
        duration: 0,
        recipes: [
          { id: 'stew.enough', when: 'count(name:SUSPICION) >= 2', out: ['NARRATE("the pot boils.")'] },
          { id: 'stew.default', out: ['SPAWN SUSPICION'] },
        ],
      },
    },
    prototypes: { SUSPICION: { kind: 'state', stackable: true } },
  }, poolDict)

  test('the pool count grows per copy and flips the branch at the threshold', () => {
    const c = gate()
    const { state } = rig()
    let cur: State = state
    const picks: string[] = []
    let id = 1
    for (let i = 0; i < 3; i++) {
      const r = run(c, cur, '0:0 @ ANA STEW', id)
      expect(r.rejected).toBeNull()
      picks.push(...act(r).recipes)
      id += r.records.length
      cur = r.state
    }
    expect(picks).toEqual(['stew.default', 'stew.default', 'stew.enough'])
    expect(cur.byName.get('SUSPICION')).toHaveLength(2) // the threshold branch narrates, it does not spawn
  })

  test('count(DREAD) / total(DREAD, aspect:x) are load errors naming the valid forms (§9.38 lint)', () => {
    expect(() => compileContent({
      verbs: { WAIT: { slots: [{ role: 'actors', min: 1, max: 1 }], duration: 0, recipes: [{ when: 'count(DREAD) >= 3', out: ['NARRATE("x")'] }] } },
    }, dict)).toThrow(/count\(\) argument 'DREAD' is neither a role/)
    expect(() => compileContent({
      verbs: { WAIT: { slots: [{ role: 'actors', min: 1, max: 1 }], duration: 0, recipes: [{ when: 'total(DREAD, aspect:weight) >= 3', out: ['NARRATE("x")'] }] } },
    }, dict)).toThrow(/total\(\) first argument 'DREAD' is not a declared role/)
    // the query form compiles — the lint only fences the role-looking typo
    expect(() => compileContent({
      verbs: { WAIT: { slots: [{ role: 'actors', min: 1, max: 1 }], duration: 0, recipes: [{ when: 'count(name:DREAD) >= 3', out: ['NARRATE("x")'] }] } },
    }, dict)).not.toThrow()
    // a comparison glued INSIDE the term would bake 'DREAD>=1' in as the NAME
    // (a silent 0) — the name: term rejects it at load instead
    expect(() => compileContent({
      verbs: { WAIT: { slots: [{ role: 'actors', min: 1, max: 1 }], duration: 0, recipes: [{ when: 'count(name:DREAD >= 1) >= 3', out: ['NARRATE("x")'] }] } },
    }, dict)).toThrow(/bad name term/)
    // total(<query>, …) is not offered — named as such, not a cryptic parse error
    expect(() => compileContent({
      verbs: { WAIT: { slots: [{ role: 'actors', min: 1, max: 1 }], duration: 0, recipes: [{ when: 'total(name:DREAD, aspect:x) >= 3', out: ['NARRATE("x")'] }] } },
    }, dict)).toThrow(/total\(\) pools a bound role/)
  })
})
