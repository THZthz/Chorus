# Chorus — a text-only card-game prototype

Cultist Simulator / Book of Hours-shaped, expressed entirely as sentences.
Spec: [`docs/spec/text-engine-v0.7.md`](docs/spec/text-engine-v0.7.md).

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

This repository is a text-only card-game engine implemented in Rust —
deterministic end to end (same dice rolls, same log bytes, same error text,
all pinned by the acceptance suite):

- `src/core/` — `dict` (L1 dictionary: verbs, reserved words, outcome operators, relation types), `sentence` `lexer` `parse/` (`mod` is the sentence grammar + token primitives, `outcome` the outcome-list grammar) `format` (L2 grammar, canonical form), `query` `predicate` (slot queries §4.5 and recipe conditions §6.3; `pred_parse` / `pred_lint` are its parser and load-time-lint halves), `fn_expand` (§10 call expansion: functions and script ops), `types` `clock` `dice` `ref` `roles`, `text` (the frozen whitespace/collation classes), `jsonw` (canonical JSON writer + object-key order + the aspects/attrs pair values), `regex_util` `num_parse`
- `src/content/` — `model` (verbs/slots/recipes/prototypes/triggers/decks as data: authored `ContentRaw` and compiled `CompiledContent`), `compile` (the compiler + load-time guards), `deck` (§4.4 permutation + draw rule), `functions` (§10 call tables: functions and script ops), `script/` (the script compiler: a text file becomes world + logic — `parse` `decls` `verbblock` `complete` `load` `lint` `when_dead`)
- `src/engine/` — `exec` (the step interpreter: slots → recipes → outcomes → time → events; step-atomic REFUSE), `validate` (slot arity/queries, recipe selection, station locks), `outcomes/` (§4.4 application: the op dispatch, `mint` rules, the RUN `chain`), `advance` (the advance pass: window completions, slot triggers, expiries), `replay` + `fold` (state = fold(log)), `record` (the BEGIN/COMPLETE/WAIT/EVENT rows), `store` (JSONL persistence), `settle` (the run-then-settle convenience the tests and campaigns share)
- `src/reach/` — the static card-reachability analyzer (`census` `world` `fixpoint` `paths` `render`; obtainable / demand / usability judgments) plus the campaign engines the bins and the tests share (`fuzz` — seeded differential campaigns against engine truth; `scale` — the CS-scale synthetic world)
- `src/session/` — `lenient` (lenient English-ish player input → canonical steps), `game` (the game loop; one turn = handle_line), `render` (journal / prose / text cards — L3 projections)
- `src/error.rs` / `src/lcg.rs` — the single error channel; the deterministic campaign-side RNG
- `src/bin/` — `chorus` (line REPL), `lint-scripts` (script linter), `reach` (reachability reports), `reach-fuzz` (seeded differential campaigns), `reach-scale` (CS-scale benchmarks)
- `tests/` — 820 acceptance tests; `tests/common/` is the shared harness (assertions, snapshots, seed fixtures, the nightjar explorer), mounted once per test binary
- `scripts/the-nightjar-flyer.txt` — a murder mystery on an aether express
  (the engine's whole twelve-verb core set, a timetable that fights back;
  the acceptance path wins in seven actions with a `wait` after each — the
  inquest's ninety-minute window must close before Vienna at 6:00, and the
  goal line says so)

## Play

```sh
cargo run --bin chorus -- scripts/the-nightjar-flyer.txt   # or any script file
cargo run --bin chorus -- my-game.txt
```

```
── Murder on the Nightjar Flyer ──
§ unmask Lord Vane's killer — gather proofs aboard and put them to the inquest before Vienna, where the Flyer arrives at six
you are MARCH · at GUARD VAN · day 0, 21:00
here: THE GUARD · THE INQUEST
try: read the manifest · talk to the guard · explore dining car
verbs: read · talk · explore · use on · travel to · wait · study · work · mansus · locate at · dream · acquire
type `help` for how to play
» look
You are at GUARD VAN — day 0, 21:00.
  THE GUARD  [character]
  THE INQUEST  [rite]
exits: north — LUGGAGE VAN · south — FIREBOX
carrying: THE MANIFEST
» talk to the guard
MARCH question THE GUARD.

« 0:42 @ MARCH TALK TO THE_GUARD »
now: day 0, 21:00 · processing: talk until 21:30
try: read the manifest · explore dining car · explore firebox · wait
» wait
"Brack. Railway security," he says, and the white gloves settle. "They wired
ahead about an inspector. The company seal is in my van — and somebody's
hands have been on it."
  (+1halfhour) time passes.
now: day 0, 21:30
try: read the manifest · talk to brack · explore dining car
» inspect brack
BRACK  [character]
  duty 2 · nerve 2
  at: GUARD VAN
  entered play at action 2
```

