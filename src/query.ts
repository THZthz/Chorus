// Slot-query mini-language — spec §4.5. Tests ONE card at a time.
//   kind:text  aspect:secret>=3  aspect:language.latin>=2  state:!sealed  -aspect:winter
//   name:SUSPICION  -name:SUSPICION   (§9.38: exact-case, like name refs/exists)
import type { Aspects, AttrValue, Entity } from './types.js'
import { aspectGet } from './types.js'

export type CmpOp = '>=' | '<=' | '>' | '<' | '=' | '!='

export type QueryTerm =
  | { neg: boolean; f: 'kind'; kind: string }
  | { neg: boolean; f: 'aspect'; path: string; op: CmpOp | null; level: number }
  | { neg: boolean; f: 'state'; key: string; op: CmpOp | '!' ; value: AttrValue | null }
  | { neg: boolean; f: 'name'; name: string }

export type Query = QueryTerm[]

export function parseQuery(src: string): Query {
  return src.trim().split(/\s+/).filter(Boolean).map(parseTerm)
}

function parseTerm(raw: string): QueryTerm {
  let term = raw
  const neg = term.startsWith('-')
  if (neg) term = term.slice(1)
  const colon = term.indexOf(':')
  if (colon === -1) throw new Error(`bad query term '${raw}' (expected kind:/aspect:/state:/name:)`)
  const field = term.slice(0, colon)
  const rest = term.slice(colon + 1)
  switch (field) {
    case 'kind':
      return { neg, f: 'kind', kind: rest }
    case 'name': {
      if (!rest) throw new Error(`bad name term '${raw}' (expected name:<NAME>)`)
      // exact-case, like name refs and exists(NAME) (§9.38). A comparison is
      // meaningless on a name — and the count()/exists() glue-tightening would
      // silently bake `X>=1` in as the NAME (a silent 0), so reject it loudly
      if (/[<>=!]/.test(rest)) {
        throw new Error(`bad name term '${raw}' (name:X is an exact-case match — put the comparison outside: count(name:X) >= 1)`)
      }
      return { neg, f: 'name', name: rest }
    }
    case 'aspect': {
      const m = /^(?:([A-Za-z_][A-Za-z0-9_.]*))?(?:\s*(>=|<=|>|<|!=|=)\s*(-?\d+))?$/.exec(rest)
      if (!m || !m[1]) throw new Error(`bad aspect term '${raw}'`)
      return { neg, f: 'aspect', path: m[1], op: (m[2] as CmpOp | undefined) ?? null, level: m[3] ? Number(m[3]) : 1 }
    }
    case 'state': {
      const m = /^(!)?([A-Za-z_][A-Za-z0-9_.]*)(?:\s*(>=|<=|>|<|!=|=)\s*(.+))?$/.exec(rest)
      if (!m || !m[2]) throw new Error(`bad state term '${raw}'`)
      if (m[1]) return { neg, f: 'state', key: m[2], op: '!', value: null }
      return { neg, f: 'state', key: m[2], op: (m[3] as CmpOp | undefined) ?? '=', value: coerce(m[4] ?? 'true') }
    }
    default:
      throw new Error(`bad query term '${raw}' (expected kind:/aspect:/state:/name:)`)
  }
}

function coerce(v: string): AttrValue {
  if (v === 'true') return true
  if (v === 'false') return false
  if (/^-?\d+$/.test(v)) return Number(v)
  return v
}

export function matchQuery(e: Entity, query: Query): boolean {
  return query.every(t => matchTerm(e, t) !== t.neg)
}

function matchTerm(e: Entity, t: QueryTerm): boolean {
  switch (t.f) {
    case 'kind':
      return e.kind === t.kind
    case 'aspect': {
      const v = aspectGet(e, t.path)
      if (v === undefined) return false
      return t.op === null ? v >= t.level : numCmp(v, t.op, t.level)
    }
    case 'state': {
      // `state:key` addresses the state.* attr namespace (§4.2): state:!sealed → attrs['state.sealed']
      const v = e.attrs[`state.${t.key}`]
      if (t.op === '!') return v !== true
      if (v === undefined) return false
      if (typeof v === 'number' && typeof t.value === 'number') return numCmp(v, t.op as CmpOp, t.value)
      return t.op === '=' ? v === t.value : t.op === '!=' ? v !== t.value : false
    }
    case 'name':
      return e.name === t.name
  }
}

function numCmp(a: number, op: CmpOp, b: number): boolean {
  switch (op) {
    case '>=': return a >= b
    case '<=': return a <= b
    case '>': return a > b
    case '<': return a < b
    case '=': return a === b
    case '!=': return a !== b
  }
}

export function aspectsFromPairs(pairs: Record<string, number>): Aspects {
  const out: Aspects = {}
  for (const [k, v] of Object.entries(pairs)) {
    const dot = k.indexOf('.')
    if (dot === -1) out[k] = v
    else {
      const head = k.slice(0, dot)
      const sub = (typeof out[head] === 'object' ? out[head] : (out[head] = {})) as Record<string, number>
      sub[k.slice(dot + 1)] = v
    }
  }
  return out
}
