// Script compiler (world + logic from one text file) and lenient player input,
// exercised against the Nightjar Flyer: zones, neighbor links, masked suspects.
import { describe, expect, test } from 'vitest'
import { readFileSync } from 'node:fs'
import { compileScript, parseScript } from '../src/script.js'
import { parsePlayerInput } from '../src/lenient.js'
import { Game } from '../src/play.js'
import { fold } from '../src/fold.js'
import { seedDictionary } from '../src/seed.js'

const dict = seedDictionary()
const scriptText = readFileSync(new URL('../scripts/the-nightjar-flyer.txt', import.meta.url), 'utf8')

describe('script compiler', () => {
  const game = compileScript(scriptText, dict)

  test('meta lines set title, player, goal, and the starting clock', () => {
    expect(game.meta).toEqual({
      title: 'Murder on the Nightjar Flyer',
      player: 'MARCH',
      goal: "unmask Lord Vane's killer — gather proofs aboard and put them to the inquest before Vienna",
      start: { day: 0, halfhour: 42 },
    })
    expect(game.state.now).toBe(42 * 30)
  })

  test('entities carry kinds, aspects, and containers', () => {
    const march = game.state.byName.get('MARCH')!
    expect(game.state.entities.get(march)!.kind).toBe('character')
    expect(game.state.entities.get(march)!.attrs['container']).toBe('s_guard_van')
    const guard = game.state.entities.get('s_the_guard')!
    expect(guard.aspects).toEqual({ duty: 2, unmet: 1 })
    expect(guard.attrs['container']).toBe('s_guard_van')
    // a character can carry a card: the claim ticket rides in the guard's pocket
    const ticket = game.state.entities.get('s_the_claim_ticket')!
    expect(ticket.attrs['container']).toBe('s_the_guard')
  })

  test('the train is a zone, not a card — her cars carry it as an attribute', () => {
    expect(game.state.byName.has('NIGHTJAR_FLYER')).toBe(false)
    expect(game.state.entities.get('s_guard_van')!.attrs['zone']).toBe('NIGHTJAR_FLYER')
    expect(game.state.entities.get('s_firebox')!.attrs['zone']).toBe('NIGHTJAR_FLYER')
  })

  test('places sit on the corridor with explicit directions', () => {
    // authored once: FIREBOX is south of GUARD_VAN — the stored edge keeps the
    // authored direction, and queries find it from either endpoint order
    expect(game.state.relations.get('s_firebox|s_guard_van|south')).toMatchObject({ type: 'south' })
    expect(game.state.relations.get('s_dining_car|s_first_cabin|south')).toMatchObject({ type: 'south' })
    const read = game.content.verbs.get('TRAVEL_TO')!
    const gate = read.recipes[0]!
    expect(gate.id).toBe('travel.no_link')
    expect(gate.when).not.toBeNull()
  })

  test('verb blocks compile slots, durations, and recipes', () => {
    const read = game.content.verbs.get('READ')!
    expect(read.duration).toBe(60)
    expect(read.slotByRole.get('object')).toMatchObject({ min: 1, max: 1 })
    expect(read.recipes.map(r => r.id)).toEqual([
      'read.manifest', 'read.french_gate', 'read.cylinder', 'read.telegram', 'read.ledger', 'read.letter', 'read.default',
    ])
    expect(read.recipes[1]!.shown).toBe(false)  // hidden gate
    expect(read.recipes[0]!.when).not.toBeNull()
    expect(read.recipes[6]!.when).toBeNull()    // default
  })

  test('triggers and prototypes compile', () => {
    expect(game.content.triggers.map(t => [t.id, t.slot, t.once])).toEqual([
      ['body_found', 44, true],
      ['night_porter', 46, true],
      ['border_stop', 0, true],
      ['the_long_tunnel', 2, true],
      ['the_furnace_takes_it', 4, true],
      ['vienna_arrival', 8, true],
    ])
    const lateLord = game.content.prototypes.get('THE_LATE_LORD')!
    expect(lateLord).toMatchObject({ kind: 'item', aspects: { aether: 1 } })
    const brack = game.content.prototypes.get('BRACK')!
    expect(brack).toMatchObject({ kind: 'character', aspects: { duty: 2 } })
  })

  test('templates compile', () => {
    expect(game.content.templates.get('READ')).toBe('{actors} read {object}.')
    expect(game.content.templates.get('TALK')).toBe('{actors} question {to}.')
  })

  test('bad scripts fail loudly', () => {
    expect(() => parseScript('entities:\n  X is dragon\n', dict)).toThrow(/unknown kind/)
    expect(() => parseScript('entities:\n  A is pals of B\n', dict)).toThrow(/unknown kind/) // 'pals' is not a registered relation
    expect(() => parseScript('verbs:\n  FLY:\n    duration: 1h\n', dict)).toThrow(/not registered/)
    expect(() => parseScript('nonsense line\n', dict)).toThrow(/section header/)
  })
})