The session opens with the script's title, its `goal:` line, where and when
you stand, who shares the place, a `try:` row of actions you can type as
shown, and the verb vocabulary; `help` explains the grammar in the game's
own words. The player types English-ish text (`read the manifest`, `explore
the firebox`, `question the guard`); the engine assigns the clock, resolves
names fuzzily (case, articles, underscores, a single distinctive word), picks
the card that fits the verb when a word is ambiguous, echoes the canonical
sentence it executed in `« … »`, and on a miss suggests a corrected line to
type back. People go BEFORE the verb — `seraphine read the telegram`,
`march brack work` — and the try row shows that shape whenever a verb needs
company. The night keeps score, as the steward's line above hints —
careless searches and the conductor's rounds stack SUSPICION — a pool, not a
card — and three copies make the case leave the train. Strict canonical
sentences (`0:20 @ MARCH WAIT`) pass through unchanged. Characters you have
not talked to appear as archetypes (`THE GUARD`) — the name is learned by
meeting them — and the corridor has directions: `look` lists the exits, laid
out by the script's `north`/`south`/`east`/`west` links, and a travel verb
only reaches the next car. The meta views (`look`, `cards`, `inspect <card>`,
`story`, `journal`, `undo`, `wait`, `help`, `quit`) are one-word commands.
In a terminal the `» ` prompt is drawn; a piped session echoes each line
instead, so transcripts read like the one above.

Committed actions **process** over their verb's duration as *parallel
stations*: committing an action opens a processing window — the verb and
its placed cards are locked until it closes, authored `PROGRESS` ticks
describe what is happening inside it, and outcomes (consume / mutate /
mint) land when the window completes. The `now:` line after a turn shows
the clock and what is cooking until when; several stations run at once; time
passes only when you advance it (type `wait`, or a clock like `1:4`), which
resolves due completions and the timetable in one deterministic pass — and
the night ends where the story does (an ending closes the pass):

```
» read the manifest
MARCH read THE MANIFEST.

« 0:42 @ MARCH READ THE_MANIFEST »
now: day 0, 21:00 · processing: read until 22:00
try: talk to the guard · explore dining car · march the guard work at firebox · wait
» wait
The passenger manifest, first class: the lord Vane, his wife Verity, a
secretary named Quill, a spiritualist trading as Seraphine. Crew of two on
the sheet — a guard and a stoker. Six strangers and a dead man, once the cry
goes up.
A cry from the first cabin. Lord Vane lies behind his own aether-seal,
unrobbed, unwounded, and entirely dead.
  (+1h) time passes.
now: day 0, 22:00
try: read the manifest · talk to the guard · explore dining car
```

## Script format

A game is one text file — world and logic together:

```
script: The Forgotten House
player: ANA
start: 0:14

kinds:
  omen   # new noun classes (spec '4.1): one lower_snake name per line

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
  INTERROGATE:                       # a brand-new verb (spec §4.3): any UPPER_SNAKE
    actors: 1 kind:character         # lexeme that is not a reserved word and not
    object: 1 kind:character         # an entity/prototype name; the block makes it playable
    duration: 30m
    recipes:
      default -> NARRATE "{actors} question {object}."
```

## Run

```sh
cargo test                # 820 acceptance tests
cargo test --test demo    # the demo world's fold-verify smoke
cargo build --release     # chorus / lint-scripts / reach / reach-fuzz / reach-scale land in target/release/
```

## Try an action

```rust
let r = execute_step(&content, &dict, state, step,
                     ExecOptions { first_record_id: log.next_record_id(), id_gen: None })?;
// r.rejected.is_some() → slot/recipe rejection, nothing logged
// r.records           → action (+ event) rows; append to the log, state = r.state
```

## Appendix: the frozen quirk list (Q1–Q17)

The engine had bugs and inconsistencies that were, by now, behavior: tests
pinned them, logs encoded them, and "fixing" one would break byte-identity
with every recorded log. The idiomatization plan's phase 3 settled every row:
the ten approved fixes LANDED (their rows describe the new behavior); the
rest are annotated no-fix / spec-pinned / implementation note — those keep
their `// QUIRK(<id>):` comment at the source row (deliberately dead code
carries a `// NOTE` instead) and stay byte-still.

