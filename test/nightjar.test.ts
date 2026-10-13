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
  // v0.5: auto-settle — every committed action's window completes before the
  // next input, restoring the pre-parallel commit-completes-at-once shape
  const game = Game.fromScript(scriptText, dict)
  const orig = game.handleLine.bind(game)
  game.handleLine = (line: string) => {
    const t = orig(line)
    if (t.kind === 'action' && game.state.windows.size > 0) {
      const settle = game.wait()
      return { kind: 'action', lines: [...t.lines, ...settle.lines] }
    }
    return t
  }
  return game
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

  test('compiles: the train, the masked suspects, and the twelve-verb core set', () => {
    const game = fresh()
    expect(game.title).toBe('Murder on the Nightjar Flyer')
    for (const name of ['MARCH', 'THE_PATENTEE', 'THE_WIDOW', 'THE_SECRETARY', 'THE_STOKER', 'THE_SPIRITUALIST', 'THE_GUARD', 'THE_INQUEST', 'THE_MANIFEST']) {
      expect(game.state.byName.has(name)).toBe(true)
    }
    // real names mint into being only through the talk.meet_* reveals
    for (const name of ['LORD_VANE', 'VERITY', 'QUILL', 'COLE', 'SERAPHINE', 'BRACK']) {
      expect(game.state.byName.has(name)).toBe(false)
    }
    expect([...game.content.verbs.keys()].sort()).toEqual([
      'ACQUIRE', 'DREAM', 'EXPLORE', 'LOCATE_AT', 'MANSUS', 'READ',
      'STUDY', 'TALK', 'TRAVEL_TO', 'USE_ON', 'WAIT', 'WORK',
    ])
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
    const canonical = turn.lines.filter(l => l.startsWith('«')).join('\n')
    expect(canonical).toContain('PROGRESS(1halfhour,') // the tick rides the BEGIN row
    expect(canonical).toContain('SPAWN THE_SOLICITOR_LETTER INTO #s_first_cabin') // the mint the completion
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

  test('every v0.4 mechanic fires on the scripted path (v0.5 timeline)', () => {
    const game = fresh()
    play(game, WIN_PATH)
    const ints = (o: { op: string; args: { t: string; v?: unknown }[] }): [number, number] | null => {
      const [a, b] = o.args
      return a?.t === 'int' && b?.t === 'int' ? [a.v as number, b.v as number] : null
    }
    // F1 — the SUSPICION pool exists: the 22:30 rounds landed the middle
    // tier under seed 1899; the cabin's own patrol roll now resolves at the
    // window's close — after the rounds — and lands the pass-by tier
    expect(game.state.byName.get('SUSPICION')).toHaveLength(1)
    // F4 — three ROLLED rows (conductor's rounds, cabin patrol, firebox
    // find — v0.5 completion order), the counter landing where play left it
    const rolled = game.log.records.flatMap(r =>
      r.outcomes.filter(o => o.op === 'ROLLED').map(o => ints(o)),
    )
    expect(rolled).toEqual([[5, 2], [8, 4], [14, 7]]) // calibrated against seed 1899
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
    // F3 — the conductor's rounds: the story's recurring patrol (the dawn
    // watch is the other recurring slot trigger, off-screen tonight), and
    // its event row carries the expanded patrol_check outcomes (ROLLED row)
    const rounds = game.log.records.filter(r => r.trigger === 'conductor_rounds')
    expect(rounds).toHaveLength(1)
    expect(rounds[0]!.outcomes.some(o => o.op === 'ROLLED')).toBe(true)
    // F2 — the talk recipe's exists() branch took the evidence arm (the seal
    // idea is on the table by 2:00)
    const meetBegin = game.log.records.find(r => r.recipes.includes('talk.meet_spiritualist'))!
    const meetDone = game.log.records.find(r => r.kind === 'complete' && r.window === `${meetBegin.id}:0`)!
    const narrate = meetDone.outcomes.find(o => o.op === 'NARRATE')
    const said = narrate?.args[0]
    expect(said && said.t === 'str' && said.v.includes('we all read the same card: the lord is dead')).toBe(true)
    // the escalation never tripped — but the inquest's grand ending now
    // lands at the window's close, one crossing after the vienna quiet
    // night: first ENDING wins (§9.35), and the train gets there first
    expect(game.state.ended).toMatchObject({ id: 'QUIET_NIGHT', flavour: 'melancholy' })
    expect(game.state.byName.has('CONFESSION_OF_QUILL')).toBe(true) // the inquest still won the case
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

  test('careless searches feed the pool until watch(3) ends the case (escalation)', () => {
    const game = fresh()
    play(game, ['wait', 'explore the first cabin'])
    // the 22:30 rounds fed the pool once; the cabin's own patrol roll now
    // resolves at the window's close — under seed 1899 it lands the pass-by
    // tier, so the pool sits at one until the searches pile on
    expect(game.state.byName.get('SUSPICION')).toHaveLength(1)
    expect(game.state.ended).toBeNull()
    // the searched-again recipe spawns a copy AND calls watch(3) — the pool
    // climbs one copy per careless search until the third ends the story
    const turn3 = game.handleLine('explore the first cabin')
    expect(turn3.kind).toBe('action')
    expect(game.state.byName.get('SUSPICION')).toHaveLength(2)
    expect(game.state.ended).toBeNull()
    const turn4 = game.handleLine('explore the first cabin')
    expect(turn4.kind).toBe('action')
    expect(game.state.ended).toMatchObject({ id: 'OFF_THE_FLYER', flavour: 'melancholy' })
    const actions = game.log.records.filter(r => r.kind === 'action')
    const last = actions[actions.length - 1]!
    expect(last.recipes).toContain('explore.searched')
    const lastComplete = game.log.records.filter(r => r.kind === 'complete').at(-1)!
    expect(lastComplete.outcomes.some(o => o.op === 'ENDING')).toBe(true) // v0.5: the ENDING lands at the completion
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

  test('once-triggers fire exactly once, and the lord expires into his successor (§9.29, §9.32)', () => {
    const game = fresh()
    play(game, ['wait']) // day 0, hour 22 — body_found fires
    expect(game.log.records.filter(r => r.trigger === 'body_found')).toHaveLength(1)
    // jump a day ahead in ONE step: the once-trigger must neither re-fire nor
    // reject — and the lord's 1-day `expires` crosses on the way, so the
    // authored `on expire` trigger replaces him (REPLACE, §9.32–§9.33). The
    // span also crosses the vienna ending (§9.35) — the ending rides along:
    // the pass that fired it runs to its full target, and play stops after.
    const jump = game.handleLine('2:0 @ MARCH WAIT')
    expect(jump.kind).toBe('action')
    expect(game.log.records.filter(r => r.trigger === 'body_found')).toHaveLength(1) // once means once
    expect(game.log.records.filter(r => r.trigger === 'lord_tended')).toHaveLength(1)
    expect(game.state.byName.has('THE_LATE_LORD')).toBe(false)
    expect(game.state.byName.has('THE_TENDED_LORD')).toBe(true)
    expect(game.state.ended).not.toBeNull() // the flyer reached vienna en route
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
    const rec = game.log.records.filter(r => r.kind === 'action' && r.verb === 'TALK')[1]!
    expect(rec.to).toMatchObject({ form: 'name', name: 'COLE' })
    expect(rec.sentence).toContain('MARCH TALK TO COLE ABOUT THE_AETHER_SEAL')
  })

  test('the showcase verbs play: wages, the tunnel closure, and a ticket with one price', () => {
    const game = fresh()
    expect(game.handleLine('talk to the guard').kind).toBe('action') // meet Brack — RELATE(MARCH, BRACK, neighbor)
    expect(game.handleLine('march, brack work at the firebox').kind).toBe('action')
    expect(game.state.byName.get('FUNDS')).toHaveLength(2)           // the load copy + the shift's wage
    const marchId = game.state.byName.get('MARCH')![0]
    expect(game.state.entities.get(marchId)!.attrs['coin']).toBe(2)
    expect(game.handleLine('locate_at the firebox').kind).toBe('action')
    game.handleLine('wait') // to 1:00 — the tunnel severs the crew door
    expect(game.state.relations.get('s_firebox|s_guard_van|south')!.deadAction).not.toBeNull()
    const blocked = game.handleLine('travel to the guard van')
    expect(blocked.kind).toBe('action')
    expect(blocked.lines.join('\n')).toContain('does not bend')
    game.handleLine('wait')
    game.handleLine('wait')
    game.handleLine('wait') // to 4:30 — dawn_watch relinks the corridor
    expect(game.state.relations.get('s_firebox|s_guard_van|south')!.deadAction).toBeNull()
    expect(game.handleLine('travel to the guard van').kind).toBe('action')
    // ACQUIRE: a cost that can go below zero — exactly once per ticket (§9.31)
    expect(game.handleLine('acquire the claim ticket').kind).toBe('action')
    expect(game.state.entities.get(marchId)!.attrs['coin']).toBe(1)
    const again = game.handleLine('acquire the claim ticket')
    expect(again.kind).toBe('action')
    expect(again.lines.join('\n')).toContain('Nobody aboard is selling')
  })

  test('dreaming draws the DREAMS deck until it runs dry (§9.42, §9.23)', () => {
    const game = fresh()
    play(game, ['wait', 'explore the firebox', 'read the wax cylinder', 'read the vane ledger'])
    expect(game.state.byName.has('HUNCH_THE_VANE_LEDGER')).toBe(true) // the derive prototype minted
    // playing the cylinder unsealed the seal idea, so it fits the dream slot
    expect(game.handleLine('dream with the aether seal').kind).toBe('action')
    expect(game.handleLine('dream with the aether seal').kind).toBe('action')
    expect(game.handleLine('dream with the aether seal').kind).toBe('action')
    // the seed-fixed permutation served VOID, PORTENT, PORTENT — stackable pools
    expect(game.state.byName.get('PORTENT')).toHaveLength(2)
    expect(game.state.byName.get('VOID')).toHaveLength(1)
    const dry = game.handleLine('dream with the aether seal')
    expect(dry.kind).toBe('action') // v0.5: the draw decides at the completion…
    // …where the exhausted deck fails loud — the completion row refuses (§9.19 analog)
    const lastComplete = game.log.records.filter(r => r.kind === 'complete').at(-1)!
    expect(lastComplete.outcomes.some(o => o.op === 'REFUSE')).toBe(true)
    expect(game.journal().join('\n')).toMatch(/exhausted/)
  })

  test('the whole night replays from the log (state = fold(log))', () => {
    const game = fresh()
    play(game, WIN_PATH)
    const replayed = fold(game.log.records, game['base'], game.dict, game.content)
    expect([...replayed.byName.keys()].sort()).toEqual([...game.state.byName.keys()].sort())
    expect(replayed.now).toBe(game.state.now)
  })
})
