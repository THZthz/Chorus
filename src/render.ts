// L3 projection — spec §5.3, §6.7, §8. Pure functions over records/state.
import type { ActionRecord } from './engine.js'
import type { CompiledContent } from './content.js'
import type { Clause } from './sentence.js'
import type { Ref } from './ref.js'
import { parseSentence } from './parse.js'
import { formatDuration } from './clock.js'
import type { Dictionary } from './dict.js'
import type { Entity } from './types.js'

export function journal(records: ActionRecord[]): string {
  return records.map(r => r.sentence).join('\n')
}

/** Prose rendering: one template line per clause, the processing ticks (§6.7)
 *  indented under them, then NARRATE lines. */
export function proseLines(record: ActionRecord, content: CompiledContent, dict: Dictionary): string[] {
  const lines: string[] = []
  if (record.kind === 'action') {
    const s = parseSentence(record.sentence, dict)
    for (const clause of s.clauses) {
      const template = content.templates.get(clause.verb)
      if (template) lines.push(interpolateClause(template, clause))
    }
  }
  for (const o of record.outcomes) {
    const narr = o.args.find(a => a.t === 'str')
    if (!narr || narr.t !== 'str') continue
    if (o.op === 'PROGRESS') {
      const d = o.args.find(a => a.t === 'dur')
      const off = d && d.t === 'dur' ? formatDuration(d.v) : '0m'
      lines.push(`  (+${off}) ${interpolateRecord(narr.v, record)}`)
    } else if (o.op === 'NARRATE') {
      lines.push(interpolateRecord(narr.v, record))
    } else if (o.op === 'REFUSE') {
      lines.push(narr.v) // §6.2: refusal is always spoken
    }
  }
  return lines
}

export function prose(record: ActionRecord, content: CompiledContent, dict: Dictionary): string {
  return proseLines(record, content, dict).join('\n')
}

const refName = (r: Ref | null): string => (r && r.form === 'name' ? r.name : r ? `#${r.id}` : '—')
const refNames = (rs: Ref[]): string[] => rs.map(r => (r.form === 'name' ? r.name : `#${r.id}`))

function interpolateClause(template: string, clause: Clause): string {
  return template
    .replace(/\{actors\}/g, joinAnd(refNames(clause.actors)))
    .replace(/\{object\}/g, refNames(clause.objects)[0] ?? '—')
    .replace(/\{at\}/g, refName(clause.at))
    .replace(/\{about\}/g, refName(clause.about))
    .replace(/\{to\}/g, refName(clause.to))
    .replace(/\{with\}/g, joinAnd(refNames(clause.with)))
}

function interpolateRecord(template: string, record: ActionRecord): string {
  return template
    .replace(/\{actors\}/g, joinAnd(refNames(record.actors)))
    .replace(/\{object\}/g, refNames(record.objects)[0] ?? '—')
    .replace(/\{at\}/g, refName(record.at))
    .replace(/\{about\}/g, refName(record.about))
    .replace(/\{to\}/g, refName(record.to))
    .replace(/\{with\}/g, joinAnd(refNames(record.instruments)))
}

function joinAnd(names: string[]): string {
  if (names.length <= 1) return names[0] ?? ''
  return names.slice(0, -1).join(', ') + ' and ' + names[names.length - 1]
}

/** Text card — spec §8. Every field comes from the dictionary. */
export function card(e: Entity): string {
  const aspectChips = Object.entries(e.aspects)
    .map(([k, v]) => typeof v === 'number' ? `${k}:${v}` : Object.entries(v).map(([sk, sv]) => `${k}.${sk}:${sv}`).join(' '))
    .join(' · ')
  const flagChips = Object.entries(e.attrs)
    .filter(([k]) => k.startsWith('state.'))
    .map(([k, v]) => v === true ? k : `${k}=${v}`)
    .join(' · ')
  const tags = [e.kind, aspectChips, flagChips].filter(Boolean).join(' · ')
  const width = Math.max(e.name.length, tags.length) + 2
  const line = '─'.repeat(width)
  return [
    `┌${line}┐`,
    `│ ${e.name.padEnd(width - 2)} │`,
    `│ ${tags.padEnd(width - 2)} │`,
    `└${line}┘`,
  ].join('\n')
}
