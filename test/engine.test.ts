// Engine acceptance tests — the spec §6.4 READ ladder, REFUSE semantics (§6.5),
// time ownership (§9.12), events (§9.16), fold determinism (§7).
import { describe, expect, test } from 'vitest'
import { compileContent } from '../src/content.js'
import { applyRecord, executeStep, type ActionRecord, type ExecResult } from '../src/engine.js'
import { fold, foldUpTo } from '../src/fold.js'
import { parseSentence } from '../src/parse.js'
import { seedContent, seedDictionary, seedState } from '../src/seed.js'
import { loadLog, saveLog, Log } from '../src/store.js'
import { newState, type State } from '../src/types.js'
import { mkdtempSync, rmSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'

const dict = seedDictionary()
const content = compileContent(seedContent(), dict)

function setup() {
  return { dict, content, state: seedState() }
}

function run(state: State, src: string, firstRecordId = 1): ExecResult {
  const s = parseSentence(src, dict)
  let n = 0
  return executeStep(content, dict, state, { clock: s.clock ?? { day: 0, hour: 0 }, clauses: s.clauses }, {
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
    const r = run(state, '0:14 @ ANA READ MAGICK_BOOK')
    expect(r.rejected).toBeNull()
    expect(r.records[0].recipes).toEqual(['read.default'])
    expect(r.records[0].advancedMin).toBe(60)
    expect(r.state.now).toBe(14 * 60 + 60)
  })

  test('magick gate REFUSEs an ignorant reader: no state change, no time, attempt logged (§6.5)', () => {
    const { state } = setup()
    const before = snap(state)
    const r = run(state, '0:14 @ BERTHA READ MAGICK_BOOK')
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
    const r = run(state, '0:14 @ ANA, BERTHA READ FAIRY_TALES')
    expect(r.records[0].recipes).toEqual(['read.parent_child'])
    const spawn = r.records[0].outcomes[0]
    expect(spawn.op).toBe('SPAWN')
    const nameArg = spawn.args[0]
    expect(nameArg && nameArg.t === 'ref' && nameArg.ref.form === 'name' && nameArg.ref.name).toBe('BONDING_MEMORY')
    expect(r.state.byName.has('BONDING_MEMORY')).toBe(true)
  })

  test('two unrelated readers get the poorer outcome — no spawn', () => {
    const { state } = setup()
    const r = run(state, '0:14 @ ANA, MY_FOLLOWER READ FAIRY_TALES')
    expect(r.records[0].recipes).toEqual(['read.unrelated_pair'])
    expect(r.records[0].outcomes.every(o => o.op === 'NARRATE')).toBe(true)
  })

  test('prototype-parameterized spawn resolves into a concrete named card (§9.15)', () => {
    const { state } = setup()
    const r = run(state, '0:14 @ ANA READ THE_FORGETTING')
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
    const r = run(state, '0:14 @ ANA, BERTHA, MY_FOLLOWER READ FAIRY_TALES')
    expect(r.rejected).toMatch(/'actors' takes 1..2/)
    expect(r.records).toHaveLength(0)
  })

  test('slot discipline: undeclared roles are rejected', () => {
    const { state } = setup()
    const r = run(state, '0:14 @ ANA READ FAIRY_TALES WITH CHALICE')
    expect(r.rejected).toMatch(/does not accept 'with'/)
    expect(r.records).toHaveLength(0)
  })
})

describe('TALK with ABOUT (§4.3)', () => {
  test('talking about a secret idea spawns a plan', () => {
    const { state } = setup()
    const r = run(state, '0:14 @ ANA, BERTHA AT DINING_HALL TALK ABOUT CLUE_SECRET_HIDEOUT')
    expect(r.records[0].recipes).toEqual(['talk.plan'])
    expect(r.state.byName.has('PLAN_INFILTRATION')).toBe(true)
  })
})

describe('multi-clause steps and time ownership (§9.12)', () => {
  test('travel then read: durations sum, recipes listed in clause order', () => {
    const { state } = setup()
    const r = run(state, '0:14 @ ANA TRAVEL_TO STUDY; ANA AT STUDY READ THE_FORGETTING')
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
    const r = run(state, '0:14 @ ANA AT HOUSE USE_ON RITE_OF_ASCENSION')
    expect(r.records[0].outcomes.map(o => o.op)).toEqual(['REFUSE'])
    expect(r.state.now).toBe(14 * 60)
    expect(snap(r.state)).toBe(before)
  })

  test('pooled lantern meets demand: rite consumed, instruments destroyed, mark spawned INTO place', () => {
    const { state } = setup()
    const r = run(state, '0:14 @ ANA, MY_FOLLOWER AT HOUSE USE_ON RITE_OF_ASCENSION WITH CHALICE')
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
    const r1 = run(state, '0:14 @ ANA WAIT') // crosses 15 only
    expect(r1.records).toHaveLength(1)
    const r2 = run(r1.state, '0:15 @ ANA WAIT') // crosses 16 → dusk
    expect(r2.records).toHaveLength(2)
    const [action, event] = r2.records
    expect(event.kind).toBe('event')
    expect(event.trigger).toBe('dusk_falls')
    expect(event.tGame).toBe('0:16')
    expect(action.sentence).toContain('->')
    expect(r2.state.byName.has('DREAD')).toBe(true)
  })

  test('expiry crossing destroys the card via an expire event', () => {
    const { state } = setup()
    state.entities.get('chalice')!.attrs['expiresAt'] = 16 * 60 // 0:16
    const r = run(state, '0:14 @ ANA WAIT; ANA WAIT; ANA WAIT') // 14 → 17
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
      '0:14 @ ANA READ THE_FORGETTING',
      '0:15 @ ANA, BERTHA READ FAIRY_TALES', // ends 0:16 → dusk event row
      '0:16 @ ANA, BERTHA AT DINING_HALL TALK ABOUT CLUE_SECRET_HIDEOUT',
      '0:17 @ ANA TRAVEL_TO STUDY; ANA AT STUDY READ MAGICK_BOOK',
      '0:20 @ ANA WAIT',
      '0:21 @ ANA WAIT',
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
    const r = run(state, '0:14 @ BERTHA READ MAGICK_BOOK')
    const replayed = fold(r.records, seedState(), dict, content)
    expect(snap(replayed)).toBe(before)
  })

  test('JSONL persistence round-trips the log (§7)', () => {
    const { state } = setup()
    const r = run(state, '0:14 @ ANA READ THE_FORGETTING')
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
    const r1 = run(state, '0:14 @ ANA READ THE_FORGETTING')
    const snapshotAfterOne = snap(r1.state)
    const r2 = run(r1.state, '0:16 @ ANA READ FAIRY_TALES', r1.records[0].id + 1)
    const all = [...r1.records, ...r2.records]
    const undone = foldUpTo(all, seedState(), dict, content, 2)
    expect(snap(undone)).toBe(snapshotAfterOne)
  })
})

