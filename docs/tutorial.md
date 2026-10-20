# The Chorus Tutorial

Chorus is a text-only card-game engine in the shape of Cultist Simulator /
Book of Hours: a whole game is **one script file** — places, people, cards,
verbs, recipes, triggers — and playing it means typing English-ish sentences
(`read the manifest`, `wait`) at a line REPL. Two bundled analyzers read your
script statically: `reach` tells you what the player can and cannot obtain,
and `lint-scripts` names authoring mistakes.

This tutorial assumes you have **never read the spec or the source**. Every
command and every script in it was run against the real binaries; every
transcript is real output (occasionally trimmed with `…`). The example
scripts live beside this file, in [`docs/examples/`](examples/), and you can
play each one yourself as you read.

Contents:

1. [Five-minute quick start](#1-five-minute-quick-start)
2. [Write your first script](#2-write-your-first-script) — seven small games that grow
3. [Reading `reach`](#3-reading-reach)
4. [Reading `lint-scripts`](#4-reading-lint-scripts)
5. [CLI cheat sheet](#5-cli-cheat-sheet)
6. [Troubleshooting FAQ](#6-troubleshooting-faq)

Throughout, `»` marks a line typed at the game prompt and `« … »` is the
engine's canonical echo of what actually happened — the engine, not your
wording, is the truth.

---

## 1. Five-minute quick start

Build everything:

```sh
cargo build
```

You get five binaries in `target/debug/` (or `target/release/` after
`cargo build --release`):

| binary         | what it is                                                        |
|----------------|-------------------------------------------------------------------|
| `chorus`       | the game REPL — play any script file                               |
| `reach`        | static reachability analysis over a script                         |
| `lint-scripts` | the script linter                                                  |
| `reach-fuzz`   | internal test tool: differentially fuzzes `reach` against the engine |
| `reach-scale`  | internal test tool: runs the census at Cultist-Simulator scale     |

Play the bundled showcase, a murder mystery on a night train:

```sh
./target/debug/chorus scripts/the-nightjar-flyer.txt
```

A real session (piped input is echoed as `» line`; in a terminal you get a
`» ` prompt instead):

```
── Murder on the Nightjar Flyer ──
§ unmask Lord Vane's killer — gather proofs aboard and put them to the inquest before Vienna, where the Flyer arrives at six
you are MARCH · at GUARD VAN · day 0, 21:00
here: THE GUARD · THE INQUEST
try: read the manifest · talk to the guard · explore dining car
verbs: read · talk · explore · use on · travel to · wait · study · work · mansus · locate at · dream · acquire
type `help` for how to play
» read the manifest
MARCH read THE MANIFEST.

« 0:126 @ MARCH READ THE_MANIFEST »
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

What just happened, line by line:

- The banner shows the title, the script's `goal:` line, where and when you
  stand, who is `here`, and a **try row** of actions you can type verbatim.
- `read the manifest` did not finish. It **opened a station** — a processing
  window that runs 21:00–22:00. The canonical sentence
  `« 0:126 @ MARCH READ THE_MANIFEST »` is the record of the attempt.
- `wait` advanced time; the window completed, its narration printed, and the
  world moved (the 22:00 timetable trigger — the body being found — fired in
  the same pass).

The meta words are typed commands: `look`, `cards`,
`inspect <card>`, `hold <card>` (bind a card into an open held-capable
station — §6.7), `story` (the prose so far), `journal` (the canonical
sentences), `save <file>` (the log as JSONL), `undo`, `wait`, `help`,
`quit`.

Now run the two analyzers over the same script:

```sh
./target/debug/reach scripts/the-nightjar-flyer.txt world
./target/debug/lint-scripts scripts/the-nightjar-flyer.txt
```

Real output (the world dashboard, trimmed):

```
scripts/the-nightjar-flyer.txt · world
  overview: 31 live / 0 refused site(s), 12 verb(s), 28 prototype(s), 2 deck(s), triggers 8 slot / 1 expire / 0 complete
  freeze: not frozen
  collision names: GUARD_VAN, LUGGAGE_VAN, DINING_CAR, …
  destroyed-by:
  · THE_GUARD ← recipe talk.meet_guard of TALK
  · THE_PATENTEE ← recipe talk.meet_lord of TALK, trigger body_found
  …
```

and the lint:

```
scripts/the-nightjar-flyer.txt: clean

1 file(s), 0 finding(s)
```

`clean` is what you want. The nightjar is the reference for "everything the
script format can do" — when you wonder whether something is possible,
search that file first. Now write your own.

---

## 2. Write your first script

A game is one text file: `#` starts a comment, blank lines are ignored, and
the file is a list of **sections** (`script:`, `entities:`, `verbs:`, …).
We will build a small game — *The Keeper's First Light* — seven times,
adding one idea at a time, and meet an eighth file (`08-smells.txt`) that
deliberately does everything wrong. Every stage is a complete file in
`docs/examples/`, and every one loads and plays (01–07 lint clean;
08 exists to be diagnosed — see §4).

The grammar you will meet, in one paragraph: cards are **entities** declared
with a **kind** and **aspects** (`THE_LOGBOOK is text with tome:1, cipher:2
at LAMP_ROOM`); things to do are **verbs** with **slots** (how many cards of
what kind each role takes) and **recipes** (which outcomes fire when, chosen
by `when` conditions, first match wins); **triggers** fire on the clock, on
card expiry, or on completions; **decks** deal fixed permutations of cards;
**endings** stop the story. The clock is `day:tick` — `0:126` is day 0,
tick 126 = 21:00 (144 ticks of 10 minutes per day).

### 2.1 The minimal game — one room, one verb, one ending

`docs/examples/01-minimal.txt`:

```
script: The Keeper's First Light
player: WICK
start: 0:24

entities:
  LAMP_ROOM is place
  WICK is character at LAMP_ROOM
  THE_LOGBOOK is text with tome:1 at LAMP_ROOM

endings:
  FIRST_LIGHT: "The lamp burns" (grand)

verbs:
  READ:
    actors: 1 kind:character
    object: 1 kind:text
    duration: 30m
    recipes:
      default -> NARRATE "{actors} read {object} cover to cover.", ENDING FIRST_LIGHT
```

Notes:

- The four meta lines: `script:` is the title, `player:` names the default
  actor (must be a declared `character`), `start:` sets the clock, and
  `seed:` (not used here) fixes the dice. Add `goal: <text>` to put a
  one-line goal under the title.
- **Eight built-in kinds** exist: `character place item text idea state rite
  resource`. (You can declare more — §2.3.)
- Every entity line follows one word order — memorize it:
  `NAME is KIND [stackable] [with aspect:level, …] [at PLACE]`
  (or `[in PLACE]` for containment); `stackable` comes *before* the
  `with`/`at`/`in` clauses, which may swap with each other. A second line
  shape links two already-declared cards:
  `A is RELTYPE of B` — e.g. `FIREBOX is south of GUARD_VAN` (§2.7).
  Another order is a load error naming the line:
  `Error: line 5: bad aspect spec 'shiny:1 stackable' (expected aspect:level)`.
- A verb block declares **slots**: `role: count [flag]… [query]`. The six
  clause roles are `actors object with at about to`; the seventh, `held`,
  is bound mid-window (§6.1, §6.7). Counts: `1`, `2`, `1..2`, `2..`, `0..1`.
  A query filters each member — `kind:text`, `aspect:tome>=1`, `state:sealed`,
  `name:X`, `-aspect:winter` (negation). Two held-only flags sit between the
  count and the query: `greedy` (the station pulls fitting cards at begin)
  and `consuming` (the held cards burn at completion).
- `duration:` is how long the station processes before outcomes apply.
- The recipe is the whole logic. Here there is only a `default` — it always
  fires. `NARRATE` writes a journal line; `{actors}`/`{object}` are filled in
  from the slot bindings. `ENDING FIRST_LIGHT` stops the story.

Play it:

```
$ ./target/debug/chorus docs/examples/01-minimal.txt
── The Keeper's First Light ──
you are WICK · at LAMP ROOM · day 0, 04:00
here: THE LOGBOOK
try: read the logbook
verbs: read
type `help` for how to play
» read the logbook

« 0:24 @ WICK READ THE_LOGBOOK »
now: day 0, 04:00 · processing: read until 04:30
try: wait
» wait
WICK read THE LOGBOOK cover to cover.
— The lamp burns —
  (+30m) time passes.
the story has ended — The lamp burns (grand). `undo` takes back the last turn; `quit` leaves.
```

Three things to notice:

- Names are forgiving at the prompt: `read logbook`, `read the logbook` and
  `READ THE_LOGBOOK` all work (case, underscores and articles are dropped).
- The ending applied when the **window completed** — after the `wait` — and
  the pass closed there.
- Time only moves when you advance it. `wait` jumps to the next due
  completion (here 04:30); with nothing cooking it advances one hour.

Common errors at this stage:

- `Error: bad kind term 'kind:txet' (unknown kind 'txet' — expected one of character, place, item, text, idea, state, rite, resource)` — a typo in a slot query. Kinds are checked at load.
- Forgetting the verb block entirely (`READ:` with nothing under it) or
  declaring slots but no recipes: the game loads, but every action rejects —
  `lint-scripts` tells you (`verb READ: declares slots but no recipes — every
  action rejects at selection (§6.6)`).

### 2.2 Recipes — first match wins, hidden gates, REFUSE

A verb's recipes are tried **top to bottom**; the first whose `when`
condition holds fires. That makes ordering load-bearing: put gates above
their fall-throughs, and `default` last. `docs/examples/02-recipes.txt` adds
a locked book (the full file):

```
script: The Keeper's First Light
player: WICK
goal: light the lamp — the keeper's logbook says how
start: 0:24

entities:
  LAMP_ROOM is place
  WICK is character at LAMP_ROOM
  THE_LOGBOOK is text with tome:1, cipher:2 at LAMP_ROOM
  THE_CODEBOOK is text with primer:1 at LAMP_ROOM

endings:
  FIRST_LIGHT: "The lamp burns" (grand)

verbs:
  READ:
    actors: 1 kind:character
    object: 1 kind:text
    duration: 30m
    recipes:
      read.cipher_gate hidden when object.aspect.cipher >= 1 AND exists(THE_CODEBOOK) -> REFUSE("The logbook is written in the keeper's cipher. The worn primer at your belt holds the key.")
      read.primer when object.aspect.primer >= 1 -> DESTROY OBJECT, NARRATE "You work through the primer until the cipher's shapes stop swimming."
      read.deciphered when object.aspect.cipher >= 1 -> SPAWN THE_KEEPERS_NOTE, NARRATE "The logbook opens. 'Trim the wick. Feed the lamp the good oil. Speak the old word at dusk.' You set the page down, lighter by one secret.", ENDING FIRST_LIGHT
      default -> NARRATE "{actors} read {object} and find nothing of use."

prototypes:
  THE_KEEPERS_NOTE is idea with secret:1
```

New ideas:

- Recipe ids (`read.cipher_gate`) name the recipe in logs and diagnostics;
  a leading token that is not `when`/`hidden`/`shown`/`max`/`aspects`/`hint`
  is taken as the id. Omit it and the compiler mints one.
- `hidden` gates speak only when you try (the player "discovers by doing");
  `shown hint "…"` posts a visible warning instead. Visibility is
  display-only — it never changes matching.
- Conditions read the **slots bound this step** (`object.aspect.cipher >= 1`)
  or the **whole world** (`exists(THE_CODEBOOK)` — true iff any living card
  bears that name). Other world forms: `exists(aspect:hunter>=1)`,
  `count(name:SUSPICION) >= 3`. Logic: `AND OR NOT`, comparisons
  `>= <= > < = !=`, quantifiers `every(actors, aspect:occult >= 2)` /
  `any(...)`, relations `rel(parent_child, actors)`.
- `SPAWN THE_KEEPERS_NOTE` mints a card (from the **prototype** in the
  `prototypes:` section — a template for cards that begin the game not yet
  alive). `DESTROY OBJECT` removes a bound card.
- `REFUSE("…")` aborts the step: nothing changes, no time passes — but the
  attempt is still logged.

Play (the win is: read the primer, then the logbook — the deciphered read ends the story):

```
» read the logbook
The logbook is written in the keeper's cipher. The worn primer at your belt holds the key.

« 0:24 @ WICK READ THE_LOGBOOK -> REFUSE("The logbook is written in the keeper's cipher. The worn primer at your belt holds the key.") »
now: day 0, 04:00
try: read the codebook
» wait
  (+1h) time passes.
now: day 0, 05:00
try: read the codebook
» read the codebook

« 0:30 @ WICK READ THE_CODEBOOK »
now: day 0, 05:00 · processing: read until 05:30
try: wait
» wait
You work through the primer until the cipher's shapes stop swimming.
  (+30m) time passes.
now: day 0, 05:30
try: read the logbook
» read the logbook

« 0:33 @ WICK READ THE_LOGBOOK »
now: day 0, 05:30 · processing: read until 06:00
try: wait
» wait
The logbook opens. 'Trim the wick. Feed the lamp the good oil. Speak the old word at dusk.' You set the page down, lighter by one secret.
— The lamp burns —
  (+30m) time passes.
the story has ended — The lamp burns (grand). `undo` takes back the last turn; `quit` leaves.
```

Common errors:

- Putting `default` first makes every recipe after it unreachable — the lint
  says so by name:

  ```
  bad2.txt:
    - recipe read.late (READ): unreachable — read.default already matches everything (first match wins, §6.4)
    - aspect 'secret' is queried but never declared on any entity or prototype — check the spelling

  1 file(s), 2 finding(s)
  ```

  (Findings do **not** fail the lint run — see §4 for the exit-code rules.)
- A typo in a recipe keyword is caught at load, with the positional grammar
  spelled out: `Error: line 11: unexpected 'whnce' before '->' — positionals are [id] [hidden|shown] [hint "..."] [max N] [aspects NAME, ...], then [when COND]`.
- `count(DREAD) >= 3` — a NAME where a role or query belongs — is a load
  error: `Error: recipe mope.deep: count() argument 'DREAD' is neither a role (actors, object, with, at, about, to) nor a query (name:x, kind:k, aspect:a>=1, state:s, -term) — did you mean count(name:DREAD)?`

### 2.3 A bigger world — new kinds, new verbs, aspect arithmetic

`docs/examples/03-world.txt` grows the game to a real win chain: read the
primer, read the logbook, fill the lamp with the oil jug, light it. The new
sections and verbs (full file in `docs/examples/`; here are the additions
over 02):

```
kinds:
  fuel

entities:
  …
  THE_LAMP is item with lamp:1, fueled:0 at LAMP_ROOM
  OIL_JUG is fuel with jug:1 at LAMP_ROOM

prototypes:
  THE_KEEPERS_NOTE is idea with secret:1
  HUNCH is idea deriving name from object, hunch from object.secret-1

  FILL:
    actors: 1 kind:character
    object: 1 kind:item aspect:lamp>=1
    with: 0..1 kind:fuel aspect:jug>=1
    duration: 20m
    recipes:
      fill.dry hidden when count(with) < 1 -> REFUSE("The lamp drinks oil before it will burn. Find fuel.")
      default -> SET(OBJECT, aspect:fueled, 1), DESTROY WITH, NARRATE "{actors} pour {with} into {object}, and the lamp drinks."

  LIGHT:
    actors: 1 kind:character
    object: 1 kind:item aspect:lamp>=1
    duration: 10m
    recipes:
      light.dark hidden when object.aspect.fueled < 1 -> REFUSE("The wick is dry. Fill the lamp first.")
      default -> SET(OBJECT, aspect:lit, 1), NARRATE "The wick takes the flame, and the room goes gold.", ENDING FIRST_LIGHT
```

New ideas:

- `kinds:` declares a new noun class (`fuel`) — one lower_snake name per
  line. **Declare before use**: the section must precede the entity lines
  that use it. Kinds are checked everywhere a query parses — slots,
  `exists()`, `count()`, and `reach … demand`.
- `FILL` and `LIGHT` are **brand-new verbs** — any `UPPER_SNAKE` name is
  legal as a `verbs:` header, and the block under it (slots, duration,
  recipes) is what makes it playable. The try row and `help` pick them up
  automatically.
- `SET(OBJECT, aspect:fueled, 1)` writes an **aspect** onto a bound card.
  Signed values are deltas: `SET(ACTORS, aspect:nerve, +1)` adds. Aspects
  are the matching currency — queries (`aspect:lamp>=1`), conditions
  (`object.aspect.fueled < 1`) and the tools all read them.
- **The missing-key rule — read this twice.** A numeric comparison over an
  aspect the card does not carry is false **in both directions**: on a lamp
  without `fueled`, `object.aspect.fueled < 1` is false *and*
  `object.aspect.fueled >= 1` is false. So the `light.dark` gate above would
  be dead code against a lamp that never declares the key — and lighting the
  dry lamp would simply win. That is why `THE_LAMP` declares `fueled:0`
  up front: **declare an initial value (`with fueled:0`) for any aspect a
  `<`/`>=` comparison reads.** (Attrs are the friendly exception: a missing
  *numeric attr* on a bound card compares as 0 — `actors.attr.nerve < 1`
  holds from missing, the SET-delta rule — but writing the start value is
  still the honest style.) Probed so you do not have to:

  ```
  lamp without fueled:  fueled >= 1 fires nothing · fueled < 1 fires nothing
  orb with glow:3:      glow >= 1 fires       · glow < 1 fires nothing
  lamp without mystery: attr.mystery < 1 fires · attr.mystery >= 1 fires nothing
  ```

- Slot queries compose: `with: 0..1 kind:fuel aspect:jug>=1` takes at most
  one fuel card that also carries `jug`.
- `HUNCH` is a **derive prototype**: `deriving name from object, hunch from
  object.secret-1` — spawning it in a recipe with an `object` bound mints
  `HUNCH_THE_LOGBOOK` with `hunch = 2 − 1 = 1` (you can watch it in `cards`).
- The lenient front door understands role words: `fill the lamp with the oil
  jug` binds `with`. A card that does not fit the slot is refused with the
  verb's usage line:

  ```
  » fill the lamp with the logbook
  THE_LOGBOOK does not fit slot 'with' of FILL
  usage: fill <item> [with <fuel>]
  ```

A trap worth meeting on purpose: re-running a recipe that mints a
non-stackable card while one is alive — reading the logbook twice — makes
the completion fail loudly:

```
SPAWN: name 'THE_KEEPERS_NOTE' already in use
```

nothing changes, the window closes empty. The fix is a guard on the recipe
(`AND NOT exists(name:THE_KEEPERS_NOTE)`) — or `max 1`, introduced next.

With `fueled:0` declared, the guard does its job — lighting the dry lamp is
refused at the table, exactly as authored:

```
» light the lamp

« 0:24 @ WICK LIGHT THE_LAMP -> REFUSE("The wick is dry. Fill the lamp first.") »
now: day 0, 04:00
try: read the codebook · fill the lamp · light the lamp
```

Full win path to try:

```
read the codebook · wait · read the logbook · wait ·
fill the lamp with the oil jug · wait · light the lamp · wait
```

### 2.4 The timetable — triggers, expiry, deadlines

`docs/examples/04-triggers.txt` adds a `triggers:` section and a deadline
(full file in `docs/examples/`; additions below).
The night now fights back: the note fades, dawn ends the game at 07:00.

```
prototypes:
  …
  THE_FADED_NOTE is idea with secret:1, faded:1
  OIL is fuel stackable

triggers:
  oil_delivery at hour 6 -> SPAWN OIL INTO LAMP_ROOM, NARRATE "The chandler leaves a can of lamp oil by the door."
  dawn_comes once at hour 7 -> NARRATE "Dawn grays the window. The lamp was never lit.", ENDING DAWN_UNLIT
  note_fades on expire of THE_KEEPERS_NOTE -> REPLACE THE_KEEPERS_NOTE WITH THE_FADED_NOTE, NARRATE "The note yellows to a faded one; the words go faint. The shape of the secret stays with you."
```

and in the verb blocks:

```
      read.deciphered max 1 when object.aspect.cipher >= 1 -> PROGRESS(15m, "The cipher gives way a line at a time."), SPAWN THE_KEEPERS_NOTE, SET(THE_KEEPERS_NOTE, expires, 1h), SPAWN HUNCH, NARRATE "The logbook opens: …"

  WAIT:
    actors: 1 kind:character
    duration: 30m
    recipes:
      wait.musing when exists(name:THE_KEEPERS_NOTE) -> ADVANCE(1h), NARRATE "{actors} sleep sitting up, the note on their chest, and the hour slides by."
      default -> NARRATE "{actors} watch the dark window."
```

New ideas:

- **Three trigger kinds:**
  - *Slot trigger* — `ID [once] at (hour|tick) N -> OUTCOMES`. `hour 7`
    means 07:00 (`tick 7` would be 01:10). It fires on **every** crossing
    of that time of day — `oil_delivery` happens daily — and on the first
    crossing only with `once` (`dawn_comes`).
  - *Expiry trigger* — `ID on expire of NAME -> OUTCOMES`. When a card whose
    `expires` was set crosses its deadline, this replaces the built-in
    destroy. Here the fading note is `REPLACE`d by a successor (minted into
    the dying card's place). With no authored trigger, the card is just
    destroyed.
  - *Completion trigger* — `on complete …` written on an entity line; see
    §2.6.
- `REFUSE`, `RUN` and `ADVANCE` in the two event kinds are **load errors** —
  an event has no step to refuse and no pass to stretch:
  `Error: trigger dusk_falls: REFUSE belongs to verb recipes — events have no step to refuse`.
- `PROGRESS(15m, "…")` is a read-bar tick: it prints mid-window and lands in
  the journal, but changes nothing:

  ```
  » read the logbook
    (+15m) The cipher gives way a line at a time.

  « 0:27 @ WICK READ THE_LOGBOOK -> PROGRESS(15m, "The cipher gives way a line at a time.") »
  ```

- `max 1` caps a recipe's executions — the recipe-analog of a `once`
  trigger, and the clean fix for §2.3's mint-collision trap.
- `ADVANCE(1h)` in a **verb recipe** stretches the advance pass already in
  flight. Try `read the codebook · wait · read the logbook · wait ·
  wick wait · wait`: the WAIT station (30m) completes, its ADVANCE pushes the
  pass to 06:30, and everything due on the way fires in order:

  ```
  » wick wait

  « 0:30 @ WICK WAIT »
  now: day 0, 05:00 · processing: wait until 05:30
  try: read the logbook · fill the lamp · light the lamp · wait
  » wait
  WICK sleep sitting up, the note on their chest, and the hour slides by.
  The note yellows to a faded one; the words go faint. The shape of the secret stays with you.
  The chandler leaves a can of lamp oil by the door.
    (+1h30m) time passes.
  now: day 0, 06:30
  ```

  Note `wick wait` — the **WAIT verb** (`march wait` in the nightjar) is
  typed with a person before it, because bare `wait` is the meta word. That
  is why the try row sometimes shows `wait` twice: one is the verb, one the
  meta word.

Two clock subtleties that surprise everyone:

- `SET(x, expires, …)` applied at a **completion** is anchored to the
  window's *start* clock, not its end.
- Typing a future clock (`0:48 @ WICK READ THE_LOGBOOK`) advances the world
  to 08:00 in the same atomic pass that commits the action — "+4h time
  passes". If the action is then refused at the table, the whole pass
  (advance included) is discarded.

Dawdle and the deadline gets you:

```
» wait
Dawn grays the window. The lamp was never lit.
— Dawn on a dark lamp —
  (+30m) time passes.
the story has ended — Dawn on a dark lamp (melancholy). …
```

After an ending, input returns the ending notice; `undo` reopens the story,
`quit` leaves.

### 2.5 Chance and repetition — IF/ELSE, dice, decks, functions

`docs/examples/05-decks-dice.txt` adds a deck of omens, a `DREAM` verb that
draws from it, a fumbled-match branch on `LIGHT`, and a function. The new
sections:

```
prototypes:
  …
  A_GUST is idea stackable
  A_CALM is idea stackable
  A_SILENCE is idea stackable

functions:
  delivery(n) -> SPAWN OIL INTO LAMP_ROOM, NARRATE "The chandler leaves {n} can of lamp oil by the door — the same word for one or many, out here."

decks:
  OMENS spec A_GUST, A_GUST, A_CALM, A_SILENCE defaultcard A_GUST reset

  LIGHT:
      light.matchbox when actors.attr.nerve < 1 -> ROLL(1, 6), IF roll >= 3 -> (SET(OBJECT, aspect:lit, 1), NARRATE "The match catches on the second strike ({roll}). The wick takes the flame, and the room goes gold.", ENDING FIRST_LIGHT) ELSE -> (NARRATE "The match snaps ({roll}). Your hands are too cold for this — sleep, and try steadier.")
      default -> SET(OBJECT, aspect:lit, 1), NARRATE "The wick takes the flame, and the room goes gold.", ENDING FIRST_LIGHT

  DREAM:
    actors: 1 kind:character
    duration: 1h
    recipes:
      dream.on_note when exists(name:THE_KEEPERS_NOTE) -> DESTROY THE_KEEPERS_NOTE, DRAW OMENS, ROLL(1, 6), IF roll >= 4 -> (SET(ACTORS, aspect:nerve, +1), NARRATE "{actors} sleep on the note and wake steadier — the dream went easy ({roll}).") ELSE -> (NARRATE "{actors} sleep on the note and wake tired ({roll}).")
      default -> DRAW OMENS, NARRATE "{actors} sleep, and the night sends an omen."

templates:
  DREAM says "{actors} dream."
  LIGHT says "{actors} strike a light."
```

New ideas:

- **IF / ELSE IF / ELSE** inside an outcome list; branch bodies *require
  parens*. Conditions evaluate in order against the **draft** — earlier
  outcomes in the same list are visible to later IFs. The log records only
  the taken branch.
- **`ROLL(count, sides[, bonus])`** is the engine's only randomness, fully
  determined by `seed:`. It binds `roll` for the rest of the list;
  `{roll}` in a NARRATE prints the number. Re-rolls after a refusal throw
  the same dice — deterministic by design.
- Why `nerve:0` on WICK (`docs/examples/04-triggers.txt` declares
  `WICK is character with keeper:1, nerve:0 at LAMP_ROOM`)?
  `light.matchbox` compares
  `actors.attr.nerve < 1`. Attrs are the friendly case: a **missing numeric
  attr** on a bound card compares as 0, so the recipe would hold even if
  WICK declared nothing — writing `nerve:0` documents the start and makes
  the value show in `inspect`. Aspects have no such default (§2.3's
  missing-key rule): pick `attr` when zero-from-missing is what you mean,
  `aspect` when presence itself is the point.
- **Decks** — `ID spec NAME[, NAME…] [defaultcard NAME] [reset]`. The draw
  order is a fixed permutation of the spec entries (repeats weight the
  draw), a function of the seed. `DRAW OMENS` deals the next card; `INTO
  <target>` places it. `defaultcard` serves forever once the period is
  exhausted; `reset` wraps and deals the same sequence again; **neither**
  means the deck empties and further draws reject the step:

  ```
  deck 'DREAMS' is exhausted — no 'reset' and no 'defaultcard' (§4.4)
  ```

  (The nightjar's `DREAMS spec PORTENT, PORTENT, VOID` is exactly that
  shape — a door that empties — on purpose. Mark deck entries `stackable`
  while you are at it: the lint flags a non-stackable entry unless it can
  physically be dealt only once — one serving position across every deck,
  no `reset`; §4 has the details.)
- **Functions** are parametric outcome macros; the call `delivery(1)` expands
  at load time — the log never shows the call. `{n}` interpolates the
  argument. Write `{who}`-style tokens only for declared parameters; an
  unknown `{token}` is a lint error
  (`unknown template token {obect} — roles are {actors} {object} {with} {at} {about} {to}, plus {roll}`).
- **`templates:`** give each verb its prose line (`WICK dream.`); role
  tokens (`{to}`, `{with}`, `{about}`) render wherever that role is actually
  bound. A verb with no template still speaks through its NARRATEs.

The deck-drawn cards are real cards — `dream` then `wait`, then `cards`:

```
» dream
WICK dream.

« 0:24 @ WICK DREAM »
now: day 0, 04:00 · processing: dream until 05:00
try: read the codebook · fill the lamp · light the lamp · wait
» wait
WICK sleep, and the night sends an omen.
  (+1h) time passes.
now: day 0, 05:00
try: read the codebook · fill the lamp · light the lamp
» cards
LAMP ROOM  [place]
WICK  [character]
THE LAMP  [item]
OIL JUG  [fuel]
THE LOGBOOK  [text]
THE CODEBOOK  [text]
A CALM  [idea]
```

(What `cards` shows is a projection, not the whole world — the visibility
rules are probed and written down in §2.7.)

### 2.6 Card-borne machines — `on complete`, RUN, ops, relations

`docs/examples/06-complete-run-ops.txt` is the payoff: a cat, a lamp that
reacts to being filled, and an automatic follow-up station. The changed
lines:

```
entities:
  …
  THE_CAT is character with naps:1 at LAMP_ROOM
  THE_LAMP is item with lamp:1, fueled:0 at LAMP_ROOM on complete fueled -> (NARRATE "The lamp settles, oil moving in the well glass.") on complete job -> (RUN "tend.sit")

ops:
  SPILL(who) -> SET(who, aspect:oil_stained, 1)

  FILL:
    …
    recipes:
      fill.dry hidden when count(with) < 1 -> REFUSE("The lamp drinks oil before it will burn. Find fuel.")
      fill.pour aspects fueled, job when count(with) >= 1 -> SET(OBJECT, aspect:fueled, 1), DESTROY WITH, SPILL(ACTORS), NARRATE "{actors} wipe the slick off on the apron, pour {with} into {object}, and the lamp drinks."

  TEND:
    actors: 1 kind:character
    object: 0..1 kind:item aspect:lamp>=1
    duration: 30m
    recipes:
      tend.sit when count(object) < 1 OR object.aspect.fueled >= 1 -> NARRATE "{actors} sits with the lamp a while, watching the oil move."

  TALK:
    actors: 1 kind:character
    to: 0..1 kind:character
    about: 0..1 kind:idea
    duration: 30m
    recipes:
      talk.befriend when to.aspect.naps >= 1 AND NOT rel(neighbor, actors, to) -> RELATE(WICK, THE_CAT, neighbor), NARRATE "{actors} scratch {to} behind the ear. A small, formal friendship."
      default -> NARRATE "{actors} mentions {about} to {to}. The cat blinks. That is the whole conversation."
```

New ideas:

- **Completion triggers** ride an entity (or prototype) line. The **aspect
  form** — `on complete fueled` — fires when a processing window completes
  binding that card **and** the completed recipe declares that aspect in its
  `aspects` list (`fill.pour aspects fueled, job`). The bare form (no
  aspect) fires on every completion binding the card. They produce no EVENT
  row; their outcomes append to the completion.
- **RUN chains.** `RUN "tend.sit"` (legal inside `on complete` bodies) opens
  a follow-up window on that recipe's verb with the parent's bindings
  projected onto it. Fill the lamp and a TEND station opens by itself:

  ```
  » fill the lamp with the oil jug
  WICK fill the lamp.

  « 0:30 @ WICK FILL THE_LAMP WITH OIL_JUG »
  now: day 0, 05:00 · processing: fill until 05:20
  try: tend · talk to the cat · dream · wait
  » wait
  WICK wipe the slick off on the apron, pour OIL JUG into THE LAMP, and the lamp drinks.
  The lamp settles, oil moving in the well glass.
  The note yellows; the words go faint. The shape of the secret stays with you.
    (+30m) time passes.

  « 0:30 @ WICK TEND THE_LAMP »
  now: day 0, 05:30 · processing: tend until 05:50
  ```

  That is the Cultist-Simulator "job induces needs" shape. Rules to know:
  a recipe body may RUN only the same verb's earlier recipes; an `on
  complete` body may RUN across verbs (ids are then global — keep them
  unique); a second RUN reached in one completion rejects the window; and
  cycles of zero-duration chains are guarded loud at 1024 windows per pass.
  Mind the price: the follow-up window **locks its projected cards**, so
  after filling the lamp the lamp is busy until the tend station frees —
  one more `wait` before `light the lamp`. The deterministic win path is
  `read the codebook · wait · read the logbook · wait · fill the lamp with
  the oil jug · wait · wait · light the lamp · wait` (lighting rolls a die
  while your nerve is 0 — the `light.matchbox` branch — and this fixed
  sequence rolls a 6; a snapped match just tells you to try again, and
  dreaming on the note raises nerve so the plain `default` fires instead).
- **Ops** are your own outcome operators — `SPILL(ACTORS)` in `fill.pour`
  expands to the op body at load. Unlike functions they are arity-checked
  at every call site and callable from triggers. One wart to avoid: a role
  name interpolates into the op's *strings* as the raw word — writing
  `NARRATE "{who} wipes…"` in the op prints `ACTORS wipes…`. Keep prose at
  the call site (as `06` does) and let ops do state.
- **Relations** are typed edges between cards: `RELATE(WICK, THE_CAT,
  neighbor)` creates one, `rel(neighbor, actors, to)` reads one, `UNRELATE`
  removes. The relation type must be one of the **seven registered** ones —
  `parent_child parent_of neighbor north south east west` — and there is
  currently no script syntax to register more, so a made-up type refuses at
  runtime. Two gotchas we hit writing this tutorial: `RELATE` needs literal
  names (role refs like `ACTORS, TO` are not concretized — the completion
  fails with `RELATE needs (ref, ref, type[, value])`), and the four
  directions are what `look`'s exits and travel gates read.
- `journal` shows the machinery — expansions done, raw templates stored:

  ```
  0:30 @ COMPLETE FILL -> SET(#s_the_lamp, aspect:fueled, 1), DESTROY(#s_oil_jug), SET(#s_wick, aspect:oil_stained, 1), NARRATE "{actors} wipe the slick off on the apron, pour {with} into {object}, and the lamp drinks.", NARRATE "The lamp settles, oil moving in the well glass."
  0:30 @ WICK TEND THE_LAMP
  ```

You now have every section of the format in your head:
`script/kinds/entities/prototypes/triggers/functions/ops/decks/endings/verbs/templates`.
One geography is left — a second room and the corridor between — and then
beyond it all: multi-actor steps (`march brack work`), spawn placement
(`expires`, `AS`), `state.*` flags, `visible()`-style fog of war — read
`scripts/the-nightjar-flyer.txt`, which uses all of it with comments, and
the spec's §4–§11 when you need exact rules.

### 2.7 A second place — exits, travel, and what `cards` shows

`docs/examples/07-second-place.txt` adds a stair below the lamp room and
the cards to get there, get back, and make the trip worth taking. (It also
moves the deadline to 08:00 — the stair costs time.) The new lines:

```
entities:
  …
  THE_STAIR is place with site.stair:1
  THE_MATCHBOX is item with matchbox:1 at THE_STAIR

  # the corridor: a directed layout link — `A is south of B` puts A south of B
  THE_STAIR is south of LAMP_ROOM

  LIGHT:
    actors: 1 kind:character
    object: 1 kind:item aspect:lamp>=1
    with: 0..1 kind:item aspect:matchbox>=1
    duration: 10m
    recipes:
      light.dark hidden when object.aspect.fueled < 1 -> REFUSE("The wick is dry. Fill the lamp first.")
      light.box when actors.attr.nerve < 1 AND total(with, aspect:matchbox) >= 1 -> SET(OBJECT, aspect:lit, 1), DESTROY WITH, NARRATE "One of the box's dry matches catches on the first strike. The room goes gold.", ENDING FIRST_LIGHT
      …

  TRAVEL_TO:
    actors: 1 kind:character
    object: 1 kind:place
    duration: 30m
    recipes:
      travel.no_link hidden when NOT (rel(north, actors.container, object) OR rel(south, actors.container, object) OR rel(east, actors.container, object) OR rel(west, actors.container, object)) -> REFUSE("The keeper's tower is one straight stair — that is not next door.")
      default -> MOVE ACTORS INTO OBJECT, NARRATE "{actors} take the stair to {object}."

  TAKE:
    actors: 1 kind:character
    at: 0..1 kind:place
    object: 1 kind:item
    duration: 10m
    recipes:
      take.matchbox when object.aspect.matchbox >= 1 AND at.aspect.site.stair >= 1 -> MOVE OBJECT INTO ACTORS, NARRATE "{actors} pocket {object}."
      take.here hidden when NOT object.aspect.matchbox >= 1 -> REFUSE("That is not yours to take — or not here to take.")
      take.where hidden when count(at) < 1 -> REFUSE("Take it where it lies — name the place with `at`.")
      default -> REFUSE("You cannot take {object} from here.")
```

(Yes, `take.here` is written `NOT object.aspect.matchbox >= 1`, not
`< 1` — §2.3's missing-key rule again: `< 1` would be false on every card
that never declares `matchbox`, dead as a doornail. Probed:
`take the lamp` — an item with no matchbox aspect — refuses through
`take.here`, and `take the matchbox` without a place refuses through
`take.where`: "Take it where it lies — name the place with `at`".)

New ideas:

- **Exits are ordinary entity rows** using the four registered direction
  types `north south east west`: `THE_STAIR is south of LAMP_ROOM` puts the
  stair south of the room (both endpoints must already be declared). The
  REPL reads the same edges back — `look` grows an `exits:` line:

  ```
  » look
  You are at LAMP ROOM — day 0, 04:00.
    THE CAT  [character]
    THE LAMP  [item]
    OIL JUG  [fuel]
    THE LOGBOOK  [text]
    THE CODEBOOK  [text]
  exits: south — THE STAIR
  ```

- **The travel gate is the nightjar's corridor idiom**:
  `rel(north, actors.container, object)` — a `X.container` role path stands
  for *where the cards of role X sit*, so the gate asks "is there a
  direction edge between where I stand and where I want to go?" Only
  adjacent places pass; anything else refuses hidden at commit.
- `MOVE ref, INTO ref` is the general relocation op — here
  `MOVE ACTORS INTO OBJECT` (walk there) and `MOVE OBJECT INTO ACTORS`
  (pocket it). Mind the timing: the move applies at the **window's
  completion**, so `look` during a travel window still shows the old room
  (and a `processing: travel to until 04:30` line appears under it).
- **There is no co-location predicate** — no `same_place(actors, object)`.
  The working idiom is a place-marker aspect
  plus the `at` role: `THE_STAIR` carries `site.stair:1`, TAKE binds an
  optional `at` slot, and the recipe demands
  `at.aspect.site.stair >= 1`. The player types the place with the role
  word, and the canonical sentence comes out pre-verbal:

  ```
  » take the matchbox at the stair

  « 0:27 @ WICK AT THE_STAIR TAKE THE_MATCHBOX »
  ```

- **`cards` (and `look`) show a projection, not the world.** Probed on this
  exact script: at the start, `cards` lists both places, the people, and
  the lamp-room cards — `THE MATCHBOX` is *not* on it. Walk to the stair and
  it appears (cards sharing your place show; the lamp-room cards vanish —
  nobody has touched them). Draw an omen there and it follows you: anything
  a logged sentence has named stays on the table wherever it sits. The
  rules in one breath: *places and the player always; cards sharing your
  place; cards someone carries; contents of any place the log has named;
  anything the log has named. Everything else is out of sight until you go
  and look.* Even naming a place is enough — right after committing the
  travel (before the `wait` that completes it), `cards` already lists `THE
  MATCHBOX`, because the travel row named `THE_STAIR`:

  ```
  » travel to the stair
  …
  » cards                      # still standing in LAMP ROOM
  LAMP ROOM  [place]
  THE STAIR  [place]
  WICK  [character]
  THE CAT  [character]
  THE LAMP  [item]
  OIL JUG  [fuel]
  THE LOGBOOK  [text]
  THE CODEBOOK  [text]
  THE MATCHBOX  [item]
  ```

  ```
  » cards                      # at THE STAIR, after one dream
  LAMP ROOM  [place]
  THE STAIR  [place]
  WICK  [character]
  THE MATCHBOX  [item]
  A CALM  [idea]
  ```

  (`A CALM` was dealt at the stair but stays visible after you leave — the
  dream logged its name. `THE CAT`, untouched a floor below, does not.)

- `light.box` is the payoff: carrying the matchbox (`with: 0..1 kind:item
  aspect:matchbox>=1` + `total(with, aspect:matchbox) >= 1`) lights the lamp
  on the first strike, no die. The deterministic win path:

  ```
  travel to the stair · wait · take the matchbox at the stair · wait ·
  travel to the lamp room · wait · read the codebook · wait ·
  read the logbook · wait · fill the lamp with the oil jug · wait · wait ·
  light the lamp with the matchbox · wait
  ```

  ending, as always:

  ```
  » wait
  One of the box's dry matches catches on the first strike. The room goes gold.
  — The lamp burns —
    (+10m) time passes.
  the story has ended — The lamp burns (grand). `undo` takes back the last turn; `quit` leaves.
  ```

---

## 3. Reading `reach`

`reach` answers, statically and before play: **can the player obtain card X,
and what would it take?** It compiles your script exactly like the game does,
then walks every spawn/replace/draw site (recipes, all three trigger kinds,
decks, prototypes) to see which mints are *provable*. No engine runs; it is
deterministic — the same script always gives the same report.

```sh
./target/debug/reach <script.txt> <COMMAND> [OPTIONS]
```

### 3.1 The six commands

| command          | the question it answers                                                    |
|------------------|-----------------------------------------------------------------------------|
| `card <NAME>`    | How can the player get this one card? Its verdict, proven paths, and which verb slots it can be bound into. |
| `all`            | The same for every name (prototypes ∪ live initial entities ∪ deck order — the `defaultcard` is not walked). |
| `demand "<QUERY>"` | Which cards match a slot query like `"aspect:secret>=2"` — and can they exist? |
| `verb <NAME>`    | Is this verb committable (a station can ever open)? What do its recipes mint? |
| `world`          | The whole-world dashboard: census, freeze state, dead verbs, deck caps, collisions, destroy/set maps. |
| `graph`          | The whole mint-dependency graph as Graphviz DOT.                            |

Real examples on the bundled nightjar:

```
$ ./target/debug/reach scripts/the-nightjar-flyer.txt card THE_WAX_CYLINDER
scripts/the-nightjar-flyer.txt · card THE_WAX_CYLINDER
  verdict: UNPROVEN  (existence: candidate)
  candidate source: recipe explore.firebox of EXPLORE — guarded (this list mints a name that can collide with a living card (duplicate mint or an initial copy) — the refusal would discard the draft, an earlier recipe of the same verb may shadow it (first-match), 1 IF condition(s) on the path, recipe `when` guard)
  bindable via READ.object (query matches)
  bindable via STUDY.object (query matches)
```

```
$ ./target/debug/reach scripts/the-nightjar-flyer.txt demand "aspect:secret>=2"
scripts/the-nightjar-flyer.txt · card matching 'aspect:secret>=2'
  verdict: OBTAINABLE  (existence: proven)
  path 1/1:
    · initial 'THE_SECRETARY' — via initial entity THE_SECRETARY (character)
        · THE_SECRETARY is declared in the initial entities (character, aspects {"secret":2,"unmet":1})
  bindable via READ.actors (query matches)
  …
```

```
$ ./target/debug/reach scripts/the-nightjar-flyer.txt verb WORK
scripts/the-nightjar-flyer.txt · verb WORK
  duration: 120 min
  slots:
  · actors ×2 — kind:character
  · at ×0 — kind:place !aspect:site.first
  committable: yes
  · WORK committable: actors ×2 ← initial entity MARCH (character), initial entity THE_PATENTEE (character)
  recipes:
  · work.firebox (when: ? >= 1) — SPAWN 'FUNDS': unproven — the recipe `when` guard is not a static truth — it reads runtime state or evaluates false
  · work.owed (when: ? != 0) — no live mint site
  · work.default (when: none) — no live mint site
```

On one of our own examples the dashboard is small enough to read whole:

```
$ ./target/debug/reach docs/examples/04-triggers.txt world
docs/examples/04-triggers.txt · world
  overview: 4 live / 0 refused site(s), 4 verb(s), 4 prototype(s), 0 deck(s), triggers 2 slot / 1 expire / 0 complete
  freeze: not frozen
  collision names: LAMP_ROOM, WICK, THE_LAMP, OIL_JUG, THE_LOGBOOK, THE_CODEBOOK, THE_KEEPERS_NOTE, HUNCH, THE_FADED_NOTE
  destroyed-by:
  · THE_KEEPERS_NOTE ← trigger note_fades
  set pumps:
  · expires — recipe read.deciphered of READ
  · aspect:fueled — recipe fill.default of FILL
  · aspect:lit — recipe light.default of LIGHT
```

Reading the world face: *dead verbs* (none here — the smell demo in §4.1
has one), *deck caps*
(how many draws a proven deck can ever serve; `MANY` renders `unbounded`),
*collision names* (every minted name checked for uniqueness), *destroyed-by*
(who kills what), *set pumps* (who writes which key), *refused sites*
(spawn sites killed by an authored REFUSE), and the freeze line (§4.1).

### 3.2 The verdict vocabulary

Two axes, printed as `verdict: X (existence: y)`:

- **verdict** — `OBTAINABLE` (a constructible path proves it), `UNPROVEN`
  (a candidate source exists but some guard defeats static proof),
  `UNOBTAINABLE` (no source can ever apply). With `--usable`, a proven card
  that can additionally be bound into a committable verb slot passes; the
  report head then reads `[usable semantics]`.
- **existence** — `proven` / `candidate` / `none`.

So a card only a winning ending mints is `OBTAINABLE (existence: proven)` —
reachability is not "a good idea"; it is "some legal play makes it exist".

### 3.3 The `guarded (…)` glossary

When a site cannot be proven, the report names its reasons. Translations:

| report says | means |
|---|---|
| `recipe `when` guard` | the recipe's condition reads runtime state the analyzer cannot predict (dice, a count, a world check). Not a bug — just not statically provable. |
| `an earlier recipe of the same verb may shadow it (first-match)` | some earlier, always-true recipe could fire instead, so this line may never be reached. Check your recipe order. |
| `N IF condition(s) on the path` | the mint sits inside an IF; the analyzer cannot prove the branch is taken. |
| `this list mints a name that can collide with a living card (duplicate mint or an initial copy) — the refusal would discard the draft` | the same name could be minted twice (or minted over an initial card); the second mint refuses the step. Guard the recipe or `stackable` the card. |
| `a throwing op in this list may discard the whole draft at completion` | MOVE/RELATE/UNRELATE/REPLACE in the list can hard-fail at completion and void everything else in it. |
| `the verb's 30m window completes past the by 600 horizon` | with `--by <minute>`: the action would finish after the deadline you asked about. |
| `gated on (or filled by) a card the list itself mints` (the `[reach]` lint) | the recipe's own guard needs a card that only this recipe makes — it can never fire. |

### 3.4 The diagnostic options

- **`--why`** — per-site blocker table for the unproven paths:

  ```
  $ ./target/debug/reach scripts/the-nightjar-flyer.txt card THE_WAX_CYLINDER --why
  …
  scripts/the-nightjar-flyer.txt · card THE_WAX_CYLINDER — why
    · SPAWN 'THE_WAX_CYLINDER' — recipe explore.firebox of EXPLORE: this list mints a name that can collide with a living card (duplicate mint or an initial copy) — the refusal would discard the draft
  ```

- **`--usable`** — additionally require bindability into a committable slot:

  ```
  $ ./target/debug/reach docs/examples/04-triggers.txt card OIL --usable
  docs/examples/04-triggers.txt · card OIL  [usable semantics]
    verdict: OBTAINABLE  (existence: proven)
    path 1/1:
      · SPAWN 'OIL' — via trigger oil_delivery
          · trigger oil_delivery fires on slot 12 (every crossing) — the clock is unbounded forward
    bindable via FILL.with (query matches)
  ```

- **`--cost`** — lower-bound price of a proven card: fewest verb
  applications (with the witness chain), fewest clock minutes, how many
  copies can coexist. Appended to `card`/`all`; every report states it is a
  *lower bound*.

  ```
  $ ./target/debug/reach scripts/the-nightjar-flyer.txt all --cost --by 600
  scripts/the-nightjar-flyer.txt · card BRACK  [by minute 600]
    verdict: UNPROVEN  (existence: candidate)
    …
  ```

- **`--by <minute>`** — judge everything against an absolute deadline: slot
  triggers beyond minute M have not fired yet, long windows do not finish in
  time. The head gains `[by minute N]`. Use it to answer "can the player win
  *before the train arrives*".
- **`--from-state <log.jsonl>`** — replay a real save as the starting
  world. The REPL's `save <file>` writes one — play a few turns, `save`,
  `quit`, then point reach at the same script the log came from:

  ```sh
  printf 'look\nread the manifest\nsave /tmp/midgame.jsonl\nquit\n' | ./target/debug/chorus scripts/the-nightjar-flyer.txt
  ./target/debug/reach scripts/the-nightjar-flyer.txt card THE_MANIFEST
  ./target/debug/reach scripts/the-nightjar-flyer.txt card THE_MANIFEST --from-state /tmp/midgame.jsonl
  ```

  Real output (the last call; the log and the script travel as a pair):

  ```
  scripts/the-nightjar-flyer.txt · card THE_MANIFEST
    verdict: OBTAINABLE  (existence: proven)
    path 1/1:
      · initial 'THE_MANIFEST' — via initial entity THE_MANIFEST (text)
          · THE_MANIFEST is declared in the initial entities (text, aspects {"manifest":1})
    bindable via READ.object (query matches)
    bindable via STUDY.object (query matches)
    note: midgame analysis: the initial world is replayed from a log — modeled: the replayed entities and clock (first crossings re-anchor to minute 1260), 0 already-fired once trigger(s) (they never fire again), 0 non-reset deck cursor(s) (draws serve from the cursor forward); read but not modeled: open windows (the lock table), the executed-recipe map, the rng counter
  ```

  The verdict is anchored to the save: the clock rides the log (21:00, so
  crossings re-anchor to minute 1260), and the report carries the
  fidelity note listing what is modeled (entities, clock, spent `once`
  triggers, deck cursors) and what is not (open windows, executed recipes,
  the dice counter). An *ended* log is refused — a finished world has
  nothing left to reach.

### 3.5 Machine faces

- `card/all/demand --json` — the report envelope as JSON (`file`, `demand`,
  `usable`, `report`; `--why --json` adds a `why` array). `world`, `verb`
  and `graph` ignore the flag.
- `graph` — DOT of every proven mint: who spawns what, which slot fillers
  each site needs (edge labels), dashed `destroys` edges. Pipe it through
  Graphviz:

  ```sh
  ./target/debug/reach docs/examples/04-triggers.txt graph | dot -Tsvg > reach.svg
  ```

  Our example renders ten card nodes and two trigger boxes:

  ```
  digraph reach {
    rankdir=LR;
    "OIL" [label="OIL\nfuel", shape=ellipse];
    …
    "trigger oil_delivery" [shape=box];
    "trigger note_fades" -> "THE_KEEPERS_NOTE" [label="destroys", style=dashed];
    "trigger oil_delivery" -> "OIL" [label="SPAWN"];
  }
  ```

Exit codes: success 0; compile or analyze errors 1 (an unknown *verb* name,
an unreadable script — note an unknown *card* name is not an error, it just
reports `UNOBTAINABLE (existence: none)`); argv errors 2. `--color` adds
ANSI highlighting (off by default; JSON and DOT are never colored).

---

## 4. Reading `lint-scripts`

`lint-scripts` compiles each script and reports what a careful reader would
flag. It reads named files, or every `scripts/*.txt` under the current
directory when given none:

```sh
./target/debug/lint-scripts [OPTIONS] [FILES...]
```

Three finding classes:

- plain **error** — broken by construction: a verb with slots but no
  recipes, a recipe after `default` (unreachable), two recipes with the same
  normalized `when` (shadowing), a `count(actors) <= 0` gate the slot
  cardinality can never satisfy, an unknown `{token}` in a template, a deck
  entry that is not stackable, an aspect that is read but never written
  anywhere, an entity name that needs quoting everywhere. One of these has
  a physical out: the **deck-entry rule exempts an entry that can only ever
  be dealt once** — its name is served by exactly one position across every
  deck's `spec`/`defaultcard`, it appears once in its deck's spec, and the
  deck has no `reset` (a spec entry served once can never meet its own
  copy — the deck exhausts first). Everything else is flagged: a name
  served twice, a `defaultcard` entry (it serves forever), a `reset` deck
  (the wrap deals everything again). Real case, from a deck whose one name
  sits in both the spec and the defaultcard:

  ```
  omens-deck.txt:
    - deck OMENS: entry PORTENT is not stackable — served again while a copy lives, the spawn collides (§10 livelock rule)

  1 file(s), 1 finding(s)
  ```

  Mark the entries `stackable` (or give the name a single serving
  position) and the file goes clean. A count-gate ERROR and the
  never-matching WARNING below are one root cause on one recipe — the lint
  reports the ERROR alone.
- **`[warning]`** — a `when` that can *provably* never match, e.g. a recipe
  gated on `NOT count(actors) >= 1` while the verb's actors slot takes
  `1..1` (the slot always binds exactly one card, so the count can never
  drop below one):
  `- [warning] recipe wait.doomed (WAIT): the when can never match — count(actors) < 1 can never hold — actors takes 1..1 here (§6.1) (§6.3)`.
- **`[reach]`** — with `--reach`, the reach analyzer's whole-world design
  smells, addressed by owner (recipe/trigger/verb) rather than line.

Exit-code discipline (memorize this): **findings never change the exit code
— it stays 0 even with errors.** A *hard input problem* exits 1: an
unreadable file, or a script that fails to compile (the lint only speaks
after a successful compile, and the compile error is what you see). Bad
command-line usage exits 2. The lesson: check the output, not the exit code;
wire "zero findings" into CI by grepping, not by `$?`.

### 4.1 The six `[reach]` families

`docs/examples/08-smells.txt` is a deliberately smelly script that trips
every family. Run:

```sh
./target/debug/lint-scripts --reach docs/examples/08-smells.txt
```

Real output (numbering added):

```
docs/examples/08-smells.txt:
  1.  - recipe work.loop (WORK): unreachable — work.moonlight already matches everything (first match wins, §6.4)
  2.  - aspect 'wishlist' is queried but never declared on any entity or prototype — check the spelling
  3.  - [reach] verb READ never commits — its object slot's query (kind:text aspect:wishlist>=1) can never match anything the world can produce (§13.1)
  4.  - [reach] recipe work.loop of WORK: gated on (or filled by) a card the list itself mints (§13.1)
  5.  - [reach] trigger morning_charm_a: its list's non-stackable mints can collide with a living card or another owner's mint at a crossing — the collision refuses the whole draft (§13.1)
  6.  - [reach] trigger morning_charm_b: its list's non-stackable mints can collide with a living card or another owner's mint at a crossing — the collision refuses the whole draft (§13.1)
  7.  - [reach] trigger charm_sweep: its DRAW from deck POUCH freezes the cursor at 1 served position(s) — a non-stackable co-mint in the same list pins the draw (§13.1)
  8.  - [reach] the world is frozen — the first risky crossing lands at minute 240 (trigger morning_charm_a, trigger morning_charm_b): every draft completing there or later refuses (§13.1)
  9.  - [reach] 1 producer site(s) sit behind an authored REFUSE — they can never apply: recipe work.moonlight of WORK (§13.1)

1 file(s), 2 finding(s), 0 warning(s), 7 reach finding(s) — warnings and reach findings advise, the exit code stays 0
```

The families, in plain words, each with what you probably want to do:

1. **Dead verb** (3) — some slot's query can never match any card that can
   ever exist, so the verb can never commit. *Fix:* the query is wrong
   (typo — see finding 2), or nothing in your economy ever produces that
   aspect at that level.
2. **Self-gated list** (4) — the recipe's own guard or fill needs a card
   that only the recipe itself mints; it can never fire. *Fix:* loosen the
   guard (`NOT exists(name:X)` before the first mint, not after), or mint
   the card elsewhere first.
3. **Risky owner** (5, 6) — this recipe/trigger mints a non-stackable card
   in a situation where it can collide with a living copy; the collision
   refuses the whole draft. *Fix:* `stackable` the card, guard with `NOT
   exists(name:…)`, or make the trigger `once`.
4. **Deck-cap cursor freeze** (7) — a repeatable site draws from a deck
   beside a non-stackable co-mint; the collision pins the deck's cursor so
   only the first positions can ever be served. *Fix:* stackable deck
   entries, or consume/rename the co-mint on the same step. (A `once`
   trigger drawing a single card is its design — the lint does not flag it.)
5. **Frozen world** (8) — the risky sites share a crossing minute; from
   there on, every draft completing refuses. Reach reports past the freeze
   boundary are candidate at best. *Fix:* same as risky owner — usually
   make the colliding mints stackable.
6. **REFUSE-doomed sites** (9) — spawn sites listed in the same outcome
   list *after* (or under) an authored `REFUSE` can never apply. *Fix:*
   dead code — delete them, or move them to a recipe that can actually
   fire.

Lines 1–2 are the ordinary lint doing its job underneath: the same file has
a genuinely unreachable recipe and a misspelled aspect.

---

## 5. CLI cheat sheet

### chorus — play

```
usage: chorus <script.txt>
       a line REPL: type what happens, or help/look/cards/story/journal/save/undo/wait/inspect/quit
try 'chorus --help' for how to play
```

One argument: the script file. Exit 0 on quit/EOF, 1 on unreadable or
uncompilable script, 2 on argv errors (no script path, an unknown flag, an
extra positional word).

Typed words: actions in plain English (`read the manifest`, `question the
guard`, `fill the lamp with the oil jug`); people go **before** the verb when
several act (`march brack work`); role words `to with at about` shape the
slots (`use on <thing> with <proof> <proof>`); names forgive case,
underscores, articles and possessives, and one distinctive word is enough
(`read ledger` finds `THE_VANE_LEDGER`). Strict canonical sentences
(`0:126 @ MARCH WAIT`) pass through unchanged. Meta words: `look`, `cards`,
`inspect <card>`, `story`, `journal`, `save <file.jsonl>`, `undo`, `wait`,
`help`, `quit`. Time: an action opens a station; `wait` jumps to the next
due thing (or one hour if nothing is due); a clock like `1:12` jumps there;
`undo` truncates the log to the turn boundary. `save` writes the JSONL log
to a file — hand it to `reach --from-state` (§3.4) to analyze the midgame.

`chorus --help` (verbatim, exit 0; `-h` anywhere in the arguments does the same):

```
chorus — the line REPL: a script file played as a text game

usage: chorus <script.txt>
try 'chorus --help' for how to play

The player types plain English-ish sentences; the banner's `try:` row
lists what the game understands right now. Meta words (typed as-is):

  look                the place, the clock, what is here and carried
  cards               the visible table
  inspect <card>      one card's face; a bare `inspect` prints usage + the table
  story               the prose so far
  journal             the canonical log
  save <file.jsonl>   write the action log — `reach --from-state <file>` replays it
  undo                take back the last turn
  wait                jump to the next due completion (or one hour)
  help                how to play
  quit                leave

Options:
  -h, --help  print help

examples:
  chorus scripts/the-nightjar-flyer.txt
  printf 'look\nread the manifest\nsave /tmp/log.jsonl\nquit\n' | chorus scripts/the-nightjar-flyer.txt
  reach scripts/the-nightjar-flyer.txt card THE_WAX_CYLINDER --from-state /tmp/log.jsonl
```

The game's own `help` (verbatim, from the nightjar):

```
how to play
  act: <verb> <thing…> — e.g. read the manifest
  people go BEFORE the verb: `march <someone> <verb> …` makes two of you act together
  role words: to · with · at · about — `use on <thing> with <proof> <proof>`
  names forgive case, underscores and articles; one distinctive word is enough
  time: an action opens a station that finishes later — `wait` jumps to the next thing due; a clock like 1:12 jumps there
  save: write the play log to a file — `save <file.jsonl>`; `reach --from-state <file>` analyzes the midgame
  verbs: read · talk · explore · use on · travel to · wait · study · work · mansus · locate at · dream · acquire
  views: look · cards · inspect <card> · story · journal · save <file> · undo · help · quit
```

### reach — analyze

Verbatim `--help` (exit 0):

```
reach — static card reachability over a compiled script

usage: reach <script.txt> <COMMAND> [OPTIONS]
try 'reach --help' for the command list

Commands:
  card <NAME>      one card's verdict: proven paths and bindable slots
  all              the verdict for every name
  demand <QUERY>   the cards whose aspects/properties match a slot query like aspect:secret>=2
  verb <NAME>      one verb's report
  world            the whole-world dashboard
  graph            the mint dependency graph as Graphviz DOT

Options:
  --usable             also require bindability into a committable verb slot
  --why                append the per-site blocker table (card/all/demand)
  --cost               append the cost/count section (card/all; ignored under demand)
  --json               machine-readable output (card/all/demand; world/verb/graph ignore it)
  --by <MINUTE>        bind every time-keyed judgment to absolute minute MINUTE
  --from-state <LOG>   replay a JSONL action log as the analysis initial world
  --color              ANSI color (default off; when off the output is byte-identical to the plain face)
  -h, --help           print help

  a value option (--by, --from-state) repeated on one line: the last occurrence wins

examples:
  reach scripts/the-nightjar-flyer.txt card THE_WAX_CYLINDER --why
  reach scripts/the-nightjar-flyer.txt all --cost --by 600
  reach scripts/the-nightjar-flyer.txt world
```

Compositions: `--why`, `--cost`, `--by`, `--from-state` all compose with
each other and with the card faces (a value option repeated on one line
takes its last occurrence — e.g. `--by 100 --by 700` judges against minute
700); `--json` rides `card`/`all`/`demand`
only. Exit codes: 0 success, 1 compile/analyze error, 2 argv error (no
command, unknown command, missing value, unknown flag).

### lint-scripts — lint

Verbatim `--help` (exit 0):

```
lint-scripts — the script lint (§13.2) over named or bundled scripts

usage: lint-scripts [OPTIONS] [FILES...]

Args:
  FILES...    the scripts to lint; the default (no FILES) reads every scripts/*.txt relative to the CWD

Options:
  --reach     append the reach bridge's design-smell findings (the §13.1 → §13.2 six families; advisory — a reach finding never changes the exit code)
  --color     ANSI color (default off; when off the output is byte-identical to the plain face)
  -h, --help  print help

examples:
  lint-scripts
  lint-scripts --reach scripts/the-nightjar-flyer.txt
  lint-scripts --color scripts/the-nightjar-flyer.txt
```

Exit codes: **0 with any number of findings**; 1 for unreadable input or a
script that fails to compile; 2 for an unknown flag. Note the default: with
no files it scans `scripts/` — so keep your work-in-progress elsewhere
(say `docs/examples/` or a scratch dir) or you will lint the bundled
games.

### reach-fuzz / reach-scale — internal

Not authoring tools, and they speak flags, not `--help` (typo `--help` and
`reach-fuzz` just runs its default 200-seed campaign): `reach-fuzz`
differentially compares the analyzer against the real engine over generated
scripts (`--seeds/--steps/--states/--from`); `reach-scale` benchmarks the
census at Cultist-Simulator scale. `reach-fuzz` exits 1 only on a found
failure.

---

## 6. Troubleshooting FAQ

Load-time errors print as `Error: <message>` and exit 1; lint findings are
listed per file. In play, rejections print one line and log nothing.

**"bad kind term 'kind:txet' (unknown kind 'txet' — expected one of …)"**
Typo in a `kind:` term, or a `kinds:` declaration placed *after* its first
use (kinds are visible only below their declaration). Fix the spelling or
move the `kinds:` section up.

**"verb STUDY: declares slots but no recipes — every action rejects at
selection"** (lint) — the verb block has slots but its `recipes:` list is
missing or empty. Even one `default ->` line fixes it.

**"recipe read.late (READ): unreachable — read.default already matches
everything"** (lint) — you put `default` (or another always-true `when`)
above it. Reorder: gates and specials first, `default` last.

**"the when can never match — count(actors) < 1 can never hold — actors
takes 1..1 here"** (lint warning) — the condition contradicts the slot
cardinality (an `actors: 1` slot always binds exactly one card). Fix the
bound or the condition.

**My gate never fires — `when object.aspect.fueled < 1` looks right** — the
missing-key rule (§2.3): a numeric comparison over an aspect the card does
not carry is false in *both* directions, so a `< 1` gate on a
never-declared key is dead code and the ungated recipe wins. Neither the
lint nor `reach` catches it — the aspect is written (by your `SET`), it
just never exists before the first write. Fix: declare the start value on
the card (`with fueled:0`). Audit every `<`/`>=` comparison against the
`with` list of the cards it reads.

**"unknown template token {obect} — roles are {actors} {object} {with} {at}
{about} {to}, plus {roll}"** (lint) — typo in a NARRATE/PROGRESS/template
token (or an undeclared function parameter).

**"`default` takes no modifiers — got 'aspects fueled, job'"** — a `default`
recipe is exactly `default -> OUTCOMES`. If you need `aspects` (or `max`,
or `hidden`), give the recipe an id and a `when`:

```
fill.pour aspects fueled, job when count(with) >= 1 -> …
```

**"line 11: unexpected 'whnce' before '->' — positionals are [id]
[hidden|shown] [hint "..."] [max N] [aspects NAME, ...], then [when COND]"**
— misspelled recipe keyword. `when` must come last, just before `->`.

**"count() argument 'DREAD' is neither a role … did you mean
count(name:DREAD)?"** — `count()` takes a *role* (`count(with)`) or a
*query* (`count(name:DREAD)`), never a bare entity name.

**"trigger dusk_falls: REFUSE belongs to verb recipes — events have no step
to refuse"** — same rule bans `ADVANCE` and `RUN` in `at`/`on expire`
triggers: an event has no step and no window. Move the logic into a verb
recipe, or end with `ENDING`.

**"deck OMENS: entry PORTENT is not stackable — served again while a copy
lives, the spawn collides"** (lint) — the deck serves this non-stackable
name more than once (a repeated spec entry, a `defaultcard`, a `reset`
wrap, or a second deck dealing the same name), so a second copy will
collide with a living one. Mark the card `stackable`, or give the name a
single serving position. A non-stackable spec entry that can only ever be
dealt once (one position across every deck, once in its spec, no `reset`)
is exempt — the deck exhausts before a second copy exists.

**Play: `SPAWN: name 'THE_KEEPERS_NOTE' already in use`** — a completion
tried to mint a non-stackable card whose name is alive (usually: a
repeatable recipe with an unguarded SPAWN). Guard it
(`AND NOT exists(name:THE_KEEPERS_NOTE)`), mark the card `stackable`, or
cap the recipe with `max 1`.

**Play: `THE_LOGBOOK does not fit slot 'with' of FILL` + `usage: fill <item>
[with <fuel>]`** — the card you named does not satisfy that slot's query.
The usage line is generated from the slots; read it as the verb's contract.

**Play: `READ is already processing — its station frees at 0:27`** (or
`THE LAMP is busy with TEND — it frees at 0:33`) — one verb = one open
station, and cards bound in committed roles are busy. `wait` until the
window closes (or use a different verb, or a different card).

**Play: `deck 'DREAMS' is exhausted — no 'reset' and no 'defaultcard'
(§4.4)`** — the deck ran out. Add `reset` (repeat the sequence) or
`defaultcard X` (serve X forever) — or keep the exhaustion; it is a legal,
loud design.

**Play: `clock 0:0 is before state.now (0:24)`** — strict canonical input
with a past clock. Mechanics time is monotonic; use `undo` to go back, or
type a clock at/after `now`.

**Play: `bad tick (0..143)`** — the strict `d:u @ …` form bounds the
tick field at 143 (144 ticks per day). Type `1:0` for the next day's
start, or drop the `@` and let the lenient door wrap it.

**Play: `I don't know 'moon'` / `no verb found — try one of: …` + `did you
mean: …`** — lenient input failed and suggested corrections. Type one of
the suggestions, or `look`/`cards` to see what is actually here. A card
that left play answers nothing:

```
» inspect the oil jug
nothing on the table answers to 'the oil jug' — try 'cards' for the table
```

**Play: `RELATE needs (ref, ref, type[, value])`** — you passed role refs
(`RELATE(ACTORS, TO, neighbor)`); RELATE wants literal names
(`RELATE(WICK, THE_CAT, neighbor)`), and the type must be one of the seven
registered relation types (`parent_child parent_of neighbor north south
east west`).

**Play: `x` verb does nothing / try row is empty but the game "loads"** —
your verb header has no body (or its recipes were all unreachable). Run
`lint-scripts`; it names the verb.

**The lint exits 1 with an `Error:` line** — that is not a finding; the
script failed to *compile*, and the lint never runs on a broken script.
Fix the compile error first.

---

## Where to go next

- `scripts/the-nightjar-flyer.txt` — every feature, commented, in one
  working mystery.
- `docs/spec/text-engine-v0.7.md` — the exact rules: §4 dictionary and
  outcome operators, §5 sentence grammar, §6 slots/recipes/conditions, §10
  the script format, §11 the play interface, §13 the tools.
- `docs/examples/` — this tutorial's runnable companions: `01-minimal`,
  `02-recipes`, `03-world`, `04-triggers`, `05-decks-dice`,
  `06-complete-run-ops`, `07-second-place`, `08-smells`.

One working habit to take away: after every change to your script, run both

```sh
./target/debug/lint-scripts --reach docs/examples/your-script.txt
./target/debug/reach docs/examples/your-script.txt world
```

The lint catches the construction mistakes; `reach world` catches the
design ones. Then play — the try row is the game telling you what it
understood.
