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

This repository is a text-only card-game engine implemented in Rust —
deterministic end to end (same dice rolls, same log bytes, same error text,
all pinned by the acceptance suite):

- `src/core/` — `dict` (L1 dictionary: verbs, reserved words, outcome operators, relation types), `sentence` `lexer` `parse` `format` (L2 grammar, canonical form), `query` `predicate` (slot queries §4.5 and recipe conditions §6.3), `types` `clock` `dice` `ref` `roles`, `text` (the frozen whitespace/collation classes), `jsonw` (canonical JSON writer + object-key order), `regex_util` `num_parse`
- `src/content/` — `content` (verbs/slots/recipes/prototypes/triggers as data), `script` (the script compiler: a text file becomes world + logic)
- `src/engine/` — `engine` (slots → recipes → outcomes → time → events; BEGIN/COMPLETE/WAIT rows, advance passes, station locks; step-atomic REFUSE), `fold` `store` (replay from the log; JSONL persistence)
- `src/reach/` — the static card-reachability analyzer (`census` `world` `fixpoint` `paths` `render`; obtainable / demand / usability judgments)
- `src/session/` — `lenient` (lenient English-ish player input → canonical steps), `game` (the game loop; one turn = handle_line), `render` (journal / prose / text cards — L3 projections)
- `src/error.rs` / `src/lcg.rs` — the single error channel; the deterministic test-side RNG
- `src/bin/` — `chorus` (line REPL), `lint-scripts` (script linter), `reach` (reachability reports), `reach-fuzz` (seeded differential campaigns), `reach-scale` (CS-scale benchmarks)
- `tests/` — 700+ acceptance tests
- `scripts/the-nightjar-flyer.txt` — a murder mystery on an aether express
  (the engine's whole twelve-verb core set, a timetable that fights back;
  winnable in 9 commands)

## Play

```sh
cargo run --bin chorus -- scripts/the-nightjar-flyer.txt   # or any script file
cargo run --bin chorus -- my-game.txt
```

```
◈ Murder on the Nightjar Flyer
you are MARCH · at GUARD VAN · day 0:42 · 9 cards
here: THE GUARD · THE INQUEST
try: read THE MANIFEST · talk with THE GUARD · explore DINING CAR
verbs: read · talk · explore · use on · travel to · wait · study · work · mansus · locate at · dream · acquire
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
executable actions (`session::game`'s `suggestions()` — state-driven, refreshed every
turn). The player types English-ish text (`read the pale lady`, `explore the
firebox`, `talk with the guard`); the engine assigns the clock, resolves
names fuzzily (case, articles, underscores), and suggests corrections.
The night keeps score, as the steward's line above hints — careless searches
and the conductor's rounds stack SUSPICION — a pool, not a card — and three
copies make the case leave the train.
Strict canonical sentences (`0:20 @ MARCH WAIT`) pass through unchanged.
Characters you have not talked to appear as archetypes (`THE GUARD`) — the
name is learned by meeting them — and the corridor has directions: places
sit on a table laid out by `north`/`south`/`east`/`west` links, and a travel
verb only reaches the next car. Everything the player can do is visible in
the live area — the verb vocabulary and the cards sharing the player's
location; the meta views (`look`, `cards`, `story`, `journal`, `undo`,
`wait`, `quit`) are one-word commands, and `inspect <card>` prints any card
in play.

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
cargo test                # 700+ acceptance tests
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

The engine has bugs and inconsistencies that are, by now, behavior: tests pin
them, logs encode them, and "fixing" one would break byte-identity with every
recorded log. This repository therefore reproduces each one as-is — the
source rows implementing each quirk carry a `// QUIRK(<id>):` comment at
their site (deliberately dead code carries a `// NOTE` instead), and changing
any of them is a behavior change, not a cleanup.

| # | quirk (frozen, not fixed) | lives at |
|---|---------------------------|----------|
| Q1 | recipe ids are 0-based when compiled (`.recipe0`) but 1-based in the lint mirror (`.recipe1`) | `content.rs` / `script.rs` |
| Q2 | the predicate string tokenizer silently swallows illegal characters (`aspect:x >= @3` ≡ `aspect:x>=3`) | `predicate.rs` |
| Q3 | `arg()` cannot read a param key (`SET(myparam = 3)` is a parse error) while SPAWN args support one | `parse.rs` |
| Q4 | FIXED (batch 1): slot roles are validated at load — a typo'd slot is a load error naming the six roles, not a silent never-match | `script.rs` |
| Q5 | FIXED (batch 1): duplicate declarations are a uniform load error in every section (entities, prototypes, templates, verbs, decks, endings); `ANA`/`ana` case-collide on the minted id `s_ana` and that collision is its own load error | `script.rs` |
| Q6 | a BEGIN line whose format∘parse round-trip fails silently opens no window | `engine.rs` / `fold.rs` |
| Q7 | a REFUSED step's NARRATE carries the rehearsed dice numbers; the counter never moved | `engine.rs` |
| Q8 | JS `\s` ≠ Rust `\s` (`\uFEFF`, `\u{85}`) — every regex uses the explicit JS class | `core/text.rs` `JS_WS_CLASS` |
| Q9 | `DUR_RE` alternation order `halfhour\|h\|m\|d` is load-bearing (`1halfhour` would mis-lex as `1h`) | `lexer.rs` |
| Q10 | quoted-string `\X` takes the next character literally (`"\n"` is the letter n); a trailing backslash still throws unterminated | `lexer.rs` |
| Q11 | bare `inspect` shows the first visible card (`startsWith('')` is always true) | `game.rs`, `bin/chorus.rs` |
| Q12 | ROLES are soft-reserved in outcome-arg position (`as_role` intercepts before bare refs) but are not reserved words | `parse.rs` |
| Q13 | aspects are one level deep with last-write-wins flattened keys (`'a.b.c'` lands as `{a:{'b.c':v}}`) | `types.rs` `AspectVal` |
| Q14 | same-ref comparison errors on a param ref instead of returning false; relation keys trust the engine's `#hex` ids against `\|` collisions | `ref.rs` |
| Q15 | CLI current shapes: a trailing `--demand` silently vanishes; the linter exits 1 on a missing directory; an unknown meta key is blamed on "bad clock" | `bin/reach.rs`, `bin/lint-scripts.rs`, `script.rs` |
| Q16 | a `-0` delta formats as `+0`; `SET(x,-0)` can be authored | `format.rs` |
| Q17 | reach `site_reason`'s empty-init-names branch is reachable (zero-entity scripts are legal), not dead code | `reach/paths.rs` |

Suggestions that were rejected exactly because they would change behavior:
worklist/event-driven reach fixpoints (the naive round-robin is the only known
byte-identical schedule), load-time rejection of integer-like keys (use the
JS `OrdinaryOwnPropertyKeys` key order instead), unifying the recipe-id
bases, tightening slot-role validation or duplicate-declaration handling, and
merging the two deliberately different string/pair arg spellings.
