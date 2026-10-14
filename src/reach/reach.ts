// Static card-reachability — "can the player obtain card X?" (design per the
// 2026-10 reachability review; engine semantics verified against src/engine.ts
// and pinned by the verification demos):
//
//   Every path by which a card can ENTER the world is enumerated over the
//   compiled content — initial entities, SPAWN, REPLACE (producer AND killer),
//   DRAW (the deck permutation is seed-fixed at load, so "which card sits at
//   position k" is static fact). Each site must itself be REACHABLE:
//   slot triggers fire on every crossing of an unbounded clock (A6/A7);
//   recipe sites need a committable verb — every slot min filled from cards
//   already known available (a fixpoint: mints feed slots feed mints).
//
// Two semantics, both implemented:
//   (a) existence  — the card can be alive at some point (default);
//   (b) usability  — and some committable verb slot can bind it (`usable`);
//   (c) demand     — some obtainable card SATISFIES a §4.5 query with any
//                    comparison op (`demand: 'aspect:secret >= 3'`); thresholds
//                    judge mint-time descriptors, and stay provably-impossible
//                    ONLY when no SET/derive/pair can ever write the path.
//
// Verdicts are CONSERVATIVE in the impossibility direction ("unobtainable"
// must never fire wrongly):
//   - 'proven'    — every hop unconditional and statically checkable;
//   - 'candidate' — a source exists but a hop is guarded (role-dependent
//     `when`, IF on roles/roll, a throwing op anywhere in the list, IF
//     fallbacks — an ELSE
//     runs only when earlier arms are false, which we cannot prove, and the
//     first-match rule shadows a recipe behind any earlier recipe we cannot
//     prove silent), so the report says 'unproven' instead;
//   - 'none'      — STRUCTURAL absence: no initial declaration, no producer
//     site at all. Sound because site enumeration is exhaustive
//     (forEachAuthoredOutcome-order recursion over every function-expanded
//     list — expansion runs before any guard walk) and because the only paths
//     out of aliveness are DESTROY, REPLACE, and built-in expiry, none of
//     which create cards.
// Mixing is the norm, not the exception, and the fixpoint is what handles it:
// mints feed slots feed mints across ALL four sources at once (a DRAW'd card
// can fill the slot that unlocks a SPAWN; a deck-fed card can satisfy an IF
// exists(...) guard on another site — path conditions re-evaluate against the
// known world every fixpoint round).
// Intra-list discipline (verified C3/A5: a mid-list failure or authored
// REFUSE discards the WHOLE draft, effects applied earlier included): each IF
// branch is its own SEGMENT — an authored REFUSE kills exactly the sites
// whose ancestor chain contains its segment (they apply only to be
// discarded), while a throwing op anywhere in the list demotes every site in
// it to candidate (the throw is ref-dependent, so not categorical). DESTROY
// is in neither class — an unresolvable DESTROY is a silent no-op, not a
// failure, so it never blocks anything.
import type { CompiledContent, CompiledDeck, CompiledVerb, PrototypeDef } from '../content/content.js'
import type { Outcome, Role } from '../core/sentence.js'
import type { CmpOp, Query } from '../core/query.js'
import { matchQuery, parseQuery } from '../core/query.js'
import type { Pred, ValExpr } from '../core/predicate.js'
import type { Aspects, Entity, State } from '../core/types.js'

export interface CardOptions {
  /** semantics (b): the card must also be bindable into some committable verb slot */
  usable?: boolean
  /** semantics (c): a §4.5 query the card must SATISFY — kind:/name:/aspect: with
   *  any comparison op (>= <= > < = !=). When set, `target` is ignored: the
   *  analysis asks whether ANY obtainable card matches. Thresholds are judged
   *  against mint-time descriptors; a threshold can also be PUMPED — any SET
   *  writing the demanded path (or a derive feed, or an unparseable authored
   *  aspects= pair) demotes 'none' to 'unproven', because deltas are unbounded
   *  and negative results are legal (verified A10). A threshold is soundly
   *  impossible only when its path is written NOWHERE and every mint-time
   *  descriptor misses. */
  demand?: string
}

export type CardVerdict = 'obtainable' | 'unobtainable' | 'unproven' | 'unusable'

export interface CardReport {
  target: string
  verdict: CardVerdict
  /** proven / candidate / none — the existence tier before usable is folded in */
  existence: 'proven' | 'candidate' | 'none'
  requestedUsable: boolean
  /** the provenance chain, one line per hop (proven paths only) */
  chain: string[]
  /** (b) verb.role sites that can bind the card (committable verbs only) */
  usableVia: string[]
  notes: string[]
}

// ---------- one available card: a static descriptor + how many live copies it can contribute ----------

interface CardFact {
  entity: Entity          // synthesized for mints — static kind/aspects descriptor
  count: number           // simultaneous living copies this source contributes (Infinity for stackable mints)
  how: string             // provenance text
}

const MANY = 1e9

// ---------- site census ----------

/** ops that throw on an unresolvable target and therefore DEFER to a
 *  completion-time window refusal (verified A5) — and a completion refusal
 *  discards the WHOLE draft, effects applied earlier included (verified C3).
 *  One anywhere in a list therefore demotes EVERY site in that list to
 *  candidate: the throw is ref-dependent, so it is a risk, not a certainty. */
const THROWING_OPS = new Set(['MOVE', 'RELATE', 'UNRELATE', 'REPLACE'])

interface SegFlags { refused: boolean; throwing: number }

interface RawSite {
  kind: 'spawn' | 'replace' | 'draw'
  name: string            // spawn/replace: the minted name; draw: the deck id
  kindOverride?: string   // SPAWN kind:X — overrides the minted kind (NOT a kind lookup; verified B5)
  deck?: CompiledDeck
  aspectsJson?: string   // authored `aspects=` pair — a static mint-time aspect override
  owner: string           // display: 'recipe <id> of VERB' | 'trigger <id>'
  verb?: CompiledVerb
  when: Pred | null       // recipe selection guard (null → default)
  recipeIndex: number
  shadowRisk: boolean     // an earlier recipe of the same verb exists (first-match may shadow)
  path: Pred[]            // enclosing IF conditions
  seg: number             // the innermost IF segment containing this site
  pos: number             // position of the site within its list (walk order)
  segs: number[]          // ancestor chain of IF segments containing this site (0 = list top level)
  fallback: boolean       // inside an ELSE body — runs only when earlier arms are false
  runtimeGated: boolean   // an enclosing IF condition is non-static (roll/role-dependent) — the mint applies only at runtime whims
  throwOps: number        // throwing-capable ops in this list (a completion refusal discards the draft)
  cap: number             // draws this site contributes to its deck (1 for a once trigger, ∞ otherwise)
  replaceTarget?: string  // REPLACE: the name-form ref being replaced (resolvability check)
  killedInList: boolean   // a later op in the list destroys/replaces this very mint — it never survives a step
  dupRisk: boolean        // this list can mint a name that is (or may become) already alive — a completion
                          // refusal would discard the whole draft: duplicate mints, or a mint of an
                          // initial-entity name (the initial copy is alive at the first commit)
  targetKilledInList: boolean // REPLACE: another op in this list also kills the replace-target
  listGateMints?: Set<string>  // non-stackable names this LIST mints (for the bootstrap-gate check)
  listStackMints?: Set<string> // STACKABLE names this LIST mints — a gate on them reads false at a
                               // once-trigger's single firing (nothing has minted X yet)
  derives?: boolean            // SPAWN of a `deriving name from object` prototype — the minted name is
                               // `${PROTO}_${OBJECT}`, and if the verb's object slot is min≥1 the
                               // bare name is NEVER what mints
  where: string}

function spawnNameRef(o: Outcome): { name: string; kind?: string } | null {
  const a = o.args[0]
  if (o.op !== 'SPAWN' || !a || a.t !== 'ref' || a.ref.form !== 'name') return null
  return { name: a.ref.name, kind: a.ref.kind }
}

function replaceWithRef(o: Outcome): string | null {
  const a = o.args[1]
  if (o.op !== 'REPLACE' || !a || a.t !== 'ref' || a.ref.form !== 'name') return null
  return a.ref.name
}

function drawDeckId(o: Outcome): string | null {
  const a = o.args[0]
  if (o.op !== 'DRAW' || !a || a.t !== 'ref' || a.ref.form !== 'name') return null
  return a.ref.name
}

/** mirrors forEachAuthoredOutcome's recursion (every IF arm + ELSE fallback —
 *  exhaustive over function-expanded lists). Each IF branch is its own
 *  SEGMENT with fresh flags: an authored REFUSE marks its OWN segment, and
 *  the post-pass kills every site whose ancestor chain contains a refused
 *  segment — position-independent, because a mid-list REFUSE discards the
 *  whole draft (C3). A throwing op marks its segment too, but demotion is
 *  list-wide: any completion refusal discards every effect in the list. */
function walkSites(
  outs: Outcome[],
  path: Pred[],
  segs: number[],
  fallback: boolean,
  flags: Map<number, SegFlags>,
  nextSeg: () => number,
  visit: (o: Outcome, path: Pred[], segs: number[], fallback: boolean) => void,
): void {
  const seg = segs[segs.length - 1]!
  if (!flags.has(seg)) flags.set(seg, { refused: false, throwing: 0 })
  const f = flags.get(seg)!
  for (const o of outs) {
    visit(o, path, segs, fallback)
    if (o.op === 'IF') {
      for (const arm of o.arms ?? []) walkSites(arm.then, [...path, arm.when], [...segs, nextSeg()], false, flags, nextSeg, visit)
      if (o.fallback) walkSites(o.fallback, path, [...segs, nextSeg()], true, flags, nextSeg, visit)
    } else if (o.op === 'REFUSE') {
      f.refused = true
    } else if (THROWING_OPS.has(o.op) || (o.op === 'DRAW' && o.args.some(a => a.t === 'into'))) {
      f.throwing += 1
    }
  }}

