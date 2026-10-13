// F2 — conditionals: IF / ELSE IF / ELSE in outcome lists (spec §6.8, §9.39,
// plan v0.4 F2). Branch selection runs against the DRAFT (earlier outcomes in
// the same list are visible to later IFs); first true branch wins; the log
// records only the taken branch's outcomes, flattened in position — the
// record never contains an IF op. Asymmetric failure discipline: a
// condition operand that cannot resolve rejects the step (§9.19) where a
// `when:` predicate would read false. The trigger guards and the endings
// check recurse into branch bodies; role-dependent operands in trigger IF
// conditions are a load-time error.
import { describe, expect, test } from 'vitest'
import { compileContent, type CompiledContent, type ContentRaw, type TriggerDef } from '../src/content.js'
import { coreDictionary } from '../src/dict.js'
import { executeStep, type ActionRecord, type ExecResult } from '../src/engine.js'
import { fold } from '../src/fold.js'
import { formatSentence } from '../src/format.js'
import type { Outcome } from '../src/sentence.js'
import { parseOutcomes, parseSentence } from '../src/parse.js'
import { compileScript } from '../src/script.js'
import { loadLog, saveLog } from '../src/store.js'
import { addEntity, cloneState, newState, type State } from '../src/types.js'
import { seedState } from './seed.js'
import { snap } from './snap.js'
import { settleStep, act, done, refusal } from './settle.js'
import { mkdtempSync, rmSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'

const dict = coreDictionary()
// the rig's own verb lexemes (verbs are code-registered, §10)
dict.registerVerb('INSPECT')
dict.registerVerb('ATTUNE')
dict.registerVerb('STEW')

function run(content: CompiledContent, state: State, src: string, firstRecordId = 1): ExecResult {
  const s = parseSentence(src, dict)
  let n = 0
  return settleStep(content, dict, state, { clock: s.clock ?? { day: 0, halfhour: 0 }, clauses: s.clauses }, firstRecordId, () => `x${firstRecordId}_${++n}`)
}

const narrated = (outcomes: Outcome[]): string[] =>
  outcomes.filter(o => o.op === 'NARRATE').map(o => {
    const a = o.args[0]
    return a && a.t === 'str' ? a.v : '(not a string)'
  })
// v0.5: NARRATE defers to the completion — gather across the COMPLETE rows
const said = (r: ExecResult): string[] =>
  r.records.filter(x => x.kind === 'complete').flatMap(x => narrated(x.outcomes))

/** a one-verb rig: INSPECT takes actors 1..2 and an optional object */
function inspectContent(out: string[], extra: Partial<ContentRaw> = {}): CompiledContent {
  return compileContent({
    verbs: {
      INSPECT: {
        slots: [
          { role: 'actors', min: 1, max: 2, query: 'kind:character' },
          { role: 'object', min: 0, max: 1 },
        ],
        duration: 0,
        recipes: [{ out }],
      },
    },
    ...extra,
  }, dict)
}

/** a one-verb rig with a REQUIRED object role (ATTUNE) */
function attuneContent(out: string[], duration = 0): CompiledContent {
  return compileContent({
    verbs: {
      ATTUNE: {
        slots: [
          { role: 'actors', min: 1, max: 1, query: 'kind:character' },
          { role: 'object', min: 1, max: 1 },
        ],
        duration,
        recipes: [{ out }],
      },
    },
  }, dict)
}

/** the seed world with every lantern stripped — exists(aspect:lantern>=1) is false */
function dimWorld(): State {
  const s = seedState()
  for (const e of s.entities.values()) delete e.aspects['lantern']
  return s
}

// ---------- grammar (§5.1, §9.39) ----------

describe('IF / ELSE IF / ELSE grammar (§5.1, §9.39)', () => {
  test('parses into arms + fallback; the ELSE chain is consumed inside the IF node', () => {
    // the plan's flagship example verbatim
    const out = parseOutcomes(
      `PROGRESS(30m, "You work the crowd."),
       IF count(name:SUSPICION) >= 3 -> (NARRATE "Too many eyes.", SPAWN PARANOIA)
       ELSE IF exists(THE_BRAZEN_COP) -> (NARRATE "The cop watches.")
       ELSE -> (NARRATE "Nobody notices."),
       NARRATE "after"`,
      dict,
    )
    expect(out).toHaveLength(3) // PROGRESS, IF, NARRATE — a stranded ELSE would end the list early
    expect(out[0]!.op).toBe('PROGRESS')
    expect(out[1]!.op).toBe('IF')
    expect(out[2]!.op).toBe('NARRATE')
    expect(out[1]!.arms).toHaveLength(2)
    expect(out[1]!.arms![0]!.then.map(o => o.op)).toEqual(['NARRATE', 'SPAWN'])
    expect(out[1]!.arms![1]!.then.map(o => o.op)).toEqual(['NARRATE'])
    expect(out[1]!.fallback!.map(o => o.op)).toEqual(['NARRATE'])
  })

  test('conditions keep their commas: rel()/every() never split the chain', () => {
    const out = parseOutcomes(
      `IF count(actors) = 2 AND rel(parent_child, actors) AND every(actors, aspect:occult >= 1) -> (NARRATE "a")`,
      dict,
    )
    expect(out).toHaveLength(1)
    expect(out[0]!.op).toBe('IF')
  })

  test('a nested ELSE binds to the nearest IF (greedy)', () => {
    const inner = parseOutcomes(
      `IF exists(A) -> (IF exists(B) -> (NARRATE "in") ELSE -> (NARRATE "inner else"))`,
      dict,
    )
    expect(inner[0]!.fallback).toBeNull()
    expect(narrated(inner[0]!.arms![0]!.then[0]!.fallback!)).toEqual(['inner else'])
    const outer = parseOutcomes(
      `IF exists(A) -> (IF exists(B) -> (NARRATE "in")) ELSE -> (NARRATE "outer else")`,
      dict,
    )
    expect(outer[0]!.arms![0]!.then[0]!.fallback).toBeNull()
    expect(narrated(outer[0]!.fallback!)).toEqual(['outer else'])
  })

  test('IF and ELSE are reserved words (§9.14) — quoted names still parse', () => {
    expect(dict.reserved.has('IF')).toBe(true)
    expect(dict.reserved.has('ELSE')).toBe(true)
    expect(() => parseSentence('0:0 @ ANA WAIT -> SPAWN IF', dict)).toThrow(/reserved/)
    expect(parseOutcomes(`SPAWN "IF"`, dict)).toHaveLength(1) // the quote is the escape hatch
  })

  test('parse errors: missing branch parens, ELSE without IF, dangling ELSE IF, condition without ->', () => {
    expect(() => parseOutcomes(`IF exists(X) -> SPAWN Y`, dict)).toThrow(/expected '\('/) // bodies require parens
    expect(() => parseSentence('0:0 @ ANA WAIT -> SPAWN X ELSE -> (NARRATE "y")', dict)).toThrow(/ELSE/)
    expect(() => parseOutcomes(`IF exists(X) -> (NARRATE "a") ELSE IF`, dict)).toThrow(/->/)
    expect(() => parseOutcomes(`IF exists(X)`, dict)).toThrow(/->/)
    expect(() => parseOutcomes(`IF exists(X), NARRATE "y"`, dict)).toThrow(/->/) // a comma never splits a condition
    expect(() => parseOutcomes(`IF exists(X) ELSE -> (NARRATE "y")`, dict)).toThrow(/->/)
  })

  test('authored outcome strings must parse COMPLETELY — trailing tokens are load errors too', () => {
    // content compile goes through parseOutcomes; a second ELSE, junk after a
    // complete chain, or a forgotten IF keyword must not vanish silently there
    expect(() => parseOutcomes(`IF exists(X) -> (NARRATE "a") ELSE -> (NARRATE "b") ELSE -> (NARRATE "c")`, dict))
      .toThrow(/trailing token 'ELSE'/)
    expect(() => parseOutcomes(`IF exists(X) -> (NARRATE "a") GARBAGE`, dict)).toThrow(/trailing token 'GARBAGE'/)
    // v0.3.3: a name followed by '(' in outcome position is now a function-call
    // dispatch — an undeclared one is a compile error listing the vocabulary,
    // with a forgotten-IF hint for the predicate helpers (§9.40)
    expect(() => parseOutcomes(`exists(X) -> (NARRATE "a")`, dict))
      .toThrow(/unknown 'exists\(\.\.\.\)' in outcome position.*IF keyword/s) // the IF keyword is not optional
    expect(() => parseOutcomes(`SPAWN X ELSE -> (NARRATE "y")`, dict)).toThrow(/trailing token 'ELSE'/)
    expect(parseOutcomes(`IF exists(X) -> (NARRATE "a") ELSE -> (NARRATE "b")`, dict)).toHaveLength(1) // a complete chain still parses
  })
})

// ---------- branch selection & draft visibility (§9.39) ----------

describe('branch selection against the draft (§9.39)', () => {
  test('first true branch wins; later arms never apply', () => {
    const c = inspectContent([
      `IF count(actors) >= 1 -> (NARRATE "first") ELSE IF exists(HOUSE) -> (NARRATE "second") ELSE -> (NARRATE "third")`,
    ])
    const r = run(c, seedState(), '0:0 @ ANA INSPECT CHALICE')
    expect(r.rejected).toBeNull()
    expect(said(r)).toEqual(['first'])
  })

  test('an ELSE IF chain picks the first true arm, skipping false ones', () => {
    const c = inspectContent([
      `IF exists(NOSUCH_CARD) -> (NARRATE "one") ELSE IF exists(HOUSE) -> (NARRATE "two") ELSE -> (NARRATE "three")`,
    ])
    const r = run(c, seedState(), '0:0 @ ANA INSPECT CHALICE')
    expect(r.rejected).toBeNull()
    expect(said(r)).toEqual(['two'])
  })

  test('all false with an ELSE takes the else branch', () => {
    const c = inspectContent([`IF exists(NOSUCH_CARD) -> (NARRATE "a") ELSE -> (NARRATE "b")`])
    const r = run(c, seedState(), '0:0 @ ANA INSPECT CHALICE')
    expect(r.rejected).toBeNull()
    expect(said(r)).toEqual(['b'])
  })

  test('all false without an ELSE applies nothing — no error, step completes', () => {
    const c = inspectContent([
      `NARRATE("before"), IF exists(NOSUCH_CARD) -> (NARRATE "a") ELSE IF exists(NOSUCH_OTHER) -> (NARRATE "b"), NARRATE("after")`,
    ])
    const r = run(c, seedState(), '0:0 @ ANA INSPECT CHALICE')
    expect(r.rejected).toBeNull()
    expect(done(r).outcomes.map(o => o.op)).toEqual(['NARRATE', 'NARRATE'])
    expect(said(r)).toEqual(['before', 'after'])
  })

  test('draft visibility: SET(OBJECT, aspect:level, +2) then IF reads the NEW level, not the input state', () => {
    const c = attuneContent([
      'SET(OBJECT, aspect:level, +2)',
      `IF object.aspect.level >= 2 -> (NARRATE "then — the draft") ELSE -> (NARRATE "else — the input")`,
    ])
    const r = run(c, seedState(), '0:0 @ ANA ATTUNE CHALICE') // CHALICE carries no level aspect
    expect(r.rejected).toBeNull()
    expect(said(r)).toEqual(['then — the draft'])
    // the delta logged its concrete absolute (§9.31), taken branch only
    expect(done(r).outcomes.map(o => o.op)).toEqual(['SET', 'NARRATE'])
    expect(r.state.entities.get('chalice')!.aspects['level']).toBe(2)
  })

  test('world-form conditions too: SPAWN then IF count(name:X) >= 1 fires in the same list', () => {
    const c = inspectContent(
      [`SPAWN WHISPER, IF count(name:WHISPER) >= 1 -> (NARRATE "heard") ELSE -> (NARRATE "silence")`],
      { prototypes: { WHISPER: { kind: 'idea' } } },
    )
    const r = run(c, seedState(), '0:0 @ ANA INSPECT CHALICE')
    expect(r.rejected).toBeNull()
    expect(said(r)).toEqual(['heard'])
  })

  test('draft visibility crosses the branch boundary: a branch spawn is visible to the NEXT IF', () => {
    const c = inspectContent(
      [`IF exists(HOUSE) -> (SPAWN PARANOIA), IF count(name:PARANOIA) >= 1 -> (NARRATE "saw it") ELSE -> (NARRATE "blind")`],
      { prototypes: { PARANOIA: { kind: 'state' } } },
    )
    const r = run(c, seedState(), '0:0 @ ANA INSPECT CHALICE')
    expect(r.rejected).toBeNull()
    expect(done(r).outcomes.map(o => o.op)).toEqual(['SPAWN', 'NARRATE']) // taken branch flattened in position
    expect(said(r)).toEqual(['saw it'])
  })

  test('an IF after a REPLACE reads the successor the SPAWN+DESTROY expansion minted', () => {
    const c = inspectContent([
      'REPLACE OBJECT WITH GHOST',
      `IF exists(GHOST) -> (NARRATE "successor here") ELSE -> (NARRATE "no successor")`,
    ], { prototypes: { GHOST: { kind: 'idea' } } })
    const r = run(c, seedState(), '0:0 @ ANA INSPECT RITE_OF_ASCENSION')
    expect(r.rejected).toBeNull()
    // the expansion's concrete SPAWN+DESTROY pair, then the taken branch — in position
    expect(done(r).outcomes.map(o => o.op)).toEqual(['SPAWN', 'DESTROY', 'NARRATE'])
    expect(said(r)).toEqual(['successor here'])
    expect(snap(fold(r.records, cloneState(seedState()), dict, c))).toBe(snap(r.state))
  })

  test('multi-clause steps: each clause carries its own bindings, IFs in both apply', () => {
    const c = compileContent({
      verbs: {
        INSPECT: {
          slots: [
            { role: 'actors', min: 1, max: 1, query: 'kind:character' },
            { role: 'object', min: 1, max: 1 },
          ],
          duration: 0,
          recipes: [{ out: [`IF exists(HOUSE) -> (NARRATE "clause one") ELSE -> (NARRATE "clause one void")`] }],
        },
        ATTUNE: {
          slots: [
            { role: 'actors', min: 1, max: 1, query: 'kind:character' },
            { role: 'object', min: 1, max: 1 },
          ],
          duration: 0,
          recipes: [{ out: [`IF exists(NOSUCH_CARD) -> (NARRATE "clause two bad") ELSE -> (NARRATE "clause two else")`] }],
        },
      },
    }, dict)
    const r = run(c, seedState(), '0:0 @ ANA INSPECT CHALICE; ANA ATTUNE RITE_OF_ASCENSION')
    expect(r.rejected).toBeNull()
    expect(said(r)).toEqual(['clause one', 'clause two else'])
    expect(snap(fold(r.records, cloneState(seedState()), dict, c))).toBe(snap(r.state))
  })

  test('the dead-stamp rule: DESTROY OBJECT then IF reads the dead card as absent — silent false', () => {
    const c = attuneContent([
      'DESTROY OBJECT',
      `IF object.aspect.demand >= 1 -> (NARRATE "still there") ELSE -> (NARRATE "gone")`,
    ])
    // RITE_OF_ASCENSION carries demand:3 — destroyed earlier in the SAME list,
    // so the condition reads a dead-stamped entity: false, silently (§9.39)
    const r = run(c, seedState(), '0:0 @ ANA ATTUNE RITE_OF_ASCENSION')
    expect(r.rejected).toBeNull()
    expect(said(r)).toEqual(['gone'])
  })
})

// ---------- concretization (§9.39): the log carries the taken branch only ----------

describe('concretization — the record never contains an IF op (§9.39)', () => {
  const c = inspectContent(
    [`NARRATE("before"), IF exists(HOUSE) -> (PROGRESS(30m, "tick"), SPAWN PARANOIA) ELSE -> (NARRATE "untaken"), NARRATE("after")`],
    { prototypes: { PARANOIA: { kind: 'state' } } },
  )

  test('the taken branch flattens in position; the untaken branch is gone', () => {
    const r = run(c, seedState(), '0:0 @ ANA INSPECT CHALICE')
    expect(r.rejected).toBeNull()
    expect(done(r).outcomes.map(o => o.op)).toEqual(['NARRATE', 'PROGRESS', 'SPAWN', 'NARRATE'])
    const all = JSON.stringify(r.records)
    expect(all).not.toContain('"IF"')
    expect(all).not.toContain('ELSE')
    expect(all).not.toContain('untaken')
    // v0.5: a branch-nested PROGRESS defers with the branch — the completion
    // row carries the flattened, taken branch verbatim
    expect(done(r).sentence).toContain('PROGRESS(1halfhour, "tick")') // canonical duration form
    expect(done(r).sentence).not.toContain('IF ')
  })

  test('the recorded sentence round-trips', () => {
    const r = run(c, seedState(), '0:0 @ ANA INSPECT CHALICE')
    const s = done(r).sentence
    expect(formatSentence(parseSentence(s, dict), dict)).toBe(s)
  })

  test('fold and JSONL equality: replaying the flattened rows reproduces play exactly', () => {
    const state = seedState()
    const r1 = run(c, state, '0:0 @ ANA INSPECT CHALICE')
    const quiet = inspectContent([`IF exists(NOSUCH_CARD) -> (NARRATE "no")`])
    const r2 = run(quiet, r1.state, '0:0 @ ANA INSPECT CHALICE', 2) // no ELSE: nothing to replay
    expect(r2.rejected).toBeNull()
    expect(done(r2).outcomes).toHaveLength(0)
    const dir = mkdtempSync(join(tmpdir(), 'chorus-if-'))
    const path = join(dir, 'log.jsonl')
    try {
      saveLog(path, [...r1.records, ...r2.records])
      const replayed = fold(loadLog(path), cloneState(state), dict, c)
      expect(snap(replayed)).toBe(snap(r2.state))
    } finally {
      rmSync(dir, { recursive: true, force: true })
    }
  })
})

// ---------- REFUSE inside a taken branch (§6.5, §9.39) ----------

describe('REFUSE inside a taken branch refuses the window (§6.5, v0.5 completion)', () => {
  const c = attuneContent(
    [`IF exists(HOUSE) -> (DESTROY OBJECT, REFUSE("not yet")) ELSE -> (NARRATE "calm")`],
    60, // a duration the refusal cancels — the window opens, then collapses
  )

  test('attempt logged at completion, effects discarded, state untouched, fold no-op', () => {
    const state = seedState()
    const before = snap(state)
    const r = run(c, state, '0:0 @ ANA ATTUNE CHALICE')
    expect(r.rejected).toBeNull() // v0.5: the commit is clean; the REFUSE decides at completion
    expect(done(r).outcomes.map(o => o.op)).toEqual(['DESTROY', 'REFUSE'])
    expect(act(r).advancedMin).toBe(60) // the window DID open…
    expect(r.state.now).toBe(state.now + 60) // …and the clock crossed it
    expect(r.state).not.toBe(state)
    expect(snap(state)).toBe(before) // the scratch died with the refusal
    expect(snap(fold(r.records, cloneState(state), dict, c))).toBe(snap(r.state)) // replay agrees, effects skipped
    expect(said(r)).toEqual([]) // the untaken else never spoke
  })
})

// ---------- asymmetric failure discipline (§9.39) ----------

describe('condition failure refuses the window; the same grammar reads false in when: (§9.39)', () => {
  test('an IF condition operand with no bound card — the window refuses at completion', () => {
    const c = inspectContent([`IF object.aspect.demand >= 1 -> (NARRATE "needs")`])
    const state = seedState()
    const before = snap(state)
    const r = run(c, state, '0:0 @ ANA INSPECT') // no object bound: the operand cannot resolve
    expect(r.rejected).toBeNull() // v0.5: strict conditions decide at the completion
    expect(refusal(r)).toMatch(/IF condition: operand 'object\.aspect\.demand' cannot resolve/)
    expect(snap(state)).toBe(before)
  })

  test('the same predicate in a when: position reads false and selects the default', () => {
    const c = compileContent({
      verbs: {
        INSPECT: {
          slots: [
            { role: 'actors', min: 1, max: 1, query: 'kind:character' },
            { role: 'object', min: 0, max: 1 },
          ],
          duration: 0,
          recipes: [
            { id: 'inspect.gate', when: 'object.aspect.demand >= 1', out: ['NARRATE("gate")'] },
            { id: 'inspect.default', out: ['NARRATE("default")'] },
          ],
        },
      },
    }, dict)
    const r = run(c, seedState(), '0:0 @ ANA INSPECT')
    expect(r.rejected).toBeNull()
    expect(act(r).recipes).toEqual(['inspect.default']) // false, not a rejection — the asymmetry
  })

  test('a missing aspect on a BOUND card still reads false inside an IF (legitimate selection, not failure)', () => {
    const c = inspectContent([`IF object.aspect.demand >= 1 -> (NARRATE "needs") ELSE -> (NARRATE "harmless")`])
    const r = run(c, seedState(), '0:0 @ ANA INSPECT CHALICE') // bound, but no demand aspect
    expect(r.rejected).toBeNull()
    expect(said(r)).toEqual(['harmless'])
  })

  test('count/total over an empty role read 0 — branch selection, never a rejection (§9.39 boundary)', () => {
    const count = inspectContent([`IF count(object) >= 1 -> (NARRATE "has") ELSE -> (NARRATE "none")`])
    expect(said(run(count, seedState(), '0:0 @ ANA INSPECT'))).toEqual(['none']) // object slot unbound
    const total = inspectContent([`IF total(object, aspect:demand) >= 1 -> (NARRATE "has") ELSE -> (NARRATE "none")`])
    expect(said(run(total, seedState(), '0:0 @ ANA INSPECT'))).toEqual(['none'])
  })

  test('the documented guard for an OPTIONAL role: count(object) >= 1 AND object.aspect.x … never rejects', () => {
    const c = inspectContent([`IF count(object) >= 1 AND object.aspect.demand >= 1 -> (NARRATE "demands") ELSE -> (NARRATE "no demand")`])
    expect(said(run(c, seedState(), '0:0 @ ANA INSPECT'))).toEqual(['no demand']) // AND short-circuits before the strict path reads
    expect(said(run(c, seedState(), '0:0 @ ANA INSPECT RITE_OF_ASCENSION'))).toEqual(['demands'])
  })
})

// ---------- role operands in recipes work; in triggers they are load errors ----------

describe('IF conditions in recipes read the step bindings (§9.39)', () => {
  const c = inspectContent([
    `IF count(actors) = 2 AND rel(parent_child, actors) -> (NARRATE "kin")
     ELSE IF exists(aspect:lantern >= 1) -> (NARRATE "lantern light")
     ELSE -> (NARRATE "plain")`,
  ])

  test('count(actors), rel() and exists(<query>) bind against the step and the world', () => {
    expect(said(run(c, seedState(), '0:0 @ ANA, BERTHA INSPECT'))).toEqual(['kin'])
    expect(said(run(c, dimWorld(), '0:0 @ ANA INSPECT'))).toEqual(['plain']) // the world form can be false
  })

  test('object.aspect.x reads the bound card; the count(DREAD) typo lint covers IF conditions too', () => {
    const d = inspectContent([`IF object.aspect.demand >= 2 -> (NARRATE "demanding") ELSE -> (NARRATE "modest")`])
    expect(said(run(d, seedState(), '0:0 @ ANA INSPECT RITE_OF_ASCENSION'))).toEqual(['demanding']) // demand:3
    expect(said(run(d, seedState(), '0:0 @ ANA INSPECT CHALICE'))).toEqual(['modest'])
    expect(() => inspectContent([`IF count(DREAD) >= 3 -> (NARRATE "x")`])).toThrow(/count\(\) argument 'DREAD' is neither a role/)
    expect(() => inspectContent([`IF count(name:DREAD) >= 3 -> (NARRATE "x")`])).not.toThrow()
  })
})

describe('role-dependent operands in trigger IF conditions are load errors (§9.39)', () => {
  const triggers = (out: string[]): TriggerDef[] => [{ id: 't', slot: 1, out }]

  test('path, count(role), total(role), rel, every/any — each named by the error', () => {
    expect(() => compileContent({ verbs: {}, triggers: triggers([`IF object.aspect.duty >= 1 -> (NARRATE "x")`]) }, dict))
      .toThrow(/IF operand object\.aspect\.duty reads a step role/)
    expect(() => compileContent({ verbs: {}, triggers: triggers([`IF object.attr.seats >= 1 -> (NARRATE "x")`]) }, dict))
      .toThrow(/IF operand object\.attr\.seats reads a step role/)
    expect(() => compileContent({ verbs: {}, triggers: triggers([`IF count(actors) >= 1 -> (NARRATE "x")`]) }, dict))
      .toThrow(/IF operand count\(actors\) reads a step role/)
    expect(() => compileContent({ verbs: {}, triggers: triggers([`IF total(actors, aspect:nerve) >= 2 -> (NARRATE "x")`]) }, dict))
      .toThrow(/IF operand total\(actors, aspect:nerve\) reads a step role/)
    expect(() => compileContent({ verbs: {}, triggers: triggers([`IF rel(parent_child, actors) -> (NARRATE "x")`]) }, dict))
      .toThrow(/IF operand rel\(parent_child, actors\) reads a step role/)
    expect(() => compileContent({ verbs: {}, triggers: triggers([`IF any(actors, aspect:occult >= 1) -> (NARRATE "x")`]) }, dict))
      .toThrow(/IF operand any\(actors, …\) reads a step role/)
  })

  test('the check recurses into branch bodies, and world forms pass', () => {
    expect(() => compileContent({ verbs: {}, triggers: triggers([`IF exists(X) -> (IF count(actors) >= 1 -> (NARRATE "deep"))`]) }, dict))
      .toThrow(/trigger t: IF operand count\(actors\) reads a step role/)
    // two IF levels down — the walk must not stop at the first branch
    expect(() => compileContent({ verbs: {}, triggers: triggers([`IF exists(X) -> (IF exists(Y) -> (IF count(actors) >= 1 -> (NARRATE "deeper")))`]) }, dict))
      .toThrow(/trigger t: IF operand count\(actors\) reads a step role/)
    expect(() => compileContent({
      verbs: {},
      triggers: triggers([`IF exists(X) -> (NARRATE "ok") ELSE IF count(name:X) >= 3 -> (NARRATE "ok too")`]),
    }, dict)).not.toThrow()
  })

  test('expiry triggers lint their IF conditions too (same empty-bindings argument)', () => {
    const expiry = (out: string[]): TriggerDef => ({ id: 't', onExpireOf: 'CHALICE', out })
    expect(() => compileContent({ verbs: {}, triggers: [expiry([`IF object.aspect.duty >= 1 -> (NARRATE "x")`])] }, dict))
      .toThrow(/trigger t: IF operand object\.aspect\.duty reads a step role/)
    expect(() => compileContent({ verbs: {}, triggers: [expiry([`IF exists(X) -> (IF total(actors, aspect:nerve) >= 1 -> (NARRATE "x"))`])] }, dict))
      .toThrow(/trigger t: IF operand total\(actors, aspect:nerve\) reads a step role/)
    expect(() => compileContent({ verbs: {}, triggers: [expiry([`IF count(name:CHALICE) >= 1 -> (NARRATE "ok")`])] }, dict)).not.toThrow()
  })

  test('the recursive guards catch PROGRESS / REFUSE / unknown ENDING nested in IF branches', () => {
    expect(() => compileContent({ verbs: {}, triggers: triggers([`IF exists(X) -> (PROGRESS(30m, "tick"))`]) }, dict))
      .toThrow(/trigger t: PROGRESS belongs to verb recipes, not events/)
    expect(() => compileContent({ verbs: {}, triggers: triggers([`IF exists(X) -> (REFUSE("no"))`]) }, dict))
      .toThrow(/trigger t: REFUSE belongs to verb recipes — events have no step to refuse/)
    expect(() => compileContent({
      verbs: {},
      triggers: triggers([`IF exists(X) -> (ENDING NOPE)`]),
      endings: { REAL: { title: 'Real' } },
    }, dict)).toThrow(/trigger t: ENDING 'NOPE' is not in the endings table/)
    // deep nesting: two IF levels down
    expect(() => compileContent({ verbs: {}, triggers: triggers([`IF exists(X) -> (IF exists(Y) -> (ENDING NOPE))`]) }, dict))
      .toThrow(/ENDING 'NOPE' is not in the endings table/)
  })
})

// ---------- IF inside working triggers (§9.39, world-form conditions) ----------

describe('slot and expiry triggers evaluate IF against the world (§9.39)', () => {
  const wait = (triggers: TriggerDef[]): CompiledContent => compileContent({
    verbs: {
      WAIT: {
        slots: [{ role: 'actors', min: 1, max: 1, query: 'kind:character' }],
        duration: 60,
        recipes: [{ out: ['NARRATE("{actors} wait.")'] }],
      },
    },
    triggers,
    prototypes: { PARANOIA: { kind: 'state' } },
  }, dict)

  test('a slot trigger takes its IF branch (exists form) and the record flattens it', () => {
    const c = wait([{ id: 'patrol', slot: 2, once: true, out: [`IF exists(HOUSE) -> (SPAWN PARANOIA) ELSE -> (NARRATE "quiet.")`] }])
    const state = seedState()
    const r = run(c, state, '0:0 @ ANA WAIT') // 1h crosses slot 2 (1:00)
    expect(r.rejected).toBeNull()
    // v0.5 order: the WAIT window completes, then the crossed slot's events
    expect(r.records.map(x => x.kind)).toEqual(['action', 'complete', 'event', 'wait'])
    const event = r.records.find(x => x.kind === 'event')!
    expect(event.trigger).toBe('patrol')
    expect(event.outcomes.map(o => o.op)).toEqual(['SPAWN']) // the taken branch, no IF op, no else NARRATE
    expect(r.state.byName.has('PARANOIA')).toBe(true)
    expect(snap(fold(r.records, cloneState(state), dict, c))).toBe(snap(r.state))
  })

  test('an expiry trigger takes its IF branch (count(name:) form)', () => {
    const c = wait([{
      id: 'chalice_dust',
      onExpireOf: 'CHALICE',
      out: [`IF count(name:RITE_OF_ASCENSION) >= 1 -> (DESTROY CHALICE) ELSE -> (NARRATE "lingers")`],
    }])
    const state = seedState()
    state.entities.get('chalice')!.attrs['expiresAt'] = 30 // crosses slot 1 (0:30)
    const r = run(c, state, '0:0 @ ANA WAIT')
    expect(r.rejected).toBeNull()
    const event = r.records.find(x => x.kind === 'event')!
    expect(event.trigger).toBe('chalice_dust')
    expect(event.outcomes.map(o => o.op)).toEqual(['DESTROY'])
    expect(r.state.entities.get('chalice')!.deadAction).not.toBeNull()
    expect(snap(fold(r.records, cloneState(state), dict, c))).toBe(snap(r.state))
  })

  test('a trigger IF whose condition is false takes the ELSE branch', () => {
    const c = wait([{ id: 'patrol', slot: 2, out: [`IF exists(NOSUCH_CARD) -> (SPAWN PARANOIA) ELSE -> (NARRATE "all quiet.")`] }])
    const r = run(c, seedState(), '0:0 @ ANA WAIT')
    const event = r.records.find(x => x.kind === 'event')!
    expect(event.outcomes.map(o => o.op)).toEqual(['NARRATE'])
    expect(narrated(event.outcomes)[0]).toBe('all quiet.')
  })
})

// ---------- pooled worlds compose: the plan's arrest case, end to end ----------

describe('the arrest case — pool count flips the branch inside the firing recipe (§9.39)', () => {
  const arrestScript = `
script: Arrest
player: ANA
start: 0:0

entities:
  HOUSE is place
  ANA is character at HOUSE

prototypes:
  SUSPICION is state stackable

endings:
  ARRESTED: "Taken"

verbs:
  STEW:
    actors: 1 kind:character
    duration: 0m
    recipes:
      default when count(name:SUSPICION) < 3 -> SPAWN SUSPICION, IF count(name:SUSPICION) >= 3 -> (ENDING ARRESTED) ELSE -> (NARRATE "Nobody notices.")
`

  test('selection reads the input state (the when fires at 2); the recipe reads its own SPAWN on the draft', () => {
    const { content, state } = compileScript(arrestScript, dict)
    const base = cloneState(state)
    let cur = state
    let id = 1
    const all: ActionRecord[] = []
    const told: string[][] = []
    for (let i = 0; i < 3; i++) {
      const r = run(content, cur, '0:0 @ ANA STEW', id)
      expect(r.rejected).toBeNull()
      expect(act(r).recipes).toEqual(['stew.default']) // the `when count < 3` recipe keeps firing on input state
      told.push(said(r))
      all.push(...r.records)
      id += r.records.length
      cur = r.state
    }
    expect(told).toEqual([['Nobody notices.'], ['Nobody notices.'], []]) // the third SPAWN flipped its own IF
    expect(cur.byName.get('SUSPICION')).toHaveLength(3)
    expect(cur.ended).toEqual({ id: 'ARRESTED', title: 'Taken', flavour: 'melancholy' })
    // game over afterwards
    const over = run(content, cur, '0:0 @ ANA STEW', id)
    expect(over.rejected).toMatch(/the story has ended — Taken/)
    expect(over.records).toHaveLength(0)
    // and the whole run folds back identically — the flattening is lossless
    expect(snap(fold(all, base, dict, content))).toBe(snap(cur))
  })

  test('a second actor works the same pool — count(name:) is world-scoped (§9.38 x §9.39)', () => {
    const { content, state } = compileScript(arrestScript, dict)
    // a fresh world where the pool is pre-stocked to 2: the very next stew arrests
    const s = newState(0)
    addEntity(s, { id: 's_house', kind: 'place', name: 'HOUSE', aspects: {}, attrs: {}, bornAction: null, deadAction: null, stackable: false })
    addEntity(s, { id: 's_ana', kind: 'character', name: 'ANA', aspects: {}, attrs: { container: 's_house' }, bornAction: null, deadAction: null, stackable: false })
    addEntity(s, { id: 'w1', kind: 'state', name: 'SUSPICION', aspects: {}, attrs: {}, bornAction: null, deadAction: null, stackable: true })
    addEntity(s, { id: 'w2', kind: 'state', name: 'SUSPICION', aspects: {}, attrs: {}, bornAction: null, deadAction: null, stackable: true })
    const r = run(content, s, '0:0 @ ANA STEW')
    expect(r.rejected).toBeNull()
    expect(done(r).outcomes.map(o => o.op)).toEqual(['SPAWN', 'ENDING'])
    expect(r.state.ended?.id).toBe('ARRESTED')
  })
})
