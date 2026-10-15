// W4 golden generation — plan §4-W4 / PR2-09 / PR3-08 / PR3-10. READ-ONLY
// ADDITION (§7 rule 10: the one sanctioned TS-side change): generates the
// cross-language golden library under rust/tests/goldens/ for the Rust replay
// harness (rust/tests/golden.rs). Never mutates src/, test/, scripts/.
//
//   npx tsx tools/golden-dump.ts                       # regenerate all goldens
//   npx tsx tools/golden-dump.ts --out <dir>           # different target dir
//   npx tsx tools/golden-dump.ts --fold-prefix <golden> <n>
//   npx tsx tools/golden-dump.ts --fold-prefix-file <log.jsonl> <golden> <n>
//
// The fold-prefix modes are the §6.2 bisection step 2 on the TS side: fold a
// log prefix over a golden's base state and print the snap string, for the
// TS/Rust snap对拍 that splits a divergence into fold vs play.
//
// Families (§4-W4's 8 items):
//   1. nightjar-main        — Game.fromScript + WIN_PATH handleLine, seed 1899
//   2. walk-20..200         — the deterministic sampled walk (explorer-lib.ts:275)
//   3. s01..s14             — the adversarial scenarios; each builder FIRST
//                             recomputes the original reach-adversarial.test.ts
//                             expect assertions (analyzer + engine + explore,
//                             the PR1-02/PR3-08 gate); any failure aborts the
//                             dump before a byte is written
//   4. anchor-*             — the engine ordering anchors (PR1-01); rigs mirror
//                             rust/tests/engine_ordering.rs test-for-test so its
//                             HAND placeholders can be replaced by dump-measured
//                             values
//   5. suggestions-nightjar — Game.suggestions() full lists along the win path
//                             (§3.21: pins the localeCompare approximator)
//   6. fuzz-001..200        — genScenario(seed) × K=25 settleSteps under the
//                             fixed LCG action selector (its source ships as
//                             action-selector.ts — the fact-frozen surface)
//   7. (fold-prefix         — the CLI modes above, not a golden family)
//   8. manifest.json        — { name, kind, driver, sha256(jsonl), snap, … }
//        + demo             — examples/demo.ts journal/prose goldens (W6's diff
//                             target, generated here per §4-W6)
//
// Every JSONL is `records.map(JSON.stringify).join('\n') + '\n'` (the TS
// saveLog shape — ALWAYS \n-terminated, so a 0-record golden is a single
// newline byte); the Rust harness reproduces exactly this shape.
import { createHash } from 'node:crypto'
import { mkdirSync, readFileSync, writeFileSync } from 'node:fs'
import { dirname, join } from 'node:path'
import { fileURLToPath } from 'node:url'

import { compileContent, type CompiledContent, type ContentRaw } from '../src/content/content.js'
import { coreDictionary, type Dictionary } from '../src/core/dict.js'
import { rollDie } from '../src/core/dice.js'
import { absMin, clockFromAbsMin, formatClock, nextStepClock, parseClock, type Clock } from '../src/core/clock.js'
import { parseSentence } from '../src/core/parse.js'
import { matchQuery } from '../src/core/query.js'
import { executeStep, type ActionRecord, type ExecResult, type Step } from '../src/engine/engine.js'
import { fold } from '../src/engine/fold.js'
import { analyzeCard } from '../src/reach/reach.js'
import { journal as journalOf, prose } from '../src/ui/render.js'
import { Game } from '../src/ui/play.js'
import { addEntity, cloneState, newState, type Entity, type State } from '../src/core/types.js'
import { idRef, nameRef, type Ref } from '../src/core/ref.js'
import { emptyClause, type Clause, type Role } from '../src/core/sentence.js'

import { settleStep } from '../test/settle.js'
import { explore, genScenario, rng } from '../test/reach-fuzz-lib.js'
import { snap } from '../test/snap.js'
import { seedContent, seedState } from '../test/seed.js'
import {
  PLAYER,
  WIN_PATH,
  base as nightjarBase,
  candidatesFor,
  content as nightjarContent,
  dict as nightjarDict,
  scriptText,
} from '../test/explorer-lib.js'

// ---------------------------------------------------------------------------
// plumbing
// ---------------------------------------------------------------------------

/** the per-golden world its fold-prefix mode folds against */
interface World {
  dict: Dictionary
  content: CompiledContent
  base: State
}

interface GoldenSpec {
  name: string
  kind: 'script' | 'scenario' | 'anchor' | 'walk' | 'fuzz' | 'demo'
  driver: Record<string, unknown>
  world: World
  records: ActionRecord[]
  snap: string
  journal?: string
  prose?: string
  steps?: string
  suggestions?: string
}

const fail = (msg: string): never => {
  throw new Error(`golden-dump: ${msg}`)
}
/** the PR1-02/PR3-08 recomputation gate — one failed check aborts the run */
const check = (cond: unknown, msg: string): void => {
  if (!cond) fail(`assertion failed: ${msg}`)
}

const setsEq = (a: Set<string>, b: Set<string>): boolean =>
  a.size === b.size && [...a].every(x => b.has(x))

/** ts: store.ts:24-27 — ALWAYS \n-terminated (an empty log is one newline) */
const jsonlOf = (records: ActionRecord[]): string =>
  records.map(r => JSON.stringify(r)).join('\n') + '\n'

const sha256Of = (bytes: string): string => createHash('sha256').update(bytes, 'utf8').digest('hex')

// ---------------------------------------------------------------------------
// the fuzz action selector — THE FACT-FROZEN SURFACE (review-1 appendix 4(a)).
// This exact source text is copied to <out>/action-selector.ts and must be
// ported verbatim to rust/tests/common/reach_fuzz_lib.rs (`choose_step`); the
// caller discipline (K applied steps, the attempt cap, one selector draw per
// attempt, rejected attempts consume the draw and nothing else) is frozen with
// it.
// ---------------------------------------------------------------------------

/** ts: reach-fuzz-lib.ts:39 (`type Rand = () => number`) — local alias, the
 *  original is module-private */
type Rand = () => number

/** pick one candidate step: a single uniform draw over the candidate list
 *  (never empty — the waits are always offered). */
const chooseStep = <T>(r: Rand, steps: T[]): T => steps[Math.floor(r() * steps.length)]!

const SELECTOR_SOURCE = `// golden-dump artifact — the fuzz action selector, frozen verbatim from
// tools/golden-dump.ts (review-1 appendix 4(a): the selector source ships with
// the goldens so the Rust mirror can be diffed against it letter by letter).
// Ported 1:1 to rust/tests/common/reach_fuzz_lib.rs as choose_step; the LCG is
// js::Lcg / tests/common/reach_fuzz_lib.rs (same discipline as
// reach-fuzz-lib.ts rng — XOR 0x9e3779b9 seeding, u32 wrapping multiply-add).
// Caller discipline (frozen): one selector instance per seed seeded rng(seed);
// per attempt draw once over the full candidate list; a rejected settleStep
// consumes the draw and retries; K=25 applied steps or the 400-attempt cap.
export function chooseStep<T>(rand: () => number, steps: T[]): T {
  return steps[Math.floor(rand() * steps.length)]!
}
`

// ---------------------------------------------------------------------------
// shared rig helpers — exact mirrors of rust/tests/engine_ordering.rs
// ---------------------------------------------------------------------------

/** ts: engine_ordering.rs seed() — house/ana/bertha/book, bornAction null */
const orderingSeed = (): State => {
  const s = newState(0)
  const card = (id: string, kind: string, name: string, attrs: Record<string, unknown>): Entity => ({
    id,
    kind,
    name,
    aspects: {},
    attrs: attrs as Entity['attrs'],
    bornAction: null,
    deadAction: null,
    stackable: false,
  })
  addEntity(s, card('house', 'place', 'HOUSE', {}))
  addEntity(s, card('ana', 'character', 'ANA', { container: 'house' }))
  addEntity(s, card('bertha', 'character', 'BERTHA', { container: 'house' }))
  addEntity(s, card('book', 'text', 'MAGICK_BOOK', { container: 'house' }))
  return s
}

/** ts: engine_ordering.rs expiring_item() — the §9.36 materialized form */
const expiringItem = (s: State, id: string, name: string, at: number): void => {
  addEntity(s, {
    id,
    kind: 'item',
    name,
    aspects: {},
    attrs: { container: 'house', expiresAt: at } as Entity['attrs'],
    bornAction: null,
    deadAction: null,
    stackable: false,
  })
}

interface RawSlot {
  role: string
  min: number
  max: number
  query?: string
}
const actorSlot = (): RawSlot => ({ role: 'actors', min: 1, max: 1, query: 'kind:character' })
const objectSlot = (query?: string): RawSlot => ({ role: 'object', min: 0, max: 1, ...(query ? { query } : {}) })

const verbOf = (
  name: string,
  slots: RawSlot[],
  duration: number,
  out: string[],
): [string, { slots: RawSlot[]; duration: number; recipes: { out: string[] }[] }] => [
  name,
  { slots, duration, recipes: [{ out }] },
]

/** ts: engine_ordering.rs rig() — fresh Dictionary per anchor (TEST-10) */
const rig = (
  verbs: [string, unknown][],
  extra: (raw: ContentRaw) => void = () => {},
): { dict: Dictionary; content: CompiledContent } => {
  const dict = coreDictionary()
  const map: Record<string, unknown> = {}
  for (const [name, v] of verbs) {
    if (!dict.isVerb(name)) dict.registerVerb(name)
    map[name] = v
  }
  const raw: ContentRaw = { verbs: map as ContentRaw['verbs'] }
  extra(raw)
  const content = compileContent(raw, dict)
  return { dict, content }
}

const stepOf = (clock: Clock | null | undefined, clauses: Clause[]): Step => ({
  startMin: undefined,
  clock: clock ?? { day: 0, halfhour: 0 },
  clauses,
  tReal: undefined,
})

/** ts: engine_ordering.rs run() — parse + executeStep at the sentence clock */
const runStep = (
  content: CompiledContent,
  dict: Dictionary,
  state: State,
  src: string,
  first: number,
): ExecResult => {
  const s = parseSentence(src, dict)
  return executeStep(content, dict, state, stepOf(s.clock, s.clauses), { firstRecordId: first })
}

/** ts: engine_ordering.rs run_clock() — a lone clock token is a pure advance */
const runClock = (
  content: CompiledContent,
  dict: Dictionary,
  state: State,
  clock: string,
  first: number,
): ExecResult =>
  executeStep(content, dict, state, { startMin: undefined, clock: parseClock(clock) as Clock, clauses: [], tReal: undefined }, { firstRecordId: first })

/** steps sidecar — one line per driver call; rejection texts ride along so the
 *  Rust replay pins them byte-for-byte (rejections never reach the JSONL) */
