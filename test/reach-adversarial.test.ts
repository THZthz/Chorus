// Hand-crafted adversarial scenarios for the static card-reachability analyzer
// (src/reach.ts) — mechanisms that random generation under-samples. Each test
// states the mechanism, the DESIRED guarantee, the analyzer verdict DERIVED
// from the engine semantics (src/engine.ts / src/types.ts /
// docs/spec/text-engine-v0.2.md), and then CONFIRMS the derivation against
// forward play: the exhaustive bounded explorer (reach-fuzz-lib explore) and
// manual settleStep traces.
//
// Verdict contract under test (reach.ts header): 'unobtainable' must never
// fire wrongly; 'obtainable' must be backed by a real chain; guarded sites
// demote to 'unproven'. A forward trace refuting an 'obtainable'/'unobtainable'
// claim would be an analyzer bug (asserted with test.fails inversions); an
// 'unproven' next to a reachable truth is sound conservatism and is DOCUMENTED,
// not flipped.
import { describe, expect, test } from 'vitest'
import { compileContent, type CompiledContent, type ContentRaw } from '../src/content.js'
import { coreDictionary } from '../src/dict.js'
import { rollDie } from '../src/dice.js'
import { parseClock } from '../src/clock.js'
import { parseSentence } from '../src/parse.js'
import { executeStep } from '../src/engine.js'
import { analyzeCard } from '../src/reach.js'
import { settleStep } from './settle.js'
import { explore } from './reach-fuzz-lib.js'
import { addEntity, newState, type Entity, type State } from '../src/types.js'

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

const compile = (raw: ContentRaw): CompiledContent => {
  // rig verbs are code-registered (§10) — the parallel.test.ts precedent
  for (const name of Object.keys(raw.verbs)) if (!dict.isVerb(name)) dict.registerVerb(name)
  return compileContent(raw, dict)
}

/** settleStep runner with monotonic record ids, bound to one scenario */
const runner = () => {
  let nextId = 1
  return {
    /** parse + settle one sentence step; asserts acceptance */
    run: (content: CompiledContent, state: State, src: string) => {
      const s = parseSentence(src, dict)
      const res = settleStep(content, dict, state, { clock: s.clock ?? { day: 0, halfhour: 0 }, clauses: s.clauses }, nextId)
      nextId += res.records.length + 1
      expect(res.rejected, src).toBeNull()
      return res
    },
    /** pure clock advance to `clock` (settleStep closes every window due there) */
    advance: (content: CompiledContent, state: State, clock: string) => {
      const res = settleStep(content, dict, state, { clock: parseClock(clock), clauses: [] }, nextId)
      nextId += res.records.length + 1
      return res
    },
  }
}

const aliveNames = (s: State): Set<string> =>
  new Set([...s.entities.values()].filter(e => e.deadAction === null).map(e => e.name))

/** a clause built programmatically (pool copies bind by #id, §9.38) */
const probeClause = (verb: string, actors: { form: 'name'; name: string }[], objects: { form: 'id'; id: string }[]) => ({
  actors, at: null, verb, to: null, objects, with: [], about: null,
})

