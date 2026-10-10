# Chorus — a text-only card-game prototype

Cultist Simulator / Book of Hours-shaped, expressed entirely as sentences.
Spec: [`docs/spec/text-engine-v0.2.md`](docs/spec/text-engine-v0.2.md).

```
0:14 @ ANA AT HOUSE READ THE_FORGETTING -> SPAWN CLUE_THE_FORGETTING INTO house AS x1_1, NARRATE("...")
```

Actions are sentences (`SUBJECTS VERB [OBJECTS] -> OUTCOMES`), recorded in an
append-only log with a `day:hour` clock (day 0-based). Game state is a fold of
the log — replay, provenance, and undo come free.

## Layout

- `src/dict.ts` — L1 dictionary: verbs, reserved words, outcome operators, relation types
- `src/sentence.ts` `lexer.ts` `parse.ts` `format.ts` — L2 grammar, canonical form
- `src/query.ts` `predicate.ts` — slot queries (§4.5) and recipe conditions (§6.3)
- `src/content.ts` `seed.ts` — verbs/slots/recipes/prototypes/triggers as data; the sample world
- `src/engine.ts` — slots → recipes → outcomes → time → events; step-atomic REFUSE
- `src/fold.ts` `store.ts` — replay from the log; JSONL persistence
- `src/render.ts` — journal / prose / text cards (L3 projections)

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
