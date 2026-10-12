// Script compiler — a game script is a piece of text: meta lines, section
// blocks, and sentence-like declarations (spec: "sentences + small blocks").
//
//   script: The Forgotten House
//   player: ANA
//   goal:   what the player is trying to do, shown up front
//   start: 0:14
//   seed:   7                                # dice seed (§9.41, default 0)
//
//   entities:
//     HOUSE is place
//     ANA is character with occult:2, lantern:1 at HOUSE
//     THE_STUDY is place in HOUSE with study:1
//     ANA is parent_child of BERTHA
//
//   prototypes:
//     CLUE is idea deriving name from object, secret from object.secret-2
//
//   triggers:
//     dusk_falls at hour 16 -> SPAWN DREAD, NARRATE "The lamps come on."
//
//   endings:
//     DESPAIR_DEATH: "No More" (melancholy)   # flavour optional, default melancholy
//     LANTERN_VICTORY: "The Light in the Ruins" (grand)
//
//   verbs:
//     READ:
//       actors: 1..2 kind:character
//       duration: 1h
//       recipes:
//         read.secret when object.aspect.secret >= 3 -> SPAWN CLUE
//         default -> NARRATE "{actors} read {object}."
//
//   templates:
//     READ says "{actors} read {object} at {at}."
import type { Dictionary } from './dict.js'
import { KINDS } from './dict.js'
import type { ContentRaw, EndingDef, FunctionDef, PrototypeDef, RecipeDef, SlotDef, TriggerDef, VerbContent } from './content.js'
import { compileContent, type CompiledContent } from './content.js'
import { parseClock, absMin, parseDuration, type Clock } from './clock.js'
import { parseOutcomes } from './parse.js'
import { aspectsFromPairs } from './query.js'
import { addEntity, relationKey, newState, type Aspects, type State } from './types.js'

type Section = 'entities' | 'prototypes' | 'triggers' | 'endings' | 'verbs' | 'templates' | 'functions'

export interface ScriptMeta {
  title?: string
  player?: string
  goal?: string
  start?: Clock
  /** §9.41: the dice seed (default 0) — the run's whole dice sequence is a
   *  pure function of (seed, rng counter) */
  seed?: number
}

export interface EntitySpec {
  name: string
  kind: string
  /** §9.38: `NAME is KIND stackable …` — this line's card may share its name */
  stackable?: boolean
  aspects: Aspects
  containerName?: string
}

export interface RelationSpec {
  a: string
  type: string
  b: string
}

export interface Script {
  meta: ScriptMeta
  content: ContentRaw
  entities: EntitySpec[]
  relations: RelationSpec[]
}

