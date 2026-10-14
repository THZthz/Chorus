// Script compiler (world + logic from one text file) and lenient player input,
// exercised against the Nightjar Flyer: zones, neighbor links, masked suspects.
import { describe, expect, test } from 'vitest'
import { readFileSync } from 'node:fs'
import { coreDictionary } from '../src/core/dict.js'
import { compileScript, parseScript } from '../src/content/script.js'
import { compileContent, forEachAuthoredOutcome } from '../src/content/content.js'
import { parsePlayerInput } from '../src/ui/lenient.js'
import { Game } from '../src/ui/play.js'
import { fold } from '../src/engine/fold.js'
import { addEntity } from '../src/core/types.js'
import { seedState } from './seed.js'

const dict = coreDictionary()

/** v0.5 test adapter: settle each committed action's window right away, so
 *  the loop tests keep their pre-parallel commit-completes-at-once shape */
function autoSettle(game: Game): Game {
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
const scriptText = readFileSync(new URL('../scripts/the-nightjar-flyer.txt', import.meta.url), 'utf8')

describe('script compiler', () => {
  const game = compileScript(scriptText, dict)

  test('meta lines set title, player, goal, the starting clock, and the dice seed', () => {
    expect(game.meta).toEqual({
      title: 'Murder on the Nightjar Flyer',
      player: 'MARCH',
      goal: "unmask Lord Vane's killer — gather proofs aboard and put them to the inquest before Vienna",
      start: { day: 0, halfhour: 42 },
      seed: 1899,
    })
    expect(game.state.now).toBe(42 * 30)
  })

  test('entities carry kinds, aspects, and containers', () => {
    const march = game.state.byName.get('MARCH')![0]
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
      'read.manifest', 'read.french_gate', 'read.cylinder', 'read.telegram', 'read.ledger', 'read.letter', 'read.companions', 'read.default',
    ])
    expect(read.recipes[1]!.shown).toBe(false)  // hidden gate
    expect(read.recipes[0]!.when).not.toBeNull()
    expect(read.recipes[7]!.when).toBeNull()    // default
  })

  test('triggers and prototypes compile', () => {
    // the flyer's timetable is mostly clock triggers: conductor_rounds and
    // dawn_watch recur (no `once`), the_long_tunnel shows the `at hour N`
    // alias form (hour 1 = slot 2), and lord_tended is an expiry trigger
    // (§9.32), which narrows as [id, onExpireOf] instead
    expect(game.content.triggers.map(t => 'slot' in t ? [t.id, t.slot, t.once] : [t.id, t.onExpireOf])).toEqual([
      ['body_found', 44, true],
      ['night_porter', 46, true],
      ['conductor_rounds', 45, false],
      ['border_stop', 0, true],
      ['the_long_tunnel', 2, true],
      ['the_furnace_takes_it', 4, true],
      ['dawn_watch', 8, false],
      ['lord_tended', 'THE_LATE_LORD'],
      ['vienna_arrival', 12, true],
    ])
    const lateLord = game.content.prototypes.get('THE_LATE_LORD')!
    expect(lateLord).toMatchObject({ kind: 'item', aspects: { aether: 1 } })
    const brack = game.content.prototypes.get('BRACK')!
    expect(brack).toMatchObject({ kind: 'character', aspects: { duty: 2 } })
  })

  test('the v0.4 sections compile: pools, deck, endings, and expanded functions', () => {
    expect(game.content.seed).toBe(1899) // the header's documented, calibrated seed
    // pools: SUSPICION and the gossip cards are declared stackable
    for (const name of ['SUSPICION', 'RUMOUR', 'HEARSAY', 'SCANDAL']) {
      expect(game.content.stackableNames.has(name)).toBe(true)
    }
    // the deck: resolved entries, the seed-fixed permutation of the 2:1:1
    // spec, and its exhaustion rule
    const deck = game.content.decks.get('RUMOURS')!
    expect(deck.order.map(c => c.name)).toEqual(['SCANDAL', 'RUMOUR', 'RUMOUR', 'HEARSAY']) // permutation at seed 1899
    expect(deck.order.map(c => c.kind)).toEqual(['idea', 'idea', 'idea', 'idea'])
    expect(deck.defaultcard).toMatchObject({ name: 'RUMOUR', kind: 'idea' })
    expect(deck.reset).toBe(true)
    // the escalation ending is declared (patrol_check's ENDING resolves at load)
    expect(game.content.endings.get('OFF_THE_FLYER')).toMatchObject({ title: 'Off the Flyer at Speed', flavour: 'melancholy' })
    // functions expand away at load (there is no runtime function table):
    // the compiled bodies carry the call sites' splices
    const explore = game.content.verbs.get('EXPLORE')!
    // patrol_check expanded into the cabin recipe — its ROLL is there
    expect(explore.recipes.find(r => r.id === 'explore.cabin')!.out.some(o => o.op === 'ROLL')).toBe(true)
    // sus expanded with the {who} interpolation baked at the call site
    const steward = explore.recipes.find(r => r.id === 'explore.searched')!.out.find(o => o.op === 'NARRATE')?.args[0]
    expect(steward && steward.t === 'str' ? steward.v : '').toContain('A steward will remember MARCH')
    // the nerve pool feeds the rude searches' dice
    const luggage = explore.recipes.find(r => r.id === 'explore.luggage')!.out
    expect(luggage.some(o => o.op === 'ROLL')).toBe(true)
    expect(luggage.some(o => o.op === 'IF')).toBe(true)
    // the patrol trigger carries its expanded DRAW + patrol_check roll
    const rounds = game.content.triggers.find(t => 'slot' in t && t.id === 'conductor_rounds')!
    expect('slot' in rounds && rounds.out.some(o => o.op === 'DRAW')).toBe(true)
    expect('slot' in rounds && rounds.out.some(o => o.op === 'ROLL')).toBe(true)
  })

  test('the showcase compiles the full script grammar (§4–§10)', () => {
    // entity lines: stackable (§9.38) on the resource kind (§4.1), plus a
    // symmetric relation (§4.2) and a derive prototype (§6.4, §9.23)
    const funds = game.state.entities.get('s_funds')!
    expect(funds).toMatchObject({ kind: 'resource', stackable: true })
    expect(game.content.stackableNames.has('FUNDS')).toBe(true)
    expect(game.state.relations.get('s_firebox|s_guard_van|neighbor')).toMatchObject({ type: 'neighbor', value: 1 })
    const hunch = game.content.prototypes.get('HUNCH')!
    expect(hunch.derive).toEqual({
      nameFrom: 'object',
      aspects: { hunch: { fromObjectAspect: 'secret', minus: 1 } },
    })
    expect(hunch.aspects).toEqual({ aether: 1 })
    // the bare deck line: no defaultcard, no reset (§9.42)
    const dreams = game.content.decks.get('DREAMS')!
    expect(dreams.order.map(c => c.name)).toEqual(['VOID', 'PORTENT', 'PORTENT']) // permutation at seed 1899
    expect(dreams.defaultcard).toBeNull()
    expect(dreams.reset).toBe(false)
    // the ending table: explicit flavour, grand flavour, and the default (§9.35)
    expect(game.content.endings.get('QUIET_NIGHT')).toEqual({ title: 'The Flyer Reaches Vienna', flavour: 'melancholy' })
    expect(game.content.endings.get('CASE_CLOSED')).toMatchObject({ flavour: 'grand' })
    // slot-count vocabulary (§6.1): exact 2, open-ended 2.. and 0..
    expect(game.content.verbs.get('MANSUS')!.slotByRole.get('actors')).toMatchObject({ min: 2, max: 2 })
    expect(game.content.verbs.get('WORK')!.slotByRole.get('actors')).toMatchObject({ min: 2, max: Infinity })
    expect(game.content.verbs.get('DREAM')!.slotByRole.get('with')).toMatchObject({ min: 0, max: Infinity })
    // slot queries (§4.5): aspect level, negated aspect, and the state flag
    const workAt = game.content.verbs.get('WORK')!.slotByRole.get('at')!.query!
    expect(workAt).toHaveLength(2)
    expect(game.content.verbs.get('DREAM')!.slotByRole.get('with')!.query).toHaveLength(3)
    // compound durations (§9.45): 1h30m in a verb config and a PROGRESS tick
    expect(game.content.verbs.get('STUDY')!.duration).toBe(90)
    // the id-less recipe mints a stable id; the shown gate carries its hint (§6.2)
    const studyGate = game.content.verbs.get('STUDY')!.recipes[0]!
    expect(studyGate.id).toBe('study.recipe0')
    expect(studyGate.shown).toBe(true)
    expect(studyGate.hint).toBe('Whoever studies must read the tongue.')
    const inquestGate = game.content.verbs.get('USE_ON')!.recipes.find(r => r.id === 'inquest.short')!
    expect(inquestGate!.shown).toBe(true)
    expect(inquestGate!.hint).toContain('three proofs')
    // outcome ops new to the showcase, where the story authors them
    const out = (verb: string, id: string) => game.content.verbs.get(verb)!.recipes.find(r => r.id === id)!.out
    expect(out('USE_ON', 'inquest.win').map(o => o.op)).toEqual(
      ['PROGRESS', 'PROGRESS', 'NARRATE', 'SPAWN', 'REPLACE', 'ENDING']) // the win ends the story (§9.35)
    expect(out('ACQUIRE', 'acquire.claim').map(o => o.op)).toEqual(['SET', 'MOVE', 'SET', 'NARRATE'])
    expect(out('DREAM', 'dream.seal').map(o => o.op)).toEqual(['DRAW', 'ADVANCE', 'NARRATE'])
    // REVEAL rides inside the luggage find's taken-branch IF (§9.39): walk arms
    const ops: string[] = []
    forEachAuthoredOutcome(out('EXPLORE', 'explore.luggage'), o => { ops.push(o.op) })
    expect(ops).toContain('REVEAL')
    expect(out('EXPLORE', 'explore.searched').some(o => o.op === 'IF')).toBe(true) // the SPAWN-then-count escalation (§6.8)
    expect(out('WAIT', 'wait.till_dawn').some(o => o.op === 'ADVANCE')).toBe(true)
    const meetGuard = game.content.verbs.get('TALK')!.recipes.find(r => r.id === 'talk.meet_guard')!.out
    expect(meetGuard.some(o => o.op === 'RELATE')).toBe(true)
    // triggers: RELATE/UNRELATE corridor closures and the expiry REPLACE
    const tunnel = game.content.triggers.find(t => 'slot' in t && t.id === 'the_long_tunnel')!
    expect('slot' in tunnel && tunnel.out.some(o => o.op === 'UNRELATE')).toBe(true)
    const dawn = game.content.triggers.find(t => 'slot' in t && t.id === 'dawn_watch')!
    expect('slot' in dawn && dawn.out.some(o => o.op === 'RELATE')).toBe(true)
    const tended = game.content.triggers.find(t => 'onExpireOf' in t && t.id === 'lord_tended')!
    expect('onExpireOf' in tended && tended.out.some(o => o.op === 'REPLACE')).toBe(true)
  })

  test('templates compile', () => {
    expect(game.content.templates.get('READ')).toBe('{actors} read {object}.')
    expect(game.content.templates.get('TALK')).toBe('{actors} question {to}.')
    // all twelve core verbs carry a prose template
    expect(game.content.templates.size).toBe(12)
  })

  test('expiry triggers parse into their own trigger form (§9.32)', () => {
    const script = parseScript('triggers:\n  decay_moth on expire of INFLUENCE_MOTH_G -> REPLACE INFLUENCE_MOTH_G WITH INFLUENCE_MOTH_E\n', dict)
    expect(script.content.triggers).toEqual([
      { id: 'decay_moth', onExpireOf: 'INFLUENCE_MOTH_G', out: ['REPLACE INFLUENCE_MOTH_G WITH INFLUENCE_MOTH_E'] },
    ])
    // compiled: the expire variant with parsed outcomes; no existence check on the name
    const compiled = compileContent(script.content, dict)
    expect(compiled.triggers).toEqual([
      {
        id: 'decay_moth', onExpireOf: 'INFLUENCE_MOTH_G',
        out: [{
          op: 'REPLACE',
          args: [
            { t: 'ref', ref: { form: 'name', name: 'INFLUENCE_MOTH_G' } },
            { t: 'ref', ref: { form: 'name', name: 'INFLUENCE_MOTH_E' } },
          ],
        }],
      },
    ])
  })

  test('bad expiry-trigger grammar fails loudly', () => {
    expect(() => parseScript('triggers:\n  decay_moth on expire -> SPAWN DREAD\n', dict)).toThrow(/triggers expect/)
    expect(() => parseScript('triggers:\n  decay_moth expire of MOTH -> SPAWN DREAD\n', dict)).toThrow(/triggers expect/)
    expect(() => parseScript('triggers:\n  decay_moth on expire of MOTH\n', dict)).toThrow(/triggers expect/)
  })

  test('bad scripts fail loudly', () => {
    expect(() => parseScript('entities:\n  X is dragon\n', dict)).toThrow(/unknown kind/)
    expect(() => parseScript('entities:\n  A is pals of B\n', dict)).toThrow(/unknown kind/) // 'pals' is not a registered relation
    expect(() => parseScript('verbs:\n  FLY:\n    duration: 1h\n', dict)).toThrow(/not registered/)
    expect(() => parseScript('nonsense line\n', dict)).toThrow(/section header/)
  })
})