function collectSites(content: CompiledContent, initNames: Map<string, Entity>): {
  live: RawSite[]
  refused: RawSite[]
  setSites: { key: string; owner: string; verbName?: string; targetName?: string; targetRole?: string; segs: number[]; shadowed: boolean; refused: boolean; pathNonStatic: boolean; whenPred: Pred | null; ownerSlot: number | null }[]
  destroyedBy: Map<string, Set<string>>
  /** names minted NON-stackably anywhere in the content — a re-mint of one of
   *  these can collide with that poisoned copy (§9.14 shared namespace) */
  nonStackNames: Set<string>
  /** nonStackNames ∪ initial non-stackable entity names — the full set of
   *  re-mint-collision-capable names (§9.14: `!pool.every(stackable)`) */
  collisionNames: Set<string>
} {
  const live: RawSite[] = []
  const refused: RawSite[] = []
  const setSites: { key: string; owner: string; verbName?: string; targetName?: string; targetRole?: string; segs: number[]; shadowed: boolean; refused: boolean; pathNonStatic: boolean; whenPred: Pred | null; ownerSlot: number | null }[] = []
  const destroyedBy = new Map<string, Set<string>>()
  const nonStackNames = new Set<string>()
  const collisionNames = new Set<string>()
  for (const e of initNames.values()) if (!e.stackable) collisionNames.add(e.name)
  const walkList = (out: Outcome[], owner: string, verb: CompiledVerb | undefined, when: Pred | null, recipeIndex: number, shadowRisk: boolean, shadowed: boolean): void => {
    const ownSlot = (() => {
      const t = content.triggers.find(tt => `trigger ${tt.id}` === owner)
      return t && 'slot' in t ? t.slot : null
    })()
    const flags = new Map<number, SegFlags>()
    let segCounter = 0
    let opCounter = 0
    const sites: RawSite[] = []
    const kills: { seg: number; pos: number; name: string; throws: boolean }[] = []
    const record = (o: Outcome, path: Pred[], segs: number[], fallback: boolean): void => {
      const seg = segs[segs.length - 1]!
      const pos = opCounter++
      const killName = (): string | null => {
        const a = o.args[0]
        return a && a.t === 'ref' && a.ref.form === 'name' ? a.ref.name : null
      }
      if (o.op === 'DESTROY' || o.op === 'REPLACE') {
        const name = killName()
        if (name) {
          kills.push({ seg, pos, name, throws: o.op === 'REPLACE' })
          const owners = destroyedBy.get(name) ?? new Set<string>()
          owners.add(owner)
          destroyedBy.set(name, owners)   // cross-trigger temporal interference: anything expecting this name alive may find it dead
        }
      }
      if (o.op === 'SET') {
        // SET(ref|role, key, value) — the key names the written path
        // ('aspect:X' or an attr); the target tells WHERE it can land
        const keyArg = o.args[1]
        const key = keyArg && (keyArg.t === 'key' || keyArg.t === 'str')
          ? keyArg.v
          : keyArg && keyArg.t === 'ref' && keyArg.ref.form === 'name' ? keyArg.ref.name : null
        if (key) {
          const targetArg = o.args[0]
          setSites.push({
            key,
            owner,
            verbName: verb?.name,
            targetName: targetArg && targetArg.t === 'ref' && targetArg.ref.form === 'name' ? targetArg.ref.name : undefined,
            targetRole: targetArg && targetArg.t === 'role' ? targetArg.role : undefined,
            segs: [...segs],
            shadowed,
            refused: false,   // annotated below, once deadSegs are known
            pathNonStatic: path.some(p => !predStatic(p)),
            whenPred: when,
            ownerSlot: ownSlot,
          })
        }
      }
      const aspectsPair = o.args.find(a => a.t === 'pair' && a.k === 'aspects')
      const aspectsJson = aspectsPair && aspectsPair.t === 'pair' && typeof aspectsPair.v === 'string' ? aspectsPair.v : undefined
      const replaceTarget = (): string | undefined => {
        const a = o.args[0]
        return a && a.t === 'ref' && a.ref.form === 'name' ? a.ref.name : undefined
      }
      const site: RawSite | null =
        spawnNameRef(o) !== null ? { kind: 'spawn', name: spawnNameRef(o)!.name, kindOverride: spawnNameRef(o)!.kind, aspectsJson, derives: content.prototypes.get(spawnNameRef(o)!.name)?.derive?.nameFrom === 'object', owner, verb, when, recipeIndex, shadowRisk, path, seg, pos, segs, fallback, runtimeGated: path.some(p => !predStatic(p)), throwOps: 0, cap: 0, replaceTarget: undefined, killedInList: false, dupRisk: false, targetKilledInList: false, where: owner }
        : replaceWithRef(o) !== null ? { kind: 'replace', name: replaceWithRef(o)!, replaceTarget: replaceTarget(), owner, verb, when, recipeIndex, shadowRisk, path, seg, pos, segs, fallback, runtimeGated: path.some(p => !predStatic(p)), throwOps: 0, cap: 0, killedInList: false, dupRisk: false, targetKilledInList: false, where: owner }
        : drawDeckId(o) !== null ? { kind: 'draw', name: drawDeckId(o)!, deck: content.decks.get(drawDeckId(o)!), owner, verb, when, recipeIndex, shadowRisk, path, seg, pos, segs, fallback, runtimeGated: false, throwOps: 0, cap: 0, replaceTarget: undefined, killedInList: false, dupRisk: false, targetKilledInList: false, where: owner }
        : null
      if (!site) return
      // a DRAW naming an unknown deck is a load error (checkDeckRefs), so deck
      // can only be undefined for a draw site if content was built by hand
      if (site.kind === 'draw' && !site.deck) return
      sites.push(site)
    }
    walkSites(out, [], [0], false, flags, () => ++segCounter, record)
    // post-pass — segment-scoped REFUSE death, list-wide throwing demotion,
    // and transient-mint demotion: a DESTROY/REPLACE later in the SAME list
    // that targets this very mint means the card never survives a step (the
    // player can never act on it). A kill in an ANCESTOR segment always lands
    // after the site (the arm completes before the list continues); a kill in
    // the SAME segment must sit later in walk order.
    const deadSegs = new Set([...flags].filter(([, f]) => f.refused).map(([id]) => id))
    const throwOps = [...flags.values()].reduce((n, f) => n + f.throwing, 0)
    // duplicate non-stackable mints in one list: the second SPAWN collides
    // (`SPAWN: name 'X' already in use`, verified B5) and the completion
    // refusal discards the WHOLE draft — every site in the list is at risk.
    // A DRAW contributes its deck's card names: applications advance the
    // cursor through every position (a repeatable site) or land on any
    // position other sites have advanced to (a once trigger), and ANY of
    // them colliding with a living card dooms the whole list (fuzz seed
    // 7908: DRAW's LEDGER vs the initial LEDGER). Also: two kill-ops
    // (DESTROY/REPLACE-target) on the same name — the second REPLACE finds
    // its target already dead and throws deterministically (fuzz seed 1012:
    // REPLACE CRATE WITH P1 twice in one once-trigger freezes the clock).
    const mintCounts = new Map<string, { count: number; allStack: boolean }>()
    const addMint = (name: string, kind?: string): void => {
      // per-name UNIFORMITY: a list re-minting X collides unless EVERY mint
      // of X stacks (§9.14 shared namespace — a non-stacking mint of X
      // collides with any living X, and it also poisons the pool so later
      // stacking mints of X throw too). Seed 2654: talk.r0's `SPAWN item:P2`
      // stacks (kind item via P1) but its sibling bare `SPAWN P2` does not —
      // the bare spawn refuses and discards the draft.
      const stacks = mintStacks(content, name, kind)
      if (!stacks) {
        nonStackNames.add(name)
        collisionNames.add(name)
      }
      const e = mintCounts.get(name) ?? { count: 0, allStack: true }
      e.count += 1
      e.allStack = e.allStack && stacks
      mintCounts.set(name, e)
    }
    for (const s of sites) {
      if (s.kind === 'draw' && s.deck) {
        for (const c of s.deck.order) addMint(c.name, c.kind)
        if (s.deck.defaultcard) addMint(s.deck.defaultcard.name, s.deck.defaultcard.kind)
      } else {
        addMint(s.name, s.kindOverride)
      }
    }
    for (const k of kills) {
      if (!k.throws) continue   // a second DESTROY of a dead name is a silent no-op — the list still completes
      if (mintStacks(content, k.name)) continue
      const key = `kill:${k.name}`
      const e = mintCounts.get(key) ?? { count: 0, allStack: false }   // kill pairs always collide (1012)
      e.count += 1
      mintCounts.set(key, e)
    }
    const dupRisk = [...mintCounts.values()].some(e => e.count >= 2 && !e.allStack)
    const listGateMints = new Set([...mintCounts.entries()].filter(([, e]) => !e.allStack).map(([n]) => n))
    const listStackMints = new Set<string>()
    const stacky = (n: string, kind?: string): void => { if (mintStacks(content, n, kind)) listStackMints.add(n) }
    for (const s of sites) {
      if (s.kind === 'draw' && s.deck) {
        for (const c of s.deck.order) stacky(c.name, c.kind)
        if (s.deck.defaultcard) stacky(s.deck.defaultcard.name, s.deck.defaultcard.kind)
      } else {
        stacky(s.name, s.kindOverride)
      }
    }
    const postPass = (s: RawSite): void => {
      s.throwOps = throwOps
      s.killedInList = s.kind !== 'draw' && kills.some(k =>
        s.segs.includes(k.seg) && (k.seg !== s.seg || k.pos > s.pos) && k.name === s.name)
      s.dupRisk = dupRisk
      s.listGateMints = listGateMints
      s.listStackMints = listStackMints
      s.targetKilledInList = s.kind === 'replace' && !!s.replaceTarget
        && kills.some(k => k.name === s.replaceTarget && (k.seg !== s.seg || k.pos !== s.pos))
      if (s.kind === 'draw' && s.deck) {
        const t = content.triggers.find(tt => `trigger ${tt.id}` === s.owner)
        let cap = t && 'slot' in t && t.once ? 1 : MANY   // once trigger: one draw ever; recipes & recurring triggers: unbounded (A6/A11)
        if (cap === MANY) {
          // repeatable site: position k is served only if applications
          // 0..k-1 all succeeded — a non-stackable co-mint of this list
          // persists after the first full application, so its op re-collides
          // on every later application and the cursor freezes at 1 (fuzz
          // seed 1963: P3 blocks position 1). Not even position 0 when
          // order[0] itself is a co-mint.
          const coMints = new Set<string>()
          for (const o of sites) {
            if (o === s) continue
            if (o.kind === 'draw' && o.deck) {
              for (const c of o.deck.order) coMints.add(c.name)
              if (o.deck.defaultcard) coMints.add(o.deck.defaultcard.name)
              continue
            }
            if (!collisionNames.has(o.name)) continue
            coMints.add(o.name)
          }
          if (coMints.size > 0) cap = coMints.has(s.deck.order[0]!.name) ? 0 : 1
        }
        s.cap = cap
      } else {
        s.cap = MANY
      }
      ;(s.segs.some(id => deadSegs.has(id)) ? refused : live).push(s)
    }
    for (const s of sites) postPass(s)
    // a SET whose own segment chain contains an authored REFUSE can never
    // persist (the refusal discards the whole draft) — it must not serve as
    // an expiry-arming witness (fuzz seed 41047)
    for (const st of setSites) {
      if (st.owner === owner && st.segs.some(id => deadSegs.has(id))) st.refused = true
    }
  }
  for (const verb of content.verbs.values()) {
    verb.recipes.forEach((r, i) => {
      // first-match (§6.4): any earlier recipe can shadow this one; an earlier
      // when-less recipe shadows PERMANENTLY (its guard is always true)
      const shadowed = verb.recipes.slice(0, i).some(rr => rr.when === null)
      walkList(r.out, `recipe ${r.id} of ${verb.name}`, verb, r.when, i, i > 0, shadowed)
    })
  }
  for (const t of content.triggers) {
    walkList(t.out, `trigger ${t.id}`, undefined, null, 0, false, false)
  }
  return { live, refused, setSites, destroyedBy, nonStackNames, collisionNames }
}

