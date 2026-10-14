// Settle helper — v0.5 test adapter. Under parallel verb stations a committed
// action only BEGINS its window; these helpers complete it (a pure advance to
// the last window's end) so outcome-level tests can assert on settled state
// the way the pre-v0.5 engine behaved (now = start + Σdurations — the same
// minute, by construction). Timing-specific behavior (locks, windows,
// completion order) is tested in test/parallel.test.ts WITHOUT this helper.
import type { CompiledContent } from '../src/content/content.js'
import type { Dictionary } from '../src/core/dict.js'
import { executeStep, type ExecResult, type Step } from '../src/engine/engine.js'
import type { State } from '../src/core/types.js'

/** run one step, then a pure advance closing every window it left open.
 *  Records come back chronological ([advance…, begin, complete…, wait]);
 *  a failure anywhere rejects the whole turn — nothing logged. */
export function settleStep(
  content: CompiledContent,
  dict: Dictionary,
  state: State,
  step: Step,
  firstRecordId: number,
  idGen?: () => string,
): ExecResult {
  const first = executeStep(content, dict, state, step, { firstRecordId, idGen })
  if (first.rejected !== null || first.state.windows.size === 0) return first
  const end = Math.max(...[...first.state.windows.values()].map(w => w.end))
  const pass = executeStep(content, dict, first.state, { startMin: end, clauses: [] }, {
    firstRecordId: firstRecordId + first.records.length,
    idGen,
  })
  if (pass.rejected !== null) {
    return { records: [], state, rejected: pass.rejected } // the turn rolls back whole (§9.19)
  }
  return { records: [...first.records, ...pass.records], state: pass.state, rejected: null }
}

/** the BEGIN row of a settled turn — the record the pre-v0.5 tests indexed as
 *  records[0] (under v0.5 a turn may open with a bridge wait or close with a
 *  completion; the begin row is its spine). Falls back to the last record. */
export function act(r: ExecResult): ExecResult['records'][number] {
  return r.records.find(x => x.kind === 'action') ?? r.records[r.records.length - 1]!
}

/** the COMPLETE row of a settled turn — where the recipe's real outcomes
 *  landed (v0.5: BEGIN carries only PROGRESS ticks). Falls back to the last
 *  record (a refused turn has no completion; its REFUSE rides the BEGIN). */
export function done(r: ExecResult): ExecResult['records'][number] {
  return r.records.find(x => x.kind === 'complete') ?? r.records[r.records.length - 1]!
}

/** the refusal reason of a turn — an authored gate (REFUSE on the BEGIN row)
 *  or a completion-time failure (REFUSE on the COMPLETE row, §9.19's
 *  completion analog); null when the turn did not refuse */
export function refusal(r: ExecResult): string | null {
  for (const rec of r.records) {
    const ref = rec.outcomes.find(o => o.op === 'REFUSE')
    if (!ref) continue
    const s = ref.args.find(a => a.t === 'str')
    return s && s.t === 'str' ? s.v : ''
  }
  return null
}
