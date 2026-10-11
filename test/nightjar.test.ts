// The Nightjar Flyer — story acceptance: the mystery compiles, plays, and
// is winnable; the timetable (triggers) is a real adversary; PROGRESS ticks
// (§6.7) render as the action's read-bar.
import { describe, expect, test } from 'vitest'
import { readFileSync } from 'node:fs'
import { Game, type Turn } from '../src/play.js'
import { parsePlayerInput } from '../src/lenient.js'
import { fold } from '../src/fold.js'
import { seedDictionary } from '../src/seed.js'

const dict = seedDictionary()
const scriptText = readFileSync(new URL('../scripts/the-nightjar-flyer.txt', import.meta.url), 'utf8')

function fresh(): Game {
  return Game.fromScript(scriptText, dict)
}

function play(game: Game, lines: string[]): Turn[] {
  return lines.map(l => game.handleLine(l))
}

const WIN_PATH = [
  'wait',                          // 22:00 — the body is found
  'explore the first cabin',       // 23:00 — the solicitor's letter
  'read the solicitor letter',     // 0:00  — border stop; the telegram lands
  'explore the firebox',           // 1:00  — the wax cylinder
  'read the vane ledger',          // 2:00  — read in the same hour it burns
  'seraphine read the wax cylinder',
  'seraphine read the telegram',
  'use on the inquest with the embezzlement the stolen voice the vienna warning',
]

