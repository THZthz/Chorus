// Shared full-state snapshot for fold-equality assertions (spec §7, plan law 6).
// ONE helper so play/fold comparisons cannot drift — engine.test,
// endings.test and nightjar-explorer.test all compare through this. New State
// fields land here with the state itself (v0.4's rng/decks landed with F4/F5).
import type { State } from '../src/types.js'

export function snap(s: State): string {
  return JSON.stringify({
    ents: [...s.entities.entries()].sort(([a], [b]) => a < b ? -1 : 1),
    rels: [...s.relations.entries()].sort(([a], [b]) => a < b ? -1 : 1),
    byName: [...s.byName.entries()].sort(([a], [b]) => a < b ? -1 : 1), // pools included (§9.38)
    now: s.now,
    rng: s.rng, // §9.41: the dice counter — play and fold must land it identically
    decks: [...s.decks.entries()].sort(([a], [b]) => a < b ? -1 : 1), // §9.42: the deck cursors
    fired: [...s.fired].sort(), // §9.34: play and fold must agree on fired too
    ended: s.ended, // §9.35: …and on the story's ending
    // v0.5: the open windows — a log may END with them (quit mid-process), so
    // fold equality must see them; sorted by key for a stable snapshot
    windows: [...s.windows.entries()]
      .sort(([a], [b]) => a < b ? -1 : 1)
      .map(([k, w]) => [k, { ...w }]),
  })
}
