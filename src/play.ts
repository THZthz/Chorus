// Play interface — a text script becomes a playable game.
// Game = script + log; handleLine is the whole turn loop (testable without a TTY);
// src/tui.tsx renders it in the terminal (Ink, with a readline fallback).
import type { Dictionary } from './dict.js'
import type { CompiledContent, CompiledVerb, SlotRole } from './content.js'
import { compileScript, type ScriptMeta } from './script.js'
import { executeStep } from './engine.js'
import { fold } from './fold.js'
import { parsePlayerInput } from './lenient.js'
import { journal, flattenAspects, pretty, proseLines } from './render.js'
import { matchQuery } from './query.js'
import { evalPred, type PredCtx, type RoleBindings } from './predicate.js'
import type { Ref } from './ref.js'
import { cloneState, type Entity, type State } from './types.js'
import { Log } from './store.js'
import { seedDictionary } from './seed.js'

export interface Turn {
  kind: 'action' | 'info' | 'error'
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

  // ---- meta views for the player interface (rendered as buttons) ----

  /** the cards this player can see: places; the player; cards sharing the
   *  player's location; carried cards; contents of an explored (named) place;
   *  and anything the log has named through a role or a spawn/destroy — a
   *  projection over (state, log), never stored */
  visible(): Entity[] {
    const alive = [...this.state.entities.values()].filter(e => e.deadAction === null)
    const me = this.meta.player ? alive.find(e => e.name === this.meta.player) : undefined
    const named = new Set<string>()
    for (const r of this.log.records) {
      const refs: Ref[] = [...r.actors, ...r.objects, ...r.instruments]
      if (r.at) refs.push(r.at)
      if (r.about) refs.push(r.about)
      if (r.to) refs.push(r.to)
      for (const ref of refs) if (ref.form === 'name') named.add(ref.name)
      for (const o of r.outcomes) {
        for (const a of o.args) {
          // INTO targets name a destination, not a discovery — keep them out
          if ((a.t === 'ref' || a.t === 'into') && !('role' in a.ref) && a.ref.form === 'name') named.add(a.ref.name)
        }
      }
    }
    const here = me?.attrs['container']
    return alive.filter(e => {
      if (e.kind === 'place' || (me && e.id === me.id)) return true
      if (e.attrs['container'] === here || (me && e.attrs['container'] === me.id)) return true
      if (named.has(e.name)) return true
      const container = typeof e.attrs['container'] === 'string' ? this.state.entities.get(e.attrs['container']) : undefined
      return container !== undefined && container.kind === 'place' && named.has(container.name)
    })
  }

  /** what is around the player */
  look(): string[] {    const player = this.meta.player
      ? [...this.state.entities.values()].find(e => e.deadAction === null && e.name === this.meta.player)
      : undefined
    if (!player) return ['(no player character — open the cards view)']
    const here = player.attrs['container']
    const place = typeof here === 'string' ? this.state.entities.get(here) : undefined
    const lines = [`You are at ${place?.name ?? 'nowhere'}.`]
    const things = [...this.state.entities.values()]
      .filter(e => e.deadAction === null && e.attrs['container'] === here && e.id !== player.id)
    for (const thing of things) lines.push(`  ${thing.name}  [${thing.kind}]`)
    if (things.length === 0) lines.push('  (nothing else here)')
    return lines
  }

  /** every visible card — the table the player actually sees */
  cards(): string[] {
    const lines = this.visible().map(e => `${e.name}  [${e.kind}]`)
    return lines.length > 0 ? lines : ['(no cards)']
  }

  /** a card's face: kind, aspects, where it sits, when it entered play.
   *  Visible cards only — the table has no face-down deck to fish from. */
  inspect(rawName: string): string[] {
    const q = rawName.trim().toLowerCase().replace(/_/g, ' ')
    const table = this.visible()
    const card = table.find(e => e.name.toLowerCase().replace(/_/g, ' ') === q)
      ?? table.find(e => e.name.toLowerCase().replace(/_/g, ' ').startsWith(q))
    if (!card) return [`nothing on the table answers to '${rawName.trim()}' — press 2 for the table`]
    const lines = [`${card.name}  [${card.kind}]`]
    const aspects = flattenAspects(card.aspects)
    if (aspects.length > 0) lines.push(`  ${aspects.join(' · ')}`)
    const container = typeof card.attrs['container'] === 'string' ? this.state.entities.get(card.attrs['container']) : undefined
    if (container) lines.push(`  at: ${pretty(container.name)}`)
    if (card.attrs['zone']) lines.push(`  zone: ${card.attrs['zone']}`)
    if (card.bornAction != null && card.bornAction > 0) lines.push(`  entered play at action ${card.bornAction}`)
    return lines
  }

  /** the log as prose */
  story(): string[] {
    const lines = this.log.records.map(r => '· ' + proseLines(r, this.content, this.dict).join('\n  '))
    return lines.length > 0 ? lines : ['(nothing has happened yet)']
  }