| # | quirk (status after phase 3) | lives at |
|---|------------------------------|----------|
| Q1 | FIXED (batch 2): recipe ids are 0-based everywhere — the lint mirror now mints the compile side's ids (`.recipe0`) | `content/compile.rs` / `content/script/lint.rs` |
| Q2 | FIXED (batch 1): the predicate tokenizer is a load error on grammar-illegal characters (naming the char and its offset); whitespace stays glue | `core/pred_parse.rs` |
| Q3 | FIXED (batch 2): a param in a `key=value` pair position in outcome args gets the teach error (how a param may stand: bare, signed delta, ref/INTO; only a SPAWN pair takes params) — no more bare `expected ')', got '='` | `core/parse/outcome.rs` |
| Q4 | FIXED (batch 1): slot roles are validated at load — a typo'd slot is a load error naming the six roles, not a silent never-match | `content/script/verbblock.rs` |
| Q5 | FIXED (batch 1): duplicate declarations are a uniform load error in every section (entities, prototypes, templates, verbs, decks, endings); `ANA`/`ana` case-collide on the minted id `s_ana` and that collision is its own load error | `content/script/parse.rs` |
| Q6 | FIXED (batch 3): a BEGIN row whose sentence fails the format∘parse re-parse is a loud play/fold drift error naming the row, the sentence and the parse failure — it no longer silently opens no window | `engine/exec.rs` / `engine/fold.rs` |
| Q7 | SPEC-PINNED (§4.4): a REFUSED step's NARRATE carries the rehearsed dice numbers; the counter never moved — spec behavior, not a defect | `engine/outcomes/mod.rs` |
| Q8 | NO-FIX (verified): the DSL `\s` class ≠ Rust `\s` (`\uFEFF`, `\u{85}`) — every regex keeps the explicit frozen class, the engine's frozen choice (the divergence is unreachable from bundled content and the fix has the widest regression surface) | `core/text.rs` `DSL_WS_CLASS` |
| Q9 | IMPLEMENTATION NOTE: the `DUR_RE` alternation order `halfhour\|h\|m\|d` is load-bearing — the behavior itself is spec-pinned (§5.1, `1halfhour` is ONE token) | `core/lexer.rs` |
| Q10 | FIXED (batch 3): quoted strings carry real escape semantics — `\n` is a newline, `\t` a tab, `\r`/`\b`/`\f`/`\v`/`\0` likewise, `\"`/`\\` as before, an unknown escape keeps its literal unit; a trailing backslash still throws unterminated | `core/lexer.rs` |
| Q11 | FIXED (batch 2): bare `inspect` names the usage and lists the visible table — it never guesses the first card | `session/game.rs`, `bin/chorus.rs` |
| Q12 | NO-FIX (refuted): roles soft-reserved in outcome-arg position is §10's pinned shadowing order ("before entity names and before role names"), and §5.1's reserved-word list deliberately excludes the role names — the current behavior IS the spec | `core/parse/outcome.rs` |
| Q13 | NO-FIX (refuted): one-level aspects with last-write-wins flattened dot-keys are §4.2's pinned data model ("weighted principle tags, dot sub-keys") — a nested tree would exceed the spec | `core/types.rs` `AspectVal` |
| Q14 | NO-FIX (unreachable): the TS "same-ref errors on a param ref" half died with the port (dead code, never ported); the remaining relation-key half trusts the engine-minted `#hex` ids against `\|` collisions — an input the engine can never produce | `core/types.rs` |
| Q15 | FIXED (batch 2): the CLIs say what failed — a trailing `--demand` requires a value, a missing scripts directory is its own error, an unknown meta key is blamed on the key (not "bad clock") | `bin/reach.rs`, `bin/lint-scripts.rs`, `content/script/parse.rs` |
| Q16 | NO-FIX (refuted, vacuous): i64 has no `-0` and the parser already normalizes `-0` to a zero delta; "canonicalizing at parse" is a no-op, and the real alternatives would break §4.4's delta-vs-absolute grammar distinction | `core/format.rs` |
| Q17 | NOT A DEFECT: reach `site_reason`'s empty-init-names branch is reachable (zero-entity scripts are legal) — a "not dead code" note, no fix object | `reach/render.rs` |

Suggestions that were rejected exactly because they would change behavior:
worklist/event-driven reach fixpoints (the naive round-robin is the only known
byte-identical schedule), load-time rejection of integer-like keys (use the
canonical `OrdinaryOwnPropertyKeys` key order instead), and merging the two
deliberately different string/pair arg spellings. Those surface conventions
are KEPT as this engine's own frozen choices (plan B-18–B-21):
the exact error-message wordings, `cmp_utf16` collation, the
`OrdinaryOwnPropertyKeys` key order + the canonical JSON escape set
(`jsonw`), and the
`cmp_ascii_ci` suggestion sort — each documented at its site.
