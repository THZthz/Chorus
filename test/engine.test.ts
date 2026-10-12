// Engine acceptance tests — the spec §6.4 READ ladder, REFUSE semantics (§6.5),
// time ownership (§9.12), events (§9.16), fold determinism (§7).
import { describe, expect, test } from 'vitest'
import { compileContent, type CompiledContent, type ContentRaw } from '../src/content.js'
import { applyRecord, executeStep, type ActionRecord, type ExecResult } from '../src/engine.js'
import { formatDuration } from '../src/clock.js'
import { fold, foldUpTo } from '../src/fold.js'
import { formatSentence } from '../src/format.js'
import { parseSentence } from '../src/parse.js'
import { seedContent, seedDictionary, seedState } from '../src/seed.js'
import { loadLog, saveLog, Log } from '../src/store.js'
import { addEntity, newState, type State } from '../src/types.js'
import { mkdtempSync, rmSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'

const dict = seedDictionary()
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
  return executeStep(theContent, dict, state, { clock: s.clock ?? { day: 0, halfhour: 0 }, clauses: s.clauses }, {
    firstRecordId,
    idGen: () => `x${firstRecordId}_${++n}`, // unique across steps: keyed by record id
  })
}

const snap = (s: State): string => JSON.stringify({
  ents: [...s.entities.entries()].sort(([a], [b]) => a < b ? -1 : 1),
  rels: [...s.relations.entries()].sort(([a], [b]) => a < b ? -1 : 1),
  byName: [...s.byName.entries()].sort(([a], [b]) => a < b ? -1 : 1),
  now: s.now,
})

describe('READ ladder — first match wins, most specific first (§6.4)', () => {
  test('solo reader with knowledge passes the gate; default narrates', () => {
    const { state } = setup()
    const r = run(state, '0:28 @ ANA READ MAGICK_BOOK')
    expect(r.rejected).toBeNull()
    expect(r.records[0].recipes).toEqual(['read.default'])
    expect(r.records[0].advancedMin).toBe(60)
    expect(r.state.now).toBe(14 * 60 + 60)
  })

  test('magick gate REFUSEs an ignorant reader: no state change, no time, attempt logged (§6.5)', () => {
    const { state } = setup()
    const before = snap(state)
    const r = run(state, '0:28 @ BERTHA READ MAGICK_BOOK')
    expect(r.rejected).toBeNull()
    expect(r.records).toHaveLength(1)
    expect(r.records[0].outcomes.map(o => o.op)).toEqual(['REFUSE'])
    expect(r.records[0].advancedMin).toBe(0)
    expect(r.state).toBe(state) // same object, untouched
    expect(snap(r.state)).toBe(before)
    expect(r.records[0].sentence).toContain('REFUSE("The letters swim like minnows.")')
  })

  test('parent + child reading fairy tales branches to bonding memory', () => {
    const { state } = setup()
    const r = run(state, '0:28 @ ANA, BERTHA READ FAIRY_TALES')
    expect(r.records[0].recipes).toEqual(['read.parent_child'])
    const spawn = r.records[0].outcomes[0]
    expect(spawn.op).toBe('SPAWN')
    const nameArg = spawn.args[0]
    expect(nameArg && nameArg.t === 'ref' && nameArg.ref.form === 'name' && nameArg.ref.name).toBe('BONDING_MEMORY')
    expect(r.state.byName.has('BONDING_MEMORY')).toBe(true)
  })

  test('two unrelated readers get the poorer outcome — no spawn', () => {
    const { state } = setup()
    const r = run(state, '0:28 @ ANA, MY_FOLLOWER READ FAIRY_TALES')
    expect(r.records[0].recipes).toEqual(['read.unrelated_pair'])
    expect(r.records[0].outcomes.every(o => o.op === 'NARRATE')).toBe(true)
  })

  test('prototype-parameterized spawn resolves into a concrete named card (§9.15)', () => {
    const { state } = setup()
    const r = run(state, '0:28 @ ANA READ THE_FORGETTING')
    expect(r.records[0].recipes).toEqual(['read.secret'])
    const spawn = r.records[0].outcomes[0]
    const nameArg = spawn.args[0]
    expect(nameArg && nameArg.t === 'ref' && nameArg.ref.form === 'name' && nameArg.ref.name).toBe('CLUE_THE_FORGETTING')
    const asId = spawn.args.find(a => a.t === 'asId')
    expect(asId).toBeDefined()
    const minted = r.state.byName.get('CLUE_THE_FORGETTING')!
    expect(r.state.entities.get(minted)!.aspects['secret']).toBe(2) // 4 - 2
    expect(r.state.entities.get(minted)!.bornAction).toBe(1)
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
    expect(r.records[0].recipes).toEqual(['talk.plan'])
    expect(r.state.byName.has('PLAN_INFILTRATION')).toBe(true)
  })
})