export function parseScript(text: string, dict: Dictionary): Script {
  const meta: ScriptMeta = {}
  const entities: EntitySpec[] = []
  const relations: RelationSpec[] = []
  const prototypes: Record<string, PrototypeDef> = {}
  const triggers: TriggerDef[] = []
  const templates: Record<string, string> = {}
  const endings: Record<string, EndingDef> = {}
  const verbs: Record<string, VerbContent> = {}

  const lines = text.split('\n')
  let section: Section | null = null
  let verbName: string | null = null
  let inRecipes = false
  const functions: FunctionDef[] = []

  for (let ln = 0; ln < lines.length; ln++) {
    const rawLine = lines[ln]
    const line = rawLine.replace(/\t/g, '  ')
    if (!line.trim() || line.trim().startsWith('#')) continue
    const indent = line.length - line.trimStart().length
    const t = line.trim()

    // meta lines (indent 0, outside sections)
    if (indent === 0 && section === null) {
      const m = /^(script|player|goal|start|seed):\s*(.+)$/.exec(t)
      if (m) {
        if (m[1] === 'script') meta.title = m[2].trim()
        else if (m[1] === 'player') meta.player = m[2].trim()
        else if (m[1] === 'goal') meta.goal = m[2].trim()
        else if (m[1] === 'seed') {
          // §9.41: integer seed, XORed with the rng counter as a 64-bit value
          if (!/^-?\d+$/.test(m[2].trim())) throw new Error(`line ${ln + 1}: seed must be an integer, got '${m[2].trim()}'`)
          meta.seed = Number(m[2].trim())
        }
        else meta.start = parseClock(m[2].trim())
        continue
      }
    }

    // section headers (indent 0); free-form order — compileContent fixes the
    // compile order (functions first), so `functions:` may sit anywhere
    if (indent === 0 && /^(entities|prototypes|triggers|endings|verbs|templates|functions):$/.test(t)) {
      section = t.slice(0, -1) as Section
      verbName = null
      inRecipes = false
      continue
    }
    if (section === null) throw new Error(`line ${ln + 1}: expected a section header, got '${t}'`)

    if (section === 'entities') {
      parseEntityLine(t, ln + 1, dict, entities, relations)
    } else if (section === 'prototypes') {
      const [key, proto] = parsePrototypeLine(t, ln + 1, dict)
      prototypes[key] = proto
    } else if (section === 'triggers') {
      triggers.push(parseTriggerLine(t, ln + 1, dict))
    } else if (section === 'functions') {
      functions.push(parseFunctionLine(t, ln + 1))
    } else if (section === 'endings') {
      const [id, ending] = parseEndingLine(t, ln + 1)
      if (endings[id]) throw new Error(`line ${ln + 1}: duplicate ending id '${id}'`)
      endings[id] = ending
    } else if (section === 'templates') {
      const m = /^([A-Z_]+) says "(.*)"$/.exec(t)
      if (!m) throw new Error(`line ${ln + 1}: templates expect  VERB says "..."`)
      templates[m[1]] = m[2]
    } else if (section === 'verbs') {
      // verb header: indent 2, UPPER_NAME:
      if (indent === 2 && /^[A-Z][A-Z0-9_]*:$/.test(t)) {
        verbName = t.slice(0, -1)
        if (!dict.isVerb(verbName)) throw new Error(`line ${ln + 1}: verb '${verbName}' is not registered in the dictionary`)
        verbs[verbName] = { slots: [], duration: 0, recipes: [] }
        inRecipes = false
        continue
      }
      if (!verbName) throw new Error(`line ${ln + 1}: expected a verb header inside 'verbs:'`)
      const v = verbs[verbName]!
      if (indent === 4 && /^recipes:$/.test(t)) { inRecipes = true; continue }
      if (!inRecipes && indent >= 4) {
        const m = /^([a-z]+):\s*(.+)$/.exec(t)
        if (!m) throw new Error(`line ${ln + 1}: expected 'role: count query', 'duration: X', or 'recipes:'`)
        if (m[1] === 'duration') { v.duration = parseDuration(m[2].trim()); continue }
        v.slots.push(parseSlotLine(m[1], m[2], ln + 1))
      } else if (inRecipes && indent >= 6) {
        v.recipes.push(parseRecipeLine(t, ln + 1, dict))
      } else {
        throw new Error(`line ${ln + 1}: unexpected line in verb ${verbName}: '${t}'`)
      }
    }
  }

  return { meta, content: { verbs, triggers, prototypes, templates, endings, functions }, entities, relations }
}

// ---------- entity lines ----------