class StepsLog {
  private lines: string[] = []
  ok(r: ExecResult): ExecResult {
    this.lines.push(`ok ${r.records.length}`)
    return r
  }
  rejected(r: ExecResult): ExecResult {
    check(r.rejected !== null, 'expected a rejected result')
    this.lines.push(`rejected ${r.rejected}`)
    return r
  }
  raw(line: string): void {
    this.lines.push(line)
  }
  text(): string {
    return this.lines.join('\n') + '\n'
  }
}

// ---------------------------------------------------------------------------
// family 1 + 5 — nightjar main anchor + suggestions snapshot (PR2-02)
// ---------------------------------------------------------------------------

const nightjarWorld = (): World => ({ dict: nightjarDict, content: nightjarContent, base: nightjarBase })

/** ts: nightjar.test.ts:18-32 (`fresh`) — the auto-settle adapter: after a
 *  committed action with open windows, the settle rides the SAME turn. The
 *  win path only commits under this discipline (the clue mints land at the
 *  completions). The Rust mirror is rust/tests/nightjar.rs `auto_settle_line`
 *  and tests/golden.rs' nightjar drivers. */
const autoSettleLine = (game: Game, line: string): { kind: string; settled: boolean } => {
  const t = game.handleLine(line)
  if (t.kind !== 'action') return { kind: t.kind, settled: false }
  if (game.state.windows.size > 0) {
    game.wait()
    return { kind: t.kind, settled: true }
  }
  return { kind: t.kind, settled: false }
}

const nightjarFamily = (): GoldenSpec[] => {
  // -- nightjar-main ---------------------------------------------------------
  const game = Game.fromScript(scriptText, coreDictionary())
  const slog: string[] = []
  for (const line of WIN_PATH) {
    const t = autoSettleLine(game, line)
    check(t.kind === 'action', `win path line '${line}' did not commit: ${t.kind}`)
    slog.push(t.settled ? 'action+wait' : 'action')
  }
  const specs: GoldenSpec[] = [
    {
      name: 'nightjar-main',
      kind: 'script',
      driver: {
        type: 'game',
        script: 'scripts/the-nightjar-flyer.txt',
        seed: 1899,
        lines: [...WIN_PATH],
      },
      world: nightjarWorld(),
      records: [...game.log.records],
      snap: snap(game.state),
      journal: game.journal()[0] ?? '',
      steps: slog.join('\n') + '\n',
    },
  ]

  // -- nightjar-dream (§4-W4 acceptance: the DREAMS-deck anchors, ts:
  //    nightjar.test.ts:340-353 — VOID, PORTENT, PORTENT then the exhausted
  //    deck's completion refusal) --------------------------------------------
  const g3 = Game.fromScript(scriptText, coreDictionary())
  const slog3: string[] = []
  const dreamLines = ['wait', 'explore the firebox', 'read the wax cylinder', 'read the vane ledger']
  for (const line of [...dreamLines, 'dream with the aether seal', 'dream with the aether seal', 'dream with the aether seal', 'dream with the aether seal']) {
    const t = autoSettleLine(g3, line)
    check(t.kind === 'action', `dream path line '${line}' did not commit: ${t.kind}`)
    slog3.push(t.settled ? 'action+wait' : 'action')
  }
  check((g3.state.byName.get('PORTENT') ?? []).length === 2, 'nightjar-dream: PORTENT×2')
  check((g3.state.byName.get('VOID') ?? []).length === 1, 'nightjar-dream: VOID×1')
  check(g3.journal()[0]!.includes('exhausted'), 'nightjar-dream: the exhausted deck refuses')
  specs.push({
    name: 'nightjar-dream',
    kind: 'script',
    driver: {
      type: 'game',
      script: 'scripts/the-nightjar-flyer.txt',
      seed: 1899,
      lines: [...dreamLines, 'dream with the aether seal', 'dream with the aether seal', 'dream with the aether seal', 'dream with the aether seal'],
    },
    world: nightjarWorld(),
    records: [...g3.log.records],
    snap: snap(g3.state),
    journal: g3.journal()[0] ?? '',
    steps: slog3.join('\n') + '\n',
  })

  // -- suggestions-nightjar (§3.21: pins the localeCompare approximator) -----
  const g2 = Game.fromScript(scriptText, coreDictionary())
  const sugg: string[] = []
  sugg.push('[start]')
  sugg.push(...g2.suggestions())
  sugg.push('')
  for (const line of WIN_PATH) {
    autoSettleLine(g2, line)
    sugg.push(`[after ${line}]`)
    sugg.push(...g2.suggestions())
    sugg.push('')
  }
  specs.push({
    name: 'suggestions-nightjar',
    kind: 'script',
    driver: { type: 'suggestions', script: 'scripts/the-nightjar-flyer.txt', seed: 1899, lines: [...WIN_PATH] },
    world: nightjarWorld(),
    records: [...g2.log.records],
    snap: snap(g2.state),
    suggestions: sugg.join('\n'),
  })
  return specs
}

// ---------------------------------------------------------------------------
// family 2 — the deterministic sampled walk (ts: explorer-lib.ts:275-306 with
// record capture; the loop is copied because sampledWalkStates() discards its
// records)
// ---------------------------------------------------------------------------

const walkFamily = (): GoldenSpec[] => {
  const world = nightjarWorld()
  let seed = 0x4e494748 // 'NIGH' — same LCG discipline as the walk pass
  const rand = (): number => ((seed = (Math.imul(seed, 1664525) + 1013904223) >>> 0) / 0x100000000)
  let state = cloneState(world.base)
  const records: ActionRecord[] = []
  const specs: GoldenSpec[] = []
  const turnLengths: number[] = [0] // ending-undo truncates to the previous turn (v0.5)
  let id = 1
  let executed = 0
  for (let i = 0; i < 500 && specs.length < 10; i++) {
    const cands = candidatesFor(world.content, state, PLAYER)
    const benign = cands.filter(c => !c.tag.includes('wrong-kind') && c.tag !== 'clock rewind')
    const pickFrom = rand() < 0.2 || benign.length === 0 ? cands : benign
    const cand = pickFrom[Math.floor(rand() * pickFrom.length) % pickFrom.length]!
    const r = settleStep(world.content, world.dict, state, cand.step, id)
    if (r.rejected !== null) continue
    turnLengths.push(records.length)
    records.push(...r.records)
    id += r.records.length
    state = r.state
    // §9.35 ending-awareness, as in the walk pass: undo through the ending
    // step so the harvest keeps exercising real play and reaches later hours
    if (state.ended) {
      records.length = turnLengths.pop()!
      state = fold(records, cloneState(world.base), world.dict, world.content)
      continue
    }
    executed++
    if (executed % 20 === 0) {
      specs.push({
        name: `walk-${executed}`,
        kind: 'walk',
        driver: { type: 'walk', harvest: executed, lcg: "0x4e494748 'NIGH'", loopCap: 500 },
        world,
        records: [...records],
        snap: snap(state),
      })
    }
  }
  // the harvest count is whatever the deterministic walk yields within the
  // 500-iteration cap (explorer-lib's "up to ten"): measured at 5 in this
  // build (walk 20..100). The Rust driver mirrors the loop and lands the same.
  check(specs.length >= 1, `the walk harvested ${specs.length} states`)
  return specs
}

// ---------------------------------------------------------------------------
// family 3 — the adversarial scenarios S1–S14. Every builder is a faithful
// copy of the reach-adversarial.test.ts engine traces + assertions (the
// analyzer/explore expects included, PR1-02/PR3-08): a scenario that fails any
// original assertion aborts the dump instead of writing goldens.
// ---------------------------------------------------------------------------

// the SHARED dictionary — the TS test file compiles every scenario against one
// module-level coreDictionary(), registering rig verbs as they appear; the
// accumulation order is part of the discipline being copied
const sDict = coreDictionary()
const compileS = (raw: ContentRaw): CompiledContent => {
  for (const name of Object.keys(raw.verbs)) if (!sDict.isVerb(name)) sDict.registerVerb(name)
  return compileContent(raw, sDict)
}

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

const aliveNames = (s: State): Set<string> =>
  new Set([...s.entities.values()].filter(e => e.deadAction === null).map(e => e.name))

/** ts: reach-adversarial.test.ts:77-79 — a programmatic clause (#id bindings) */
const probeClause = (
  verb: string,
  actors: { form: 'name'; name: string }[],
  objects: { form: 'id'; id: string }[],
): Clause => ({ actors, at: null, verb, to: null, objects, with: [], about: null })

/** ts: reach-adversarial.test.ts:145-148 — the raw BEGIN (settleStep would
 *  complete the windows it opens) */
const mkBegin = (content: CompiledContent, state: State, src: string, id: number): ExecResult => {
  const s = parseSentence(src, sDict)
  return executeStep(content, sDict, state, stepOf(s.clock, s.clauses), { firstRecordId: id })
}

/** a per-scenario capture case: records + steps sidecar + the test's runner
 *  (ts: reach-adversarial.test.ts:53-71 — monotonic ids, nextId += n + 1) */
const newCase = () => {
  const records: ActionRecord[] = []
  const slog: string[] = []
  let nextId = 1
  const run = (content: CompiledContent, state: State, src: string): ExecResult => {
    const s = parseSentence(src, sDict)
    const res = settleStep(content, sDict, state, stepOf(s.clock, s.clauses), nextId)
    nextId += res.records.length + 1
    check(res.rejected === null, `${src}: expected acceptance, got: ${res.rejected}`)
    records.push(...res.records)
    slog.push(`ok ${res.records.length}`)
    return res
  }
  const advance = (content: CompiledContent, state: State, clock: string): ExecResult => {
    const res = settleStep(content, sDict, state, { clock: parseClock(clock) as Clock, clauses: [] }, nextId)
    nextId += res.records.length + 1
    records.push(...res.records)
    slog.push(res.rejected === null ? `ok ${res.records.length}` : `rejected ${res.rejected}`)
    return res
  }
  /** a manual settleStep (fixed id, as in the test bodies) */
  const manual = (content: CompiledContent, state: State, step: Step, id: number): ExecResult =>
    settleStep(content, sDict, state, step, id)
  return { records, slog, run, advance, manual }
}