// ---------- world-static predicates (verified B12) ----------
//
// exists(NAME), exists(<query>), count(<query>) over int literals — the exact
// vocabulary the engine itself allows in trigger IF conditions — are decidable
// against a static card set. Everything else (role operands, roll) is not.

function exprStatic(v: ValExpr): boolean {
  return v.t === 'int' || v.t === 'countQuery'
}

function predStatic(p: Pred): boolean {
  switch (p.t) {
    case 'and': case 'or': return predStatic(p.a) && predStatic(p.b)
    case 'not': return predStatic(p.a)
    case 'cmp': return exprStatic(p.l) && exprStatic(p.r)
    case 'exists': case 'existsQuery': return true
    default: return false
  }
}

function evalExprStatic(v: ValExpr, cards: Entity[]): number {
  if (v.t === 'int') return v.v
  if (v.t === 'countQuery') return cards.filter(c => matchQuery(c, v.query)).length
  throw new Error('internal: evalExprStatic over a non-static operand')
}

function evalStatic(p: Pred, cards: Entity[]): boolean {
  switch (p.t) {
    case 'and': return evalStatic(p.a, cards) && evalStatic(p.b, cards)
    case 'or': return evalStatic(p.a, cards) || evalStatic(p.b, cards)
    case 'not': return !evalStatic(p.a, cards)
    case 'cmp': {
      const l = evalExprStatic(p.l, cards)
      const r = evalExprStatic(p.r, cards)
      const op: CmpOp = p.op
      return op === '>=' ? l >= r : op === '<=' ? l <= r : op === '>' ? l > r : op === '<' ? l < r : op === '=' ? l === r : l !== r
    }
    case 'exists': return cards.some(c => c.name === p.name)
    case 'existsQuery': return cards.some(c => matchQuery(c, p.query))
    default: throw new Error('internal: evalStatic over a non-static predicate')
  }
}

/** every condition on the path must be static AND true against the current
 *  known world — anything else demotes the site to candidate */
function pathOpen(path: Pred[], cards: Entity[]): boolean {
  return path.every(p => predStatic(p) && evalStatic(p, cards))
}

/** the writable paths a query's aspect/state thresholds demand, in SET-key
 *  form ('aspect:x' / 'state.x') — flatMap keeps the term union narrowed */
function writablePaths(query: Query): string[] {
  return query.flatMap(t => {
    if (t.neg) return []
    if (t.f === 'aspect') return [`aspect:${t.path}`]
    if (t.f === 'state') return [`state.${t.key}`]
    return []
  })
}

// ---------- the analysis ----------

