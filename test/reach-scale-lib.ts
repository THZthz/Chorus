// Cultist-Simulator-scale load generator for the reachability analyzer (the
// companion of reach-fuzz-lib: that one checks SOUNDNESS against engine truth,
// this one checks the analyzer's SCALING — compile + per-card census + path
// export over CS-shaped content). NO vitest here (explorer-lib precedent —
// scripts and tests import alike).
//
// The content is SYNTHETIC, not a conversion (docs/reverse/cultist-simulator.md
// §6): 1,566 prototypes ≈ CS elements, N recipes ≈ CS's 2,084-recipe rulebook
// modeled the Chorus way (ONE VERB PER SITUATION, single recipe — same-verb
// multi-recipe is first-match and self-shadow), 67 decks, season/expiry
// triggers. Deterministic in the arguments: same (nProtos, nRecipes) ⇒ byte-
// identical content, so two builds produce independent, comparable worlds.
import { compileContent, type CompiledContent, type ContentRaw } from '../src/content/content.js'
import { coreDictionary, type Dictionary } from '../src/core/dict.js'
import { analyzeCard, type CardReport } from '../src/reach/reach.js'
import { addEntity, newState, type Entity, type State } from '../src/core/types.js'

export interface ScaleWorld {
  content: CompiledContent
  dict: Dictionary
  initial: State
  /** every card name worth probing: initial entities + prototypes + deck cards */
  names: string[]
  stats: { protos: number; recipes: number; ops: number; decks: number; triggers: number }
}

// ---------- deterministic RNG (explorer-lib's LCG discipline) ----------
function lcg(seed: number): () => number {
  let s = (seed ^ 0x9e3779b9) >>> 0
  return () => ((s = (Math.imul(s, 1664525) + 1013904223) >>> 0) / 0x100000000)
}

export function buildCsScale(nProtos = 1566, nRecipes = 2084): ScaleWorld {
  const rnd = lcg(0xc5)
  const int = (n: number): number => Math.floor(rnd() * n)
  const pick = <T,>(arr: T[]): T => arr[int(arr.length)]!

  const KINDS = ['item', 'text', 'idea', 'resource', 'rite', 'state', 'place', 'character']
  const ASPECTS = ['lantern', 'forge', 'edge', 'winter', 'knock', 'heart', 'grail', 'moth', 'lore', 'ingredient', 'tool', 'follower']
  const RESOURCES = Array.from({ length: 20 }, (_, i) => `R${i + 1}`)

  const protos: Record<string, Record<string, unknown>> = {}
  const protoNames: string[] = []
  for (let i = 0; i < nProtos; i++) {
    const id = `E${String(i).padStart(4, '0')}`
    protoNames.push(id)
    const kind = i < 30 ? 'character' : pick(KINDS.filter(k => k !== 'character'))
    const aspects: Record<string, number> = {}
    for (let a = 0, n = 1 + int(4); a < n; a++) aspects[pick(ASPECTS)] = 1 + int(15)
    protos[id] = { kind, aspects, ...(rnd() < 0.24 ? {} : { stackable: true }) }   // CS: 373/1566 unique
  }

  // CS's "big verb + requirements selection" is modeled one verb per situation:
  // same-verb multi-recipe is first-match in Chorus, so recipe #2+ self-shadows
  const VERB_SLOTS: [string, string][] = [
    ['WORK', 'aspect:lore>=1'], ['DREAM', 'aspect:heart>=1'], ['READ', 'aspect:lantern>=1'],
    ['TALK', 'aspect:moth>=1'], ['EXPLORE', 'aspect:winter>=1'], ['MANSUS', 'aspect:knock>=1'],
  ]
  const HUBS = protoNames.slice(900, 905)
  const nDecks = 67
  const op = (allowRefuse: boolean): string => {
    const roll = rnd()
    if (roll < 0.6) return `SPAWN ${pick(protoNames)}`
    if (roll < 0.7) return `DESTROY ${pick(RESOURCES)}`
    if (roll < 0.8) return `DRAW D${1 + int(nDecks)}`
    if (roll < 0.9) return `SET(${pick(RESOURCES)}, aspect:${pick(ASPECTS)}, ${1 + int(10)})`
    if (roll < 0.95 && allowRefuse) return 'REFUSE("no")'
    return 'NARRATE "beat"'
  }
  const verbsRaw: NonNullable<ContentRaw['verbs']> = {
    WAIT: { slots: [{ role: 'actors', min: 1, max: 1, query: 'kind:character' }], duration: 30, recipes: [{ id: 'time.needs', out: [] }] },
  }
  let ops = 0
  for (let i = 0; i < nRecipes; i++) {
    const [, query] = pick(VERB_SLOTS)
    const name = `V${i}`
    let out: string[]
    let when: string | undefined
    if (i % 4 === 0) {           // clean chain: deterministic CS-style producers (hubs minted widely)
      const a = `SPAWN ${pick(HUBS)}`, b = `SPAWN ${pick(protoNames)}`
      out = rnd() < 0.5 ? [a, b] : [b, a]
    } else {
      out = Array.from({ length: 1 + int(4) }, () => op(true))
      if (rnd() < 0.12) out.push(`ROLL(1, 6), IF roll >= ${2 + int(4)} -> (SPAWN ${pick(protoNames)}) ELSE -> (NARRATE "low")`)
      if (rnd() < 0.1) out.push(`IF exists(${pick(protoNames)}) -> (SPAWN ${pick(protoNames)}) ELSE -> (NARRATE "nested")`)
      if (rnd() < 0.25) when = pick([
        `count(name:${pick(protoNames)}) >= 1`, `exists(${pick(protoNames)})`, `actors.aspect.${pick(ASPECTS)} >= ${1 + int(4)}`,
      ])
    }
    ops += out.length
    verbsRaw[name] = {
      slots: [
        { role: 'actors', min: 1, max: 1, query: 'kind:character' },
        { role: 'object', min: 0, max: 1, query },
        ...(rnd() < 0.5 ? [{ role: 'with' as const, min: 0, max: 1, query: `aspect:${pick(ASPECTS)}>=2` }] : []),
      ],
      duration: pick([0, 30, 60, 60, 60]),
      recipes: [{ id: `${name}.r0`, ...(when ? { when } : {}), out }],
    }
  }
  const decks = Array.from({ length: nDecks }, (_, i) => ({
    id: `D${i + 1}`,
    spec: Array.from({ length: 5 + int(26) }, () => pick(protoNames)),
    ...(rnd() < 0.45 ? { defaultcard: pick(protoNames) } : {}),
    ...(rnd() < 0.8 ? { reset: true } : {}),
  }))
  const triggers = (Array.from({ length: 8 }, (_, i) => ({
    id: `season${i}`, slot: i * 6, once: false,
    out: [`SPAWN ${pick(protoNames)}`, `SET(${pick(RESOURCES)}, aspect:${pick(ASPECTS)}, ${1 + int(5)})`],
  })) as (Record<string, unknown> & { id: string })[]).concat([
    { id: 't_expiry', slot: 40, once: true, out: [`SPAWN ${pick(protoNames)}`] },
    { id: 't_big', onExpireOf: 'R1', out: [`SPAWN ${pick(protoNames)}`, `DESTROY ${pick(RESOURCES)}`] },
  ])

  const raw: ContentRaw = {
    seed: 424242,
    verbs: verbsRaw,
    prototypes: protos as unknown as ContentRaw['prototypes'],
    decks,
    triggers: triggers as unknown as ContentRaw['triggers'],
  }
  const ent = (name: string, kind: string, aspects: Entity['aspects'] = {}): Entity => ({
    id: `s_${name.toLowerCase()}`, kind, name, aspects, attrs: {}, bornAction: 0, deadAction: null, stackable: false,
  })
  const initial = newState()
  addEntity(initial, ent('ANA', 'character'))
  for (const r of RESOURCES) {
    const aspects: Record<string, number> = {}
    for (const a of ASPECTS) aspects[a] = 1 + int(3)
    addEntity(initial, ent(r, 'item', aspects))
  }
  addEntity(initial, ent('BERTHA', 'character', { follower: 1 }))
  addEntity(initial, ent('CARL', 'character', { follower: 1 }))

  const dict = coreDictionary()
  for (const name of Object.keys(verbsRaw)) if (!dict.isVerb(name)) dict.registerVerb(name)
  const content = compileContent(raw, dict)
  const names = [...new Set([...protoNames, ...RESOURCES, 'ANA', 'BERTHA', 'CARL', ...decks.flatMap(d => d.spec)])]
  return {
    content, dict, initial, names,
    stats: { protos: nProtos, recipes: nRecipes, ops, decks: nDecks, triggers: triggers.length },
  }
}

