// Scaling test for the reachability analyzer over Cultist-Simulator-shaped
// synthetic content (test/reach-scale-lib.ts — the mechanics mapping is
// docs/reverse/cultist-simulator.md §6; the analyzer's soundness against
// engine truth is reach-fuzz's job, this file pins its SCALING behavior):
//
//   - the world facts (census + fixpoint) are cached per (content, initial):
//     a warm analyzeCard call must be far cheaper than the cold first call
//   - cached and independently-rebuilt worlds produce IDENTICAL reports
//   - the path-export invariants hold at scale (obtainable ⇒ paths ≥ 1,
//     roots mint the probed name, unobtainable ⇒ no roots)
//   - a full-name census completes within a generous wall bound
import { describe, expect, test } from 'vitest'
import { analyzeCard } from '../src/reach/reach.js'
import { buildCsScale, census, summarize, type NameStat } from './reach-scale-lib.js'

// CI-sized slice of CS scale (the full 1,566×2,084 world is `npm run bench:reach`)
const PROTOS = 400
const RECIPES = 400

describe('reach scale — CS-shaped content', () => {
  const world = buildCsScale(PROTOS, RECIPES)

  test('the synthesized world has the expected shape', () => {
    expect(world.stats.protos).toBe(PROTOS)
    expect(world.stats.recipes).toBe(RECIPES)
    expect(world.content.verbs.size).toBe(RECIPES + 1)   // + WAIT
    expect(world.stats.decks).toBe(67)
    expect(world.names.length).toBeGreaterThan(PROTOS)
  })

  test('cold vs warm: the second analysis on the same world amortizes the fixpoint', () => {
    // a fresh world pair pays the census+fixpoint exactly once; every later
    // call is a target lookup + path walk
    const fresh = buildCsScale(PROTOS, RECIPES)
    const probe = fresh.names[fresh.names.length >> 1]!
    const coldStart = performance.now()
    analyzeCard(fresh.content, fresh.initial, probe, {})
    const cold = performance.now() - coldStart
    const warmStart = performance.now()
    for (let i = 0; i < 20; i++) analyzeCard(fresh.content, fresh.initial, fresh.names[i % 100], {})
    const warm = (performance.now() - warmStart) / 20
    expect(cold, 'the cold call must actually pay for the world facts').toBeGreaterThan(15)
    expect(warm * 3, `warm (${warm.toFixed(2)}ms) must amortize the cold fixpoint (${cold.toFixed(1)}ms)`).toBeLessThan(cold)
    expect(warm, 'a warm call is a lookup + path walk, not a re-analysis').toBeLessThan(50)
  })

  test('two independently built worlds produce byte-identical reports (cache purity)', () => {
    const a = buildCsScale(PROTOS, RECIPES)
    const b = buildCsScale(PROTOS, RECIPES)
    const sample = [...a.names.filter((_, i) => i % 25 === 0), ...a.names.slice(-5)]
    for (const name of sample) {
      const ra = analyzeCard(a.content, a.initial, name, {})
      const rb = analyzeCard(b.content, b.initial, name, {})
      expect(JSON.stringify(rb), name).toBe(JSON.stringify(ra))
    }
  })

  test('census: full-name sweep stays sound at scale', () => {
    const stats: NameStat[] = census(world)
    const line = summarize(stats)
    expect(line).toContain(`${stats.length} names`)
    const byVerdict = (v: string): number => stats.filter(st => st.verdict === v).length
    // CS-shaped content with clean chains: most cards are judgeable, and the
    // analyzer never leaves the soundness rails
    expect(byVerdict('obtainable')).toBeGreaterThan(stats.length * 0.3)
    for (const st of stats) {
      if (st.verdict === 'obtainable') expect(st.roots, `${st.name}: obtainable ⇒ a proven path`).toBeGreaterThan(0)
      if (st.verdict === 'unobtainable') expect(st.roots, `${st.name}: unobtainable ⇒ no path`).toBe(0)
    }
  })

  test('path-export invariants over a broad sample (fuzz-style, at scale)', () => {
    let checked = 0
    for (const name of world.names.filter((_, i) => i % 7 === 0)) {
      const rep = analyzeCard(world.content, world.initial, name, {})
      if (rep.verdict !== 'obtainable' && rep.verdict !== 'unusable') continue
      expect(rep.paths.length, name).toBeGreaterThan(0)
      for (const root of rep.paths) expect(root.mints, name).toBe(name)
      checked++
    }
    expect(checked, 'the sample must exercise obtainable names').toBeGreaterThan(30)
  })

  test('demand semantics survive the scale (any obtainable card matches)', () => {
    const rep = analyzeCard(world.content, world.initial, '', { demand: 'aspect:lore>=2' })
    expect(rep.verdict).toBe('obtainable')
    expect(rep.paths.length).toBeGreaterThan(0)
  })
})