function parseEntityLine(t: string, ln: number, dict: Dictionary, entities: EntitySpec[], relations: RelationSpec[]): void {
  const m = /^(\S+)\s+is\s+([A-Za-z_.]+)\b\s*(.*)$/.exec(t)
  if (!m) throw new Error(`line ${ln}: entities expect  NAME is KIND [with ...] [at PLACE]`)
  const [, name, second, rest0] = m
  // relation form: A is RELTYPE of B
  if (dict.relationTypes.has(second)) {
    const rel = /^of\s+(\S+)$/.exec(rest0)
    if (!rel) throw new Error(`line ${ln}: relations expect  A is TYPE of B`)
    relations.push({ a: name, type: second, b: rel[1] })
    return
  }
  if (!(KINDS as readonly string[]).includes(second)) {
    throw new Error(`line ${ln}: unknown kind '${second}' (expected one of ${KINDS.join(', ')})`)
  }
  let rest = rest0
  // §9.38: the `stackable` keyword sits before `with`/`at`/`in` clauses
  let stackable = false
  const stackM = /^stackable\b\s*/.exec(rest)
  if (stackM) { stackable = true; rest = rest.slice(stackM[0].length) }
  // trailing clauses, in either order: "with specs" and "at/in PLACE" — strip whichever matches last
  let containerName: string | undefined
  let aspects: Aspects = {}
  for (;;) {
    const atIn = /(?:^|\s+)(?:at|in)\s+(\S+)\s*$/.exec(rest)
    const withM = /(?:^|\s+)with\s+(.+)$/.exec(rest)
    if (atIn && (!withM || atIn.index > withM.index)) {
      containerName = atIn[1]
      rest = rest.slice(0, atIn.index)
      continue
    }
    if (withM) {
      const pairs: Record<string, number> = {}
      for (const spec of withM[1].split(',')) {
        const sm = /^\s*([A-Za-z_][A-Za-z0-9_.]*):(-?\d+)\s*$/.exec(spec)
        if (!sm) throw new Error(`line ${ln}: bad aspect spec '${spec.trim()}' (expected aspect:level)`)
        pairs[sm[1]] = Number(sm[2])
      }
      aspects = aspectsFromPairs(pairs)
      rest = rest.slice(0, withM.index)
      continue
    }
    break
  }
  if (rest.trim() !== '') throw new Error(`line ${ln}: could not parse entity line '${t}'`)
  entities.push({ name, kind: second, stackable, aspects, containerName })
}

// ---------- prototype lines ----------

function parsePrototypeLine(t: string, ln: number, dict: Dictionary): [string, PrototypeDef] {
  const m = /^(\S+)\s+is\s+([a-z]+)\b\s*(.*)$/.exec(t)
  if (!m) throw new Error(`line ${ln}: prototypes expect  NAME is KIND [stackable] [deriving ...] [with ...]`)
  const proto: PrototypeDef = { kind: m[2] }
  if (!(KINDS as readonly string[]).includes(m[2])) throw new Error(`line ${ln}: unknown kind '${m[2]}'`)
  let rest = m[3]
  // §9.38: the `stackable` keyword sits before `deriving`/`with` clauses
  const stackM = /^stackable\b\s*/.exec(rest)
  if (stackM) { proto.stackable = true; rest = rest.slice(stackM[0].length) }
  let aspectsDone = false
  let deriveDone = false
  for (;;) {
    const withM = /(?:^|\s+)with\s+(.+)$/.exec(rest)
    const derivingM = /(?:^|\s+)deriving\s+(.+)$/.exec(rest)
    if (withM && (!derivingM || withM.index > derivingM.index)) {
      if (aspectsDone) throw new Error(`line ${ln}: duplicate 'with' clause`)
      aspectsDone = true
      const pairs: Record<string, number> = {}
      for (const spec of withM[1].split(',')) {
        const sm = /^\s*([A-Za-z_][A-Za-z0-9_.]*):(-?\d+)\s*$/.exec(spec)
        if (!sm) throw new Error(`line ${ln}: bad aspect spec '${spec.trim()}'`)
        pairs[sm[1]] = Number(sm[2])
      }
      proto.aspects = aspectsFromPairs(pairs)
      rest = rest.slice(0, withM.index)
      continue
    }
    if (derivingM) {
      if (deriveDone) throw new Error(`line ${ln}: duplicate 'deriving' clause`)
      deriveDone = true
      rest = rest.slice(0, derivingM.index)
      proto.derive = {}
      for (const item of derivingM[1].split(',')) {
        const nameFrom = /^\s*name\s+from\s+object\s*$/.exec(item)
        if (nameFrom) { proto.derive.nameFrom = 'object'; continue }
        const dm = /^\s*([A-Za-z_][A-Za-z0-9_.]*)\s+from\s+object\.([A-Za-z_][A-Za-z0-9_.]*)(?:\s*-\s*(\d+))?\s*$/.exec(item)
        if (!dm) throw new Error(`line ${ln}: bad derive '${item.trim()}' (expected 'name from object' or 'field from object.field-2')`)
        proto.derive.aspects = proto.derive.aspects ?? {}
        proto.derive.aspects[dm[1]] = { fromObjectAspect: dm[2], minus: dm[3] ? Number(dm[3]) : 0 }
      }
      continue
    }
    break
  }
  if (rest.trim() !== '') throw new Error(`line ${ln}: could not parse prototype line '${t}'`)
  return [m[1], proto]
}