export function analyzeCard(content: CompiledContent, initial: State, target: string, opts: CardOptions = {}): CardReport {
  const notes: string[] = []
  const chain: string[] = []

  // available cards: initial living entities, grouped by name
  const avail = new Map<string, CardFact[]>()
  const push = (name: string, fact: CardFact): void => {
    const list = avail.get(name)
    if (list) list.push(fact)
    else avail.set(name, [fact])
  }
  const initNames = new Map<string, Entity>()   // one representative per name (descriptors)
  const initCount = new Map<string, number>()
  for (const e of initial.entities.values()) {
    if (e.deadAction !== null) continue
    initNames.set(e.name, e)
    initCount.set(e.name, (initCount.get(e.name) ?? 0) + 1)
  }
  for (const [name, e] of initNames) {
    push(name, { entity: e, count: initCount.get(name) ?? 1, how: `initial entity ${name} (${e.kind})` })
  }

  const protoOf = (name: string): PrototypeDef | undefined => content.prototypes.get(name)
  const startSlot = Math.floor((initial.now % 1440) / 30)
  const cyclic = (s: number): number => (((s - startSlot) % 48) + 48) % 48
  // each slot trigger's non-stackable mint names, hoisted out of the risky
  // block for the trigger-vs-recipe collision rule in siteProven (81915)
  const triggerMintNames = new Map<string, Set<string>>()
  /** minutes until the FIRST crossing of slot s (a start-slot trigger's first
   *  crossing is the next cycle — boundary == now is already spent) */
  const firstCross = (s: number): number => {
    const d = ((s * 30 - initial.now) % 1440 + 1440) % 1440
    return d === 0 ? 1440 : d
  }
  const synth = (name: string, kind: string, proto?: PrototypeDef, aspectsOverride?: Aspects): Entity => ({
    id: `~${name}`,
    kind,
    name,
    aspects: aspectsOverride ?? proto?.aspects ?? {},
    attrs: proto?.attrs ?? {},
    bornAction: 0,
    deadAction: null,
    stackable: proto?.stackable ?? false,
  })

  const { live, refused, setSites, destroyedBy, nonStackNames, collisionNames } = collectSites(content, initNames)
  // collisionNames (nonStackNames ∪ initial non-stackable names) is built in
  // collectSites — see the note there for why initial copies poison too
  // (seeds 773/1253/61271/1182)
  const setWrites = new Set(setSites.map(s => s.key))
  const riskyOwners = new Set<string>()                   // trigger owners with a deterministic refusal at their slot
  // list-level mint risk needs the INITIAL world and CROSS-LIST trigger
  // timing: (1) a list that mints the name of a living initial card collides
  // on its first commit (fuzz seeds 40/43); (2) two triggers sharing a slot
  // fire at every crossing of that halfhour in declaration order — a shared
  // non-stackable mint name collides on the very first crossing and rejects
  // the whole step forever (fuzz seed 226: the world soft-locks). Both join
  // the intra-list duplicate rule as a list-wide demotion.
  {
    const byList = new Map<string, RawSite[]>()
    for (const s of [...live, ...refused]) {
      const arr = byList.get(s.owner) ?? []
      arr.push(s)
      byList.set(s.owner, arr)
    }
    const mintSetOf = (sites: RawSite[]): Set<string> => {
      const names = new Set<string>()
      for (const s of sites) {
        if (s.kind === 'draw' && s.deck) {
          for (const c of s.deck.order) if (collisionNames.has(c.name)) names.add(c.name)
          if (s.deck.defaultcard && collisionNames.has(s.deck.defaultcard.name)) names.add(s.deck.defaultcard.name)
        } else if (collisionNames.has(s.name)) {
          names.add(s.name)
        }
      }
      return names
    }
    const risky = new Map<string, boolean>()
    for (const [owner, names] of [...byList].map(([o, ss]) => [o, mintSetOf(ss)] as const)) {
      risky.set(owner, [...names].some(n => {
        const init = initNames.get(n)
        return init !== undefined && !init.stackable
      }))
    }
    const trigBySlot = new Map<number, string[]>()
    for (const t of content.triggers) {
      if (!('slot' in t)) continue
      const arr = trigBySlot.get(t.slot) ?? []
      arr.push(`trigger ${t.id}`)
      trigBySlot.set(t.slot, arr)
    }
    for (const owners of trigBySlot.values()) {
      // a shared mint name makes the pair mutually risky in the first place…
      for (let i = 0; i < owners.length; i++) {
        for (let j = i + 1; j < owners.length; j++) {
          const a = mintSetOf(byList.get(owners[i]!) ?? [])
          const b = mintSetOf(byList.get(owners[j]!) ?? [])
          if ([...a].some(n => b.has(n))) {
            risky.set(owners[i]!, true)
            risky.set(owners[j]!, true)
          }
        }
      }
      // …and a risky trigger kills the whole EVENT PASS (A12: an event-context
      // failure rejects the step and rolls back every effect) — its throw is
      // deterministic, so every same-slot sibling's mints never persist either
      if (owners.some(o => risky.get(o) === true)) {
        for (const o of owners) risky.set(o, true)
      }
    }
    // across DIFFERENT slots the engine's firing order is the cyclic slot
    // order from the initial clock — of two triggers sharing a non-stackable
    // mint name, the LATER one's first (and every) firing collides with the
    // earlier's still-living copy, so only the later's claims demote
    // (fuzz seed 7475)
    const startSlot = Math.floor((initial.now % 1440) / 30)
    const cyclic = (s: number): number => (((s - startSlot) % 48) + 48) % 48
    const trigFlat = [...content.triggers]
      .filter((t): t is Extract<typeof t, { slot: number }> => 'slot' in t)
      .map(t => ({ owner: `trigger ${t.id}`, c: firstCross(t.slot) }))
      .sort((x, y) => x.c - y.c)
    for (let i = 0; i < trigFlat.length; i++) {
      for (let j = i + 1; j < trigFlat.length; j++) {
        if (trigFlat[i]!.c === trigFlat[j]!.c) continue   // same slot — handled above (both risky)
        const a = mintSetOf(byList.get(trigFlat[i]!.owner) ?? [])
        const b = mintSetOf(byList.get(trigFlat[j]!.owner) ?? [])
        if ([...a].some(n => b.has(n))) risky.set(trigFlat[j]!.owner, true)
      }
    }
    // cyclic slot-blocking: a risky trigger freezes the clock at its slot
    // (every crossing re-attempts the same deterministic refusal — A12), and
    // a RECURRING trigger that mints a non-stackable name collides with its
    // own earlier mint once it re-fires (fuzz seeds 5931/6458). Either way,
    // every trigger at a LATER slot — cyclically from the initial clock —
    // loses its claims: the world cannot advance past the blocking boundary.
    // Roll/role-gated mint arms join the risky set when their mint name is
    // ALSO minted elsewhere (or initial): the gated branch collides with that
    // other copy, and a refusal freezes the dice counter so the escape roll
    // never comes (fuzz seed 686).
    const otherMints = new Set<string>()
    for (const init of initNames.values()) if (!init.stackable) otherMints.add(init.name)
    for (const [ownerO, sitesO] of byList) {
      for (const s of sitesO) {
        if (s.derives && objectAlways(s.verb)) continue   // derived mints carry a different name
        if (!collisionNames.has(s.name)) continue
        otherMints.add(s.name)
      }
    }
    for (const [ownerO, sitesO] of byList) {
      const gated = sitesO.filter(s => s.runtimeGated && collisionNames.has(s.name)).map(s => s.name)
      if (gated.length === 0) continue
      const elsewhere = [...otherMints].filter(n => {
        if (gated.includes(n)) {
          // also minted by a DIFFERENT list?
          return [...byList].some(([o2, ss2]) => o2 !== ownerO && ss2.some(s2 => s2.name === n && !(s2.runtimeGated && gated.includes(n))))
        }
        return true
      })
      if (elsewhere.length > 0) {
        for (const s of sitesO) s.dupRisk = true
      }
    }
    // slot-blocking, with first-crossing distances: the FIRST crossing of a
    // blocking boundary freezes the clock there forever — a risky trigger
    // refuses deterministically (A12: the whole step rolls back, so a
    // once-trigger's `fired` never lands and it retries identically every
    // cycle — fuzz seeds 1318/1482), and a recurring trigger minting a
    // non-stackable name collides with its own earlier mint (6458). A
    // trigger whose own first crossing lies AFTER a blocking boundary loses
    // its claims — note a start-slot trigger's first crossing is the NEXT
    // cycle (boundary == now is already spent; fuzz seed 1482).
    // NOTE: firstCross (the first-crossing distance) lives at function scope —
    // the gate-timing demotion in siteProven reads it too.
    const listBlocked = (owner: string): boolean => {
      if (risky.get(owner)) return true
      return (byList.get(owner) ?? []).some(s => s.dupRisk)
    }
    // deterministic-throw freeze (fuzz seeds 2876, 10388): a slot trigger
    // whose list REPLACEs a name ANOTHER list also kills throws
    // deterministically once the killer has crossed — the killer's crossing
    // lies strictly before the thrower's first crossing (or shares the slot
    // with an earlier declaration, firing first in the pass), so the very
    // first firing's event throw rejects the WHOLE step (§9.19): the clock
    // freezes at that boundary and every claim crossing later dies. The
    // thrower's own mints roll back with the pass (A12), so its sites demote
    // too — seed 2876: t2 destroys ROPE at slot 15, t0's REPLACE ROPE at
    // slot 30 throws at minute 900, and t1's slot-0 SPAWN P1 (first crossing
    // minute 1440) never happens; seed 10388: t1 kills VIAL at minute 30,
    // t2's REPLACE VIAL throws at minute 630, freezing the world before
    // t0's slot-42 claims. A same-list kill of the REPLACE's own target is
    // deterministic as well (the DESTROY runs first, or the second crossing
    // finds the target dead).
    const throwFrozen = new Set<string>()
    const declOf = (owner: string): number => content.triggers.findIndex(tt => `trigger ${tt.id}` === owner)
    const firstCrossOf = (owner: string): number | null => {
      const t = content.triggers.find(tt => `trigger ${tt.id}` === owner)
      return t && 'slot' in t ? firstCross(t.slot) : null   // recipes commit at player-chosen times
    }
    for (const t of content.triggers) {
      if (!('slot' in t)) continue
      const owner = `trigger ${t.id}`
      const myFirst = firstCross(t.slot)
      const myDecl = declOf(owner)
      const replaces = (byList.get(owner) ?? []).filter(s => s.kind === 'replace' && s.replaceTarget !== undefined)
      if (replaces.length === 0) continue
      const selfKilled = replaces.some(s => s.targetKilledInList)
      const crossKilled = replaces.some(s =>
        [...(destroyedBy.get(s.replaceTarget!) ?? new Set<string>())].some(o => o !== owner))
      if (!selfKilled && !crossKilled) continue
      const deterministic = selfKilled || replaces.some(s =>
        [...(destroyedBy.get(s.replaceTarget!) ?? new Set<string>())].some(o => {
          if (o === owner) return false
          const fc = firstCrossOf(o)
          return fc !== null && (fc < myFirst || (fc === myFirst && declOf(o) < myDecl))
        }))
      if (deterministic) throwFrozen.add(owner)
    }
    const firstFreeze = new Map<number, number>()
    for (const t of content.triggers) {
      if (!('slot' in t)) continue
      const owner = `trigger ${t.id}`
      const recurringMinter = !t.once && mintSetOf(byList.get(owner) ?? []).size > 0
      if (listBlocked(owner) || recurringMinter) {
        const d = firstCross(t.slot)
        firstFreeze.set(d, Math.min(firstFreeze.get(d) ?? d, d))
      }
    }
    for (const owner of throwFrozen) {
      const t = content.triggers.find(tt => `trigger ${tt.id}` === owner)
      if (t && 'slot' in t) {
        const d = firstCross(t.slot)
        firstFreeze.set(d, Math.min(firstFreeze.get(d) ?? d, d))
      }
    }
    const freezeAt = firstFreeze.size > 0 ? Math.min(...firstFreeze.keys()) : Infinity
    // same-slot pass-mates of a freezing trigger never persist either —
    // the whole advance rolls back (A12). The exemption is EXACT: only a
    // recurring minter standing ON the boundary slot keeps its first-firing
    // mints, and only when nothing's FIRST firing throws there (the
    // boundary is then some recurring minter's SECOND crossing — crossing 1
    // succeeded). A recurring minter PAST the boundary never fires at all
    // and gets no exemption (fuzz seeds 471/1318/1894/60381/61094: tX
    // `SPAWN P2` at a slot past another trigger's freeze was exempted by
    // the unqualified recurring-minter case and its mints were called
    // obtainable — the world had already frozen). And when any same-slot
    // trigger's first firing throws (a condemned draft, a deterministic
    // throw), crossing 1 rolls everyone back — no exemption either
    // (fuzz seeds 74554/74985/75190/122897: same-slot freeze siblings).
    const firstFiringThrows = (owner: string): boolean => listBlocked(owner) || throwFrozen.has(owner)
    const boundarySlot = freezeAt !== Infinity
      ? content.triggers.find((t): t is Extract<typeof t, { slot: number }> => 'slot' in t && firstCross(t.slot) === freezeAt)?.slot ?? null
      : null
    for (const t of content.triggers) {
      if (!('slot' in t)) continue
      const owner = `trigger ${t.id}`
      const fc = firstCross(t.slot)
      if (fc < freezeAt) continue
      const recurringMinterSelf = !t.once && mintSetOf(byList.get(owner) ?? []).size > 0
      const onCleanBoundary = boundarySlot !== null && t.slot === boundarySlot
        && recurringMinterSelf
        && !content.triggers.some(o => 'slot' in o && o.slot === t.slot && firstFiringThrows(`trigger ${o.id}`))
      if (!onCleanBoundary) {
        for (const s of byList.get(owner) ?? []) s.dupRisk = true
      }
    }
    for (const [owner, isRisky] of risky) {
      if (!isRisky) continue
      riskyOwners.add(owner)
      for (const s of byList.get(owner) ?? []) s.dupRisk = true
    }
    // a deterministically-throwing trigger's own mints roll back with its
    // first (throwing) pass — demote them and count the trigger as risky so
    // the freeze line / sStar see the boundary it creates (fuzz seed 2876)
    for (const owner of throwFrozen) {
      riskyOwners.add(owner)
      for (const s of byList.get(owner) ?? []) s.dupRisk = true
    }
    // a draft-condemned trigger (an intra-list duplicate mint, an initial
    // collision, …) refuses deterministically at its slot just the same —
    // its first crossing carries the failing step, so it joins the risky
    // set and the frozen-completion rule / deck-cap collapse see the
    // boundary (fuzz seed 65415: t0's `SPAWN P1, SPAWN P1` on the
    // non-stackable P1 refuses at slot 1, freezing the clock at minute 30 —
    // every duration-60 recipe (read.r0's gated SPAWN P1 included) can only
    // complete PAST the freeze, so its window is rolled back forever and P1
    // never survives; the blocker fed firstFreeze but was invisible to
    // `frozen`)
    for (const t of content.triggers) {
      if (!('slot' in t)) continue
      const owner = `trigger ${t.id}`
      if (!listBlocked(owner)) continue
      riskyOwners.add(owner)
      for (const s of byList.get(owner) ?? []) s.dupRisk = true
    }
    // hoisted for siteProven's trigger-vs-recipe collision rule (81915)
    for (const t of content.triggers) {
      if (!('slot' in t)) continue
      triggerMintNames.set(`trigger ${t.id}`, mintSetOf(byList.get(`trigger ${t.id}`) ?? []))
    }
  }
  // owners whose every draft is refused at completion (a duplicate-mint
  // collision) can never persist effects — their SETs cannot arm an expiry
  // (fuzz seed 41047's witness lived in a list condemned this way)
  const dirtyOwners = new Set<string>()
  {
    for (const s of [...live, ...refused]) {
      if (s.dupRisk) dirtyOwners.add(s.owner)
    }
  }
  // the freeze line: a risky trigger deterministically refuses at its slot,
  // freezing the clock there — everything after that boundary is dead
  const riskyTriggers = content.triggers.filter((t): t is Extract<typeof t, { slot: number; once: boolean }> => 'slot' in t && riskyOwners.has(`trigger ${t.id}`))
  const frozen = riskyTriggers.length > 0
  // the freeze boundary in FIRST-CROSSING distance units — NOT cyclic order
  // (a start-slot trigger's first crossing is next cycle, minute 1440; the
  // old cyclic-order comparison called it distance 0 and zeroed every other
  // trigger draw in the world — verification agent D, seed 65415 shape)
  const freezeDist = frozen ? Math.min(...riskyTriggers.map(t => firstCross(t.slot))) : Infinity
  if (refused.length > 0) {
    notes.push(`${refused.length} producer site(s) sit behind an authored REFUSE — they can never apply and are excluded`)
  }

  // slot-trigger sites are reachable unconditionally (the clock is unbounded
  // forward and the trigger strikes every crossing — verified A6/A7); recipe
  // sites need their verb committable; expiry triggers need their named card
  // available. Committability and availability feed each other — fixpoint.
  const committable = new Map<CompiledVerb, string[]>()   // verb → slot-support lines
  const proven = new Map<RawSite, string[]>()             // site → support lines
  // proven provenance: which PROVEN sites mint each name (bare names only —
  // derive-mints behind object-mandatory verbs carry `${PROTO}_${OBJ}`), read
  // by the gate-timing and gate-source-kills-filler demotions
  const mintSources = new Map<string, RawSite[]>()
  const recordMintSource = (site: RawSite): void => {
    const names = site.kind === 'draw'
      ? [...site.deck!.order.map(c => c.name), ...(site.deck!.defaultcard ? [site.deck!.defaultcard.name] : [])]
      : [site.name]
    for (const n of names) {
      if (site.kind === 'draw' ? drawBlindsBare(content, site, n) : (site.derives && objectAlways(site.verb))) continue
      const arr = mintSources.get(n) ?? []
      arr.push(site)
      mintSources.set(n, arr)
    }
  }
  const fillers = new Map<CompiledVerb, Set<string>>()    // verb → names its committability proof filled slots with

  const slotSupport = (verb: CompiledVerb): string[] | null => {
    // the engine validates ONE slot per role: validateSlots iterates ROLES
    // and reads slotByRole, whose LAST entry per role wins (content.ts §6
    // compile) — earlier duplicate defs are inert at commit time. Modeling
    // them as extra requirements marked verbs DEAD over slots the engine
    // never checks (fuzz seed 262: WORK actors + with×1 kind:idea + with×1 —
    // the shadowed kind:idea def demanded an idea no site can ever mint, so
    // the verb was called structurally dead and P1 — minted by the committed
    // `ANA WORK WITH ROPE` — was called unobtainable; the engine trace shows
    // the commit succeeding).
    const slots = [...verb.slotByRole.values()].filter(s => s.min > 0)
    if (slots.length === 0) return []
    // per-slot candidates are EXPANDED INSTANCES (name#copy): roles take
    // DISTINCT cards — both within a role and across roles — so the same
    // instance cannot fill two roles, and a min:2 slot needs two copies
    // (fuzz seed 315: actors=ANA starved the query-less `with` role)
    const cap = 4
    const cand = slots.map(slot => {
      const list: { how: string; key: string }[] = []
      for (const facts of avail.values()) {
        for (const f of facts) {
          const fits = slot.query ? matchQuery(f.entity, slot.query) : true
          if (!fits) continue
          const copies = f.entity.stackable ? Math.min(f.count === MANY ? cap : f.count, cap) : 1
          for (let c = 0; c < copies; c++) list.push({ how: f.how, key: `${f.entity.name}#${c}` })
        }
      }
      return list
    })
    const used = new Set<string>()
    const chosen: { how: string; key: string }[][] = slots.map(() => [])
    // assign(i, from): fill slot i with exactly slot.min DISTINCT instances
    // (instances are name#copy keys — no reuse within or across roles)
    const assign = (i: number, from: number): boolean => {
      if (i === slots.length) return true
      if (chosen[i]!.length === slots[i]!.min) return assign(i + 1, 0)
      for (let k = from; k < cand[i]!.length; k++) {
        const inst = cand[i]![k]!
        if (used.has(inst.key)) continue
        used.add(inst.key)
        chosen[i]!.push(inst)
        if (assign(i, k + 1)) return true
        chosen[i]!.pop()
        used.delete(inst.key)
      }
      return false
    }
    if (!assign(0, 0)) return null
    // record the filler NAMES this committability proof relied on — the
    // gate-source-kills-filler rule reads them (fuzz seed 12460)
    const names = new Set<string>()
    for (const ch of chosen) for (const inst of ch) names.add(inst.key.split('#')[0]!)
    fillers.set(verb, names)
    return slots.map((slot, i) => {
      const whos = [...new Set(chosen[i]!.map(c => c.how))].slice(0, 3)
      return `${verb.name} committable: ${slot.role} ×${slot.min} ← ${whos.join(', ')}`
    })
  }

  let changed = true
  while (changed) {
    changed = false
    for (const verb of content.verbs.values()) {
      if (committable.has(verb)) continue
      const sup = slotSupport(verb)
      if (sup) { committable.set(verb, sup); changed = true }
    }
    for (const site of live) {
      if (proven.has(site)) continue
      const sup = siteProven(content, site, avail, committable, initNames, setWrites, setSites, destroyedBy, frozen, freezeDist, notes, mintSources, fillers, dirtyOwners, firstCross, triggerMintNames)
      if (!sup) continue
      proven.set(site, sup)
      recordMintSource(site)
      addProduct(content, site, avail, push, synth)
      changed = true
    }
  }

  // ---------- the universe of cards that can ever exist ----------
  // Static mint-time descriptors: initial ∪ prototypes ∪ deck cards ∪ mint
  // names (with kind overrides and authored `aspects=` pair overrides — that
  // pair is static JSON, so it BOUNDS the mint). Aspect values here are
  // bounds, not facts: a threshold can also be PUMPED by any SET writing the
  // demanded path (deltas unbounded, negatives legal — verified A10), or fed
  // by derive from the bound object's aspects. Those keep 'none' unprovable.
  const universe: Entity[] = [...flatten(avail)]
  for (const [name, p] of content.prototypes) universe.push(synth(name, p.kind, p))
  for (const deck of content.decks.values()) {
    for (const c of deck.order) universe.push(synth(c.name, c.kind, content.prototypes.get(c.name)))
    if (deck.defaultcard) universe.push(synth(deck.defaultcard.name, deck.defaultcard.kind, content.prototypes.get(deck.defaultcard.name)))
  }
  let opaqueSpawnAspects = false
  for (const site of live) {
    if (site.kind !== 'spawn') continue
    const p = content.prototypes.get(site.name)
    let override: Aspects | undefined
    if (site.aspectsJson) {
      try { override = { ...(p?.aspects ?? {}), ...JSON.parse(site.aspectsJson) as Aspects } }
      catch { opaqueSpawnAspects = true }   // authored pair we cannot parse — cannot bound
    }
    universe.push(synth(site.name, site.kindOverride ?? p?.kind ?? 'item', p, override))
  }
  const derivePaths = new Set<string>()
  for (const p of content.prototypes.values()) {
    for (const k of Object.keys(p.derive?.aspects ?? {})) derivePaths.add(`aspect:${k}`)
  }
  // can a card matching `query` EVER exist? A universe hit settles yes; a
  // purely structural query with no hit settles no (the universe is
  // exhaustive over entry paths and kind/name are mint-immutable); an aspect
  // threshold with no descriptor hit stays undecided if anything can write
  // its path (SET / derive / opaque authored pairs).
  const canEverMatch = (query: Query | undefined): boolean => {
    if (!query || query.length === 0) return true
    if (universe.some(u => matchQuery(u, query))) return true
    if (opaqueSpawnAspects) return true
    const paths = writablePaths(query)
    if (paths.length === 0) return false          // purely structural — the universe is exhaustive
    if (paths.some(p => setWrites.has(p))) return true
    if (paths.some(p => derivePaths.has(p))) return true
    return false
  }

  // ---------- dead verbs ----------
  // A verb still uncommittable at fixpoint is DEAD — hence its sites are
  // structurally dead, not merely guarded — when one of its min>0 slots can
  // never be filled. canEverMatch is the sound boundary: structural absence,
  // or a threshold bounded away with NO SET/derive/pair writing its path
  // anywhere in the content.
  const deadVerbs = new Set<CompiledVerb>()
  for (const verb of content.verbs.values()) {
    if (committable.has(verb)) continue
    // slotByRole, not verb.slots — the engine validates one slot per role
    // (last def wins); a shadowed duplicate def must not doom the verb
    // (fuzz seed 262)
    const hopeless = [...verb.slotByRole.values()].some(slot => slot.min > 0 && !canEverMatch(slot.query))
    if (hopeless) deadVerbs.add(verb)
  }

  const provenSourceFor = (name: string): string[] | null => {
    if (initNames.has(name)) {
      const e = initNames.get(name)!
      return [`${name} is declared in the initial entities (${e.kind}${Object.keys(e.aspects).length ? ', aspects ' + JSON.stringify(e.aspects) : ''})`]
    }
    for (const [site, sup] of proven) {
      const hit = site.kind === 'draw'
        ? (!drawBlindsBare(content, site, name) && servedByCap(site.deck!, name, deckCap.get(site.name) ?? 0))
        : site.name === name
      if (hit) return siteChain(content, site, name, sup)
    }
    return null
  }

  // deck capacity from proven sites. Under NO risky trigger, crossings are
  // unbounded: a once trigger contributes exactly one draw, recipes and
  // recurring triggers are unbounded (A6/A11). With a risky trigger at
  // cyclic slot S*, the world FREEZES at S* forever (fuzz seed 20164: the
  // day-1 re-crossing of slot 12 never happens) — so only the crossings
  // strictly before S* occur, exactly once each: every pre-S* draw site
  // contributes exactly ONE draw, everything at/after S* contributes zero.
  // Unproven sites only ADD capacity, so this sum is a sound lower bound.
  const deckCap = new Map<string, number>()
  for (const [site] of proven) {
    if (site.kind !== 'draw') continue
    let cap = site.cap
    if (frozen) {
      if (site.verb !== undefined) {
        cap = site.cap            // recipe-owned: commits at/short of the freeze are still unbounded (0-duration)
      } else {
        const t = content.triggers.find(tt => `trigger ${tt.id}` === site.owner)
        cap = t && 'slot' in t && firstCross(t.slot) < freezeDist ? 1 : 0
      }
    }
    deckCap.set(site.name, Math.max(deckCap.get(site.name) ?? 0, cap))
  }

  let existence: CardReport['existence'] = 'none'
  let demandMatch: Entity | null = null
  const demandQ = opts.demand ? parseQuery(opts.demand) : null

  if (demandQ) {
    // semantics (c): ANY obtainable card satisfying the demand query — matched
    // against each source's ACTUAL product descriptor (a kind: override
    // changes the minted kind — B5), never the bare prototype
    for (const e of initNames.values()) {
      if (!matchQuery(e, demandQ)) continue
      existence = 'proven'
      demandMatch = e
      chain.push(`${e.name} (${e.kind}${Object.keys(e.aspects).length ? ', aspects ' + JSON.stringify(e.aspects) : ''}) satisfies the demand '${opts.demand}'`)
      chain.push(...provenSourceFor(e.name)!)
      break
    }
    if (existence === 'none') {
      for (const [site, sup] of proven) {
        const descs: Entity[] = site.kind === 'draw'
          ? [...site.deck!.order, ...(site.deck!.defaultcard ? [site.deck!.defaultcard!] : [])]
              .map(c => {
                const desc = synth(c.name, c.kind, content.prototypes.get(c.name))
                // a draw of a derive-card behind an object-mandatory verb never
                // carries the bare name (fuzz seed 12320) — blind it so
                // `name:X` demands cannot match, while kind/aspect demands can
                return drawBlindsBare(content, site, c.name) ? { ...desc, name: '\u0000derived' } : desc
              })
          : [synth(site.name, site.kindOverride ?? content.prototypes.get(site.name)?.kind ?? 'item', content.prototypes.get(site.name), siteAspects(site))]
        for (const desc of descs) {
          // a derive-mint behind an object-mandatory verb never carries the
          // bare name — blind the name so `name:X` demands cannot match it,
          // while kind/aspect demands still can
          const match = site.derives && objectAlways(site.verb) ? { ...desc, name: '\u0000derived' } : desc
          if (!matchQuery(match, demandQ)) continue
          if (site.kind === 'draw' && !servedByCap(site.deck!, desc.name, deckCap.get(site.name) ?? 0)) continue
          existence = 'proven'
          demandMatch = desc
          chain.push(`${desc.name} (${desc.kind}${Object.keys(desc.aspects).length ? ', aspects ' + JSON.stringify(desc.aspects) : ''}) satisfies the demand '${opts.demand}'`)
          chain.push(...siteChain(content, site, desc.name, sup))
          break
        }
        if (existence === 'proven') break
      }
    }
    if (existence === 'none') {
      // A static descriptor may match behind guarded sites; a threshold may
      // be pumped by SET or fed by derive. Any of these keeps 'none'
      // unprovable — the sound impossibility requires a bounded path.
      const matchesSomewhere = universe.some(u => matchQuery(u, demandQ))
      const paths = writablePaths(demandQ)
      const pumps = paths.filter(p => setWrites.has(p))
      const derives = paths.filter(p => derivePaths.has(p))
      for (const p of pumps) {
        notes.push(`'${p}' is written by SET at: ${setSites.filter(s => s.key === p).map(s => s.owner).join(', ')} — the threshold can be pumped at play time`)
      }
      for (const p of derives) {
        notes.push(`'${p}' is derive-fed at mint time (from the bound object's aspects) — its threshold cannot be bounded statically`)
      }
      if (matchesSomewhere) notes.push(`a static descriptor matches the demand but no proven source reaches it — witness needed`)
      existence = matchesSomewhere || pumps.length > 0 || derives.length > 0 || opaqueSpawnAspects ? 'candidate' : 'none'
    }
  } else if (initNames.has(target)) {
    existence = 'proven'
    chain.push(...provenSourceFor(target)!)
  } else {
    for (const [site, sup] of proven) {
      // a derive-mint behind an object-mandatory verb never mints the BARE
      // name (`${PROTO}_${OBJECT}` always — verified C4/B6); a DRAW of a
      // derive-card behind such a verb renames the mint the same way (fuzz
      // seed 12320)
      if (site.derives && objectAlways(site.verb)) continue
      const hit =
        site.kind === 'draw'
          ? (!drawBlindsBare(content, site, target) && servedByCap(site.deck!, target, deckCap.get(site.name) ?? 0))
          : site.name === target
      if (!hit) continue
      existence = 'proven'
      chain.push(...siteChain(content, site, target, sup))
      break
    }
    if (existence === 'none') {
      // no proven path — but a live, guarded site still blocks 'unobtainable'
      const candidateSite = live.find(s =>
        !deadVerbs.has(s.verb as CompiledVerb) &&
        !(s.derives && objectAlways(s.verb)) &&
        (s.kind === 'draw'
          ? (!drawBlindsBare(content, s, target) && deckServes(s.deck!, target) &&
            // a PROVEN draw site whose cap cannot reach this position is
            // still a guarded source in a frozen world: the freeze model
            // puts the boundary at the risky trigger's FIRST crossing, but
            // a clean recurring minter only collides on its SECOND — the
            // pre-boundary draw count can exceed the modeled 1 (fuzz seed
            // 81401). Unfrozen caps are exact structural facts (a once
            // trigger draws exactly once; co-mint caps come from
            // deterministic collision analysis), so they keep ruling the
            // position out.
            (!proven.has(s) || (frozen && !servedByCap(s.deck!, target, deckCap.get(s.name) ?? 0))))
          : (s.name === target && !proven.has(s))))
      if (candidateSite) existence = 'candidate'
    }
    if (existence === 'none') {
      // every producer of the bare name is a derive-mint behind an
      // object-mandatory verb — BUT the bare name is still reachable when
      // the object binding dies mid-window and drops at completion (the
      // derive silently no-ops — verified B6; fuzz seed 30790 mints the bare
      // name exactly this way). Soundly unprovable → candidate.
      const derivedOnly = live.some(s =>
        // a dead verb's sites never produce — they must not negate the
        // derive-drop escape (fuzz seed 91492: explore.r1's REPLACE LEDGER P3
        // is a bare-name producer, but EXPLORE is structurally dead — the
        // with×2 kind:text slot can never be filled — while bare P3 still
        // mints when travel_to's derive no-ops on the dropped object binding)
        !deadVerbs.has(s.verb as CompiledVerb) &&
        (s.kind === 'draw'
          ? (!drawBlindsBare(content, s, target) && deckServes(s.deck!, target))
          : s.name === target) &&
        !(s.derives && objectAlways(s.verb)) &&
        !initNames.has(target))
      const anyDerived = live.some(s =>
        s.kind === 'draw' ? drawBlindsBare(content, s, target)
          : (s.derives && s.name === target && objectAlways(s.verb)))
      if (!derivedOnly && anyDerived) {
        existence = 'candidate'
        notes.push(`'${target}' derives its name from the bound object at every commit (the verb's object slot is mandatory) — normally the bare name never mints, but a mid-window death of the bound object drops the binding at completion and the derive silently no-ops`)
      }
    }
    // derived mints (`deriving name from object` → `${PROTO}_${OBJECT}`, verified
    // C4) can mint names we cannot enumerate — their shape must never be called
    // structurally absent
    const derivedPattern = [...content.prototypes.entries()]
      .some(([pname, p]) => p.derive?.nameFrom === 'object' && target.startsWith(`${pname}_`))
    if (existence === 'none' && derivedPattern) {
      existence = 'candidate'
      notes.push(`'${target}' matches a derived-mint shape (deriving name from object) — the object half is chosen at play time, so structural absence cannot be proven`)
    }
  }

  // ---------- (b) usability ----------
  const usableVia: string[] = []
  const descriptor = demandMatch
    ?? initNames.get(target)
    ?? (protoOf(target) ? synth(target, protoOf(target)!.kind, protoOf(target)) : undefined)
  if (existence === 'proven' && descriptor === undefined) {
    notes.push(`no static descriptor for '${target}' (minted without a declared prototype?) — usability could not be checked`)
  }
  if (descriptor) {
    for (const verb of content.verbs.values()) {
      if (!committable.has(verb)) continue
      for (const slot of verb.slots) {
        const fits = slot.query ? matchQuery(descriptor, slot.query) : true
        if (fits) usableVia.push(`${verb.name}.${slot.role}${slot.query ? ' (query matches)' : ' (no query — binds anything)'}`)
      }
    }
  }

  let verdict: CardVerdict
  if (existence === 'none') verdict = 'unobtainable'
  else if (existence === 'candidate') verdict = 'unproven'
  else if (opts.usable && usableVia.length === 0) verdict = 'unusable'
  else verdict = 'obtainable'

  if (!demandQ && existence === 'candidate' && chain.length === 0) {
    for (const site of live) {
      if (proven.has(site)) continue
      const serves = site.kind === 'draw' ? deckServes(site.deck!, target) : site.name === target
      if (serves) notes.push(`candidate source: ${site.where} — guarded (${siteReason(site, initNames)})`)
    }
  }

  return {
    target,
    verdict,
    existence,
    requestedUsable: opts.usable ?? false,
    chain,
    usableVia,
    notes,
  }
}

