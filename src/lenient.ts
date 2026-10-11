// Lenient player input → canonical Step. The player types English-ish text;
// the engine assigns the clock, resolves names fuzzily, and suggests fixes.
//   "read the forgetting"                          → ANA READ THE_FORGETTING
//   "ana, bertha talk about clue at dining hall"   → multi-actor + ABOUT + AT
//   "0:20 @ ANA WAIT"                              → canonical passthrough
import type { Dictionary } from './dict.js'
import type { CompiledContent } from './content.js'
import type { Clause, Role } from './sentence.js'
import { parseSentence } from './parse.js'
import { nextStepClock, parseClock, type Clock } from './clock.js'
import { nameRef, type Ref } from './ref.js'
import { matchQuery } from './query.js'
import type { Entity, State } from './types.js'
import type { Step } from './engine.js'

export interface LenientCtx {
  dict: Dictionary
  content: CompiledContent
  state: State
  /** default actor when the player omits one */
  player?: string
}

export type ParsedInput =
  | { ok: true; step: Step; canonical: string }
  | { ok: false; error: string; suggestions: string[] }

const ARTICLES = new Set(['the', 'a', 'an', 'my', 'her', 'his', 'their'])
const ROLE_KEYWORDS = ['at', 'with', 'about', 'to'] as const
type GroupKey = 'object' | 'at' | 'with' | 'about' | 'to'

export function parsePlayerInput(line: string, ctx: LenientCtx): ParsedInput {
  const trimmed = line.trim()
  if (!trimmed) return { ok: false, error: 'empty input', suggestions: [] }

  // canonical passthrough: clock-prefixed or containing an outcome arrow
  if (/^\d+:\d+\s*@/.test(trimmed) || trimmed.includes('->')) {
    try {
      const s = parseSentence(trimmed, ctx.dict)
      const clock: Clock = s.clock ?? nextStepClock(ctx.state.now) // §9.28
      return { ok: true, step: { clock, clauses: s.clauses }, canonical: trimmed }
    } catch (err) {
      return { ok: false, error: (err as Error).message, suggestions: [] }
    }
  }

  const tokens = trimmed.split(/[\s,;]+/).filter(Boolean)
  if (tokens.length === 0) return { ok: false, error: 'empty input', suggestions: [] }

  const names = aliveNames(ctx.state)

  // optional leading clock token; default: the next halfhour tick (§9.28)
  let i = 0
  let clock: Clock = nextStepClock(ctx.state.now)
  if (/^\d+:\d+$/.test(tokens[0])) {
    try { clock = parseClock(tokens[0]) } catch { return { ok: false, error: `bad clock '${tokens[0]}'`, suggestions: [] } }
    i = 1
  }

  // find the verb: single token ('READ') or two-token preposition form ('use on' → USE_ON)
  let verbIndex = -1
  let verbLen = 1
  for (let j = i; j < tokens.length; j++) {
    const up = tokens[j].toUpperCase()
    if (ctx.dict.isVerb(up)) { verbIndex = j; verbLen = 1; break }
    if ((ROLE_KEYWORDS as readonly string[]).includes(tokens[j].toLowerCase())) break
    const joined = up + '_' + (tokens[j + 1]?.toUpperCase() ?? '')
    if (ctx.dict.isVerb(joined)) { verbIndex = j; verbLen = 2; break }
  }
  if (verbIndex === -1) {
    const verbs = [...ctx.dict.verbs].map(v => v.toLowerCase())
    return {
      ok: false,
      error: `no verb found — try one of: ${verbs.map(v => v.replace(/_/g, ' ')).join(', ')}`,
      suggestions: suggestVerb(tokens[i], verbs),
    }
  }
  const verb = tokens.slice(verbIndex, verbIndex + verbLen).join('_').toUpperCase()

  // split the tail into role phrases; bare names before a keyword are objects
  const groups: Record<GroupKey, string[]> = { object: [], at: [], with: [], about: [], to: [] }
  let current: GroupKey = 'object'
  for (let j = verbIndex + verbLen; j < tokens.length; j++) {
    const low = tokens[j].toLowerCase()
    if ((ROLE_KEYWORDS as readonly string[]).includes(low)) { current = low as GroupKey; continue }
    groups[current].push(tokens[j])
  }

  const unknown: string[] = []
  const resolve = (raw: string[]): Ref[] => {
    const { refs, failed } = resolveGroup(raw, names)
    unknown.push(...failed)
    return refs
  }

  const actorsRaw = tokens.slice(i, verbIndex)
  const actorRefs = actorsRaw.length > 0 ? resolve(actorsRaw) : playerRefs(ctx)
  const objects = resolve(groups.object)
  const at = resolveGroup(groups.at, names)
  const about = resolveGroup(groups.about, names)
  const toRefs = resolve(groups.to)
  const withRefs = resolve(groups.with)
  unknown.push(...at.failed, ...about.failed) // about/at fail loudly too (§11)

  if (unknown.length > 0) {
    const word = unknown[0]
    const pool = [...names.values()].map(v => v.entity.name)
    return { ok: false, error: `I don't know '${word}'`, suggestions: suggest(word, pool) }
  }

  // English prefers "talk with quill"; slots say `to` (nightjar) or a second
  // actor (forgotten-house) — route 'with' content to the verb's real slot,
  // but only where the entity fits (§6 slot queries); anything that does not
  // fit stays in `with` so the engine rejects the phrasing the player typed
  let actors = actorRefs
  let to = toRefs[0] ?? null
  let withRole = withRefs
  const verbDef = ctx.content.verbs.get(verb)
  if (verbDef && withRole.length > 0 && !verbDef.slotByRole.has('with')) {
    const fits = (ref: Ref, role: 'to' | 'actors'): boolean => {
      const slot = verbDef.slotByRole.get(role)
      if (!slot) return false
      const id = ref.form === 'name' ? ctx.state.byName.get(ref.name) : undefined
      const e = id !== undefined ? ctx.state.entities.get(id) : undefined
      return e !== undefined && (!slot.query || matchQuery(e, slot.query))
    }
    if (to === null && withRole.length > 0 && fits(withRole[0]!, 'to')) {
      to = withRole[0]!
      withRole = withRole.slice(1)
    } else if (verbDef.slotByRole.has('actors')) {
      const actorsSlot = verbDef.slotByRole.get('actors')!
      const fitting = withRole.filter(r => fits(r, 'actors'))
      if (fitting.length > 0 && actors.length + fitting.length <= actorsSlot.max) {
        actors = [...actors, ...fitting]
        withRole = withRole.filter(r => !fitting.includes(r))
      }
    }
  }

  const clause: Clause = {
    actors,
    at: at.refs[0] ?? null,
    verb,
    to,
    objects,
    with: withRole,
    about: about.refs[0] ?? null,
  }
  return { ok: true, step: { clock, clauses: [clause] }, canonical: '' }
}

