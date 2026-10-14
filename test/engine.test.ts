// Engine acceptance tests — the spec §6.4 READ ladder, REFUSE semantics (§6.5),
// time ownership (§9.12), events (§9.16), fold determinism (§7).
import { describe, expect, test } from 'vitest'
import { compileContent, type CompiledContent, type ContentRaw, type VerbContent } from '../src/content/content.js'
import { coreDictionary } from '../src/core/dict.js'
import { applyRecord, executeStep, type ActionRecord, type ExecResult } from '../src/engine/engine.js'
import { formatDuration } from '../src/core/clock.js'
import { fold, foldUpTo } from '../src/engine/fold.js'
import { formatSentence } from '../src/core/format.js'
import { parseSentence } from '../src/core/parse.js'
import { prose } from '../src/ui/render.js'
import { loadLog, saveLog, Log } from '../src/engine/store.js'
import { addEntity, newState, type State } from '../src/core/types.js'
import { seedContent, seedState } from './seed.js'
import { snap } from './snap.js'
import { settleStep, act, done, refusal } from './settle.js'
import { mkdtempSync, rmSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'

const dict = coreDictionary()
const content = compileContent(seedContent(), dict)

function setup() {
  return { dict, content, state: seedState() }
}

function run(state: State, src: string, firstRecordId = 1): ExecResult {
  return runWith(content, state, src, firstRecordId)
}

function runWith(theContent: typeof content, state: State, src: string, firstRecordId = 1): ExecResult {
  const s = parseSentence(src, dict)
  let n = 0
  return settleStep(theContent, dict, state, { clock: s.clock ?? { day: 0, halfhour: 0 }, clauses: s.clauses }, firstRecordId, () => `x${firstRecordId}_${++n}`) // unique across steps: keyed by record id
}

describe('READ ladder — first match wins, most specific first (§6.4)', () => {
  test('solo reader with knowledge passes the gate; default narrates', () => {
    const { state } = setup()
    const r = run(state, '0:28 @ ANA READ MAGICK_BOOK')
    expect(r.rejected).toBeNull()
    expect(act(r).recipes).toEqual(['read.default'])
    expect(act(r).advancedMin).toBe(60)
    expect(r.state.now).toBe(14 * 60 + 60)
  })

  test('magick gate REFUSEs an ignorant reader: no state change, no time, attempt logged (§6.5)', () => {
    const { state } = setup()
    const before = snap(state)
    const r = run(state, '0:28 @ BERTHA READ MAGICK_BOOK')
    expect(r.rejected).toBeNull()
    expect(r.records).toHaveLength(1)
    expect(done(r).outcomes.map(o => o.op)).toEqual(['REFUSE'])
    expect(act(r).advancedMin).toBe(0)
    expect(r.state).toBe(state) // same object, untouched
    expect(snap(r.state)).toBe(before)
    expect(done(r).sentence).toContain('REFUSE("The letters swim like minnows.")')
  })

  test('parent + child reading fairy tales branches to bonding memory', () => {
    const { state } = setup()
    const r = run(state, '0:28 @ ANA, BERTHA READ FAIRY_TALES')
    expect(act(r).recipes).toEqual(['read.parent_child'])
    const spawn = done(r).outcomes[0]
    expect(spawn.op).toBe('SPAWN')
    const nameArg = spawn.args[0]
    expect(nameArg && nameArg.t === 'ref' && nameArg.ref.form === 'name' && nameArg.ref.name).toBe('BONDING_MEMORY')
    expect(r.state.byName.has('BONDING_MEMORY')).toBe(true)
  })

  test('two unrelated readers get the poorer outcome — no spawn', () => {
    const { state } = setup()
    const r = run(state, '0:28 @ ANA, MY_FOLLOWER READ FAIRY_TALES')
    expect(act(r).recipes).toEqual(['read.unrelated_pair'])
    expect(done(r).outcomes.every(o => o.op === 'NARRATE')).toBe(true)
  })

  test('prototype-parameterized spawn resolves into a concrete named card (§9.15)', () => {
    const { state } = setup()
    const r = run(state, '0:28 @ ANA READ THE_FORGETTING')
    expect(act(r).recipes).toEqual(['read.secret'])
    const spawn = done(r).outcomes[0]
    const nameArg = spawn.args[0]
    expect(nameArg && nameArg.t === 'ref' && nameArg.ref.form === 'name' && nameArg.ref.name).toBe('CLUE_THE_FORGETTING')
    const asId = spawn.args.find(a => a.t === 'asId')
    expect(asId).toBeDefined()
    const minted = r.state.byName.get('CLUE_THE_FORGETTING')![0]
    expect(r.state.entities.get(minted)!.aspects['secret']).toBe(2) // 4 - 2
    expect(r.state.entities.get(minted)!.bornAction).toBe(done(r)!.id) // v0.5: the completion row mints
  })
})

describe('slot discipline (§6.1)', () => {
  test('cardinality violations are rejected before anything applies', () => {
    const { state } = setup()
    const r = run(state, '0:28 @ ANA, BERTHA, MY_FOLLOWER READ FAIRY_TALES')
    expect(r.rejected).toMatch(/'actors' takes 1..2/)
    expect(r.records).toHaveLength(0)
  })

  test('slot discipline: undeclared roles are rejected', () => {
    const { state } = setup()
    const r = run(state, '0:28 @ ANA READ FAIRY_TALES WITH CHALICE')
    expect(r.rejected).toMatch(/does not accept 'with'/)
    expect(r.records).toHaveLength(0)
  })

  test('slot discipline: one card cannot fill a role twice (§6.7 lock discipline)', () => {
    const { state } = setup()
    const r = run(state, '0:28 @ ANA, ANA READ FAIRY_TALES')
    expect(r.rejected).toMatch(/already placed in 'actors'/)
    expect(r.records).toHaveLength(0)
  })
})

describe('TALK with ABOUT (§4.3)', () => {
  test('talking about a secret idea spawns a plan', () => {
    const { state } = setup()
    const r = run(state, '0:28 @ ANA, BERTHA AT DINING_HALL TALK ABOUT CLUE_SECRET_HIDEOUT')
    expect(act(r).recipes).toEqual(['talk.plan'])
    expect(r.state.byName.has('PLAN_INFILTRATION')).toBe(true)
  })
})

describe('multi-clause steps and time ownership (§9.12)', () => {
  test('travel then read: durations sum, recipes listed in clause order', () => {
    const { state } = setup()
    const r = run(state, '0:28 @ ANA TRAVEL_TO THE_STUDY; ANA AT THE_STUDY READ THE_FORGETTING')
    expect(act(r).recipes).toEqual(['travel_to.default', 'read.secret'])
    expect(act(r).advancedMin).toBe(120)
    expect(r.state.entities.get('ana')!.attrs['container']).toBe('study')
    expect(r.state.now).toBe(14 * 60 + 120)
  })
})

describe('USE_ON rite — aggregate gate, shown hint (§6.4)', () => {
  test('one lantern starves the rite → REFUSE, state untouched', () => {
    const { state } = setup()
    state.now = 14 * 60
    const before = snap(state)
    const r = run(state, '0:28 @ ANA AT HOUSE USE_ON RITE_OF_ASCENSION')
    expect(done(r).outcomes.map(o => o.op)).toEqual(['REFUSE'])
    expect(r.state.now).toBe(14 * 60)
    expect(snap(r.state)).toBe(before)
  })

  test('pooled lantern meets demand: rite consumed, instruments destroyed, mark spawned INTO place', () => {
    const { state } = setup()
    const r = run(state, '0:28 @ ANA, MY_FOLLOWER AT HOUSE USE_ON RITE_OF_ASCENSION WITH CHALICE')
    expect(r.rejected).toBeNull()
    expect(r.state.entities.get('rite')!.deadAction).toBe(done(r)!.id) // v0.5: the completion row consumes
    expect(r.state.entities.get('chalice')!.deadAction).toBe(done(r)!.id)
    const mark = r.state.byName.get('MARK_OF_ASCENSION')![0]
    expect(r.state.entities.get(mark)!.attrs['container']).toBe('house')
    expect(act(r).advancedMin).toBe(180)
  })
})

describe('events — authored triggers and expiries (§6.6, §9.16)', () => {
  test('crossing hour 16 fires dusk_falls as its own EVENT row', () => {
    const { state } = setup()
    const r1 = run(state, '0:28 @ ANA WAIT') // crosses 15 only
    const kinds1 = r1.records.map(x => x.kind)
    if (kinds1[0] === 'wait') kinds1.shift() // the typed clock bridges from now=0
    expect(kinds1).toEqual(['action', 'complete', 'wait']) // v0.5 turn shape
    const r2 = run(r1.state, '0:30 @ ANA WAIT') // crosses 16 → dusk
    const event = r2.records.find(x => x.kind === 'event')!
    expect(event.trigger).toBe('dusk_falls')
    expect(event.tGame).toBe('0:32')
    expect(r2.records.some(x => x.kind === 'action' && x.sentence)).toBe(true)
    expect(r2.state.byName.has('DREAD')).toBe(true)
  })

  test('expiry crossing destroys the card via an expire event', () => {
    const { state } = setup()
    state.entities.get('chalice')!.attrs['expiresAt'] = 16 * 60 // 0:32
    const r = run(state, '0:28 @ ANA WAIT; ANA WAIT; ANA WAIT') // three staggered windows, 14 → 17
    // v0.5 order: each window completes at its end; at minute 960 the window
    // completes FIRST, then slot 32 strikes — dusk (authored) before the
    // chalice expiry, deterministic
    const kinds = r.records.map(x => x.kind)
    if (kinds[0] === 'wait') kinds.shift() // bridge
    expect(kinds).toEqual([
      'action', 'complete', 'complete', 'event', 'event', 'complete', 'wait',
    ])
    const events = r.records.filter(x => x.kind === 'event')
    expect(events[0]!.trigger).toBe('dusk_falls')
    expect(events[1]!.trigger).toBe('expire:CHALICE')
    expect(r.state.entities.get('chalice')!.deadAction).toBe(events[1]!.id)
  })
})

describe('fold — state = fold(log), determinism, undo (§7)', () => {
  test('replaying the log from seed reproduces the played state exactly', () => {
    let state = setup().state
    const log = new Log(seedState())
    const steps = [
      '0:28 @ ANA READ THE_FORGETTING',
      '0:30 @ ANA, BERTHA READ FAIRY_TALES', // ends 0:32 → dusk event row
      '0:32 @ ANA, BERTHA AT DINING_HALL TALK ABOUT CLUE_SECRET_HIDEOUT',
      '0:34 @ ANA TRAVEL_TO THE_STUDY; ANA AT THE_STUDY READ MAGICK_BOOK',
      '0:40 @ ANA WAIT',
      '0:42 @ ANA WAIT',
    ]
    let id = 1
    for (const step of steps) {
      const r = run(state, step, id)
      expect(r.rejected).toBeNull()
      log.appendAll(r.records)
      state = r.state
      id += r.records.length
    }
    const replayed = fold(log.records, seedState(), dict, content)
    expect(snap(replayed)).toBe(snap(state))
  })

  test('refused attempts replay as no-ops', () => {
    const { state } = setup()
    const before = snap(state)
    const r = run(state, '0:28 @ BERTHA READ MAGICK_BOOK')
    const replayed = fold(r.records, seedState(), dict, content)
    expect(snap(replayed)).toBe(before)
  })

  test('JSONL persistence round-trips the log (§7)', () => {
    const { state } = setup()
    const r = run(state, '0:28 @ ANA READ THE_FORGETTING')
    const dir = mkdtempSync(join(tmpdir(), 'chorus-log-'))
    const path = join(dir, 'log.jsonl')
    try {
      saveLog(path, r.records)
      const loaded = loadLog(path)
      const replayed = fold(loaded, seedState(), dict, content)
      expect(snap(replayed)).toBe(snap(r.state))
    } finally {
      rmSync(dir, { recursive: true, force: true })
    }
  })

  test('undo = re-fold without the tail (§7)', () => {
    const { state } = setup()
    const r1 = run(state, '0:28 @ ANA READ THE_FORGETTING')
    const snapshotAfterOne = snap(r1.state)
    const r2 = run(r1.state, '0:32 @ ANA READ FAIRY_TALES', r1.records.length + 1)
    const all = [...r1.records, ...r2.records]
    const undone = foldUpTo(all, seedState(), dict, content, r1.records.length + 1) // drop turn 2
    expect(snap(undone)).toBe(snapshotAfterOne)
  })
})

describe('dictionary is the law (§9.14)', () => {
  test('SPAWN cannot mint a duplicate name among the living (§9.14) — the second window refuses', () => {
    const { state } = setup()
    const r = run(state, '0:28 @ ANA, BERTHA READ FAIRY_TALES') // window 1 opens
    expect(r.rejected).toBeNull()
    const r2 = run(r.state, '0:32 @ ANA, BERTHA READ FAIRY_TALES') // window 2 opens; window 1 completes en route
    expect(r2.rejected).toBeNull()
    // window 2's BONDING_MEMORY mint collides with the living one — the
    // window refuses (§9.19's completion analog), attempt logged, no second copy
    expect(refusal(r2)).toMatch(/already in use/)
    expect(r2.state.byName.get('BONDING_MEMORY')).toHaveLength(1)
  })

  test('names of dead entities are reusable (§9.14: unique among the LIVING)', () => {
    const { state } = setup()
    const r = run(state, '0:28 @ ANA, MY_FOLLOWER AT HOUSE USE_ON RITE_OF_ASCENSION WITH CHALICE')
    expect(r.rejected).toBeNull()
    expect(r.state.entities.get('chalice')!.deadAction).not.toBeNull()
    expect(r.state.byName.has('CHALICE')).toBe(false)
    // a new CHALICE can be minted after the old one died
    const rec = {
      id: 99, kind: 'complete' as const, trigger: null, tGame: '0:40', tReal: null, startMin: null, window: 'fixture', advancedMin: 0,
      actors: [], verb: '', objects: [], instruments: [], at: null, about: null, to: null,
      outcomes: [{ op: 'SPAWN', args: [{ t: 'ref', ref: { form: 'name', kind: 'item', name: 'CHALICE' } }, { t: 'asId', id: 'new_chalice' }] }],
      recipes: [], sentence: '',
    }
    applyRecord(rec as ActionRecord, r.state, dict, content)
    expect(r.state.byName.get('CHALICE')![0]).toBe('new_chalice')
  })
})

describe('review regressions', () => {
  test('SET stores the given value positionally (ref, key, value)', () => {
    const { state } = setup()
    const rec = {
      id: 1, kind: 'complete' as const, trigger: null, tGame: '0:28', tReal: null, startMin: null, window: 'fixture', advancedMin: 0,
      actors: [], verb: '', objects: [], instruments: [], at: null, about: null, to: null,
      outcomes: [{
        op: 'SET',
        args: [
          { t: 'ref', ref: { form: 'name', name: 'CHALICE' } },
          { t: 'str', v: 'state.sealed' },
          { t: 'bool', v: true },
        ],
      }],
      recipes: [], sentence: '',
    }
    applyRecord(rec as ActionRecord, state, dict, content)
    expect(state.entities.get('chalice')!.attrs['state.sealed']).toBe(true)
  })

  test('SET expires materializes expiresAt from the record clock', () => {
    const { state } = setup()
    const rec = {
      id: 1, kind: 'complete' as const, trigger: null, tGame: '0:28', tReal: null, startMin: null, window: 'fixture', advancedMin: 0,
      actors: [], verb: '', objects: [], instruments: [], at: null, about: null, to: null,
      outcomes: [{
        op: 'SET',
        args: [
          { t: 'ref', ref: { form: 'name', name: 'CHALICE' } },
          { t: 'str', v: 'expires' },
          { t: 'dur', v: 120 },
        ],
      }],
      recipes: [], sentence: '',
    }
    applyRecord(rec as ActionRecord, state, dict, content)
    expect(state.entities.get('chalice')!.attrs['expiresAt']).toBe(14 * 60 + 120)
  })

  test('relationKey: symmetric edges share one canonical key; directed keep order', () => {
    const { state } = setup()
    const relate = (outcomeSrc: string, id: number) => {
      const s = parseSentence(`0:0 @ ANA WAIT -> ${outcomeSrc}`, dict)
      applyRecord({
        id, kind: 'complete', trigger: null, tGame: '0:0', tReal: null, startMin: null, window: 'fixture', advancedMin: 0,
        actors: [], verb: '', objects: [], instruments: [], at: null, about: null, to: null,
        outcomes: s.outcomes, recipes: [], sentence: '',
      }, state, dict, content)
    }
    relate('RELATE(BERTHA, ANA, parent_child)', 1)
    relate('RELATE(BERTHA, ANA, parent_of)', 2)
    const keys = [...state.relations.keys()]
    expect(keys.filter(k => k.includes('parent_child'))).toEqual(['ana|bertha|parent_child']) // one canonical edge
    expect(keys.filter(k => k.includes('parent_of'))).toEqual(['bertha|ana|parent_of'])      // directed, as stored
    relate('UNRELATE(ANA, BERTHA, parent_child)', 3)
    expect([...state.relations.values()].find(r => r.type === 'parent_child')!.deadAction).toBe(3)
  })

  test('fold clock: a log ending past an event keeps the action end time (§7)', () => {
    const { state } = setup()
    state.entities.get('chalice')!.attrs['expiresAt'] = 15 * 60
    const r = run(state, '0:28 @ ANA WAIT; ANA WAIT; ANA WAIT') // 14→17, events at 15/16
    expect(r.records.filter(x => x.kind === 'event').length).toBeGreaterThanOrEqual(2)
    const replayed = fold(r.records, seedState(), dict, content)
    expect(replayed.now).toBe(r.state.now) // events must not drag the clock back
    expect(replayed.now).toBe(17 * 60)
  })

  test('role sugar is concretized in the log (§9.15): replay matches play exactly', () => {
    const { state } = setup()
    const r = run(state, '0:28 @ ANA, BERTHA TRAVEL_TO VILLA; MY_FOLLOWER WAIT')
    expect(r.rejected).toBeNull()
    // the recorded MOVE must name ANA and BERTHA, not ACTORS
    const move = done(r).outcomes.find(o => o.op === 'MOVE')!
    const refs = move.args.filter((a): a is Extract<import('../src/core/sentence.js').Arg, { t: 'ref' }> => a.t === 'ref')
    expect(refs.map(a => a.ref.form === 'id' && a.ref.id).sort()).toEqual(['ana', 'bertha'])
    const replayed = fold(r.records, seedState(), dict, content)
    expect(replayed.entities.get('follower')!.attrs['container']).toBe('house') // not dragged along
    expect(replayed.entities.get('ana')!.attrs['container']).toBe('villa')
  })

  test('steps cannot rewind the clock (§9.5)', () => {
    const { state } = setup()
    state.now = 10 * 60
    const r = run(state, '0:6 @ ANA WAIT')
    expect(r.rejected).toMatch(/before state\.now/)
    expect(r.records).toHaveLength(0)
  })

  test('application failures refuse the window instead of throwing (§6.6, v0.5 completion)', () => {
    const { state } = setup()
    const r = run(state, '0:28 @ ANA, BERTHA READ FAIRY_TALES')
    expect(r.rejected).toBeNull()
    const r2 = run(r.state, '0:32 @ ANA, BERTHA READ FAIRY_TALES') // duplicate mint name
    expect(r2.rejected).toBeNull()
    expect(refusal(r2)).toMatch(/already in use/)
    expect(r2.state).not.toBe(r.state)
    expect(r2.state.byName.get('BONDING_MEMORY')).toHaveLength(1) // the second mint never landed
  })
})

describe('exists(...) recipes — world-scope conditions (§6.3, CS extantreqs)', () => {
  const probeDict = coreDictionary()
  probeDict.registerVerb('PROBE')
  const probeContent = (recipes: ContentRaw['verbs'][string]['recipes']): CompiledContent => compileContent({
    verbs: {
      PROBE: {
        slots: [{ role: 'actors', min: 1, max: 2, query: 'kind:character' }],
        duration: 30,
        recipes,
      },
    },
  }, probeDict)
  const runProbe = (theContent: CompiledContent, state: State, src: string, firstRecordId = 1): ExecResult => {
    const s = parseSentence(src, probeDict)
    let n = 0
    return settleStep(theContent, probeDict, state, { clock: s.clock ?? { day: 0, halfhour: 0 }, clauses: s.clauses }, firstRecordId, () => `x${firstRecordId}_${++n}`)
  }
  // two recipes, order-dependent: the gate branch needs a hunter in the world
  // AND no ward anywhere; the default is the calm world
  const gate = (): CompiledContent => probeContent([
    { id: 'probe.hunted', when: 'exists(kind:character aspect:hunter>=1) AND NOT exists(kind:item aspect:ward>=1)', out: ['NARRATE("A hunter walks the world.")'] },
    { out: ['NARRATE("The world is calm.")'] },
  ])

  test('scope proof: an unbound hunter in a far place selects the exists AND NOT exists branch', () => {
    const { state } = setup()
    // prowls the villa: bound to no role of this step and in another place
    // than ANA at the house — world scope is the whole point
    addEntity(state, { id: 'hunter', kind: 'character', name: 'HUNTER', aspects: { hunter: 1 }, attrs: { container: 'villa' }, bornAction: null, deadAction: null, stackable: false })
    const r = runProbe(gate(), state, '0:28 @ ANA PROBE')
    expect(r.rejected).toBeNull()
    expect(act(r).recipes).toEqual(['probe.hunted'])
    expect(done(r).sentence).toContain('A hunter walks the world.')
  })

  test('a ward anywhere in the world flips the branch — NOT exists(kind:item aspect:ward>=1) fails', () => {
    const { state } = setup()
    addEntity(state, { id: 'hunter', kind: 'character', name: 'HUNTER', aspects: { hunter: 1 }, attrs: { container: 'villa' }, bornAction: null, deadAction: null, stackable: false })
    addEntity(state, { id: 'ward', kind: 'item', name: 'IRON_WARD', aspects: { ward: 2 }, attrs: { container: 'greenhouse' }, bornAction: null, deadAction: null, stackable: false })
    const r = runProbe(gate(), state, '0:28 @ ANA PROBE')
    expect(act(r).recipes).toEqual(['probe.default'])
  })

  test('destroyed cards stop existing: the gate un-latches after the hunter dies', () => {
    const { state } = setup()
    addEntity(state, { id: 'hunter', kind: 'character', name: 'HUNTER', aspects: { hunter: 1 }, attrs: { container: 'villa' }, bornAction: null, deadAction: null, stackable: false })
    state.entities.get('hunter')!.deadAction = 99 // DESTROY shape: stamp + release the name (§9.14)
    state.byName.delete('HUNTER')
    const r = runProbe(gate(), state, '0:28 @ ANA PROBE')
    expect(act(r).recipes).toEqual(['probe.default'])
  })

  test('name form: exists(THE_SEALED_DOOR) sees a named card across the map', () => {
    const { state } = setup()
    addEntity(state, { id: 'sealed_door', kind: 'place', name: 'THE_SEALED_DOOR', aspects: {}, attrs: { container: 'villa' }, bornAction: null, deadAction: null, stackable: false })
    const c = probeContent([
      { id: 'probe.door', when: 'exists(THE_SEALED_DOOR)', out: ['NARRATE("The door still stands.")'] },
      { out: ['NARRATE("No door.")'] },
    ])
    const r = runProbe(c, state, '0:28 @ ANA PROBE')
    expect(act(r).recipes).toEqual(['probe.door'])
  })

  test('relations are untouched by exists evaluation: rel gates still select as before', () => {
    const { state } = setup()
    const c = probeContent([
      { id: 'probe.bonded', when: 'exists(aspect:lantern>=1) AND rel(parent_child, actors)', out: ['NARRATE("Kin in a haunted world.")'] },
      { out: ['NARRATE("No kinship.")'] },
    ])
    const r = runProbe(c, state, '0:28 @ ANA, BERTHA PROBE') // ANA is BERTHA's parent (seed edge)
    expect(act(r).recipes).toEqual(['probe.bonded'])
    const r2 = runProbe(c, state, '0:30 @ ANA, MY_FOLLOWER PROBE', 2) // no parent_child edge
    expect(act(r2).recipes).toEqual(['probe.default'])
  })
})

describe('progress ticks — the processing read-bar (§6.7)', () => {
  test('ticks record in emission order, touch no state, cost no extra time', () => {
    const { state } = setup()
    const r = run(state, '0:28 @ ANA, MY_FOLLOWER AT HOUSE USE_ON RITE_OF_ASCENSION WITH CHALICE')
    expect(r.rejected).toBeNull()
    // v0.5: top-level PROGRESS ticks ride the BEGIN row; the real outcomes
    // land on the COMPLETE row
    expect(act(r).outcomes.map(o => o.op)).toEqual(['PROGRESS', 'PROGRESS'])
    expect(done(r).outcomes.map(o => o.op)).toEqual(['DESTROY', 'DESTROY', 'SPAWN'])
    expect(act(r).advancedMin).toBe(180) // the verb's duration only
    expect(act(r).sentence).toContain('PROGRESS(1h, "The circle begins to hum against your palms.")')
    // the log alone reproduces the played state exactly (ticks are journal-only at fold)
    const replayed = fold(r.records, seedState(), dict, content)
    expect(snap(replayed)).toBe(snap(r.state))
  })

  test('malformed PROGRESS arity is an application failure, not a silent no-op (§9.19)', () => {
    const { state } = setup()
    const rec = {
      id: 1, kind: 'complete' as const, trigger: null, tGame: '0:28', tReal: null, startMin: null, window: 'fixture', advancedMin: 0,
      actors: [], verb: '', objects: [], instruments: [], at: null, about: null, to: null,
      outcomes: [{ op: 'PROGRESS', args: [{ t: 'str', v: 'no duration' }] }],
      recipes: [], sentence: '',
    }
    expect(() => applyRecord(rec as ActionRecord, state, dict, content)).toThrow(/PROGRESS needs/)
  })

  test('NARRATE keeps exactly one string; extras are refused at parse (§9.6 losslessness)', () => {
    expect(() => parseSentence('0:0 @ ANA WAIT -> NARRATE("one", "two")', dict)).toThrow(/exactly one string/)
  })
})

describe('clock & duration literals', () => {
  test('a sub-halfhour action leaves dead air; the next step resumes on the tick and the gap fires (§9.28)', () => {
    // a 20m verb pushes the world clock mid-tick; the timetable speaks at 14:30 (slot 29)
    const content20 = compileContent({
      verbs: {
        WAIT: {
          slots: [{ role: 'actors', min: 1, max: 1, query: 'kind:character' }],
          duration: 20,
          recipes: [{ out: ['NARRATE("{actors} waits a moment.")'] }],
        },
      },
      triggers: [{ id: 'the_chime', slot: 29, out: ['NARRATE("The halfhour chime sings through the frames.")'] }],
    }, dict)
    const s = seedState()
    const r1 = runWith(content20, s, '0:28 @ ANA WAIT') // 14:00 → 14:20, now mid-tick
    expect(r1.rejected).toBeNull()
    expect(r1.state.now).toBe(28 * 30 + 20)
    // the next step resumes on the halfhour tick (14:30); the chime crosses
    // inside the quantized gap, so it fires even though no window spans 14:30
    const r2 = runWith(content20, r1.state, '0:29 @ ANA WAIT')
    expect(r2.rejected).toBeNull()
    // v0.5: the chime crosses inside the advance that leads the step, before
    // the BEGIN — the quantized dead air is a logged wait row now
    expect(r2.records.map(x => x.kind)).toEqual(['event', 'wait', 'action', 'complete', 'wait'])
    expect(r2.records[0]!.trigger).toBe('the_chime')
    expect(r2.records[0]!.tGame).toBe('0:29')
  })

  test('durations spill into the next day', () => {
    const { state } = setup()
    const r = run(state, '0:44 @ ANA WORK AT HOUSE') // 2h → day 1, 0:00
    expect(r.state.now).toBe(22 * 60 + 120)
    expect(act(r).tGame).toBe('0:44')
  })

  test('formatDuration renders zero as 0m', () => {
    expect(formatDuration(0)).toBe('0m')
  })
})

describe('record shape (§7)', () => {
  test('records carry sorted actors, provenance, canonical sentence', () => {
    const { state } = setup()
    const r = run(state, '0:28 @ BERTHA, ANA AT DINING_HALL TALK ABOUT CLUE_SECRET_HIDEOUT')
    const rec: ActionRecord = act(r)
    expect(rec.actors.map(a => a.form === 'name' && a.name)).toEqual(['ANA', 'BERTHA'])
    expect(rec.verb).toBe('TALK')
    expect(rec.tGame).toBe('0:28')
    expect(rec.sentence.startsWith('0:28 @ ANA, BERTHA AT DINING_HALL TALK')).toBe(true)
  })
})

describe('empty base state fold (fresh world, no content spawns)', () => {
  test('fold of an empty log returns the base state', () => {
    const base = newState()
    expect(fold([], base, dict, content).entities.size).toBe(0)
  })
})

describe('SET deltas and aspect keys (§9.31)', () => {
  const tuneDict = coreDictionary()
  tuneDict.registerVerb('TUNE')
  const counterContent = (recipes: ContentRaw['verbs'][string]['recipes']): CompiledContent => compileContent({
    verbs: {
      TUNE: {
        slots: [{ role: 'actors', min: 1, max: 2, query: 'kind:character' }],
        duration: 30,
        recipes,
      },
    },
  }, tuneDict)
  const runTune = (theContent: CompiledContent, state: State, src: string, firstRecordId = 1): ExecResult => {
    const s = parseSentence(src, tuneDict)
    let n = 0
    return settleStep(theContent, tuneDict, state, { clock: s.clock ?? { day: 0, halfhour: 0 }, clauses: s.clauses }, firstRecordId, () => `x${firstRecordId}_${++n}`)
  }

  test('a signed value adds to the attr (missing = 0) and the log carries the absolute', () => {
    const { state } = setup()
    const c = counterContent([{ out: ['SET(ANA, funds, +2)'] }])
    const r = runTune(c, state, '0:20 @ ANA TUNE')
    expect(r.rejected).toBeNull()
    expect(r.state.entities.get('ana')!.attrs['funds']).toBe(2) // missing entry counted as 0
    const set = done(r).outcomes[0]
    expect(set.op).toBe('SET')
    expect(set.args[2]).toEqual({ t: 'int', v: 2 }) // the record holds the concrete absolute, not the delta
    expect(done(r).sentence).toContain('-> SET(ANA, funds, 2)') // name ref kept, value absolutized
    // the concrete sentence round-trips
    expect(formatSentence(parseSentence(done(r).sentence, tuneDict), tuneDict)).toBe(done(r).sentence)
    // the next delta applies to the new value
    const r2 = runTune(c, r.state, '0:21 @ ANA TUNE', 2)
    expect(r2.state.entities.get('ana')!.attrs['funds']).toBe(4)
  })

  test('decrements may go negative — no clamping (§9.31)', () => {
    const { state } = setup()
    const c = counterContent([{ out: ['SET(ACTORS, funds, -3)'] }])
    const r = runTune(c, state, '0:20 @ ANA TUNE')
    expect(r.rejected).toBeNull()
    expect(r.state.entities.get('ana')!.attrs['funds']).toBe(-3)
    expect(done(r).sentence).toContain('SET(#ana, funds, -3)')
  })

  test('aspect: + delta increments the aspects map; an absolute aspect set writes it too', () => {
    const { state } = setup()
    const c = counterContent([{ out: ['SET(ACTORS, aspect:dread, +1)'] }])
    const r = runTune(c, state, '0:20 @ ANA TUNE')
    expect(r.rejected).toBeNull()
    expect(r.state.entities.get('ana')!.aspects['dread']).toBe(1)
    expect(done(r).sentence).toContain('SET(#ana, aspect:dread, 1)')
    // absolute form through the fold path (concrete outcomes replay as-is)
    const s = parseSentence('0:0 @ ANA WAIT -> SET(#ana, aspect:language.latin, 2)', dict)
    applyRecord({
      id: 2, kind: 'complete', trigger: null, tGame: '0:0', tReal: null, startMin: null, window: 'fixture', advancedMin: 0,
      actors: [], verb: '', objects: [], instruments: [], at: null, about: null, to: null,
      outcomes: s.outcomes, recipes: [], sentence: '',
    }, r.state, dict, content)
    expect(r.state.entities.get('ana')!.aspects['language']).toEqual({ latin: 2 })
  })

  test('absolute values stay absolute — repeated casts write the same literal', () => {
    const { state } = setup()
    const c = counterContent([{ out: ['SET(ACTORS, funds, 5)'] }])
    const r = runTune(c, state, '0:20 @ ANA TUNE')
    expect(r.state.entities.get('ana')!.attrs['funds']).toBe(5)
    const r2 = runTune(c, r.state, '0:21 @ ANA TUNE', 2)
    expect(r2.state.entities.get('ana')!.attrs['funds']).toBe(5) // not 5+5
    expect(done(r2).sentence).toContain('SET(#ana, funds, 5)') // role concretized (§9.15), value literal
  })

  test('a role-arg absolute SET in a multi-clause step logs one concrete row per card and replays exactly (§9.15, §7)', () => {
    const { state } = setup()
    const c = compileContent({
      verbs: {
        TUNE: {
          slots: [
            { role: 'actors', min: 1, max: 2, query: 'kind:character' },
            { role: 'object', min: 1, max: 1, query: 'kind:text' },
          ],
          duration: 30,
          recipes: [{ out: ['SET(OBJECT, searched, 1)'] }],
        },
      },
    }, tuneDict)
    const r = runTune(c, state, '0:20 @ ANA TUNE THE_FORGETTING; BERTHA TUNE FAIRY_TALES')
    expect(r.rejected).toBeNull()
    // each clause's OBJECT becomes its own id row — the role itself never
    // reaches the log, so the fold needs no per-clause attribution (v0.5:
    // each clause's window closes with its own COMPLETE row)
    const sets = r.records.filter(x => x.kind === 'complete').map(x => x.sentence)
    expect(sets[0]).toContain('SET(#forgetting, searched, 1)')
    expect(sets[1]).toContain('SET(#fairy, searched, 1)')
    // §7: the records alone reproduce the played state. (Pre-fix, the fold
    // resolved OBJECT against the whole-record binding — the first clause's
    // card — and the second card never got searched.)
    expect(snap(fold(r.records, seedState(), tuneDict, c))).toBe(snap(r.state))
    expect(r.state.entities.get('forgetting')!.attrs['searched']).toBe(1)
    expect(r.state.entities.get('fairy')!.attrs['searched']).toBe(1)
  })

  test('a delta over a role ref logs one concrete SET per card, each with its own result', () => {
    const { state } = setup()
    state.entities.get('ana')!.aspects['dread'] = 2 // the two bases differ
    const c = counterContent([{ id: 'tune.gain', when: 'count(actors) = 2', out: ['SET(ACTORS, aspect:dread, +1)'] }])
    const r = runTune(c, state, '0:20 @ ANA, BERTHA TUNE')
    expect(r.rejected).toBeNull()
    expect(done(r).outcomes.filter(o => o.op === 'SET')).toHaveLength(2)
    expect(done(r).sentence).toContain('SET(#ana, aspect:dread, 3), SET(#bertha, aspect:dread, 1)')
    expect(r.state.entities.get('ana')!.aspects['dread']).toBe(3)
    expect(r.state.entities.get('bertha')!.aspects['dread']).toBe(1)
  })

  test('fold of a JSONL-saved delta log reproduces the played state (§7)', () => {
    const c = counterContent([{ out: ['SET(ACTORS, funds, +2)', 'SET(ACTORS, aspect:dread, +1)'] }])
    let state = setup().state
    state.entities.get('bertha')!.attrs['funds'] = 10 // the cards carry different bases
    state.entities.get('bertha')!.aspects['dread'] = 7
    let id = 1
    const all: ActionRecord[] = []
    for (const step of ['0:20 @ ANA, BERTHA TUNE', '0:21 @ ANA TUNE', '0:22 @ BERTHA TUNE']) {
      const r = runTune(c, state, step, id)
      expect(r.rejected).toBeNull()
      all.push(...r.records)
      state = r.state
      id += r.records.length
    }
    expect(state.entities.get('ana')!.attrs['funds']).toBe(4)
    expect(state.entities.get('ana')!.aspects['dread']).toBe(2)
    expect(state.entities.get('bertha')!.attrs['funds']).toBe(14)
    expect(state.entities.get('bertha')!.aspects['dread']).toBe(9)
    const dir = mkdtempSync(join(tmpdir(), 'chorus-log-'))
    const path = join(dir, 'log.jsonl')
    try {
      saveLog(path, all)
      const replayed = fold(loadLog(path), seedState(), tuneDict, c)
      expect(snap(replayed)).toBe(snap(state))
    } finally {
      rmSync(dir, { recursive: true, force: true })
    }
  })

  test('REFUSE after a delta SET in the same step: state untouched, no time, attempt logged (§6.5)', () => {
    const { state } = setup()
    const before = snap(state)
    const c = counterContent([{ out: ['SET(ACTORS, aspect:dread, +1)', 'SET(ACTORS, funds, +2)', 'REFUSE("too costly")'] }])
    const r = runTune(c, state, '0:20 @ ANA TUNE')
    expect(r.rejected).toBeNull()
    expect(done(r).outcomes.map(o => o.op)).toEqual(['SET', 'SET', 'REFUSE'])
    expect(act(r).advancedMin).toBe(0)
    expect(r.state).toBe(state) // same object, untouched — the deltas ran on the discarded draft
    expect(snap(r.state)).toBe(before)
    expect(snap(fold(r.records, seedState(), tuneDict, c))).toBe(before) // refused record folds as a no-op
  })

  test('a delta on a non-numeric entry refuses the window — nothing applied (§9.19 completion analog)', () => {
    const { state } = setup()
    const c = counterContent([{ out: ['SET(ACTORS, container, +1)'] }]) // container is a string attr
    const r = runTune(c, state, '0:20 @ ANA TUNE')
    expect(r.rejected).toBeNull()
    expect(refusal(r)).toMatch(/non-numeric/)
    expect(r.state.entities.get('ana')!.attrs['funds']).toBeUndefined() // nothing applied
  })

  test('a delta on an aspect sub-map refuses the window — the map survives (§9.31)', () => {
    const { state } = setup()
    state.entities.get('ana')!.aspects['language'] = { french: 2 } // a sub-map, not a number
    const c = counterContent([{ out: ['SET(ACTORS, aspect:language, +1)'] }])
    const r = runTune(c, state, '0:20 @ ANA TUNE')
    expect(r.rejected).toBeNull()
    expect(refusal(r)).toMatch(/non-numeric/)
    expect(r.state.entities.get('ana')!.aspects['language']).toEqual({ french: 2 }) // survives
  })

  test('a quoted key stays quoted in the concrete record (§9.15)', () => {
    const { state } = setup()
    const c = counterContent([{ out: ['SET(ACTORS, "about", +5)'] }]) // 'about' is a reserved role word
    const r = runTune(c, state, '0:20 @ ANA TUNE')
    expect(r.rejected).toBeNull()
    expect(r.state.entities.get('ana')!.attrs['about']).toBe(5)
    expect(done(r).sentence).toContain('SET(#ana, "about", 5)')
    // the record must round-trip: unquoted it would re-parse as the about ROLE
    expect(formatSentence(parseSentence(done(r).sentence, tuneDict), tuneDict)).toBe(done(r).sentence)
  })
})

// ---------- authorable decay: REPLACE + `on expire of` (§9.32–§9.34) ----------

describe('REPLACE — decay into a successor (§4.4, §9.33)', () => {
  const decayDict = coreDictionary()
  decayDict.registerVerb('DECAY')
  const decayContent = (out: string, prototypes: ContentRaw['prototypes'] = {}) => compileContent({
    verbs: {
      DECAY: {
        slots: [
          { role: 'actors', min: 1, max: 1, query: 'kind:character' },
          { role: 'object', min: 1, max: 1 },
        ],
        duration: 0,
        recipes: [{ out: [out] }],
      },
    },
    prototypes,
  }, decayDict)
  const runDecay = (theContent: CompiledContent, state: State, src: string, firstRecordId = 1): ExecResult => {
    const s = parseSentence(src, decayDict)
    let n = 0
    return settleStep(theContent, decayDict, state, { clock: s.clock ?? { day: 0, halfhour: 0 }, clauses: s.clauses }, firstRecordId, () => `x${firstRecordId}_${++n}`)
  }

  test('a place-contained card decays into the prototype, in the same place', () => {
    const { state } = setup() // CHALICE sits in HOUSE
    const c = decayContent('REPLACE OBJECT WITH ASHES', { ASHES: { kind: 'item', aspects: { spent: 1 } } })
    const r = runDecay(c, state, '0:20 @ ANA DECAY CHALICE')
    expect(r.rejected).toBeNull()
    // zero new log surface: the record carries the concrete SPAWN+DESTROY pair (§9.33)
    expect(done(r).outcomes.map(o => o.op)).toEqual(['SPAWN', 'DESTROY'])
    expect(done(r).sentence).toMatch(/-> SPAWN ASHES INTO #house AS #x\d+_1 /)
    expect(done(r).sentence).toContain('DESTROY(#chalice)')
    const ash = r.state.entities.get(r.state.byName.get('ASHES')![0])!
    expect(ash.kind).toBe('item')
    expect(ash.aspects).toEqual({ spent: 1 })
    expect(ash.attrs['container']).toBe('house') // inherits the dying card's place
    expect(ash.bornAction).toBe(done(r)!.id)
    expect(r.state.entities.get('chalice')!.deadAction).toBe(done(r)!.id)
    expect(r.state.byName.has('CHALICE')).toBe(false)
    // the fold replays the pair exactly
    expect(snap(fold(r.records, seedState(), decayDict, c))).toBe(snap(r.state))
  })

  test('a carried card stays carried by the same character', () => {
    const { state } = setup()
    state.entities.get('chalice')!.attrs['container'] = 'ana' // now carried, not shelved
    const c = decayContent('REPLACE OBJECT WITH ASHES', { ASHES: { kind: 'item' } })
    const r = runDecay(c, state, '0:20 @ ANA DECAY CHALICE')
    expect(r.rejected).toBeNull()
    expect(r.state.entities.get(r.state.byName.get('ASHES')![0])!.attrs['container']).toBe('ana')
  })

  test('a zone-tagged card passes its zone to the successor — no container card (§10)', () => {
    const base = seedState()
    delete base.entities.get('chalice')!.attrs['container']
    base.entities.get('chalice')!.attrs['zone'] = 'NIGHTJAR_FLYER'
    const c = decayContent('REPLACE OBJECT WITH ASHES', { ASHES: { kind: 'item' } })
    const r = runDecay(c, base, '0:20 @ ANA DECAY CHALICE')
    expect(r.rejected).toBeNull()
    const ash = r.state.entities.get(r.state.byName.get('ASHES')![0])!
    expect(ash.attrs['zone']).toBe('NIGHTJAR_FLYER') // the grouping has no card: the tag passes on
    expect(ash.attrs['container']).toBeUndefined()
    // the zone rides the concrete attrs pair, so the fold replays it (§9.33)
    expect(snap(fold(r.records, base, decayDict, c))).toBe(snap(r.state))
  })

  test('REPLACE of an unbound role ref rejects the step — nothing logged (§9.19)', () => {
    const c = compileContent({
      verbs: {
        DECAY: {
          slots: [
            { role: 'actors', min: 1, max: 1, query: 'kind:character' },
            { role: 'object', min: 0, max: 1 }, // optional, and left unbound
          ],
          duration: 0,
          recipes: [{ out: ['REPLACE OBJECT WITH ASHES'] }],
        },
      },
      prototypes: { ASHES: { kind: 'item' } },
    }, decayDict)
    const { state } = setup()
    const before = snap(state)
    const r = runDecay(c, state, '0:20 @ ANA DECAY')
    expect(r.rejected).toBeNull()
    expect(refusal(r)).toMatch(/unresolvable ref/)
    expect(r.state.entities.get('chalice')!.deadAction).toBeNull() // nothing applied
  })

  test('the old card soft-destroys: its name is released and reusable (§9.14)', () => {
    const { state } = setup()
    const c = decayContent('REPLACE OBJECT WITH ASHES', { ASHES: { kind: 'item' } })
    const r = runDecay(c, state, '0:20 @ ANA DECAY CHALICE')
    expect(r.rejected).toBeNull()
    expect(r.state.entities.get('chalice')!.deadAction).not.toBeNull()
    expect(r.state.byName.has('CHALICE')).toBe(false)
    const rec = {
      id: 99, kind: 'complete' as const, trigger: null, tGame: '0:20', tReal: null, startMin: null, window: 'fixture', advancedMin: 0,
      actors: [], verb: '', objects: [], instruments: [], at: null, about: null, to: null,
      outcomes: [{ op: 'SPAWN', args: [{ t: 'ref', ref: { form: 'name', kind: 'item', name: 'CHALICE' } }, { t: 'asId', id: 'new_chalice' }] }],
      recipes: [], sentence: '',
    }
    applyRecord(rec as ActionRecord, r.state, decayDict, c)
    expect(r.state.byName.get('CHALICE')![0]).toBe('new_chalice')
  })

  test('a plain kind:NAME ref mints without a prototype — everything clause-less SPAWN accepts', () => {
    const { state } = setup()
    const c = decayContent('REPLACE OBJECT WITH item:CRUMBS')
    const r = runDecay(c, state, '0:20 @ ANA DECAY CHALICE')
    expect(r.rejected).toBeNull()
    const crumbs = r.state.entities.get(r.state.byName.get('CRUMBS')![0])!
    expect(crumbs.kind).toBe('item')
    expect(crumbs.attrs['container']).toBe('house')
  })

  test('collision with a living entity — the dying card included — rejects the whole step (§9.33)', () => {
    const { state } = setup()
    const before = snap(state)
    const c = decayContent('REPLACE OBJECT WITH CHALICE', { CHALICE: { kind: 'item' } })
    const r = runDecay(c, state, '0:20 @ ANA DECAY CHALICE') // CHALICE is still alive at spawn time
    expect(r.rejected).toBeNull()
    expect(refusal(r)).toMatch(/already in use/)
    expect(r.state.entities.get('chalice')!.deadAction).toBeNull() // nothing applied
    expect(state.byName.has('CHALICE')).toBe(true)
  })

  test('authored REPLACE round-trips; the paren-less form is the only form', () => {
    const nameForm = parseSentence('0:0 @ ANA DECAY CHALICE -> REPLACE CHALICE WITH ASHES', decayDict)
    expect(formatSentence(nameForm, decayDict)).toBe('0:0 @ ANA DECAY CHALICE -> REPLACE CHALICE WITH ASHES')
    const roleForm = parseSentence('0:0 @ ANA DECAY CHALICE -> REPLACE OBJECT WITH ASHES', decayDict)
    expect(formatSentence(roleForm, decayDict)).toBe('0:0 @ ANA DECAY CHALICE -> REPLACE OBJECT WITH ASHES')
    expect(() => parseSentence('0:0 @ ANA WAIT -> REPLACE(CHALICE, ASHES)', decayDict)).toThrow(/paren-less/)
    expect(() => parseSentence('0:0 @ ANA WAIT -> REPLACE CHALICE', decayDict)).toThrow(/REPLACE expects WITH/)
  })
})

describe('authored expiry triggers — `on expire of` (§6.6, §9.32)', () => {
  const mothDict = coreDictionary()
  const waitVerb: VerbContent = {
    slots: [{ role: 'actors', min: 1, max: 1, query: 'kind:character' }],
    duration: 60,
    recipes: [{ out: ['NARRATE("{actors} wait.")'] }],
  }
  // influence decay: G → E → C → (no trigger: built-in destroy)
  const mothContent = () => compileContent({
    verbs: { WAIT: waitVerb },
    triggers: [
      { id: 'decay_moth_g', onExpireOf: 'INFLUENCE_MOTH_G', out: ['REPLACE INFLUENCE_MOTH_G WITH INFLUENCE_MOTH_E', 'SET(INFLUENCE_MOTH_E, expires, 1h)'] },
      { id: 'decay_moth_e', onExpireOf: 'INFLUENCE_MOTH_E', out: ['REPLACE INFLUENCE_MOTH_E WITH INFLUENCE_MOTH_C', 'SET(INFLUENCE_MOTH_C, expires, 1h)'] },
    ],
    prototypes: {
      INFLUENCE_MOTH_E: { kind: 'idea', aspects: { influence: 2 } },
      INFLUENCE_MOTH_C: { kind: 'idea', aspects: { influence: 1 } },
    },
  }, mothDict)
  const mothBase = () => {
    const s = seedState()
    addEntity(s, { id: 'moth_g', kind: 'idea', name: 'INFLUENCE_MOTH_G', aspects: { influence: 3 }, attrs: { container: 'house', expiresAt: 16 * 60 }, bornAction: null, deadAction: null, stackable: false })
    return s
  }
  const runMoth = (state: State, src: string, firstRecordId: number): ExecResult => {
    const s = parseSentence(src, mothDict)
    let n = 0
    return settleStep(mothContent(), mothDict, state, { clock: s.clock ?? { day: 0, halfhour: 0 }, clauses: s.clauses }, firstRecordId, () => `x${firstRecordId}_${++n}`)
  }

  test('a decay chain runs across successive waits, then default-destroys (§9.32)', () => {
    let state = mothBase()
    const triggers: (string | null)[] = []
    let id = 1
    let eId: string | undefined
    let cId: string | undefined
    const allRecords: ActionRecord[] = []
    for (const step of ['0:30 @ ANA WAIT', '0:32 @ ANA WAIT', '0:34 @ ANA WAIT']) {
      const r = runMoth(state, step, id)
      expect(r.rejected).toBeNull()
      allRecords.push(...r.records)
      triggers.push(...r.records.filter(x => x.kind === 'event').map(x => x.trigger))
      state = r.state
      eId = eId ?? state.byName.get('INFLUENCE_MOTH_E')?.[0]
      cId = cId ?? state.byName.get('INFLUENCE_MOTH_C')?.[0]
      id += r.records.length
    }
    // G decayed into E, E into C, then C expired with no trigger → built-in destroy
    expect(triggers).toEqual(['decay_moth_g', 'decay_moth_e', 'expire:INFLUENCE_MOTH_C'])
    // v0.5: every stamp lands on the completion row that did the work —
    // derive the ids from the log instead of hardcoding them
    const stamp = (trigger: string): number =>
      allRecords.filter(x => x.kind === 'event' && x.trigger === trigger)[0]!.id
    expect(state.entities.get('moth_g')!.deadAction).toBe(stamp('decay_moth_g'))
    const e = state.entities.get(eId!)!
    expect(e.aspects).toEqual({ influence: 2 })        // plain prototype mint (no derive)
    expect(e.attrs['container']).toBe('house')         // inherited container
    expect(e.deadAction).toBe(stamp('decay_moth_e'))
    expect(state.entities.get(cId!)!.deadAction).toBe(stamp('expire:INFLUENCE_MOTH_C'))
    expect(state.byName.has('INFLUENCE_MOTH_G')).toBe(false)
    expect(state.byName.has('INFLUENCE_MOTH_E')).toBe(false)
    expect(state.byName.has('INFLUENCE_MOTH_C')).toBe(false) // destroyed: name released, no successor
  })

  test('an authored time trigger and an authored expiry fire in the same advance — triggers first (§6.6)', () => {
    const c = compileContent({
      verbs: { WAIT: waitVerb },
      triggers: [
        { id: 'dusk_falls', slot: 32, out: ['SPAWN DREAD'] },
        { id: 'decay_chalice', onExpireOf: 'CHALICE', out: ['REPLACE CHALICE WITH RUSTY_CHALICE'] },
      ],
      prototypes: { DREAD: { kind: 'state' }, RUSTY_CHALICE: { kind: 'item' } },
    }, dict)
    const { state } = setup()
    state.entities.get('chalice')!.attrs['expiresAt'] = 16 * 60 // same crossing as dusk (slot 32)
    const r = runWith(c, state, '0:28 @ ANA WAIT; ANA WAIT; ANA WAIT')
    expect(r.rejected).toBeNull()
    const events = r.records.filter(x => x.kind === 'event')
    expect(events.map(x => x.trigger)).toEqual(['dusk_falls', 'decay_chalice']) // triggers first
    expect(events[1]!.sentence).toContain('EVENT decay_chalice')
    expect(r.state.entities.get('chalice')!.deadAction).toBe(events[1]!.id)
    expect(r.state.entities.get(r.state.byName.get('RUSTY_CHALICE')![0])!.attrs['container']).toBe('house')
    expect(r.state.byName.has('DREAD')).toBe(true)
  })

  test('a colliding authored expiry rejects the whole step — nothing logged (§9.19, §9.33)', () => {
    const c = compileContent({
      verbs: { WAIT: waitVerb },
      triggers: [{ id: 'decay_chalice', onExpireOf: 'CHALICE', out: ['REPLACE CHALICE WITH CHALICE'] }],
      prototypes: { CHALICE: { kind: 'item' } },
    }, dict)
    const { state } = setup()
    state.entities.get('chalice')!.attrs['expiresAt'] = 15 * 60
    const before = snap(state)
    const r = runWith(c, state, '0:28 @ ANA WAIT; ANA WAIT')
    expect(r.rejected).toMatch(/already in use/)
    expect(r.records).toHaveLength(0)
    expect(r.state).toBe(state)
    expect(snap(r.state)).toBe(before)
  })

  test('play and fold agree on fired for every EVENT row — full-state fold equality incl. JSONL (§9.34)', () => {
    let state = mothBase()
    const all: ActionRecord[] = []
    let id = 1
    for (const step of ['0:30 @ ANA WAIT', '0:32 @ ANA WAIT', '0:34 @ ANA WAIT']) {
      const r = runMoth(state, step, id)
      expect(r.rejected).toBeNull()
      all.push(...r.records)
      state = r.state
      id += r.records.length
    }
    const dir = mkdtempSync(join(tmpdir(), 'chorus-log-'))
    const path = join(dir, 'log.jsonl')
    try {
      saveLog(path, all)
      const replayed = fold(loadLog(path), mothBase(), mothDict, mothContent())
      expect(snap(replayed)).toBe(snap(state)) // snap includes fired (§9.34)
      // one fired entry per EVENT row's trigger — authored ids and expire:<NAME> alike
      expect([...replayed.fired].sort()).toEqual(['decay_moth_e', 'decay_moth_g', 'expire:INFLUENCE_MOTH_C'])
    } finally {
      rmSync(dir, { recursive: true, force: true })
    }
  })
})

describe('trigger outcome guards (§6.7, §9.34)', () => {
  test('PROGRESS is a load-time error in both trigger forms', () => {
    expect(() => compileContent({ verbs: {}, triggers: [{ id: 't', slot: 1, out: ['PROGRESS(30m, "tick")'] }] }, dict)).toThrow(/PROGRESS belongs/)
    expect(() => compileContent({ verbs: {}, triggers: [{ id: 't', onExpireOf: 'X', out: ['PROGRESS(30m, "tick")'] }] }, dict)).toThrow(/PROGRESS belongs/)
  })

  test('REFUSE is a load-time error in both trigger forms — events have no step to refuse (§9.34)', () => {
    expect(() => compileContent({ verbs: {}, triggers: [{ id: 't', slot: 1, out: ['REFUSE("no")'] }] }, dict)).toThrow(/REFUSE belongs/)
    expect(() => compileContent({ verbs: {}, triggers: [{ id: 't', onExpireOf: 'X', out: ['REFUSE("no")'] }] }, dict)).toThrow(/REFUSE belongs/)
  })
})

// ---------- v0.3: `expires` accepts every §5.1 duration form (§4.2, §9.36) ----------

describe('SET expires — every §5.1 duration form materializes expiresAt (§4.2, §9.36)', () => {
  const tuneDict = coreDictionary()
  tuneDict.registerVerb('TUNE') // not a core verb — the rig from the §9.31 describes
  const tuneContent = (out: string): CompiledContent => compileContent({
    verbs: {
      TUNE: {
        slots: [{ role: 'actors', min: 1, max: 1, query: 'kind:character' }],
        duration: 0,
        recipes: [{ out: [out] }],
      },
    },
  }, tuneDict)
  const runTune = (theContent: CompiledContent, state: State, src: string, firstRecordId = 1): ExecResult => {
    const s = parseSentence(src, tuneDict)
    let n = 0
    return settleStep(theContent, tuneDict, state, { clock: s.clock ?? { day: 0, halfhour: 0 }, clauses: s.clauses }, firstRecordId, () => `x${firstRecordId}_${++n}`)
  }
  const at = (minutes: number): number => 20 * 30 + minutes // 0:20 = absolute minute 600

  test('quoted durations materialize expiresAt: "2halfhour", "90m", "1h30m", "2h", "1d", "0m"', () => {
    for (const [lit, minutes] of [['"2halfhour"', 60], ['"90m"', 90], ['"1h30m"', 90], ['"2h"', 120], ['"1d"', 1440], ['"0m"', 0]] as const) {
      const { state } = setup()
      const r = runTune(tuneContent(`SET(CHALICE, expires, ${lit})`), state, '0:20 @ ANA TUNE')
      expect(r.rejected).toBeNull()
      expect(r.state.entities.get('chalice')!.attrs['expiresAt']).toBe(at(minutes))
      // the log keeps the authored quoted string and round-trips (§9.15)
      expect(done(r).sentence).toContain(`-> SET(CHALICE, expires, ${lit})`)
    }
  })

  test('bare duration literals are unchanged — the lexer pre-converts them (§5.1 regression)', () => {
    const { state } = setup()
    const r = runTune(tuneContent('SET(CHALICE, expires, 2halfhour)'), state, '0:20 @ ANA TUNE')
    expect(r.rejected).toBeNull()
    expect(r.state.entities.get('chalice')!.attrs['expiresAt']).toBe(at(60))
    // the log stores the pre-converted minutes; canonical output prefers the shorter unit (§5.1)
    expect(done(r).sentence).toContain('-> SET(CHALICE, expires, 1h)')
    // quoted and bare materialize the same engine fact — identical expiresAt.
    // The authored `expires` attr itself keeps its authored form (§4.2: the
    // engine materializes expiresAt *from* it): minutes for bare, the string
    // for quoted — so only the logged sentence and that one attr differ (§9.15)
    const { state: quotedState } = setup()
    const rq = runTune(tuneContent('SET(CHALICE, expires, "2halfhour")'), quotedState, '0:20 @ ANA TUNE')
    expect(rq.rejected).toBeNull()
    expect(rq.state.entities.get('chalice')!.attrs['expiresAt']).toBe(r.state.entities.get('chalice')!.attrs['expiresAt'])
    expect(rq.state.entities.get('chalice')!.attrs['expires']).toBe('2halfhour')
    expect(r.state.entities.get('chalice')!.attrs['expires']).toBe(60)
  })

  test('a bad unit refuses the window — nothing logged as state (§9.19 completion analog)', () => {
    const { state } = setup()
    const r = runTune(tuneContent('SET(CHALICE, expires, "2fortnights")'), state, '0:20 @ ANA TUNE')
    expect(r.rejected).toBeNull()
    expect(refusal(r)).toMatch(/bad duration '2fortnights'/)
    expect(state.entities.get('chalice')!.attrs['expiresAt']).toBeUndefined()
  })

  test('end-to-end: a quoted expiry fires its authored `on expire` trigger at the next crossing (§9.32)', () => {
    const c = compileContent({
      verbs: {
        TUNE: {
          slots: [{ role: 'actors', min: 1, max: 1, query: 'kind:character' }],
          duration: 0,
          recipes: [{ out: ['SET(CHALICE, expires, "2halfhour")'] }],
        },
        WAIT: {
          slots: [{ role: 'actors', min: 1, max: 1, query: 'kind:character' }],
          duration: 60,
          recipes: [{ out: ['NARRATE("{actors} wait.")'] }],
        },
      },
      triggers: [{ id: 'melt_chalice', onExpireOf: 'CHALICE', out: ['REPLACE CHALICE WITH RUSTY_CHALICE'] }],
      prototypes: { RUSTY_CHALICE: { kind: 'item' } },
    }, tuneDict)
    const { state } = setup()
    const r1 = runTune(c, state, '0:28 @ ANA TUNE') // expiresAt = 28*30 + 60 = 0:30
    expect(r1.rejected).toBeNull()
    expect(r1.records.every(x => x.kind !== 'event')).toBe(true) // set this pass: resolves at the NEXT advance (§6.6 snapshot)
    const r2 = runWith(c, r1.state, '0:30 @ ANA WAIT', 1 + r1.records.length) // crosses slot 31 (0:31)
    expect(r2.rejected).toBeNull()
    const event = r2.records.find(x => x.kind === 'event')!
    expect(event.trigger).toBe('melt_chalice')
    expect(event.sentence).toContain('EVENT melt_chalice')
    expect(r2.state.entities.get('chalice')!.deadAction).toBe(event.id)
    expect(r2.state.entities.get(r2.state.byName.get('RUSTY_CHALICE')![0])!.attrs['container']).toBe('house')
    // play and fold agree through the quoted path too (§9.34)
    expect(snap(fold([...r1.records, ...r2.records], seedState(), tuneDict, c))).toBe(snap(r2.state))
  })
})

// ---------- v0.3: the core verb set is twelve (§4.3, §9.37) ----------

describe('core verbs STUDY and MANSUS (§4.3, §9.37)', () => {
  // the mirroring the spec declares: STUDY copies READ's slots, MANSUS copies
  // EXPLORE's — cardinalities and queries stay content (§6.1)
  const verbDict = coreDictionary() // STUDY/MANSUS are core since v0.3
  const verbsContent = compileContent({
    verbs: {
      STUDY: {
        slots: [
          { role: 'actors', min: 1, max: 2, query: 'kind:character' },
          { role: 'object', min: 1, max: 1, query: 'kind:text' },
          { role: 'at', min: 0, max: 1, query: 'kind:place' },
        ],
        duration: 60,
        recipes: [
          { id: 'study.secret', when: 'object.aspect.secret >= 3', out: ['SPAWN CLUE', 'NARRATE("{actors} tease out a hidden thread.")'] },
          { out: ['NARRATE("{actors} study {object}.")'] },
        ],
      },
      MANSUS: {
        slots: [
          { role: 'actors', min: 1, max: 1, query: 'kind:character' },
          { role: 'object', min: 1, max: 1, query: 'kind:place' },
        ],
        duration: 60,
        recipes: [{ out: ['NARRATE("{actors} walk the Mansus to {object}.")'] }],
      },
    },
    prototypes: {
      CLUE: { kind: 'idea', derive: { nameFrom: 'object', aspects: { secret: { fromObjectAspect: 'secret', minus: 2 } } } },
    },
  }, verbDict)
  const world = (): State => {
    const s = seedState()
    addEntity(s, { id: 'manifest', kind: 'text', name: 'THE_MANIFEST', aspects: { secret: 4 }, attrs: { container: 'house' }, bornAction: null, deadAction: null, stackable: false })
    return s
  }
  const runVerb = (theContent: CompiledContent, state: State, src: string, firstRecordId = 1): ExecResult => {
    const s = parseSentence(src, verbDict)
    let n = 0
    return settleStep(theContent, verbDict, state, { clock: s.clock ?? { day: 0, halfhour: 0 }, clauses: s.clauses }, firstRecordId, () => `x${firstRecordId}_${++n}`)
  }

  test('a canonical STUDY sentence parses, selects, and executes (§6.4)', () => {
    const r = runVerb(verbsContent, world(), '0:14 @ ANA STUDY THE_MANIFEST')
    expect(r.rejected).toBeNull()
    expect(act(r).recipes).toEqual(['study.secret'])
    expect(act(r).advancedMin).toBe(60)
    const minted = r.state.byName.get('CLUE_THE_MANIFEST')![0]
    expect(r.state.entities.get(minted)!.aspects['secret']).toBe(2) // 4 − 2 (§9.23)
    // the recorded sentence round-trips
    expect(formatSentence(parseSentence(done(r).sentence, verbDict), verbDict)).toBe(done(r).sentence)
  })

  test('the mirrored grammar bites: MANSUS takes a place, not a text', () => {
    const r = runVerb(verbsContent, world(), '0:14 @ ANA MANSUS THE_MANIFEST')
    expect(r.rejected).toMatch(/does not fit slot 'object'/)
    expect(r.records).toHaveLength(0)
    const r2 = runVerb(verbsContent, world(), '0:14 @ ANA MANSUS HOUSE')
    expect(r2.rejected).toBeNull()
    expect(act(r2).recipes).toEqual(['mansus.default'])
    expect(act(r2).sentence).toContain('ANA MANSUS HOUSE')
  })

  test('untemplated content renders acceptably: recipe NARRATEs speak, no template line (§5.3)', () => {
    const bare = compileContent({
      verbs: {
        MANSUS: {
          slots: [
            { role: 'actors', min: 1, max: 1, query: 'kind:character' },
            { role: 'object', min: 1, max: 1, query: 'kind:place' },
          ],
          duration: 60,
          recipes: [{ out: ['NARRATE("{actors} walk the Mansus to {object}.")'] }],
        },
      },
      // no `templates:` section at all
    }, verbDict)
    const r = runVerb(bare, world(), '0:14 @ ANA MANSUS HOUSE')
    expect(r.rejected).toBeNull()
    // v0.5: the NARRATE speaks at the completion, so the prose renders from there
    expect(prose(done(r), bare, verbDict)).toBe('ANA walk the Mansus to HOUSE.')
  })

  test('the seed verbs pin the mirror in code: STUDY ≡ READ, MANSUS ≡ EXPLORE (§4.3)', () => {
    // the mirroring the spec declares, asserted against the seed content —
    // not prose: slots are the grammar, cardinalities and queries included
    expect(content.verbs.get('STUDY')!.slots).toEqual(content.verbs.get('READ')!.slots)
    expect(content.verbs.get('MANSUS')!.slots).toEqual(content.verbs.get('EXPLORE')!.slots)
  })

  test('a story that does not author the verbs rejects at the content gate (§6.6)', () => {
    // registration puts STUDY/MANSUS in the dictionary (they parse, they are
    // reserved); content that never declares them refuses cleanly — no crash
    const unauthored = compileContent({ verbs: {} }, verbDict)
    const s1 = world()
    const r = runVerb(unauthored, s1, '0:14 @ ANA STUDY THE_MANIFEST')
    expect(r.rejected).toMatch(/STUDY is not a content verb/)
    expect(r.records).toHaveLength(0)
    expect(r.state).toBe(s1) // same object, untouched
    const s2 = world()
    const r2 = runVerb(unauthored, s2, '0:14 @ ANA MANSUS HOUSE')
    expect(r2.rejected).toMatch(/MANSUS is not a content verb/)
    expect(r2.records).toHaveLength(0)
    expect(r2.state).toBe(s2)
  })
})