/** the verb binds an object on EVERY commit (its object slot is mandatory) —
 *  so a `deriving name from object` mint never carries the bare name */
function objectAlways(verb: CompiledVerb | undefined): boolean {
  const slot = verb?.slotByRole.get('object')
  return slot !== undefined && slot.min >= 1
}

/** §9.38: a mint's stackability comes from the RESOLVED name+kind — the name
 *  is declared stackable, or its KIND carries a stackable declaration
 *  (content.ts compiles `stackableKinds` from every stackable prototype's
 *  kind; the engine's mintIsStackable recomputes from the same pair). A
 *  prototype-flag-only check mis-models kind-level stacks: seed 81401's P2
 *  {kind idea, stackable} makes EVERY idea mint stack, so t2's recurring
 *  `SPAWN P1` (P1 = a plain idea prototype) never collides with its own
 *  earlier mint — modeling it as a non-stackable recurring minter froze the
 *  clock at firstCross(27) = 810, capped t1's deck at one draw, and hid
 *  D1 order[2] = P3 behind 'unobtainable' (the engine trace shows t1
 *  drawing P2, P2, P3 across three daily crossings). */
function mintStacks(content: CompiledContent, name: string, kind?: string): boolean {
  if (content.stackableNames.has(name)) return true
  const proto = content.prototypes.get(name)
  const resolved = kind ?? proto?.kind
  return resolved !== undefined && content.stackableKinds.has(resolved)
}

