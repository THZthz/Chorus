// Script compiler (world + logic from one text file) and lenient player input.
import { describe, expect, test } from 'vitest'
import { readFileSync } from 'node:fs'
import { compileScript, parseScript } from '../src/script.js'
import { parsePlayerInput } from '../src/lenient.js'
import { Game } from '../src/play.js'
import { fold } from '../src/fold.js'
import { seedDictionary } from '../src/seed.js'

const dict = seedDictionary()
const scriptText = readFileSync(new URL('../scripts/the-forgotten-house.txt', import.meta.url), 'utf8')

describe('script compiler', () => {
  const game = compileScript(scriptText, dict)

  test('meta lines set title, player, and the starting clock', () => {
    expect(game.meta).toEqual({ title: 'The Forgotten House', player: 'ANA', start: { day: 0, halfhour: 28 } })
    expect(game.state.now).toBe(28 * 30)
  })

  test('entities carry kinds, aspects, and containers', () => {
    const ana = game.state.byName.get('ANA')!
    expect(game.state.entities.get(ana)!.kind).toBe('character')
    expect(game.state.entities.get(ana)!.aspects).toEqual({ occult: 2, lantern: 1 })
    expect(game.state.entities.get(ana)!.attrs['container']).toBe('s_house')
    const study = game.state.entities.get('s_study')!
    expect(study.attrs['container']).toBe('s_house')
    expect(study.aspects).toEqual({ study: 1 })
  })

  test('relation sentences create the social graph', () => {
    expect(game.state.relations.get('s_ana|s_bertha|parent_child')).toMatchObject({ a: 's_ana', b: 's_bertha' })
  })

  test('verb blocks compile slots, durations, and recipes', () => {
    const read = game.content.verbs.get('READ')!
    expect(read.duration).toBe(60)
    expect(read.slotByRole.get('actors')).toMatchObject({ min: 1, max: 2 })
    expect(read.recipes.map(r => r.id)).toEqual([
      'read.magick_gate', 'read.parent_child', 'read.unrelated_pair', 'read.secret', 'read.default',
    ])
    expect(read.recipes[0].shown).toBe(false)   // hidden gate
    expect(read.recipes[0].when).not.toBeNull()
    expect(read.recipes[4].when).toBeNull()     // default
  })

  test('shown gates carry hints from the script', () => {
    const useOn = game.content.verbs.get('USE_ON')!
    expect(useOn.recipes[0].shown).toBe(true)
    expect(useOn.recipes[0].hint).toContain('pooled lantern')
  })

  test('triggers and prototypes compile', () => {
    expect(game.content.triggers).toEqual([{ id: 'dusk_falls', slot: 32, once: false, out: expect.any(Array) }])
    const clue = game.content.prototypes.get('CLUE')!
    expect(clue.derive).toEqual({ nameFrom: 'object', aspects: { secret: { fromObjectAspect: 'secret', minus: 2 } } })
  })

  test('templates compile', () => {
    expect(game.content.templates.get('READ')).toBe('{actors} read {object} at {at}.')
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
  const ctx = { dict, content: game.content, state: game.state, player: 'ANA' }

  test('bare verb defaults to the player character', () => {
    const r = parsePlayerInput('read the forgetting', ctx)
    expect(r.ok).toBe(true)
    if (r.ok) {
      expect(r.step.clock).toEqual({ day: 0, halfhour: 28 })
      expect(r.step.clauses[0]).toMatchObject({ verb: 'READ', actors: [{ form: 'name', name: 'ANA' }], objects: [{ form: 'name', name: 'THE_FORGETTING' }] })
    }
  })

  test('multi-actor, multi-word names, ABOUT and AT phrases', () => {
    const r = parsePlayerInput('ana, bertha talk about clue secret hideout at dining hall', ctx)
    expect(r.ok).toBe(true)
    if (r.ok) {
      const c = r.step.clauses[0]
      expect(c.verb).toBe('TALK')
      expect(c.actors.map((a: import('../src/ref.js').Ref) => a.form === 'name' && a.name).sort()).toEqual(['ANA', 'BERTHA'])
      expect(c.about).toMatchObject({ name: 'CLUE_SECRET_HIDEOUT' })
      expect(c.at).toMatchObject({ name: 'DINING_HALL' })
    }
  })

  test('role phrases and objects do not bleed into each other', () => {
    const r = parsePlayerInput('use on rite of ascension with chalice at house', ctx)
    expect(r.ok).toBe(true)
    if (r.ok) {
      const c = r.step.clauses[0]
      expect(c.verb).toBe('USE_ON')
      expect(c.objects[0]).toMatchObject({ name: 'RITE_OF_ASCENSION' })
      expect(c.with).toEqual([{ form: 'name', name: 'CHALICE' }])
      expect(c.at).toMatchObject({ name: 'HOUSE' })
    }
  })

  test('canonical passthrough still works', () => {
    const r = parsePlayerInput('0:40 @ ANA WAIT', ctx)
    expect(r.ok).toBe(true)
    if (r.ok) expect(r.step.clock).toEqual({ day: 0, halfhour: 40 })
  })

  test('unknown words get suggestions', () => {
    const r = parsePlayerInput('read the pale ladt', ctx)
    expect(r.ok).toBe(false)
    if (!r.ok) expect(r.suggestions).toContain('THE_PALE_LADY')
  })

  test('verbs get suggestions too', () => {
    const r = parsePlayerInput('raed a book', ctx)
    expect(r.ok).toBe(false)
    if (!r.ok) expect(r.suggestions).toContain('read')
  })

  test('verb suggestions match multi-word verbs on their first word', () => {
    const r = parsePlayerInput('tavel to the hall', ctx)
    expect(r.ok).toBe(false)
    if (!r.ok) expect(r.suggestions).toContain('travel to')
  })

  test("'with X' folds into the verb's real slot — a second actor for TALK here", () => {
    const r = parsePlayerInput('talk with bertha about clue secret hideout', ctx)
    expect(r.ok).toBe(true)
    if (r.ok) {
      const c = r.step.clauses[0]
      expect(c.verb).toBe('TALK')
      expect(c.with).toEqual([])
      expect(c.actors.map((a: import('../src/ref.js').Ref) => a.form === 'name' && a.name).sort()).toEqual(['ANA', 'BERTHA'])
      expect(c.about).toMatchObject({ name: 'CLUE_SECRET_HIDEOUT' })
    }
  })
})

describe('the game loop', () => {
  test('a script becomes a playable game', () => {
    const game = Game.fromScript(scriptText, dict)
    const turn = game.handleLine('read the forgetting')
    expect(turn.kind).toBe('action')
    expect(turn.lines.join(' ')).toContain('ANA read THE_FORGETTING')

    // dusk is reachable by waiting through hour 16
    game.handleLine('wait')
    const dusk = game.handleLine('wait') // 15→16? 16→17: crosses 16
    void dusk
    expect(game.log.records.some(r => r.kind === 'event' && r.trigger === 'dusk_falls')).toBe(true)

    // the REFUSE path through lenient input
    const refuse = game.handleLine('bertha read magick book')
    expect(refuse.kind).toBe('action')
    expect(refuse.lines.join(' ')).toContain('The letters swim like minnows.')

    // undo takes back the refusal, leaving the log consistent
    const nBefore = game.log.records.length
    expect(game.handleLine('/undo').kind).toBe('info')
    expect(game.log.records.length).toBeLessThan(nBefore)

    // slash views
    expect(game.handleLine('/journal').lines.join('\n')).toContain('EVENT dusk_falls')
    expect(game.handleLine('/look').lines.join('\n')).toContain('You are at')
    expect(game.handleLine('/bogus').kind).toBe('error')
  })

  test('log replays from the script base state (state = fold(log))', () => {
    const game = Game.fromScript(scriptText, dict)
    game.handleLine('read the forgetting')
    game.handleLine('wait')
    expect(replayNames(game).get('CLUE_THE_FORGETTING')).toBeDefined()
  })
})

// small helper: refold the log through the public surface
function replayNames(game: Game) {
  const state = fold(game.log.records, game['base'], game.dict, game.content)
  return state.byName
}