describe('multi-clause steps and time ownership (§9.12)', () => {
  test('travel then read: durations sum, recipes listed in clause order', () => {
    const { state } = setup()
    const r = run(state, '0:28 @ ANA TRAVEL_TO STUDY; ANA AT STUDY READ THE_FORGETTING')
    expect(r.records[0].recipes).toEqual(['travel_to.default', 'read.secret'])
    expect(r.records[0].advancedMin).toBe(120)
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
    expect(r.records[0].outcomes.map(o => o.op)).toEqual(['REFUSE'])
    expect(r.state.now).toBe(14 * 60)
    expect(snap(r.state)).toBe(before)
  })

  test('pooled lantern meets demand: rite consumed, instruments destroyed, mark spawned INTO place', () => {
    const { state } = setup()
    const r = run(state, '0:28 @ ANA, MY_FOLLOWER AT HOUSE USE_ON RITE_OF_ASCENSION WITH CHALICE')
    expect(r.rejected).toBeNull()
    expect(r.state.entities.get('rite')!.deadAction).toBe(1)
    expect(r.state.entities.get('chalice')!.deadAction).toBe(1)
    const mark = r.state.byName.get('MARK_OF_ASCENSION')!
    expect(r.state.entities.get(mark)!.attrs['container']).toBe('house')
    expect(r.records[0].advancedMin).toBe(180)
  })
})

describe('events — authored triggers and expiries (§6.6, §9.16)', () => {
  test('crossing hour 16 fires dusk_falls as its own EVENT row', () => {
    const { state } = setup()
    const r1 = run(state, '0:28 @ ANA WAIT') // crosses 15 only
    expect(r1.records).toHaveLength(1)
    const r2 = run(r1.state, '0:30 @ ANA WAIT') // crosses 16 → dusk
    expect(r2.records).toHaveLength(2)
    const [action, event] = r2.records
    expect(event.kind).toBe('event')
    expect(event.trigger).toBe('dusk_falls')
    expect(event.tGame).toBe('0:32')
    expect(action.sentence).toContain('->')
    expect(r2.state.byName.has('DREAD')).toBe(true)
  })

  test('expiry crossing destroys the card via an expire event', () => {
    const { state } = setup()
    state.entities.get('chalice')!.attrs['expiresAt'] = 16 * 60 // 0:32
    const r = run(state, '0:28 @ ANA WAIT; ANA WAIT; ANA WAIT') // 14 → 17
    // hour 16 fires dusk_falls (authored) AND the chalice expiry — deterministic order
    expect(r.records.map(x => x.kind)).toEqual(['action', 'event', 'event'])
    expect(r.records[1].trigger).toBe('dusk_falls')
    expect(r.records[2].trigger).toBe('expire:CHALICE')
    expect(r.state.entities.get('chalice')!.deadAction).toBe(3)
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
      '0:34 @ ANA TRAVEL_TO STUDY; ANA AT STUDY READ MAGICK_BOOK',
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
    const r2 = run(r1.state, '0:32 @ ANA READ FAIRY_TALES', r1.records[0].id + 1)
    const all = [...r1.records, ...r2.records]
    const undone = foldUpTo(all, seedState(), dict, content, 2)
    expect(snap(undone)).toBe(snapshotAfterOne)
  })
})