  /** the canonical log */
  journal(): string[] {
    return [journal(this.log.records) || '(the journal is empty)']
  }

  /** take back the last action */
  undo(): Turn {
    if (this.log.records.length === 0) return { kind: 'info', lines: ['nothing to undo'] }
    // drop trailing events, then the last action
    while (this.log.records.length > 0 && this.log.records[this.log.records.length - 1].kind === 'event') this.log.records.pop()
    if (this.log.records.length > 0) this.log.records.pop()
    this.state = fold(this.log.records, cloneState(this.base), this.dict, this.content)
    return { kind: 'info', lines: ['undone.'] }
  }

  /** concrete actions the player can take right now: verb × card bindings
   *  that fit the slots, those lighting up a non-default recipe first —
   *  every suggestion is lenient-parseable, so typing it just works */
  suggestions(limit = 3): string[] {
    const alive = this.visible()
    const me = this.meta.player ? alive.find(e => e.name === this.meta.player) : undefined
    if (!me) return []
    const rank = (e: Entity) => (e.attrs['container'] === me.attrs['container'] ? 0 : 1)
    const lights = (verb: CompiledVerb, bindings: RoleBindings): boolean => {
      const p: PredCtx = { b: bindings, state: this.state }
      // a REFUSE gate that matches is a wall, not an invitation
      return verb.recipes.some(r => r.when !== null && !r.out.some(o => o.op === 'REFUSE') && evalPred(r.when, p))
    }
    const phrase = (display: string, b: RoleBindings): string => {
      const parts = [display]
      for (const role of ['object', 'to', 'with', 'at', 'about'] as SlotRole[]) {
        const es = b[role] ?? []
        if (es.length === 0) continue
        const names = es.map(e => pretty(e.name)).join(' ')
        parts.push(role === 'object' ? names : `${role === 'to' ? 'with' : role} ${names}`)
      }
      return parts.join(' ')
    }

    const story: string[] = []
    const flavor: string[] = []
    for (const verb of this.content.verbs.values()) {
      const actorsSlot = verb.slotByRole.get('actors')
      if (!actorsSlot || actorsSlot.min < 1) continue
      if (actorsSlot.query && !matchQuery(me, actorsSlot.query)) continue
      const display = verb.name.toLowerCase().replace(/_/g, ' ')

      const pools = new Map<SlotRole, Entity[]>()
      let missing = false
      for (const slot of verb.slots) {
        if (slot.role === 'actors') continue
        const pool = alive
          .filter(e => e.name !== me.name && e.id !== me.attrs['container'] && (!slot.query || matchQuery(e, slot.query)))
          .sort((a, b) => rank(a) - rank(b) || a.name.localeCompare(b.name))
        if (slot.min >= 1 && pool.length < slot.min) { missing = true; break }
        pools.set(slot.role, pool)
      }
      if (missing) continue

      // base binding: each role filled to its minimum
      const base: RoleBindings = { actors: [me] }
      for (const [role, pool] of pools) {
        base[role] = pool.slice(0, Math.min(verb.slotByRole.get(role)!.min, 3))
      }
      const verbStory: string[] = []
      if (lights(verb, base)) verbStory.push(phrase(display, base))
      else flavor.push(phrase(display, base))

      // alternative bindings: swap or extend role cards; keep what lights
      for (const [role, pool] of pools) {
        const slot = verb.slotByRole.get(role)!
        const cap = Math.max(slot.min, Math.min(slot.max, 2))
        const baseEs = base[role] ?? []
        for (const e of pool) {
          if (baseEs.some(x => x.id === e.id)) continue
          let combined = [...baseEs, e]
          if (combined.length > cap) combined = [e]
          const alt: RoleBindings = { ...base, [role]: combined }
          if (lights(verb, alt)) verbStory.push(phrase(display, alt))
        }
      }
      story.push(...verbStory)
    }
    // round-robin across verbs so one greedy verb does not own the list
    const byVerb: string[][] = []
    for (let i = 0; i < story.length; ) {
      const group: string[] = []
      const verb = story[i]!.split(' ')[0]
      while (i < story.length && story[i]!.split(' ')[0] === verb) group.push(story[i++])
      if (group.length > 0) byVerb.push(group)
    }
    const mixed: string[] = []
    for (let round = 0; mixed.length < 64 && byVerb.some(g => g.length > 0); round++) {
      for (const g of byVerb) {
        const v = g.shift()
        if (v) mixed.push(v)
      }
    }
    const seen = new Set<string>()
    return [...mixed, ...flavor]
      .filter(s => {
        if (seen.has(s)) return false
        seen.add(s)
        return parsePlayerInput(s, { dict: this.dict, content: this.content, state: this.state, player: this.meta.player }).ok
      })
      .slice(0, limit)
  }
}
