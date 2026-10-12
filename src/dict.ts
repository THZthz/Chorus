// L1 Dictionary — spec §4, §9.14 (reserved words), §4.4 (normative outcome arities)

export const CORE_VERBS = [
  'READ', 'WORK', 'TALK', 'EXPLORE', 'TRAVEL_TO',
  'LOCATE_AT', 'DREAM', 'USE_ON', 'ACQUIRE', 'WAIT',
] as const

/** noun classes — spec §4.1 */
export const KINDS = ['character', 'place', 'item', 'text', 'idea', 'state', 'rite', 'resource'] as const

export const RESERVED_WORDS = [
  'EVENT', 'TO', 'WITH', 'AT', 'ABOUT', 'INTO', 'AS', 'AND', 'OR', 'NOT',
  ...CORE_VERBS,
]

export type OutcomeOp =
  | 'SPAWN' | 'DESTROY' | 'REPLACE' | 'SET' | 'MOVE' | 'REVEAL'
  | 'ADVANCE' | 'NARRATE' | 'PROGRESS' | 'RELATE' | 'UNRELATE' | 'REFUSE'
  | 'ENDING'

export const OUTCOME_OPS: OutcomeOp[] = [
  'SPAWN', 'DESTROY', 'REPLACE', 'SET', 'MOVE', 'REVEAL',
  'ADVANCE', 'NARRATE', 'PROGRESS', 'RELATE', 'UNRELATE', 'REFUSE',
  'ENDING',
]

export interface RelationTypeDef {
  symmetric: boolean
}

export class Dictionary {
  readonly verbs = new Set<string>()
  readonly relationTypes = new Map<string, RelationTypeDef>()
  readonly reserved = new Set<string>(RESERVED_WORDS)

  registerVerb(lexeme: string): void {
    this.assertNameable(lexeme, 'verb')
    this.verbs.add(lexeme)
    this.reserved.add(lexeme)
  }

  registerRelation(type: string, def: RelationTypeDef = { symmetric: true }): void {
    this.relationTypes.set(type, def)
  }

  isVerb(tok: string): boolean {
    return this.verbs.has(tok)
  }

  isOp(tok: string): tok is OutcomeOp {
    return (OUTCOME_OPS as string[]).includes(tok)
  }

  relationDef(type: string): RelationTypeDef {
    const def = this.relationTypes.get(type)
    if (!def) throw new Error(`unknown relation type '${type}' (register it in the dictionary)`)
    return def
  }

  /** reserved words cannot be registered as names/verbs — spec §5.1, §9.14 */
  private assertNameable(lexeme: string, what: string): void {
    if (!/^[A-Z][A-Z0-9_]*$/.test(lexeme)) {
      throw new Error(`${what} '${lexeme}' must be UPPER_SNAKE`)
    }
    if (this.verbs.has(lexeme)) throw new Error(`${what} '${lexeme}' collides with a verb`)
  }
}

export function coreDictionary(): Dictionary {
  const d = new Dictionary()
  for (const v of CORE_VERBS) d.registerVerb(v)
  d.registerRelation('parent_child', { symmetric: true })
  d.registerRelation('parent_of', { symmetric: false })
  d.registerRelation('neighbor', { symmetric: true })
  // the four directions of the table: `A is south of B` — stored edges keep
  // their authored direction, so a layout can be read back from the log
  d.registerRelation('north', { symmetric: false })
  d.registerRelation('south', { symmetric: false })
  d.registerRelation('east', { symmetric: false })
  d.registerRelation('west', { symmetric: false })
  return d
}