/** a DRAW behind an object-mandatory verb never serves the BARE name of a
 *  deck card whose prototype `derives name from object`: the engine routes
 *  the drawn SPAWN through the same derive path as authored SPAWNs, and the
 *  clause's mandatory object binding renames the mint to `${PROTO}_${OBJ}`
 *  (fuzz seed 12320: D1 order[1] was P3, whose prototype derives — every
 *  draw minted `P3_<OBJ>`, so bare 'P3' never existed despite the deck
 *  serving it). Recipe-only: an event DRAW has empty bindings, the derive
 *  no-ops, and the bare name DOES mint. */
function drawBlindsBare(content: CompiledContent, site: RawSite, name: string): boolean {
  return site.kind === 'draw' && site.verb !== undefined && objectAlways(site.verb)
    && content.prototypes.get(name)?.derive?.nameFrom === 'object'
}

/** authored `aspects=` pair on a SPAWN site, parsed — a static mint-time
 *  aspect override; unparseable pairs are the caller's opaque case */
function siteAspects(site: RawSite): Aspects | undefined {
  if (!site.aspectsJson) return undefined
  try { return JSON.parse(site.aspectsJson) as Aspects } catch { return undefined }
}

// ---------- helpers ----------

function flatten(avail: Map<string, CardFact[]>): Entity[] {
  const out: Entity[] = []
  for (const list of avail.values()) {
    for (const f of list) {
      out.push(f.entity)
      // expand multiplicity so count(<query>) thresholds can read it (capped)
      for (let i = 1; i < Math.min(f.count, 32); i++) out.push(f.entity)
    }
  }
  return out
}