// ---------- trigger lines ----------

function parseTriggerLine(t: string, ln: number, dict: Dictionary): TriggerDef {
  // expiry form: ID on expire of NAME -> OUTCOMES (§6.6, §9.32)
  const exp = /^(\S+)\s+on\s+expire\s+of\s+(\S+)\s*->\s*(.+)$/.exec(t)
  if (exp) return { id: exp[1], onExpireOf: exp[2], out: [exp[3]] }
  const m = /^(\S+)(\s+once)?\s+at\s+(hour|halfhour)\s+(\d+)\s*->\s*(.+)$/.exec(t)
  if (!m) throw new Error(`line ${ln}: triggers expect  ID [once] at (hour|halfhour) N -> OUTCOMES  or  ID on expire of NAME -> OUTCOMES`)
  const n = Number(m[4])
  if (m[3] === 'hour' && n > 23) throw new Error(`line ${ln}: trigger hour must be 0..23`)
  if (m[3] === 'halfhour' && n > 47) throw new Error(`line ${ln}: trigger halfhour must be 0..47`)
  return { id: m[1], slot: m[3] === 'hour' ? n * 2 : n, once: m[2] !== undefined, out: [m[5]] }
}

// ---------- function lines (§9.40, §10) ----------

/** functions:  NAME(param, …) -> OUTCOMES  (or the bare form NAME param …,
 *  or NAME with none — the plan's flagship uses both spellings). Grammar only:
 *  name/param shape, op collisions and duplicates are compileContent's. */
function parseFunctionLine(t: string, ln: number): FunctionDef {
  let m = /^(\S+)\s*\(([^)]*)\)\s*->\s*(.+)$/.exec(t) // paren'd, comma-separated params
  if (m) {
    return { name: m[1], params: m[2].trim() === '' ? [] : m[2].split(',').map(s => s.trim()), body: m[3].trim() }
  }
  m = /^(\S+)\s*->\s*(.+)$/.exec(t) // no params, no parens
  if (m) return { name: m[1], params: [], body: m[2].trim() }
  m = /^(\S+)\s+(\S.*?)\s*->\s*(.+)$/.exec(t) // bare, space-separated params
  if (m) return { name: m[1], params: m[2].split(/\s+/), body: m[3].trim() }
  throw new Error(`line ${ln}: functions expect  NAME(param, …) -> OUTCOMES  or  NAME param … -> OUTCOMES`)
}

// ---------- ending lines (§9.35, §10) ----------

function parseEndingLine(t: string, ln: number): [string, EndingDef] {
  // ID: "Title" (flavour) — flavour optional, default melancholy (resolved at compile)
  const m = /^([A-Z][A-Z0-9_]*):\s*"(.+)"(?:\s+\(([a-z]+)\))?$/.exec(t)
  if (!m) {
    throw new Error(`line ${ln}: endings expect  ID: "Title" (flavour)  — flavour optional, default melancholy`)
  }
  return [m[1], { title: m[2], flavour: m[3] }]
}

// ---------- slots & recipes ----------