describe('lenient player input', () => {
  const game = autoSettle(Game.fromScript(scriptText, dict))
  const ctx = { dict, content: game.content, state: game.state, player: 'MARCH' }

  test('bare verb defaults to the player character', () => {
    const r = parsePlayerInput('wait', ctx)
    expect(r.ok).toBe(true)
    if (r.ok) {
      expect(r.step.startMin).toBe(42 * 30) // v0.5: the default begin minute is NOW
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

describe('lenient input routes the new core verbs (v0.3)', () => {
  // registration is the gate: before v0.3 this compileContent threw 'not registered'
  const verbsDict = coreDictionary()
  const verbsContent = compileContent({
    verbs: {
      STUDY: { // READ's grammar (§4.3)
        slots: [
          { role: 'actors', min: 1, max: 2, query: 'kind:character' },
          { role: 'object', min: 1, max: 1, query: 'kind:text' },
          { role: 'at', min: 0, max: 1, query: 'kind:place' },
        ],
        duration: 60,
        recipes: [{ out: ['NARRATE("{actors} study {object}.")'] }],
      },
      MANSUS: { // EXPLORE's grammar (§4.3)
        slots: [
          { role: 'actors', min: 1, max: 1, query: 'kind:character' },
          { role: 'object', min: 1, max: 1, query: 'kind:place' },
        ],
        duration: 60,
        recipes: [{ out: ['NARRATE("{actors} walk the Mansus to {object}.")'] }],
      },
    },
  }, verbsDict)
  const ctx = () => {
    const state = seedState()
    addEntity(state, { id: 'manifest', kind: 'text', name: 'THE_MANIFEST', aspects: {}, attrs: { container: 'house' }, bornAction: null, deadAction: null, stackable: false })
    addEntity(state, { id: 'white_door', kind: 'place', name: 'THE_WHITE_DOOR', aspects: {}, attrs: { container: 'villa' }, bornAction: null, deadAction: null, stackable: false })
    return { dict: verbsDict, content: verbsContent, state, player: 'ANA' }
  }

  test('study + bare name routes to the object role, like read', () => {
    const r = parsePlayerInput('study the manifest', ctx())
    expect(r.ok).toBe(true)
    if (r.ok) {
      expect(r.step.clauses[0]).toMatchObject({
        verb: 'STUDY',
        actors: [{ form: 'name', name: 'ANA' }],
        objects: [{ form: 'name', name: 'THE_MANIFEST' }],
      })
    }
  })

  test('mansus + place routes to the object role, like explore', () => {
    const r = parsePlayerInput('mansus the white door', ctx())
    expect(r.ok).toBe(true)
    if (r.ok) {
      expect(r.step.clauses[0]).toMatchObject({
        verb: 'MANSUS',
        actors: [{ form: 'name', name: 'ANA' }],
        objects: [{ form: 'name', name: 'THE_WHITE_DOOR' }],
      })
    }
  })
})

describe('the game loop', () => {
  test('a script becomes a playable game', () => {
    const game = autoSettle(Game.fromScript(scriptText, dict))
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
    const game = autoSettle(Game.fromScript(scriptText, dict))
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
    const game = autoSettle(Game.fromScript(scriptText, dict))
    const face = game.inspect('the manifest')
    expect(face[0]).toBe('THE_MANIFEST  [text]')
    expect(face.join('\n')).toContain('manifest 1')
    expect(face.join('\n')).toContain('at: MARCH')
    // the sealed cabin's contents are not on the table yet
    expect(game.inspect('the vane ledger')[0]).toContain('nothing on the table')
    expect(game.inspect('the loch ness monster')[0]).toContain('nothing on the table')
  })

  test('suggestions are concrete, executable actions', () => {
    const game = autoSettle(Game.fromScript(scriptText, dict))
    const s = game.suggestions()
    expect(s.length).toBeGreaterThan(0)
    for (const line of s) {
      const r = parsePlayerInput(line, { dict, content: game.content, state: game.state, player: 'MARCH' })
      expect(r.ok).toBe(true)
    }
    expect(s[0]).toBe('read THE MANIFEST')
  })

  test('the log replays from the script base state (state = fold(log))', () => {
    const game = autoSettle(Game.fromScript(scriptText, dict))
    game.handleLine('read the manifest')
    const replayed = fold(game.log.records, game['base'], game.dict, game.content)
    expect([...replayed.byName.keys()].sort()).toEqual([...game.state.byName.keys()].sort())
    expect(replayed.now).toBe(game.state.now)
  })
})
