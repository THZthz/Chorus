// Spec-conformance tests: every §5.2 worked example parses, canonicalizes,
// and round-trips. Plus actor-order equality (§9.6), sugar canonicalization,
// reserved words (§9.14).
import { describe, expect, test } from 'vitest'
import { coreDictionary } from '../src/dict.js'
import { parseSentence } from '../src/parse.js'
import { formatSentence } from '../src/format.js'

const dict = coreDictionary()

function canonical(src: string): string {
  const s1 = parseSentence(src, dict)
  const c = formatSentence(s1, dict)
  const s2 = parseSentence(c, dict) // canonical text must re-parse
  expect(formatSentence(s2, dict)).toBe(c) // and re-format to itself
  return c
}

describe('spec §5.2 worked examples', () => {
  test('solo read with AT context; bare-ref shorthand becomes SPAWN', () => {
    expect(canonical('0:14 @ ANA AT HOUSE READ THE_FORGETTING -> CLUE_SECRET_HIDEOUT'))
      .toBe('0:14 @ ANA AT HOUSE READ THE_FORGETTING -> SPAWN CLUE_SECRET_HIDEOUT')
  })

  test('two characters talking — actor order is not semantic (§9.6)', () => {
    const a = canonical('2:19 @ ANA, BERTHA AT DINING_HALL TALK ABOUT CLUE_SECRET_HIDEOUT -> SPAWN PLAN_INFILTRATION AS #p1')
    const b = canonical('2:19 @ BERTHA, ANA AT DINING_HALL TALK ABOUT CLUE_SECRET_HIDEOUT -> SPAWN PLAN_INFILTRATION AS #p1')
    expect(a).toBe(b)
    expect(a).toBe('2:19 @ ANA, BERTHA AT DINING_HALL TALK ABOUT CLUE_SECRET_HIDEOUT -> SPAWN PLAN_INFILTRATION AS #p1')
  })

  test('two-clause step', () => {
    expect(canonical('0:14 @ ANA TRAVEL_TO VILLA; ANA AT STUDY READ THE_PALE_LADY -> CLUE_SECRET_HIDEOUT'))
      .toBe('0:14 @ ANA TRAVEL_TO VILLA; ANA AT STUDY READ THE_PALE_LADY -> SPAWN CLUE_SECRET_HIDEOUT')
  })

  test('event sentence', () => {
    // NARRATE's canonical form is paren-less; SPAWN stays pretty
    expect(canonical('0:16 @ EVENT dusk_falls -> SPAWN DREAD, NARRATE("The lamps come on.")'))
      .toBe('0:16 @ EVENT dusk_falls -> SPAWN DREAD, NARRATE "The lamps come on."')
    // …but the paren form parses equally
    expect(canonical('0:16 @ EVENT dusk_falls -> SPAWN DREAD, NARRATE "The lamps come on."'))
      .toBe('0:16 @ EVENT dusk_falls -> SPAWN DREAD, NARRATE "The lamps come on."')
  })

  test('rite with instruments and INTO', () => {
    expect(canonical('1:23 @ ANA, MY_FOLLOWER AT HOUSE USE_ON RITE_OF_ASCENSION WITH CHALICE -> DESTROY(CHALICE), SPAWN MARK_OF_ASCENSION INTO HOUSE'))
      .toBe('1:23 @ ANA, MY_FOLLOWER AT HOUSE USE_ON RITE_OF_ASCENSION WITH CHALICE -> DESTROY(CHALICE), SPAWN MARK_OF_ASCENSION INTO HOUSE')
  })
})

describe('canonicalization', () => {
  test('typed ref sugar (kind:NAME) drops the kind in canonical form', () => {
    expect(canonical('ANA READ text:THE_PALE_LADY AT STUDY'))
      .toBe('ANA AT STUDY READ THE_PALE_LADY')
  })

  test('post-verbal AT moves pre-verbal (§9.10)', () => {
    expect(canonical('ANA READ THE_PALE_LADY AT STUDY'))
      .toBe('ANA AT STUDY READ THE_PALE_LADY')
  })

  test('id refs and quoted names round-trip', () => {
    expect(canonical('#ana READ "ODD NAME, WITH COMMA"'))
      .toBe('#ana READ "ODD NAME, WITH COMMA"')
  })

  test('role refs in outcomes render uppercase (MOVE ACTORS INTO OBJECT)', () => {
    expect(canonical('ANA TRAVEL_TO VILLA -> MOVE ACTORS INTO OBJECT'))
      .toBe('ANA TRAVEL_TO VILLA -> MOVE(ACTORS, INTO OBJECT)')
  })
})

describe('SET deltas and aspect keys (§9.31)', () => {
  test('signed values parse as deltas and print with their sign', () => {
    expect(canonical('0:0 @ ANA WAIT -> SET(ANA, level, +1)'))
      .toBe('0:0 @ ANA WAIT -> SET(ANA, level, +1)')
    expect(canonical('0:0 @ ANA WAIT -> SET(ANA, level, -2)'))
      .toBe('0:0 @ ANA WAIT -> SET(ANA, level, -2)')
  })

  test('aspect: keys keep their prefix; dotted attr keys print bare (§5.1 dottedName)', () => {
    expect(canonical('0:0 @ ANA WAIT -> SET(ANA, aspect:dread, -1)'))
      .toBe('0:0 @ ANA WAIT -> SET(ANA, aspect:dread, -1)')
    expect(canonical('0:0 @ ANA WAIT -> SET(ANA, aspect:language.latin, 2)'))
      .toBe('0:0 @ ANA WAIT -> SET(ANA, aspect:language.latin, 2)')
    expect(canonical('0:0 @ ANA WAIT -> SET(ANA, state.sealed, true)'))
      .toBe('0:0 @ ANA WAIT -> SET(ANA, state.sealed, true)')
  })

  test('deltas are integers only — a duration after a sign is a parse error (§9.31)', () => {
    expect(() => parseSentence('0:0 @ ANA WAIT -> SET(ANA, funds, +2h)', dict)).toThrow(/integers only/)
    expect(() => parseSentence('0:0 @ ANA WAIT -> SET(ANA, funds, +)', dict)).toThrow(/expected an integer/)
  })
})

describe('lexical discipline (§9.14)', () => {
  test('reserved words cannot be names', () => {
    expect(() => parseSentence('0:0 @ ABOUT READ X', dict)).toThrow(/reserved/)
    expect(() => parseSentence('ANA READ INTO', dict)).toThrow(/reserved/)
    expect(() => parseSentence('ANA WITH VILLA', dict)).toThrow(/registered verb/) // WITH can't start the verb slot either
  })

  test('unregistered verbs are rejected at the actors/verb boundary', () => {
    expect(() => parseSentence('ANA FLY VILLA', dict)).toThrow(/registered verb/)
  })

  test('names colliding with verbs must be quoted', () => {
    expect(() => parseSentence('0:0 @ READ TALK ABOUT X', dict)).toThrow(/reserved/)
    expect(canonical('0:0 @ "READ" TALK ABOUT X')).toBe('0:0 @ "READ" TALK ABOUT X')
  })
})
