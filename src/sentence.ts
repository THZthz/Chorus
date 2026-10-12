// Sentence AST — mirrors spec §5.1 exactly.
import type { Clock } from './types.js'
import type { Ref } from './ref.js'
import type { Pred } from './predicate.js'

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

/** one IF/ELSE IF arm (§9.39): a condition and its paren'd outcome body */
export interface IfArm {
  when: Pred
  then: Outcome[]
}

export interface Outcome {
  op: string
  args: Arg[]
  /** op 'IF' only (§9.39): the arms in source order, ELSE IF chain consumed
   *  greedily INSIDE the node (the `,`-loop must not strand an ELSE; a nested
   *  ELSE binds to the nearest IF). Authored form only — evaluation flattens
   *  the TAKEN branch's outcomes into the record position; the log never
   *  contains an IF op. */
  arms?: IfArm[]
  /** the ELSE body, or null when authored without one (no true arm → nothing) */
  fallback?: Outcome[] | null
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