/** exists()/name-terms mentioned by a predicate — the bootstrap-gate check.
 *  A `count(name:X) >= k` comparison gates on X's aliveness exactly like
 *  exists(X) does (fuzz seed 2654: use_on.r0 gated `count(name:P3) >= 1`
 *  while its own list SPAWNed the non-stackable P3 — at every commit where
 *  the gate held, SPAWN P3 collided and the draft was refused, so the
 *  list's other mints never survived). The same holds for seeds 11952
 *  (dream.r0 / count(name:P3)), 40507 (study.r0 / count(name:P2)) and
 *  41134 (read.r0 / count(name:P1)); polarity is ignored on purpose — the
 *  check only ever DEMOTES. */
function exprExistsNames(v: ValExpr, acc: Set<string>): void {
  if (v.t === 'countQuery') {
    for (const t of v.query) if (t.f === 'name') acc.add(t.name)
  }
}

function predExistsNames(p: Pred, acc: Set<string>): void {
  switch (p.t) {
    case 'and': case 'or': predExistsNames(p.a, acc); predExistsNames(p.b, acc); break
    case 'not': predExistsNames(p.a, acc); break
    case 'cmp': exprExistsNames(p.l, acc); exprExistsNames(p.r, acc); break
    case 'exists': acc.add(p.name); break
    case 'existsQuery':
      for (const t of p.query) if (t.f === 'name') acc.add(t.name)
      break
    default: break
  }
}

// ---------- world-static predicates (verified B12) ----------
/** does a static-true condition gate this list on a card THE LIST ITSELF
 *  mints (any op)? Post-bootstrap the gate is true and the self-mint collides
 *  with the gated card — and a completion refusal freezes the dice counter
 *  (A4: counters burn on the draft), so the escape roll never comes. The
 *  same holds when one of the list's own mints is REQUIRED ALIVE as a slot
 *  filler (post-bootstrap the filler IS the earlier mint). Soundly
 *  unprovable → demote to candidate. (fuzz seeds 523, 5631) */
function selfGated(content: CompiledContent, site: RawSite, avail: Map<string, CardFact[]>): boolean {
  const gates = new Set<string>()
  for (const p of [...site.path, ...(site.when ? [site.when] : [])]) {
    if (predStatic(p)) predExistsNames(p, gates)
  }
  const mints = site.listGateMints
  if (!mints) return false
  if ([...gates].some(g => mints.has(g))) return true
  // own-list STACKABLE mint behind a gate: the gate reads FALSE at a
  // once-trigger's single firing (nothing has minted X yet, and the pass
  // rolls back so `fired` never lands). Recurring and recipe lists bootstrap
  // on their second application (stackable X persists) — only once-owners
  // are doomed (verification agents D/E follow-up)
  if (site.verb === undefined) {
    const t = content.triggers.find(tt => `trigger ${tt.id}` === site.owner)
    if (t && 'slot' in t && t.once && site.listStackMints && [...gates].some(g => site.listStackMints!.has(g))) return true
  }
  if (site.verb) {
    for (const slot of site.verb.slots) {
      if (slot.min === 0) continue
      for (const facts of avail.values()) {
        for (const f of facts) {
          if (mints.has(f.entity.name) && (slot.query ? matchQuery(f.entity, slot.query) : true)) return true
        }
      }
    }
  }
  return false
}

function siteProven(
  content: CompiledContent,
  site: RawSite,
  avail: Map<string, CardFact[]>,
  committable: Map<CompiledVerb, string[]>,
  initNames: Map<string, Entity>,
  setWrites: Set<string>,
  setSites: { key: string; owner: string; verbName?: string; targetName?: string; targetRole?: string; segs: number[]; shadowed: boolean; refused: boolean; pathNonStatic: boolean; whenPred: Pred | null; ownerSlot: number | null }[],
  destroyedBy: Map<string, Set<string>>,
  frozen: boolean,
  freezeDist: number,
  notes: string[],
  mintSources: Map<string, RawSite[]>,
  fillers: Map<CompiledVerb, Set<string>>,
  dirtyOwners: Set<string>,
  firstCross: (slot: number) => number,
  triggerMintNames: Map<string, Set<string>>,
): string[] | null {
  const cards = flatten(avail)
  if (site.fallback) return null                       // ELSE runs only when earlier arms are false — unprovable
  if (site.killedInList) return null                   // the mint is destroyed later in the same list — never player-visible
  if (site.dupRisk) return null                        // a duplicate non-stackable mint in this list refuses the draft
  if (selfGated(content, site, avail)) return null              // gated on (or filled by) a card the list itself mints — post-bootstrap self-collision
  // throwing ops in the list demote every site in it — EXCEPT a REPLACE whose
  // own name target is provably available and whose death is not scheduled
  // anywhere else: its throw cannot happen at a commit where the target is
  // alive. (A DESTROY hiding in another trigger's body can kill the target
  // first — fuzz seed 5070: t1 DESTROY LEDGER at slot 22, t0 REPLACE LEDGER
  // at slot 34.) Other throwing ops still demote.
  const killers = site.replaceTarget !== undefined ? (destroyedBy.get(site.replaceTarget) ?? new Set<string>()) : new Set<string>()
  const otherKillers = [...killers].filter(o => o !== site.owner)
  const selfSafe = site.kind === 'replace' && site.replaceTarget !== undefined
    && avail.has(site.replaceTarget) && otherKillers.length === 0 && !site.targetKilledInList
  if (site.throwOps - (selfSafe ? 1 : 0) > 0) return null
  if (site.shadowRisk) return null                     // first-match: an earlier recipe may shadow
  if (!pathOpen(site.path, cards)) return null
  // a static gate on exists(X) is only as durable as X's aliveness — if
  // anything DESTROYS/replaces X anywhere, a later crossing may find the
  // gate false (fuzz seed 6028: t1 gates on exists(ROPE); t0 destroys ROPE
  // the evening before the gate's first crossing)
  const gatedNames = new Set<string>()
  for (const p of [...site.path, ...(site.when ? [site.when] : [])]) {
    if (predStatic(p)) predExistsNames(p, gatedNames)
  }
  if ([...gatedNames].some(n => destroyedBy.has(n))) return null
  // gate-timing (fuzz seed 40449): a SLOT trigger's static gate names a card
  // whose only proven producers fire at (or after) the trigger's own first
  // crossing — a same-slot sibling declared LATER fires after this trigger in
  // every shared pass, and a later-slot trigger crosses later — so the gate
  // reads false exactly when this body applies, and a `once` trigger never
  // gets a second chance (seed 40449: t0 once at slot 3 gated
  // `exists(P3)` on P3 minted only by t1, the LATER same-slot sibling — at
  // t0's single firing P3 did not exist yet, and `fired` was already spent).
  // A recipe-timed source keeps the gate satisfiable (the player commits it
  // first), as does an expiry-timed one (the arming SET is player-timed).
  if (site.verb === undefined) {
    const ownerTrig = content.triggers.find(t => `trigger ${t.id}` === site.owner)
    if (ownerTrig && 'slot' in ownerTrig) {
      const myFirstCross = firstCross(ownerTrig.slot)
      const myDecl = content.triggers.findIndex(t => `trigger ${t.id}` === site.owner)
      const deadAtMyFiring = (src: RawSite): boolean => {
        if (src.verb !== undefined) return false            // recipe commit — player-timed
        const t2 = content.triggers.find(tt => `trigger ${tt.id}` === src.owner)
        if (!t2) return true
        if (!('slot' in t2)) return false                   // expiry — arrangeable before the crossing
        if (t2.slot === ownerTrig.slot) {
          const decl2 = content.triggers.findIndex(tt => `trigger ${tt.id}` === src.owner)
          return decl2 > myDecl                             // same slot, later declaration → fires after us
        }
        return firstCross(t2.slot) > myFirstCross           // later crossing → not yet alive
      }
      const gateUnsatisfiableAtFiring = [...gatedNames].some(n =>
        !initNames.has(n) && !site.listGateMints?.has(n) &&
        (mintSources.get(n) ?? []).length > 0 && (mintSources.get(n) ?? []).every(deadAtMyFiring))
      if (gateUnsatisfiableAtFiring) return null
    }
  }
  // gate-source kills a slot filler (fuzz seed 12460): every proven producer
  // of a gate name acquires it through a list that ALSO kills a card this
  // verb's committability proof filled a slot with — the gate can only hold
  // AFTER that acquisition, and the acquisition has destroyed the filler, so
  // no linear play ever has both. (seed 12460: work.r0 gated `exists(P3)`;
  // P3's only source REPLACEd ROPE, one of WORK's two `with` fillers —
  // minting P3 always cost WORK its slot.)
  if (site.verb !== undefined) {
    const F = fillers.get(site.verb)
    if (F && F.size > 0) {
      const killsFiller = (src: RawSite): boolean => [...F].some(y => destroyedBy.get(y)?.has(src.owner))
      const fillerDoomed = [...gatedNames].some(n =>
        !initNames.has(n) &&
        (mintSources.get(n) ?? []).length > 0 && (mintSources.get(n) ?? []).every(killsFiller))
      if (fillerDoomed) return null
    }
  }
  // cross-list trigger-vs-recipe mint collision (fuzz seed 81915): the list
  // mints a non-stackable name X that a slot trigger ALSO mints, and the
  // trigger's first crossing B satisfies B ≤ verb.duration — every window
  // completes at begin+duration ≥ B, so the boundary at B has already fired
  // the trigger (its X-mint is alive) or rolled that boundary pass back
  // entirely; the list's own X-mint then refuses (`name 'X' already in
  // use`) and the completion refusal discards the WHOLE draft. Seed 81915:
  // mansus.r0 mints P3 then P1, while t0 (slot 2, first crossing 60 ≤ 60)
  // mints P1 via `REPLACE ROPE WITH P1` — a 0:0 MANSUS completes at 60
  // before t0, mints P3+P1 and destroys ROPE, so t0's REPLACE throws and the
  // pass rolls back; any later completion collides on P1. P3 never survives
  // a step. (Order-independent: with the sibling X-mint first, it refuses
  // and the later mints never apply.)
  if (site.verb !== undefined && site.listGateMints) {
    for (const [owner2, mints2] of triggerMintNames) {
      if (mints2.size === 0) continue
      const t2 = content.triggers.find(tt => `trigger ${tt.id}` === owner2)
      if (!t2 || !('slot' in t2)) continue
      if (site.verb.duration < firstCross(t2.slot)) continue
      for (const n of mints2) {
        if (site.listGateMints.has(n)) return null
      }
    }
  }
  // frozen-completion rule: with a risky trigger in play the clock freezes at
  // S* — a recipe with duration > 0 completes at window end, and that
  // boundary carries the failing step, rolling the whole draft back (A12).
  // Provable only for duration-0 recipes (they complete at the begin minute,
  // which the player keeps below S*). (fuzz seed 296.)
  if (frozen && site.verb !== undefined && site.verb.duration > 0) return null
  if (site.verb) {
    if (site.when !== null && !(predStatic(site.when) && evalStatic(site.when, cards))) return null
    const sup = committable.get(site.verb)
    if (!sup) return null
    return sup
  }
  // trigger owner
  const owner = site.owner
  const trig = content.triggers.find(t => `trigger ${t.id}` === owner)
  if (!trig) return null
  if ('slot' in trig) {
    const onceNote = trig.once ? ' (once: true — still fires its first crossing)' : ' (every crossing)'
    return [`trigger ${trig.id} fires on slot ${trig.slot}${onceNote} — the clock is unbounded forward`]
  }
  // expiry trigger: the named card must be obtainable AND able to expire —
  // an expiry that nothing can schedule never fires. "expires writable" is
  // TARGETED: a SET(ACTORS, expires) on a character does not expire a gated
  // item (fuzz seed 237) — the SET's target must be the gated card's name,
  // or a role whose slot the gated card's descriptor fits. The witness SET
  // must also be able to PERSIST: a SET behind an authored REFUSE, a
  // permanently-shadowed recipe, or a draft-refusing (dupRisk) list never
  // lands (fuzz seed 41047: the only `SET(ANA, expires)` sat in read.r1,
  // shadowed by the when-less read.r0 whose top-level REFUSE refused every
  // commit — ANA could never expire, so t0's SPAWN P1 was dead). And it
  // must not live in an expiry trigger of the SAME card — that arming is
  // circular (fuzz seed 41247: t0's own body carried the only
  // `SET(ANA, expires)`, so nothing could arm the very first expiry).
  const desc = initNames.get(trig.onExpireOf) ?? avail.get(trig.onExpireOf)?.[0]?.entity
  const selfArm = (owner: string): boolean => {
    const t2 = content.triggers.find(tt => `trigger ${tt.id}` === owner)
    return t2 !== undefined && !('slot' in t2) && t2.onExpireOf === trig.onExpireOf
  }
  const expirable = (desc !== undefined && desc.attrs['expires'] !== undefined)
    || setSites.some(s => (s.key === 'expires' || s.key === 'expiresAt') && (
      !s.refused && !s.shadowed && !dirtyOwners.has(s.owner) && !selfArm(s.owner)
      // the arming SET must actually APPLY: a runtime-gated path (roll/role
      // condition) can be durably false, a frozen world hides owner firings
      // past the freeze boundary, and a duration>0 recipe window completes
      // inside the failing step (verification agent D, IF-gated + post-freeze
      // counterexamples)
      && !s.pathNonStatic
      && !(frozen && (s.verbName
        ? (() => { const v = content.verbs.get(s.verbName); return v !== undefined && v.duration > 0 })()
        : s.ownerSlot !== null && firstCross(s.ownerSlot) >= freezeDist))
      && (!s.verbName || (() => { const v = content.verbs.get(s.verbName!); return v !== undefined && committable.has(v) })())
      && (s.whenPred === null || (predStatic(s.whenPred) && evalStatic(s.whenPred, cards)))
      && (s.targetName === trig.onExpireOf
        || (s.targetRole !== undefined && desc !== undefined && (() => {
          const v = s.verbName ? content.verbs.get(s.verbName) : undefined
          const slot = v?.slotByRole.get(s.targetRole as Role)
          return slot ? (slot.query ? matchQuery(desc, slot.query) : true) : false
        })()))
    ))
  if (desc === undefined || !expirable) return null
  notes.push(`expiry trigger ${trig.id} reaches its body only while a living '${trig.onExpireOf}' can expire`)
  return [`trigger ${trig.id} fires on the expiry of ${trig.onExpireOf} (available: ${initNames.has(trig.onExpireOf) ? 'initial' : 'produced'}; expires writable${initNames.has(trig.onExpireOf) ? '' : ' or minted'})`]
}

