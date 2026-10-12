// Shared full-state snapshot for fold-equality assertions (spec §7, plan law 6).
// ONE helper so play/fold comparisons cannot drift — engine.test,
// endings.test and nightjar-explorer.test all compare through this. New State
// fields land here with the state itself (v0.4's rng/decks will extend it).
import type { State } from '../src/types.js'

export function snap(s: State): string {
  return JSON.stringify({
    ents: [...s.entities.entries()].sort(([a], [b]) => a < b ? -1 : 1),
    rels: [...s.relations.entries()].sort(([a], [b]) => a < b ? -1 : 1),
    byName: [...s.byName.entries()].sort(([a], [b]) => a < b ? -1 : 1), // pools included (§9.38)
    now: s.now,
    fired: [...s.fired].sort(), // §9.34: play and fold must agree on fired too
    ended: s.ended, // §9.35: …and on the story's ending
  })
}
