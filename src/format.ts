// Canonical serializer — spec §5.1, §9.6, §9.10, §9.14, §9.15.
// Format(parse(x)) is the canonical form: actors sorted, pre-verbal AT,
// bare-name refs (re-quoted when unsafe/reserved), concrete outcomes.
import type { Arg, Clause, Outcome, Sentence } from './sentence.js'
import { formatRef, sortRefs, type SafeDict } from './ref.js'
import { formatClock, formatDuration } from './clock.js'

export function formatSentence(s: Sentence, dict?: SafeDict): string {
  const parts: string[] = []
  if (s.clock) parts.push(`${formatClock(s.clock)} @`)
  if (s.kind === 'event') {
    parts.push('EVENT', s.trigger ?? '?')
  } else {
    parts.push(s.clauses.map(c => formatClause(c, dict)).join('; '))
  }
  if (s.outcomes.length > 0) {
    parts.push('->', s.outcomes.map(o => formatOutcome(o, dict)).join(', '))
  }
  return parts.join(' ')
}

function formatClause(c: Clause, dict?: SafeDict): string {
  const parts: string[] = [sortRefs(c.actors).map(r => formatRef(r, dict)).join(', ')]
  if (c.at) parts.push('AT', formatRef(c.at, dict))
  parts.push(c.verb)
  if (c.to) parts.push('TO', formatRef(c.to, dict))
  if (c.objects.length > 0) parts.push(sortRefs(c.objects).map(r => formatRef(r, dict)).join(', '))
  if (c.with.length > 0) parts.push('WITH', sortRefs(c.with).map(r => formatRef(r, dict)).join(', '))
  if (c.about) parts.push('ABOUT', formatRef(c.about, dict))
  return parts.join(' ')
}

function formatOutcome(o: Outcome, dict?: SafeDict): string {
  if (o.op === 'SPAWN') {
    // pretty form: SPAWN X [INTO Y] [AS #id]
    const parts = ['SPAWN']
    for (const a of o.args) parts.push(formatArg(a, dict))
    return parts.join(' ')
  }
  if (o.op === 'NARRATE') {
    const s = o.args[0]
    return `NARRATE ${s && s.t === 'str' ? quote(s.v) : formatArgs(o.args, dict)}`
  }
  return `${o.op}(${o.args.map(a => formatArg(a, dict)).join(', ')})`
}

function formatArgs(args: Arg[], dict?: SafeDict): string {
  return args.map(a => formatArg(a, dict)).join(', ')
}

function formatArg(a: Arg, dict?: SafeDict): string {
  switch (a.t) {
    case 'ref': return formatRef(a.ref, dict)
    case 'role': return a.role.toUpperCase()
    case 'int': return String(a.v)
    case 'delta': return a.v < 0 ? String(a.v) : `+${a.v}` // signed args print with their sign (§9.31)
    case 'str': return quote(a.v)
    case 'bool': return String(a.v)
    case 'dur': return formatDuration(a.v)
    case 'key': return a.v // plain, dotted and aspect:-prefixed keys print bare (§9.31)
    case 'into': return 'INTO ' + ('role' in a.ref ? a.ref.role.toUpperCase() : formatRef(a.ref, dict))
    case 'asId': return `AS #${a.id}`
    case 'pair': return `${a.k}=${typeof a.v === 'string' ? quote(a.v) : String(a.v)}`
  }
}

function quote(s: string): string {
  return '"' + s.replace(/\\/g, '\\\\').replace(/"/g, '\\"') + '"'
}
