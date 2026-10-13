// F3 — functions: parametric outcome macros, compile-time expanded (spec
// §9.40, plan v0.4 F3). A `functions:` declaration expands at LOAD: every
// call site becomes its function's body with parameters substituted (AST
// splicing + string interpolation), BEFORE the trigger guards and the
// endings check run — canonical equals expansion, so the engine and the log
// never see a call. Recursion cycles, depth > 8, arity mismatches, op-name
// collisions and undeclared `{token}` interpolation are load errors.
import { describe, expect, test } from 'vitest'
import { compileContent, type CompiledContent, type ContentRaw, type FunctionDef } from '../src/content.js'
import { executeStep, type ActionRecord, type ExecResult } from '../src/engine.js'
import { fold } from '../src/fold.js'
import { formatSentence } from '../src/format.js'
import { parseFunctionBody, parseOutcomes, parseSentence, type FnTable, type FnTemplate } from '../src/parse.js'
import type { Outcome } from '../src/sentence.js'
import { compileScript } from '../src/script.js'
import { seedDictionary } from '../src/seed.js'
import { loadLog, saveLog } from '../src/store.js'
import { addEntity, cloneState, newState, type State } from '../src/types.js'
import { snap } from './snap.js'
import { settleStep, act , done, refusal } from './settle.js'
import { mkdtempSync, rmSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'

const dict = seedDictionary()
dict.registerVerb('STEW') // the rig's own verb lexeme (verbs are code-registered, §10)

function run(content: CompiledContent, state: State, src: string, firstRecordId = 1): ExecResult {
  const s = parseSentence(src, dict)
  let n = 0
  return settleStep(content, dict, state, { clock: s.clock ?? { day: 0, halfhour: 0 }, clauses: s.clauses }, firstRecordId, () => `x${firstRecordId}_${++n}`)
}

// v0.5: NARRATE defers to the completion — gather across the COMPLETE rows
const said = (r: ExecResult): string[] =>
  r.records.filter(x => x.kind === 'complete').flatMap((x) => {
    const rec: ActionRecord = x
    return rec.outcomes.filter(o => o.op === 'NARRATE').map((o) => {
      const a = o.args[0]
      return a && a.t === 'str' ? a.v : '(not a string)'
    })
  })

// ---------- rig: the plan's flagship pair, verbatim grammar ----------

const CALL_FUNCTIONS = `
functions:
  sus capital -> SPAWN SUSPICION, NARRATE "The conductor's eyes linger on {capital}."
  watch(n) -> IF count(name:SUSPICION) >= n -> (ENDING ARRESTED) ELSE -> (NARRATE "not yet.")
`

const CALL_RECIPE = 'default -> sus(THE_GUARD), watch(3)'

/** the hand-expanded twin: each call replaced by its body with the argument
 *  substituted — the expansion-equivalence oracle */
const TWIN_RECIPE =
  'default -> SPAWN SUSPICION, NARRATE "The conductor\'s eyes linger on THE_GUARD.", ' +
  'IF count(name:SUSPICION) >= 3 -> (ENDING ARRESTED) ELSE -> (NARRATE "not yet.")'

const rigScript = (recipe: string, functions = ''): string => `
script: Function Rig
player: ANA
start: 0:0

entities:
  ANA is character
  THE_GUARD is character

prototypes:
  SUSPICION is state stackable

endings:
  ARRESTED: "Taken by the constabulary"
${functions}
verbs:
  STEW:
    actors: 1 kind:character
    duration: 0m
    recipes:
      ${recipe}
`

// ---------- expansion equivalence (the string-substitution case included) ----------

describe('expansion equivalence: call site === hand-expanded twin (§9.40)', () => {
  const call = compileScript(rigScript(CALL_RECIPE, CALL_FUNCTIONS), dict)
  const twin = compileScript(rigScript(TWIN_RECIPE), dict)

  const recipeOut = (c: CompiledContent): string => JSON.stringify(c.verbs.get('STEW')!.recipes[0]!.out)

  test('compiled recipe outcomes are identical, byte for byte', () => {
    expect(recipeOut(call.content)).toBe(recipeOut(twin.content))
  })

  test('playing the call and the twin produces byte-identical records', () => {
    const ra = run(call.content, call.state, '0:0 @ ANA STEW')
    const rb = run(twin.content, twin.state, '0:0 @ ANA STEW')
    expect(ra.rejected).toBeNull()
    expect(rb.rejected).toBeNull()
    expect(JSON.stringify(ra.records)).toBe(JSON.stringify(rb.records))
    // and the substitution really did land: the interpolated spelling, the
    // spawned pool, and the taken watch arm are all there
    expect(said(ra)).toEqual(["The conductor's eyes linger on THE_GUARD.", 'not yet.'])
    expect(ra.state.byName.get('SUSPICION')).toHaveLength(1)
    expect(snap(fold(ra.records, cloneState(call.state), dict, call.content))).toBe(snap(ra.state))
  })
})

// ---------- grammar: declaration forms and dispatch ----------

describe('functions: call-site dispatch and arity (§9.40)', () => {
  const table = rawTable([
    { name: 'one', params: ['x'], body: 'NARRATE "x is {x}."' },
    { name: 'zero', params: [], body: 'NARRATE "none."' },
  ])

  test('a call parses into an authored FN node (expansion is a separate pass)', () => {
    const out = parseOutcomes(`one(THE_GUARD), zero()`, dict, table)
    expect(out.map(o => o.op)).toEqual(['FN', 'FN'])
    expect(out[0]!.call).toBe('one')
    expect(out[1]!.call).toBe('zero')
    expect(out[1]!.args).toHaveLength(0)
  })

  test('arity mismatch is a load error: too few, too many, and on a parameterless call', () => {
    expect(() => parseOutcomes(`one()`, dict, table)).toThrow(/function 'one' expects 1 argument\(s\) \(x\), got 0/)
    expect(() => parseOutcomes(`one(A, B)`, dict, table)).toThrow(/function 'one' expects 1 argument\(s\) \(x\), got 2/)
    expect(() => parseOutcomes(`zero(1)`, dict, table)).toThrow(/function 'zero' expects 0 argument\(s\), got 1/)
  })

  test('unknown NAME( in outcome position is a compile error listing the vocabulary', () => {
    // no functions declared: ops only
    expect(() => parseOutcomes(`mystery(3)`, dict)).toThrow(
      /unknown 'mystery\(\.\.\.\)' in outcome position.*SPAWN, DESTROY.*no functions declared/s,
    )
    // with functions declared, they are listed too
    expect(() => parseOutcomes(`mystery(3)`, dict, table)).toThrow(
      /unknown 'mystery\(\.\.\.\)' in outcome position.*declared functions: one, zero/s,
    )
    // predicate helpers get the forgotten-IF hint
    expect(() => parseOutcomes(`exists(X) -> (NARRATE "a")`, dict)).toThrow(/IF keyword/)
  })

  test('declaring a function named like an outcome op is a compile error (§4.4, dict.isOp)', () => {
    const verb = rigVerb(['noop()'])
    expect(() => compileContent({ verbs: verb, functions: [{ name: 'SET', params: [], body: 'NARRATE "no."' }] }, dict))
      .toThrow(/function 'SET' collides with an outcome op/)
    expect(() => compileContent({ verbs: verb, functions: [{ name: 'IF', params: [], body: 'NARRATE "no."' }] }, dict))
      .toThrow(/cannot be named a conditional keyword/)
    expect(() => compileContent({
      verbs: verb,
      functions: [
        { name: 'dup', params: [], body: 'NARRATE "a"' },
        { name: 'dup', params: [], body: 'NARRATE "b"' },
      ],
    }, dict)).toThrow(/duplicate function 'dup'/)
  })

  test('the ContentRaw path compiles and plays end to end (compileContent is the only entry)', () => {
    const c = compileContent({
      verbs: rigVerb(['one(THE_GUARD)']),
      functions: [{ name: 'one', params: ['x'], body: 'NARRATE "x is {x}."' }],
    }, dict)
    const r = run(c, world(), '0:0 @ ANA STEW')
    expect(r.rejected).toBeNull()
    expect(said(r)).toEqual(['x is THE_GUARD.'])
  })
})

// ---------- the two substitution mechanisms ----------

describe('parameter substitution: AST positions and string interpolation (§9.40)', () => {
  const rig = (functions: FunctionDef[], recipe: string): CompiledContent => compileContent({
    verbs: rigVerb([recipe]),
    functions,
    // watch's body carries ENDING ARRESTED — the endings check runs on
    // expanded bodies, so the rig declares the table; SUSPICION is the
    // mint target for ref-position params
    prototypes: { SUSPICION: { kind: 'state', stackable: true } },
    endings: { ARRESTED: { title: 'Taken by the constabulary' } },
  }, dict)

  test('ref position: a param mints whatever prototype the call names (SPAWN x)', () => {
    const c = rig([{ name: 'mint', params: ['card'], body: 'SPAWN card' }], 'mint(SUSPICION)')
    const r = run(c, world(), '0:0 @ ANA STEW')
    expect(r.rejected).toBeNull()
    const spawn = done(r)!.outcomes[0]!
    expect(spawn.op).toBe('SPAWN')
    expect(spawn.args[0]).toMatchObject({ t: 'ref', ref: { name: 'SUSPICION', kind: 'state' } })
    expect(r.state.byName.get('SUSPICION')).toHaveLength(1)
  })

  test('delta position: the authored sign folds into the call’s number (SET(x, k, +n))', () => {
    const c = rig(
      [
        { name: 'bump', params: ['n'], body: 'SET(TOME, aspect:secret, +n)' },
        { name: 'drop', params: ['n'], body: 'SET(TOME, aspect:secret, -n)' },
      ],
      'bump(2), drop(4)',
    )
    const r = run(c, world(), '0:0 @ ANA STEW')
    expect(r.rejected).toBeNull()
    // secret starts at 1: +2 → 3, then −4 → −1; the log carries computed absolutes (§9.31)
    expect(done(r)!.outcomes.map(o => o.args[2])).toEqual([{ t: 'int', v: 3 }, { t: 'int', v: -1 }])
    expect(r.state.entities.get('s_tome')!.aspects['secret']).toBe(-1)
  })

  test('string position: paren-less and paren’d NARRATE take a param; {param} interpolates', () => {
    const c = rig(
      [{ name: 'cry', params: ['msg', 'who'], body: 'NARRATE msg, NARRATE("so does {who}.")' }],
      'cry("halt!", THE_GUARD)',
    )
    const r = run(c, world(), '0:0 @ ANA STEW')
    expect(r.rejected).toBeNull()
    expect(said(r)).toEqual(['halt!', 'so does THE_GUARD.'])
  })

  test('reserved tokens pass through; an undeclared {token} is a load error (pinned decision)', () => {
    // {actors} is §5.3 prose, never a parameter — it survives expansion
    const ok = rig([{ name: 'witness', params: ['who'], body: 'NARRATE "{actors} saw {who}."' }], 'witness(THE_GUARD)')
    const r = run(ok, world(), '0:0 @ ANA STEW')
    expect(said(r)).toEqual(['{actors} saw THE_GUARD.'])
    // a typo’d parameter would silently bake into every call’s journal prose —
    // this compiler has no warning channel, so undeclared interpolation is a
    // load error naming the token, the function, and the reserved set
    expect(() => rig([{ name: 'bad', params: ['x'], body: 'NARRATE "hi {x} and {nope}."' }], 'bad(THE_GUARD)'))
      .toThrow(/'\{nope\}' in a body string names neither a parameter of 'bad' nor a reserved token/)
    // spaced spellings never rewrite (§5.3 reads exact {token}s either), so a
    // spaced brace can only bake literally — the same typo, same load error,
    // whether or not the identifier names a parameter
    expect(() => rig([{ name: 's1', params: ['x'], body: 'NARRATE "hi { x }."' }], 's1(THE_GUARD)'))
      .toThrow(/'\{ x \}' in a body string names neither a parameter/)
    expect(() => rig([{ name: 's2', params: [], body: 'NARRATE "{ actors } gather."' }], 's2()'))
      .toThrow(/'\{ actors \}' in a body string names neither a parameter/)
    // commas/quotes inside braces are ordinary prose, not a token
    expect(() => rig([{ name: 'prose', params: ['x'], body: 'NARRATE "the set { a, b } holds {x}"' }], 'prose(THE_GUARD)'))
      .not.toThrow()
  })

  test('IF-condition position: watch(3) makes the count threshold the call’s number', () => {
    const c = rig(
      [{ name: 'watch', params: ['n'], body: 'IF count(name:SUSPICION) >= n -> (ENDING ARRESTED) ELSE -> (NARRATE "not yet.")' }],
      'watch(3)',
    )
    const s = world()
    addSuspicion(s, 2) // 2 >= 3 is false → the ELSE arm runs
    const r = run(c, s, '0:0 @ ANA STEW')
    expect(r.rejected).toBeNull()
    expect(said(r)).toEqual(['not yet.'])
    expect(r.state.ended).toBeNull()
    addSuspicion(r.state, 1) // now 3 >= 3: the same compiled content arrests
    const r2 = run(c, r.state, '0:1 @ ANA STEW', 10)
    expect(r2.state.ended).toEqual({ id: 'ARRESTED', title: 'Taken by the constabulary', flavour: 'melancholy' })
  })

  test('a kind mismatch the AST can represent refuses the window at runtime, like any kind error', () => {
    // DESTROY of a value literal would iterate nothing (iteration-style op);
    // SPAWN actually needs its name ref, so the mismatch refuses the window
    const c = rig([{ name: 'mint', params: ['card'], body: 'SPAWN card' }], 'mint(7)')
    const r = run(c, world(), '0:0 @ ANA STEW')
    expect(r.rejected).toBeNull()
    expect(refusal(r)).toMatch(/SPAWN needs a name ref/)
  })
})

// ---------- nesting, recursion, depth ----------

describe('nesting, recursion cycles, and the depth cap (§9.40)', () => {
  const chain = (n: number): FunctionDef[] =>
    Array.from({ length: n }, (_, i) => ({
      name: `f${i + 1}`,
      params: [],
      body: i + 1 < n ? `f${i + 2}()` : 'NARRATE "bottom."',
    }))

  test('a body calling a function expands transitively at the call site (forward refs fine)', () => {
    const c = compileContent({
      verbs: rigVerb(['outer(THE_GUARD)']),
      functions: [
        { name: 'inner', params: ['x'], body: 'NARRATE "inner {x}."' },
        { name: 'outer', params: ['x'], body: 'inner(x), SPAWN SUSPICION' },
      ],
      prototypes: { SUSPICION: { kind: 'state', stackable: true } },
    }, dict)
    const r = run(c, world(), '0:0 @ ANA STEW')
    expect(r.rejected).toBeNull()
    expect(said(r)).toEqual(['inner THE_GUARD.'])
    expect(r.state.byName.get('SUSPICION')).toHaveLength(1)
  })

  test('mutual recursion is a load error naming the cycle; self-recursion names itself', () => {
    expect(() => compileContent({
      verbs: {}, functions: [
        { name: 'a', params: [], body: 'b()' },
        { name: 'b', params: [], body: 'a()' },
      ],
    }, dict)).toThrow(/functions recurse: a -> b -> a/)
    expect(() => compileContent({
      verbs: {}, functions: [{ name: 'self', params: [], body: 'self()' }],
    }, dict)).toThrow(/functions recurse: self -> self/)
  })

  test('a depth-8 chain of distinct functions loads; depth 9 is a load error', () => {
    const verb = rigVerb(['f1()'])
    const eight = compileContent({ verbs: verb, functions: chain(8) }, dict)
    const r = run(eight, world(), '0:0 @ ANA STEW')
    expect(said(r)).toEqual(['bottom.'])
    expect(() => compileContent({ verbs: verb, functions: chain(9) }, dict))
      .toThrow(/function 'f1' sits 9 calls deep — the nesting cap is 8/)
  })

  test('nested-call arity mismatches are load errors even before any call site runs', () => {
    expect(() => compileContent({
      verbs: {}, functions: [
        { name: 'one', params: ['x'], body: 'NARRATE "x is {x}"' },
        { name: 'outer', params: [], body: 'one()' },
      ],
    }, dict)).toThrow(/function 'outer': function 'one' expects 1 argument\(s\)/)
  })
})

// ---------- param shadowing ----------

describe('a parameter shadows an entity of the same name inside the body (§9.40)', () => {
  test('DESTROY GUARD in the body destroys the BOUND card, not the entity GUARD', () => {
    const c = compileContent({
      verbs: rigVerb(['f(THE_GUARD)']),
      functions: [{ name: 'f', params: ['GUARD'], body: 'DESTROY GUARD' }],
    }, dict)
    const s = world()
    // GUARD is a decoy entity living beside THE_GUARD
    addEntity(s, { id: 's_guard', kind: 'character', name: 'GUARD', aspects: {}, attrs: {}, bornAction: null, deadAction: null, stackable: false })
    const r = run(c, s, '0:0 @ ANA STEW')
    expect(r.rejected).toBeNull()
    expect(r.state.entities.get('s_the_guard')!.deadAction).not.toBeNull() // the bound card died
    expect(r.state.entities.get('s_guard')!.deadAction).toBeNull()          // the namesake lives
  })
})

// ---------- call sites in triggers; expansion before the guards ----------

describe('trigger call sites and the expansion-before-guards ordering (§9.40)', () => {
  test('a trigger outcome calls a function; its expansion rides the EVENT record', () => {
    const c = compileScript(`
script: Trigger Rig
player: ANA
start: 0:0

entities:
  ANA is character
  THE_GUARD is character

prototypes:
  SUSPICION is state stackable

triggers:
  patrol at hour 1 -> sus(THE_GUARD)
${CALL_FUNCTIONS}
verbs:
  STEW:
    actors: 1 kind:character
    duration: 1h
    recipes:
      default -> NARRATE "time passes."
`, dict)
    const r = run(c.content, c.state, '0:0 @ ANA STEW') // the 1h duration crosses hour 1 (slot 2)
    expect(r.rejected).toBeNull()
    const event = r.records.find(x => x.kind === 'event')!
    expect(event.trigger).toBe('patrol')
    // the trigger's narration reads straight off the EVENT row's outcomes
    const a = event.outcomes.find(o => o.op === 'NARRATE')!.args[0]!
    expect(a.t === 'str' ? a.v : '').toBe("The conductor's eyes linger on THE_GUARD.")
    expect(r.state.byName.get('SUSPICION')).toHaveLength(1)
    expect(snap(fold(r.records, cloneState(c.state), dict, c.content))).toBe(snap(r.state))
  })

  test('a role-ref body outcome from a trigger rejects the step at runtime (empty event bindings)', () => {
    const c = compileScript(`
script: Quake Rig
player: ANA
start: 0:0

entities:
  ANA is character
  TOME is text at ANA

triggers:
  quake at hour 1 -> shake()

functions:
  shake -> MOVE ACTORS INTO OBJECT

verbs:
  STEW:
    actors: 1 kind:character
    duration: 1h
    recipes:
      default -> NARRATE "time passes."
`, dict)
    const before = snap(c.state)
    const r = run(c.content, c.state, '0:0 @ ANA STEW')
    expect(r.rejected).toMatch(/unresolvable destination/) // MOVE cannot proceed with empty bindings (§9.19)
    expect(r.records).toHaveLength(0)
    expect(snap(c.state)).toBe(before) // nothing applied, clock unmoved
  })

  test('PROGRESS in a body: load error from a trigger, fine from a recipe (ordering test)', () => {
    const fn: FunctionDef = { name: 'tick', params: [], body: 'PROGRESS(30m, "tick")' }
    // the trigger guard walks EXPANDED bodies — the call site is guilty
    expect(() => compileContent({
      verbs: rigVerb(['tick()']),
      triggers: [{ id: 'tock', slot: 2, once: true, out: ['tick()'] }],
      functions: [fn],
    }, dict)).toThrow(/trigger tock: PROGRESS belongs to verb recipes, not events/)

    // the same body from a recipe is ordinary content
    const c = compileContent({ verbs: rigVerb(['tick()']), functions: [fn] }, dict)
    const r = run(c, world(), '0:0 @ ANA STEW')
    expect(r.rejected).toBeNull()
    expect(act(r)!.outcomes[0]).toMatchObject({ op: 'PROGRESS', args: [{ t: 'dur', v: 30 }, { t: 'str', v: 'tick' }] })
  })

  test('an undeclared ENDING id inside a body is a load error through the expansion', () => {
    expect(() => compileContent({
      verbs: rigVerb(['doom()']),
      functions: [{ name: 'doom', params: [], body: 'ENDING NOT_DECLARED' }],
    }, dict)).toThrow(/ENDING 'NOT_DECLARED' is not in the endings table/)
    // the same through the TRIGGER path — expansion precedes checkEndingRefs there too
    expect(() => compileContent({
      verbs: rigVerb(['NARRATE "idle."']),
      triggers: [{ id: 'doom_t', slot: 2, once: true, out: ['doom()'] }],
      functions: [{ name: 'doom', params: [], body: 'ENDING NOT_DECLARED' }],
    }, dict)).toThrow(/trigger doom_t: ENDING 'NOT_DECLARED' is not in the endings table/)
  })

  test('guards see INTO a nested IF inside a body: PROGRESS in a branch is still a trigger load error', () => {
    // expansion flattens the call, then the recursive guard walks the branch —
    // neither the FN boundary nor the IF boundary may hide a PROGRESS
    expect(() => compileContent({
      verbs: rigVerb(['NARRATE "idle."']),
      triggers: [{ id: 'sneak', slot: 2, once: true, out: ['sneak()'] }],
      functions: [{ name: 'sneak', params: [], body: 'IF exists(ANA) -> (PROGRESS(30m, "tick")) ELSE -> (NARRATE "no")' }],
    }, dict)).toThrow(/trigger sneak: PROGRESS belongs to verb recipes, not events/)
  })

  test('the count()-typo lint applies to an expanded body condition on the trigger path too', () => {
    expect(() => compileContent({
      verbs: rigVerb(['NARRATE "idle."']),
      triggers: [{ id: 'linted', slot: 2, once: true, out: ['watch(3)'] }],
      functions: [{ name: 'watch', params: ['n'], body: 'IF count(DREAD) >= n -> (NARRATE "raised")' }],
    }, dict)).toThrow(/trigger linted: count\(\) argument 'DREAD' is neither a role/)
    // and the role-operand lint: count(actors) over a trigger-called body condition
    expect(() => compileContent({
      verbs: rigVerb(['NARRATE "idle."']),
      triggers: [{ id: 'roled', slot: 2, once: true, out: ['watch(3)'] }],
      functions: [{ name: 'watch', params: ['n'], body: 'IF count(actors) >= n -> (NARRATE "raised")' }],
    }, dict)).toThrow(/trigger roled: IF operand count\(actors\) reads a step role/)
  })
})

// ---------- IF in a body, end to end ----------

describe('IF inside a function body: both arms survive substitution, ENDING included (§9.39 x §9.40)', () => {
  const { content, state } = compileScript(rigScript(CALL_RECIPE, CALL_FUNCTIONS), dict)

  test('three stews: the ELSE arm twice, then watch(3) arrests through its param', () => {
    let cur = state
    let id = 1
    const all: ActionRecord[] = []
    const told: string[][] = []
    for (let i = 0; i < 3; i++) {
      const r = run(content, cur, '0:0 @ ANA STEW', id)
      expect(r.rejected).toBeNull()
      told.push(said(r))
      all.push(...r.records)
      id += r.records.length
      cur = r.state
    }
    expect(told).toEqual([
      ["The conductor's eyes linger on THE_GUARD.", 'not yet.'],
      ["The conductor's eyes linger on THE_GUARD.", 'not yet.'],
      ["The conductor's eyes linger on THE_GUARD."], // the third stew's own SPAWN flipped watch(3)
    ])
    expect(cur.byName.get('SUSPICION')).toHaveLength(3)
    expect(cur.ended).toEqual({ id: 'ARRESTED', title: 'Taken by the constabulary', flavour: 'melancholy' })
    const over = run(content, cur, '0:0 @ ANA STEW', id)
    expect(over.rejected).toMatch(/the story has ended/)
    // and the whole run folds back identically
    expect(snap(fold(all, cloneState(state), dict, content))).toBe(snap(cur))
  })
})

// ---------- round-trip: the log never sees a call ----------

describe('canonical equals expansion: records round-trip and carry no call text (§9.40)', () => {
  test('expanded outcomes round-trip through JSONL and the fold; no FN op or call text anywhere', () => {
    const { content, state } = compileScript(rigScript(CALL_RECIPE, CALL_FUNCTIONS), dict)
    let cur = state
    let id = 1
    const all: ActionRecord[] = []
    for (let i = 0; i < 3; i++) {
      const r = run(content, cur, '0:0 @ ANA STEW', id)
      all.push(...r.records)
      id += r.records.length
      cur = r.state
    }
    const dump = JSON.stringify(all)
    expect(dump).not.toMatch(/\b(sus|watch)\(/)  // no call text anywhere
    expect(dump).not.toContain('"FN"')           // the authored op never reaches a record
    for (const rec of all) {
      // canonical sentences re-parse to themselves (every outcome is concrete)
      expect(formatSentence(parseSentence(rec.sentence, dict), dict)).toBe(rec.sentence)
    }
    const dir = mkdtempSync(join(tmpdir(), 'chorus-fn-'))
    const path = join(dir, 'log.jsonl')
    try {
      saveLog(path, all)
      const replayed = fold(loadLog(path), cloneState(state), dict, content)
      expect(snap(replayed)).toBe(snap(cur))
    } finally {
      rmSync(dir, { recursive: true, force: true })
    }
  })

  test('the formatter refuses authored form: a call node and a parameter both throw', () => {
    const table = rawTable([{ name: 'one', params: ['x'], body: 'NARRATE "x is {x}"' }])
    const [node] = parseOutcomes(`one(THE_GUARD)`, dict, table)
    expect(() => formatSentence({
      kind: 'action', clock: null, trigger: null,
      clauses: [], outcomes: [node!],
    }, dict)).toThrow(/a function call is authored form only/)
    expect(() => formatSentence({
      kind: 'action', clock: null, trigger: null,
      clauses: [], outcomes: [{ op: 'SPAWN', args: [{ t: 'param', name: 'x' }] }],
    }, dict)).toThrow(/a function parameter is authored form only/)
  })
})

// ---------- rig helpers ----------

function rigVerb(recipe: string[]): ContentRaw['verbs'] {
  return {
    STEW: {
      slots: [{ role: 'actors', min: 1, max: 1, query: 'kind:character' }],
      duration: 0,
      recipes: recipe.map(out => ({ out: [out] })),
    },
  } as ContentRaw['verbs']
}

/** a tiny world: ANA (actor), THE_GUARD (shadow target), TOME (secret:1) */
function world(): State {
  const s = newState(0)
  addEntity(s, { id: 's_house', kind: 'place', name: 'HOUSE', aspects: {}, attrs: {}, bornAction: null, deadAction: null, stackable: false })
  addEntity(s, { id: 's_ana', kind: 'character', name: 'ANA', aspects: {}, attrs: { container: 's_house' }, bornAction: null, deadAction: null, stackable: false })
  addEntity(s, { id: 's_the_guard', kind: 'character', name: 'THE_GUARD', aspects: {}, attrs: { container: 's_house' }, bornAction: null, deadAction: null, stackable: false })
  addEntity(s, { id: 's_tome', kind: 'text', name: 'TOME', aspects: { secret: 1 }, attrs: { container: 's_ana' }, bornAction: null, deadAction: null, stackable: false })
  return s
}

function addSuspicion(s: State, n: number): void {
  for (let i = 0; i < n; i++) {
    addEntity(s, {
      id: `w${s.entities.size}`, kind: 'state', name: 'SUSPICION', aspects: {}, attrs: {},
      bornAction: null, deadAction: null, stackable: true,
    })
  }
}

/** build a parse-level FnTable the way compileContent's passes do — enough
 *  for dispatch/arity tests without a full content compile */
function rawTable(defs: FunctionDef[]): FnTable {
  const table: FnTable = { byName: new Map<string, FnTemplate>() }
  for (const d of defs) table.byName.set(d.name, { name: d.name, params: d.params, body: [] })
  for (const d of defs) table.byName.get(d.name)!.body = parseFunctionBody(d.body, dict, table, d.params)
  return table
}