describe('dictionary is the law (§9.14)', () => {
  test('SPAWN cannot mint a duplicate name among the living (§9.14)', () => {
    const { state } = setup()
    const r = run(state, '0:28 @ ANA, BERTHA READ FAIRY_TALES') // mints BONDING_MEMORY
    expect(r.rejected).toBeNull()
    const r2 = run(r.state, '0:32 @ ANA, BERTHA READ FAIRY_TALES')
    expect(r2.rejected).toMatch(/already in use/)
    expect(r2.records).toHaveLength(0)
  })

  test('names of dead entities are reusable (§9.14: unique among the LIVING)', () => {
    const { state } = setup()
    const r = run(state, '0:28 @ ANA, MY_FOLLOWER AT HOUSE USE_ON RITE_OF_ASCENSION WITH CHALICE')
    expect(r.rejected).toBeNull()
    expect(r.state.entities.get('chalice')!.deadAction).not.toBeNull()
    expect(r.state.byName.has('CHALICE')).toBe(false)
    // a new CHALICE can be minted after the old one died
    const rec = {
      id: 99, kind: 'action' as const, trigger: null, tGame: '0:40', tReal: null, advancedMin: 0,
      actors: [], verb: '', objects: [], instruments: [], at: null, about: null, to: null,
      outcomes: [{ op: 'SPAWN', args: [{ t: 'ref', ref: { form: 'name', kind: 'item', name: 'CHALICE' } }, { t: 'asId', id: 'new_chalice' }] }],
      recipes: [], sentence: '',
    }
    applyRecord(rec as ActionRecord, r.state, dict, content)
    expect(r.state.byName.get('CHALICE')).toBe('new_chalice')
  })
})