describe('reach adversarial scenarios', () => {
  // ---------------------------------------------------------------- S1
  test('S1 — parallel windows: verb A (60) + verb B (30) overlap; the card minted by A\u2019s completion is then usable by B; locks free cards only until window close', () => {
    // Mechanism: two verb stations run overlapping windows (§6.7 v0.5). FORGE
    // (60m) mints KEYCARD at its COMPLETION; INSPECT (30m) binds a kind:item
    // into its committed object role. LAMP is locked by INSPECT's window for
    // exactly [0,30] — a step beginning at the close (§6.1: validation runs
    // against the post-advance draft) binds it again. The mint lands only at
    // FORGE's completion, so KEYCARD cannot exist before minute 60, and the
    // INSPECT station frees at 30 (its window closed), so a SECOND INSPECT at
    // minute 60 can bind the fresh KEYCARD.
    //
    // Desired guarantee: KEYCARD obtainable AND usable (bindable into a
    // committable slot — semantics (b)); LAMP stays obtainable+usable across
    // the lock. The analyzer is time-free (existential over unbounded play),
    // so window geometry cannot demote the claims — but the static fixpoint
    // must chain the mint (FORGE recipe) into the slot (INSPECT.object).
    const content = compile({
      prototypes: { KEYCARD: { kind: 'item' } },
      verbs: {
        FORGE: {
          slots: [{ role: 'actors', min: 1, max: 1, query: 'kind:character' }],
          duration: 60,
          recipes: [{ out: ['SPAWN KEYCARD'] }],
        },
        INSPECT: {
          slots: [
            { role: 'actors', min: 1, max: 1, query: 'kind:character' },
            { role: 'object', min: 1, max: 1, query: 'kind:item' },
          ],
          duration: 30,
          recipes: [{ out: ['NARRATE "inspected."'] }],
        },
        TOUCH: {
          slots: [
            { role: 'actors', min: 1, max: 1, query: 'kind:character' },
            { role: 'object', min: 1, max: 1, query: 'kind:item' },
          ],
          duration: 30,
          recipes: [{ out: ['NARRATE "touched."'] }],
        },
        WAIT: {
          slots: [{ role: 'actors', min: 1, max: 1, query: 'kind:character' }],
          duration: 30,
          recipes: [{ out: [] }],
        },
      },
    })
    const state = stateWith(ent('ANA', 'character'), ent('LAMP', 'item'))

    // analyzer derivation: FORGE is committable (ANA fills actors) → its SPAWN
    // site is proven → KEYCARD obtainable; INSPECT is committable and its
    // object slot query kind:item matches KEYCARD's descriptor → usable.
    const card = analyzeCard(content, state, 'KEYCARD', { usable: true })
    expect(card.verdict).toBe('obtainable')
    expect(card.existence).toBe('proven')
    expect(card.chain.join('\n')).toContain('FORGE')
    expect(card.usableVia.some(v => v.startsWith('INSPECT.object'))).toBe(true)
    const lamp = analyzeCard(content, state, 'LAMP', { usable: true })
    expect(lamp.verdict).toBe('obtainable')

    // forward truth: the parallel geometry above, step by step. BEGIN steps
    // go through executeStep (settleStep would complete the windows it opens);
    // raw windows: [0,60] and [0,30] open IN PARALLEL.
    const mkBegin = (content: CompiledContent, state: State, src: string, id: number) => {
      const s = parseSentence(src, dict)
      return executeStep(content, dict, state, { clock: s.clock ?? { day: 0, halfhour: 0 }, clauses: s.clauses }, { firstRecordId: id })
    }
    const r = runner()
    let cur = state
    const b1 = mkBegin(content, cur, '0:0 @ ANA FORGE', 1)          // window [0,60]
    expect(b1.rejected).toBeNull()
    cur = b1.state
    expect(cur.windows.size).toBe(1)
    const b2 = mkBegin(content, cur, '0:0 @ ANA INSPECT LAMP', 2)   // window [0,30] — IN PARALLEL
    expect(b2.rejected).toBeNull()
    cur = b2.state
    expect(cur.windows.size).toBe(2)
    // LAMP is busy until 0:1; a step beginning exactly at the close rebinds it.
    // Settling that step also runs the advance through minute 60: INSPECT's
    // window closed at 30, FORGE's closes at 60 → KEYCARD mints.
    const touch = r.run(content, cur, '0:1 @ ANA TOUCH LAMP')
    expect(touch.rejected).toBeNull()
    cur = touch.state
    expect(aliveNames(cur)).toContain('KEYCARD')
    // the INSPECT station is free again (its window closed at 30): bind the mint
    cur = r.run(content, cur, '0:2 @ ANA INSPECT KEYCARD').state
    expect(aliveNames(cur)).toContain('KEYCARD')

    // secondary ground truth: the exhaustive bounded explorer reaches it too
    const fw = explore(content, dict, state, 10, 2000)
    expect(fw.capped).toBe(false)
    expect(fw.everNames.has('KEYCARD')).toBe(true)
  })

  // ---------------------------------------------------------------- S2
  test('S2 — expiry chain: SET(X, expires) + `on expire of X -> SPAWN Y`; the authored trigger REPLACES the built-in destroy (X survives) and re-fires', () => {
    // Mechanism: TORCH (duration 0) writes PAPER.expires = 30m — the SET
    // materializes expiresAt = now + 30 (§4.2/§9.46). The first crossed
    // halfhour boundary at/after expiresAt fires the AUTHORED `on expire of
    // PAPER` trigger (§9.32), which REPLACES the built-in destroy: its body is
    // the whole event — PAPER is neither destroyed nor re-armed, so it keeps
    // the now-past expiresAt and the trigger re-fires at EVERY subsequent
    // crossing. The first firing mints ASH; the second collides with the
    // living ASH and rejects the whole advance (§9.19 — the event throw is
    // deterministic, so the world can never advance past that boundary again).
    //
    // Desired guarantee: ASH obtainable (the expiry trigger must count as a
    // producer once its named card is available AND expirable — reach.ts's
    // expiry branch); PAPER obtainable (initial). The re-fire bomb does not
    // un-mint ASH: obtainability is existential, and the FIRST firing suffices.
    const content = compile({
      prototypes: { ASH: { kind: 'item' } },
      verbs: {
        TORCH: {
          slots: [{ role: 'actors', min: 1, max: 1, query: 'kind:character' }],
          duration: 0,
          recipes: [{ out: ['SET(PAPER, expires, 30m)'] }],
        },
        WAIT: {
          slots: [{ role: 'actors', min: 1, max: 1, query: 'kind:character' }],
          duration: 30,
          recipes: [{ out: [] }],
        },
      },
      triggers: [{ id: 'paper_to_ash', onExpireOf: 'PAPER', out: ['SPAWN ASH'] }],
    })
    const state = stateWith(ent('ANA', 'character'), ent('PAPER', 'item'))

    // analyzer derivation: PAPER is initial → available; the SET site names
    // key 'expires' with targetName 'PAPER' → PAPER is "expirable"; the expiry
    // trigger's SPAWN site is therefore proven → ASH obtainable.
    const ash = analyzeCard(content, state, 'ASH')
    expect(ash.verdict).toBe('obtainable')
    expect(ash.chain.join('\n')).toContain('paper_to_ash')
    expect(analyzeCard(content, state, 'PAPER').verdict).toBe('obtainable')

    // forward truth: commit TORCH at 0:0 (duration 0 → completes at once,
    // expiresAt anchored to the window START = minute 0 → 30)
    const r = runner()
    let cur = r.run(content, state, '0:0 @ ANA TORCH').state
    expect(cur.entities.get('s_paper')!.attrs['expiresAt']).toBe(30)
    // first crossing (minute 30): the authored trigger fires, ASH mints…
    cur = r.advance(content, cur, '0:1').state
    expect(aliveNames(cur)).toContain('ASH')
    // …and PAPER SURVIVES: the authored trigger replaced the built-in destroy
    expect(aliveNames(cur)).toContain('PAPER')
    // second crossing (minute 60): the un-destroyed PAPER re-fires the same
    // trigger (it matches by NAME whenever expiresAt has passed) → SPAWN ASH
    // collides with the living ASH → the WHOLE advance is rejected
    const bomb = r.advance(content, cur, '0:2')
    expect(bomb.rejected).not.toBeNull()
    expect(bomb.rejected).toContain('ASH')
    expect(aliveNames(bomb.state)).toContain('ASH')
    expect(aliveNames(bomb.state)).toContain('PAPER')

    // secondary ground truth: the explorer also witnesses the first firing
    const fw = explore(content, dict, state, 8, 2000)
    expect(fw.everNames.has('ASH')).toBe(true)
    expect(fw.everNames.has('PAPER')).toBe(true)
  })

  // ---------------------------------------------------------------- S3
  test('S3 — REPLACE chain: SPAWN A; REPLACE A WITH B; REPLACE B WITH C — C obtainable, A/B transient', () => {
    // Mechanism: each REPLACE (§9.33) mints the WITH prototype from the
    // prototype table while soft-destroying the target. REPLACE is a throwing
    // op (an unresolvable target refuses the draft at completion), but
    // reach.ts's selfSafe rule excuses it: the target is provably available
    // and no other list kills it, so the throw cannot fire on this chain.
    //
    // Desired guarantee: TREE obtainable through the two-hop chain (the
    // fixpoint must prove SEED first, then the REPLACE sites in later rounds);
    // SPROUT obtainable too (it exists between WATER and TEND — existence is
    // existential, transience does not demote). Forward: SEED and SPROUT die,
    // only TREE survives the sequence.
    const content = compile({
      prototypes: {
        SEED: { kind: 'item' },
        SPROUT: { kind: 'item' },
        TREE: { kind: 'item' },
      },
      verbs: {
        PLANT: {
          slots: [{ role: 'actors', min: 1, max: 1, query: 'kind:character' }],
          duration: 0,
          recipes: [{ out: ['SPAWN SEED'] }],
        },
        WATER: {
          slots: [{ role: 'actors', min: 1, max: 1, query: 'kind:character' }],
          duration: 0,
          recipes: [{ out: ['REPLACE SEED WITH SPROUT'] }],
        },
        TEND: {
          slots: [{ role: 'actors', min: 1, max: 1, query: 'kind:character' }],
          duration: 0,
          recipes: [{ out: ['REPLACE SPROUT WITH TREE'] }],
        },
      },
    })
    const state = stateWith(ent('ANA', 'character'))

    expect(analyzeCard(content, state, 'TREE').verdict).toBe('obtainable')
    expect(analyzeCard(content, state, 'SPROUT').verdict).toBe('obtainable')
    const seed = analyzeCard(content, state, 'SEED')
    expect(seed.verdict).toBe('obtainable')
    expect(seed.chain.join('\n')).toContain('SPAWN mints')

    // forward truth: the chain plays out; each hop kills its input
    const r = runner()
    let cur = r.run(content, state, '0:0 @ ANA PLANT').state
    expect(aliveNames(cur)).toContain('SEED')
    cur = r.run(content, cur, '0:0 @ ANA WATER').state
    expect(aliveNames(cur)).not.toContain('SEED')
    expect(aliveNames(cur)).toContain('SPROUT')
    cur = r.run(content, cur, '0:0 @ ANA TEND').state
    expect(aliveNames(cur)).not.toContain('SPROUT')
    expect(aliveNames(cur)).toContain('TREE')

    const fw = explore(content, dict, state, 10, 2000)
    expect(fw.everNames.has('TREE')).toBe(true)
    expect(fw.everNames.has('SPROUT')).toBe(true) // transient but witnessed
  })

  // ---------------------------------------------------------------- S4
  test('S4 — stackable pool feeding a min:2 slot: PROBE commits only after two STEW mints', () => {
    // Mechanism: SUSPICION is `stackable` (§9.38) — every STEW appends a copy
    // to the name pool. PROBE's object slot takes TWO kind:state cards; roles
    // take DISTINCT cards, and the pool discipline binds same-named copies
    // individually (by #id). The analyzer's slotSupport expands a stackable
    // mint fact into multiple name#copy instances (capped), so min:2 is
    // satisfiable from ONE source — this is exactly fuzz seed 315's
    // distinctness rule, exercised on a stackable pool.
    //
    // Desired guarantee: INSIGHT obtainable (two instances of one stackable
    // mint fill the slot across unbounded play). Forward: one STEW is not
    // enough (PROBE cannot bind), two STEWs commit PROBE and mint INSIGHT.
    const content = compile({
      prototypes: {
        SUSPICION: { kind: 'state', stackable: true },
        INSIGHT: { kind: 'idea' },
      },
      verbs: {
        STEW: {
          slots: [{ role: 'actors', min: 1, max: 1, query: 'kind:character' }],
          duration: 0,
          recipes: [{ out: ['SPAWN SUSPICION'] }],
        },
        PROBE: {
          slots: [
            { role: 'actors', min: 1, max: 1, query: 'kind:character' },
            { role: 'object', min: 2, max: 2, query: 'kind:state' },
          ],
          duration: 0,
          recipes: [{ out: ['SPAWN INSIGHT'] }],
        },
      },
    })
    const state = stateWith(ent('ANA', 'character'))

    const insight = analyzeCard(content, state, 'INSIGHT')
    expect(insight.verdict).toBe('obtainable')
    expect(insight.chain.join('\n')).toContain('SUSPICION')
    expect(insight.chain.join('\n')).toContain('PROBE committable')

    // forward truth: STEW twice (pool of two), then bind BOTH copies by #id
    const r = runner()
    let cur = r.run(content, state, '0:0 @ ANA STEW').state
    cur = r.run(content, cur, '0:0 @ ANA STEW').state
    const pool = cur.byName.get('SUSPICION')!
    expect(pool).toHaveLength(2)
    // one copy is not enough: PROBE's object slot takes 2 — a single binding
    // under-fills the slot and rejects at validation (§6.1)
    const single = settleStep(
      content, dict, cur,
      { clock: parseClock('0:0'), clauses: [{ actors: [{ form: 'name', name: 'ANA' }], at: null, verb: 'PROBE', to: null, objects: [{ form: 'id', id: pool[0]! }], with: [], about: null }] },
      800,
    )
    expect(single.rejected).not.toBeNull()
    expect(single.rejected).toContain("'object' takes 2..2, got 1")
    // both copies bind individually (the pool rule, §9.38) and PROBE commits
    const probe = settleStep(
      content, dict, cur,
      { clock: parseClock('0:0'), clauses: [probeClause('PROBE', [{ form: 'name', name: 'ANA' }], pool.map(id => ({ form: 'id' as const, id })))] },
      900,
    )
    expect(probe.rejected).toBeNull()
    expect(aliveNames(probe.state)).toContain('INSIGHT')

    // secondary ground truth: reach-fuzz-lib's explore binds roles by NAME
    // refs only, and a name ref resolves to the OLDEST pool copy (§9.38) — so
    // the generic explorer can never bind two same-named SUSPICIONs into one
    // clause (they dedup to one entity). The manual #id trace above is the
    // forward ground truth for the pool-individual binding; explore still
    // witnesses both STEW mints.
    const fw = explore(content, dict, state, 10, 4000)
    expect(fw.everNames.has('SUSPICION')).toBe(true)
  })

  // ---------------------------------------------------------------- S5
  test('S5 — derive of derive: SPAWN D1 from an object, then SPAWN D2 with the D1-mint as object → name chains D2_D1_OBJ', () => {
    // Mechanism: `deriving name from object` (§6.4) mints `${PROTO}_${OBJECT}`
    // — the object half is chosen at play time from the clause's object
    // binding. Chaining: HEAR derives RUMOUR from THE_BOOK → RUMOUR_THE_BOOK
    // (kind idea); RETELL derives WHISPER from that mint → the name compounds:
    // WHISPER_RUMOUR_THE_BOOK.
    //
    // Desired guarantee: the analyzer must never call a derived shape
    // structurally absent — 'RUMOUR_THE_BOOK' and 'WHISPER_RUMOUR_THE_BOOK'
    // are candidate (unproven), NOT unobtainable (reach.ts's derivedPattern
    // rule). Bare 'RUMOUR' is candidate too (the documented mid-window-death
    // drop can mint it). Forward truth: the EXACT chained names mint.
    const content = compile({
      prototypes: {
        RUMOUR: { kind: 'idea', derive: { nameFrom: 'object' } },
        WHISPER: { kind: 'idea', derive: { nameFrom: 'object' } },
      },
      verbs: {
        HEAR: {
          slots: [
            { role: 'actors', min: 1, max: 1, query: 'kind:character' },
            { role: 'object', min: 1, max: 1, query: 'kind:item' },
          ],
          duration: 0,
          recipes: [{ out: ['SPAWN RUMOUR'] }],
        },
        RETELL: {
          slots: [
            { role: 'actors', min: 1, max: 1, query: 'kind:character' },
            { role: 'object', min: 1, max: 1, query: 'kind:idea' },
          ],
          duration: 0,
          recipes: [{ out: ['SPAWN WHISPER'] }],
        },
      },
    })
    const state = stateWith(ent('ANA', 'character'), ent('THE_BOOK', 'item', { secret: 2 }))

    const d1 = analyzeCard(content, state, 'RUMOUR_THE_BOOK')
    expect(d1.verdict).toBe('unproven')
    expect(d1.existence).toBe('candidate')
    expect(d1.notes.join(' ')).toContain('derived-mint')
    const d2 = analyzeCard(content, state, 'WHISPER_RUMOUR_THE_BOOK')
    expect(d2.verdict).toBe('unproven')
    expect(d2.existence).toBe('candidate')
    const bare = analyzeCard(content, state, 'RUMOUR')
    expect(bare.verdict).toBe('unproven')
    expect(bare.notes.join(' ')).toContain('derive')

    // forward truth: the two-hop derive chain mints the compounded names
    const r = runner()
    let cur = r.run(content, state, '0:0 @ ANA HEAR THE_BOOK').state
    expect(aliveNames(cur)).toContain('RUMOUR_THE_BOOK')
    cur = r.run(content, cur, '0:0 @ ANA RETELL RUMOUR_THE_BOOK').state
    expect(aliveNames(cur)).toContain('WHISPER_RUMOUR_THE_BOOK')
    expect(aliveNames(cur)).toContain('RUMOUR_THE_BOOK') // the object survives its use

    const fw = explore(content, dict, state, 8, 4000)
    expect(fw.everNames.has('RUMOUR_THE_BOOK')).toBe(true)
    expect(fw.everNames.has('WHISPER_RUMOUR_THE_BOOK')).toBe(true)
  })

  // ---------------------------------------------------------------- S6
  test('S6 — deck reset cycles past exhaustion; defaultcard serves after exhaustion (and the non-stackable freeze nuance)', () => {
    // Mechanism: five recurring halfhour triggers draw two decks across day 0
    // (each slot trigger fires once per day — slots 1..5, minutes 30..150).
    // CYCLE (spec DAWN, DUSK; reset) wraps its seed-fixed permutation forever
    // — the draw order is a PERMUTATION, here [DUSK, DAWN], so draws go
    // DUSK, DAWN, then (3rd draw) wrap to DUSK again. FALLBACK (spec EMBER;
    // defaultcard ECHO) serves EMBER once, then ECHO forever (§9.42).
    //
    // Desired guarantee (stackable cards — no name collisions): DAWN, DUSK,
    // EMBER, ECHO all obtainable; recurring triggers give unbounded draw
    // capacity, so deep permutation positions AND the defaultcard are served.
    // Forward: slots 1..5 yield DUSK, DAWN, EMBER, ECHO(defaultcard), DUSK(wrap).
    //
    // Documented nuance (non-stackable control): the same content with plain
    // prototypes soft-locks — the slot-5 draw re-mints the still-living DUSK
    // and the deterministic throw rejects the whole advance, freezing the
    // world at minute 150. The analyzer's freeze line is coarser than the
    // truth (it caps every pre-freeze trigger draw site at ONE draw, and takes
    // the MAX over a deck's sites rather than the sum), so DAWN (permutation
    // position 1 needs 2 draws of CYCLE) and ECHO (the defaultcard needs 2
    // draws of FALLBACK) demote to unproven. Forward truth: all four cards DO
    // mint before the freeze — 'unproven' is weaker than truth, the sound
    // direction; and nothing is called 'unobtainable'.
    const mk = (stackable: boolean): ContentRaw => ({
      prototypes: {
        DAWN: { kind: 'item', ...(stackable ? { stackable: true } : {}) },
        DUSK: { kind: 'item', ...(stackable ? { stackable: true } : {}) },
        EMBER: { kind: 'item', ...(stackable ? { stackable: true } : {}) },
        ECHO: { kind: 'item', ...(stackable ? { stackable: true } : {}) },
      },
      decks: [
        { id: 'CYCLE', spec: ['DAWN', 'DUSK'], reset: true },
        { id: 'FALLBACK', spec: ['EMBER'], defaultcard: 'ECHO' },
      ],
      triggers: [
        { id: 'draw_c1', slot: 1, out: ['DRAW CYCLE'] },
        { id: 'draw_c2', slot: 2, out: ['DRAW CYCLE'] },
        { id: 'draw_f1', slot: 3, out: ['DRAW FALLBACK'] },
        { id: 'draw_f2', slot: 4, out: ['DRAW FALLBACK'] },
        { id: 'draw_c3', slot: 5, out: ['DRAW CYCLE'] },
      ],
      verbs: {
        WAIT: {
          slots: [{ role: 'actors', min: 1, max: 1, query: 'kind:character' }],
          duration: 30,
          recipes: [{ out: [] }],
        },
      },
    })

    // --- the clean (stackable) variant: full reachability, forward agreement
    const content = compile(mk(true))
    const state = stateWith(ent('ANA', 'character'))
    for (const name of ['DAWN', 'DUSK', 'EMBER', 'ECHO']) {
      const rep = analyzeCard(content, state, name)
      expect(rep.verdict, name).toBe('obtainable')
      expect(rep.chain.join('\n'), name).toContain('deck ')
    }
    const r = runner()
    let cur = state
    for (let i = 1; i <= 5; i++) cur = r.run(content, cur, `0:${i} @ ANA WAIT`).state
    const names = aliveNames(cur)
    expect(names).toContain('DAWN')
    expect(names).toContain('DUSK')
    expect(names).toContain('EMBER')
    expect(names).toContain('ECHO') // served by the defaultcard (draw 2 of FALLBACK)
    expect((cur.byName.get('DUSK') ?? []).length).toBe(2) // the wrap stacked a second DUSK
    const fw = explore(content, dict, state, 10, 4000)
    expect(fw.capped).toBe(false)
    for (const name of ['DAWN', 'DUSK', 'EMBER', 'ECHO']) expect(fw.everNames.has(name), name).toBe(true)

    // --- the non-stackable control: the freeze at the re-mint, and the
    // analyzer's coarser freeze line
    const plain = compile(mk(false))
    // every pre-freeze trigger draw site is capped at ONE draw (and the deck
    // takes the MAX over its sites), and every trigger at a cyclically-later
    // slot than a recurring minting trigger is dupRisk'd — so only DUSK (one
    // draw suffices) stays proven; DAWN, EMBER, ECHO demote to unproven
    expect(analyzeCard(plain, state, 'DUSK').verdict).toBe('obtainable')  // order[0]: one draw suffices
    expect(analyzeCard(plain, state, 'DAWN').verdict).toBe('unproven')    // needs 2 CYCLE draws
    expect(analyzeCard(plain, state, 'EMBER').verdict).toBe('unproven')   // its trigger is dupRisk'd
    expect(analyzeCard(plain, state, 'ECHO').verdict).toBe('unproven')    // needs 2 FALLBACK draws
    // forward truth (non-stackable): each wait's settle crosses the NEXT slot
    // too (the wait window spans it), so waits 1-3 mint DUSK, DAWN, EMBER, and
    // ECHO (the defaultcard, on FALLBACK's second draw at minute 120). The 4th
    // wait's window would span minute 150 (slot 5): the CYCLE wrap re-mints
    // the living DUSK — rejected, and the world is frozen there for good.
    const rp = runner()
    let pcur = state
    for (let i = 1; i <= 3; i++) pcur = rp.run(plain, pcur, `0:${i} @ ANA WAIT`).state
    expect(aliveNames(pcur)).toEqual(new Set(['ANA', 'DUSK', 'DAWN', 'EMBER', 'ECHO']))
    const boom = rp.advance(plain, pcur, '0:5') // crosses slot 5: DRAW CYCLE again
    expect(boom.rejected).not.toBeNull()
    expect(boom.rejected).toContain('DUSK')
    // the analyzer's unproven claims were conservative: the cards DID appear
    const fwPlain = explore(plain, dict, state, 8, 4000)
    for (const name of ['DAWN', 'DUSK', 'EMBER', 'ECHO']) expect(fwPlain.everNames.has(name), name).toBe(true)
  })

  // ---------------------------------------------------------------- S7
  test('S7 — kind-override + kind-stackable mint + demand `kind:X` matching the overridden descriptor', () => {
    // Mechanism: `SPAWN rite:RELIC` (§9.38/B5) OVERRIDES the minted kind —
    // kind: is not a lookup, RELIC's authored kind 'item' is ignored. The KIND
    // 'rite' carries a stackable declaration (via RITE_PROTO), so
    // mintIsStackable resolves (name RELIC, kind rite) → the mint STACKS even
    // though the prototype is not declared stackable.
    //
    // Desired guarantee: semantics (c) demand 'kind:rite' is proven by the
    // overridden MINT descriptor (not the bare prototype — reach.ts's demand
    // loop synth-esizes with site.kindOverride); plain 'RELIC' obtainable;
    // RITE_PROTO itself unobtainable (declared only to make the kind stackable
    // — no site mints it). Forward: two CONSECRATEs stack two kind:rite RELICs.
    const content = compile({
      prototypes: {
        RELIC: { kind: 'item' },
        RITE_PROTO: { kind: 'rite', stackable: true },
      },
      verbs: {
        CONSECRATE: {
          slots: [{ role: 'actors', min: 1, max: 1, query: 'kind:character' }],
          duration: 0,
          recipes: [{ out: ['SPAWN rite:RELIC'] }],
        },
      },
    })
    const state = stateWith(ent('ANA', 'character'))

    const demand = analyzeCard(content, state, '', { demand: 'kind:rite' })
    expect(demand.verdict).toBe('obtainable')
    expect(demand.chain.join('\n')).toContain('kind overridden')
    expect(analyzeCard(content, state, 'RELIC').verdict).toBe('obtainable')
    expect(analyzeCard(content, state, 'RITE_PROTO').verdict).toBe('unobtainable')

    // forward truth: two commits stack; every copy carries the overridden kind
    const r = runner()
    let cur = r.run(content, state, '0:0 @ ANA CONSECRATE').state
    cur = r.run(content, cur, '0:0 @ ANA CONSECRATE').state
    const pool = cur.byName.get('RELIC')!
    expect(pool).toHaveLength(2)
    for (const id of pool) {
      const e = cur.entities.get(id)!
      expect(e.kind).toBe('rite')
      expect(e.stackable).toBe(true)
    }
    const fw = explore(content, dict, state, 8, 2000)
    expect(fw.everDesc.some(e => e.name === 'RELIC' && e.kind === 'rite')).toBe(true)
  })

  // ---------------------------------------------------------------- S8
  test('S8 — same-slot TRIPLE triggers with overlapping mint names: the shared mint dooms the whole event pass, in both declaration orders', () => {
    // Mechanism: three slot triggers share slot 5 and each mints P_TRIPLE plus
    // a unique card. At the first crossing they fire in DECLARATION order
    // (§9.16): ta fully applies, then tb's SPAWN P_TRIPLE throws (`name already
    // in use`) — fireSlot's throw rejects the ENTIRE advance (§9.19 atomicity:
    // the pass is discarded, nothing from ANY sibling persists), and since the
    // refusal is deterministic the world can never cross slot 5 at all.
    //
    // Desired guarantee: the analyzer must NOT claim any of the four names
    // obtainable — its same-slot pair rule marks every co-slot trigger risky
    // (a shared non-stackable mint collides on the very first crossing) and a
    // risky trigger kills the whole EVENT PASS, so every sibling's mints demote
    // (fuzz seed 226's soft-lock, generalized to three). Both declaration
    // orders must agree. Forward: nothing at slot 5 ever persists.
    const mk = (order: 'abc' | 'cba'): ContentRaw => {
      const ta = { id: 'ta', slot: 5, out: ['SPAWN P_TRIPLE', 'SPAWN NAME_A'] }
      const tb = { id: 'tb', slot: 5, out: ['SPAWN P_TRIPLE', 'SPAWN NAME_B'] }
      const tc = { id: 'tc', slot: 5, out: ['SPAWN P_TRIPLE', 'SPAWN NAME_C'] }
      return {
        prototypes: {
          P_TRIPLE: { kind: 'item' },
          NAME_A: { kind: 'idea' },
          NAME_B: { kind: 'idea' },
          NAME_C: { kind: 'idea' },
        },
        triggers: order === 'abc' ? [ta, tb, tc] : [tc, tb, ta],
        verbs: {
          WAIT: {
            slots: [{ role: 'actors', min: 1, max: 1, query: 'kind:character' }],
            duration: 30,
            recipes: [{ out: [] }],
          },
        },
      }
    }
    const state = stateWith(ent('ANA', 'character'))

    for (const order of ['abc', 'cba'] as const) {
      const content = compile(mk(order))
      for (const name of ['P_TRIPLE', 'NAME_A', 'NAME_B', 'NAME_C']) {
        const rep = analyzeCard(content, state, name)
        expect(rep.verdict, `${order}:${name}`).not.toBe('obtainable')
        expect(rep.verdict, `${order}:${name}`).toBe('unproven')
        expect(rep.existence, `${order}:${name}`).toBe('candidate')
      }
    }

    // control: the same triple WITHOUT the shared mint — three triggers on one
    // slot with DISTINCT non-stackable mints all apply in declaration order
    // (§9.16) and all three cards obtainable, forward-present. The demotion
    // above is attributable to the OVERLAP, not to same-slot coexistence.
    const distinct = compile({
      prototypes: { NAME_A: { kind: 'idea' }, NAME_B: { kind: 'idea' }, NAME_C: { kind: 'idea' } },
      triggers: [
        { id: 'ta', slot: 5, out: ['SPAWN NAME_A'] },
        { id: 'tb', slot: 5, out: ['SPAWN NAME_B'] },
        { id: 'tc', slot: 5, out: ['SPAWN NAME_C'] },
      ],
      verbs: mk('abc').verbs,
    })
    for (const name of ['NAME_A', 'NAME_B', 'NAME_C']) {
      expect(analyzeCard(distinct, state, name).verdict, name).toBe('obtainable')
    }
    const fwDistinct = explore(distinct, dict, state, 8, 2000)
    for (const name of ['NAME_A', 'NAME_B', 'NAME_C']) expect(fwDistinct.everNames.has(name), name).toBe(true)

    // forward truth (declaration order abc): the first crossing of slot 5
    // rejects the whole advance — nothing mints, ever
    const content = compile(mk('abc'))
    const r = runner()
    const early = r.advance(content, state, '0:4') // slot 4: nothing fires yet
    expect(early.rejected).toBeNull()
    expect(aliveNames(early.state)).toEqual(new Set(['ANA']))
    const boom = r.advance(content, early.state, '0:5') // crosses slot 5
    expect(boom.rejected).not.toBeNull()
    expect(boom.rejected).toContain('P_TRIPLE')
    expect(aliveNames(boom.state)).toEqual(new Set(['ANA']))
    // and the freeze is permanent: retrying can never pass the slot
    const again = r.advance(content, state, '0:6')
    expect(again.rejected).not.toBeNull()
    const fw = explore(content, dict, state, 8, 2000)
    expect(fw.everNames.has('P_TRIPLE')).toBe(false)
    expect(fw.everNames.has('NAME_A')).toBe(false)
    expect(fw.everNames.has('NAME_B')).toBe(false)
    expect(fw.everNames.has('NAME_C')).toBe(false)
  })

  // ---------------------------------------------------------------- S9
  test('S9 — gate-death: recipe gated `exists(ROPE)` while a trigger destroys ROPE — obtainable before the kill, dead after', () => {
    // Mechanism: RITUAL is committable only while its ONLY recipe's guard
    // `exists(ROPE)` selects (§6.4 — selection is a commit-time snapshot). The
    // trigger rope_snaps destroys ROPE at slot 2 (minute 60), so commits before
    // minute 60 mint GOAL and every commit after finds no matching recipe
    // ("no recipe matched" — rejected at the table).
    //
    // Desired guarantee: reach.ts's gate-durability rule (fuzz seed 6028) — a
    // static gate on exists(X) is only as durable as X's aliveness, and ROPE
    // has a killer — demotes the site to candidate: GOAL must be 'unproven',
    // never 'obtainable' (the gate is mortal) and never 'unobtainable'
    // (existentially obtainable before the kill). Forward truth: GOAL DOES
    // appear (committed at 0:0), and the post-kill commit is rejected — the
    // analyzer's 'unproven' is weaker than the existential truth, which is the
    // sound direction.
    const content = compile({
      prototypes: { GOAL: { kind: 'item' } },
      verbs: {
        RITUAL: {
          slots: [{ role: 'actors', min: 1, max: 1, query: 'kind:character' }],
          duration: 0,
          recipes: [{ when: 'exists(ROPE)', out: ['SPAWN GOAL'] }],
        },
      },
      triggers: [{ id: 'rope_snaps', slot: 2, out: ['DESTROY ROPE'] }],
    })
    const state = stateWith(ent('ANA', 'character'), ent('ROPE', 'item'))

    const goal = analyzeCard(content, state, 'GOAL')
    expect(goal.verdict).toBe('unproven')
    expect(goal.existence).toBe('candidate')
    expect(goal.notes.join(' ')).toContain('candidate source')

    // forward truth: mint GOAL before the kill; the kill; then the gate is false
    const r = runner()
    let cur = r.run(content, state, '0:0 @ ANA RITUAL').state
    expect(aliveNames(cur)).toContain('GOAL')
    cur = r.advance(content, cur, '0:2').state
    expect(aliveNames(cur)).not.toContain('ROPE')
    const after = settleStep(content, dict, cur, parseSentence('0:2 @ ANA RITUAL', dict), 700)
    expect(after.rejected).not.toBeNull()
    expect(after.rejected).toContain('no recipe matched')

    const fw = explore(content, dict, state, 8, 2000)
    expect(fw.everNames.has('GOAL')).toBe(true)   // existential obtainability — forward agrees
    expect(fw.everNames.has('ROPE')).toBe(true)   // initial, then killed mid-play
  })

  // ---------------------------------------------------------------- S10
  test('S10 — self-filler collision: the recipe\u2019s own mint fills a required slot and collides post-bootstrap (selfGated)', () => {
    // Mechanism: MAKER mints X (proven). SELFMADE's recipe mints X AND GOAL,
    // while SELFMADE's `with` slot (min 1, kind:item) can only ever bind X (the
    // only item the world can hold). So every SELFMADE commit binds the very
    // card its own completion re-mints → `SPAWN: name 'X' already in use` → a
    // completion refusal discards the WHOLE draft — GOAL can never survive a
    // step. There is no bootstrap commit (the with slot needs X alive first).
    //
    // Desired guarantee: reach.ts's selfGated rule — "one of the list's own
    // mints is REQUIRED ALIVE as a slot filler (post-bootstrap the filler IS
    // the earlier mint)" — is the ONLY demotion that fires here (no dupRisk:
    // X is minted once per list and not initial; no throwing ops; no shadow),
    // so GOAL must be 'unproven', never 'obtainable'. Forward truth: GOAL
    // never appears — the refusal is deterministic from the first commit.
    const content = compile({
      prototypes: { X: { kind: 'item' }, GOAL: { kind: 'idea' } },
      verbs: {
        MAKER: {
          slots: [{ role: 'actors', min: 1, max: 1, query: 'kind:character' }],
          duration: 0,
          recipes: [{ out: ['SPAWN X'] }],
        },
        SELFMADE: {
          slots: [
            { role: 'actors', min: 1, max: 1, query: 'kind:character' },
            { role: 'with', min: 1, max: 1, query: 'kind:item' },
          ],
          duration: 0,
          recipes: [{ out: ['SPAWN X', 'SPAWN GOAL'] }],
        },
      },
    })
    const state = stateWith(ent('ANA', 'character'))

    expect(analyzeCard(content, state, 'X').verdict).toBe('obtainable') // via MAKER
    const goal = analyzeCard(content, state, 'GOAL')
    expect(goal.verdict).toBe('unproven')
    expect(goal.existence).toBe('candidate')
    expect(goal.notes.join(' ')).toContain('candidate source')

    // forward truth: MAKER mints X; every SELFMADE completion then refuses
    const r = runner()
    let cur = r.run(content, state, '0:0 @ ANA MAKER').state
    expect(aliveNames(cur)).toContain('X')
    const attempt = settleStep(content, dict, cur, parseSentence('0:0 @ ANA SELFMADE WITH X', dict), 600)
    expect(attempt.rejected).toBeNull()                 // a refusal is not a rejection…
    expect(attempt.records.some(rec => rec.outcomes.some(o => o.op === 'REFUSE'))).toBe(true)
    expect(aliveNames(attempt.state)).not.toContain('GOAL') // …but nothing survived
    expect(aliveNames(attempt.state)).toContain('X')        // the first mint stands

    const fw = explore(content, dict, state, 10, 4000)
    expect(fw.everNames.has('X')).toBe(true)
    expect(fw.everNames.has('GOAL')).toBe(false) // the analyzer's 'not obtainable' held

    // control (the rule's precision): with the with-slot OPTIONAL the bootstrap
    // EXISTS — commit SELFMADE before any X is alive; the completion's SPAWN X
    // then mints cleanly and GOAL follows. selfGated's filler branch skips
    // min:0 slots, so the analyzer flips to obtainable — and the forward
    // exploration VINDICATES it. The demotion fires exactly when the slot
    // requirement blocks the bootstrap.
    const optional = compile({
      prototypes: { X: { kind: 'item' }, GOAL: { kind: 'idea' } },
      verbs: {
        MAKER: {
          slots: [{ role: 'actors', min: 1, max: 1, query: 'kind:character' }],
          duration: 0,
          recipes: [{ out: ['SPAWN X'] }],
        },
        SELFMADE: {
          slots: [
            { role: 'actors', min: 1, max: 1, query: 'kind:character' },
            { role: 'with', min: 0, max: 1, query: 'kind:item' },
          ],
          duration: 0,
          recipes: [{ out: ['SPAWN X', 'SPAWN GOAL'] }],
        },
      },
    })
    expect(analyzeCard(optional, state, 'GOAL').verdict).toBe('obtainable')
    const fwOpt = explore(optional, dict, state, 10, 4000)
    expect(fwOpt.everNames.has('GOAL')).toBe(true)
  })

  // ---------------------------------------------------------------- S11
  test('S11 — multi-clause commit: two clauses in one step, each completing its own recipe mint', () => {
    // Mechanism: one step, two clauses (§6.6/§6.7): BEGIN registers one window
    // per clause with SERIAL offsets (clause ci starts at begin + Σ earlier
    // durations), so `FORGE_LEFT (30m); FORGE_RIGHT (60m)` at 0:0 opens [0,30]
    // and [30,90]; each completion applies its own recipe → LEFT at 30, RIGHT
    // at 90. Actors are never locked, so one character drives both clauses.
    //
    // Desired guarantee: both mints obtainable (each recipe is independently
    // backed by a committable verb — nothing about co-clause commits can demote
    // them), and forward confirms the staged completions.
    const content = compile({
      prototypes: { LEFT: { kind: 'item' }, RIGHT: { kind: 'item' } },
      verbs: {
        FORGE_LEFT: {
          slots: [{ role: 'actors', min: 1, max: 1, query: 'kind:character' }],
          duration: 30,
          recipes: [{ out: ['SPAWN LEFT'] }],
        },
        FORGE_RIGHT: {
          slots: [{ role: 'actors', min: 1, max: 1, query: 'kind:character' }],
          duration: 60,
          recipes: [{ out: ['SPAWN RIGHT'] }],
        },
        WAIT: {
          slots: [{ role: 'actors', min: 1, max: 1, query: 'kind:character' }],
          duration: 30,
          recipes: [{ out: [] }],
        },
      },
    })
    const state = stateWith(ent('ANA', 'character'))

    expect(analyzeCard(content, state, 'LEFT').verdict).toBe('obtainable')
    expect(analyzeCard(content, state, 'RIGHT').verdict).toBe('obtainable')

    // forward truth: the multi-clause commit, then staged completions. The
    // BEGIN geometry and the intermediate advance go through executeStep
    // (settleStep would close every window it is handed).
    const parsed = parseSentence('0:0 @ ANA FORGE_LEFT; ANA FORGE_RIGHT', dict)
    const begin = executeStep(content, dict, state, { clock: parsed.clock ?? { day: 0, halfhour: 0 }, clauses: parsed.clauses }, { firstRecordId: 1 })
    expect(begin.rejected).toBeNull()
    const windows = [...begin.state.windows.values()].sort((a, b) => a.start - b.start)
    expect(windows.map(w => [w.verb, w.start, w.end])).toEqual([['FORGE_LEFT', 0, 30], ['FORGE_RIGHT', 30, 90]])
    const mid = executeStep(content, dict, begin.state, { clock: parseClock('0:1'), clauses: [] }, { firstRecordId: 2 })
    expect(mid.rejected).toBeNull()
    expect(aliveNames(mid.state)).toContain('LEFT')
    expect(aliveNames(mid.state)).not.toContain('RIGHT') // its window is still open
    const r = runner()
    let cur = r.advance(content, mid.state, '0:3').state
    expect(aliveNames(cur)).toContain('RIGHT')

    const fw = explore(content, dict, state, 8, 4000)
    expect(fw.everNames.has('LEFT')).toBe(true)
    expect(fw.everNames.has('RIGHT')).toBe(true)
  })

  // ---------------------------------------------------------------- S12
  test('S12 — a world-static FALSE `when: exists(GHOST)` recipe must NOT have its mints claimed obtainable', () => {
    // Mechanism: SEANCE's only recipe is gated `exists(GHOST)`; GHOST has no
    // declaration and no producer anywhere in the content, so the guard is
    // world-static false — recipe selection never matches, the commit is
    // rejected at the table ("no recipe matched"), and PHANTOM can never mint.
    //
    // Desired guarantee: the analyzer must NOT claim 'obtainable' — the guard
    // is static and false against the known world (evalStatic), so the site is
    // not provable; and it must not claim 'unobtainable' either unless the
    // site is STRUCTURALLY absent, which it is not (the recipe exists, it is
    // merely gated) → the honest verdict is 'unproven'/candidate. Forward
    // truth: PHANTOM never appears.
    const content = compile({
      prototypes: { PHANTOM: { kind: 'idea' } },
      verbs: {
        SEANCE: {
          slots: [{ role: 'actors', min: 1, max: 1, query: 'kind:character' }],
          duration: 0,
          recipes: [{ when: 'exists(GHOST)', out: ['SPAWN PHANTOM'] }],
        },
      },
    })
    const state = stateWith(ent('ANA', 'character'))

    const phantom = analyzeCard(content, state, 'PHANTOM')
    expect(phantom.verdict).toBe('unproven')
    expect(phantom.existence).toBe('candidate')

    // forward truth: the gated commit is rejected at the table, forever
    const attempt = settleStep(content, dict, state, parseSentence('0:0 @ ANA SEANCE', dict), 500)
    expect(attempt.rejected).not.toBeNull()
    expect(attempt.rejected).toContain('no recipe matched')
    const fw = explore(content, dict, state, 8, 2000)
    expect(fw.everNames.has('PHANTOM')).toBe(false)
    expect(fw.everNames.has('GHOST')).toBe(false)
  })

  // ---------------------------------------------------------------- S13
  test('S13 — roll-guarded mint whose dice counter is frozen by a deterministic refusal: the escape roll never comes (analyzer stays unproven)', () => {
    // Mechanism: GAMBLE rolls 1d6 and mints PRIZE only on roll >= 4; the ELSE
    // arm REFUSES. §9.41 + law 4: the counter advances ON THE DRAFT, and a
    // refusal discards the draft — so a refused attempt leaves State.rng
    // UNMOVED, and the deterministic die sequence re-derives the SAME value at
    // counter 0 on every retry (A4: counters burn on the draft). With a seed
    // whose first die is low, the refusal is DETERMINISTIC and the escape roll
    // never comes: PRIZE can never mint. With a high first die, the very first
    // commit mints PRIZE.
    //
    // Desired guarantee: the analyzer cannot evaluate roll — the mint arm is
    // runtime-gated (path not static) → 'unproven' for BOTH seeds: never
    // 'obtainable' (it cannot prove the roll), never 'unobtainable' (a lucky
    // seed mints). Forward truth: the low seed NEVER mints (the counter-freeze
    // makes the low roll a fixed point); the high seed mints on commit one.
    const mk = (seed: number): ContentRaw => ({
      seed,
      prototypes: { PRIZE: { kind: 'item' } },
      verbs: {
        GAMBLE: {
          slots: [{ role: 'actors', min: 1, max: 1, query: 'kind:character' }],
          duration: 0,
          recipes: [{ out: ['ROLL(1, 6), IF roll >= 4 -> (SPAWN PRIZE) ELSE -> (REFUSE("frozen — the house always wins"))'] }],
        },
      },
    })
    const lowSeed = (() => { for (let s = 0; ; s++) if (rollDie(s, 0, 6) < 4) return s })()
    const highSeed = (() => { for (let s = 0; ; s++) if (rollDie(s, 0, 6) >= 4) return s })()
    const state = stateWith(ent('ANA', 'character'))

    expect(rollDie(lowSeed, 0, 6)).toBeLessThan(4)
    expect(rollDie(highSeed, 0, 6)).toBeGreaterThanOrEqual(4)
    for (const seed of [lowSeed, highSeed]) {
      const rep = analyzeCard(compile(mk(seed)), state, 'PRIZE')
      expect(rep.verdict, `seed ${seed}`).toBe('unproven')
      expect(rep.existence, `seed ${seed}`).toBe('candidate')
    }

    // forward truth, low seed: the refusal is a fixed point — the counter never
    // moves, so the same low value re-derives forever; PRIZE never mints
    const low = compile(mk(lowSeed))
    const r = runner()
    let cur = state
    for (let i = 0; i < 3; i++) {
      const attempt = settleStep(low, dict, cur, parseSentence('0:0 @ ANA GAMBLE', dict), 400 + i)
      expect(attempt.rejected).toBeNull()                 // refused, not rejected…
      expect(attempt.state.rng).toBe(0)                   // …and the counter never moved
      expect(aliveNames(attempt.state)).not.toContain('PRIZE')
      cur = attempt.state
    }
    const fwLow = explore(low, dict, state, 8, 2000)
    expect(fwLow.everNames.has('PRIZE')).toBe(false)

    // forward truth, high seed: the first commit already escapes the guard
    const high = compile(mk(highSeed))
    const win = settleStep(high, dict, state, parseSentence('0:0 @ ANA GAMBLE', dict), 399)
    expect(win.rejected).toBeNull()
    expect(aliveNames(win.state)).toContain('PRIZE')
    expect(win.state.rng).toBe(1)
    const fwHigh = explore(high, dict, state, 8, 2000)
    expect(fwHigh.everNames.has('PRIZE')).toBe(true)
  })
})