describe('the nightjar flyer', () => {
  test("lenient 'talk with X' maps onto TALK's to-slot", () => {
    const game = fresh()
    const r = parsePlayerInput('talk with quill', {
      dict, content: game.content, state: game.state, player: 'MARCH',
    })
    expect(r.ok).toBe(true)
    if (r.ok) {
      const c = r.step.clauses[0]
      expect(c.verb).toBe('TALK')
      expect(c.to).toMatchObject({ form: 'name', name: 'QUILL' })
      expect(c.with).toEqual([])
    }
  })

  test('compiles: world, suspects, and the five-verb toolkit', () => {
    const game = fresh()
    expect(game.title).toBe('Murder on the Nightjar Flyer')
    for (const name of ['MARCH', 'LORD_VANE', 'VERITY', 'QUILL', 'COLE', 'SERAPHINE', 'BRACK', 'THE_INQUEST']) {
      expect(game.state.byName.has(name)).toBe(true)
    }
    expect([...game.content.verbs.keys()].sort()).toEqual(['EXPLORE', 'READ', 'TALK', 'USE_ON', 'WAIT'])
  })

  test('the timetable discovers the body at 22:00', () => {
    const game = fresh()
    const turn = game.handleLine('wait')
    expect(turn.kind).toBe('action')
    expect(game.log.records.some(r => r.kind === 'event' && r.trigger === 'body_found')).toBe(true)
    expect(game.state.byName.has('LORD_VANE')).toBe(false)
    expect(game.state.byName.has('THE_LATE_LORD')).toBe(true)
    expect(game.state.byName.has('THE_AETHER_SEAL')).toBe(true)
    expect(turn.lines.join('\n')).toContain('entirely dead')
  })

  test('progress ticks render as the action\'s read-bar (§6.7)', () => {
    const game = fresh()
    game.handleLine('wait')
    const turn = game.handleLine('explore the first cabin')
    expect(turn.kind).toBe('action')
    const joined = turn.lines.join('\n')
    expect(joined).toContain('(+1halfhour)')     // live tick line
    expect(joined).toContain("solicitor's letter")
    const canonical = turn.lines[turn.lines.length - 1]
    expect(canonical).toContain('PROGRESS(1halfhour,') // the tick is in the record
    expect(canonical).toContain('SPAWN THE_SOLICITOR_LETTER INTO #s_first_cabin')
  })

  test('full win path: motive, means, opportunity → confession', () => {
    const game = fresh()
    const turns = play(game, WIN_PATH)
    expect(turns.every(t => t.kind === 'action')).toBe(true)
    for (const clue of ['THE_EMBEZZLEMENT', 'THE_STOLEN_VOICE', 'THE_VIENNA_WARNING']) {
      expect(game.state.byName.has(clue)).toBe(true)
    }
    expect(game.state.byName.has('CONFESSION_OF_QUILL')).toBe(true)
    const inquest = game.log.records.find(r => r.verb === 'USE_ON')!
    expect(inquest.recipes).toEqual(['inquest.win'])
    expect(game.log.records.some(r => r.trigger === 'border_stop')).toBe(true)
    expect(game.log.records.some(r => r.trigger === 'the_furnace_takes_it')).toBe(true)
  })

  test('the french gate refuses an anglophone reader, and speaks in prose (§6.2)', () => {
    const game = fresh()
    play(game, ['wait', 'wait', 'wait']) // to 0:00 — the telegram lands
    const turn = game.handleLine('read the telegram')
    expect(turn.kind).toBe('action')
    expect(turn.lines.some(l => l.startsWith('The words are French'))).toBe(true)
    expect(game.state.byName.has('THE_VIENNA_WARNING')).toBe(false)
  })

  test('searching the sealed cabin before the murder neither spoils nor breaks the search', () => {
    const game = fresh()
    const pre = game.handleLine('explore the first cabin')
    expect(pre.kind).toBe('action')
    expect(pre.lines.join('\n')).toContain('does not open for a piece of paper')
    expect(game.state.byName.has('THE_SOLICITOR_LETTER')).toBe(false)
    play(game, ['wait']) // the body is found; the seal comes down
    const post = game.handleLine('explore the first cabin')
    expect(post.kind).toBe('action')
    expect(game.state.byName.has('THE_SOLICITOR_LETTER')).toBe(true)
  })

  test('unknown topics and places fail loudly (lenient about/at)', () => {
    const game = fresh()
    const turn = game.handleLine('talk to seraphine about the lost tracklist')
    expect(turn.kind).toBe('error')
    expect(turn.lines.join('\n')).toContain("I don't know")
  })

  test('an inquest on a hunch or on empty hands refuses', () => {
    const game = fresh()
    play(game, ['wait', 'explore the first cabin', 'read the solicitor letter'])
    const thin = game.handleLine('use on the inquest with the disinheritance')
    expect(thin.kind).toBe('action')
    expect(thin.lines.join('\n')).toContain('wobbles')
    expect(game.state.byName.has('CONFESSION_OF_QUILL')).toBe(false)
    const empty = game.handleLine('use on the inquest')
    expect(empty.kind).toBe('action')
    expect(empty.lines.join('\n')).toContain('empty hands')
  })

  test('halfhour actions land back on the ticks and the timetable fires as the night passes', () => {
    const game = fresh()
    expect(game.handleLine('talk to cole').kind).toBe('action')     // 21:00 → 21:30 (1halfhour)
    // interrogations are slot-aligned now; the 22:00 crossing arrives with the
    // next step's window — the body is found
    expect(game.handleLine('talk to brack').kind).toBe('action')    // 21:30 → 22:00
    expect(game.log.records.some(r => r.trigger === 'body_found')).toBe(true)
    expect(game.state.byName.has('THE_LATE_LORD')).toBe(true)
    expect(game.handleLine('talk to seraphine').kind).toBe('action') // 22:00 → 22:30
    expect(game.handleLine('wait').kind).toBe('action')              // 22:30 → 23:30
    expect(game.log.records.some(r => r.trigger === 'night_porter')).toBe(true)
  })

  test('once-triggers fire exactly once — day two does not re-murder the lord (§9.29)', () => {
    const game = fresh()
    play(game, ['wait']) // day 0, hour 22 — body_found fires
    expect(game.log.records.filter(r => r.trigger === 'body_found')).toHaveLength(1)
    // jump to day 1, hour 22: the once-trigger must neither re-fire nor reject the step
    const jump = game.handleLine('1:42 @ MARCH WAIT')
    expect(jump.kind).toBe('action')
    expect(game.log.records.filter(r => r.trigger === 'body_found')).toHaveLength(1)
    expect(game.state.byName.has('THE_LATE_LORD')).toBe(true)
  })

  test('the ledger burns at 2:00 — the timetable is the culprit too', () => {
    const game = fresh()
    play(game, ['wait', 'wait', 'wait', 'wait', 'wait']) // 21:00 → 2:00
    expect(game.state.byName.has('THE_VANE_LEDGER')).toBe(false)
    const turn = game.handleLine('read the vane ledger')
    expect(turn.kind).toBe('error') // the card is gone; the step is rejected
  })

  test('interrogation exercises the to role end to end', () => {
    const game = fresh()
    game.handleLine('wait')
    const turn = game.handleLine('talk to cole about the aether seal')
    expect(turn.kind).toBe('action')
    expect(turn.lines.join('\n')).toContain('rake the firebox hot at midnight')
    const rec = game.log.records.find(r => r.verb === 'TALK')!
    expect(rec.to).toMatchObject({ form: 'name', name: 'COLE' })
    expect(rec.sentence).toContain('MARCH TALK TO COLE ABOUT THE_AETHER_SEAL')
  })

  test('the whole night replays from the log (state = fold(log))', () => {
    const game = fresh()
    play(game, WIN_PATH)
    const replayed = fold(game.log.records, game['base'], game.dict, game.content)
    expect([...replayed.byName.keys()].sort()).toEqual([...game.state.byName.keys()].sort())
    expect(replayed.now).toBe(game.state.now)
  })
})