function parseSlotLine(role: string, rest: string, ln: number): SlotDef {
  const m = /^(\S+)(?:\s+(.*))?$/.exec(rest.trim())
  if (!m) throw new Error(`line ${ln}: bad slot '${role}: ${rest}'`)
  const countSpec = m[1]
  const cm = /^(\d+)(?:\.\.(\d+)?)?$/.exec(countSpec)
  if (!cm) throw new Error(`line ${ln}: bad count '${countSpec}' (expected 1, 1..2, 2.., 0..1)`)
  const min = Number(cm[1])
  const max = cm[2] !== undefined ? Number(cm[2]) : (cm[1] !== undefined && countSpec.endsWith('..') ? Infinity : min)
  return { role: role as SlotDef['role'], min, max, query: m[2] }
}

function parseRecipeLine(t: string, ln: number, dict: Dictionary): RecipeDef {
  const arrow = t.indexOf('->')
  if (arrow === -1) throw new Error(`line ${ln}: recipes expect  [id] [hidden|shown] [hint "..."] [when COND] -> OUTCOMES`)
  const lhs = t.slice(0, arrow).trim()
  const rhs = t.slice(arrow + 2).trim()
  const def: RecipeDef = { out: [rhs] }
  if (/^default\b/.test(lhs)) return def
  const tokens = lhs.split(/\s+/)
  let i = 0
  if (i < tokens.length && /^[a-z0-9_.]+$/.test(tokens[i]) && tokens[i] !== 'when' && tokens[i] !== 'hidden' && tokens[i] !== 'shown') {
    def.id = tokens[i]; i++
  }
  if (i < tokens.length && (tokens[i] === 'hidden' || tokens[i] === 'shown')) { def.shown = tokens[i] === 'shown'; i++ }
  const hintM = lhs.match(/hint\s+"([^"]*)"/)
  if (hintM) { def.hint = hintM[1]; i = i } // hint extracted positionally; consumes nothing from tokens
  const whenM = /(?:^|\s)when\s+(.+)$/.exec(lhs)
  if (whenM) def.when = whenM[1]
  void i
  return def
}

// ---------- compile: script → runnable game ----------

export function compileScript(text: string, dict: Dictionary): { content: CompiledContent; state: State; meta: ScriptMeta } {
  const script = parseScript(text, dict)
  // §9.41: the meta `seed:` rides into the compiled content — every die is a
  // pure function of (seed, counter), so the sequence is fixed at load
  const content = compileContent({ ...script.content, seed: script.meta.seed }, dict)
  // §9.38: stackable entity-line declarations join the prototype-derived sets,
  // so SPAWN of such a name (and kind-qualified mints of its kind) stack
  for (const e of script.entities) {
    if (e.stackable) { content.stackableNames.add(e.name); content.stackableKinds.add(e.kind) }
  }
  const state = newState(script.meta.start ? absMin(script.meta.start) : 0)

  const idOf = (name: string): string => `s_${name.toLowerCase()}`
  for (const e of script.entities) {
    addEntity(state, {
      id: idOf(e.name), kind: e.kind, name: e.name,
      aspects: e.aspects, attrs: {}, bornAction: 0, deadAction: null,
      stackable: e.stackable ?? false,
    })
  }
  for (const e of script.entities) {
    if (e.containerName) {
      const container = state.entities.get(idOf(e.containerName))
      const self = state.entities.get(idOf(e.name))!
      if (container) self.attrs['container'] = container.id
      else if (e.kind === 'place') self.attrs['zone'] = e.containerName // a zone: a grouping with no card (§10)
      else throw new Error(`entity ${e.name}: unknown container '${e.containerName}'`)
    }
  }
  for (const rel of script.relations) {
    const a = state.entities.get(idOf(rel.a)), b = state.entities.get(idOf(rel.b))
    if (!a || !b) throw new Error(`relation ${rel.a} is ${rel.type} of ${rel.b}: unknown entity`)
    const def = dict.relationDef(rel.type)
    state.relations.set(relationKey(a.id, b.id, rel.type, def.symmetric), {
      a: a.id, b: b.id, type: rel.type, value: 1, bornAction: 0, deadAction: null,
    })
  }
  return { content, state, meta: script.meta }
}
