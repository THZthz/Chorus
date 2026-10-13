// Clock & duration unit tests — spec §5.1 grammar, §9.45 compound durations.
import { describe, expect, test } from 'vitest'
import { formatClock, formatDuration, parseClock, parseDuration } from '../src/clock.js'
import { lex } from '../src/lexer.js'
import { coreDictionary } from '../src/dict.js'
import { parseSentence } from '../src/parse.js'
import { formatSentence } from '../src/format.js'

const dict = coreDictionary()

/** parse → format → re-parse → re-format, mirroring sentence.test.ts (§5.2) */
function canonical(src: string): string {
  const s1 = parseSentence(src, dict)
  const c = formatSentence(s1, dict)
  const s2 = parseSentence(c, dict)
  expect(formatSentence(s2, dict)).toBe(c)
  return c
}

describe('parseClock', () => {
  test('accepts day:halfhour', () => {
    expect(parseClock('0:0')).toEqual({ day: 0, halfhour: 0 })
    expect(parseClock('3:47')).toEqual({ day: 3, halfhour: 47 })
  })
  test('day cannot be negative — \\d+ cannot lex a sign', () => {
    expect(() => parseClock('-1:5')).toThrow(/bad clock/)
  })
  test('rejects halfhour out of range and malformed input', () => {
    expect(() => parseClock('0:48')).toThrow(/bad halfhour/)
    expect(() => parseClock('0:-1')).toThrow(/bad clock/)
    expect(() => parseClock('1:')).toThrow(/bad clock/)
    expect(() => parseClock('now')).toThrow(/bad clock/)
  })
})

describe('parseDuration', () => {
  test('single §5.1 units', () => {
    expect(parseDuration('0m')).toBe(0)
    expect(parseDuration('1halfhour')).toBe(30)
    expect(parseDuration('2h')).toBe(120)
    expect(parseDuration('90m')).toBe(90)
    expect(parseDuration('1d')).toBe(1440)
  })
  test('compound literals sum their unit pairs (§9.45)', () => {
    expect(parseDuration('1h30m')).toBe(90)
    expect(parseDuration('2halfhour15m')).toBe(75)
    expect(parseDuration('1d2h30m')).toBe(1590)
  })
  test('rejects unknown units, bare ints, and stray suffixes', () => {
    expect(() => parseDuration('2fortnights')).toThrow(/bad duration/)
    expect(() => parseDuration('12')).toThrow(/bad duration/)
    expect(() => parseDuration('1h30min')).toThrow(/bad duration/)
    expect(() => parseDuration('h')).toThrow(/bad duration/)
  })
})

describe('formatDuration', () => {
  test('prefers the shortest exact single unit', () => {
    expect(formatDuration(0)).toBe('0m')
    expect(formatDuration(30)).toBe('1halfhour')
    expect(formatDuration(60)).toBe('1h')
    expect(formatDuration(90)).toBe('3halfhour') // exact halfhour beats compound
    expect(formatDuration(1440)).toBe('1d')
    expect(formatDuration(1470)).toBe('49halfhour') // exact single unit wins
  })
  test('decomposes only when no single unit fits (§9.45)', () => {
    expect(formatDuration(45)).toBe('45m')
    expect(formatDuration(100)).toBe('1h40m')
    expect(formatDuration(1441)).toBe('1d1m')
    expect(formatDuration(1501)).toBe('1d1h1m')
  })
  test('round-trips through parseDuration (dense sweep)', () => {
    for (let m = 0; m <= 720; m++) expect(parseDuration(formatDuration(m))).toBe(m)
    for (const m of [1441, 1501, 2882, 4321, 59049]) expect(parseDuration(formatDuration(m))).toBe(m)
  })
})

describe('formatClock / parseClock round-trip', () => {
  test('format → parse is the identity', () => {
    for (const day of [0, 1, 12]) for (const halfhour of [0, 23, 47])
      expect(parseClock(formatClock({ day, halfhour }))).toEqual({ day, halfhour })
  })
})

describe('compound literals through the lexer and sentence pipeline (§9.45)', () => {
  test('lexes as ONE dur token — a DUR_RE regression would split it', () => {
    expect(lex('1h30m')).toEqual([{ t: 'dur', v: 90 }])
    expect(lex('1d2h30m')).toEqual([{ t: 'dur', v: 1590 }])
    expect(lex('12')).toEqual([{ t: 'int', v: 12 }]) // no unit → still an int
  })
  test('canonicalizes in an outcome list and round-trips (§5.2)', () => {
    // formatDuration(90) prefers the exact single unit: 1h30m → 3halfhour
    expect(canonical('0:16 @ EVENT dusk_falls -> PROGRESS(1h30m, "tick")'))
      .toBe('0:16 @ EVENT dusk_falls -> PROGRESS(3halfhour, "tick")')
  })
})
