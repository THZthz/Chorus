// The Nightjar Flyer — story acceptance: the mystery compiles, plays, and
// is winnable; the timetable (triggers) is a real adversary; PROGRESS ticks
// (§6.7) render as the action's read-bar. Since v0.4 it is also the
// showcase: the scripted win path must fire every new mechanic (the
// SUSPICION pool, patrol_check's roll and tiers, the RUMOURS deck, the
// exists() branch in the talk recipe) without ever tripping the patrol's
// escalation ending — seed 1899 is calibrated for exactly that.
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
  'talk with the spiritualist',    // meet Seraphine (the name is learned by talking)
  'seraphine read the wax cylinder',
  'seraphine read the telegram',
  'use on the inquest with the embezzlement the stolen voice the vienna warning',
]

describe('the nightjar flyer', () => {
  test("lenient 'talk with X' maps onto TALK's to-slot", () => {
    const game = fresh()
    const r = parsePlayerInput('talk with the guard', {
      dict, content: game.content, state: game.state, player: 'MARCH',
    })
    expect(r.ok).toBe(true)
    if (r.ok) {
      const c = r.step.clauses[0]
      expect(c.verb).toBe('TALK')
      expect(c.to).toMatchObject({ form: 'name', name: 'THE_GUARD' })
      expect(c.with).toEqual([])
    }
  })

  test('compiles: the train, the masked suspects, and the six-verb toolkit', () => {
    const game = fresh()
    expect(game.title).toBe('Murder on the Nightjar Flyer')
    for (const name of ['MARCH', 'THE_PATENTEE', 'THE_WIDOW', 'THE_SECRETARY', 'THE_STOKER', 'THE_SPIRITUALIST', 'THE_GUARD', 'THE_INQUEST', 'THE_MANIFEST']) {
      expect(game.state.byName.has(name)).toBe(true)
    }
    // real names mint into being only through the talk.meet_* reveals
    for (const name of ['LORD_VANE', 'VERITY', 'QUILL', 'COLE', 'SERAPHINE', 'BRACK']) {
      expect(game.state.byName.has(name)).toBe(false)
    }
    expect([...game.content.verbs.keys()].sort()).toEqual(['EXPLORE', 'READ', 'TALK', 'TRAVEL_TO', 'USE_ON', 'WAIT'])
  })

  test('the train is a zone, not a card; her cars carry it as an attribute', () => {
    const game = fresh()
    expect(game.state.byName.has('NIGHTJAR_FLYER')).toBe(false)
    expect(game.state.entities.get('s_guard_van')!.attrs['zone']).toBe('NIGHTJAR_FLYER')
  })

  test('travel follows the corridor — adjacent cars only', () => {
    const game = fresh()
    const refused = game.handleLine('travel to the dining car')
    expect(refused.kind).toBe('action')
    expect(refused.lines.join('\n')).toContain('does not bend')
    const march = game.state.byName.get('MARCH')![0]
    expect(game.state.entities.get(march)!.attrs['container']).toBe('s_guard_van')
    const ok = game.handleLine('travel to the luggage van')
    expect(ok.kind).toBe('action')
    expect(game.state.entities.get(march)!.attrs['container']).toBe('s_luggage_van')
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

  test('every v0.4 mechanic fires on the scripted path, and none of them ends it', () => {
    const game = fresh()
    play(game, WIN_PATH)
    const ints = (o: { op: string; args: { t: string; v?: unknown }[] }): [number, number] | null => {
      const [a, b] = o.args
      return a?.t === 'int' && b?.t === 'int' ? [a.v as number, b.v as number] : null
    }
    // F1 — the SUSPICION pool exists: the cabin search's patrol_check and the
    // 22:30 rounds both landed the middle tier under seed 1899 (never 3)
    expect(game.state.byName.get('SUSPICION')).toHaveLength(2)
    // F4 — three ROLLED rows (cabin patrol, conductor's rounds, firebox
    // find), the counter landing exactly where play left it
    const rolled = game.log.records.flatMap(r =>
      r.outcomes.filter(o => o.op === 'ROLLED').map(o => ints(o)),
    )
    expect(rolled).toEqual([[6, 2], [7, 4], [14, 7]]) // calibrated against seed 1899
    expect(game.state.rng).toBe(7)
    // F5 — the RUMOURS deck drew twice: once from the patrol trigger (into
    // the guard van), once from the talk recipe (into the dining car)
    const drawn = game.log.records.flatMap(r =>
      r.outcomes.filter(o => o.op === 'DRAWN').map(o => {
        const n = o.args[1]
        return n && n.t === 'ref' && n.ref.form === 'name' ? n.ref.name : '(?)'
      }),
    )
    expect(drawn).toEqual(['SCANDAL', 'RUMOUR']) // the seed-fixed permutation, in order
    expect(game.state.byName.has('RUMOUR')).toBe(true)
    expect(game.state.byName.has('SCANDAL')).toBe(true)
    // F3 — the conductor's rounds: the story's only non-`once` trigger, and
    // its event row carries the expanded patrol_check outcomes (ROLLED row)
    const rounds = game.log.records.filter(r => r.trigger === 'conductor_rounds')
    expect(rounds).toHaveLength(1)
    expect(rounds[0]!.outcomes.some(o => o.op === 'ROLLED')).toBe(true)
    // F2 — the talk recipe's exists() branch took the evidence arm (the seal
    // idea is on the table by 2:00)
    const meet = game.log.records.find(r => r.recipes.includes('talk.meet_spiritualist'))!
    const narrate = meet.outcomes.find(o => o.op === 'NARRATE')
    const said = narrate?.args[0]
    expect(said && said.t === 'str' && said.v.includes('we all read the same card: the lord is dead')).toBe(true)
    // the escalation never tripped: the case ends at the inquest, not the border
    expect(game.state.ended).toBeNull()
  })

  test('a careless search spawns SUSPICION — the pool is real, not flavor (F1)', () => {
    const game = fresh()
    play(game, ['wait', 'explore the first cabin'])
    const before = (game.state.byName.get('SUSPICION') ?? []).length
    const turn = game.handleLine('explore the first cabin') // rummaging again
    expect(turn.kind).toBe('action')
    expect((game.state.byName.get('SUSPICION') ?? []).length).toBe(before + 1)
    expect(turn.lines.join('\n')).toContain('A steward will remember MARCH')
  })

  test('three careless searches and the conductor ends the case at his rounds (escalation)', () => {
    const game = fresh()
    play(game, ['wait', 'explore the first cabin', 'explore the first cabin', 'explore the first cabin'])
    expect((game.state.byName.get('SUSPICION') ?? []).length).toBeGreaterThanOrEqual(3)
    let guard = 0
    while (!game.state.ended && guard++ < 40) game.handleLine('wait') // to the next 22:30
    expect(game.state.ended).toMatchObject({ id: 'OFF_THE_FLYER', flavour: 'melancholy' })
    // undo reopens the story — `ended` derives from the fold (§9.35)
    expect(game.undo().kind).toBe('info')
    expect(game.state.ended).toBeNull()
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
    expect(game.handleLine('talk to the stoker').kind).toBe('action')      // 21:00 → 21:30 — meet Cole
    expect(game.state.byName.has('COLE')).toBe(true)
    expect(game.handleLine('talk to the guard').kind).toBe('action')       // 21:30 → 22:00 — meet Brack; the body is found
    expect(game.log.records.some(r => r.trigger === 'body_found')).toBe(true)
    expect(game.state.byName.has('THE_LATE_LORD')).toBe(true)
    expect(game.handleLine('talk to the spiritualist').kind).toBe('action') // 22:00 → 22:30 — meet Seraphine
    expect(game.handleLine('wait').kind).toBe('action')                     // 22:30 → 23:30
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
    play(game, ['wait', 'talk to the stoker']) // the seal idea spawns; meet Cole
    const turn = game.handleLine('talk to cole about the aether seal')
    expect(turn.kind).toBe('action')
    expect(turn.lines.join('\n')).toContain('rake the firebox hot at midnight')
    const rec = game.log.records.filter(r => r.verb === 'TALK')[1]!
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
