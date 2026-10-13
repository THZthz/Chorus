// Canonical serializer — spec §5.1, §9.6, §9.10, §9.14, §9.15.
// Format(parse(x)) is the canonical form: actors sorted, pre-verbal AT,
// bare-name refs (re-quoted when unsafe/reserved), concrete outcomes.
import type { Arg, Clause, Outcome, Sentence } from './sentence.js'
import { formatRef, isSafeName, sortRefs, type SafeDict } from './ref.js'
import { formatClock, formatDuration } from './clock.js'

export function formatSentence(s: Sentence, dict?: SafeDict): string {
  const parts: string[] = []
  if (s.clock) parts.push(`${formatClock(s.clock)} @`)
  if (s.kind === 'event') {
    parts.push('EVENT', formatTriggerId(s.trigger ?? '?'))
  } else if (s.kind === 'complete') {
    // v0.5: `1:42 @ COMPLETE READ -> <outcomes>` — the verb prints bare
    // (UPPER_SNAKE always re-parses as a name token)
    parts.push('COMPLETE', s.trigger ?? '?')
  } else if (s.kind === 'wait') {
    // v0.5: `0:42 @ WAIT 1h` — the pure-advance clock row
    parts.push('WAIT', formatDuration(s.spanMin ?? 0))
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
  if (o.op === 'IF') throw new Error('an IF outcome is authored form only — the log records the taken branch (§9.39)')
  if (o.op === 'FN') throw new Error('a function call is authored form only — calls expand at load (§9.40)')
  if (o.op === 'ROLL') throw new Error('a ROLL is authored form only — the log records the engine-written ROLLED result (§9.41)')
  if (o.op === 'DRAW') throw new Error('a DRAW is authored form only — the log records the engine-written DRAWN+SPAWN rows (§9.42)')
  // ROLLED (§9.41) and DRAWN (§9.42) ride the generic paren'd fallback on
  // purpose: their args format and re-parse — `ROLLED(14, 3)` is two plain
  // ints, `DRAWN(RUMOURS, HEARSAY)` two plain name refs (unsafe spellings
  // re-quote through formatRef/parse) — the round-trip the fold's counter and
  // cursor rows need (a fallback that emitted anything else would make every
  // recorded roll or draw unparseable)
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
  if (o.op === 'REPLACE') {
    // canonical paren-less authored form: REPLACE ref WITH prototype (§4.4)
    const target = o.args[0]
    const proto = o.args[1]
    return `REPLACE ${target ? formatArg(target, dict) : '?'} WITH ${proto ? formatArg(proto, dict) : '?'}`
  }
  if (o.op === 'ENDING') {
    // canonical paren-less form: ENDING id (§9.35) — the id round-trips as a name ref
    const id = o.args[0]
    return `ENDING ${id ? formatArg(id, dict) : '?'}`
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
    case 'vexpr': throw new Error('a ROLL value expression is authored form only — the log records the computed ROLLED ints (§9.41)')
    case 'param': throw new Error('a function parameter is authored form only — calls expand at load (§9.40)')
  }
}

function quote(s: string): string {
  return '"' + s.replace(/\\/g, '\\\\').replace(/"/g, '\\"') + '"'
}

/** colon-joined trigger ids (`expire:<NAME>`, §6.6; authored ids may carry
 *  colons too): each segment prints bare when it re-parses as a name token
 *  and quoted when it does not, so every logged event row stays lossless
 *  (§1, §7) whatever the expiring entity was named */
function formatTriggerId(id: string): string {
  return id.split(':').map(seg => (isSafeName(seg) ? seg : quote(seg))).join(':')
}
