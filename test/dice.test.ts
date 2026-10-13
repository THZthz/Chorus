// F4 — seeded dice: ROLL, the `roll` binding, tiers via IF (spec §9.41, plan
// v0.4 F4). The engine's ONLY randomness is a counter-based pure function:
// die i of a roll at counter c is 1 + floor(sides * splitmix64(seed XOR (c+i))
// / 2^64), computed BigInt end-to-end. A roll of m dice consumes exactly m
// counters; the concrete ROLLED(<total>, <ctrAfter>) row lets the fold land
// the counter exactly where play left it (law 3), so undo reconstructs dice
// state from the folded log. The `roll` operand reads the per-clause binding
// (last-wins, dies with the draft on REFUSE); {roll} in NARRATE prose is
// rewritten AT EXECUTION. Vectors below were computed independently (python3,
// arbitrary-precision integers) against the same algorithm.
import { describe, expect, test } from 'vitest'
import { readdirSync, readFileSync, mkdtempSync, rmSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { compileContent, type CompiledContent, type ContentRaw } from '../src/content.js'
import { rollDie, splitmix64 } from '../src/dice.js'
import { executeStep, type ActionRecord, type ExecResult } from '../src/engine.js'
import { fold, foldUpTo } from '../src/fold.js'
import { formatSentence } from '../src/format.js'
import { Game } from '../src/play.js'
import { parseOutcomes, parseSentence } from '../src/parse.js'
import { compileScript } from '../src/script.js'
import { seedDictionary, seedState } from '../src/seed.js'
import { loadLog, saveLog } from '../src/store.js'
import { cloneState, newState, type State } from '../src/types.js'
import { snap } from './snap.js'
import { settleStep, done, refusal } from './settle.js'

const dict = seedDictionary()
// the rigs' own verb lexemes (verbs are code-registered, §10)
dict.registerVerb('INSPECT')
dict.registerVerb('ATTUNE')

function run(content: CompiledContent, state: State, src: string, firstRecordId = 1): ExecResult {
  const s = parseSentence(src, dict)
  let n = 0
  return settleStep(content, dict, state, { clock: s.clock ?? { day: 0, halfhour: 0 }, clauses: s.clauses }, firstRecordId, () => `x${firstRecordId}_${++n}`)
}

const rolledRow = (rec: ActionRecord): [number, number] | null => {
  const o = rec.outcomes.find(x => x.op === 'ROLLED')
  if (!o) return null
  const a = o.args[0], b = o.args[1]
  return a && a.t === 'int' && b && b.t === 'int' ? [a.v, b.v] : null
}
/** every ROLLED row of a record, in order */
const rolledRows = (rec: ActionRecord): [number, number][] => {
  const rows: [number, number][] = []
  for (const o of rec.outcomes) {
    if (o.op !== 'ROLLED') continue
    const a = o.args[0], b = o.args[1]
    if (a && a.t === 'int' && b && b.t === 'int') rows.push([a.v, b.v])
  }
  return rows
}
const said = (rec: ActionRecord): string[] =>
  rec.outcomes.filter(o => o.op === 'NARRATE').map(o => {
    const a = o.args[0]
    return a && a.t === 'str' ? a.v : '(not a string)'
  })

/** one-verb rig over the seed world: ANA (occult 2, lantern 1, nerve 3 via
 *  extra aspects) INSPECTs; duration 0 keeps the clock out of the picture */
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

// ---------- the RNG itself: pinned vectors (computed in python3) ----------

describe('splitmix64 counter RNG — pinned test vectors (§9.41)', () => {
  // raw finalizer pins: catch ANY drift in constants or shift structure
  test('splitmix64 raw values', () => {
    expect(splitmix64(0n)).toBe(16294208416658607535n)
    expect(splitmix64(1n)).toBe(10451216379200822465n)
    expect(splitmix64(42n)).toBe(13679457532755275413n)
  })

  // the (seed, counter, sides) -> die table, computed independently in python3
  // with arbitrary-precision integers; BigInt constants and the two's-
  // complement XOR (asUintN) must reproduce every row exactly
  const VECTORS: [number, number, number, number][] = [
    // seed, counter, sides, die
    [0, 0, 6, 6], [0, 1, 6, 4], [0, 2, 6, 4], [0, 3, 6, 1], [0, 4, 6, 3],
    [1, 0, 6, 4], [1, 1, 6, 6], [1, 2, 6, 1],
    [42, 0, 6, 5], [42, 1, 6, 5], [42, 2, 6, 2],
    [7, 5, 20, 12],
    [0, 100, 6, 1], [0, 101, 6, 5], [0, 102, 6, 1],
    [9007199254740991, 3, 6, 5], // 2^53-1: exercises the full BigInt path
    [0, 0, 2, 2],
    [123456789, 12, 10, 10],
    [2147483648, 7, 4, 1],
    [65536, 999, 8, 4],
    [0, 999999, 6, 3],
    // negative seed: the key is the seed's 64-bit TWO'S COMPLEMENT (asUintN)
    // XORed with the counter — pinned so the treatment can never drift
    [-1, 0, 6, 6], [-1, 1, 6, 6], [-42, 7, 20, 13],
    [9007199254740991, 0, 6, 1], // 2^53-1 at counter 0
    [0, 1099511627776, 6, 1], // counter 2^40 — the full 64-bit key path
    [42, 1099511627783, 20, 7], // 2^40+7
  ]

  test('the (seed, counter, sides) -> value table', () => {
    for (const [seed, counter, sides, want] of VECTORS) {
      expect(rollDie(seed, counter, sides)).toBe(want)
    }
  })

  test('die values stay in 1..sides and every value of a d2 occurs', () => {
    for (let c = 0; c < 200; c++) {
      const d6 = rollDie(9, c, 6)
      expect(d6).toBeGreaterThanOrEqual(1)
      expect(d6).toBeLessThanOrEqual(6)
    }
    const faces = new Set<number>()
    for (let c = 0; c < 64; c++) faces.add(rollDie(3, c, 2))
    expect([...faces].sort()).toEqual([1, 2])
  })
})

// ---------- determinism: same seed same dice, different seed different dice ----------

const diceScript = (seed: number): string => `
script: Dice Rig
player: ANA
start: 0:0
seed: ${seed}

entities:
  ANA is character

verbs:
  WAIT:
    actors: 1 kind:character
    duration: 0m
    recipes:
      default -> ROLL(3, 6), NARRATE "the bones say {roll}."
`

describe('determinism (§9.41): the dice are a pure function of (seed, counter)', () => {
  test('same seed + script -> identical dice across two fresh runs', () => {
    const a = compileScript(diceScript(0), dict)
    const b = compileScript(diceScript(0), dict)
    const ra = run(a.content, a.state, '0:0 @ ANA WAIT')
    const rb = run(b.content, b.state, '0:0 @ ANA WAIT')
    expect(ra.rejected).toBeNull()
    expect(rb.rejected).toBeNull()
    expect(rolledRow(done(ra)!)).toEqual(rolledRow(done(rb)!))
    expect(rolledRow(done(ra)!)).toEqual([14, 3]) // 6+4+4, counters 0..2
  })

  test('different seed -> different sequence', () => {
    const runs = [0, 1, 42].map(seed => {
      const g = compileScript(diceScript(seed), dict)
      const r = run(g.content, g.state, '0:0 @ ANA WAIT')
      return rolledRow(done(r)!)
    })
    expect(runs[0]).toEqual([14, 3])
    expect(runs[1]).toEqual([11, 3]) // 4+6+1
    expect(runs[2]).toEqual([12, 3]) // 5+5+2
    expect(runs[0]).not.toEqual(runs[1])
    expect(runs[0]).not.toEqual(runs[2])
  })

  test('the meta seed lands in content; default is 0; a bad seed is a load error', () => {
    expect(compileScript(diceScript(42), dict).content.seed).toBe(42)
    expect(compileScript(diceScript(0), dict).content.seed).toBe(0)
    expect(compileContent({ verbs: {} }, dict).seed).toBe(0)
    expect(() => compileScript(diceScript(0).replace('seed: 0', 'seed: six'), dict)).toThrow(/seed must be an integer/)
  })
})

// ---------- the op: args, tiers, bonus, zero dice, rejections ----------

describe('the ROLL op: expression args and tier selection (§9.41)', () => {
  test('parses into vexpr args; arity is pinned; parens required', () => {
    const out = parseOutcomes(`ROLL(total(actors, aspect:nerve), 6, 2), ROLL(count(name:X), object.aspect.dc)`, dict)
    expect(out.map(o => o.op)).toEqual(['ROLL', 'ROLL'])
    expect(out[0]!.args.map(a => a.t)).toEqual(['vexpr', 'vexpr', 'vexpr'])
    expect(() => parseOutcomes(`ROLL(2, 6, 0, 0)`, dict)).toThrow(/ROLL takes/)
    expect(() => parseOutcomes(`ROLL(2)`, dict)).toThrow(/ROLL takes/)
    expect(() => parseOutcomes(`ROLL 2, 6`, dict)).toThrow(/paren form/)
    expect(() => parseOutcomes(`ROLL()`, dict)).toThrow(/ROLL takes/)
    // a comma inside total(...) never splits the args (depth-aware scan)
    expect(out[0]!.args).toHaveLength(3)
  })

  test('tier selection: ROLL(total(actors, aspect:nerve), 6) with IF/ELSE IF/ELSE fires the mid tier', () => {
    // ANA carries nerve:3 in the seed world -> 3 dice at counters 0,1,2 = 6+4+4 = 14
    const c = inspectContent([
      `ROLL(total(actors, aspect:nerve), 6), ` +
      `IF roll >= 15 -> (NARRATE "high") ELSE IF roll >= 8 -> (NARRATE "mid") ELSE -> (NARRATE "low")`,
    ])
    const r = run(c, seedWorld(), '0:0 @ ANA INSPECT')
    expect(r.rejected).toBeNull()
    expect(rolledRow(done(r)!)).toEqual([14, 3])
    expect(said(done(r)!)).toEqual(['mid'])
    expect(r.state.rng).toBe(3)
  })

  test('the same tiers fire LOW from a FORCED counter (fold a ROLLED row first)', () => {
    // force the counter to 100 by folding a recorded roll; counters 100..102 = 1+5+1 = 7
    const c = inspectContent([
      `ROLL(total(actors, aspect:nerve), 6), ` +
      `IF roll >= 15 -> (NARRATE "high") ELSE IF roll >= 8 -> (NARRATE "mid") ELSE -> (NARRATE "low")`,
    ])
    const forced = fold([{
      id: 1, kind: 'complete', trigger: null, tGame: '0:0', tReal: null, startMin: null, window: 'fixture', advancedMin: 0,
      actors: [], verb: '', objects: [], instruments: [], at: null, about: null, to: null,
      outcomes: [{ op: 'ROLLED', args: [{ t: 'int', v: 0 }, { t: 'int', v: 100 }] }],
      recipes: [], sentence: '',
    }], seedWorld(), dict, c)
    expect(forced.rng).toBe(100)
    const r = run(c, forced, '0:0 @ ANA INSPECT', 2)
    expect(r.rejected).toBeNull()
    expect(rolledRow(done(r)!)).toEqual([7, 103])
    expect(said(done(r)!)).toEqual(['low'])
  })

  test('bonus expression adds without consuming counters; count 0 rolls no dice', () => {
    // 5 + 6 + 4 = 15 at counters 0,1 — two consumed
    const bonus = inspectContent([`ROLL(2, 6, 5), NARRATE "got {roll}"`])
    const rb = run(bonus, seedWorld(), '0:0 @ ANA INSPECT')
    expect(rb.rejected).toBeNull()
    expect(rolledRow(done(rb)!)).toEqual([15, 2])
    expect(said(done(rb)!)).toEqual(['got 15'])
    // a computed bonus too: object.aspect.demand = 3 (RITE_OF_ASCENSION)
    const bonusExpr = inspectContent([`ROLL(1, 6, object.aspect.demand), NARRATE "got {roll}"`])
    const re = run(bonusExpr, seedWorld(), '0:0 @ ANA INSPECT RITE_OF_ASCENSION')
    expect(rolledRow(done(re)!)).toEqual([9, 1]) // 6 (die at c0) + 3
    // count 0: total is the bonus, counter unmoved, step still fine
    const zero = inspectContent([`ROLL(0, 6, 2)`])
    const rz = run(zero, seedWorld(), '0:0 @ ANA INSPECT')
    expect(rz.rejected).toBeNull()
    expect(rolledRow(done(rz)!)).toEqual([2, 0])
    expect(rz.state.rng).toBe(0)
  })

  test('count(name:) and total() arg forms evaluate; 0-count is legal', () => {
    // countQuery arg: the seed world holds one RITE_OF_ASCENSION
    const c = inspectContent([`ROLL(count(name:RITE_OF_ASCENSION), 6)`])
    const r = run(c, seedWorld(), '0:0 @ ANA INSPECT')
    expect(rolledRow(done(r)!)).toEqual([6, 1]) // one die, value 6
    // a computed count of 0 rolls no dice: total over a missing aspect reads 0
    const zero = inspectContent([`ROLL(total(actors, aspect:nonesuch), 6), NARRATE "after"`])
    const rz = run(zero, seedWorld(), '0:0 @ ANA INSPECT')
    expect(rz.rejected).toBeNull()
    expect(rolledRow(done(rz)!)).toEqual([0, 0])
    expect(done(rz)!.outcomes.map(o => o.op)).toEqual(['ROLLED', 'NARRATE'])
    expect(rz.state.rng).toBe(0)
  })

  test('an operand that cannot resolve (missing attr over a bound role) refuses the window (§9.19 completion analog)', () => {
    const attr = inspectContent([`ROLL(actors.attr.nonesuch, 6), NARRATE "never"`])
    const ra = run(attr, seedWorld(), '0:0 @ ANA INSPECT')
    // v0.5: the error defers to the completion, where the window refuses —
    // the attempt is logged, the scratch (and its counters) discarded
    expect(ra.rejected).toBeNull()
    expect(refusal(ra)).toMatch(/ROLL: dice count must be an integer >= 0/)
    expect(ra.state.rng).toBe(0)
  })

  test('ROLL arguments get the count(DREAD)-typo lint; ROLL/ROLLED are reserved (§9.14, §9.38)', () => {
    expect(() => inspectContent([`ROLL(count(DREAD), 6)`]))
      .toThrow(/count\(\) argument 'DREAD' is neither a role/)
    expect(() => inspectContent([`ROLL(total(DREAD, aspect:nerve), 6)`]))
      .toThrow(/total\(\) first argument 'DREAD' is not a declared role/)
    expect(() => inspectContent([`ROLL(count(name:DREAD), 6)`])).not.toThrow()
    // reserved words: an entity named ROLL needs quotes; a function cannot take the op's name
    expect(dict.reserved.has('ROLL')).toBe(true)
    expect(dict.reserved.has('ROLLED')).toBe(true)
    expect(() => parseSentence('0:0 @ ANA INSPECT ROLL', dict)).toThrow(/reserved/)
    expect(parseOutcomes(`SPAWN "ROLLED"`, dict)).toHaveLength(1) // the quote is the escape hatch
    expect(() => compileContent({
      functions: [{ name: 'ROLL', params: [], body: 'NARRATE "x"' }],
      verbs: {},
    }, dict)).toThrow(/collides with an outcome op/)
  })

  test('a ROLL inside an UNTAKEN branch consumes nothing (probe)', () => {
    // applyIf only applies the taken arm — the untaken branch's ROLL never
    // runs, so no counters burn and no ROLLED row exists
    const c = inspectContent([`IF exists(NOSUCH) -> (ROLL(2, 6)) ELSE -> (NARRATE "quiet")`])
    const state = seedWorld()
    const before = snap(state)
    const r = run(c, state, '0:0 @ ANA INSPECT')
    expect(r.rejected).toBeNull()
    expect(done(r)!.outcomes.map(o => o.op)).toEqual(['NARRATE'])
    expect(r.state.rng).toBe(0)
    expect(snap(state)).toBe(before)
  })

  test('a failing LATER outcome after a successful ROLL refuses the window (§9.19 completion analog)', () => {
    // the first ROLL succeeds and consumes counters ON THE SCRATCH; the
    // second ROLL's sides < 2 then refuses the window — the attempt is
    // logged, the real counter unmoved (the scratch died with the refusal,
    // exactly like a REFUSE's)
    const c = inspectContent([`ROLL(2, 6), NARRATE "mid {roll}", ROLL(0, 1)`])
    const state = seedWorld()
    const before = snap(state)
    const r = run(c, state, '0:0 @ ANA INSPECT')
    expect(r.rejected).toBeNull()
    expect(refusal(r)).toMatch(/sides must be an integer >= 2/)
    expect(state.rng).toBe(0)
    expect(snap(state)).toBe(before)
    // fold agreement: a REFUSE row is a state no-op, so the counter stays put
    expect(fold(r.records, cloneState(state), dict, c).rng).toBe(0)
  })

  test('sides < 2 and count < 0 refuse the window (§9.19 completion analog: nothing moved)', () => {
    const state = seedWorld()
    const before = snap(state)
    const badSides = inspectContent([`ROLL(1, 1), NARRATE "never"`])
    const rs = run(badSides, state, '0:0 @ ANA INSPECT')
    expect(rs.rejected).toBeNull()
    expect(refusal(rs)).toMatch(/sides must be an integer >= 2/)
    expect(state.rng).toBe(0)
    expect(snap(state)).toBe(before)
    // negative count via a negative aspect (entity lines allow -n)
    const neg = compileContent({
      verbs: {
        INSPECT: {
          slots: [{ role: 'actors', min: 1, max: 1, query: 'kind:character' }],
          duration: 0,
          recipes: [{ out: [`ROLL(actors.aspect.dread, 6), NARRATE "never"`] }],
        },
      },
    }, dict)
    const s2 = seedWorld()
    s2.entities.get('ana')!.aspects['dread'] = -3
    const before2 = snap(s2)
    const rn = run(neg, s2, '0:0 @ ANA INSPECT')
    expect(rn.rejected).toBeNull()
    expect(refusal(rn)).toMatch(/dice count must be an integer >= 0/)
    expect(snap(s2)).toBe(before2)
    // and a fractional/missing operand is likewise a rejection, never a silent roll
    const missing = inspectContent([`ROLL(object.aspect.nonesuch, 6)`])
    const rm = run(missing, seedWorld(), '0:0 @ ANA INSPECT') // no object bound
    expect(refusal(rm)).toMatch(/cannot resolve/)
  })
})

// ---------- the `roll` binding: last-wins, per-clause, runtime rejection ----------

describe('the roll binding (§9.41): last-wins, per-clause, unbound = rejection', () => {
  test('last-wins: a second ROLL rebinds; both consumed counters show in the rows', () => {
    const c = inspectContent([`ROLL(1, 6), ROLL(1, 6), NARRATE "final {roll}"`])
    const r = run(c, seedWorld(), '0:0 @ ANA INSPECT')
    expect(r.rejected).toBeNull()
    expect(rolledRows(done(r)!)).toEqual([[6, 1], [4, 2]])
    expect(said(done(r)!)).toEqual(['final 4']) // die(0,1,6) = 4, not the first roll's 6
    expect(r.state.rng).toBe(2)
  })

  test('a roll does not cross clauses: clause two\u2019s read is a load error (per-clause binding)', () => {
    // clause one rolls; clause two's list provably has no ROLL of its own —
    // with the per-clause reset that read is dead on arrival, and the may-bind
    // lint proves it: the recipe cannot even compile
    expect(() => compileContent({
      verbs: {
        INSPECT: {
          slots: [{ role: 'actors', min: 1, max: 1, query: 'kind:character' }],
          duration: 0,
          recipes: [{ out: [`ROLL(1, 6), NARRATE "clause one {roll}"`] }],
        },
        ATTUNE: {
          slots: [{ role: 'actors', min: 1, max: 1, query: 'kind:character' }],
          duration: 0,
          recipes: [{ out: [`IF roll >= 0 -> (NARRATE "clause two {roll}")`] }],
        },
      },
    }, dict)).toThrow(/recipe attune\.default: 'roll' is read but no ROLL can have run before it/)
  })

  test('each clause binds its own dice; the counter chain continues across clauses', () => {
    const c = compileContent({
      verbs: {
        INSPECT: {
          slots: [{ role: 'actors', min: 1, max: 1, query: 'kind:character' }],
          duration: 0,
          recipes: [{ out: [`ROLL(1, 6), NARRATE "clause one {roll}"`] }],
        },
        ATTUNE: {
          slots: [{ role: 'actors', min: 1, max: 1, query: 'kind:character' }],
          duration: 0,
          recipes: [{ out: [`ROLL(1, 6), NARRATE "clause two {roll}"`] }],
        },
      },
    }, dict)
    const r = run(c, seedWorld(), '0:0 @ ANA INSPECT; ANA ATTUNE')
    expect(r.rejected).toBeNull()
    // clause one rolls die(0,0,6)=6; clause two rolls die(0,1,6)=4 at the
    // CONTINUED counter — shared counter, independent bindings; each clause's
    // window closes with its own COMPLETE row
    const completes = r.records.filter(x => x.kind === 'complete')
    expect(completes.flatMap(x => said(x))).toEqual(['clause one 6', 'clause two 4'])
    expect(completes.flatMap(x => rolledRows(x))).toEqual([[6, 1], [4, 2]])
  })

  test('unbound roll in a TAKEN branch refuses the window at runtime (the lint\u2019s false-negative path)', () => {
    // branch one may bind (so the lint passes); the ELSE path never rolls
    const c = inspectContent([
      `IF exists(HOUSE) -> (ROLL(1, 6)) ELSE -> (NARRATE "no house"), IF roll >= 0 -> (NARRATE "read {roll}")`,
    ])
    const withHouse = run(c, seedWorld(), '0:0 @ ANA INSPECT')
    expect(withHouse.rejected).toBeNull()
    expect(said(done(withHouse)!)).toEqual(['read 6']) // rolled, so `roll` reads
    const withoutHouse = inspectContent([`IF exists(NOSUCH) -> (ROLL(1, 6)) ELSE -> (NARRATE "skip"), IF roll >= 0 -> (NARRATE "read {roll}")`])
    const r = run(withoutHouse, seedWorld(), '0:0 @ ANA INSPECT')
    expect(r.rejected).toBeNull()
    expect(refusal(r)).toMatch(/'roll' is unbound/)
  })

  test('`roll` as a ROLL ARGUMENT with no prior ROLL refuses the window at runtime (lint false-negative)', () => {
    // the may-bind lint scans IF conditions only — a ROLL argument reading
    // `roll` loads fine and is caught by the runtime refusal (false
    // negatives are allowed; the refusal is the backstop)
    const c = inspectContent([`ROLL(roll, 6)`])
    const r = run(c, seedWorld(), '0:0 @ ANA INSPECT')
    expect(r.rejected).toBeNull()
    expect(refusal(r)).toMatch(/'roll' is unbound/)
  })

  test('a NARRATE in a LATER clause keeps {roll} literal (the binding resets per clause)', () => {
    // clause one rolls and its NARRATE rewrites; clause two's NARRATE runs
    // after the per-clause reset with no ROLL of its own — the token stays
    // literal text in the journal (documented §9.41 ruling)
    const c = compileContent({
      verbs: {
        INSPECT: {
          slots: [{ role: 'actors', min: 1, max: 1, query: 'kind:character' }],
          duration: 0,
          recipes: [{ out: [`ROLL(1, 6), NARRATE "clause one {roll}"`] }],
        },
        ATTUNE: {
          slots: [{ role: 'actors', min: 1, max: 1, query: 'kind:character' }],
          duration: 0,
          recipes: [{ out: [`NARRATE "clause two {roll}"`] }],
        },
      },
    }, dict)
    const r = run(c, seedWorld(), '0:0 @ ANA INSPECT; ANA ATTUNE')
    expect(r.rejected).toBeNull()
    const completes = r.records.filter(x => x.kind === 'complete')
    expect(completes.flatMap(x => said(x))).toEqual(['clause one 6', 'clause two {roll}'])
  })

  test('`when:` predicates cannot read roll (load error); IF conditions can', () => {
    // a recipe IF with no possible prior ROLL: the may-bind error
    expect(() => inspectContent([`IF roll >= 3 -> (NARRATE "x")`]))
      .toThrow(/recipe inspect\.default: 'roll' is read but no ROLL can have run before it/)
    // the same operand in a when: line: the string front-end refuses it outright
    expect(() => compileContent({
      verbs: {
        INSPECT: {
          slots: [{ role: 'actors', min: 1, max: 1, query: 'kind:character' }],
          duration: 0,
          recipes: [{ id: 'inspect.gate', when: 'roll >= 3', out: ['NARRATE("gate")'] }, { out: ['NARRATE("default")'] }],
        },
      },
    }, dict)).toThrow(/'roll' is not available in 'when:' predicates/)
  })
})

// ---------- compile-time may-bind lint ----------

describe('compile lint: roll without a provable prior ROLL is a load error (§9.41)', () => {
  test('provable cases name the recipe; a preceding ROLL clears them', () => {
    expect(() => inspectContent([`IF roll < 3 -> (NARRATE "low")`]))
      .toThrow(/recipe inspect\.default: 'roll' is read but no ROLL can have run before it/)
    expect(() => inspectContent([`ROLL(1, 6), IF roll < 3 -> (NARRATE "low") ELSE -> (NARRATE "high")`])).not.toThrow()
    // nested in a branch with no ROLL before it, inside the branch
    expect(() => inspectContent([`IF exists(HOUSE) -> (IF roll < 3 -> (NARRATE "deep"))`])).toThrow(/no ROLL can have run before it/)
    // a ROLL before the nested IF clears the inner reference
    expect(() => inspectContent([`ROLL(1, 6), IF exists(HOUSE) -> (IF roll < 3 -> (NARRATE "deep"))`])).not.toThrow()
  })

  test('no false positive for a branch-hidden prior ROLL', () => {
    // the first IF's branch MIGHT roll — the lint cannot prove otherwise
    expect(() => inspectContent([
      `IF exists(HOUSE) -> (ROLL(1, 6)) ELSE -> (NARRATE "skip"), IF roll >= 3 -> (NARRATE "big") ELSE -> (NARRATE "small")`,
    ])).not.toThrow()
  })
})

// ---------- {roll} prose interpolation ----------

describe('{roll} interpolation (§9.41): rewritten at execution, load-fixed elsewhere', () => {
  test('the concrete journal row carries the number', () => {
    const c = inspectContent([`ROLL(3, 6), NARRATE "the bones say {roll}."`])
    const r = run(c, seedWorld(), '0:0 @ ANA INSPECT')
    expect(r.rejected).toBeNull()
    expect(said(done(r)!)).toEqual(['the bones say 14.'])
    // the recorded SENTENCE carries the number — the fold never rewrites prose
    expect(done(r)!.sentence).toContain('NARRATE "the bones say 14."')
    expect(formatSentence(parseSentence(done(r)!.sentence, dict), dict)).toBe(done(r)!.sentence)
  })

  test('a NARRATE before the ROLL keeps the literal token; after a second ROLL it rebinds', () => {
    const c = inspectContent([`NARRATE "early {roll}", ROLL(1, 6), NARRATE "late {roll}"`])
    const r = run(c, seedWorld(), '0:0 @ ANA INSPECT')
    expect(said(done(r)!)).toEqual(['early {roll}', 'late 6'])
  })

  test('{roll} in PROGRESS or REFUSE strings is a load error', () => {
    expect(() => inspectContent([`ROLL(1, 6), PROGRESS(30m, "ticking {roll}")`]))
      .toThrow(/recipe inspect\.default: \{roll\} is only available in NARRATE strings — PROGRESS prose is load-fixed/)
    expect(() => inspectContent([`ROLL(1, 6), REFUSE("the dice said {roll}")`]))
      .toThrow(/recipe inspect\.default: \{roll\} is only available in NARRATE strings — REFUSE prose is load-fixed/)
    expect(() => inspectContent([`ROLL(1, 6), NARRATE "ok {roll}"`])).not.toThrow()
    // a function body's PROGRESS {roll} is caught too: expansion runs before
    // the lint, so the RECIPE's compile names the offender
    expect(() => inspectContent([`helper()`], { functions: [{ name: 'helper', params: [], body: `PROGRESS(30m, "ticking {roll}")` }] }))
      .toThrow(/recipe inspect\.default: \{roll\} is only available in NARRATE strings/)
  })
})

// ---------- ROLLED: round-trip, authoring rejection, fold replay ----------

describe('ROLLED (§9.41): engine-written, parse+format round-trip, fold sets rng', () => {
  test('the concrete row round-trips through parse and format', () => {
    const c = inspectContent([`ROLL(3, 6)`])
    const r = run(c, seedWorld(), '0:0 @ ANA INSPECT')
    const row = done(r)!.sentence
    expect(row).toContain('ROLLED(14, 3)')
    expect(formatSentence(parseSentence(row, dict), dict)).toBe(row)
    // and the parse entry pins the arity/shape
    expect(() => parseOutcomes(`ROLLED(1)`, dict)).toThrow(/ROLLED takes exactly two integers/)
    expect(() => parseOutcomes(`ROLLED(1, 2, 3)`, dict)).toThrow(/ROLLED takes exactly two integers/)
    // non-int args cannot even get through the generic arg grammar
    expect(() => parseOutcomes(`ROLLED(total(actors, aspect:nerve), 3)`, dict)).toThrow()
  })

  test('authoring ROLLED in content is a load error (engine-written only)', () => {
    expect(() => inspectContent([`ROLLED(14, 3)`])).toThrow(/ROLLED is engine-written/)
    expect(() => inspectContent([`IF exists(HOUSE) -> (ROLLED(1, 2))`])).toThrow(/ROLLED is engine-written/)
    expect(() => compileContent({ verbs: {}, triggers: [{ id: 't', slot: 1, out: [`ROLLED(1, 2)`] }] }, dict))
      .toThrow(/trigger t: ROLLED is engine-written/)
    // and inside a FUNCTION body: expansion runs before the lint, so the call
    // site's compile names the recipe, not the function
    expect(() => inspectContent([`helper()`], { functions: [{ name: 'helper', params: [], body: `ROLLED(1, 2)` }] }))
      .toThrow(/recipe inspect\.default: ROLLED is engine-written/)
  })

  test('fold of a bare ROLLED row moves ONLY the counter (snap probe)', () => {
    // hand-feeding one ROLLED row over a fresh world must equal the same
    // world with rng set by hand — no other field may move
    const c = inspectContent([`NARRATE "x"`])
    const base = seedWorld()
    const forced = fold([{
      id: 1, kind: 'complete', trigger: null, tGame: '0:0', tReal: null, startMin: null, window: 'fixture', advancedMin: 0,
      actors: [], verb: '', objects: [], instruments: [], at: null, about: null, to: null,
      outcomes: [{ op: 'ROLLED', args: [{ t: 'int', v: 11 }, { t: 'int', v: 55 }] }],
      recipes: [], sentence: '',
    }], cloneState(base), dict, c)
    const expected = cloneState(base)
    expected.rng = 55
    expect(snap(forced)).toBe(snap(expected))
  })

  test('fold sets state.rng from the row — and NOTHING else (no die re-derived)', () => {
    const c = inspectContent([`ROLL(3, 6)`])
    const r = run(c, seedWorld(), '0:0 @ ANA INSPECT')
    const replayed = fold(r.records, seedWorld(), dict, c)
    expect(replayed.rng).toBe(3)
    expect(snap(replayed)).toBe(snap(r.state))
    // a hand-written ROLLED row lands the counter at ctrAfter, whatever it says
    const forced = fold([{
      id: 1, kind: 'complete', trigger: null, tGame: '0:0', tReal: null, startMin: null, window: 'fixture', advancedMin: 0,
      actors: [], verb: '', objects: [], instruments: [], at: null, about: null, to: null,
      outcomes: [{ op: 'ROLLED', args: [{ t: 'int', v: 11 }, { t: 'int', v: 55 }] }],
      recipes: [], sentence: '',
    }], seedWorld(), dict, c)
    expect(forced.rng).toBe(55)
  })
})

// ---------- REFUSE after ROLL: the draft dies, the counter never moves ----------

describe('REFUSE after ROLL (§9.41): play and fold agree the counter is unmoved', () => {
  const c = inspectContent([`ROLL(2, 6), NARRATE "considering {roll}...", REFUSE("not yet")`], undefined)

  test('zero time, attempt logged with its ROLLED row, counter unmoved in play AND fold', () => {
    const state = seedWorld()
    const before = snap(state)
    const r = run(c, state, '0:0 @ ANA INSPECT')
    expect(r.rejected).toBeNull()
    expect(r.records).toHaveLength(1)
    expect(done(r)!.outcomes.map(o => o.op)).toEqual(['ROLLED', 'NARRATE', 'REFUSE'])
    expect(rolledRow(done(r)!)).toEqual([10, 2]) // the dice WERE thrown (6+4)
    expect(said(done(r)!)).toEqual(['considering 10...']) // journal-visible
    expect(done(r)!.advancedMin).toBe(0)
    expect(r.state).toBe(state) // the draft died with the refusal
    expect(r.state.rng).toBe(0) // so the counter never moved in play
    expect(snap(state)).toBe(before)
    // and the fold treats the refused record as a no-op: counter unmoved there too
    const replayed = fold(r.records, cloneState(state), dict, c)
    expect(replayed.rng).toBe(0)
    expect(snap(replayed)).toBe(before)
  })
})

// ---------- undo reconstruction (THE acceptance test) ----------

describe('undo reconstruction (§9.41): the counter rebuilds from the folded log', () => {
  const undoScript = `
script: Undo Rig
player: ANA
start: 0:0
seed: 0

entities:
  ANA is character

verbs:
  WAIT:
    actors: 1 kind:character
    duration: 0m
    recipes:
      default -> ROLL(3, 6), NARRATE "waited on {roll}."
  DREAM:
    actors: 1 kind:character
    duration: 0m
    recipes:
      default -> ROLL(1, 6), NARRATE "dreamed of {roll}."
`

  test('play -> undo past the roll -> different action with a roll -> save -> fold(loadLog) === state', () => {
    const game = Game.fromScript(undoScript, dict)
    // step 1: WAIT rolls 3 dice (counters 0..2, total 14) — this is step k.
    // v0.5: the roll lands when the window completes, so settle it
    const t1 = game.handleLine('wait')
    expect(t1.kind).toBe('action')
    game.wait()
    expect(game.state.rng).toBe(3)
    // undo past k: the reconstructed state's counter is folded, not remembered.
    // Two undos — the settle turn and the begin turn — leave no stale window
    game.undo()
    game.undo()
    expect(game.state.rng).toBe(0)
    expect(game.log.records).toHaveLength(0)
    // a DIFFERENT action with a roll: DREAM rolls 1 die — at counter 0 again,
    // i.e. the reconstructed counter, not the discarded history's
    const t2 = game.handleLine('dream')
    expect(t2.kind).toBe('action')
    game.wait()
    expect(game.state.rng).toBe(1)
    // the die equals the vector value at (seed 0, counter 0, d6) — the same a
    // fresh run's FIRST roll would produce: reconstruction, not drift
    expect(rollDie(0, 0, 6)).toBe(6)
    expect(game.journal().join('\n')).toContain('ROLLED(6, 1)')
    // save the whole log and fold it back: byte-identical state, rng included
    const dir = mkdtempSync(join(tmpdir(), 'chorus-dice-'))
    const path = join(dir, 'log.jsonl')
    try {
      saveLog(path, game.log.records)
      const replayed = fold(loadLog(path), cloneState(game.log.initialState), dict, game.content)
      expect(snap(replayed)).toBe(snap(game.state))
    } finally {
      rmSync(dir, { recursive: true, force: true })
    }
  })

  test('foldUpTo rebuilds the counter mid-log without the tail', () => {
    const { content, state } = compileScript(undoScript, dict)
    const base = cloneState(state)
    const r1 = run(content, state, '0:0 @ ANA WAIT', 1) // rng 0 -> 3
    const r2 = run(content, r1.state, '0:0 @ ANA DREAM', 1 + r1.records.length) // rng 3 -> 4
    expect(r2.state.rng).toBe(4)
    const mid = foldUpTo([...r1.records, ...r2.records], base, dict, content, 1 + r1.records.length)
    expect(mid.rng).toBe(3) // exactly where the folded history left it
    const all = foldUpTo([...r1.records, ...r2.records], base, dict, content, 99)
    expect(all.rng).toBe(4)
  })

  test('undo to MID-log, keep playing: the counter resumes from the fold, the old ROLLED row is gone', () => {
    const game = Game.fromScript(undoScript, dict)
    game.handleLine('wait') // rng 0 -> 3
    game.wait()
    game.handleLine('dream') // rng 3 -> 4
    game.wait()
    expect(game.journal().join('\n')).toContain('ROLLED(1, 4)')
    // drop the DREAM turn (its settle + its begin) — the ROLLED row goes WITH
    // it (no re-roll anywhere), and no stale window is left open
    game.undo()
    game.undo()
    expect(game.state.rng).toBe(3)
    expect(game.journal().join('\n')).not.toContain('DREAM')
    expect(game.journal().join('\n')).not.toContain('ROLLED(1, 4)')
    // a THIRD roll continues from the FOLDED counter: counters 3..5 = 1+5+1 = 7
    game.handleLine('wait')
    game.wait()
    expect(game.state.rng).toBe(6)
    expect(game.journal().join('\n')).toContain('ROLLED(7, 6)')
    // the undone-and-continued path is indistinguishable from a straight-line
    // run of the same surviving history — state AND journal
    const straight = Game.fromScript(undoScript, dict)
    straight.handleLine('wait')
    straight.wait()
    straight.handleLine('wait')
    straight.wait()
    expect(snap(straight.state)).toBe(snap(game.state))
    expect(straight.journal()).toEqual(game.journal())
    // the save/fold law still holds on the post-undo log
    const dir = mkdtempSync(join(tmpdir(), 'chorus-dice-undo-mid-'))
    const path = join(dir, 'log.jsonl')
    try {
      saveLog(path, game.log.records)
      const replayed = fold(loadLog(path), cloneState(game.log.initialState), dict, game.content)
      expect(snap(replayed)).toBe(snap(game.state))
    } finally {
      rmSync(dir, { recursive: true, force: true })
    }
  })
})

// ---------- triggers roll ----------

describe('triggers roll (§9.41): slot and expiry events may ROLL and tier on roll', () => {
  const waitContent = (triggers: ContentRaw['triggers']): CompiledContent => compileContent({
    verbs: {
      WAIT: {
        slots: [{ role: 'actors', min: 1, max: 1, query: 'kind:character' }],
        duration: 60,
        recipes: [{ out: ['NARRATE("{actors} wait.")'] }],
      },
    },
    triggers,
    prototypes: { OMEN: { kind: 'idea' }, DUST: { kind: 'state' } },
  }, dict)

  test('a slot trigger rolls, tiers on roll, and the EVENT row carries the ROLLED result', () => {
    // 2 dice at counters 0,1 = 6+4 = 10 -> the high tier
    const c = waitContent([{ id: 'patrol', slot: 2, once: true, out: [`ROLL(2, 6), IF roll >= 8 -> (SPAWN OMEN, NARRATE "an omen stirs ({roll}).") ELSE -> (NARRATE "all quiet.")`] }])
    const state = seedWorld()
    const r = run(c, state, '0:0 @ ANA WAIT') // 1h crosses slot 2
    expect(r.rejected).toBeNull()
    const event = r.records.find(x => x.kind === 'event')!
    expect(event.trigger).toBe('patrol')
    expect(event.outcomes.map(o => o.op)).toEqual(['ROLLED', 'SPAWN', 'NARRATE'])
    expect(rolledRow(event)).toEqual([10, 2])
    expect(said(event)).toEqual(['an omen stirs (10).'])
    expect(r.state.rng).toBe(2)
    // the fold replays the EVENT row's ROLLED exactly
    expect(snap(fold(r.records, cloneState(state), dict, c))).toBe(snap(r.state))
  })

  test('an expiry trigger rolls too; the card goes with its authored outcomes', () => {
    const c = waitContent([{
      id: 'chalice_dust',
      onExpireOf: 'CHALICE',
      out: [`ROLL(1, 6), IF roll >= 4 -> (DESTROY CHALICE, SPAWN DUST, NARRATE "it crumbles ({roll}).") ELSE -> (NARRATE "it holds ({roll}).")`],
    }])
    const state = seedWorld()
    state.entities.get('chalice')!.attrs['expiresAt'] = 30 // crosses slot 1
    const r = run(c, state, '0:0 @ ANA WAIT')
    expect(r.rejected).toBeNull()
    const event = r.records.find(x => x.kind === 'event')!
    expect(event.trigger).toBe('chalice_dust')
    expect(event.outcomes.map(o => o.op)).toEqual(['ROLLED', 'DESTROY', 'SPAWN', 'NARRATE'])
    expect(rolledRow(event)).toEqual([6, 1]) // die(0,0,6) = 6 -> the crumble tier
    expect(said(event)).toEqual(['it crumbles (6).'])
    expect(snap(fold(r.records, cloneState(state), dict, c))).toBe(snap(r.state))
  })

  test('trigger ROLL arguments must be role-free (load error, naming the argument)', () => {
    expect(() => waitContent([{ id: 't', slot: 1, out: [`ROLL(count(actors), 6)`] }]))
      .toThrow(/trigger t: ROLL argument count\(actors\) reads a step role/)
    expect(() => waitContent([{ id: 't', slot: 1, out: [`ROLL(total(actors, aspect:nerve), 6)`] }]))
      .toThrow(/trigger t: ROLL argument total\(actors, aspect:nerve\) reads a step role/)
    expect(() => waitContent([{ id: 't', onExpireOf: 'CHALICE', out: [`ROLL(object.attr.seats, 6)`] }]))
      .toThrow(/trigger t: ROLL argument object\.attr\.seats reads a step role/)
    // world forms are the trigger vocabulary
    expect(() => waitContent([{ id: 't', slot: 1, out: [`ROLL(count(name:HOUSE), 6)`] }])).not.toThrow()
  })

  test('a trigger reading roll with no possible ROLL is a load error (may-bind lint)', () => {
    expect(() => waitContent([{ id: 't', slot: 1, out: [`IF roll >= 4 -> (NARRATE "x")`] }]))
      .toThrow(/trigger t: 'roll' is read but no ROLL can have run before it/)
  })

  test('a failing LATER event after a trigger ROLL rejects the WHOLE step (§9.19, play = fold)', () => {
    // the first trigger rolls (counters consumed ON THE DRAFT); the second
    // trigger's sides < 2 then throws mid-events — the whole step rejects:
    // nothing logged (not even the action row), the real counter unmoved,
    // and the fold (which would see no records at all) agrees trivially
    const c = waitContent([
      { id: 'roller', slot: 2, once: true, out: [`ROLL(2, 6), NARRATE "rolled."`] },
      { id: 'breaker', slot: 2, once: true, out: [`ROLL(1, 1)`] },
    ])
    const state = seedWorld()
    const before = snap(state)
    const r = run(c, state, '0:0 @ ANA WAIT')
    expect(r.rejected).toMatch(/sides must be an integer >= 2/)
    expect(r.records).toHaveLength(0)
    expect(state.rng).toBe(0)
    expect(snap(state)).toBe(before)
    expect(fold([], cloneState(state), dict, c).rng).toBe(0)
  })

  test('trigger rolls replay through saved JSONL rows', () => {
    const c = waitContent([{ id: 'patrol', slot: 2, once: true, out: [`ROLL(2, 6), IF roll >= 8 -> (SPAWN OMEN) ELSE -> (NARRATE "quiet.")`] }])
    const state = seedWorld()
    const r = run(c, state, '0:0 @ ANA WAIT', 7)
    const dir = mkdtempSync(join(tmpdir(), 'chorus-dice-trig-'))
    const path = join(dir, 'log.jsonl')
    try {
      saveLog(path, r.records)
      const replayed = fold(loadLog(path), cloneState(state), dict, c)
      expect(snap(replayed)).toBe(snap(r.state))
      expect(replayed.rng).toBe(2)
    } finally {
      rmSync(dir, { recursive: true, force: true })
    }
  })
})

// ---------- functions: ROLL inside bodies, called from recipes and triggers ----------

describe('functions with dice (§9.41 x §9.40): ROLL and roll-tiers inside bodies', () => {
  const fnScript = (extra: string): string => `
script: Fn Dice Rig
player: ANA
start: 0:0
seed: 0

entities:
  ANA is character
  HOUSE is place

prototypes:
  OMEN is idea

triggers:
  patrol at hour 1 -> check(8)

functions:
  check(dc) -> ROLL(2, 6), IF roll >= dc -> (SPAWN OMEN, NARRATE "passed {roll} vs {dc}.") ELSE -> (NARRATE "failed {roll} vs {dc}.")
  pile(n) -> ROLL(n, 6)

verbs:
  WAIT:
    actors: 1 kind:character
    duration: ${extra.includes('cross') ? '1h' : '0m'}
    recipes:
      default -> check(8)${extra.replace(' cross', '')}
`

  test('a body ROLL with roll-tiers expands into a recipe and plays', () => {
    const { content, state } = compileScript(fnScript(''), dict)
    const r = run(content, state, '0:0 @ ANA WAIT')
    expect(r.rejected).toBeNull()
    // 2 dice at counters 0,1 = 10 >= 8 -> the pass tier, param interpolated
    expect(rolledRow(done(r)!)).toEqual([10, 2])
    expect(said(done(r)!)).toEqual(['passed 10 vs 8.'])
    expect(snap(fold(r.records, cloneState(state), dict, content))).toBe(snap(r.state))
  })

  test('a param as the ROLL count expands from the call site', () => {
    const { content, state } = compileScript(fnScript(', pile(3)'), dict)
    const r = run(content, state, '0:0 @ ANA WAIT')
    expect(r.rejected).toBeNull()
    const rows = rolledRows(done(r)!)
    expect(rows).toEqual([[10, 2], [8, 5]]) // second call: 3 dice at counters 2..4 = 4+1+3 = 8
  })

  test('the same function called from a TRIGGER rolls and replays through the EVENT row', () => {
    const { content, state } = compileScript(fnScript(' cross'), dict) // WAIT lasts 1h, crossing hour 1 (slot 2)
    const r = run(content, state, '0:0 @ ANA WAIT')
    expect(r.rejected).toBeNull()
    // the recipe's own check(8) consumed counters 0,1 (10, passed); the event's
    // call continues at counters 2,3 = 4+1 = 5 < 8 -> the fail tier
    const event = r.records.find(x => x.kind === 'event')!
    expect(event.trigger).toBe('patrol')
    expect(rolledRow(event)).toEqual([5, 4])
    expect(said(event)).toEqual(['failed 5 vs 8.'])
    expect(snap(fold(r.records, cloneState(state), dict, content))).toBe(snap(r.state))
  })
})

// ---------- seed drift + the grep gate ----------

describe('seed drift and hygiene (§9.30, §9.41)', () => {
  test('a different seed shifts future dice but never past rows (fold is row-driven)', () => {
    const row = (seed: number): [number, number] => {
      const g = compileScript(diceScript(seed), dict)
      const r = run(g.content, g.state, '0:0 @ ANA WAIT')
      return rolledRow(done(r)!)!
    }
    expect(row(0)).not.toEqual(row(42))
    // the recorded row is what the fold replays: hand-feeding the SAME row to
    // differently-seeded content lands the identical counter
    const c0 = compileScript(diceScript(0), dict).content
    const c42 = compileScript(diceScript(42), dict).content
    const record = {
      id: 1, kind: 'complete', trigger: null, tGame: '0:0', tReal: null, startMin: null, window: 'fixture', advancedMin: 0,
      actors: [], verb: '', objects: [], instruments: [], at: null, about: null, to: null,
      outcomes: [{ op: 'ROLLED', args: [{ t: 'int', v: 14 }, { t: 'int', v: 3 }] }],
      recipes: [], sentence: '',
    } as ActionRecord
    expect(fold([record], newState(), dict, c0).rng).toBe(3)
    expect(fold([record], newState(), dict, c42).rng).toBe(3)
  })

  test('two fresh same-seed runs write byte-identical JSONL logs; a different seed diverges', () => {
    const play = (seed: number): string => {
      const game = Game.fromScript(diceScript(seed), dict)
      expect(game.handleLine('wait').kind).toBe('action')
      game.wait() // v0.5: the roll lands on the completion — the row must be in the log
      const dir = mkdtempSync(join(tmpdir(), 'chorus-dice-det-'))
      const path = join(dir, 'log.jsonl')
      try {
        saveLog(path, game.log.records)
        return readFileSync(path, 'utf8')
      } finally {
        rmSync(dir, { recursive: true, force: true })
      }
    }
    expect(play(7)).toBe(play(7))
    expect(play(7)).not.toBe(play(8))
  })

  test('grep gate: no Math.random anywhere in src — the counter RNG is the only randomness', () => {
    const srcDir = new URL('../src', import.meta.url).pathname
    const offenders: string[] = []
    for (const f of readdirSync(srcDir)) {
      if (!f.endsWith('.ts') && !f.endsWith('.tsx')) continue
      if (readFileSync(join(srcDir, f), 'utf8').includes('Math.random')) offenders.push(f)
    }
    expect(offenders).toEqual([])
  })
})

/** the standard seed world plus a `nerve:3` aspect on ANA — the total() cases
 *  roll 3 dice at counters 0..2 = 6+4+4 = 14 */
function seedWorld(): State {
  const s = seedState()
  s.entities.get('ana')!.aspects['nerve'] = 3
  return s
}