describe('review regressions', () => {
  test('SET stores the given value positionally (ref, key, value)', () => {
    const { state } = setup()
    const rec = {
      id: 1, kind: 'action' as const, trigger: null, tGame: '0:28', tReal: null, advancedMin: 0,
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
      id: 1, kind: 'action' as const, trigger: null, tGame: '0:28', tReal: null, advancedMin: 0,
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
        id, kind: 'action', trigger: null, tGame: '0:0', tReal: null, advancedMin: 0,
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
    const move = r.records[0].outcomes.find(o => o.op === 'MOVE')!
    const refs = move.args.filter((a): a is Extract<import('../src/sentence.js').Arg, { t: 'ref' }> => a.t === 'ref')
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

  test('application failures become rejections, not throws (§6.6)', () => {
    const { state } = setup()
    const r = run(state, '0:28 @ ANA, BERTHA READ FAIRY_TALES')
    expect(r.rejected).toBeNull()
    const r2 = run(r.state, '0:32 @ ANA, BERTHA READ FAIRY_TALES') // duplicate mint name
    expect(r2.rejected).toMatch(/already in use/)
    expect(r2.records).toHaveLength(0)
    expect(r2.state).toBe(r.state)
  })
})

describe('exists(...) recipes — world-scope conditions (§6.3, CS extantreqs)', () => {
  const probeDict = seedDictionary()
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
    return executeStep(theContent, probeDict, state, { clock: s.clock ?? { day: 0, halfhour: 0 }, clauses: s.clauses }, {
      firstRecordId,
      idGen: () => `x${firstRecordId}_${++n}`,
    })
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
    addEntity(state, { id: 'hunter', kind: 'character', name: 'HUNTER', aspects: { hunter: 1 }, attrs: { container: 'villa' }, bornAction: null, deadAction: null })
    const r = runProbe(gate(), state, '0:28 @ ANA PROBE')
    expect(r.rejected).toBeNull()
    expect(r.records[0].recipes).toEqual(['probe.hunted'])
    expect(r.records[0].sentence).toContain('A hunter walks the world.')
  })

  test('a ward anywhere in the world flips the branch — NOT exists(kind:item aspect:ward>=1) fails', () => {
    const { state } = setup()
    addEntity(state, { id: 'hunter', kind: 'character', name: 'HUNTER', aspects: { hunter: 1 }, attrs: { container: 'villa' }, bornAction: null, deadAction: null })
    addEntity(state, { id: 'ward', kind: 'item', name: 'IRON_WARD', aspects: { ward: 2 }, attrs: { container: 'greenhouse' }, bornAction: null, deadAction: null })
    const r = runProbe(gate(), state, '0:28 @ ANA PROBE')
    expect(r.records[0].recipes).toEqual(['probe.default'])
  })

  test('destroyed cards stop existing: the gate un-latches after the hunter dies', () => {
    const { state } = setup()
    addEntity(state, { id: 'hunter', kind: 'character', name: 'HUNTER', aspects: { hunter: 1 }, attrs: { container: 'villa' }, bornAction: null, deadAction: null })
    state.entities.get('hunter')!.deadAction = 99 // DESTROY shape: stamp + release the name (§9.14)
    state.byName.delete('HUNTER')
    const r = runProbe(gate(), state, '0:28 @ ANA PROBE')
    expect(r.records[0].recipes).toEqual(['probe.default'])
  })

  test('name form: exists(THE_SEALED_DOOR) sees a named card across the map', () => {
    const { state } = setup()
    addEntity(state, { id: 'sealed_door', kind: 'place', name: 'THE_SEALED_DOOR', aspects: {}, attrs: { container: 'villa' }, bornAction: null, deadAction: null })
    const c = probeContent([
      { id: 'probe.door', when: 'exists(THE_SEALED_DOOR)', out: ['NARRATE("The door still stands.")'] },
      { out: ['NARRATE("No door.")'] },
    ])
    const r = runProbe(c, state, '0:28 @ ANA PROBE')
    expect(r.records[0].recipes).toEqual(['probe.door'])
  })

  test('relations are untouched by exists evaluation: rel gates still select as before', () => {
    const { state } = setup()
    const c = probeContent([
      { id: 'probe.bonded', when: 'exists(aspect:lantern>=1) AND rel(parent_child, actors)', out: ['NARRATE("Kin in a haunted world.")'] },
      { out: ['NARRATE("No kinship.")'] },
    ])
    const r = runProbe(c, state, '0:28 @ ANA, BERTHA PROBE') // ANA is BERTHA's parent (seed edge)
    expect(r.records[0].recipes).toEqual(['probe.bonded'])
    const r2 = runProbe(c, state, '0:30 @ ANA, MY_FOLLOWER PROBE', 2) // no parent_child edge
    expect(r2.records[0].recipes).toEqual(['probe.default'])
  })
})

describe('progress ticks — the processing read-bar (§6.7)', () => {
  test('ticks record in emission order, touch no state, cost no extra time', () => {
    const { state } = setup()
    const r = run(state, '0:28 @ ANA, MY_FOLLOWER AT HOUSE USE_ON RITE_OF_ASCENSION WITH CHALICE')
    expect(r.rejected).toBeNull()
    expect(r.records[0].outcomes.map(o => o.op)).toEqual(['PROGRESS', 'PROGRESS', 'DESTROY', 'DESTROY', 'SPAWN'])
    expect(r.records[0].advancedMin).toBe(180) // the verb's duration only
    expect(r.records[0].sentence).toContain('PROGRESS(1h, "The circle begins to hum against your palms.")')
    // the log alone reproduces the played state exactly (ticks are journal-only at fold)
    const replayed = fold(r.records, seedState(), dict, content)
    expect(snap(replayed)).toBe(snap(r.state))
  })

  test('malformed PROGRESS arity is an application failure, not a silent no-op (§9.19)', () => {
    const { state } = setup()
    const rec = {
      id: 1, kind: 'action' as const, trigger: null, tGame: '0:28', tReal: null, advancedMin: 0,
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
    expect(r2.records.map(x => x.kind)).toEqual(['action', 'event'])
    expect(r2.records[1].trigger).toBe('the_chime')
    expect(r2.records[1].tGame).toBe('0:29')
  })

  test('durations spill into the next day', () => {
    const { state } = setup()
    const r = run(state, '0:44 @ ANA WORK AT HOUSE') // 2h → day 1, 0:00
    expect(r.state.now).toBe(22 * 60 + 120)
    expect(r.records[0].tGame).toBe('0:44')
  })

  test('formatDuration renders zero as 0m', () => {
    expect(formatDuration(0)).toBe('0m')
  })
})

describe('record shape (§7)', () => {
  test('records carry sorted actors, provenance, canonical sentence', () => {
    const { state } = setup()
    const r = run(state, '0:28 @ BERTHA, ANA AT DINING_HALL TALK ABOUT CLUE_SECRET_HIDEOUT')
    const rec: ActionRecord = r.records[0]
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
  const tuneDict = seedDictionary()
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
    return executeStep(theContent, tuneDict, state, { clock: s.clock ?? { day: 0, halfhour: 0 }, clauses: s.clauses }, {
      firstRecordId,
      idGen: () => `x${firstRecordId}_${++n}`,
    })
  }

  test('a signed value adds to the attr (missing = 0) and the log carries the absolute', () => {
    const { state } = setup()
    const c = counterContent([{ out: ['SET(ANA, funds, +2)'] }])
    const r = runTune(c, state, '0:20 @ ANA TUNE')
    expect(r.rejected).toBeNull()
    expect(r.state.entities.get('ana')!.attrs['funds']).toBe(2) // missing entry counted as 0
    const set = r.records[0].outcomes[0]
    expect(set.op).toBe('SET')
    expect(set.args[2]).toEqual({ t: 'int', v: 2 }) // the record holds the concrete absolute, not the delta
    expect(r.records[0].sentence).toContain('-> SET(ANA, funds, 2)') // name ref kept, value absolutized
    // the concrete sentence round-trips
    expect(formatSentence(parseSentence(r.records[0].sentence, tuneDict), tuneDict)).toBe(r.records[0].sentence)
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
    expect(r.records[0].sentence).toContain('SET(#ana, funds, -3)')
  })

  test('aspect: + delta increments the aspects map; an absolute aspect set writes it too', () => {
    const { state } = setup()
    const c = counterContent([{ out: ['SET(ACTORS, aspect:dread, +1)'] }])
    const r = runTune(c, state, '0:20 @ ANA TUNE')
    expect(r.rejected).toBeNull()
    expect(r.state.entities.get('ana')!.aspects['dread']).toBe(1)
    expect(r.records[0].sentence).toContain('SET(#ana, aspect:dread, 1)')
    // absolute form through the fold path (concrete outcomes replay as-is)
    const s = parseSentence('0:0 @ ANA WAIT -> SET(#ana, aspect:language.latin, 2)', dict)
    applyRecord({
      id: 2, kind: 'action', trigger: null, tGame: '0:0', tReal: null, advancedMin: 0,
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
    expect(r2.records[0].sentence).toContain('SET(ACTORS, funds, 5)')
  })

  test('a delta over a role ref logs one concrete SET per card, each with its own result', () => {
    const { state } = setup()
    state.entities.get('ana')!.aspects['dread'] = 2 // the two bases differ
    const c = counterContent([{ id: 'tune.gain', when: 'count(actors) = 2', out: ['SET(ACTORS, aspect:dread, +1)'] }])
    const r = runTune(c, state, '0:20 @ ANA, BERTHA TUNE')
    expect(r.rejected).toBeNull()
    expect(r.records[0].outcomes.filter(o => o.op === 'SET')).toHaveLength(2)
    expect(r.records[0].sentence).toContain('SET(#ana, aspect:dread, 3), SET(#bertha, aspect:dread, 1)')
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
    expect(r.records[0].outcomes.map(o => o.op)).toEqual(['SET', 'SET', 'REFUSE'])
    expect(r.records[0].advancedMin).toBe(0)
    expect(r.state).toBe(state) // same object, untouched — the deltas ran on the discarded draft
    expect(snap(r.state)).toBe(before)
    expect(snap(fold(r.records, seedState(), tuneDict, c))).toBe(before) // refused record folds as a no-op
  })

  test('a delta on a non-numeric entry rejects the whole step — nothing logged (§9.19)', () => {
    const { state } = setup()
    const c = counterContent([{ out: ['SET(ACTORS, container, +1)'] }]) // container is a string attr
    const r = runTune(c, state, '0:20 @ ANA TUNE')
    expect(r.rejected).toMatch(/non-numeric/)
    expect(r.records).toHaveLength(0)
    expect(r.state).toBe(state)
  })

  test('a delta on an aspect sub-map rejects the step — the map survives (§9.31)', () => {
    const { state } = setup()
    state.entities.get('ana')!.aspects['language'] = { french: 2 } // a sub-map, not a number
    const c = counterContent([{ out: ['SET(ACTORS, aspect:language, +1)'] }])
    const r = runTune(c, state, '0:20 @ ANA TUNE')
    expect(r.rejected).toMatch(/non-numeric/)
    expect(r.records).toHaveLength(0)
    expect(r.state).toBe(state)
    expect(r.state.entities.get('ana')!.aspects['language']).toEqual({ french: 2 })
  })

  test('a quoted key stays quoted in the concrete record (§9.15)', () => {
    const { state } = setup()
    const c = counterContent([{ out: ['SET(ACTORS, "about", +5)'] }]) // 'about' is a reserved role word
    const r = runTune(c, state, '0:20 @ ANA TUNE')
    expect(r.rejected).toBeNull()
    expect(r.state.entities.get('ana')!.attrs['about']).toBe(5)
    expect(r.records[0].sentence).toContain('SET(#ana, "about", 5)')
    // the record must round-trip: unquoted it would re-parse as the about ROLE
    expect(formatSentence(parseSentence(r.records[0].sentence, tuneDict), tuneDict)).toBe(r.records[0].sentence)
  })
})