describe('lenient player input', () => {
  const game = Game.fromScript(scriptText, dict)
  const ctx = { dict, content: game.content, state: game.state, player: 'MARCH' }

  test('bare verb defaults to the player character', () => {
    const r = parsePlayerInput('wait', ctx)
    expect(r.ok).toBe(true)
    if (r.ok) {
      expect(r.step.clock).toEqual({ day: 0, halfhour: 42 })
      expect(r.step.clauses[0]).toMatchObject({ verb: 'WAIT', actors: [{ form: 'name', name: 'MARCH' }] })
    }
  })

  test('multi-actor and multi-word names resolve', () => {
    const r = parsePlayerInput('march read the vane ledger', ctx)
    expect(r.ok).toBe(true)
    if (r.ok) {
      const c = r.step.clauses[0]
      expect(c.verb).toBe('READ')
      expect(c.objects[0]).toMatchObject({ name: 'THE_VANE_LEDGER' })
    }
  })

  test('role phrases and objects do not bleed into each other', () => {
    const r = parsePlayerInput('use on the inquest with the manifest at guard van', ctx)
    expect(r.ok).toBe(true)
    if (r.ok) {
      const c = r.step.clauses[0]
      expect(c.verb).toBe('USE_ON')
      expect(c.objects[0]).toMatchObject({ name: 'THE_INQUEST' })
      expect(c.with[0]).toMatchObject({ name: 'THE_MANIFEST' })
      expect(c.at).toMatchObject({ name: 'GUARD_VAN' })
    }
  })

  test('canonical passthrough still works', () => {
    const r = parsePlayerInput('0:44 @ MARCH WAIT', ctx)
    expect(r.ok).toBe(true)
    if (r.ok) expect(r.step.clock).toEqual({ day: 0, halfhour: 44 })
  })

  test('unknown words get suggestions', () => {
    const r = parsePlayerInput('read the vane ledgr', ctx)
    expect(r.ok).toBe(false)
    if (!r.ok) expect(r.suggestions).toContain('THE_VANE_LEDGER')
  })

  test('verbs get suggestions too', () => {
    const r = parsePlayerInput('xeplore the car', ctx)
    expect(r.ok).toBe(false)
    if (!r.ok) expect(r.suggestions).toContain('explore')
  })

  test('names resolve case-insensitively (spec §11)', () => {
    const r = parsePlayerInput('read THE VANE LEDGER', ctx)
    expect(r.ok).toBe(true)
  })
})

describe('the game loop', () => {
  test('a script becomes a playable game', () => {
    const game = Game.fromScript(scriptText, dict)
    const turn = game.handleLine('read the manifest')
    expect(turn.kind).toBe('action')
    expect(turn.lines.join(' ')).toContain('passenger manifest')

    // the timetable speaks: the body is found on the 22:00 crossing
    game.handleLine('wait')
    expect(game.log.records.some(r => r.kind === 'event' && r.trigger === 'body_found')).toBe(true)
    expect(game.journal().join('\n')).toContain('EVENT body_found')
    expect(game.story().join('\n')).toContain('entirely dead')
    expect(game.look().join('\n')).toContain('You are at')

    // undo takes back the wait, leaving the log consistent
    const nBefore = game.log.records.length
    expect(game.undo().kind).toBe('info')
    expect(game.log.records.length).toBeLessThan(nBefore)
    expect(game.handleLine('bogus').kind).toBe('error')
  })

  test('the table only shows what the inspector has met', () => {
    const game = Game.fromScript(scriptText, dict)
    const names = () => game.visible().map(e => e.name)
    expect(names()).toContain('THE_GUARD')           // the archetype you can see
    expect(names()).not.toContain('BRACK')           // the name is learned by talking
    expect(names()).not.toContain('THE_WIDOW')       // another car: not yet
    expect(names()).not.toContain('THE_VANE_LEDGER') // the sealed cabin

    game.handleLine('explore the dining car')
    expect(names()).toContain('THE_WIDOW')           // seen, but not known
    expect(names()).not.toContain('VERITY')

    game.handleLine('talk with the widow')
    expect(names()).toContain('VERITY')              // met: the name is known
    expect(names()).not.toContain('THE_WIDOW')
  })

  test('inspect reads a visible card\'s face and respects the table', () => {
    const game = Game.fromScript(scriptText, dict)
    const face = game.inspect('the manifest')
    expect(face[0]).toBe('THE_MANIFEST  [text]')
    expect(face.join('\n')).toContain('manifest 1')
    expect(face.join('\n')).toContain('at: MARCH')
    // the sealed cabin's contents are not on the table yet
    expect(game.inspect('the vane ledger')[0]).toContain('nothing on the table')
    expect(game.inspect('the loch ness monster')[0]).toContain('nothing on the table')
  })

  test('suggestions are concrete, executable actions', () => {
    const game = Game.fromScript(scriptText, dict)
    const s = game.suggestions()
    expect(s.length).toBeGreaterThan(0)
    for (const line of s) {
      const r = parsePlayerInput(line, { dict, content: game.content, state: game.state, player: 'MARCH' })
      expect(r.ok).toBe(true)
    }
    expect(s[0]).toBe('read THE MANIFEST')
  })

  test('the log replays from the script base state (state = fold(log))', () => {
    const game = Game.fromScript(scriptText, dict)
    game.handleLine('read the manifest')
    const replayed = fold(game.log.records, game['base'], game.dict, game.content)
    expect([...replayed.byName.keys()].sort()).toEqual([...game.state.byName.keys()].sort())
    expect(replayed.now).toBe(game.state.now)
  })
})