describe('dictionary is the law (§9.14)', () => {
  test('SPAWN cannot mint a duplicate name among the living (§9.14)', () => {
    const { state } = setup()
    const r = run(state, '0:14 @ ANA, BERTHA READ FAIRY_TALES') // mints BONDING_MEMORY
    expect(r.rejected).toBeNull()
    const r2 = run(r.state, '0:16 @ ANA, BERTHA READ FAIRY_TALES')
    expect(r2.rejected).toMatch(/already in use/)
    expect(r2.records).toHaveLength(0)
  })

  test('names of dead entities are reusable (§9.14: unique among the LIVING)', () => {
    const { state } = setup()
    const r = run(state, '0:14 @ ANA, MY_FOLLOWER AT HOUSE USE_ON RITE_OF_ASCENSION WITH CHALICE')
    expect(r.rejected).toBeNull()
    expect(r.state.entities.get('chalice')!.deadAction).not.toBeNull()
    expect(r.state.byName.has('CHALICE')).toBe(false)
    // a new CHALICE can be minted after the old one died
    const rec = {
      id: 99, kind: 'action' as const, trigger: null, tGame: '0:20', tReal: null, advancedMin: 0,
      actors: [], verb: '', objects: [], instruments: [], at: null, about: null,
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
      id: 1, kind: 'action' as const, trigger: null, tGame: '0:14', tReal: null, advancedMin: 0,
      actors: [], verb: '', objects: [], instruments: [], at: null, about: null,
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
      id: 1, kind: 'action' as const, trigger: null, tGame: '0:14', tReal: null, advancedMin: 0,
      actors: [], verb: '', objects: [], instruments: [], at: null, about: null,
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
        actors: [], verb: '', objects: [], instruments: [], at: null, about: null,
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
    const r = run(state, '0:14 @ ANA WAIT; ANA WAIT; ANA WAIT') // 14→17, events at 15/16
    expect(r.records.filter(x => x.kind === 'event').length).toBeGreaterThanOrEqual(2)
    const replayed = fold(r.records, seedState(), dict, content)
    expect(replayed.now).toBe(r.state.now) // events must not drag the clock back
    expect(replayed.now).toBe(17 * 60)
  })

  test('role sugar is concretized in the log (§9.15): replay matches play exactly', () => {
    const { state } = setup()
    const r = run(state, '0:14 @ ANA, BERTHA TRAVEL_TO VILLA; MY_FOLLOWER WAIT')
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
    const r = run(state, '0:3 @ ANA WAIT')
    expect(r.rejected).toMatch(/before state\.now/)
    expect(r.records).toHaveLength(0)
  })

  test('application failures become rejections, not throws (§6.6)', () => {
    const { state } = setup()
    const r = run(state, '0:14 @ ANA, BERTHA READ FAIRY_TALES')
    expect(r.rejected).toBeNull()
    const r2 = run(r.state, '0:16 @ ANA, BERTHA READ FAIRY_TALES') // duplicate mint name
    expect(r2.rejected).toMatch(/already in use/)
    expect(r2.records).toHaveLength(0)
    expect(r2.state).toBe(r.state)
  })
})

describe('clock & duration literals', () => {
  test('durations spill into the next day', () => {
    const { state } = setup()
    const r = run(state, '0:22 @ ANA WORK AT HOUSE') // 2h → day 1, 0:00
    expect(r.state.now).toBe(22 * 60 + 120)
    expect(r.records[0].tGame).toBe('0:22')
  })
})

describe('record shape (§7)', () => {
  test('records carry sorted actors, provenance, canonical sentence', () => {
    const { state } = setup()
    const r = run(state, '0:14 @ BERTHA, ANA AT DINING_HALL TALK ABOUT CLUE_SECRET_HIDEOUT')
    const rec: ActionRecord = r.records[0]
    expect(rec.actors.map(a => a.form === 'name' && a.name)).toEqual(['ANA', 'BERTHA'])
    expect(rec.verb).toBe('TALK')
    expect(rec.tGame).toBe('0:14')
    expect(rec.sentence.startsWith('0:14 @ ANA, BERTHA AT DINING_HALL TALK')).toBe(true)
  })
})

describe('empty base state fold (fresh world, no content spawns)', () => {
  test('fold of an empty log returns the base state', () => {
    const base = newState()
    expect(fold([], base, dict, content).entities.size).toBe(0)
  })
})