function addProduct(
  content: CompiledContent,
  site: RawSite,
  avail: Map<string, CardFact[]>,
  push: (name: string, fact: CardFact) => void,
  synth: (name: string, kind: string, proto?: PrototypeDef) => Entity,
): void {
  if (site.kind === 'draw') {
    const deck = site.deck!
    // capacity-aware: only the first `site.cap` order positions (and the
    // defaultcard once the cursor passes the order's end) are ever served —
    // phantom entries satisfied exists/count gates with draws that never
    // happen (verification agent E, seed 150846)
    const served = deck.order.slice(0, Math.min(site.cap, deck.order.length))
    const counts = new Map<string, number>()
    for (const c of served) counts.set(c.name, (counts.get(c.name) ?? 0) + 1)
    for (const [name, n] of counts) {
      const card = deck.order.find(c => c.name === name)!
      const proto = content.prototypes.get(name)
      const infinite = deck.reset || deck.defaultcard?.name === name
      const pos = deck.order.indexOf(card)
      // a derive-card drawn behind an object-mandatory verb mints under the
      // DERIVED name — register the descriptor under a blinded key so it can
      // still fill slots by query but never satisfies exists/count gates on
      // the bare name (fuzz seed 90936)
      const blind = drawBlindsBare(content, site, name)
      push(blind ? '\u0000derived' : name, {
        entity: synth(blind ? '\u0000derived' : name, card.kind, proto),
        count: infinite ? MANY : n,
        how: `${name} ← deck ${deck.id} order[${pos}]${infinite ? (deck.reset ? ' (reset — periodic)' : ' (defaultcard)') : ''}`,
      })
    }
    if (deck.defaultcard && site.cap > deck.order.length && !counts.has(deck.defaultcard.name)) {
      const proto = content.prototypes.get(deck.defaultcard.name)
      const blind = drawBlindsBare(content, site, deck.defaultcard.name)
      push(blind ? '\u0000derived' : deck.defaultcard.name, {
        entity: synth(blind ? '\u0000derived' : deck.defaultcard.name, deck.defaultcard.kind, proto),
        count: MANY,
        how: `${deck.defaultcard.name} ← deck ${deck.id} defaultcard`,
      })
    }
    return
  }
  const proto = content.prototypes.get(site.name)
  const kind = site.kindOverride ?? proto?.kind ?? 'item'
  // a derive-mint behind an object-mandatory verb never carries the bare
  // name — register it blinded (descriptor intact for slot queries, invisible
  // to exists/count gates on the bare name; fuzz seed 90936: travel_to.r0's
  // SPAWN P2 minted `P2_LAMP`, but the bare-P2 avail fact made study.r0's
  // `count(name:P2) >= 1` gate read true and P3 was called obtainable)
  const blind = site.derives && objectAlways(site.verb)
  // a non-stackable mint holds the name exclusively (§9.14) — one living copy;
  // a KIND-level stackable declaration stacks the mint exactly like a
  // name-level one (§9.38, fuzz seed 81401)
  push(blind ? '\u0000derived' : site.name, {
    entity: synth(blind ? '\u0000derived' : site.name, kind, proto),
    count: mintStacks(content, site.name, site.kindOverride) ? MANY : 1,
    how: `${site.kind === 'replace' ? 'REPLACE' : 'SPAWN'} ${site.name} by ${site.where}${site.kindOverride ? ` (kind overridden to ${site.kindOverride})` : ''}`,
  })
}

function deckServes(deck: CompiledDeck, target: string): boolean {
  return deck.order.some(c => c.name === target) || deck.defaultcard?.name === target
}

/** does `cap` total draws suffice to serve `target`? order positions need
 *  cap > idx; the defaultcard needs the cursor to pass the order's end; reset
 *  only makes the sequence periodic, it does not create draws */
function servedByCap(deck: CompiledDeck, target: string, cap: number): boolean {
  const idx = deck.order.findIndex(c => c.name === target)
  if (idx >= 0) return cap > idx
  if (deck.reset) return false   // reset wraps before the defaultcard branch — it is never reached (seed 74985)
  if (deck.defaultcard?.name !== target) return false
  return cap > deck.order.length
}

function siteChain(content: CompiledContent, site: RawSite, target: string, supports: string[]): string[] {
  const lines: string[] = []
  if (site.kind === 'draw') {
    const deck = site.deck!
    const pos = deck.order.findIndex(c => c.name === target)
    const where = pos >= 0
      ? `deck ${deck.id} serves '${target}' at order position ${pos} (the permutation is seed-fixed at load — pure in seed × deck id)`
      : `deck ${deck.id} serves '${target}' as defaultcard${deck.reset ? ' (reset — periodic)' : ''}`
    lines.push(where)
    if (site.cap !== MANY) lines.push(`  finite draw capacity: ${site.cap} once-trigger draw(s) — only order[0..${site.cap - 1}] are ever served`)
    lines.push(`  ← ${site.where}`)
  } else {
    lines.push(`${site.kind === 'replace' ? 'REPLACE' : 'SPAWN'} mints '${target}' — ${site.where}`)
    if (site.kindOverride) lines.push(`  minted kind overridden to '${site.kindOverride}' (kind: syntax overrides kind, it does not look up by kind)`)
  }
  for (const s of supports) lines.push(`  · ${s}`)
  void content
  return lines
}

function siteReason(site: RawSite, initNames: Map<string, Entity>): string {
  const bits: string[] = []
  if (site.fallback) bits.push('ELSE fallback body')
  if (site.killedInList) bits.push('the mint is destroyed/Replaced later in the same outcome list — it never survives a step')
  if (site.dupRisk) bits.push('this list mints a name that can collide with a living card (duplicate mint or an initial copy) — the refusal would discard the draft')
  if (site.throwOps > 0) bits.push('a throwing op in this list may discard the whole draft at completion')
  if (site.shadowRisk) bits.push('an earlier recipe of the same verb may shadow it (first-match)')
  if (site.verb && !initNames.size) bits.push('verb committability unknown')
  if (site.path.length > 0) bits.push(`${site.path.length} IF condition(s) on the path`)
  if (site.verb && site.when !== null) bits.push('recipe `when` guard')
  return bits.join(', ') || 'unknown guard'
}
