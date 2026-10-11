// Play interface — a text script becomes a playable game.
// Game = script + log; handleLine is the whole turn loop (testable without a TTY);
// src/tui.tsx renders it in the terminal (Ink, with a readline fallback).
import type { Dictionary } from './dict.js'
import type { CompiledContent } from './content.js'
import { compileScript, type ScriptMeta } from './script.js'
import { executeStep } from './engine.js'
import { fold } from './fold.js'
import { parsePlayerInput } from './lenient.js'
import { journal, proseLines } from './render.js'
import { cloneState, type State } from './types.js'
import { Log } from './store.js'
import { seedDictionary } from './seed.js'

export interface Turn {
  kind: 'action' | 'info' | 'error' | 'quit'
  lines: string[]
}

export class Game {
  readonly dict: Dictionary
  readonly content: CompiledContent
  readonly meta: ScriptMeta
  readonly log: Log
  private base: State
  state: State

  private constructor(dict: Dictionary, content: CompiledContent, meta: ScriptMeta, base: State) {
    this.dict = dict
    this.content = content
    this.meta = meta
    this.base = base
    this.log = new Log(cloneState(base))
    this.state = cloneState(base)
  }

  static fromScript(text: string, dict: Dictionary = seedDictionary()): Game {
    const { content, state, meta } = compileScript(text, dict)
    return new Game(dict, content, meta, state)
  }

  get title(): string {
    return this.meta.title ?? 'Untitled'
  }

  /** one turn of play — pure enough to drive from tests */
  handleLine(line: string): Turn {
    const t = line.trim()
    if (!t) return { kind: 'info', lines: [] }
    if (t === '/quit' || t === '/exit') return { kind: 'quit', lines: ['Goodbye.'] }
    if (t === '/help') {
      return { kind: 'info', lines: [
        `You are ${this.meta.player ?? '—'}. Type what happens; the engine writes the log.`,
        '  read <text> · talk with <someone> about <idea> · travel to <place> · wait',
        '  /look      what is around you',
        '  /cards     every card in play',
        '  /journal   the canonical log',
        '  /story     the log as prose',
        '  /undo      take back the last action',
        '  /quit      leave',
      ] }
    }
    if (t === '/journal') return { kind: 'info', lines: [journal(this.log.records) || '(the journal is empty)'] }
    if (t === '/story') {
      const lines = this.log.records.map(r => '· ' + proseLines(r, this.content, this.dict).join('\n  '))
      return { kind: 'info', lines: lines.length > 0 ? lines : ['(nothing has happened yet)'] }
    }
    if (t === '/cards') {
      const lines = [...this.state.entities.values()]
        .filter(e => e.deadAction === null)
        .map(e => `${e.name}  [${e.kind}]`)
      return { kind: 'info', lines: lines.length > 0 ? lines : ['(no cards)'] }
    }
    if (t === '/look') return { kind: 'info', lines: this.look() }
    if (t === '/undo') return this.undo()

    const parsed = parsePlayerInput(t, { dict: this.dict, content: this.content, state: this.state, player: this.meta.player })
    if (!parsed.ok) {
      const lines = [parsed.error]
      if (parsed.suggestions.length > 0) lines.push(`did you mean: ${parsed.suggestions.join(', ')}?`)
      return { kind: 'error', lines }
    }
    const result = executeStep(this.content, this.dict, this.state, parsed.step, { firstRecordId: this.log.nextRecordId })
    if (result.rejected) return { kind: 'error', lines: [result.rejected] }
    this.log.appendAll(result.records)
    this.state = result.state
    const lines = result.records.flatMap(r => proseLines(r, this.content, this.dict))
    return { kind: 'action', lines: [...lines, '', `« ${result.records.map(r => r.sentence).join(' | ')} »`] }
  }

  private look(): string[] {
    const player = this.meta.player
      ? [...this.state.entities.values()].find(e => e.deadAction === null && e.name === this.meta.player)
      : undefined
    if (!player) return ['(no player character — use /cards)']
    const here = player.attrs['container']
    const place = typeof here === 'string' ? this.state.entities.get(here) : undefined
    const lines = [`You are at ${place?.name ?? 'nowhere'}.`]
    const things = [...this.state.entities.values()]
      .filter(e => e.deadAction === null && e.attrs['container'] === here && e.id !== player.id)
    for (const thing of things) lines.push(`  ${thing.name}  [${thing.kind}]`)
    if (things.length === 0) lines.push('  (nothing else here)')
    return lines
  }

  private undo(): Turn {
    if (this.log.records.length === 0) return { kind: 'info', lines: ['nothing to undo'] }
    // drop trailing events, then the last action
    while (this.log.records.length > 0 && this.log.records[this.log.records.length - 1].kind === 'event') this.log.records.pop()
    if (this.log.records.length > 0) this.log.records.pop()
    this.state = fold(this.log.records, cloneState(this.base), this.dict, this.content)
    return { kind: 'info', lines: ['undone.'] }
  }
}