// ---------- census (single-process; the CLI shards across cores) ----------

export interface NameStat {
  name: string
  ms: number
  verdict: CardReport['verdict']
  existence: CardReport['existence']
  roots: number
  nodes: number
  depth: number
}

export function census(world: ScaleWorld, names = world.names): NameStat[] {
  const depthOf = (p: CardReport['paths'][number], d: number): number =>
    Math.max(d, ...p.requires.flatMap(r => r.paths.map(sub => depthOf(sub, d + 1))))
  const nodesOf = (p: CardReport['paths'][number]): number =>
    1 + p.requires.reduce((n, r) => n + r.paths.reduce((m, sub) => m + nodesOf(sub), 0), 0)
  return names.map(name => {
    const t = performance.now()
    const rep = analyzeCard(world.content, world.initial, name, {})
    const ms = performance.now() - t
    return {
      name, ms, verdict: rep.verdict, existence: rep.existence,
      roots: rep.paths.length,
      nodes: rep.paths.reduce((n, p) => n + nodesOf(p), 0),
      depth: rep.paths.reduce((d, p) => Math.max(d, depthOf(p, 0)), 0),
    }
  })
}

export function summarize(stats: NameStat[]): string {
  const times = stats.map(st => st.ms).sort((a, b) => a - b)
  const q = (p: number): number => times[Math.min(times.length - 1, Math.floor(times.length * p))]!
  const verdicts: Record<string, number> = {}
  for (const st of stats) verdicts[st.verdict] = (verdicts[st.verdict] ?? 0) + 1
  const roots = stats.reduce((n, st) => n + st.roots, 0)
  const maxRoots = stats.reduce((m, st) => Math.max(m, st.roots), 0)
  const depth = stats.reduce((m, st) => Math.max(m, st.depth), 0)
  return `${stats.length} names: total ${(times.reduce((a, b) => a + b, 0) / 1000).toFixed(1)} s | avg ${(times.reduce((a, b) => a + b, 0) / stats.length).toFixed(1)} ms | p50 ${q(0.5).toFixed(1)} | p95 ${q(0.95).toFixed(1)} | max ${q(1).toFixed(1)} ms | ${JSON.stringify(verdicts)} | ${roots} roots (${(roots / stats.length).toFixed(2)}/name, max ${maxRoots}), deepest tree ${depth} hops`
}
