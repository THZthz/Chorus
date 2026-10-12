// Sentence AST — mirrors spec §5.1 exactly.
import type { Clock } from './types.js'
import type { Ref } from './ref.js'

export type Role = 'actors' | 'object' | 'with' | 'at' | 'about' | 'to'

export const ROLES: Role[] = ['actors', 'object', 'with', 'at', 'about', 'to']

export interface Clause {
  actors: Ref[]            // unordered set; canonical form is sorted
  at: Ref | null           // pre-verbal in canonical form (§9.10)
  verb: string
  to: Ref | null
  objects: Ref[]
  with: Ref[]
  about: Ref | null
}

export type Arg =
  | { t: 'ref'; ref: Ref }
  | { t: 'role'; role: Role }               // outcome-arg role ref: ACTORS, OBJECT…
  | { t: 'int'; v: number }
  | { t: 'delta'; v: number }               // signed SET value (+1/-2): applied to the current entry (§9.31)
  | { t: 'str'; v: string }
  | { t: 'bool'; v: boolean }
  | { t: 'dur'; v: number }                 // minutes
  | { t: 'key'; v: string }                 // SET key: 'level', 'state.sealed', 'aspect:dread' (§9.31)
  | { t: 'into'; ref: Ref | { role: Role } }
  | { t: 'asId'; id: string }
  | { t: 'pair'; k: string; v: string | number | boolean }

export interface Outcome {
  op: string
  args: Arg[]
}

export interface Sentence {
  kind: 'action' | 'event'
  clock: Clock | null
  trigger: string | null      // event trigger id
  clauses: Clause[]
  outcomes: Outcome[]
}

export function emptyClause(actors: Ref[]): Clause {
  return { actors, at: null, verb: '', to: null, objects: [], with: [], about: null }
}
