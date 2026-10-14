# Chorus — a text-only card-game prototype

Cultist Simulator / Book of Hours-shaped, expressed entirely as sentences.
Spec: [`docs/spec/text-engine-v0.2.md`](docs/spec/text-engine-v0.2.md).

```
0:14 @ ANA AT HOUSE READ THE_FORGETTING -> SPAWN CLUE_THE_FORGETTING INTO house AS x1_1, NARRATE("...")
```

Actions are sentences (`SUBJECTS VERB [OBJECTS] -> OUTCOMES`), recorded in an
append-only log with a `day:halfhour` clock (day 0-based, 48 halfhours of 30
minutes; durations may be authored in `halfhour` units — `1halfhour` = 30
minutes). Game state is a fold of the log — replay, provenance, and undo
come free.

**No backward compatibility, by design.** Formats and APIs may break between
any two commits (v0.2.3 re-scaled the clock and invalidated every earlier
log). There are no migration shims — re-author or migrate old logs and
scripts by hand.

## Layout

- `src/core/` — `dict` (L1 dictionary: verbs, reserved words, outcome operators, relation types), `sentence` `lexer` `parse` `format` (L2 grammar, canonical form), `query` `predicate` (slot queries §4.5 and recipe conditions §6.3), `types` `clock` `dice` `ref`
- `src/content/` — `content` (verbs/slots/recipes/prototypes/triggers as data), `script` (the script compiler: a text file becomes world + logic)
- `src/engine/` — `engine` (slots → recipes → outcomes → time → events; BEGIN/COMPLETE/WAIT rows, advance passes, station locks; step-atomic REFUSE), `fold` `store` (replay from the log; JSONL persistence)
- `src/reach/` — the static card-reachability analyzer (obtainable / demand / usability judgments)
- `src/ui/` — `lenient` (lenient English-ish player input → canonical steps), `play` (the game loop; one turn = handleLine), `render` (journal / prose / text cards — L3 projections), `tui` (the terminal UI (Ink); readline fallback when stdin or stdout is not a TTY)
- `tools/` — CLIs: `lint-cli` (script linter), `reach-cli` (reachability reports), `reach-fuzz-cli` (seeded differential campaigns)
- `test/seed.ts` — the sample world (the spec's flagship examples) as a test fixture
- `scripts/the-forgotten-house.txt` — a whole game as one text file
- `scripts/the-nightjar-flyer.txt` — a murder mystery on an aether express
  (the engine's whole twelve-verb core set, a timetable that fights back;
  winnable in 9 commands)

## Play

```sh
npm run play                    # pick a script interactively (falls back to the first bundled script when piped)
npm run play -- scripts/the-nightjar-flyer.txt   # or name one directly
npm run play -- my-game.txt     # any script
```

```
◈ Murder on the Nightjar Flyer
you are MARCH · at GUARD VAN · day 0:42 · 9 cards
here: THE GUARD · THE INQUEST
try: read THE MANIFEST · talk with THE GUARD · explore DINING CAR
verbs: read · talk · explore · use on · travel to · wait · study · work · mansus · locate at · dream · acquire
[1 look] [2 cards] [3 story] [4 journal] [5 undo] [6 wait] [7 quit]  ← number + enter
──────────────────────────────────────────────────────────
§ unmask Lord Vane's killer — gather proofs aboard and put them to the inquest before Vienna
» read the manifest
MARCH read THE_MANIFEST. The passenger manifest, first class: the lord Vane…
» talk with the guard
MARCH question THE_GUARD. "Brack. Railway security," he says…
» explore the first cabin
MARCH search FIRST_CABIN.
  (+1halfhour) The seal parts for your warrant with a sound like a struck match.
A steward will remember MARCH asking after things that are none of an inspector's business.
The cabin is orderly the way a stage set is orderly. Behind the writing case…
» inspect the wax cylinder
THE_WAX_CYLINDER  [text]
  aether 2 · secret 2 · tome.cylinder 1
  at: FIREBOX
```

The session opens with the script's `goal:` and a `try:` row of concrete
executable actions (`Game.suggestions()` — state-driven, refreshed every
turn). The player types English-ish text (`read the pale lady`, `explore the
firebox`, `talk with the guard`); the engine assigns the clock, resolves
names fuzzily (case, articles, underscores), and suggests corrections.
The night keeps score, as the steward's line above hints: careless searches
and the conductor's rounds stack SUSPICION — a pool, not a card — and three
copies make the case leave the train.
Strict canonical sentences (`0:20 @ MARCH WAIT`) pass through unchanged.
Characters you have not talked to appear as archetypes (`THE GUARD`) — the
name is learned by meeting them — and the corridor has directions: places
sit on a table laid out by `north`/`south`/`east`/`west` links, and a travel
verb only reaches the next car. Everything the player can do is visible in
the live area — the verb vocabulary, the cards sharing the player's
location, and number buttons for the meta views (look, cards, story,
journal, undo, quit); tab completes verbs and card names.

Committed actions **process** over their verb's duration as *parallel
stations*: committing an action opens a processing window — the verb and
its placed cards are locked until it closes, authored `PROGRESS` ticks
describe what is happening inside it, and outcomes (consume / mutate /
mint) land when the window completes. Several stations run at once; time
passes only when you advance it (type a clock like `1:30`, or press
wait), which resolves due completions and the timetable in one
deterministic pass:

```
» seraphine read the wax cylinder
SERAPHINE read THE_WAX_CYLINDER.
  (+20m) You wind the cylinder onto the phonomotor; the stylus trembles and finds its groove.
  (+50m) A voice assembles out of the hiss — the lord's own, speaking the words that open his seal.
Someone owns the lord's voice, and the seal answers it.
```

## Script format

A game is one text file — world and logic together:

```
script: The Forgotten House
player: ANA
start: 0:14

entities:
  HOUSE is place
  ANA is character with occult:2, lantern:1 at HOUSE
  THE_STUDY is place in HOUSE with study:1
  ANA is parent_child of BERTHA

prototypes:
  CLUE is idea deriving name from object, secret from object.secret-2

triggers:
  dusk_falls at hour 16 -> SPAWN DREAD, NARRATE "The lamps come on."

verbs:
  READ:
    actors: 1..2 kind:character
    object: 1 kind:text
    duration: 1h
    recipes:
      read.magick_gate hidden when object.aspect.magick >= 1 AND NOT any(actors, aspect:occult >= 2) -> REFUSE("The letters swim like minnows.")
      default -> NARRATE "{actors} read {object}."
```

## Run

```sh
npm install
npm test          # vitest — spec examples as acceptance tests
npm run typecheck
npx tsx examples/demo.ts
```

## Try an action

```ts
const r = executeStep(content, dict, state, step, { firstRecordId: log.nextRecordId })
// r.rejected !== null  → slot/recipe rejection, nothing logged
// r.records            → action (+ event) rows; append to the log, state = r.state
```
