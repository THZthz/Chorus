// L3 projection — spec §5.3, §8. Pure functions over records/state.
import type { ActionRecord } from './engine.js'
import type { CompiledContent } from './content.js'
import type { Clause } from './sentence.js'
import { parseSentence } from './parse.js'
import type { Dictionary } from './dict.js'
import type { Entity } from './types.js'

export function journal(records: ActionRecord[]): string {
  return records.map(r => r.sentence).join('\n')
}

/** Prose rendering: one template line per clause + NARRATE lines. */
export function prose(record: ActionRecord, content: CompiledContent, dict: Dictionary): string {
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
    if (o.op === 'NARRATE' && narr && narr.t === 'str') {
      lines.push(interpolateRecord(narr.v, record))
    }
  }
  return lines.join(' ')
}

function interpolateClause(template: string, clause: Clause): string {
  return template
    .replace(/\{actors\}/g, joinAnd(clause.actors.map(r => r.form === 'name' ? r.name : `#${r.id}`)))
    .replace(/\{object\}/g, clause.objects[0]?.form === 'name' ? clause.objects[0].name : '—')
    .replace(/\{at\}/g, clause.at?.form === 'name' ? clause.at.name : '—')
    .replace(/\{about\}/g, clause.about?.form === 'name' ? clause.about.name : '—')
}

function interpolateNames(template: string, actors: ActionRecord['actors']): string {
  return template.replace(/\{actors\}/g, joinAnd(actors.map(r => r.form === 'name' ? r.name : `#${r.id}`)))
}

function interpolateRecord(template: string, record: ActionRecord): string {
  return interpolateNames(template, record.actors)
    .replace(/\{object\}/g, record.objects[0]?.form === 'name' ? record.objects[0].name : '—')
    .replace(/\{at\}/g, record.at?.form === 'name' ? record.at.name : '—')
    .replace(/\{about\}/g, record.about?.form === 'name' ? record.about.name : '—')
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
