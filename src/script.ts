// Script compiler — a game script is a piece of text: meta lines, section
// blocks, and sentence-like declarations (spec: "sentences + small blocks").
//
//   script: The Forgotten House
//   player: ANA
//   goal:   what the player is trying to do, shown up front
//   start: 0:14
//
//   entities:
//     HOUSE is place
//     ANA is character with occult:2, lantern:1 at HOUSE
//     STUDY is place in HOUSE with study:1
//     ANA is parent_child of BERTHA
//
//   prototypes:
//     CLUE is idea deriving name from object, secret from object.secret-2
//
//   triggers:
//     dusk_falls at hour 16 -> SPAWN DREAD, NARRATE "The lamps come on."
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
import type { ContentRaw, PrototypeDef, RecipeDef, SlotDef, TriggerDef, VerbContent } from './content.js'
import { compileContent, type CompiledContent } from './content.js'
import { parseClock, absMin, parseDuration, type Clock } from './clock.js'
import { parseOutcomes } from './parse.js'
import { aspectsFromPairs } from './query.js'
import { addEntity, relationKey, newState, type Aspects, type State } from './types.js'

type Section = 'entities' | 'prototypes' | 'triggers' | 'verbs' | 'templates'

export interface ScriptMeta {
  title?: string
  player?: string
  goal?: string
  start?: Clock
}

export interface EntitySpec {
  name: string
  kind: string
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
  const verbs: Record<string, VerbContent> = {}

  const lines = text.split('\n')
  let section: Section | null = null
  let verbName: string | null = null
  let inRecipes = false

  for (let ln = 0; ln < lines.length; ln++) {
    const rawLine = lines[ln]
    const line = rawLine.replace(/\t/g, '  ')
    if (!line.trim() || line.trim().startsWith('#')) continue
    const indent = line.length - line.trimStart().length
    const t = line.trim()

    // meta lines (indent 0, outside sections)
    if (indent === 0 && section === null) {
      const m = /^(script|player|goal|start):\s*(.+)$/.exec(t)
      if (m) {
        if (m[1] === 'script') meta.title = m[2].trim()
        else if (m[1] === 'player') meta.player = m[2].trim()
        else if (m[1] === 'goal') meta.goal = m[2].trim()
        else meta.start = parseClock(m[2].trim())
        continue
      }
    }

    // section headers (indent 0)
    if (indent === 0 && /^(entities|prototypes|triggers|verbs|templates):$/.test(t)) {
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

  return { meta, content: { verbs, triggers, prototypes, templates }, entities, relations }
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
  entities.push({ name, kind: second, aspects, containerName })
}

// ---------- prototype lines ----------

function parsePrototypeLine(t: string, ln: number, dict: Dictionary): [string, PrototypeDef] {
  const m = /^(\S+)\s+is\s+([a-z]+)\b\s*(.*)$/.exec(t)
  if (!m) throw new Error(`line ${ln}: prototypes expect  NAME is KIND [deriving ...] [with ...]`)
  const proto: PrototypeDef = { kind: m[2] }
  if (!(KINDS as readonly string[]).includes(m[2])) throw new Error(`line ${ln}: unknown kind '${m[2]}'`)
  let rest = m[3]
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
  const m = /^(\S+)(\s+once)?\s+at\s+(hour|halfhour)\s+(\d+)\s*->\s*(.+)$/.exec(t)
  if (!m) throw new Error(`line ${ln}: triggers expect  ID [once] at (hour|halfhour) N -> OUTCOMES`)
  const n = Number(m[4])
  if (m[3] === 'hour' && n > 23) throw new Error(`line ${ln}: trigger hour must be 0..23`)
  if (m[3] === 'halfhour' && n > 47) throw new Error(`line ${ln}: trigger halfhour must be 0..47`)
  return { id: m[1], slot: m[3] === 'hour' ? n * 2 : n, once: m[2] !== undefined, out: [m[5]] }
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
  const content = compileContent(script.content, dict)
  const state = newState(script.meta.start ? absMin(script.meta.start) : 0)

  const idOf = (name: string): string => `s_${name.toLowerCase()}`
  for (const e of script.entities) {
    addEntity(state, {
      id: idOf(e.name), kind: e.kind, name: e.name,
      aspects: e.aspects, attrs: {}, bornAction: 0, deadAction: null,
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
