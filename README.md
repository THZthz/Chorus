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

- `src/dict.ts` — L1 dictionary: verbs, reserved words, outcome operators, relation types
- `src/sentence.ts` `lexer.ts` `parse.ts` `format.ts` — L2 grammar, canonical form
- `src/query.ts` `predicate.ts` — slot queries (§4.5) and recipe conditions (§6.3)
- `src/content.ts` — verbs/slots/recipes/prototypes/triggers as data
- `src/script.ts` — the script compiler: a text file becomes world + logic (§ "sentences + small blocks")
- `src/lenient.ts` — lenient English-ish player input → canonical steps
- `src/engine.ts` — slots → recipes → outcomes → time → events; step-atomic REFUSE
- `src/fold.ts` `store.ts` — replay from the log; JSONL persistence
- `src/play.ts` — the game loop and terminal REPL
- `src/seed.ts` — the same sample world in TypeScript (tests)
- `src/render.ts` — journal / prose / text cards (L3 projections)
- `scripts/the-forgotten-house.txt` — a whole game as one text file
- `scripts/the-nightjar-flyer.txt` — a murder mystery on an aether express
  (five verbs, a timetable that fights back; winnable in ~8 commands)

## Play

```sh
npm run play                    # plays scripts/the-forgotten-house.txt
npm run play -- scripts/the-nightjar-flyer.txt   # the murder-mystery story
npm run play -- my-game.txt     # any script
```

```
── The Forgotten House ──  (type /help)
» read the forgetting
ANA read THE_FORGETTING at HOUSE. ANA tease out a hidden thread.
» ana, bertha read fairy tales
ANA and BERTHA read FAIRY_TALES at HOUSE. …the child sleeps wiser.
» wait
» /story
```

The player types English-ish text (`read the pale lady`, `use on rite of
ascension with chalice at house`, `talk to cole about the aether seal`); the
engine assigns the clock, resolves names fuzzily (case, articles,
underscores), and suggests corrections. Strict canonical sentences
(`0:20 @ ANA WAIT`) pass through unchanged. Slash commands: `/look`,
`/cards`, `/journal`, `/story`, `/undo`, `/quit`.

Committed actions **process** over their verb's duration — the verb and its
placed cards are locked for that window, authored `PROGRESS` ticks describe
what is happening inside it, and outcomes (consume / mutate / mint) land
when the bar completes:

```
» seraphine read the wax cylinder
SERAPHINE read THE_WAX_CYLINDER.
  (+20m) You wind the cylinder onto the phonomotor; the stylus trembles and finds its groove.
  (+50m) A voice assembles out of the hiss — the lord's own, speaking the words that open his seal.
Someone owns the lord's voice.
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
  STUDY is place in HOUSE with study:1
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