const adversarialFamily = (): GoldenSpec[] => {
  const specs: GoldenSpec[] = []
  const emit = (
    name: string,
    title: string,
    base: State,
    content: CompiledContent,
    records: ActionRecord[],
    finalState: State,
    c: { slog: string[] } | null,
  ): void => {
    specs.push({
      name,
      kind: 'scenario',
      driver: { type: 'adversarial', scenario: title },
      world: { dict: sDict, content, base },
      records,
      snap: snap(finalState),
      steps: c ? c.slog.join('\n') + '\n' : 'no engine steps (analysis-only scenario)\n',
    })
  }

  // ---------------------------------------------------------------- S1 ----
  {
    const content = compileS({
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
    const card = analyzeCard(content, state, 'KEYCARD', { usable: true })
    check(card.verdict === 'obtainable', 'S1 KEYCARD obtainable')
    check(card.existence === 'proven', 'S1 KEYCARD proven')
    check(card.chain.join('\n').includes('FORGE'), 'S1 chain names FORGE')
    check(card.usableVia.some(v => v.startsWith('INSPECT.object')), 'S1 usable via INSPECT.object')
    check(analyzeCard(content, state, 'LAMP', { usable: true }).verdict === 'obtainable', 'S1 LAMP obtainable')

    const c = newCase()
    let cur = state
    const b1 = mkBegin(content, cur, '0:0 @ ANA FORGE', 1) // window [0,60]
    check(b1.rejected === null, 'S1 b1 accepted')
    c.records.push(...b1.records)
    c.slog.push(`ok ${b1.records.length}`)
    cur = b1.state
    check(cur.windows.size === 1, 'S1 one window after FORGE')
    const b2 = mkBegin(content, cur, '0:0 @ ANA INSPECT LAMP', 2) // [0,30] IN PARALLEL
    check(b2.rejected === null, 'S1 b2 accepted')
    c.records.push(...b2.records)
    c.slog.push(`ok ${b2.records.length}`)
    cur = b2.state
    check(cur.windows.size === 2, 'S1 two parallel windows')
    const touch = c.run(content, cur, '0:1 @ ANA TOUCH LAMP')
    cur = touch.state
    check(aliveNames(cur).has('KEYCARD'), 'S1 KEYCARD minted at the close')
    cur = c.run(content, cur, '0:2 @ ANA INSPECT KEYCARD').state
    check(aliveNames(cur).has('KEYCARD'), 'S1 KEYCARD rebinds after the station frees')
    const fw = explore(content, sDict, state, 10, 2000)
    check(fw.capped === false, 'S1 explore not capped')
    check(fw.everNames.has('KEYCARD'), 'S1 explore saw KEYCARD')
    emit('s01', 'S1 — parallel windows, mint-then-use, locks free at window close', state, content, c.records, cur, c)
  }

  // ---------------------------------------------------------------- S2 ----
  {
    const content = compileS({
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
    const ash = analyzeCard(content, state, 'ASH')
    check(ash.verdict === 'obtainable', 'S2 ASH obtainable')
    check(ash.chain.join('\n').includes('paper_to_ash'), 'S2 chain names paper_to_ash')
    check(analyzeCard(content, state, 'PAPER').verdict === 'obtainable', 'S2 PAPER obtainable')

    const c = newCase()
    let cur = c.run(content, state, '0:0 @ ANA TORCH').state
    check(cur.entities.get('s_paper')!.attrs['expiresAt'] === 30, 'S2 expiresAt materialized at 30')
    cur = c.advance(content, cur, '0:1').state
    check(aliveNames(cur).has('ASH'), 'S2 ASH minted at the first crossing')
    check(aliveNames(cur).has('PAPER'), 'S2 PAPER survives the authored trigger')
    const bomb = c.advance(content, cur, '0:2')
    check(bomb.rejected !== null, 'S2 second crossing rejected')
    check(bomb.rejected!.includes('ASH'), 'S2 rejection names ASH')
    check(aliveNames(bomb.state).has('ASH'), 'S2 nothing persisted — ASH still there')
    check(aliveNames(bomb.state).has('PAPER'), 'S2 nothing persisted — PAPER still there')
    const fw = explore(content, sDict, state, 8, 2000)
    check(fw.everNames.has('ASH'), 'S2 explore saw ASH')
    check(fw.everNames.has('PAPER'), 'S2 explore saw PAPER')
    emit('s02', 'S2 — expiry chain: authored on-expire replaces the destroy and re-fires', state, content, c.records, cur, c)
  }

  // ---------------------------------------------------------------- S3 ----
  {
    const content = compileS({
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
    check(analyzeCard(content, state, 'TREE').verdict === 'obtainable', 'S3 TREE obtainable')
    check(analyzeCard(content, state, 'SPROUT').verdict === 'obtainable', 'S3 SPROUT obtainable')
    const seedRep = analyzeCard(content, state, 'SEED')
    check(seedRep.verdict === 'obtainable', 'S3 SEED obtainable')
    check(seedRep.chain.join('\n').includes('SPAWN mints'), 'S3 SEED chain names SPAWN mints')

    const c = newCase()
    let cur = c.run(content, state, '0:0 @ ANA PLANT').state
    check(aliveNames(cur).has('SEED'), 'S3 SEED alive')
    cur = c.run(content, cur, '0:0 @ ANA WATER').state
    check(!aliveNames(cur).has('SEED'), 'S3 SEED soft-destroyed')
    check(aliveNames(cur).has('SPROUT'), 'S3 SPROUT minted')
    cur = c.run(content, cur, '0:0 @ ANA TEND').state
    check(!aliveNames(cur).has('SPROUT'), 'S3 SPROUT soft-destroyed')
    check(aliveNames(cur).has('TREE'), 'S3 TREE minted')
    const fw = explore(content, sDict, state, 10, 2000)
    check(fw.everNames.has('TREE'), 'S3 explore saw TREE')
    check(fw.everNames.has('SPROUT'), 'S3 explore witnessed the transient SPROUT')
    emit('s03', 'S3 — REPLACE chain, transients die at each hop', state, content, c.records, cur, c)
  }

  // ---------------------------------------------------------------- S4 ----
  {
    const content = compileS({
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
    check(insight.verdict === 'obtainable', 'S4 INSIGHT obtainable')
    check(insight.chain.join('\n').includes('SUSPICION'), 'S4 chain names SUSPICION')
    check(insight.chain.join('\n').includes('PROBE committable'), 'S4 chain names PROBE committable')

    const c = newCase()
    let cur = c.run(content, state, '0:0 @ ANA STEW').state
    cur = c.run(content, cur, '0:0 @ ANA STEW').state
    const pool = cur.byName.get('SUSPICION')!
    check(pool.length === 2, 'S4 the stackable pool holds 2')
    // one copy is not enough: PROBE's object slot takes 2
    const single = c.manual(
      content,
      cur,
      { clock: parseClock('0:0') as Clock, clauses: [{ actors: [{ form: 'name', name: 'ANA' } as Ref], at: null, verb: 'PROBE', to: null, objects: [{ form: 'id', id: pool[0]! } as Ref], with: [], about: null } as Clause] },
      800,
    )
    check(single.rejected !== null, 'S4 under-filled PROBE rejected')
    check(single.rejected!.includes("'object' takes 2..2, got 1"), 'S4 the under-fill message')
    c.slog.push(`rejected ${single.rejected}`)
    // both copies bind individually (the pool rule, §9.38)
    const probe = c.manual(
      content,
      cur,
      { clock: parseClock('0:0') as Clock, clauses: [probeClause('PROBE', [{ form: 'name', name: 'ANA' }], pool.map(id => ({ form: 'id', id })))] },
      900,
    )
    check(probe.rejected === null, 'S4 #id-bound PROBE commits')
    c.records.push(...probe.records)
    c.slog.push(`ok ${probe.records.length}`)
    check(aliveNames(probe.state).has('INSIGHT'), 'S4 INSIGHT minted')
    const fw = explore(content, sDict, state, 10, 4000)
    check(fw.everNames.has('SUSPICION'), 'S4 explore saw SUSPICION')
    emit('s04', 'S4 — stackable pool feeds a min:2 slot via #id bindings', state, content, c.records, probe.state, c)
  }

  // ---------------------------------------------------------------- S5 ----
  {
    const content = compileS({
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
    check(d1.verdict === 'unproven', 'S5 derived shape unproven')
    check(d1.existence === 'candidate', 'S5 derived shape candidate')
    check(d1.notes.join(' ').includes('derived-mint'), 'S5 derived-mint note')
    const d2 = analyzeCard(content, state, 'WHISPER_RUMOUR_THE_BOOK')
    check(d2.verdict === 'unproven', 'S5 chain shape unproven')
    check(d2.existence === 'candidate', 'S5 chain shape candidate')
    const bare = analyzeCard(content, state, 'RUMOUR')
    check(bare.verdict === 'unproven', 'S5 bare unproven')
    check(bare.notes.join(' ').includes('derive'), 'S5 bare derive note')

    const c = newCase()
    let cur = c.run(content, state, '0:0 @ ANA HEAR THE_BOOK').state
    check(aliveNames(cur).has('RUMOUR_THE_BOOK'), 'S5 RUMOUR_THE_BOOK minted')
    cur = c.run(content, cur, '0:0 @ ANA RETELL RUMOUR_THE_BOOK').state
    check(aliveNames(cur).has('WHISPER_RUMOUR_THE_BOOK'), 'S5 the name compounds')
    check(aliveNames(cur).has('RUMOUR_THE_BOOK'), 'S5 the object survives its use')
    const fw = explore(content, sDict, state, 8, 4000)
    check(fw.everNames.has('RUMOUR_THE_BOOK'), 'S5 explore saw the derive')
    check(fw.everNames.has('WHISPER_RUMOUR_THE_BOOK'), 'S5 explore saw the chain')
    emit('s05', 'S5 — derive of derive, the name compounds', state, content, c.records, cur, c)
  }

  // ---------------------------------------------------------------- S6 ----
  {
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

    // --- the clean (stackable) variant
    const content = compileS(mk(true))
    const state = stateWith(ent('ANA', 'character'))
    for (const name of ['DAWN', 'DUSK', 'EMBER', 'ECHO']) {
      const rep = analyzeCard(content, state, name)
      check(rep.verdict === 'obtainable', `S6 ${name} obtainable`)
      check(rep.chain.join('\n').includes('deck '), `S6 ${name} chain names a deck`)
    }
    const c = newCase()
    let cur = state
    for (let i = 1; i <= 5; i++) {
      cur = c.run(content, cur, `0:${i} @ ANA WAIT`).state
    }
    const names = aliveNames(cur)
    for (const name of ['DAWN', 'DUSK', 'EMBER', 'ECHO']) check(names.has(name), `S6 ${name} alive`)
    check((cur.byName.get('DUSK') ?? []).length === 2, 'S6 the wrap stacked a second DUSK')
    const fw = explore(content, sDict, state, 10, 4000)
    check(fw.capped === false, 'S6 explore not capped')
    for (const name of ['DAWN', 'DUSK', 'EMBER', 'ECHO']) check(fw.everNames.has(name), `S6 explore saw ${name}`)
    emit('s06', 'S6 — deck reset cycles + defaultcard (stackable variant)', state, content, c.records, cur, c)

    // --- the non-stackable control: the freeze at the re-mint
    const plain = compileS(mk(false))
    check(analyzeCard(plain, state, 'DUSK').verdict === 'obtainable', 'S6-plain DUSK obtainable')
    check(analyzeCard(plain, state, 'DAWN').verdict === 'unproven', 'S6-plain DAWN unproven')
    check(analyzeCard(plain, state, 'EMBER').verdict === 'unproven', 'S6-plain EMBER unproven')
    check(analyzeCard(plain, state, 'ECHO').verdict === 'unproven', 'S6-plain ECHO unproven')
    const cp = newCase()
    let pcur = state
    for (let i = 1; i <= 3; i++) {
      pcur = cp.run(plain, pcur, `0:${i} @ ANA WAIT`).state
    }
    check(setsEq(aliveNames(pcur), new Set(['ANA', 'DUSK', 'DAWN', 'EMBER', 'ECHO'])), 'S6-plain the first three waits mint all four cards')
    const boom = cp.advance(plain, pcur, '0:5')
    check(boom.rejected !== null, 'S6-plain the wrap re-mint freezes the world')
    check(boom.rejected!.includes('DUSK'), 'S6-plain the freeze names DUSK')
    const fwPlain = explore(plain, sDict, state, 8, 4000)
    for (const name of ['DAWN', 'DUSK', 'EMBER', 'ECHO']) check(fwPlain.everNames.has(name), `S6-plain explore saw ${name}`)
    emit('s06-plain', 'S6 control — non-stackable re-mint freeze at the wrap', state, plain, cp.records, pcur, cp)
  }

  // ---------------------------------------------------------------- S7 ----
  {
    const content = compileS({
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
    check(demand.verdict === 'obtainable', 'S7 demand kind:rite obtainable')
    check(demand.chain.join('\n').includes('kind overridden'), 'S7 chain names kind overridden')
    check(analyzeCard(content, state, 'RELIC').verdict === 'obtainable', 'S7 RELIC obtainable')
    check(analyzeCard(content, state, 'RITE_PROTO').verdict === 'unobtainable', 'S7 RITE_PROTO unobtainable')

    const c = newCase()
    let cur = c.run(content, state, '0:0 @ ANA CONSECRATE').state
    cur = c.run(content, cur, '0:0 @ ANA CONSECRATE').state
    const pool = cur.byName.get('RELIC')!
    check(pool.length === 2, 'S7 two copies stacked')
    for (const id of pool) {
      check(cur.entities.get(id)!.kind === 'rite', 'S7 the kind override rides every copy')
      check(cur.entities.get(id)!.stackable === true, 'S7 the kind-stackable declaration applies')
    }
    const fw = explore(content, sDict, state, 8, 2000)
    check(fw.everDesc.some(e => e.name === 'RELIC' && e.kind === 'rite'), 'S7 explore saw the overridden descriptor')
    emit('s07', 'S7 — kind-override + kind-stackable mint', state, content, c.records, cur, c)
  }

  // ---------------------------------------------------------------- S8 ----
  {
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
      const content = compileS(mk(order))
      for (const name of ['P_TRIPLE', 'NAME_A', 'NAME_B', 'NAME_C']) {
        const rep = analyzeCard(content, state, name)
        check(rep.verdict !== 'obtainable', `S8 ${order}:${name} not obtainable`)
        check(rep.verdict === 'unproven', `S8 ${order}:${name} unproven`)
        check(rep.existence === 'candidate', `S8 ${order}:${name} candidate`)
      }
    }
    const distinct = compileS({
      prototypes: { NAME_A: { kind: 'idea' }, NAME_B: { kind: 'idea' }, NAME_C: { kind: 'idea' } },
      triggers: [
        { id: 'ta', slot: 5, out: ['SPAWN NAME_A'] },
        { id: 'tb', slot: 5, out: ['SPAWN NAME_B'] },
        { id: 'tc', slot: 5, out: ['SPAWN NAME_C'] },
      ],
      verbs: mk('abc').verbs,
    })
    for (const name of ['NAME_A', 'NAME_B', 'NAME_C']) {
      check(analyzeCard(distinct, state, name).verdict === 'obtainable', `S8 distinct ${name} obtainable`)
    }
    const fwDistinct = explore(distinct, sDict, state, 8, 2000)
    for (const name of ['NAME_A', 'NAME_B', 'NAME_C']) check(fwDistinct.everNames.has(name), `S8 distinct explore saw ${name}`)

    // forward truth (abc): the first crossing of slot 5 rejects, forever
    const content = compileS(mk('abc'))
    const c = newCase()
    const early = c.advance(content, state, '0:4') // slot 4: nothing fires yet
    check(setsEq(aliveNames(early.state), new Set(['ANA'])), 'S8 slot 4 crosses clean')
    const boom = c.advance(content, early.state, '0:5') // crosses slot 5
    check(boom.rejected !== null, 'S8 slot 5 rejects')
    check(boom.rejected!.includes('P_TRIPLE'), 'S8 the rejection names the shared mint')
    check(setsEq(aliveNames(boom.state), new Set(['ANA'])), 'S8 nothing persisted')
    const again = c.advance(content, state, '0:6')
    check(again.rejected !== null, 'S8 the freeze is permanent')
    const fw = explore(content, sDict, state, 8, 2000)
    for (const name of ['P_TRIPLE', 'NAME_A', 'NAME_B', 'NAME_C']) check(!fw.everNames.has(name), `S8 explore never saw ${name}`)
    emit('s08', 'S8 — same-slot triple with a shared mint dooms the event pass', state, content, c.records, early.state, c)
  }

  // ---------------------------------------------------------------- S9 ----
  {
    const content = compileS({
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
    check(goal.verdict === 'unproven', 'S9 GOAL unproven')
    check(goal.existence === 'candidate', 'S9 GOAL candidate')
    check(goal.notes.join(' ').includes('candidate source'), 'S9 candidate source note')

    const c = newCase()
    let cur = c.run(content, state, '0:0 @ ANA RITUAL').state
    check(aliveNames(cur).has('GOAL'), 'S9 GOAL minted before the kill')
    cur = c.advance(content, cur, '0:2').state
    check(!aliveNames(cur).has('ROPE'), 'S9 ROPE destroyed at slot 2')
    const after = c.manual(content, cur, stepOf(parseSentence('0:2 @ ANA RITUAL', sDict).clock, parseSentence('0:2 @ ANA RITUAL', sDict).clauses), 700)
    check(after.rejected !== null, 'S9 post-kill commit rejected')
    check(after.rejected!.includes('no recipe matched'), 'S9 no recipe matched')
    c.slog.push(`rejected ${after.rejected}`)
    const fw = explore(content, sDict, state, 8, 2000)
    check(fw.everNames.has('GOAL'), 'S9 explore saw GOAL')
    check(fw.everNames.has('ROPE'), 'S9 explore saw ROPE')
    emit('s09', 'S9 — gate-death: exists(ROPE) recipe dies with its rope', state, content, c.records, cur, c)
  }

  // ---------------------------------------------------------------- S10 ----
  {
    const content = compileS({
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
    check(analyzeCard(content, state, 'X').verdict === 'obtainable', 'S10 X obtainable via MAKER')
    const goal = analyzeCard(content, state, 'GOAL')
    check(goal.verdict === 'unproven', 'S10 GOAL unproven')
    check(goal.existence === 'candidate', 'S10 GOAL candidate')
    check(goal.notes.join(' ').includes('candidate source'), 'S10 candidate source note')

    const c = newCase()
    let cur = c.run(content, state, '0:0 @ ANA MAKER').state
    check(aliveNames(cur).has('X'), 'S10 X alive')
    const attempt = c.manual(content, cur, stepOf(parseSentence('0:0 @ ANA SELFMADE WITH X', sDict).clock, parseSentence('0:0 @ ANA SELFMADE WITH X', sDict).clauses), 600)
    check(attempt.rejected === null, 'S10 a refusal is not a rejection')
    c.records.push(...attempt.records)
    c.slog.push(`ok ${attempt.records.length}`)
    check(attempt.records.some(rec => rec.outcomes.some(o => o.op === 'REFUSE')), 'S10 the refusal is logged')
    check(!aliveNames(attempt.state).has('GOAL'), 'S10 nothing survived')
    check(aliveNames(attempt.state).has('X'), 'S10 the first mint stands')
    const fw = explore(content, sDict, state, 10, 4000)
    check(fw.everNames.has('X'), 'S10 explore saw X')
    check(!fw.everNames.has('GOAL'), 'S10 GOAL never appears')
    emit('s10', 'S10 — self-filler collision, the completion refuses its own mint', state, content, c.records, attempt.state, c)
  }

  // ---------------------------------------------------------------- S11 ----
  {
    const content = compileS({
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
    check(analyzeCard(content, state, 'LEFT').verdict === 'obtainable', 'S11 LEFT obtainable')
    check(analyzeCard(content, state, 'RIGHT').verdict === 'obtainable', 'S11 RIGHT obtainable')

    const c = newCase()
    const parsed = parseSentence('0:0 @ ANA FORGE_LEFT; ANA FORGE_RIGHT', sDict)
    const begin = executeStep(content, sDict, state, stepOf(parsed.clock, parsed.clauses), { firstRecordId: 1 })
    check(begin.rejected === null, 'S11 the multi-clause begin commits')
    c.records.push(...begin.records)
    c.slog.push(`ok ${begin.records.length}`)
    const windows = [...begin.state.windows.values()].sort((a, b) => a.start - b.start)
    check(
      JSON.stringify(windows.map(w => [w.verb, w.start, w.end])) ===
        JSON.stringify([['FORGE_LEFT', 0, 30], ['FORGE_RIGHT', 30, 90]]),
      'S11 serial window offsets',
    )
    const mid = executeStep(content, sDict, begin.state, { clock: parseClock('0:1') as Clock, clauses: [] }, { firstRecordId: 2 })
    check(mid.rejected === null, 'S11 the mid advance commits')
    c.records.push(...mid.records)
    c.slog.push(`ok ${mid.records.length}`)
    check(aliveNames(mid.state).has('LEFT'), 'S11 LEFT completed')
    check(!aliveNames(mid.state).has('RIGHT'), 'S11 RIGHT still open')
    const res = c.advance(content, mid.state, '0:3')
    check(aliveNames(res.state).has('RIGHT'), 'S11 RIGHT completed at 90')
    const fw = explore(content, sDict, state, 8, 4000)
    check(fw.everNames.has('LEFT'), 'S11 explore saw LEFT')
    check(fw.everNames.has('RIGHT'), 'S11 explore saw RIGHT')
    emit('s11', 'S11 — multi-clause commit with serial window offsets', state, content, c.records, res.state, c)
  }

  // ---------------------------------------------------------------- S12 ----
  {
    const content = compileS({
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
    check(phantom.verdict === 'unproven', 'S12 PHANTOM unproven')
    check(phantom.existence === 'candidate', 'S12 PHANTOM candidate')

    const c = newCase()
    const attempt = c.manual(content, state, stepOf(parseSentence('0:0 @ ANA SEANCE', sDict).clock, parseSentence('0:0 @ ANA SEANCE', sDict).clauses), 500)
    check(attempt.rejected !== null, 'S12 the gated commit rejects at the table')
    check(attempt.rejected!.includes('no recipe matched'), 'S12 no recipe matched')
    c.slog.push(`rejected ${attempt.rejected}`)
    const fw = explore(content, sDict, state, 8, 2000)
    check(!fw.everNames.has('PHANTOM'), 'S12 PHANTOM never appears')
    check(!fw.everNames.has('GHOST'), 'S12 GHOST never appears')
    emit('s12', 'S12 — world-static FALSE gate, rejected at the table (nothing logs)', state, content, c.records, state, c)
  }

  // ---------------------------------------------------------------- S13 ----
  {
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
    let lowSeed = -1
    for (let s = 0; ; s++) if (rollDie(s, 0, 6) < 4) { lowSeed = s; break }
    let highSeed = -1
    for (let s = 0; ; s++) if (rollDie(s, 0, 6) >= 4) { highSeed = s; break }
    const state = stateWith(ent('ANA', 'character'))
    check(rollDie(lowSeed, 0, 6) < 4, 'S13 low seed rolls low')
    check(rollDie(highSeed, 0, 6) >= 4, 'S13 high seed rolls high')
    for (const seed of [lowSeed, highSeed]) {
      const rep = analyzeCard(compileS(mk(seed)), state, 'PRIZE')
      check(rep.verdict === 'unproven', `S13 seed ${seed} unproven`)
      check(rep.existence === 'candidate', `S13 seed ${seed} candidate`)
    }

    // low: the refusal is a fixed point — the counter never moves
    const low = compileS(mk(lowSeed))
    const c = newCase()
    let cur = state
    for (let i = 0; i < 3; i++) {
      const attempt = c.manual(low, cur, stepOf(parseSentence('0:0 @ ANA GAMBLE', sDict).clock, parseSentence('0:0 @ ANA GAMBLE', sDict).clauses), 400 + i)
      check(attempt.rejected === null, 'S13 low: refused, not rejected')
      c.records.push(...attempt.records)
      check(attempt.state.rng === 0, 'S13 low: the counter never moved')
      check(!aliveNames(attempt.state).has('PRIZE'), 'S13 low: PRIZE never mints')
      c.slog.push(`ok ${attempt.records.length}`)
      cur = attempt.state
    }
    const fwLow = explore(low, sDict, state, 8, 2000)
    check(!fwLow.everNames.has('PRIZE'), 'S13 low explore never saw PRIZE')
    emit('s13-low', `S13 — frozen dice counter (low seed ${lowSeed})`, state, low, c.records, cur, c)

    // high: the first commit escapes the guard
    const high = compileS(mk(highSeed))
    const win = c.manual(high, state, stepOf(parseSentence('0:0 @ ANA GAMBLE', sDict).clock, parseSentence('0:0 @ ANA GAMBLE', sDict).clauses), 399)
    check(win.rejected === null, 'S13 high: the first commit commits')
    check(aliveNames(win.state).has('PRIZE'), 'S13 high: PRIZE minted')
    check(win.state.rng === 1, 'S13 high: one counter consumed')
    const ch = newCase()
    ch.records.push(...win.records)
    ch.slog.push(`ok ${win.records.length}`)
    const fwHigh = explore(high, sDict, state, 8, 2000)
    check(fwHigh.everNames.has('PRIZE'), 'S13 high explore saw PRIZE')
    emit('s13-high', `S13 — the escape roll comes (high seed ${highSeed})`, state, high, ch.records, win.state, ch)
  }

  // ---------------------------------------------------------------- S14 ----
  {
    const mk = (withArmer: boolean): ContentRaw => ({
      prototypes: { PRIZE: { kind: 'idea' } },
      triggers: [
        { id: 't_arms', onExpireOf: 'CRATE', out: ['SET(ANA, expires, 30m)'] },
        { id: 't_grand', onExpireOf: 'ANA', out: ['SPAWN PRIZE'] },
      ],
      verbs: {
        WAIT: { slots: [{ role: 'actors', min: 1, max: 1, query: 'kind:character' }], duration: 30, recipes: [{ out: [] }] },
        ...(withArmer
          ? {
              STUDY: {
                slots: [{ role: 'actors', min: 1, max: 1, query: 'kind:character' }],
                duration: 0,
                recipes: [{ out: ['SET(CRATE, expires, 30m)'] }],
              },
            }
          : {}),
      },
    })
    const dead = compileS(mk(false))
    const state = stateWith(ent('ANA', 'character'), ent('CRATE', 'item'))
    const rep = analyzeCard(dead, state, 'PRIZE')
    check(rep.verdict === 'unproven', 'S14 the broken arming chain stays unproven')
    const fw = explore(dead, sDict, state, 10, 3000)
    check(!fw.everNames.has('PRIZE'), 'S14 dead chain never mints')
    emit('s14', 'S14 — two-stage expiry arming, the chain is dead at CRATE (analysis-only)', state, dead, [], state, null)

    const live = compileS(mk(true))
    check(analyzeCard(live, state, 'PRIZE').verdict === 'obtainable', 'S14 the armed chain proves')
    const fwLive = explore(live, sDict, state, 10, 3000)
    check(fwLive.everNames.has('PRIZE'), 'S14 live chain mints')
  }

  return specs
}

// ---------------------------------------------------------------------------
// family 4 — the engine ordering anchors (PR1-01). Rigs mirror
// rust/tests/engine_ordering.rs test-for-test so its HAND placeholders can be
// replaced with these dump-measured values.
// ---------------------------------------------------------------------------

const anchorsFamily = (): GoldenSpec[] => {
  const specs: GoldenSpec[] = []
  const emit = (name: string, world: World, records: ActionRecord[], finalState: State, steps: StepsLog, note: string): void => {
    specs.push({
      name,
      kind: 'anchor',
      driver: { type: 'anchor', scenario: name, note },
      world,
      records,
      snap: snap(finalState),
      steps: steps.text(),
    })
  }

  // §3.14 row 1a — anchor-complete-before-slot-event
  {
    const { dict, content } = rig(
      [verbOf('PACE_A', [actorSlot()], 60, ['NARRATE "a done."'])],
      raw => {
        raw.triggers = [{ id: 'the_chime', slot: 2, out: ['NARRATE "the halfhour chime sings."'] }]
      },
    )
    const base = orderingSeed()
    const steps = new StepsLog()
    const records: ActionRecord[] = []
    const r1 = steps.ok(runStep(content, dict, base, '0:0 @ ANA PACE_A', 1))
    records.push(...r1.records)
    check(r1.state.windows.size === 1 && r1.state.now === 0, 'anchor-1a begin only')
    const adv = steps.ok(runClock(content, dict, r1.state, '0:2', r1.records.length + 1))
    records.push(...adv.records)
    emit('anchor-complete-before-slot-event', { dict, content, base }, records, adv.state, steps, 'completion at 60 precedes the slot-60 event')
  }

  // §3.14 row 1b — anchor-advance-chronological
  {
    const { dict, content } = rig(
      [
        verbOf('SLOW', [actorSlot()], 90, ['NARRATE "slow."']),
        verbOf('FAST', [actorSlot()], 30, ['NARRATE "fast."']),
      ],
      raw => {
        raw.triggers = [
          { id: 'halfhour_mark', slot: 1, out: ['NARRATE "mark."'] },
          { id: 'ninety_mark', slot: 3, out: ['NARRATE "ninety."'] },
        ]
      },
    )
    const base = orderingSeed()
    const steps = new StepsLog()
    const records: ActionRecord[] = []
    const r1 = steps.ok(runStep(content, dict, base, '0:0 @ ANA SLOW', 1))
    records.push(...r1.records)
    const r2 = steps.ok(runStep(content, dict, r1.state, '0:0 @ BERTHA FAST', r1.records.length + 1))
    records.push(...r2.records)
    const adv = steps.ok(runClock(content, dict, r2.state, '0:3', r1.records.length + r2.records.length + 1))
    records.push(...adv.records)
    check(adv.state.now === 90, 'anchor-1b ends at 90')
    emit('anchor-advance-chronological', { dict, content, base }, records, adv.state, steps, 'completions lead, same-minute slot events follow')
  }

  // §3.14 row 1c(a) — anchor-due-tie (beginId tie-break)
  {
    const { dict, content } = rig([
      verbOf('PACE_A', [actorSlot()], 30, ['NARRATE "a."']),
      verbOf('PACE_B', [actorSlot()], 30, ['NARRATE "b."']),
    ])
    const base = orderingSeed()
    const steps = new StepsLog()
    const records: ActionRecord[] = []
    const r1 = steps.ok(runStep(content, dict, base, '0:0 @ ANA PACE_A', 1))
    records.push(...r1.records)
    const r2 = steps.ok(runStep(content, dict, r1.state, '0:0 @ ANA PACE_B', 2))
    records.push(...r2.records)
    const adv = steps.ok(runClock(content, dict, r2.state, '0:1', 3))
    records.push(...adv.records)
    emit('anchor-due-tie', { dict, content, base }, records, adv.state, steps, 'same-end windows from different begins close in beginId order')
  }

  // §3.14 row 1c(b) — anchor-due-tie-clause (clause tie-break)
  {
    const { dict, content } = rig([
      verbOf('PACE_A', [actorSlot()], 30, ['NARRATE "a."']),
      verbOf('PACE_Z', [actorSlot()], 0, ['NARRATE "z."']),
    ])
    const base = orderingSeed()
    const steps = new StepsLog()
    const records: ActionRecord[] = []
    const s = parseSentence('0:0 @ ANA PACE_A; ANA PACE_Z', dict)
    check(s.clauses.length === 2, 'anchor-due-tie-clause two clauses')
    const r = executeStep(content, dict, base, stepOf({ day: 0, halfhour: 0 }, s.clauses), { firstRecordId: 1 })
    check(r.rejected === null, 'anchor-due-tie-clause commits')
    steps.ok(r)
    records.push(...r.records)
    const windows = [...r.state.windows.values()].map(w => [w.start, w.end])
    check(
      JSON.stringify(windows) === JSON.stringify([[0, 30], [30, 30]]),
      `anchor-due-tie-clause spans [0,30] and [30,30], got ${JSON.stringify(windows)}`,
    )
    const adv = steps.ok(runClock(content, dict, r.state, '0:1', r.records.length + 1))
    records.push(...adv.records)
    emit('anchor-due-tie-clause', { dict, content, base }, records, adv.state, steps, 'same-end windows from one begin close in clause order')
  }

  // §3.14 row 2a — anchor-expiry-insertion-order
  {
    const { dict, content } = rig([])
    const state = orderingSeed()
    expiringItem(state, 'z_card', 'ZOMBIE_CARD', 30) // insertion order, NOT name order
    expiringItem(state, 'a_card', 'APPLE_CARD', 30)
    const base = cloneState(state)
    const steps = new StepsLog()
    const adv = steps.ok(runClock(content, dict, state, '0:1', 1))
    check(
      JSON.stringify(adv.records.map(x => x.trigger)) ===
        JSON.stringify(['expire:ZOMBIE_CARD', 'expire:APPLE_CARD', null]),
      'anchor-expiry-insertion-order fires in entity insertion order',
    )
    emit('anchor-expiry-insertion-order', { dict, content, base }, adv.records, adv.state, steps, 'same-halfhour expiries fire in insertion order')
  }

  // §3.14 row 2b — anchor-on-expire-first-match
  {
    const { dict, content } = rig([], raw => {
      raw.triggers = [
        { id: 'decay_first', onExpireOf: 'INFLUENCE_MOTH_G', out: ['NARRATE "first wins."'] },
        { id: 'decay_second', onExpireOf: 'INFLUENCE_MOTH_G', out: ['NARRATE "never fires."'] },
      ]
      raw.prototypes = { PARANOIA: { kind: 'state' } }
    })
    const state = orderingSeed()
    expiringItem(state, 'moth_g', 'INFLUENCE_MOTH_G', 30)
    expiringItem(state, 'dust', 'DUST', 30)
    const base = cloneState(state)
    const steps = new StepsLog()
    const adv = steps.ok(runClock(content, dict, state, '0:1', 1))
    check(
      JSON.stringify(adv.records.map(x => x.trigger)) ===
        JSON.stringify(['decay_first', 'expire:DUST', null]),
      'anchor-on-expire-first-match: authored first match + builtin for the un-authored name',
    )
    emit('anchor-on-expire-first-match', { dict, content, base }, adv.records, adv.state, steps, 'authored on-expire first-match + builtin destroy coexist')
  }

  // §3.14 row 2c — anchor-expiry-hoist-no-cascade
  {
    const { dict, content } = rig([
      verbOf('INCUBATE', [actorSlot(), objectSlot('kind:item')], 30, ['SET(OBJECT, expires, 1h)']),
    ])
    const state = orderingSeed()
    // 2^53-1: "no expiry of its own yet" — representable identically in TS and Rust
    expiringItem(state, 'egg', 'EGG', 9007199254740991)
    const base = cloneState(state)
    const steps = new StepsLog()
    const records: ActionRecord[] = []
    const r = steps.ok(runStep(content, dict, state, '0:0 @ ANA INCUBATE EGG', 1))
    records.push(...r.records)
    const adv1 = steps.ok(runClock(content, dict, r.state, '0:1', r.records.length + 1))
    records.push(...adv1.records)
    check(
      JSON.stringify(adv1.records.map(x => x.kind)) === JSON.stringify(['complete', 'wait']),
      'anchor-expiry-hoist: pass 1 is [complete, wait] — no cascade',
    )
    const egg = adv1.state.entities.get('egg')!
    check(egg.attrs['expiresAt'] === 60, 'anchor-expiry-hoist: SET anchors to the window START (0 + 60)')
    const adv2 = steps.ok(runClock(content, dict, adv1.state, '0:2', adv1.records.length + 1 + 10))
    records.push(...adv2.records)
    check(
      JSON.stringify(adv2.records.map(x => x.kind)) === JSON.stringify(['event', 'wait']),
      'anchor-expiry-hoist: pass 2 resolves the expiry',
    )
    check(adv2.state.now === 60, 'anchor-expiry-hoist: now lands 60')
    emit('anchor-expiry-hoist-no-cascade', { dict, content, base }, records, adv2.state, steps, 'expiry snapshot hoisted once per pass; the next advance resolves')
  }

  // §3.14 row 3 — anchor-refuse-id-recycle
  {
    const { dict, content } = rig([
      verbOf('SEEDV', [actorSlot()], 30, ['NARRATE "planted."']),
      verbOf('GATE', [actorSlot()], 30, ['REFUSE("not without a lantern.")']),
    ])
    const base = orderingSeed()
    const steps = new StepsLog()
    const records: ActionRecord[] = []
    const r1 = steps.ok(runStep(content, dict, cloneState(base), '0:0 @ ANA SEEDV', 1))
    records.push(...r1.records)
    check(r1.records.length === 1 && r1.state.windows.size === 1, 'anchor-refuse: begin only')
    const r2 = steps.ok(runStep(content, dict, cloneState(r1.state), '0:1 @ ANA GATE', 2))
    records.push(...r2.records)
    check(r2.records.length === 1, 'anchor-refuse: the refusal rides one row')
    check(r2.records[0]!.id === 2, 'anchor-refuse: the refusal row reuses firstRecordId')
    check(r2.records[0]!.advancedMin === 0, 'anchor-refuse: no window elapses (§6.5)')
    check(r2.state.now === 0 && r2.state.windows.size === 1, 'anchor-refuse: state untouched')
    const r3 = steps.ok(runStep(content, dict, cloneState(r1.state), '0:1 @ ANA GATE', 2))
    records.push(...r3.records)
    check(r3.records[0]!.id === 2, 'anchor-refuse: the recycled id mints identically')
    const adv = steps.ok(runClock(content, dict, r1.state, '0:1', 2))
    records.push(...adv.records)
    check(
      JSON.stringify(adv.records.map(x => x.id)) === JSON.stringify([2, 3]),
      'anchor-refuse: the real advance closes with ids 2,3',
    )
    emit('anchor-refuse-id-recycle', { dict, content, base }, records, adv.state, steps, 'REFUSE discards the advance pass; ids recycle')
  }

  // §3.14 row 4 — anchor-wait-partition
  {
    const { dict, content } = rig([verbOf('PACE', [actorSlot()], 30, ['NARRATE "d."'])])
    const base = orderingSeed()
    const steps = new StepsLog()
    const records: ActionRecord[] = []
    const r = steps.ok(runStep(content, dict, base, '0:0 @ ANA PACE', 1))
    records.push(...r.records)
    const adv = steps.ok(runClock(content, dict, r.state, '0:1', 2))
    records.push(...adv.records)
    check(
      JSON.stringify(adv.records.map(x => x.kind)) === JSON.stringify(['complete', 'wait']),
      'anchor-wait-partition shape',
    )
    emit('anchor-wait-partition', { dict, content, base }, records, adv.state, steps, 'the wait row is the only clock mover; spans partition time')
  }

  // §3.14 row 5 — anchor-window-completion-failure
  {
    const { dict, content } = rig([verbOf('BAD', [actorSlot()], 30, ['SPAWN NOSUCH_PROTOTYPE'])])
    const base = orderingSeed()
    const steps = new StepsLog()
    const records: ActionRecord[] = []
    const r = steps.ok(runStep(content, dict, base, '0:0 @ ANA BAD', 1))
    records.push(...r.records)
    check(r.state.windows.size === 1, 'anchor-failure: the BEGIN succeeds')
    const adv = steps.ok(runClock(content, dict, r.state, '0:1', 2))
    records.push(...adv.records)
    check(
      JSON.stringify(adv.records.map(x => x.kind)) === JSON.stringify(['complete', 'wait']),
      'anchor-failure: [complete(REFUSE), wait]',
    )
    check(adv.state.windows.size === 0 && adv.state.now === 30, 'anchor-failure: window gone, clock moved')
    const adv2 = steps.ok(runClock(content, dict, adv.state, '0:2', 10))
    records.push(...adv2.records)
    check(
      JSON.stringify(adv2.records.map(x => x.kind)) === JSON.stringify(['wait']),
      'anchor-failure: the next advance is healthy',
    )
    emit('anchor-window-completion-failure', { dict, content, base }, records, adv2.state, steps, 'a failed completion refuses only its window')
  }

  // §3.14 row 6 — anchor-first-ending
  {
    const { dict, content } = rig(
      [verbOf('FINISH', [actorSlot()], 30, ['ENDING QUIET_DUSK', 'ENDING OTHER_END'])],
      raw => {
        raw.endings = {
          QUIET_DUSK: { title: 'A quiet dusk.', flavour: 'calm' },
          OTHER_END: { title: 'Another end.' },
        }
      },
    )
    const base = orderingSeed()
    const steps = new StepsLog()
    const records: ActionRecord[] = []
    const r = steps.ok(runStep(content, dict, base, '0:0 @ ANA FINISH', 1))
    records.push(...r.records)
    check(r.state.ended === null, 'anchor-ending: the ending rides the completion')
    const adv = steps.ok(runClock(content, dict, r.state, '0:1', 2))
    records.push(...adv.records)
    check(adv.state.ended?.id === 'QUIET_DUSK', 'anchor-ending: the first ENDING wins')
    check(adv.state.ended?.title === 'A quiet dusk.', 'anchor-ending: the title')
    const gated = runStep(content, dict, adv.state, '0:2 @ ANA FINISH', 10)
    check(gated.rejected === 'the story has ended — A quiet dusk.', 'anchor-ending: the story-over gate')
    steps.rejected(gated)
    check(gated.records.length === 0, 'anchor-ending: nothing further logs')
    emit('anchor-first-ending', { dict, content, base }, records, adv.state, steps, 'first ENDING wins and gates the story')
  }

  // §3.14 row 7a — anchor-roll-last-wins
  {
    const { dict, content } = rig([
      verbOf('CAST', [actorSlot()], 0, ['ROLL(1, 6)', 'ROLL(1, 6)', 'NARRATE "r={roll}."']),
    ])
    const base = orderingSeed()
    const steps = new StepsLog()
    const records: ActionRecord[] = []
    const r = steps.ok(runStep(content, dict, base, '0:0 @ ANA CAST', 1))
    records.push(...r.records)
    const adv = steps.ok(runClock(content, dict, r.state, '0:0', 2))
    records.push(...adv.records)
    emit('anchor-roll-last-wins', { dict, content, base }, records, adv.state, steps, 'ROLL last-wins within one outcome list')
  }

  // §3.14 row 7b — anchor-roll-per-clause
  {
    const cast = {
      slots: [actorSlot()],
      duration: 0,
      recipes: [{ out: ['ROLL(1, 6)', 'IF roll >= 0 -> (NARRATE "bound.")'] }],
    }
    const bare = {
      slots: [actorSlot()],
      duration: 0,
      recipes: [{ out: ['ROLL(1, 6)', 'IF roll >= 0 -> (NARRATE "bare {roll}.")'] }],
    }
    const { dict, content } = rig([
      ['CAST', cast],
      ['BARE', bare],
    ])
    const base = orderingSeed()
    const steps = new StepsLog()
    const records: ActionRecord[] = []
    const s = parseSentence('0:0 @ ANA CAST; ANA BARE', dict)
    const r = executeStep(content, dict, base, stepOf({ day: 0, halfhour: 0 }, s.clauses), { firstRecordId: 1 })
    check(r.rejected === null, 'anchor-roll-per-clause: the IF is opaque to the gate')
    steps.ok(r)
    records.push(...r.records)
    const adv = steps.ok(runClock(content, dict, r.state, '0:0', 2))
    records.push(...adv.records)
    check(adv.state.rng === 2, 'anchor-roll-per-clause: two counters consumed')
    emit('anchor-roll-per-clause', { dict, content, base }, records, adv.state, steps, 'the dice binding resets per clause')
  }

  // §3.14 row 8(a) — anchor-advance-extension
  {
    const { dict, content } = rig([
      verbOf('HATCH', [actorSlot()], 30, ['ADVANCE(1h)', 'NARRATE "warm."']),
    ])
    const base = orderingSeed()
    const steps = new StepsLog()
    const records: ActionRecord[] = []
    const r = steps.ok(runStep(content, dict, base, '0:0 @ ANA HATCH', 1))
    records.push(...r.records)
    check(r.records[0]!.advancedMin === 30, 'anchor-extension: advancedMin is the window length')
    const adv = steps.ok(runClock(content, dict, r.state, '0:1', 2))
    records.push(...adv.records)
    const wait = adv.records[1]!
    check(wait.advancedMin === 90 && wait.startMin === 0, 'anchor-extension: the wait spans [0,90]')
    check(adv.state.now === 90, 'anchor-extension: now lands 90')
    emit('anchor-advance-extension', { dict, content, base }, records, adv.state, steps, 'ADVANCE extends the pass target into the wait span')
  }

  // §3.14 row 8(b) — anchor-flash-zero-duration
  {
    const { dict, content } = rig([verbOf('FLASH', [actorSlot()], 0, ['NARRATE "zap."'])])
    const base = orderingSeed()
    const steps = new StepsLog()
    const records: ActionRecord[] = []
    const r = steps.ok(runStep(content, dict, base, '0:0 @ ANA FLASH', 1))
    records.push(...r.records)
    const adv = steps.ok(runClock(content, dict, r.state, '0:0', 2))
    records.push(...adv.records)
    check(
      JSON.stringify(adv.records.map(x => x.kind)) === JSON.stringify(['complete', 'wait']),
      'anchor-flash: due-now zero-duration shape',
    )
    check(adv.records[1]!.advancedMin === 0 && adv.state.now === 0, 'anchor-flash: zero span')
    emit('anchor-flash-zero-duration', { dict, content, base }, records, adv.state, steps, 'zero-duration windows are due exactly at begin')
  }

  // §3.14 row 9 — anchor-station-locks
  {
    const seize = {
      slots: [actorSlot(), objectSlot()],
      duration: 60,
      recipes: [{ out: ['NARRATE "seized."'] }],
    }
    const touch = {
      slots: [actorSlot(), objectSlot()],
      duration: 30,
      recipes: [{ out: ['NARRATE "touched."'] }],
    }
    const { dict, content } = rig([
      ['SEIZE', seize],
      ['TOUCH', touch],
    ])
    const base = orderingSeed()
    const steps = new StepsLog()
    const records: ActionRecord[] = []
    const r1 = steps.ok(runStep(content, dict, base, '0:0 @ ANA SEIZE MAGICK_BOOK', 1))
    records.push(...r1.records)
    check(r1.state.windows.size === 1, 'anchor-station: one window')
    const r2 = runStep(content, dict, cloneState(r1.state), '0:0 @ BERTHA SEIZE', 3)
    check(r2.rejected === 'SEIZE is already processing — its station frees at 0:2', 'anchor-station: station exclusivity message')
    check(r2.records.length === 0, 'anchor-station: the rejected step logs nothing')
    steps.rejected(r2)
    const r3 = runStep(content, dict, cloneState(r1.state), '0:0 @ ANA TOUCH MAGICK_BOOK', 3)
    check(r3.rejected === 'MAGICK BOOK is busy with SEIZE — it frees at 0:2', 'anchor-station: committed-card busy message')
    steps.rejected(r3)
    const r4 = steps.ok(runStep(content, dict, cloneState(r1.state), '0:0 @ BERTHA TOUCH', 3))
    check(r4.state.windows.size === 2, 'anchor-station: two stations in parallel; actors never locked')
    // r5 continues from r4's state so the golden log is ONE continuous play —
    // a record log assembled from two diverged branches is not fold-able to
    // its snap (the harness enforces law 1 on every golden). The en-route
    // lock release is unchanged: the advance resolves 3:0 and 1:0, freeing
    // MAGICK BOOK, then TOUCH begins.
    const r5 = steps.ok(runStep(content, dict, r4.state, '1:0 @ ANA TOUCH MAGICK_BOOK', 3))
    records.push(...r4.records)
    records.push(...r5.records)
    check(r5.records[0]!.kind === 'complete', 'anchor-station: a window closed en route')
    emit('anchor-station-locks', { dict, content, base }, records, r5.state, steps, 'station + committed-lock gates; rejections ride the sidecar')
  }

  // §3.14 row 10 — anchor-overlapping-windows
  {
    const { dict, content } = rig([
      verbOf('PACE_A', [actorSlot()], 60, ['NARRATE "a."']),
      verbOf('PACE_B', [actorSlot()], 30, ['NARRATE "b."']),
    ])
    const base = orderingSeed()
    const steps = new StepsLog()
    const records: ActionRecord[] = []
    const r1 = steps.ok(runStep(content, dict, base, '0:0 @ ANA PACE_A', 1))
    records.push(...r1.records)
    const r2 = steps.ok(runStep(content, dict, r1.state, '0:0 @ ANA PACE_B', r1.records.length + 1))
    records.push(...r2.records)
    check(r2.state.windows.size === 2 && r2.state.now === 0, 'anchor-overlap: two open windows, clock not moved')
    const wa = r2.state.windows.get('1:0')!
    const wb = r2.state.windows.get('2:0')!
    check(wa.verb === 'PACE_A' && wa.start === 0 && wa.end === 60, 'anchor-overlap: window 1:0')
    check(wb.verb === 'PACE_B' && wb.start === 0 && wb.end === 30, 'anchor-overlap: window 2:0')
    emit('anchor-overlapping-windows', { dict, content, base }, records, r2.state, steps, 'interleaved windows; the snap carries the open table')
  }

  // §3.14 row 11 — anchor-expiry-locked-drop
  {
    const { dict, content } = rig([
      verbOf('SEIZE', [actorSlot(), objectSlot()], 60, ['SET(OBJECT, aspect:touched, 1), DESTROY OBJECT']),
    ])
    const state = orderingSeed()
    state.entities.get('book')!.attrs['expiresAt'] = 30
    const base = cloneState(state)
    const steps = new StepsLog()
    const records: ActionRecord[] = []
    const r1 = steps.ok(runStep(content, dict, state, '0:0 @ ANA SEIZE MAGICK_BOOK', 1))
    records.push(...r1.records)
    check(r1.state.windows.size === 1, 'anchor-locked: one window')
    const adv = steps.ok(runClock(content, dict, r1.state, '1:0', 2))
    records.push(...adv.records)
    check(adv.records[0]!.trigger === 'expire:MAGICK_BOOK', 'anchor-locked: the expiry fired mid-window')
    const complete = adv.records.find(x => x.kind === 'complete')!
    check(
      JSON.stringify(complete.outcomes.map(o => o.op)) === JSON.stringify(['DESTROY']),
      'anchor-locked: the completion dropped the dead ref',
    )
    check(complete.outcomes[0]!.args.length === 0, 'anchor-locked: DESTROY concretized empty')
    check(!adv.state.byName.has('MAGICK_BOOK'), 'anchor-locked: the pool entry is gone')
    emit('anchor-expiry-locked-drop', { dict, content, base }, records, adv.state, steps, 'events do not respect locks; dead refs drop')
  }

  // §3.14 row 12 — anchor-slot-cadence-once
  {
    const { dict, content } = rig([], raw => {
      raw.triggers = [
        { id: 'mark', slot: 1, out: ['NARRATE "mark."'] },
        { id: 'mark', slot: 2, out: ['NARRATE "mark again."'] }, // same id, later declaration
        { id: 'once_dusk', slot: 1, once: true, out: ['NARRATE "dusk."'] },
      ]
    })
    const base = orderingSeed()
    const steps = new StepsLog()
    const records: ActionRecord[] = []
    const adv = steps.ok(runClock(content, dict, base, '0:2', 1))
    records.push(...adv.records)
    check(
      JSON.stringify(adv.records.map(x => x.trigger)) ===
        JSON.stringify(['mark', 'once_dusk', 'mark', null]),
      'anchor-cadence: slot 1 declaration order then slot 2',
    )
    check(adv.state.now === 60, 'anchor-cadence: now 60')
    const adv2 = steps.ok(runClock(content, dict, adv.state, '1:4', 10))
    records.push(...adv2.records)
    const t2 = adv2.records.map(x => x.trigger)
    check(t2.includes('mark'), 'anchor-cadence: non-once refires')
    check(!t2.includes('once_dusk'), 'anchor-cadence: once means once')
    check(adv2.state.now === 1560, 'anchor-cadence: now 1560')
    emit('anchor-slot-cadence-once', { dict, content, base }, records, adv2.state, steps, 'per-crossing declaration order; once stays once')
  }

  return specs
}

// ---------------------------------------------------------------------------
// family 6 — the fuzz seed batch (§4-W4 item 6): genScenario(seed) × K=25
// settleSteps under the frozen selector; `candidates` is copied verbatim from
// reach-fuzz-lib.ts:294-397 (it is module-private there).
// ---------------------------------------------------------------------------

const NON_WAIT_VERBS = ['READ', 'STUDY', 'WORK', 'TALK', 'EXPLORE', 'MANSUS', 'TRAVEL_TO', 'LOCATE_AT', 'DREAM', 'USE_ON', 'ACQUIRE']
void NON_WAIT_VERBS // documentation anchor: genScenario's verb pool (its own import)

function kSubsets<T>(arr: T[], k: number): T[][] {
  const out: T[][] = []
  const walk = (start: number, cur: T[]): void => {
    if (cur.length === k) {
      out.push([...cur])
      return
    }
    for (let i = start; i < arr.length; i++) walk(i + 1, [...cur, arr[i]!])
  }
  walk(0, [])
  return out
}

/** ts: reach-fuzz-lib.ts:294-397 — copied verbatim (module-private there) */
function candidates(content: CompiledContent, dict: Dictionary, s: State): Step[] {
  const out: Step[] = []
  const minFills: Clause[] = []
  const alive = [...s.entities.values()].filter(e => e.deadAction === null)
  const clock = nextStepClock(s.now)
  // waits — advance the clock so slot triggers, expiries and completions
  // strike (reach-fuzz-lib.ts:302-307)
  for (const ahead of [0, 1, 2, 12, 24, 48]) {
    const begin = ahead === 0 ? clock : clockFromAbsMin(absMin(clock) + ahead * 30)
    const parsed = parseSentence(`${formatClock(begin)} @ ANA WAIT`, dict)
    out.push({ clock: parsed.clock ?? begin, clauses: parsed.clauses })
  }
  const waitClause = parseSentence(`${formatClock(clock)} @ ANA WAIT`, dict).clauses
  const idOf = (name: string): string => alive.find(e => e.name === name)?.id ?? name
  for (const verb of content.verbs.values()) {
    if (verb.name === 'WAIT') continue
    // the engine validates ONE slot per role: the LAST entry per role wins
    // (reach-fuzz-lib.ts:312-327)
    const effSlots: typeof verb.slots = []
    {
      const byRole = new Map<string, (typeof verb.slots)[number]>()
      for (const slot of verb.slots) byRole.set(slot.role, slot) // last def wins
      for (const slot of verb.slots) {
        if (byRole.get(slot.role) === slot) effSlots.push(slot)
      }
    }
    const perRole: { role: Role; opts: Ref[][] }[] = []
    let viable = true
    for (const slot of effSlots) {
      const pool = alive.filter(e => (slot.query ? matchQuery(e, slot.query) : true))
      const maxK = Math.min(slot.max === Infinity ? 2 : slot.max, 2)
      const opts: Ref[][] = []
      for (let k = slot.min; k <= maxK; k++) {
        for (const set of kSubsets(pool, k)) {
          // a binding taking TWO copies of one name must pin them by #id
          // (reach-fuzz-lib.ts:335-346)
          const dup = new Set(set.map(e => e.name)).size !== set.length
          opts.push(set.map(e => (dup ? idRef(e.id) : nameRef(e.name))))
        }
      }
      if (slot.min > 0 && opts.length === 0) {
        viable = false
        break
      }
      perRole.push({ role: slot.role as Role, opts: opts.length > 0 ? opts : [[]] })
    }
    if (!viable) continue
    // cross-role identity (reach-fuzz-lib.ts:353-372)
    const refKey = (r: Ref): string => (r.form === 'id' ? r.id : idOf(r.name))
    let combos: Map<Role, Ref[]>[] = [new Map()]
    for (const { role, opts } of perRole) {
      const next: Map<Role, Ref[]>[] = []
      for (const base of combos) {
        for (const opt of opts.slice(0, 6)) {
          const used = [...base.values()].flat().map(refKey)
          if (opt.map(refKey).some(id => used.includes(id))) continue
          const m = new Map(base)
          m.set(role, opt)
          next.push(m)
          if (next.length >= 8) break
        }
        if (next.length >= 8) break
      }
      combos = next
    }
    for (const combo of combos) {
      const clause: Clause = emptyClause([])
      clause.verb = verb.name
      for (const [role, refs] of combo) {
        if (role === 'actors') clause.actors = refs
        else if (role === 'object') clause.objects = refs
        else if (role === 'with') clause.with = refs
        else if (role === 'at') clause.at = refs[0] ?? null
        else if (role === 'about') clause.about = refs[0] ?? null
        else if (role === 'to') clause.to = refs[0] ?? null
      }
      out.push({ clock, clauses: [clause] })
      minFills.push(clause)
    }
  }
  // multi-clause steps (reach-fuzz-lib.ts:388-395)
  for (let i = 0; i < minFills.length && i < 3; i++) {
    out.push({ clock, clauses: [minFills[i]!, ...waitClause] })
    for (let j = i + 1; j < minFills.length && j < 3; j++) {
      out.push({ clock, clauses: [minFills[i]!, minFills[j]!] })
    }
  }
  return out
}

const FUZZ_SEEDS = 200
const FUZZ_K = 25
const FUZZ_ATTEMPT_CAP = 400

const fuzzFamily = (): GoldenSpec[] => {
  const specs: GoldenSpec[] = []
  for (let seed = 1; seed <= FUZZ_SEEDS; seed++) {
    const sc = genScenario(seed)
    const dict = coreDictionary()
    const content = compileContent(sc.raw, dict, sc.entityKinds)
    const sel = rng(seed) // the selector LCG — fresh per seed, same rng() discipline
    let state = sc.initial
    const records: ActionRecord[] = []
    let id = 1
    let applied = 0
    let attempts = 0
    while (applied < FUZZ_K && attempts < FUZZ_ATTEMPT_CAP) {
      attempts++
      const steps = candidates(content, dict, state)
      const step = chooseStep(sel, steps)
      const res = settleStep(content, dict, state, step, id)
      if (res.rejected !== null) continue
      records.push(...res.records)
      id += res.records.length
      state = res.state
      applied++
    }
    // a soft-locked world (deterministic rejections everywhere) legitimately
    // exhausts the attempt cap short of K — the applied/attempts pair is
    // itself deterministic and rides the steps sidecar for the Rust replay
    // to pin
    specs.push({
      name: `fuzz-${String(seed).padStart(3, '0')}`,
      kind: 'fuzz',
      driver: { type: 'fuzz', seed, k: FUZZ_K, attemptCap: FUZZ_ATTEMPT_CAP, selector: 'action-selector.ts' },
      world: { dict, content, base: sc.initial },
      records,
      snap: snap(state),
      steps: `applied ${applied} attempts ${attempts}\n`,
    })
  }
  return specs
}

// ---------------------------------------------------------------------------
// demo — examples/demo.ts journal/prose goldens (W6's diff target, generated
// here per §4-W6). Copied verbatim from examples/demo.ts.
// ---------------------------------------------------------------------------

const demoFamily = (): GoldenSpec[] => {
  const dict = coreDictionary()
  const content = compileContent(seedContent(), dict)
  const steps = [
    '0:28 @ ANA AT HOUSE READ THE_FORGETTING', // 14:00
    '0:30 @ ANA, BERTHA READ FAIRY_TALES', // 15:00 — ends 16:00 → dusk_falls fires
    '0:32 @ ANA, BERTHA AT DINING_HALL TALK ABOUT CLUE_SECRET_HIDEOUT',
    '0:34 @ BERTHA READ MAGICK_BOOK', // refused — nobody who knows is present
    '0:34 @ ANA TRAVEL_TO THE_STUDY; ANA AT THE_STUDY READ MAGICK_BOOK',
    '0:40 @ ANA WAIT',
  ]
  const records: ActionRecord[] = []
  let state = seedState()
  let id = 1
  let n = 0
  const idGen = (): string => `x${id}_${++n}`
  for (const src of steps) {
    const s = parseSentence(src, dict)
    const r = executeStep(content, dict, state, { clock: s.clock!, clauses: s.clauses }, { firstRecordId: id, idGen })
    if (r.rejected) fail(`demo step rejected: ${r.rejected}`)
    records.push(...r.records)
    state = r.state
    id += r.records.length
  }
  return [
    {
      name: 'demo',
      kind: 'demo',
      driver: { type: 'demo', steps },
      world: { dict, content, base: seedState() },
      records,
      snap: snap(state),
      journal: journalOf(records),
      prose: records.map(rec => '· ' + prose(rec, content, dict)).join('\n') + '\n',
    },
  ]
}

// ---------------------------------------------------------------------------
// output + CLI
// ---------------------------------------------------------------------------

const FAMILIES: Array<() => GoldenSpec[]> = [
  nightjarFamily,
  walkFamily,
  adversarialFamily,
  anchorsFamily,
  fuzzFamily,
  demoFamily,
]

const buildAll = (): GoldenSpec[] => {
  const all: GoldenSpec[] = []
  for (const build of FAMILIES) all.push(...build())
  return all
}

const buildByName = (name: string): GoldenSpec | null => {
  for (const build of FAMILIES) {
    for (const sp of build()) {
      if (sp.name === name) return sp
    }
  }
  return null
}

/** §6.2 step 2 on the TS side: fold a log prefix over a golden's base and
 *  print the snap, for the TS/Rust snap对拍 */
const foldPrefix = (g: GoldenSpec, n: number, fromFile?: string): void => {
  let records: ActionRecord[]
  if (fromFile) {
    const lines = readFileSync(fromFile, 'utf8').split('\n').filter(Boolean)
    records = lines.slice(0, n).map(line => JSON.parse(line) as ActionRecord)
  } else {
    records = g.records.slice(0, n)
  }
  const st = fold(records, g.world.base, g.world.dict, g.world.content)
  process.stdout.write(snap(st) + '\n')
}

const writeGoldens = (outDir: string, specs: GoldenSpec[]): void => {
  mkdirSync(outDir, { recursive: true })
  const entries = specs.map(g => {
    const jsonl = jsonlOf(g.records)
    writeFileSync(join(outDir, `${g.name}.jsonl`), jsonl, 'utf8')
    const entry: Record<string, unknown> = {
      name: g.name,
      kind: g.kind,
      driver: g.driver,
      jsonl: `${g.name}.jsonl`,
      sha256: sha256Of(jsonl),
      records: g.records.length,
      jsonlBytes: jsonl.length,
      snap: g.snap,
    }
    if (g.journal !== undefined) {
      const text = g.journal.endsWith('\n') ? g.journal : g.journal + '\n'
      writeFileSync(join(outDir, `${g.name}.journal.txt`), text, 'utf8')
      entry.journal = `${g.name}.journal.txt`
    }
    if (g.prose !== undefined) {
      writeFileSync(join(outDir, `${g.name}.prose.txt`), g.prose, 'utf8')
      entry.prose = `${g.name}.prose.txt`
    }
    if (g.steps !== undefined) {
      writeFileSync(join(outDir, `${g.name}.steps.txt`), g.steps, 'utf8')
      entry.steps = `${g.name}.steps.txt`
    }
    if (g.suggestions !== undefined) {
      const text = g.suggestions.endsWith('\n') ? g.suggestions : g.suggestions + '\n'
      writeFileSync(join(outDir, `${g.name}.suggestions.txt`), text, 'utf8')
      entry.suggestions = `${g.name}.suggestions.txt`
    }
    return entry
  })
  writeFileSync(join(outDir, 'action-selector.ts'), SELECTOR_SOURCE, 'utf8')
  const manifest = {
    tool: 'tools/golden-dump.ts',
    selectorSource: 'action-selector.ts',
    note: 'sha256 is bookkeeping metadata written by the TS side; the Rust harness compares bytes and snaps directly (PR2-09: no sha2 dependency).',
    goldenCount: entries.length,
    goldens: entries,
  }
  writeFileSync(join(outDir, 'manifest.json'), JSON.stringify(manifest, null, 2) + '\n', 'utf8')
  const totalBytes = entries.reduce((s, e) => s + (e.jsonlBytes as number), 0)
  const byKind: Record<string, number> = {}
  for (const e of entries) byKind[e.kind as string] = (byKind[e.kind as string] ?? 0) + 1
  process.stdout.write(`goldens: ${entries.length} (${JSON.stringify(byKind)}), jsonl bytes total: ${totalBytes}\n`)
  process.stdout.write(`wrote ${outDir}/manifest.json\n`)
}

const main = (): void => {
  const args = process.argv.slice(2)
  const here = dirname(fileURLToPath(import.meta.url))
  const repoRoot = join(here, '..')
  const outIdx = args.indexOf('--out')
  const outDir = outIdx >= 0 ? args[outIdx + 1]! : join(repoRoot, 'rust', 'tests', 'goldens')

  const prefixIdx = args.indexOf('--fold-prefix')
  if (prefixIdx >= 0) {
    const name = args[prefixIdx + 1]!
    const n = Number(args[prefixIdx + 2])
    const g = buildByName(name) ?? fail(`no golden named '${name}'`)
    foldPrefix(g, n)
    return
  }
  const fileIdx = args.indexOf('--fold-prefix-file')
  if (fileIdx >= 0) {
    const path = args[fileIdx + 1]!
    const name = args[fileIdx + 2]!
    const n = Number(args[fileIdx + 3])
    const g = buildByName(name) ?? fail(`no golden named '${name}'`)
    foldPrefix(g, n, path)
    return
  }

  writeGoldens(outDir, buildAll())
}

main()