// ---------- name resolution ----------

interface NameEntry { entity: Entity; norm: string }

function aliveNames(state: State): Map<string, NameEntry> {
  const map = new Map<string, NameEntry>()
  for (const e of state.entities.values()) {
    if (e.deadAction !== null) continue
    map.set(norm(e.name), { entity: e, norm: norm(e.name) })
  }
  return map
}

/** normalize for matching: lowercase, underscores→spaces, drop articles */
function norm(name: string): string {
  return name.toLowerCase().replace(/_/g, ' ')
    .split(/\s+/).filter(w => w !== '' && !ARTICLES.has(w)).join(' ')
}

function resolveGroup(raw: string[], names: Map<string, NameEntry>): { refs: Ref[]; failed: string[] } {
  const toks = raw.filter(t => !ARTICLES.has(t.toLowerCase()))
  const refs: Ref[] = []
  const failed: string[] = []
  let j = 0
  while (j < toks.length) {
    let hit: NameEntry | undefined
    let used = 0
    for (let len = Math.min(4, toks.length - j); len >= 1; len--) {
      const key = toks.slice(j, j + len).join(' ').toLowerCase()
      const e = names.get(key)
      if (e) { hit = e; used = len; break }
    }
    if (hit) { refs.push(nameRef(hit.entity.name)); j += used }
    else { failed.push(toks[j]); j += 1 }
  }
  return { refs, failed }
}

function playerRefs(ctx: LenientCtx): Ref[] {
  if (!ctx.player) return []
  const e = [...ctx.state.entities.values()].find(e => e.deadAction === null && e.name === ctx.player)
  return e ? [nameRef(e.name)] : []
}

// ---------- suggestions ----------

function suggest(word: string, pool: string[], limit = 3): string[] {
  const w = norm(word)
  if (!w) return []
  const normed = pool.map(p => ({ p, n: norm(p) }))
  const starts = normed.filter(x => x.n.startsWith(w))
  if (starts.length > 0) return starts.slice(0, limit).map(x => x.p)
  const near = normed.filter(x => levenshtein(x.n, w) <= 2)
  return near.slice(0, limit).map(x => x.p)
}

/** like suggest(), but multi-word verbs ('travel to') also match on their
 *  first word, so 'tavel' still lands on 'travel to'; display form uses spaces */
function suggestVerb(word: string, verbs: string[], limit = 3): string[] {
  const w = norm(word)
  if (!w) return []
  const normed = verbs.map(v => ({ v, whole: norm(v), first: norm(v.split(/[\s_]/)[0] ?? '') }))
  const show = (v: string) => v.replace(/_/g, ' ')
  const starts = normed.filter(x => x.whole.startsWith(w) || x.first.startsWith(w))
  if (starts.length > 0) return starts.slice(0, limit).map(x => show(x.v))
  const near = normed.filter(x => levenshtein(x.first, w) <= 2 || levenshtein(x.whole, w) <= 2)
  return near.slice(0, limit).map(x => show(x.v))
}

function levenshtein(a: string, b: string): number {
  const dp = Array.from({ length: a.length + 1 }, (_, i) => [i, ...Array(b.length).fill(0)]) as number[][]
  for (let j = 0; j <= b.length; j++) dp[0][j] = j
  for (let i = 1; i <= a.length; i++) {
    for (let j = 1; j <= b.length; j++) {
      dp[i][j] = Math.min(
        dp[i - 1][j] + 1,
        dp[i][j - 1] + 1,
        dp[i - 1][j - 1] + (a[i - 1] === b[j - 1] ? 0 : 1),
      )
    }
  }
  return dp[a.length][b.length]
}
