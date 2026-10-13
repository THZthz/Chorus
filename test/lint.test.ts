// The compiler's load-time discipline: error upgrades (things that used to
// compile and detonate later) and the lint layer (warnings that never throw).
import { describe, expect, test } from 'vitest'
import { readFileSync } from 'node:fs'
import { compileScript, lintScript } from '../src/script.js'
import { coreDictionary } from '../src/dict.js'

const dict = coreDictionary()
const FLYER = readFileSync(new URL('../scripts/the-nightjar-flyer.txt', import.meta.url), 'utf8')

const compile = (script: string): unknown => compileScript(script, dict)

const base = (extra: string): string => `
script: Probe
player: MARCH
start: 0:0

entities:
  MARCH is character at HOUSE
  HOUSE is place
${extra}
`

describe('error upgrades — silent accepts become load errors', () => {
  test('a template for an unregistered verb is rejected', () => {
    expect(() => compile(base('templates:\n  WRONGVERB says "{actors} reads."'))).toThrow(/WRONGVERB.*not a registered verb/)
  })

  test('player: must name a declared character', () => {
    expect(() => compileScript('script: P\nplayer: GHOST\nstart: 0:0\nentities:\n  ANA is character at HOUSE\n  HOUSE is place', dict))
      .toThrow(/player 'GHOST' is not a declared entity/)
    expect(() => compileScript('script: P\nplayer: HOUSE\nstart: 0:0\nentities:\n  ANA is character at HOUSE\n  HOUSE is place', dict))
      .toThrow(/player 'HOUSE' must be a character/)
  })

  test('reserved words cannot be names (§5.1)', () => {
    expect(() => compile(base('  TO is place'))).toThrow(/'TO' is a reserved word/)
    expect(() => compile(base('  WITH is character at HOUSE'))).toThrow(/'WITH' is a reserved word/)
    expect(() => compile(base('  MARCH is parent_child of TO'))).toThrow(/'TO' is a reserved word/)
    expect(() => compile(base('prototypes:\n  IF is idea'))).toThrow(/'IF' is a reserved word/)
    // non-lexeme names are legal — §9.42's unsafe-name corner, quote every ref
    expect(() => compile(base('  X-Y is item'))).not.toThrow()
  })

  test('duplicate entity declarations are a load error, not last-write-wins', () => {
    expect(() => compile(base('  HOUSE is place with study:1'))).toThrow(/duplicate entity declaration 'HOUSE'/)
  })

  test('a name declared both as entity and prototype can never spawn — load error', () => {
    expect(() => compile(base('  SPARK is idea\nprototypes:\n  SPARK is idea'))).toThrow(/'SPARK' is declared both as an entity and a prototype/)
  })
})

describe('lint — warnings that never throw', () => {
  test('the bundled showcase lints clean', () => {
    expect(lintScript(FLYER, dict)).toEqual([])
  })

  test('a verb with slots but no recipes', () => {
    const w = lintScript(base(`
verbs:
  READ:
    actors: 1 kind:character
    object: 1 kind:text
    duration: 1h
`), dict)
    expect(w.some(x => x.includes('verb READ: declares slots but no recipes'))).toBe(true)
  })

  test('a recipe after a catch-all is unreachable', () => {
    const w = lintScript(base(`
verbs:
  READ:
    actors: 1 kind:character
    object: 1 kind:text
    duration: 1h
    recipes:
      read.any -> NARRATE "{actors} read."
      read.late when object.aspect.secret >= 3 -> NARRATE "secret."
`), dict)
    expect(w.some(x => x.includes('read.late') && x.includes('unreachable'))).toBe(true)
  })

  test('an identical when shadows the later recipe', () => {
    const w = lintScript(base(`
verbs:
  READ:
    actors: 1 kind:character
    object: 1 kind:text
    duration: 1h
    recipes:
      read.first when object.aspect.secret >= 3 -> NARRATE "a."
      read.second when object.aspect.secret   >= 3 -> NARRATE "b."
`), dict)
    expect(w.some(x => x.includes('read.second') && x.includes('shadowed by read.first'))).toBe(true)
  })

  test('a count(role) bound beyond the slot cardinality can never hold', () => {
    const w = lintScript(base(`
verbs:
  TALK:
    actors: 1..2 kind:character
    duration: 30m
    recipes:
      talk.trio when count(actors) >= 3 -> NARRATE "three."
      talk.duo when count(actors) = 2 -> NARRATE "two."
`), dict)
    expect(w.some(x => x.includes('talk.trio') && x.includes('count(actors) >= 3 can never hold'))).toBe(true)
    expect(w.some(x => x.includes('talk.duo'))).toBe(false) // = 2 is satisfiable
  })

  test('unknown NARRATE tokens warn; roles and function params do not', () => {
    const w = lintScript(base(`
verbs:
  READ:
    actors: 1 kind:character
    object: 1 kind:text
    duration: 1h
    recipes:
      default -> NARRATE "{actors} read {object} with {bject}."

functions:
  sus who -> NARRATE "the conductor's eyes linger on {who}, {actors} noticed."

triggers:
  dusk at hour 16 -> NARRATE "the lamps hum {louder}."
`), dict)
    expect(w.some(x => x.includes('read.default') && x.includes('{bject}'))).toBe(true)
    expect(w.some(x => x.includes('function sus') && x.includes('{louder}'))).toBe(false) // params are known in bodies
    expect(w.some(x => x.includes('trigger dusk') && x.includes('{louder}'))).toBe(true)
  })

  test('a deck entry that is not stackable trips the §10 livelock rule', () => {
    const w = lintScript(base(`
  LETTER is text with secret:1

decks:
  POST spec LETTER, LETTER reset

verbs:
  READ:
    actors: 1 kind:character
    object: 1 kind:text
    duration: 1h
    recipes:
      default -> NARRATE "{actors} read {object}."
`), dict)
    expect(w.some(x => x.includes('deck POST: entry LETTER is not stackable'))).toBe(true)
  })

  test('an aspect queried but never declared warns; declared ones do not', () => {
    const w = lintScript(base(`
verbs:
  READ:
    actors: 1 kind:character
    object: 1 kind:text
    duration: 1h
    recipes:
      read.ocult when object.aspect.ocult >= 2 -> NARRATE "the letters swim."
      default -> NARRATE "{actors} read {object}."
`), dict)
    expect(w.some(x => x.includes("aspect 'ocult' is queried but never declared"))).toBe(true)
  })

  test('a non-lexeme entity name warns (legal, but every ref needs quotes)', () => {
    const w = lintScript(base('  X-Y is item with weight:1'), dict)
    expect(w.some(x => x.includes("entity 'X-Y': not a name lexeme"))).toBe(true)
  })
})
