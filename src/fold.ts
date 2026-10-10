// Fold — spec §7: state = fold(log). Rebuilds projections (entities,
// relations, provenance, clock) by replaying concrete outcomes in order.
import type { Dictionary } from './dict.js'
import type { CompiledContent } from './content.js'
import { applyRecord, type ActionRecord } from './engine.js'
import { cloneState, type State } from './types.js'

export function fold(records: ActionRecord[], base: State, dict: Dictionary, content: CompiledContent): State {
  const state = cloneState(base)
  for (const r of records) applyRecord(r, state, dict, content)
  return state
}

/** undo = re-fold without the tail (spec §7) */
export function foldUpTo(records: ActionRecord[], base: State, dict: Dictionary, content: CompiledContent, uptoExclusive: number): State {
  return fold(records.filter(r => r.id < uptoExclusive), base, dict, content)
}
