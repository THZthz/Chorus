# Chorus Text Engine — Spec v0.7

A text-only prototype of a Cultist Simulator / Book of Hours-like card game.
Everything the game *is* — cards, actions, history — is expressed as structured
text ("sentences"). Sentences are canonical data: parseable, storable in a
database, replayable. Rendering (text cards now, drawn cards later) is a
projection of that data, never the source of truth.

> **Compatibility: none, by design.** Nothing in this document is a stability
> guarantee — the grammar, the log and script formats, and the API may break
> at any version boundary (v0.2.2 changed event semantics; v0.2.3 re-scaled
> the clock). Old logs are not replay contracts: re-author or migrate them
> yourself; the engine ships no migration path (§9.2). The established
> architecture is not frozen either: it may be changed when there is need —
> but the spec must be amended in the same change, so this document always
> describes the architecture as it actually is.

**Version log** — one entry per spec version, brief; §9.3 maps the retired per-version rulings to their current homes:

> Keep **concise**, one line each.

1. **v0.2** — grammar formalized and reconciled with examples (pre-verbal `AT`, arities, reserved words); USE_ON fixed to one object; single time model; step-atomic REFUSE; recipe ids; EVENT representation; aspects/attrs split; name uniqueness. The ruling ledger opens (§5.1+).
2. **v0.2.1** — spec–implementation alignment: reserved `AS`, canonical paren-less `SPAWN`/`NARRATE`, fully materialized concrete outcomes, prototype `derive`, recipe auto-ids, monotonic clock, slot `shown`/`hidden` display-only; the script format (§10) and the play interface (§11) documented.
3. **v0.2.2** — the processing cycle: `PROGRESS` ticks; commit → lock → ticks → outcomes → unlock; `{to}`/`{with}` prose tokens; `once` triggers; quantized step clocks with dead-air event crossings; first story content (the Nightjar Flyer).
4. **v0.2.3** — the halfhour clock: the tick shrinks from the hour to 30 minutes (`day:halfhour`, `halfhour` durations); quantization and dead-air semantics re-anchor on the halfhour tick.
5. **v0.2.4** — the card mapping (§8.1): every engine operation written down as a card operation; documentation only, no grammar/log/script/API change.
6. **v0.2.5** — SET arithmetic: signed deltas (`+1`, `-2`) and `aspect:` keys; the log carries computed absolutes so the fold replays literals and never redoes arithmetic.
7. **v0.2.6** — authorable decay: `on expire` triggers and the `REPLACE` outcome minting the successor in place; REFUSE a load-time error in trigger outcomes; play and fold agree exactly on `state.fired`.
8. **v0.2.7** — endings: the `ENDING` outcome and the `endings:` table; the first ENDING to apply in a step wins; undo reopens the game.
9. **v0.3** — the additive roll-up (v0.2.5–v0.2.7 collected under one number) plus `expires` accepting every §5.1 duration form and the core verb set growing ten → twelve (`STUDY`, `MANSUS`).
10. **v0.3.1** — stackable cards: same-name pools resolved oldest-first; the `name:` query term and `count(...)` over any §4.5 query.
11. **v0.3.2** — conditionals: `IF` / `ELSE IF` / `ELSE` in outcome lists; the log records only the taken branch, flattened in position.
12. **v0.3.3** — functions: parametric outcome macros, expanded at compile time — the engine and the log never see a call.
13. **v0.3.4** — seeded dice: `ROLL(count, sides[, bonus])` as the engine's only randomness, a pure counter function of `seed:`; engine-written `ROLLED` rows.
14. **v0.3.5** — decks: seed-fixed permutations over positions; `DRAW` and the engine-written `DRAWN(<DECK>, <name>)` rows.
15. **v0.4** — the Cultist-complete roll-up, owned by the showcase: the Nightjar Flyer plays pools, conditionals, functions, dice, and decks in one bundled story.
16. **v0.4.1** — compound duration literals (`1h30m`, `1d2h30m`), one shared parser across outcome args, quoted-string `expires`, and script `duration:` entries.
17. **v0.5** — parallel verb stations: BEGIN/COMPLETE/WAIT rows; committing opens a real processing window without moving the clock; explicit advances resolve due completions atomically before the halfhour's slot events.
18. **v0.6** — the engine-findings fixes: the authored `SPAWN … expires <duration>` modifier arming decay at mint; the RUN candidate walk's `when:` evaluated against the parent window's committed bindings overlaid by the projected clause.
19. **v0.7** — the coverage convergence (spec-side only; no grammar/log/script/API change): the document brought level with the shipped Rust crate — the v0.5 record model written into §5.1/§6.6/§7/§9.2 (WAIT rows own the clock; four record kinds; `startMin`/`window`), the turn-boundary undo stated (§7/§11), the play interface rewritten to the shipped line REPL (§11), the tooling layer documented (§13: the reach analyzer, the script lint, the bins), and the port's frozen wire/collation/escape semantics recorded (§4.2, §4.4, §5.1, §7).
20. **v0.7.1** — one escape discipline: ref formatting now doubles `\` like every other quoted form, closing the `a\b` round-trip hole (§5.1); a log holding such a name re-formats differently (§9.2, no compatibility).
21. **v0.7.2** — the loud-fail sweep: five silent-drop script corners become loud load errors.
22. **v0.7.3** — the layout convergence: oversized modules split along their seams, zero grammar/log/script change.
23. **v0.7.4** — the interface convergence: endings close the advance pass, advances cap at seven days, the line REPL lands.
24. **v0.7.5** — the reach fixpoint learns completion triggers; the `--world` whole-world dashboard joins as a seam module.
25. **v0.7.6** — the fuzz generator's scenario space covers the full trigger face; the analyzer gains the `--why` and `verb` diagnostic faces.
26. **v0.7.7** — the reach Complete arm tightens to a constructible completion: the window, aspect and draft rungs surface in `--why`.
27. **v0.7.8** — the analyzer's machine-readable exits: the `export` module lands `--json` envelopes and the `--graph` DOT face.
28. **v0.7.9** — the bounded-time faces: `--by <minute>` keys judgments on one horizon; `--from-state <log.jsonl>` replays a log as the initial world.
29. **v0.7.10** — the cost face: `--cost` reads the minimum applications, minutes and copies off the proven provenance DAG.
30. **v0.7.11** — the lint bridge: reach findings surface in lint-scripts via `--reach`, advisory only.
31. **v0.7.12** — the vocabulary convergence, script-side: a `kinds:` section registers new noun kinds into the load's dictionary.
32. **v0.7.13** — the vocabulary convergence, verb-side: a `VERB:` header declares new verb lexemes into the effective dictionary.
33. **v0.7.14** — the ops convergence: an `ops:` section declares script-defined outcome operators, expanded at load like functions.
34. **v0.8.0** — the bin-interface convergence: `reach` speaks the Commands + Options grammar; `--color`, lint-scripts' Options form and the tutorial land.

**Contents**: §1 [Goals](#1-goals) · §2 [Non-goals](#2-non-goals-v02) · §3 [Architecture](#3-architecture-three-layers) · §4 [L1 — Dictionary](#4-l1--dictionary) · §5 [L2 — Sentence grammar](#5-l2--sentence-grammar) · §6 [Engine layer](#6-engine-layer--properties-slots-recipes-content-model) · §7 [Database schema](#7-database-schema-append-only-log-projections-derived) · §8 [Rendering sketches](#8-rendering-sketches-text-now-drawable-later) · §9 [Design decisions](#9-design-decisions) · §10 [The script format](#10-the-script-format--a-game-as-one-text-file) · §11 [The play interface](#11-the-play-interface--lenient-input-the-line-repl-undo) · §12 [Remaining open questions](#12-remaining-open-questions) · §13 [Tooling](#13-tooling--the-reach-analyzer-the-lint-the-bins)

---

## 1. Goals

1. **Text-only.** No images. Every card and action is a sentence or a text block.
2. **Recordable.** Every action is stored in a database with an in-game
   timestamp, in a form that can be parsed back (lossless round-trip).
3. **Closed vocabulary.** A dictionary of KINDS, properties, VERBS and OUTCOME
   operators. Content (books, characters, places) is *data* that uses this
   dictionary, not new syntax.
4. **Expressive enough** for the CS/BoH loop: verbs with slots, slot
   requirements, recipes (condition → outcome), time cost, decay, containers.
5. **Drawable later.** Each element has fixed, labeled fields, so a UI can
   render cards without changing the data model. The operation-level mapping
   onto card operations is §8.1.

## 2. Non-goals (v0.2)

- Natural-language understanding of freeform player typing.
- Graphics or UI implementation.
- Game balance / actual content (separate content files).

---

## 3. Architecture: three layers

```
┌─────────────────────────────────────────────────────────┐
│ L3  Rendering    cards · verb stations · journal        │  ← projection
├─────────────────────────────────────────────────────────┤
│ L2  Sentences    "0:14 @ ANA AT HOUSE READ X -> …"      │  ← canonical log
├─────────────────────────────────────────────────────────┤
│ L1  Dictionary   KINDS · PROPERTIES · VERBS · OUTCOMES  │  ← ontology
└─────────────────────────────────────────────────────────┘
```

- **L1 Dictionary** defines what words exist (and enforces reserved words).
- **L2 Sentence grammar** defines how words combine into actions/events.
  The action log is append-only; game state = fold(log) (event sourcing).
- **L3 Rendering** projects entities and the log into text cards, verb
  stations, and a journal. (Later: a graphical projection of the same data.)

---

## 4. L1 — Dictionary

### 4.1 KINDS (noun classes)

| KIND        | Meaning                                   | Examples                         |
|-------------|-------------------------------------------|----------------------------------|
| `character` | Anything that can be an acting subject    | the player, followers, hunters   |
| `place`     | Locations, nestable (VILLA ⊃ THE_STUDY)   | HOUSE, GREENHOUSE, STREET        |
| `item`      | Portable physical things                  | candle, key, funds, tincture     |
| `text`      | Readable artifacts                        | book, letter, codex              |
| `idea`      | Intangibles produced by thought           | lore fragment, clue, memory      |
| `state`     | Conditions attached to a character or run | dread, affliction, vision        |
| `rite`      | A procedure-as-card the player owns       | a summoning rite                 |
| `resource`  | Abstract player stats as cards            | vitality, passion, reason        |

These eight are the **built-in kinds**, and four semantic roles are frozen
to specific built-in values — the engine never generalizes them to a
declared kind: `player:` must name a `character` (§10); the actors slot's
default pool is the player's own kind (§11); `place` owns zone containment
and the visibility fallout (§4.2, §11); and the `<someone>` phrasing of a
usage line fires for `kind:character` slots (§11).

Everything else about a kind is **open vocabulary**: a script's
`kinds:` section declares new kinds — indent-2 lines under a `kinds:`
header, one name per line in the class `^[a-z][a-z0-9_]*$` (`lower_snake`)
— registered into the load's dictionary in section order. That dictionary
vocabulary is what every check site reads: entity and prototype lines
(§10), `kind:` query terms wherever a query parses (§4.5), and the reach
analyzer's `--demand` parse (§13.1, whose Analyzer carries the
load-effective dictionary). An unknown kind is a load error naming the
whole list — built-ins first, then declarations in order, which is exactly
why declaration order is the suggestion order. A declaration itself fails
loud on: a duplicate (re-declaring a built-in included — the uniform §10
duplicate rule), a name colliding with a registered relation type (the
entity line dispatches relation types before kinds, so such a kind could
never be declared on an entity line), or a name outside the class. Kinds
are NOT reserved words (§5.1) — an entity may be named `character` — and
runtime stays vocabulary-free: the engine only ever compares the kind
strings on cards, never against the dictionary.

### 4.2 Entity properties (what a card carries)

| Property  | Shape                                     | Examples                            |
|-----------|-------------------------------------------|-------------------------------------|
| aspects   | weighted principle tags, dot sub-keys     | `lantern:2`, `language.latin:2`     |
| attrs     | engine/content scalars & flags            | `container:#study`, `state.sealed`, `expires:+24h`, `portable` |
| relations | typed edges to *other* cards              | `(BERTHA, parent_child)`            |

**Aspects are principle tags only** — the matching currency of slots and
recipes. **Knowledge is aspects on a character** (`occult:3`,
`language.latin:2`) — there is no separate skill system.

Aspect and attr maps are **two levels deep, exactly** (the Q13 model): only
the FIRST dot of a path splits — `a.b.c` addresses a sub-map key literally
spelled `b.c` — and a scalar head vs a sub-map clobber each other silently in
both directions (a scalar write replaces a whole sub-map and vice versa; a
scalar head reads as a miss). Maps also carry a **frozen serialization
order**, the JS property-key shape the port preserves: integer-like keys —
canonical unsigned decimals ≤ 2^32−2 — come first in ascending numeric
order, everything else keeps insertion order. Every clone and every
`aspects=`/`attrs=` materialization walks this order, so it decides the
exact logged bytes. Attr values are string | integer | boolean — no floats,
no null; aspect leaves are integers only.

**Structural properties are attrs**, not aspects. Engine-known attr keys:
`container` (where the card sits — the container's entity id), `expires`
(any §5.1 duration form — `2h`, `90m`, `1d`, `2halfhour`, compound `1h30m`
(§5.1) — quoted or bare;
the engine materializes `expiresAt` = current clock +
duration at set time, stored as **absolute minutes**, while the authored
`expires` attr keeps its authored form — minutes for bare, the string for
quoted — so only that attr and the logged sentence differ between the two
spellings) and the boolean flag
namespace `state.*`: `state.sealed`, `state.locked`, `state.hidden`,
`state.portable`, `state.unique`. Queries read them via `state:sealed` /
`state:!sealed` (§4.5); `SET`/`REVEAL` toggle them.

**Pools — `stackable`.** Dread, Funds, and Suspicion are pools of identical
cards, not unique cards. An entity or prototype line may carry the
`stackable` keyword; every mint site (SPAWN, REPLACE, DRAW) computes its
stackable flag from the mint's RESOLVED name and kind: a mint is stackable
iff its name is declared stackable — which declares its kind too — or its
KIND carries a stackable declaration (a plain `SPAWN PROTO` whose
prototype's kind carries any stackable declaration mints stackable as
well). The flag rides on the card; the fold recomputes the same rule from
the logged ref — concrete SPAWN rows always carry the resolved `kind:K` —
so play and fold decide from the identical (name, kind) pair.

- **The uniqueness law, revised.** A stackable mint never collides with its
  own pool, but MAY NOT collide with a distinct NON-stackable living name —
  one shared namespace: a non-stackable holder blocks stackable mints of the
  same name, a mixed pool is impossible (a non-stackable mint demands an
  empty pool), and the name frees only when the LAST copy dies.
- **Pool addressing = the oldest copy.** `state.byName` maps a name to its
  living ids in BIRTH order (never id-string order); a name ref with N > 1
  living copies addresses the HEAD — the OLDEST copy — everywhere name refs
  resolve: DESTROY, SET, MOVE, REPLACE, ref positions in every outcome,
  slot resolution, lenient input, and the play surface. `exists(NAME)` is
  the exception: true if ANY copy lives.
- **Authored destroys stay name-addressed** and LOG THE NAME ref —
  destroying the oldest, then the next-oldest, is repeatable authoring; the
  fold replays the same resolution. Engine-written destroys (the built-in
  expiry) address by `#id`, because under a pool a name-addressed destroy
  would kill the oldest copy while the card whose `expiresAt` crossed
  survived and re-fired forever.
- **`SET <poolname>, …` touches only the oldest copy** — pool-wide writes
  need role bindings (each bound card gets its own concrete SET, §4.4).
- **The play surface** shows every copy in the card listing; `inspect`
  reads the oldest; journal prose renders two pool copies bound into one
  role as "SUSPICION and SUSPICION". Lenient input addresses the oldest
  copy only — typed prose has no `#id` form; the canonical sentence may
  name `#id` explicitly as the escape hatch.

**Relations** are facts *between* two cards (the social graph — and the
corridor graph: the four directed types `north`/`south`/`east`/`west` place
cards on the table, `A is south of B` reads naturally and stores the authored
direction, so a layout can be read back from the relations table; a travel
verb gates on `rel(south, actors.container, object) OR …` — see §4.5).
Typed edges are declared in the dictionary as `symmetric` (`parent_child`,
`neighbor`) or `directed` (`parent_of`, the four directions). They change
only via `RELATE`/`UNRELATE` outcomes and live in their own table (§7).
Condition predicates read them: `rel(parent_child, actors)`.

### 4.3 VERBS (v1 core set)

A verb's lexeme may embed its preposition (matching the `LOCATE_AT` style).
Each verb declares which roles it needs:

Roles: `actors` (subject, an unordered set) · `object` (direct) ·
`with` (instruments) · `at` (where) · `about` (topic) · `to` (recipient).

**ACTORS is an unordered set** — one or more characters. `A, B TALK …` and
`B, A TALK …` are the *same* action. The engine composes refs in canonical
(sorted) order and the log stores them sorted, so equality is structural.

| Verb        | Signature                              | Purpose                        |
|-------------|----------------------------------------|--------------------------------|
| `READ`      | ACTORS READ object [AT place]          | study a text → ideas           |
| `STUDY`     | ACTORS STUDY object [AT place]         | the CS study station (READ's signature) |
| `WORK`      | ACTORS WORK [AT place]                 | labor → resources              |
| `TALK`      | ACTORS TALK [AT place] [ABOUT idea]    | social outcomes, group actions |
| `EXPLORE`   | ACTORS EXPLORE place                   | discover contents of a place   |
| `MANSUS`    | ACTORS MANSUS place                    | enter a place — the Mansus crossing (EXPLORE's signature) |
| `TRAVEL_TO` | ACTORS TRAVEL_TO place                 | deliberate movement            |
| `LOCATE_AT` | ACTORS LOCATE_AT place                 | set/dump position (utility)    |
| `DREAM`     | ACTORS DREAM [WITH idea]               | visions, rest                  |
| `USE_ON`    | ACTORS USE_ON target [AT place] [WITH item] | craft, rites, keys, tools |
| `ACQUIRE`   | ACTORS ACQUIRE object                  | buy/gain via cost              |
| `WAIT`      | ACTORS WAIT                            | pass time (auto duration)      |

Signatures say which roles a verb may take; cardinalities and queries are
content (§6.1). The seed content narrows some of these (e.g. seed `ACQUIRE`
takes an object of any kind; seed `WORK` declares no `with` slot). The core
set is a default, not a closed wall: a script's `verbs:` block
declares a brand-new lexeme with an indent-2 `VERB:` header — or configures
a core one under the same header — and the load registers unknown lexemes
into its effective dictionary (register-if-unknown on the per-load clone,
the §4.1 `kinds:` discipline, so recompiling one text against one dictionary
stays idempotent). The nameable grammar applies at that registration, §5.1's:
a lexeme must be UPPER_SNAKE (`^[A-Z][A-Z0-9_]*$`), never a registered verb
(a repeated header is the uniform duplicate load error, §10), and never a
reserved word — `EVENT:` or `TO:` as a header is a load error naming the
lexeme (the core verbs are exempt as reserved-by-being-a-verb, which is what
lets a header configure one). A declared lexeme may not double as a declared
entity or prototype name either: registration grows the reserved set, so one
order-independent check over the whole load rejects the overlap in either
section order, naming both faces of the collision. **Declaration is not
playability** — the config block under the header (slots, duration, recipes,
§6.1) is what makes a verb playable: a header with no block compiles an
empty verb no recipe can match, and a lexeme the dictionary knows but the
content never declares rejects at the step ("is not a content verb").
Relation types, by contrast, remain deliberately unvalidated — any string is
a legal relation type. A verb with no template
contributes no template line; its recipe NARRATEs still speak.

### 4.4 OUTCOME operators (fixed vocabulary, **normative arities**)

| Operator  | Args (normative)                | Effect                                        |
|-----------|---------------------------------|-----------------------------------------------|
| `SPAWN`   | ref [, INTO ref] [, AS #id] [, expires duration] [, k=v …] | mint a new entity (from a prototype if one is defined); `AS #id` is engine-written so folds replay exactly; `aspects=`/`attrs=` JSON pairs are engine-written materialization (§9.2) — and the ONLY pair keys: any other `k=v` on SPAWN is a parse error; the authored `expires <duration>` modifier arms the mint at birth with the SET-expires pair (§4.2), so an authored outcome can decay the card it just minted even when the name pools — the concrete row carries the pair inside its `attrs=` materialization (§9.2), anchored to the MINT clock — the step clock at the table, the window's START clock inside a completion (§6.7), a slot/expire trigger's crossing tick (an `on complete` mint anchors to the completion's window-start clock) — never a follow-up derivation; a zero duration (`0m`) fires at the first halfhour boundary at or after that minute that the clock actually crosses |
| `DESTROY` | ref                             | card removed (deadAction stamped)             |
| `REPLACE` | ref `WITH` prototype            | decay a card into its successor: mint from the prototype into the dying card's **current location** — its `container` (place-contained or character-carried) or, for a zone-tagged card, its `zone` (§10: the grouping has no card, so the tag passes to the successor) — then soft-destroy it — spawn first, then destroy, so the old name is released as usual. Paren-less authored form `REPLACE X WITH Y`; the engine logs the concrete `SPAWN … AS #id` + `DESTROY #id` pair, so there is no REPLACE in the log. Prototype derivation (`name from object`) is unavailable — REPLACE has no clause. A name collision with any living entity — the dying card itself included, still alive at spawn time — rejects the whole step (§9.2); revised for pools: a stackable successor may share a stackable pool, so `REPLACE X WITH X` succeeds — append-then-pop, the pool never dips, and the next pool op addresses the next-oldest. A replaced place's children keep pointing at the dead place id; re-parenting them is the author's job. REPLACE always writes the `attrs=` pair, even empty — the pair is the authoritative materialization (§9.2), so the fold cannot grow a location the mint never had |
| `SET`     | ref, key, value                 | mutate an attr (`state.sealed`, `container`, `expires`…) — or the aspects map when the key is `aspect:`-prefixed (`aspect:level`); a signed integer value (`+1`, `-2`) is a delta on the entry's current value and logs the computed absolute: a missing entry counts as 0, results may go negative (no clamping — a counter may read below zero), adding to a non-numeric entry is an application failure (§9.2), and a delta over a role ref applies per bound card, logging one concrete SET per card (`#id`s, each with its own result); deltas apply to the draft, so a later REFUSE in the same step discards them whole (§6.5). `SET x, expires, 2h` also materializes `expiresAt` (§4.2) |
| `MOVE`    | ref, INTO ref                   | relocate (place→actor, actor→place)           |
| `REVEAL`  | ref                             | unlock hidden text / mark discovered          |
| `ADVANCE` | duration (`2h`, `90m`, `1d`)    | authored extra time shift (§6.6) — verb recipes and `on complete` bodies only; in a slot/expire trigger it is a load error (§6.6) |
| `NARRATE` | string                          | journal line only; raw template stored, interpolated at render |
| `PROGRESS`| duration, string                | a read-bar tick: authored progress text at `duration` into the processing window (§6.7); journal-only, no state or time effect |
| `RELATE`  | ref, ref, type [, int]          | create a relation edge between two cards      |
| `UNRELATE`| ref, ref, type                  | remove a relation edge                        |
| `ENDING`  | id                              | end the story: sets `state.ended` to `{id, title, flavour}` resolved from the `endings:` table (§10); the id must be declared — load-time error otherwise (recipe or trigger); the first ENDING to apply in a step wins, later ones are logged no-ops; the step itself completes normally — the ending's OWN completion still applies its whole outcome list (an ADVANCE extension it carries counts, §6.6). Within one advance pass the story's ENDING closes the pass: an action's own advance finishes, but an EVENT ending (a trigger's or a completion's) stops the pass AT THAT MINUTE — the wait row's span ends there, later due completions and slots do not fire, and the clock does not run past the story. A REFUSE later in the same step discards the ending with the whole draft (§6.5). Post-ending steps are rejected before validation, nothing logged, the message naming the ending's title; the fold itself never enforces game-over (it trusts logged rows) — the rejection lives in the step executor only |
| `IF`      | cond `->` `(` outcomes `)` { `ELSE IF` cond `->` `(` outcomes `)` } [ `ELSE` `->` `(` outcomes `)` ] | branching (§6.8, §6.3): arms evaluate in order against the draft, first true condition wins, `ELSE` is the default, no true arm and no `ELSE` applies nothing; the log records only the TAKEN branch's outcomes, flattened in position — no IF ever appears in the log |
| `ROLL`    | count, sides [, bonus]          | roll dice: paren-only; each argument is a §6.3 value expression (`3`, `total(actors, aspect:nerve)`, `count(name:X)`, `object.aspect.dc`, …). Rolls `count` dice of `sides` faces plus `bonus`, consuming exactly `count` rng counters; binds `roll` for the rest of the outcome list. The log records the engine-written `ROLLED(<total>, <ctrAfter>)` — no ROLL ever appears in the log. `sides` < 2 or `count` < 0 reject the whole step (§9.2); `count` = 0 is legal (total = bonus, counter unmoved); an argument expression that cannot resolve (a missing aspect over its bound role) rejects the step rather than silently rolling nothing |
| `ROLLED`  | total, ctrAfter                 | **engine-written only**: the record of one ROLL — the fold sets `state.rng = ctrAfter` and derives nothing else. Authoring one is a load error |
| `DRAW`    | deck [, INTO target]            | draw the next card of a `decks:` declaration: paren-less, the deck id is a plain name (a declared-deck check runs at load, over recipes and triggers, IF branches included; an empty spec is a load error); INTO is the SPAWN target grammar, and must RESOLVE — a role over empty event bindings or a dead ref rejects the step rather than materializing the card nowhere. With no INTO the mint reuses SPAWN's default (the step's `at` card, else the first actor's `container`; in triggers, where bindings are empty, the card is minted containerless unless an explicit INTO is authored). The deck entry may name a prototype or a declared entity; repeats in the spec weight the draw; a DRAW inside an untaken IF branch consumes nothing — the cursor rides the draft (§9.2). The log records the engine-written `DRAWN(<DECK>, <name>)` + the concrete `SPAWN … AS #id` pair — no DRAW ever appears in the log |
| `DRAWN`   | deck, name                      | **engine-written only**: the record of one DRAW — the fold advances that deck's cursor, asserting `name` is exactly what the compiled permutation serves at the cursor (content drift between play and fold fails loud); the recorded name is the SPEC entry, not a derived mint name, so the assertion reads the permutation. Authoring one is a load error |
| `RUN`     | `"recipe-id"` `, …`             | chain the completion (§6.7): walk the candidates in order, pin the first whose `when:` matches the post-outcome draft (lenient, §6.4) and whose `max` (§6.4) is unspent, and open it as a follow-up window on its verb with the parent's bindings projected onto the candidate verb's declared roles; no match — the chain ends silently; zero-concrete (the follow-up's BEGIN row is the only record). In recipe bodies: same verb, load-checked; in `on complete` bodies: cross-verb legal (global id table), the parent's own station is not busy. A second RUN op in one completion rejects the window |
| `REFUSE`  | string                          | abort the whole step (§6.5): draft discarded, attempt still logged |

Shorthand: a bare ref in an outcome list means `SPAWN ref` (a **new** entity,
from its prototype if defined, into the step's `at` place or the first actor's
`container`). Moving an *existing* card is `MOVE`. Outcomes apply in order.

**Role refs are valid outcome args**: any ref position may name a role —
`ACTORS`, `OBJECT`, `WITH`, `AT`, `ABOUT`, `TO` (case-insensitive; rendered
uppercase) — and expands to that role's bound cards (`DESTROY WITH`,
`MOVE ACTORS INTO OBJECT`, `SPAWN MARK INTO at`). Role refs are concretized to
`#id`s in the logged outcome (§9.2).

**Declared functions may head an outcome position**: `NAME(arg, …)` dispatches
to a `functions:` declaration and expands to its body AT LOAD — one vocabulary,
two namespaces of convenience: functions are not ops (§4.4's table is closed),
declaring one named like an op is a load error (§10).

**Declared ops head one too**: `OP(arg, …)` dispatches to an `ops:`
declaration and expands to its body AT LOAD through the same pass — the
built-in table above stays closed, and the log never carries an op name (the
§10 op rules cover the name rules, the per-call-site arity rule, and the
composition rules, including the ban on calling an op from a function body).

**Operator internals.**

*Dice — the seeded counter.* The engine's only randomness (§9.2,
determinism) is `State.rng` — an integer counter of how many die values the
whole history has consumed. Die `i` of a roll at counter `c` is
`1 + floor(sides * splitmix64(seed XOR (c+i)) / 2^64)`, with `seed` the
script's `seed:` meta line (§10, default 0) and splitmix64 computed in exact
64-bit integer arithmetic — u64 wrapping ops for the finalizer, the
`sides * value` product widened to u128 so the floor-divide-by-2^64 reads
the high bits a wrapping multiply would destroy (the port's frozen
mechanism; the test suite pins a python-computed vector table plus a 2^53−1
seed to keep it exact). `seed` and the counter enter the key as their low 64
bits — a negative `seed:` re-interprets two's-complement, and the −1/−42
frozen vectors pin that. A roll of m dice consumes exactly m counters. Play
consumes them on the draft; the fold's ROLLED apply sets `state.rng =
ctrAfter` and derives nothing else. Undo past a roll lands the counter
exactly where the folded history left it — no dice bookkeeping exists outside
the log. A REFUSE after a ROLL discards the draft: the real counter never
moved, and a re-attempt throws the same dice — deterministic, documented,
accepted. Changing `seed` shifts every future die but never past rows — the
fold replays recorded totals verbatim (§9.2, no compatibility). Triggers may
roll and tier on `roll`; the EVENT row carries the concrete outcomes incl.
the ROLLED row and replays exactly — but a trigger's ROLL arguments must be
ROLE-FREE (`ROLL(count(actors), 6)` is a load error naming the argument;
world forms like `count(name:X)` are the trigger vocabulary).

*The `roll` binding.* The last ROLL of an outcome list binds `roll`
(last-wins; a second ROLL rebinds); the slot resets at each `;` clause of a
multi-clause step, survives function expansion (expansion is compile-time),
and is NOT state — a REFUSE discards it with the draft. It is readable in IF
conditions (§6.8), in NARRATE strings, where `{roll}` is rewritten at
execution so the concrete row carries the number (a NARRATE before the list's
ROLL keeps the literal token), and as a ROLL argument itself — a second
`ROLL(roll, 6)` may size its dice off the first roll's total; an unbound
`roll` there refuses the window at runtime (a journaled REFUSE, §6.7 — the
backstop below). A `when:` using `roll` is a load error
(selection provably runs before any outcome could have rolled); `{roll}` in
PROGRESS or REFUSE strings is a load error (that prose is stored raw and
rendered without an execution pass); the runtime rejection of a taken branch
reading an unbound `roll` is the backstop for the best-effort may-bind lint
(a `roll` reference with no ROLL anywhere before it on any path of its list
is a load error; a ROLL hidden inside an earlier IF's branches counts as
may-bind — false negatives allowed, the runtime covers them).

*Decks — the seed-fixed permutation.* The draw order is a Fisher–Yates
shuffle over the POSITIONS of the deck's spec, position i drawing its shuffle
value from `splitmix64(seed XOR hash(deckId) XOR i)` (an FNV-1a-64 hash of
the deck id standing in for the dice counter) — pure in (seed, deck id),
fixed at load, identical at play and fold, and consuming NO rng counter (deck
streams and the dice stream are independent key spaces of the same mixer, so
drawing never shifts a die). Shuffling positions — not names — preserves the
multiset: repeats keep their weighting in every period. The permutation lives
on compiled content; `State.decks` holds one cursor per deck, cloned like
every state field (§9.2, atomic drafts). A draw serves `order[cursor]` and
advances the cursor on the draft; past the period, `reset` wraps (the
sequence is periodic — no mid-run reshuffles), `defaultcard` serves forever,
and with neither the draw rejects the step (§9.2, application failure; cursor
unmoved). Play and fold share one serving function, so the two can never
drift. Changing `seed` or the spec under an old log shifts future draws only
— but the DRAWN row's assertion re-derives its expectation (cursor +
permutation), so the re-fold fails LOUD instead of silently diverging;
re-fold such logs against the content that played them.

### 4.5 Query mini-language (slot queries — one card at a time)

```
kind:text  aspect:secret>=3  aspect:language.latin>=2  state:!sealed  -aspect:winter
name:SUSPICION  -name:SUSPICION        (§4.5: exact-case, like name refs and exists)
```

- `kind:` — exactly one of the §4.1 kinds (the built-ins plus the script's
  `kinds:` declarations); an empty or unknown kind is a
  parse error wherever a query parses — slots, `exists()`, `count()`
- `aspect:path[>=|<=|>|<|!=|=]level` — principle match: all six comparators,
  levels may be negative; bare `aspect:x` = level ≥ 1
- `state:key[=|!]value` — attr match against the `state.*` namespace
  (`state:!sealed` → `attrs['state.sealed']`). `state:k` is `=` with the
  value `true`; `state:!k` is strict inequality against the boolean `true` —
  the string `"false"`, a `0`, and a MISSING key all read unsealed (the
  default-open rule content relies on); an ordering op over a non-numeric
  value reads false; values coerce `true`/`false` → boolean, an integer →
  number, anything else a raw string
- `name:NAME` — exact-case name match (§4.5); composes with the other terms,
  and `-name:x` negates like any term. A comparison inside the value is a
  load error naming the outside form (`count(name:X) >= 1`) — the
  `exists()`/`count()` glue-tightening below would otherwise bake `X>=1` in
  as the NAME, a silent 0
- `-` prefix — negation

Slot queries test **one card at a time**. Anything about a *group* (size,
relations between members, pooled power) is a recipe condition (§6.3) — that
is deliberate: the UI can grey-out invalid drops from slots alone, while
group logic stays narrative.

Alternation (§4.5): `|` separates conjunction groups —
`aspect:ability | aspect:job aspect:ritual` matches ability, or job AND
ritual — with every term form unchanged; legal wherever a query parses
(slots, `exists()`, `count()`). `|` binds loosest; a leading or trailing `|`
or an empty group is a parse error. Selection stays per-card: the DNF
evaluates against ONE card (the UI's drop-grey-out is unchanged); group
logic remains a recipe concern (§6.3's deliberate split). CS `forbidden`
maps need no alternation — conjunctions of negations are already
expressible. Queries are authored strings, never serialized and never
logged, so the §5.2 round-trip does not apply to them (§9.2, one home
each).

---

## 5. L2 — Sentence grammar

### 5.1 EBNF (canonical form)

```ebnf
log        = { sentence } ;
sentence   = [ clock "@" ] ( action | event | complete | wait ) ;
complete   = "COMPLETE" verbo [ "->" outcomes ] ;
                                                 (* ENGINE-WRITTEN: one processing
                                                    window closing (§6.7) — the verb appears as a
                                                    raw name token riding the record's `trigger`
                                                    field (never ref(); engine-written rows always
                                                    carry a registered verb, the grammar itself does
                                                    not check registration); parses so every
                                                    concrete row round-trips *)
wait       = "WAIT" duration ;                   (* ENGINE-WRITTEN: one advance pass —
                                                    the span that moved `now` (§6.7); WAIT is
                                                    reserved (a core verb), so this sits before
                                                    the action fallthrough *)
clock      = int ":" int ;                       (* "day:halfhour", day 0-based, halfhour 0..47; 1 halfhour = 30 min *)
action     = clause { ";" clause } [ "->" outcomes ] ;
event      = "EVENT" trigger-id [ "->" outcomes ] ;
trigger-id = name { ":" name }                    (* segments reassembled — the
                                                     built-in expiry id is `expire:<NAME>`
                                                     (§6.6); an unsafe segment prints quoted *)
clause     = actors [ "AT" ref ] verbo [ "TO" ref ] [ objects ] [ "WITH" reflist ] [ "ABOUT" ref ] [ "AT" ref ] ;
(*          the trailing "AT" is input sugar only (§5.1); canonical output is pre-verbal *)
actors     = ref { "," ref } ;                   (* 1..n, order-free; canonical = sorted *)
objects    = ref { "," ref } ;
reflist    = ref { "," ref } ;
verbo      = VERB ;                              (* must be a verb registered in the dictionary *)
outcomes   = outcome { "," outcome } ;           (* commas inside ( ) belong to args *)
outcome    = OPERATOR "(" [ args ] ")"           (* SET/RELATE/UNRELATE/REFUSE/ADVANCE/PROGRESS: parens required *)
           | "SPAWN" ref [ "INTO" target ] [ "AS" "#" id ] [ "expires" duration ] { key "=" value }
                                                 (* expires: the authored mint-decay modifier (§4.4, §4.2) —
                                                    any §5.1 duration form, bare or quoted; consumed into the
                                                    attrs= materialization (§9.2), never logged itself. The bare
                                                    ref sugar stays one ref — the modifier rides the SPAWN op.
                                                    The modifier is the SPACE form only: the key=value spelling
                                                    is NOT the modifier — `SPAWN X expires=2h` is a loud parse
                                                    error (the `expires` keyword branch fires before the `k=v`
                                                    pair loop, so the pair spelling can never silently stand
                                                    in for the modifier), and a function parameter expanded
                                                    into a pair position lands as a bare value the grammar
                                                    rejects loud (§9.2). The pair loop itself admits only the
                                                    engine-written `aspects=`/`attrs=` keys — any other key is
                                                    a parse error *)
           | ( "DESTROY" | "REVEAL" | "MOVE" ) parenlessTail      (* input sugar *)
           | "REPLACE" ( ref | role ) "WITH" ref (* paren-less only; WITH is syntax, not a role *)
           | "NARRATE" string
           | "ENDING" ref                        (* paren-less only; names an `endings:` entry (§4.4) — a name ref, quoted ids round-trip; existence is a load-time check *)
           | "IF" cond "->" "(" outcomes ")"
             { "ELSE IF" cond "->" "(" outcomes ")" }
             [ "ELSE" "->" "(" outcomes ")" ]    (* §6.8, §6.3: bodies REQUIRE parens; the ELSE chain is consumed greedily by the IF — a nested ELSE binds to the nearest IF; cond is the §6.3 grammar, read from the token stream up to the depth-0 "->" *)
           | "ROLL" "(" valexpr "," valexpr [ "," valexpr ] ")"
                                                 (* §4.4: paren-only; each argument is a §6.3 cmp-operand value expression (int, count(role), count(<query>), total(role, aspect:path), role.aspect.x, role.attr.x — a body parameter inside a function template), split on depth-0 commas so nested parens survive; arity 2..3 *)
           | "ROLLED" "(" int "," int ")"        (* §4.4: ENGINE-WRITTEN only — the recorded roll; authoring one is a load error *)
           | "DRAW" name [ "INTO" target ]       (* §4.4: paren-less only — the deck id is a plain name ref (declared decks are pinned at load); INTO is the SPAWN target grammar and must resolve *)
           | "DRAWN" "(" name "," name ")"       (* §4.4: ENGINE-WRITTEN only — the recorded draw, `DRAWN(<DECK>, <name>)`; an unsafe name formats re-quoted and re-parses (ref's string form); authoring one is a load error *)
           | "RUN" string { "," string }         (* §4.4/§6.7: quoted-string recipe refs — ids are lowercase dotted
                                                    and the NAME class has no dot, so a bare id is unlexable;
                                                    a comma continues the list only when a quoted string follows,
                                                    so a next outcome opening with a string parenthesizes out
                                                    (`RUN("a"), SPAWN NAME`) *)
           | ref ;                               (* bare ref = SPAWN ref, engine adds AS #id *)
(*          canonical OUTPUT: SPAWN, NARRATE and ENDING paren-less, every other op paren'd
            — e.g. SPAWN X INTO #house AS #c1 · NARRATE "…" · MOVE(#ana, INTO #study) · ROLLED(14, 3) · DRAWN(RUMOURS, HEARSAY)
            — authored REPLACE is paren-less too, but never reaches the log: it concretizes to SPAWN+DESTROY (§4.4) *)
args       = arg { "," arg } ;                   (* arg shapes validated per §4.4 arity *)
arg        = ref | role | int | signedInt | string | bool | duration
           | "INTO" target | "AS" "#" id | key "=" value | dottedName | aspectKey ;
signedInt  = ( "+" | "-" ) int ;                 (* SET delta value only (§4.4) *)
aspectKey  = "aspect:" name { "." name } ;       (* SET key into the aspects map (§4.4) *)
target     = ref | role ;
role       = "ACTORS" | "OBJECT" | "WITH" | "AT" | "ABOUT" | "TO" ;  (* case-insensitive *)
dottedName = name { "." name } ;                 (* SET(x, state.sealed, true) *)
duration   = int ( "halfhour" | "h" | "m" | "d" ) { int ( "halfhour" | "h" | "m" | "d" ) } ;
                                                 (* 1halfhour = 30 minutes; compound
                                                    literals like `1h30m` sum their pairs; every path shares
                                                    this one parser (bare args, quoted-string `expires`,
                                                    script `duration:`). Canonical rendering prefers one
                                                    exact unit (d, then h, then halfhour, then raw minutes —
                                                    `90m` renders `3halfhour`, never `1h30m`) and decomposes
                                                    into compound form only when no single unit fits
                                                    (`100m` → `1h40m`, `61m` → `1h1m`) — every canonical
                                                    form re-parses (§5.2) *)
ref        = name | kind ":" name | "#" id | string ;  (* canonical output: name *)
```

Lexical rules:

- `name` = `[A-Za-z_][A-Za-z0-9_]*`. **Reserved words** (`EVENT COMPLETE TO WITH AT
  ABOUT INTO AS AND OR NOT IF ELSE ROLL ROLLED DRAW DRAWN RUN` + every registered
  verb lexeme) cannot be used as names; content needing them uses the quoted-string
  ref form (`"PLAN B"`). A script's `verbs:` declarations join the reserved set
  at load (§4.3) — and because that growth would otherwise make name
  legality depend on section order, one order-independent load check rejects a
  verb lexeme that doubles as a declared entity or prototype name. The role words are a SOFT reservation: an entity named
  `object` is addressable in clause positions, but in outcome-arg positions the
  role ref intercepts before the bare-ref fallback (and a §10 parameter before
  the role) — such an entity must be quoted there.
- **Quoted strings carry an escape grammar**: the named escapes `\b \t \n \v
  \f \r \0` are the control characters; `\"` `\'` `\\` and every unknown
  escape stand for the literal next unit; a trailing backslash is an
  unterminated string literal.
- **One escape discipline for every quoted form.** Refs, prose strings, RUN
  ids, pair values and trigger-id segments all escape `\` first, then `"`;
  the lexer maps `\\` → `\` and `\"` → `"`, so every quoted form re-lexes to
  the same units and an unsafe name is lossless whatever it contains.
- **Collation and classes are frozen** (the port's JS-emulation semantics):
  "sorted" means UTF-16 code-unit order (`cmp_utf16`), never Rust byte order —
  non-BMP names sort by surrogate pairs; DSL whitespace is its own frozen
  class (U+FEFF counts, U+0085 does not — it differs from both Unicode
  White_Space and `str::trim`); authored integers are 64-bit and saturate at
  the i64 bounds rather than erroring (a huge `seed:` or clock day lands on
  the bound).
- **Entity names are unique among alive entities** (enforced at SPAWN /
  registration) — except cards minted from a `stackable` declaration, which
  pool under one name (§4.2). Canonical ref output is the bare name;
  `kind:NAME` and `#id` are accepted input forms (sugar; typed refs are
  kind-checked at resolution). A name ref under a pool addresses the OLDEST
  copy (§4.2).
- Post-verbal `AT` (`ANA READ X AT HOUSE`) is accepted as input sugar and
  canonicalized to the pre-verbal position — the canonical sentence is
  `ANA AT HOUSE READ X`.

Clause semantics:

- `;` separates **clauses** within one player step. Each clause is validated
  and recipe-matched independently, in order (§6.6). `,` is reserved for lists.
- **ACTORS is an unordered set**: `A, B TALK …` and `B, A TALK …` produce the
  identical canonical sentence.
- `EVENT` sentences are fired by the clock (triggers, expiries), not initiated
  by the player; the fold trusts logged EVENT rows (§6.6).

### 5.2 Worked examples (all parse; all round-trip)

Solo read — `LOCATE_AT`'s job is done by the context phrase `AT`:

```
0:14 @ ANA AT HOUSE READ THE_FORGETTING -> CLUE_SECRET_HIDEOUT
```

Two characters, order-free — both lines record identically (bare-ref outcome
mints `PLAN_INFILTRATION` from its prototype):

```
2:19 @ ANA, BERTHA AT DINING_HALL TALK ABOUT CLUE_SECRET_HIDEOUT
    -> SPAWN PLAN_INFILTRATION AS #p1

2:19 @ BERTHA, ANA AT DINING_HALL TALK ABOUT CLUE_SECRET_HIDEOUT
    -> SPAWN PLAN_INFILTRATION AS #p1
```

One step, two clauses (travel then act); no `ADVANCE` — the clauses'
durations become the processing windows (§6.6, §6.7). This record's `t_game`
is the step's start clock:

```
0:14 @ ANA TRAVEL_TO VILLA; ANA AT THE_STUDY READ THE_PALE_LADY
    -> CLUE_SECRET_HIDEOUT
```

Time fires an event (own log row, at the crossing tick):

```
0:16 @ EVENT dusk_falls -> SPAWN DREAD, NARRATE "The lamps come on."
```

Rite via `USE_ON` with instruments (actors operate the rite together):

```
1:23 @ ANA, MY_FOLLOWER AT HOUSE USE_ON RITE_OF_ASCENSION WITH CHALICE
    -> DESTROY(CHALICE), SPAWN MARK_OF_ASCENSION INTO HOUSE
```

Counter idioms — a signed value is a **delta** on the entry's current value,
and an `aspect:` key writes the aspects map (§4.4):

```
3:2 @ ANA AT HOUSE READ THE_FORGETTING -> SET(DREAD, aspect:level, +1), SET(ANA, funds, +2)
```

Executed, the engine computes and logs the **concrete absolute** form (DREAD
carried `aspect:level` 2; ANA had no `funds` yet) — the record replays
without arithmetic:

```
3:2 @ ANA AT HOUSE READ THE_FORGETTING -> SET(DREAD, aspect:level, 3), SET(ANA, funds, 2)
```

These examples show the **parse-level canonical form** (sugar resolved: bare
ref → `SPAWN`, pre-verbal `AT`, sorted actors). When the engine *executes* a
step it materializes further and the recorded sentence is longer — minted ids,
role refs concretized to `#id`s, SPAWN's engine-written `INTO`/`aspects=`/
`attrs=` pairs (§9.2; the pairs are space-separated inside the SPAWN).
The same rite, executed, logs:

```
1:23 @ ANA, MY_FOLLOWER AT HOUSE USE_ON RITE_OF_ASCENSION WITH CHALICE -> DESTROY(#rite), DESTROY(#chalice), SPAWN MARK_OF_ASCENSION INTO #house AS #e2_1 attrs="{\"container\":\"house\"}"
```

### 5.3 Prose rendering (L3 template, not stored truth)

Each verb owns a template using **role names** (`{actors} {object} {at}
{about} {to} {with}`):

```
READ: "{actors} read {object} at {at}."
TALK: "{actors} question {to}."
```

→ *"Ana reads The Pale Lady at the Study — she finds a clue to the hideout."*
Actor sets join in prose as *"Ana and Bertha"*, following the canonical
(sorted) order. `NARRATE`/`PROGRESS` outcomes store raw templates;
interpolation happens only here. A record renders as the **processing cycle
read aloud** (§6.7): one verb-template line per clause, then the outcomes in
emission order — `PROGRESS` ticks indented under the template with their
offsets, `NARRATE` lines unindented, and a `REFUSE` spoken as its own line
(§6.2):

```
MARCH read THE_VANE_LEDGER.
  (+30m) Two hands wrote this ledger, and in the margins they argue.
Quill's neat corrections hide a steady drain on the aether royalties. The lord had found it. Quill knew he had.
```

Verb templates render **per clause**; `NARRATE`/`PROGRESS` templates render
**per record**, so their tokens resolve against the record's first-clause
roles (§7) — keep record-level templates role-agnostic or place them in
single-clause steps. A WAIT row speaks one line — `  (+<dur>) time passes.` —
so an advance pass is never silent; a record whose outcomes contain ENDING
closes its prose with `— <title> —` (first ENDING wins, mirroring the state
rule; a refused step's ending was discarded with the draft and gets no
line). Whether a live UI shows each tick as it comes due or
prints the whole read-bar at completion is a pure display choice; the
canonical record is identical either way.

---

## 6. Engine layer — properties, slots, recipes (content model)

### 6.1 Slot cardinality: how many cards a role takes

A slot declares a **role**, a **count**, an optional visibility flag, and a
query every member must satisfy. One slot per line (script syntax, §10):

```
slot  := role ":" count [ query ]
role  := "actors" | "object" | "with" | "at" | "about" | "to"   (* §4.3's six roles, exact lowercase *)
count := int [ ".." [ int ] ]                    (* "1", "2", "1..2", "2..", "0..1" *)
```

A slot with `max < min` is a compile error, and so is a slot query naming
an empty or unknown kind (§4.5).

A slot key outside the six-role vocabulary is a load error naming the valid
roles (§10's "failing loudly at load time") — never a slot that compiles and
silently never matches.

```
verb READ:
  actors: 1..2 kind:character     # one or two readers
  object: 1 kind:text

verb USE_ON:
  actors: 1..5 kind:character     # a coven may be needed
  object: 1 kind:rite
  with:    0..2 kind:item
```

The per-slot `shown`/`hidden` flag (§6.2) exists in the data model
(`CompiledSlot.shown`, default `shown`); the script line grammar does not
carry it — the flag is reachable only by building `ContentRaw` in code.

Cardinality is the **general** rule only. Exceptions ("two may read *unless*
the book is magick") are recipes, not slots — a rule the UI can apply without
understanding content (grey-out) and a narrative the engine can speak when the
exception bites (REFUSE, below).

### 6.2 Requirement visibility (`shown` / `hidden`)

Requirements surface in three places, each independently controllable:

| Surface            | Controlled by            | Default | Hidden means…                             |
|--------------------|--------------------------|---------|-------------------------------------------|
| Slot badge         | slot `shown`/`hidden`    | shown   | no badge; slot still rejects bad drops    |
| Drop pre-warning   | gate recipe `shown:`     | hidden  | no warning; discovered by doing           |
| Refuse narration   | — always spoken          | —       | committed attempts always narrate         |

Rules:

- A **slot** marked `hidden` accepts and rejects silently — the player pokes
  at it to learn its taste.
- A **gate recipe** marked `shown: true` (with a `hint:` line, §6.4) lets the
  UI *warn before committing* ("Ana lacks occult:2"). Warning ≠ blocking — the
  player may still insist, and the engine, not the UI, has the final word.
- Visibility is **display-only**: it never changes matching, outcomes, or the
  log. The canonical sentence is identical either way.

### 6.3 Condition predicates (recipe `when`)

```
role refs    actors · object · with · at · about · to   (object/at/about/to = first card of the role)
scalar       object.aspect.secret >= 3
             object.aspect.language.latin >= 2
cardinality  count(actors) = 2
quantifiers  every(actors, aspect:occult >= 2) · any(actors, aspect:child)
pairwise     rel(parent_child, actors)                    # ∃ a pair of actors with this relation
cross-role   rel(neighbor, actors.container, object)      # ∃ an edge across two roles;
                                                          # a `X.container` role stands for
                                                          # where the cards of role X sit
world        exists(HUNTER)                               # ∃ an ALIVE card with this exact name
             exists(aspect:hunter>=1)                     # ∃ an alive card matching a §4.5 query
             exists(kind:rite aspect:ritual>=1) · exists(-aspect:winter)
             exists(name:SUSPICION)                       # ∃ a living copy of the name (§4.5)
aggregate    total(actors, aspect:lantern) >= 3 # pooled power of the set
             count(name:SUSPICION) >= 3         # the whole living pool, not a role (§4.5)
paths        total(actors, aspect:lantern) < object.aspect.demand   # value vs value
             count(actors) > object.attr.seats                      # attr paths too
logic        AND · OR · NOT (uppercase; NOT binds tightest)
comparisons  >= · <= · > · < · = · !=
```

Limits: path operands read `role.aspect.key` and `role.attr.key`
(numeric attrs); the scalar conditions inside `every`/`any` read **aspects
only** (`aspect:key >= n`). A missing numeric attr entry on a bound card
compares as 0 (§6.3); a missing aspect on a bound card reads false.

Any character this grammar does not use is a **load error**: the predicate
tokenizer fails naming the character and its offset — the §4.5 terms
embedded in `exists(...)`/`count(...)` ride along verbatim (their `!`
negation included). Whitespace is the only non-token it skips (the glue
rule below), so a `@3`-class typo is visible at load instead of silently
vanishing from the condition.

**`exists(...)` — the world-existential check** (Chorus's answer to Cultist
Simulator's `extantreqs`/`tablereqs`, docs/reverse/cultist-simulator.md):
true iff at least one **alive** card anywhere in the state matches — every
alive entity in the world, not just the cards bound into this step's roles,
and regardless of where they sit. It reads the same input state as every
other condition (recipe-selection time, §6.4) and is an ordinary primary
condition: `NOT exists(...)`, `exists(...) AND ...`, `OR` chains all compose.

- `exists(NAME)` — an alive entity bears exactly this name (names are case
  exact, §5.1; a destroyed card stamps `deadAction`, releases its name, and
  stops existing).
- `exists(<query>)` — an alive entity matches the §4.5 slot query: a
  conjunction of `kind:` / `aspect:` / `state:` / `name:` terms with `-`
  negation.
- Disambiguation: if the first token inside the parens is a query-term
  starter (`kind:` / `aspect:` / `state:` / `name:` / a leading `-`), the
  contents parse as a §4.5 query; otherwise they parse as a single NAME.
  Spaces around a comparison inside a query are glue: `aspect:hunter >= 1`
  reads as `aspect:hunter>=1` (§4.5 terms are whitespace-separated).

**`count(...)` is overloaded the same way** (§4.5): `count(role)` counts the
cards bound into a role; `count(<query>)` counts every ALIVE card in the
world matching a §4.5 query — `count(name:SUSPICION) >= 3` is the pool-size
check. The first token decides, exactly as in `exists(...)`. **Load-time arg
lint**: a `count()`/`total()` argument must be a declared role or a
query-starter — `count(DREAD) >= 3` (the most tempting typo: a NAME where a
role belongs; it would silently count 0) is a load error naming the valid
forms. `total(<query>, …)` is not offered: pooled aspects read a bound role.

Idiom — a rite that only burns while the coast is clear (no HUNTER card
exists anywhere in play) and the sealed door still stands:

```
- id:    rite.unwatched
  when:  NOT exists(aspect:hunter>=1) AND exists(THE_SEALED_DOOR)
  out:   PROGRESS(30m, "The door holds; the street is empty."), SPAWN ...
```

The same grammar also serves the IF conditions of outcome lists (§6.8) —
one predicate language, two disciplines: a `when:` reads an unresolvable
operand as false, an IF condition rejects the step. The boundary,
precisely:

- counts over anything read 0; aggregates read 0; world existentials read
  false — these are branch selection, not failures.
- a missing aspect on a bound card reads false; a missing NUMERIC attr entry
  on a bound card compares as 0 (`attr:x >= 3` fails and `attr:x < 3` holds
  from missing — the SET-delta rule); a present non-numeric entry still
  never compares.
- a card-reading operand over a role with NO bound card is an unresolvable
  ref — false in a `when:`, a step rejection in IF (§9.2, application
  failure).
- a role whose cards all died mid-list reads as absent — silent false in
  both disciplines (the card existed at the clause's re-resolution; it died
  since).
- quantifiers over an empty role keep their §6.3 truth in BOTH disciplines —
  `every` vacuously true, `any` false — so an author who branches on an
  OPTIONAL role guards it first (`IF count(object) >= 1 AND object.aspect.x
  …`).
- `AND`/`OR` evaluate SHORT-CIRCUIT: when the left arm decides, the right is
  never read — a would-be unresolvable-operand rejection on the right arm is
  masked. The guard idiom above works because of this; it is normative, not
  an optimization.
- `rel(...)` over a role with no bound card reads silent false in BOTH
  disciplines (it is not a card-reading operand — the empty pair set finds
  nothing). Single-role pairs skip self-pairs; a cross-role pair MAY pair a
  card with itself when the same card sits in both roles.

### 6.4 Recipes: first match wins, most specific first, **stable ids**

A recipe is an optional stable `id` (used for log provenance; when omitted the
compiler mints `<verb>.recipe<n>` / `<verb>.default`), `when` (or `default`),
`out`, and optional `shown`/`hint`. Data-model shape and script line (§10):

```
recipe      := [id] ["when:" cond | "default"] "out:" outcomes
               ["shown:" true|false] ["hint:" "text"]      (default: shown: false)
script line := [id] ["hidden" | "shown" ["hint" «"text"»]] ["when" cond]
               ["aspects" name {"," name}] ["max" N] "->" outcomes
             | "default" "->" outcomes
               (* the bracketed keywords may sit in any order around each
                  other — `max`, `aspects`, `hidden|shown`, and `hint "…"`
                  anywhere but FIRST (a leading token matching the id class
                  is the id) — the grammar above shows the conventional
                  order; `when` is last before `->`; `default` is its own
                  whole shape — a trailing token after it is a load error
                  *)
```

**Execution limits — `max N`** (N ≥ 1, integer). At selection a recipe whose
execution count is ≥ N is skipped — the walk continues — making `max 1` the
recipe analog of a once-trigger. RUN's candidate walk applies the same gate
(§6.7): an exhausted candidate is simply non-matching. The count lives in
`State.executed` (a recipe-id → count map, cloned like every state field),
maintained by the ONE window-registration path play and fold share: a
registered window increments each clause's commit-time recipe id on the draft
(§9.2, atomic drafts). A table-refused step opens no window and counts
nothing; a window that later refuses still counted (it registered at begin).
`hintonly`/`shown` recipes respect `max` the same way — a spent recipe stops
being suggested. A max change under an old log changes future selection only
(§9.2, no compatibility).

The flagship ladder:

```
verb READ:
  slots:
    actors: 1..2, kind:character
    object: 1, kind:text
  at:       0..1, kind:place
  duration: 1h
  recipes:
    # hard gate — nobody present knows enough; costs NO time.
    # Hidden: the player discovers it by opening the book.
    - id:    read.magick_gate
      when:  object.aspect.magick >= 1 AND NOT any(actors, aspect:occult >= 2)
      shown: false
      out:   REFUSE("The letters swim like minnows.")

    # two readers who are parent & child → different outcome
    - id:    read.parent_child
      when:  count(actors) = 2 AND rel(parent_child, actors)
      out:   SPAWN BONDING_MEMORY, NARRATE "{actors} read aloud; the child sleeps wiser."

    # two readers, unrelated → allowed but poorer
    - id:    read.unrelated_pair
      when:  count(actors) = 2
      out:   NARRATE "{actors} trade the book back and forth and lose the thread."

    # solo study of a secret text
    - id:    read.secret
      when:  object.aspect.secret >= 3
      out:   PROGRESS(30m, "The margins argue with themselves."),
             SPAWN CLUE, NARRATE "{actors} tease out a hidden thread."

    - default:
      out:   NARRATE "{actors} read {object}."
```

And the "capacity is conditional" case — a rite that needs *enough* people,
with its requirement **posted openly**:

```
verb USE_ON:
  slots:
    actors: 1..5, kind:character
    object: 1, kind:rite
    with:    0..2, kind:item
  duration: 3h
  recipes:
    - id:    rite.lantern_gate
      when:  total(actors, aspect:lantern) < object.aspect.demand
      shown: true
      hint:  "The rite wants pooled lantern — it will starve."
      out:   REFUSE("The rite starves for light.")
    - default:
      out:   ...continue...
```

Parameterized spawns are **prototype derive rules**, not call-site sugar. The
prototype `CLUE` is declared `kind: idea, deriving name from object, secret
from object.secret-2`; a recipe that says `SPAWN CLUE` mints
`<PROTO>_<OBJECTNAME>` (e.g. `CLUE_THE_FORGETTING`) with `secret =
object.aspect.secret − 2`. The engine resolves everything at execution and
writes the concrete outcome — name, `INTO`, `AS #id`, materialized
`aspects=`/`attrs=` pairs — into the log (§9.2): the log never
contains sugar. Derive rules exist for aspects as well (`aspects from
object.aspect-N`, alongside `name from object`).

### 6.5 REFUSE semantics (step-atomic)

- The **step is the unit of atomicity**: all clauses' slots are validated and
  recipes selected *before* anything applies; outcomes then apply in clause
  order into a draft. A `REFUSE` **discards the whole draft**: no state
  change, no time passes — including clauses that had already "succeeded".
- The attempt **is still appended to the log** (with the outcomes collected
  up to the REFUSE) — the journal tells the story of failure too.
- `shown` gates warn at drop time; `hidden` gates speak only on commit.
  Same log entry either way — only the UI differs.
- "Pay, then find it impossible" is therefore *authored as a separate gate
  recipe*, not as partial application inside one recipe.

### 6.6 Execution model

1. **Validation**: every clause's slots are checked (roles declared by the
   verb, counts in range, members match queries, no undeclared roles).
   Invalid → rejected at the UI, nothing logged. A step whose start clock is
   before the state's current clock is also rejected outright — mechanics
   time is monotonic (§9.2).
2. **Selection**: for each clause in order, the verb's first recipe whose
   `when` holds fires (or `default`). No recipe → rejection, nothing logged.
3. **Application**: outcomes apply in clause order into a draft; role refs
   re-resolve against the draft before each clause applies, so cards an
   earlier clause destroyed are never applied stale. A `REFUSE` discards the
   whole draft (§6.5); an outcome that *cannot* apply rejects the whole step
   with nothing logged — distinct from `REFUSE`, which always logs its
   attempt (§9.2). "Cannot apply" is per-operator: MOVE's destination,
   RELATE's refs, REPLACE's ref and DRAW's INTO hard-reject (at completion
   the same failures refuse the window instead, §6.7);
   DESTROY/REVEAL over a ref that no longer resolves silently no-op (the
   row still logs its authored args), UNRELATE likewise, SET silently
   no-ops (a delta whose target vanished has no absolute to log); SPAWN's
   INTO mints the card containerless
   rather than materialize it nowhere. Otherwise the draft becomes the new
   state.
4. **Time**: the step itself moves nothing — the clauses'
   durations are the processing WINDOW's length, recorded as the BEGIN row's
   `advancedMin` (§6.7), and the clock advances only through advance
   passes: a bare clock input, an action whose typed clock lies ahead, or a
   WAIT (an authored `ADVANCE` only stretches the pass already in flight) —
   each pass resolves due completions
   first, then the halfhour's slot events, and ends in a WAIT row, the only
   record that moves `now` (§6.7). On replay (fold, §7) only WAIT rows move
   the clock — `startMin` + `advancedMin`; BEGIN, COMPLETE and EVENT rows
   never touch it (§7).
5. **Provenance**: the record stores the fired recipe ids (one per clause,
   clause order) and the canonical sentence.
6. **Events**: each advance pass resolves, in
   chronological order, across the **whole elapsed span** — from the state's
   previous clock to the pass's target, so dead air (§9.2)
   cannot swallow a crossing: authored time triggers (content: id + slot,
   the `hour`/`halfhour` syntax of §10 [+ `once`], firing on every crossing
   of that halfhour-of-day, or on the first crossing only when `once`,
   §6.6) and entity expiries (`expiresAt` crossed — authored `on expire of
   NAME` triggers fire instead when one names the expiring entity, §6.6;
   with no match, trigger id `expire:<NAME>` and one engine-written
   `DESTROY`).
   Within one crossed halfhour, authored triggers fire before expiries. Each
   fires its own EVENT log row with its outcomes applied. One pass — events
   never cascade within the same advance (new expiries
   resolve at the next advance). **The fold trusts logged EVENT rows** — it
   never re-derives them; a once-trigger's fired-ness is a projection
   rebuilt from those rows.

Perishables/decay: an entity may carry `expires:+24h`; the engine
materializes `expiresAt` when the attr is set; the crossing fires an expiry
EVENT — the authored `on expire of NAME` trigger's outcomes if one matches
(decay chains: influence → weaker influence, health → fatigue, summons →
defunct, usually via `REPLACE`), else the built-in destroy. The authored
outcomes replace the built-in destroy ENTIRELY: if they leave the entity
alive, it re-fires on every later crossing — expire it deliberately
(`DESTROY NAME`) or give it a fresh `expires`. First matching trigger in
declaration order wins; there is no compile-time existence check on NAME
(decay targets are often spawned at runtime — a wrong name is an inert
trigger, not a load error). REFUSE in any trigger outcome is a load-time
error (an event has no step to refuse; §6.7); an authored `ADVANCE` there is
a load-time error too — an event owns no advance pass to stretch; a
completion trigger fires inside a pass and keeps ADVANCE (§6.7); and the
fired set is written
by exactly one rule in both paths — one entry per EVENT row's trigger
(authored id or `expire:<NAME>`).

### 6.7 The processing cycle (begin → lock → ticks → complete → unlock)

Committing cards to a verb station is not the whole action: the action
*processes* over the verb's duration, and the engine narrates that
processing. The window is **real**: several stations run in
parallel, and the lock is machinery — the state's open-window table — not a
projection of serialized time.

1. **Begin & lock.** The step validates against the post-advance state
   (§6.6) and a recipe fires per clause. Each clause **begins a window**
   `[t, t + Σdurations]` (minute-precise; multi-clause steps stagger their
   windows). The row logged is the BEGIN row: clauses, the commit-time
   recipe ids, the `PROGRESS` ticks (journal-only), `advancedMin` = the
   window length. For the whole window the verb's station is closed (a
   station holds one window) and every card bound in a committed role —
   object/with/at/about/to; **actors stay free** — is busy: nothing else
   may act on them, or with them, meanwhile.
2. **Ticks.** The fired recipes may schedule `PROGRESS` outcomes — authored
   event descriptions at offsets inside the window (the "read bar"). They
   ride the BEGIN row; a `PROGRESS` authored inside an IF branch defers with
   the branch. An offset counts from the window's start clock; the display
   plots ticks on the window. The log stores them in emission order, so a
   replay re-derives exactly what the player saw and when.
3. **Complete & unlock.** When time reaches the window's end, a COMPLETE
   row applies the recipe's remaining outcomes: `DESTROY`/`SET`/`SPAWN`
   consume, mutate, or mint cards, `NARRATE` closes the scene, and the
   surviving cards unlock back to the table. Bindings re-resolve against
   the world as it stands at completion; refs whose cards died mid-window
   (events do not respect locks) drop from the binding. Clock-anchored
   effects (`SET x, expires, …`) use the window's **start** clock — read
   from the window table via the row's `window` key, the same source play
   used, so the fold never re-derives them from a display clock. A COMPLETE
   row closing an unknown window (log drift, or a bare-row fold probe) is
   tolerated with a tGame-anchored fallback — the real path always finds
   the window.

Selection is a commit-time snapshot: the fired recipe ids ride the BEGIN
row and the completion looks them up in compiled content (drift fails
loud). A **top-level REFUSE** in the selected recipe still rejects at the
table — the gate check runs the outcome list against a scratch draft at
commit, where an `IF` is opaque (its branches, REFUSE included, belong to
the completion). A REFUSE that only decides at completion — inside a
branch — or an outcome that cannot apply, **refuses the window**: the
attempt is logged (the COMPLETE row carries the REFUSE marker, a state
no-op at fold), the partial effects are discarded, and the window closes —
a pass that rolled back would leave the window retrying forever (the
completion analog of §9.2).

**Time is explicit.** Beginning at `now` moves nothing; the clock advances
only through advance passes — a bare clock input, an action whose typed
clock lies ahead, WAIT (an authored `ADVANCE` only stretches the pass in
flight) — and each pass resolves due completions
first (ordered by end minute, then begin row, then clause), then the
halfhour's slot events (§6.6), completions at minute m preceding slot-m
events. The pass ends in a WAIT row, the only record that moves `now`,
whose spans partition time (the fold asserts the partition: a wait row not
starting exactly at `now` is a loud log-drift error, §7). A bare advance to
the current minute with nothing due is rejected outright (`nothing to do —
the clock already reads d:h (commit an action, or name a later minute to
wait)`) rather than idling — idling is what `wait`
(§11) is for. One pass is atomic (§9.2); one step may advance the clock at most SEVEN DAYS — the pass walks every halfhour it crosses (recurring triggers fire on each), so a giant day field is a loud rejection ("more than 7 days ahead"), not an unbounded walk. Expiries created
by the pass resolve at the next one.

**Why there is no lock-flag machinery beyond the windows table**: the
table *is* the lock — play and fold rebuild it identically from BEGIN and
COMPLETE rows (§7), and every busy check is a lookup in it. Concurrent
verb stations are the model.

`PROGRESS` belongs to verb recipes only — the script compiler rejects it in
trigger outcomes (events have no window). `REFUSE` belongs to verb recipes
too, for the same reason sharpened: a refusal aborts a *step* or a window,
and an event has neither — so `REFUSE` in any trigger outcome (all three kinds)
is a load-time error (§6.6). `RUN` belongs to verb recipes as well: authored
in a slot/expire trigger's outcomes it is a load-time error — an event has
no window to chain from (`on complete` bodies fire inside one, so they are
the exempt kind, above) — and so does `ADVANCE`, which can only stretch the
pass a completion rides: in the two event kinds it is a load error, in an
`on complete` body it extends the pass.

`PROGRESS(duration, string)` is journal-only, like `NARRATE`: no state
effect, no time effect, raw template stored and interpolated at render
(§5.3). Offsets are authored discipline — a tick past the window is harmless
but meaningless. A cadence ("every hour") is simply several authored ticks;
a real-time UI may schedule their display however it likes.

**RUN chains.** `RUN "recipe-id", …` (§4.4) chains a completion into
follow-up windows — the mechanics:

- *Candidate walk.* At the carrying window's completion — after the recipe's
  own outcomes — RUN walks its candidates in order and pins the first whose
  `when:` matches LENIENT (§6.4's selection discipline) and whose `max`
  (§6.4) is unspent, then opens ONE follow-up window on that recipe's verb
  at the completion clock, with the parent's committed role refs PROJECTED
  onto the candidate verb's declared roles (a role the candidate verb does
  not declare is dropped, so the synthesized clause keeps the shape a player
  step could produce; a card bound only in a dropped role is carried by no
  window and so never locks). The projection happens at synthesis only: the
  parent window's own refs and locks are untouched.
- *Merged gate bindings.* The candidate `when:` evaluates against the
  post-outcome draft AND the parent window's committed role bindings,
  MERGED with the projected clause's own: where the candidate verb declares
  the role, the projected binding wins (it is what the follow-up will
  carry); where the projection drops the role, the parent binding stands —
  a cross-verb gate reads the parent situation (CS's linked semantics), and
  a dropped role is GATE-VISIBLE ONLY: the follow-up never carries it, an
  outcome over that unbound role expands to no targets and silently no-ops,
  and the dropped cards lock nothing. Same-verb chains project identity —
  but the declared-role bindings change source: they come from the projected
  clause re-resolved against the post-outcome draft, so a same-verb gate
  whose bound card the parent's own outcomes replaced or killed reads the
  follow-up's truth. The merge is eval-context only: the BEGIN row carries
  the projected clause alone. No candidate matches: nothing opens — the
  chain ends, silently (CS's chain ending, not a refusal). Under content
  drift an unresolvable candidate silently continues the walk.
- *The guard survives.* The pinned recipe's top-level REFUSE still gates,
  inline against the draft at the parent's completion — a scratch trial
  (PROGRESS skipped, scratch rng/id consumption discarded) whose refusal
  rejects the PARENT window (the window-refuse above: marker on the COMPLETE
  row, partial effects discarded).
- *Zero-concrete.* RUN is the first op that logs nothing of itself — the
  follow-up's BEGIN row is the record. The fire result grows an
  extra-records slot; the follow-up window is inserted after the parent
  window leaves the open table, registered through the ONE registration
  path play and fold share (the insert and the fold's BEGIN replay cannot
  drift), and the pass loop picks the new due window up on its next
  iteration. Content that drops a RUN stops producing follow-up BEGINs in
  future play; old logs replay untouched.
- *Livelock guard.* Cycles are legal (needs → discardseason → drawseason →
  needs must stay legal), but a cycle of zero-duration follow-ups would
  generate unbounded due work inside one pass: an advance pass that
  completes more than 1024 windows fails loud — a documented guard with a
  pinned test.
- *Load checks.* In a recipe body every candidate must name a recipe of the
  PARENT verb (unknown or cross-verb id: load error, IF branches included).
  In an `on complete` body (below) candidates may cross verbs, which
  requires GLOBAL recipe-id uniqueness: a cross-verb resolution consults a
  global id table, and a collision across verbs is a load error.
- *Chance is authored around it.* Candidates with identical requirements
  author `ROLL` + `IF` in the parent — one RUN per arm (the list walk would
  always pin the first). A second RUN op reached in one completion rejects
  the window (a station holds one window); the list is the only
  multi-candidate shape, and deeper chains author RUN inside the follow-up's
  own outcomes.
- *Out of slice (reserved)*: CS `linked`-entry `challenges` (authorable
  today as ROLL + IF) and the 8 inline recipe definitions (compile to
  ordinary declared recipes); `additional`/`expulsion` never occur in core
  linked entries.

**Completion triggers — `on complete`.** A third trigger kind, carried by an
entity or prototype line (ID-less, beside the section-scoped `triggers:`
forms; §10 for the line) — CS's xtriggers and induces:

    on complete [<aspect>] -> ( outcomes )

gated by the recipe-line token `aspects <name> [, <name>]*` (a presence
list, level 1). When a window completes — AFTER the recipe's own outcomes
and BEFORE any RUN — each card bound in the window's committed locked roles
(the same set §6.7 makes busy, the player-dropped station card included;
deduped — a card bound through two roles fires once; fired in the lock
list's order; actors stay free) fires its `on complete` triggers: the bare
form fires on every completion binding the card; the aspect form fires iff
the completed recipe declares that aspect. Outcomes apply on the draft; a
downstream rejection discards the whole completion (§9.2, atomic drafts).

Deliberately unlike the event triggers: completion triggers produce NO EVENT
row and never touch the fired set — their outcomes log as concrete rows
appended to the COMPLETE row's outcome list in fire order, and the fold
replays them without recomputing gates (play and fold share the one firing
function). Their bodies see the WINDOW's re-resolved bindings — the
empty-bindings rule of slot/expire triggers does NOT apply, and the
role-free load lint does not run on them. A failure inside the body refuses
the WINDOW (not a whole-step throw); `REFUSE` in the body is a load error.
RUN inside the body is legal and may cross verbs (the global id table); the
busy check for such a follow-up EXCLUDES the parent's own window — otherwise
a same-verb hand-off would see its own station busy and silently skip
forever (any OTHER busy station skips the candidate silently). This is what
makes CS's `induces job→needs` expressible: a work completion containing the
job card hands off to the time verb — a one-shot bootstrap, after which the
resident self-rechaining needs cycle keeps the time station busy and later
induces correctly suppress. The gate's emitter is authored: an aspect with
no declaring recipe never fires. `aspects` is content, not state — it never
enters the log; content drift changes which triggers fire in future play
only (§9.2, no compatibility). `ROLL` inside consumes counters normally
(§4.4); `ENDING` inside ends the story (first wins, §4.4).

### 6.8 Conditionals in outcome lists (IF / ELSE IF / ELSE)

An outcome list may branch:

```
-> PROGRESS(30m, "You work the crowd."),
   IF count(name:SUSPICION) >= 3 -> (NARRATE "Too many eyes.", SPAWN PARANOIA)
   ELSE IF exists(THE_BRAZEN_COP) -> (NARRATE "The cop watches.")
   ELSE -> (NARRATE "Nobody notices.")
```

Grammar (§5.1): `IF cond -> ( outcomes )` followed by zero or more
`ELSE IF` arms and at most one terminal `ELSE -> ( outcomes )`. Branch
bodies **require parens**. The ELSE chain is consumed **greedily by the
nearest IF**, so `IF a -> ( IF b -> (X) ) ELSE -> (Y)` gives the ELSE to the
outer IF, while an ELSE inside the parens belongs to the inner one. A bare
`ELSE` with no IF, a dangling `ELSE IF`, a condition without its
`-> ( … )`, and unparenthesized bodies are parse errors. `IF` and `ELSE`
are reserved words (§5.1); quoting is the escape hatch for names.

**Conditions** use the full §6.3 grammar — role operands, `count`/`total`,
quantifiers, `rel`, the world existentials, AND/OR/NOT. They are parsed from
the sentence token stream as a balanced unit up to the depth-0 `->`
(predicates carry commas inside `rel()`/`every()`, so a comma never splits
the chain) — the same grammar functions as the string-parsed `when:`
(§6.3).

**Evaluation — in order, against the DRAFT.** Arms test in source order;
the first true condition wins; with no true arm, `ELSE` applies; with no
true arm and no `ELSE`, nothing applies and the step proceeds — no error.
Unlike recipe selection, which reads the input state (§6.4), IF conditions
read the draft as it stands at the IF's turn, so earlier outcomes in the
same list are visible to later IFs. Normative:

- A recipe selected by `when count(name:S) < 3` fires with the pool at two,
  and its own `SPAWN SUSPICION, IF count(name:SUSPICION) >= 3 -> (ENDING
  ARRESTED)` sees the spawn it just made — the same step that spawned the
  third copy arrests (§6.3).
- An IF after `DESTROY OBJECT` in the same list reads a dead-stamped entity:
  the card reads as absent, so a comparison over it is false — silently
  (draft reads death; see §6.3 for the exact boundary vs. the rejection
  rule).

**Concretization — the taken branch only.** The log records the taken
branch's outcomes, flattened into the IF's position — the engine computes
once at execution and logs the result (§4.4). The record never contains an IF op; the untaken branches are
authored discipline the log cannot recover (the §5.3 journal-loss
corollary). A `REFUSE` inside a taken branch refuses the whole step as
usual (§6.5).

**Triggers.** Both trigger forms (§10) may author IF, with the WORLD forms
of §6.3 — `exists(...)`, `exists(<query>)`, `count(<query>)` — since event
bindings are empty. A role-dependent operand in a trigger IF condition
(`count(role)`, `total(role,…)`, `role.aspect.x`, `role.attr.x`,
`rel(…role…)`, `every`/`any(role, …)`) is a **load-time error**: it would
silently read false/0 there (§6.3). The trigger PROGRESS/REFUSE guards
(§6.7, §6.6) and the endings existence check (§4.4) recurse into branch
bodies — nothing escapes by nesting.

---

## 7. Database schema (append-only log; projections derived)

The log is the source of truth. `entity`/`relation` are projections built by
fold (§6.6); a SQL shape is given for when a queryable store is wanted.

```sql
CREATE TABLE entity (
  id           TEXT PRIMARY KEY,      -- "s_ana", "e1_1"
  kind         TEXT NOT NULL,         -- dictionary KIND
  name         TEXT NOT NULL,         -- unique among alive entities
  aspects      JSON,                  -- {"secret":4,"language":{"latin":2}}
  attrs        JSON,                  -- {"container":"house","state.sealed":true,"expiresAt":790}
  born_action  INTEGER,
  dead_action  INTEGER
);

CREATE TABLE relation (
  a           TEXT NOT NULL,          -- entity id (sorted a <= b when symmetric)
  b           TEXT NOT NULL,
  type        TEXT NOT NULL,          -- dictionary relation type
  value       INTEGER DEFAULT 1,
  born_action INTEGER,
  dead_action INTEGER
);

CREATE TABLE action (
  id           INTEGER PRIMARY KEY,
  kind         TEXT NOT NULL DEFAULT 'action',  -- 'action' (BEGIN) | 'complete' | 'wait' | 'event' — the record model (§6.7)
  trigger      TEXT,                            -- events: the trigger id (an authored `on expire` trigger's id, else 'expire:<NAME>');
                                                -- COMPLETE rows: the window's VERB (raw name token, never a ref)
  t_game       TEXT NOT NULL,                   -- display clock "day:halfhour", day 0-based — floored, DISPLAY ONLY
                                                --   (exact minutes ride `start_min` on BEGIN/wait rows; a COMPLETE row
                                                --   reaches its window start through the `window` key)
  t_real       TEXT,
  advanced_min INTEGER NOT NULL DEFAULT 0,      -- BEGIN: the window length (moves NOTHING; 0 on a refused attempt) · wait: the span (the only clock mover) · else 0
  start_min    INTEGER,                         -- exact minutes: BEGIN = begin minute, wait = span start — the fold
                                                --   lands them exactly (expires anchoring); absent on complete/event rows
  window       TEXT,                            -- COMPLETE rows: the key of the window they close
  actors       JSON,                            -- refs: sorted, de-duplicated union across clauses (order not semantic)
  verb         TEXT NOT NULL,                   -- FIRST clause's verb lexeme ('' for event/wait rows; the verb of a COMPLETE row)
  objects      JSON,                            -- FIRST clause's object/with/at/about roles (multi-clause steps
  instruments  JSON,                            --   keep per-clause detail in `sentence` — it is lossless)
  at           TEXT,
  about        TEXT,
  to           TEXT,                            -- FIRST clause's `to` role (empty for event rows)
  outcomes     JSON,                            -- concrete outcomes (ids minted, sugar resolved, roles concretized)
  recipes      JSON,                            -- BEGIN: fired recipe ids, clause order; COMPLETE: the one recipe
  sentence     TEXT NOT NULL                    -- canonical form, lossless
);
```

- **State = fold(log)** → free replay, free provenance (`born_action`),
  potential undo (truncate to the previous turn boundary, re-fold the
  surviving prefix — §11). The fold replays each record's concrete
  outcomes; refused records are state no-ops; the clock is owned by WAIT
  records alone (`start_min` + `advanced_min`, §6.7) and the fold asserts
  the wait spans partition time — a wait row not starting exactly at
  `now` is a loud log-drift error. A once-trigger's
  fired-ness is likewise a projection: the fold marks it from each EVENT
  row's `trigger` (§6.6).
- **State beyond the tables** — the fold-maintained fields, each written by
  the one path shared by play and fold (§9.2): the open-windows table (the
  lock, §6.7), `byName` (each name → its living ids in BIRTH order — the
  pool arrays; a name ref under a pool addresses the head, the OLDEST copy,
  §4.2), the `rng` counter (§4.4), one cursor per deck (§4.4), the
  executed-recipe map (§6.4), and the fired set (§6.6). A discarded draft
  deep-copies all of them, pool arrays included (§9.2, atomic drafts).
- **The log is a working file, not a stable contract** (§9.2): it replays
  under *this* spec version only. When a change breaks old logs, re-author
  or migrate them by hand.
- **Journal view** = the `sentence` column in order — the game *is* a book.
- The shipped store is JSONL — one record per line (`save_log`/`load_log`); the
  SQL shape above is the queryable target when one is wanted. The wire
  shapes are frozen: record fields are camelCase (`tGame`, `tReal`,
  `advancedMin`, `startMin`, `window`); outcome args are `t`-tagged objects —
  the log carries `ref` `int` `str` `bool` `dur` `key` `into` `asId` `pair`
  (role refs are concretized to `#id`s and SET logs the computed absolute
  `int`, so the authored-only `delta`/`role`/`expires`/`vexpr`/`param`
  shapes never reach the log), a `pair`'s value a JSON string/number/boolean,
  an `into`'s value the bare Ref object or `{ role }`; refs carry no `t`
  wrapper — they are tagged by their `form` key; a param placeholder's
  `sign` rides as the JSON `1`/`-1`, omitted when
  absent; map keys serialize through §4.2's canonical order (integer-like
  keys front).

---

## 8. Rendering sketches (text now, drawable later)

Card (every field is from the dictionary — nothing ad hoc). Shipped shape:

```
┌────────────────────────────────────┐
│ THE_PALE_LADY                      │
│ text · secret:3 · language.latin:2 │
└────────────────────────────────────┘
```

Verb station (requirement badges per §6.2 — shown gates print their hint,
hidden gates print nothing). Still a sketch: nothing renders stations.

```
▶ USE_ON  [ actors: 1..5 characters ]  [ object: a rite ]  [ with: 0..2 items ]
    ⚠ The rite wants pooled lantern — it will starve.        (shown gate)

▶ READ    [ actors: 1..2 characters ]  [ object: a text ]   (takes 1h)
                                                          (magick gate: hidden)
```

Processing (§6.7) — a committed action shows its window and ticks; live or
batched at completion is the UI's choice:

```
▶ USE_ON  ▰▰▰▱▱  1h10m of 2h   [ MARCH · THE_INQUEST · 3 ideas ]
    (+30m) Brack convenes the van; the company seal goes on the door.
    (+1h)  Motive, means, opportunity: three cards on the tally-board.
```

Journal: canonical sentences, or the prose templates — same data, two views
(the journal and story views in the player interface, §11). The cards view
lists the table, the look view shows the player's container and contents.

### 8.1 The card mapping (every engine operation is a card operation)

The text engine is not a stopgap *instead of* cards; it is the card game with
the cards drawn as text. The mapping is total and runs operation by operation:
nothing a card UI does lacks a sentence, and nothing in the grammar lacks a
card reading. A drawn UI (§1.5) would be a new renderer over the same
machinery, not a new rulebook.

| Text engine operation | Card operation |
|-----------------------|----------------|
| an entity (§7) | a card; its face is name · kind · aspects · `state.*` flags (the card sketch in §8) |
| a verb with its slot declarations (§6.1) | a verb station; each slot is one clause role (§4.3); cardinality = how many cards fit |
| a slot query (§4.5) | the grey-out rule: a card that does not match cannot drop — decidable from slots alone |
| filling a clause's roles (§5.1) | dropping cards into the station's slots — distinct cards per role; `actors` is one slot that takes several (§4.3) |
| committing a step: the engine composes and logs the canonical sentence (§9.1) | pressing play on a filled station — placing the cards *is* writing the sentence; several clauses are several stations committed as one step (`;`, §5.2) |
| slot `shown`/`hidden`, gate `shown:` + `hint:` (§6.2) | badge visibility and the drop pre-warning — display-only, never touches matching or the log |
| the processing window (§6.7; authored `ADVANCE` stretches it) | the station and its committed cards lock while "working"; `PROGRESS` ticks are the read bar |
| the outcomes at unlock (§4.4 — applied and narrated at the window's close, §6.7) | the cards' fates: `DESTROY` consumed · `SPAWN` minted onto the table (`INTO` = which tray) · `MOVE` slides to another container · `SET` repaints the face · `REVEAL` turns a hidden card face-up · `RELATE`/`UNRELATE` re-lay the table (§4.2) · `NARRATE`/`PROGRESS` touch no card |
| `REFUSE` (§6.5) | the committed cards bounce back, no time passes, and the attempt is still journaled |
| a step rejected before logging — bad slots, no matching recipe, unresolvable outcome, clock rewind (§6.6, §9.2) | the station refuses the drop outright: the table is untouched and nothing is journaled — distinct from `REFUSE`, which journals the attempt |
| an EVENT row (§5.1, §6.6) — clock-fired, no station | cards arriving and leaving on their own: an expiry consumes a card, a trigger's outcomes mint, mutate, or consume cards — journaled like any other record |
| the table projection (`Game::visible()`, §11) | what the player actually sees on the table: `container` = which tray/pile a card sits in, zones = groupings with no card (§10), N/S/E/W relations = where place cards sit (§4.2) |
| undo (§11) | a take-back: truncate the log to the previous turn boundary and re-fold — every card returns exactly where it was (§7) |
| `born_action` (§7) | a card's provenance — "entered play at action N" |

Two invariants keep the mapping safe:

- **Cards never mutate anything.** The card layer is L3, a projection over
  (state, log) (§3, §7): a drop event would call the same `execute_step`
  `handle_line` (§11) calls, and the canonical sentence — not the drop — is
  what is stored. There is no second, card-level rulebook to keep in sync.
- **The mapping is display-independent.** Text cards, drawn cards, or both at
  once are projections of the one log; the requirement-visibility (§6.2) and
  table-projection (§11) rules are display-only and change no matching,
  outcome, or record. The journal is the whole table session, replayable (§7).

---

## 9. Design decisions

### 9.1 Decided with the designer (v0.1)

1. **Player input**: UI slots — the player places cards into verb slots
   (Cultist Simulator style); the **engine composes the canonical sentence**
   and appends it to the log. No natural-language parsing. The lenient play
   interface of §11 stands in for the slot UI under the same invariant: a
   front door onto the grammar; the logged sentence is always canonical.
   §8.1 is the operation-level card mapping.
2. **Time model**: discrete, per-action. No background ticking. (The
   unattended pump remains open work — §12.)
3. **Outcomes**: hand-authored recipes only. Deterministic and testable.
4. **Prose layer**: afterthought. Canonical sentences are the deliverable.
5. **Timestamps**: `day:halfhour` (48 halfhours of 30 minutes per day, day 0
   based) — raw monotonic mechanics time; human framing is L3's business.
6. **Multiple actors**: the subject is an unordered set (`A, B TALK …` ≡
   `B, A TALK …`), stored sorted — action equality is structural. Clauses
   are separated by `;` so `,` stays free for lists.
7. **Entity properties** = aspects (weighted, dot sub-keys) + attrs +
   relations; knowledge is just aspects (`occult:3`), and relations are
   typed edges mutated only by RELATE/UNRELATE.
8. **Cardinality vs exceptions**: slots declare the general count rule;
   conditional exceptions are REFUSE recipes. A refusal costs no time but
   is logged — failure is part of the story.
9. **Requirement visibility**: `shown`/`hidden` on slots and gate recipes,
   display-only (never affects matching or the log); slots shown, gates
   hidden by default.

### 9.2 Invariants

Cross-cutting rules every section assumes. Code comments written before the
v0.6 rewrite cite the old per-ruling numbers (9.10–9.50); §9.3 maps each
to its current home.

1. **The log replays literals.** Concrete outcomes are fully materialized:
   role refs are concretized to `#id`s, and SPAWN carries its engine-written
   `INTO`, `AS #id`, and `aspects=`/`attrs=` pairs; SET logs the computed
   absolute, never the delta; the authored `expires` modifier never reaches
   the log — its `expires`/`expiresAt` pair lands inside the concrete row's
   `attrs=` materialization. The fold replays from the record alone, never
   the step context.
2. **An application failure rejects the whole step.** An outcome that
   cannot apply — a duplicate living name, bad op args, a non-duration
   `expires` value, a draw past exhaustion, an unresolvable MOVE
   destination / RELATE or REPLACE ref / DRAW INTO — rejects with nothing
   logged and the clock unmoved; at completion the same hard failures
   refuse the window instead (§6.7). The lenient corner, deliberate:
   DESTROY/REVEAL over a ref that no longer resolves silently no-op
   (still logging their authored args), UNRELATE likewise, SET silently
   no-ops (a delta whose target vanished has no absolute to log), and
   SPAWN's INTO mints containerless rather
   than materialize the card nowhere — DRAW's INTO is the loud twin (§6.6).
   Distinct from REFUSE (§6.5),
   which always logs its attempt.
3. **Steps are atomic drafts.** A REFUSE — or any rejection — discards the
   draft whole: no partial application; draft state is deep-copied (pool
   arrays included); counters, cursors, and consumptions made on the draft
   never move the real state.
4. **One clock — monotonic, wait-owned.** The clock moves only through
   advance passes, each written as a WAIT row (§6.7); a step whose typed
   clock precedes `state.now` is rejected, nothing logged; the fold sets
   the clock from WAIT records only — BEGIN, COMPLETE and EVENT rows never
   advance or rebase it. Mechanics time is minute-precise — a mid-halfhour
   begin is representable (the `d:h` clock field's quantization is input
   syntax, not mechanics; only the `t_game` display clock floors to the
   halfhour) — and dead air is real time: event resolution spans the whole
   elapsed span, so a tick's trigger fires even when no action window
   covers that instant.
5. **The fold trusts the log.** It never re-derives a decision: EVENT rows,
   the fired set, the rng counter, deck cursors, and the executed-recipe
   map are each written by exactly one rule, shared by play and fold, so
   the two can never drift.
6. **No backward compatibility, deliberately.** Grammar, log and script
   formats, and API carry no stability guarantee across versions — no
   migration shims, no version detection; re-author or migrate old files by
   hand. The architecture is not frozen either: it may change when there is
   need, but the spec is amended in the same change.
7. **Determinism.** Hand-authored recipes are the only mover of state; the
   only randomness is §4.4's seeded counter (splitmix64 computed in exact
   integer arithmetic — no float64 ever sees a die); actors are
   stored sorted, so
   sentence equality is structural.
8. **One home each.** Structural flags are attrs; aspects are principles
   (`state:` queries read attrs). Slot queries are authored strings —
   never serialized, never logged — so the §5.2 round-trip does not apply
   to them.

### 9.3 Where the rulings went

The v0.2–v0.3.6 implementation rulings were folded into their topical
sections in the v0.6 rewrite; the text that was pure duplication is gone.
Old numbers survive in code comments until the audit retires them — the
mapping:

| old | now |
|---|---|
| 9.1–9.9 | §9.1 (unchanged) |
| 9.10 | §5.1 (pre-verbal AT) |
| 9.11 | §4.3 (USE_ON signature) |
| 9.12 | §9.2 (one clock) |
| 9.13 | §9.2 (atomic drafts) |
| 9.14 | §5.1 (names, refs, reserved words) |
| 9.15 | §9.2 (log replays literals) |
| 9.16 | §6.6 (events) |
| 9.17 | §9.2 (one home each) |
| 9.18 | §9.2 (log replays literals) |
| 9.19 | §9.2 (application failure) |
| 9.20 | §9.2 (one clock) |
| 9.21 | §7 (fold clock ownership) |
| 9.22 | §6.4 (recipe ids) |
| 9.23 | §6.4 (derive rules) |
| 9.24 | §4.3 (`to` role; also §12) |
| 9.25 | §4.4 (PROGRESS row) |
| 9.26 | §6.7 (no engine locks) |
| 9.27 | §5.3 (`{to}`/`{with}`) |
| 9.28 | §9.2 (one clock) |
| 9.29 | §6.6 (once triggers) |
| 9.30 | §9.2 (no compatibility) |
| 9.31 | §4.4 (SET row) |
| 9.32 | §6.6 (expiry triggers) |
| 9.33 | §4.4/§6.6 (REPLACE, decayTo) |
| 9.34 | §6.6 (trigger guards, fired-set) |
| 9.35 | §4.4/§10/§11 (endings) |
| 9.36 | §4.2 (expires forms) |
| 9.37 | §4.3 (twelve verbs) |
| 9.38 | §4.2 (pools), §5.1/§7 (uniqueness, byName), §10/§11 (asymmetry, surface) |
| 9.39 | §6.3/§6.8 (condition boundary, conditionals) |
| 9.40 | §4.4/§10 (functions) |
| 9.41 | §4.4/§7 (dice) |
| 9.42 | §4.4 (roll binding) |
| 9.43 | §4.4/§10 (decks) |
| 9.44 | §10 (deck livelock rule) |
| 9.45 | §5.1 (compound durations) |
| 9.46 | reserved, never backfilled (expires anchoring: the §4.4 SPAWN row, §6.7 start clock) |
| 9.47 | §4.4/§6.7 (RUN chains) |
| 9.48 | §6.7/§10 (on complete, aspects) |
| 9.49 | §4.5 (query alternation) |
| 9.50 | §6.4/§10 (max) |
| closing note | §12 (the pump) |
## 10. The script format — a game as one text file

A whole game is one text file: meta lines, then section blocks of
sentence-like declarations. `#` starts a comment; blank lines are ignored.
`compile_script` parses and compiles it once, failing loudly at load time
(unknown kind, unregistered relation type, a reserved word in a verb header,
a name shared by a verb and an entity/prototype, bad grammar, duplicate
declaration). Meta lines:
`script:` (title), `player:` (the lenient input's default actor — must name
a declared entity of kind `character`), `goal:` (carried in the meta for an
interface to show; the shipped REPL does not print it),
`start: d:h` (initial clock), and `seed: N` — the §4.4 dice seed, an
integer, default 0; a negative seed enters the §4.4 key as its low 64 bits,
and integer literals saturate at the 64-bit bounds rather than erroring
(fields with small ranges — `hour`, halfhour, `max` — then reject on the
saturated value).

```
script: Murder on the Nightjar Flyer   # title
player: MARCH                          # default actor for lenient input (§11)
goal: unmask Lord Vane's killer …      # carried in the meta for the interface (§11: the shipped REPL does not print it)
start: 0:42                            # initial clock
seed: 7                                # dice seed (§4.4, default 0)

kinds:                                 # new noun classes (§4.1)
  omen                                 # one lower_snake name per indent-2 line

entities:
  GUARD_VAN is place in NIGHTJAR_FLYER with site.guard:1
  # `in X` names the parent place card when one is declared (containment);
  # with no card X it tags a zone — a grouping (the train) with no card.
  MARCH is character at GUARD_VAN
  THE_MANIFEST is text with manifest:1 at MARCH   # a card carried by a character
  GUARD_VAN is south of LUGGAGE_VAN               # direction: places sit on a
  DINING_CAR is south of LUGGAGE_VAN              # table, laid out by N/S/E/W (§4.2)

prototypes:
  CLUE is idea deriving name from object, secret from object.secret-2

triggers:
  dusk_falls at hour 16 -> SPAWN DREAD, NARRATE "The lamps come on."
  body_found once at hour 22 -> DESTROY LORD_VANE, NARRATE "A cry from the first cabin."

functions:
  sus capital -> SPAWN SUSPICION, NARRATE "The conductor's eyes linger on {capital}."
  watch(n) -> IF count(name:SUSPICION) >= n -> (ENDING ARRESTED)

ops:                                   # script-defined outcome operators (§10):
  ENCHANT(who) -> SET(who, aspect:charmed, 1), NARRATE "{who} glows."   # callable like a built-in op

decks:
  RUMOURS spec RUMOUR, RUMOUR, HEARSAY, SCANDAL defaultcard RUMOUR reset

endings:
  ARRESTED: "Taken by the constabulary"
  DESPAIR_DEATH: "No More" (melancholy)
  LANTERN_VICTORY: "The Light in the Ruins" (grand)

verbs:                                 # configures a verb — or declares a new one (§4.3)
  READ:
    actors: 1..2 kind:character
    object: 1 kind:text
    at: 0..1 kind:place
    duration: 1h
    recipes:
      read.magick_gate hidden when object.aspect.magick >= 1 AND NOT any(actors, aspect:occult >= 2) -> REFUSE("The letters swim like minnows.")
      default -> NARRATE "{actors} read {object}."

templates:
  READ says "{actors} read {object} at {at}."
```

Rules:

- **Kind lines**: one `lower_snake` name per indent-2 line under
  `kinds:` — new noun classes (§4.1) registered into the load's dictionary,
  section order = suggestion order. A duplicate (re-declaring a built-in
  included), a relation-type collision, or a name outside the class is a
  load error naming the line. A kind is visible to the lines after its
  declaration only — use before `kinds:` is the ordinary unknown-kind error
  (the same declare-then-use order containers already follow). The engine
  never special-cases a declared kind; the four built-in semantic roles
  (§4.1) stay built-in-only.
- **Entity line**: `NAME is KIND [stackable] [with aspect:level, …]
  [at|in PLACE]` (trailing `with`/`at`/`in` clauses in either order; the
  `stackable` keyword sits before them, §4.2). Script ids are `s_<name>` —
  the name lowercased, so two names differing only in case mint the same id
  (`ANA`/`ana` → `s_ana`); since names are case-exact (§5.1), the id
  collision is its own load error, naming both names. Containers resolve by
  name after all entities are read: an unresolvable `at`/`in` on a
  NON-place entity is a load error, while on a `place` it silently tags a
  `zone` — the zone fallback applies to `at` exactly as to `in`. A name
  declared BOTH as an entity and as a prototype is a load error (spawns of
  it could never mint). Names must be
  unique among the living (§5.1) — except a `stackable` entity line, whose
  name is a pool (§4.2) — and are written as-is — prefer `UPPER_SNAKE` (a
  name outside the name-lexeme class is legal but forces quoting on every
  ref to it; the lint flags it, §13.2).
- **Duplicate declarations are load errors in every section** — kinds,
  entities, prototypes, templates, verbs, decks, endings, functions alike
  (the functions/decks rule generalized): a second declaration of the same
  key fails at load naming the key, it never silently overwrites. (Verbs: a
  repeated verb header, not a body line, is the duplicate.)
- **Relation line**: `A is RELTYPE of B`, `RELTYPE` registered in the
  dictionary, both endpoints declared entities (either missing is a load
  error); symmetric/directed storage follows its definition (§4.2).
- **Verb declarations**: an indent-2 `VERB:` header declares a
  brand-new UPPER_SNAKE lexeme or configures a registered one (§4.3, §6.1);
  the load registers the unknown ones into its effective dictionary,
  register-if-unknown, so re-declaring a core verb just configures it. The
  block under the header is what makes the verb playable — a header with no
  block compiles an empty verb no recipe can match, and a lexeme the content
  never declares rejects at the step. A lexeme that is a reserved word, or
  that shares a name with a declared entity or prototype (either section
  order), is a load error (§5.1); a repeated header is the uniform duplicate
  load error, core lexemes included.
- **Recipe lines**: §6.4's script form — `[id] [hidden|shown [hint "…"]]
  [when COND] [aspects NAME[, NAME…]] [max N] -> OUTCOMES`, or
  `default -> OUTCOMES`. A leading token is an id only when it matches the
  id class and is not one of `when`, `hidden`, `shown`, `max`, `aspects` —
  so a leading `hint "…"` parses as the id and dies; the bracketed keywords
  otherwise sit in any order around each other, `when` last before `->`,
  and `hint "…"` may appear with or without
  `shown`/`hidden`. `default` is its own whole shape — first token and
  nothing else: a trailing token (`default max 2`, `default when …`) is a
  load error naming it.
  `max N` (N ≥ 1) caps executions (§6.4); `aspects` lists the
  presence-emitted aspects that gate `on complete` triggers (§6.7).
- **Trigger lines** — two section-scoped forms plus one card-carried kind:
  - `ID [once] at (hour|halfhour) N -> OUTCOMES` — `hour` N is 0..23
    (slot 2N), `halfhour` N is 0..47. Fires on every crossing of that
    halfhour-of-day — or on the first crossing only with `once` (§6.6).
  - `ID on expire of NAME -> OUTCOMES` — fires when an entity named NAME
    expires (§6.6); NAME gets no compile-time existence check (the entity
    may be spawned at runtime), and its outcomes may reference NAME directly.
    Under a pool, a crossing of ANY copy fires the trigger, and its
    name-addressed outcomes hit the OLDEST copy (§4.2) — the crossing copy
    is not consumed by its own crossing and re-fires on later crossings;
    with no such trigger, expiry destroys the crossing card by id (§6.6).
  - `on complete [<aspect>] -> ( outcomes )` — the ID-less, card-carried
    kind, written on an entity or prototype line: one clause after the
    entity's `with`/`at`/`in` tail, or several clauses across the line and
    later standalone `on complete` lines — every clause attaches to the
    most recently declared line of its own section (a standalone with no
    home
    before it is a load error), and they fire in declaration order; the
    engine mints ids `complete:<home>`, then `complete:<home>#n`. The
    aspect token may be a dotted key, rejects a trailing dot, and may not
    collide with the clause keywords. Fires when a processing
    window completes binding the card (§6.7), optionally only for
    completions of recipes declaring `aspect`.
  `REFUSE` in any trigger outcome of any kind is a load-time error (§6.6),
  and so are `RUN` and `ADVANCE` in the two event kinds (an event has no
  window to chain from and no pass to stretch, §6.6/§6.7; `on complete`
  bodies fire inside one and are the exempt kind).
- **Function lines**: `NAME(params…) -> OUTCOMES` — parametric
  outcome macros. Params are comma-separated identifiers in parens, bare
  space-separated identifiers without them, or absent (an empty `()` is the
  same as none). A body may call other functions, use `IF`, and interpolate
  `{param}` in its strings; the call sites — in recipes or triggers — write
  `NAME(arg, …)` and expand at load time. Functions may share names
  with entities and verbs (calls only ever parse in outcome position); an
  op-named or IF/ELSE-named function is a load error.
  Parameters reach the AST and the prose, but NOT everything: a parameter
  may not stand in a `kind:`-qualified name (a load error — bind the whole
  ref), inside a predicate's query terms or scalar conditions, or in a
  `when:` line (functions are outcome-position only). A kind mismatch the
  AST can represent (a string bound where a ref belongs) splices through
  and surfaces as the usual runtime step rejection; one it cannot represent
  (a non-ref in an INTO slot, a non-number in a condition) is a load error
  naming the parameter — with the corner that a duration-bound parameter in
  a condition operand lands as its MINUTE COUNT (a number), and one bound
  at a signed delta position folds the `+`/`-` into the bound number.
  A braced token is spelled exactly: `{ x }` with inner spaces is the
  undeclared-token load error even when `x` is a declared parameter (checked
  at load for every declared body, called or not — expansion alone would
  check only the bodies a call reached).
  `{param}` interpolates AT EXPANSION to the
  call-site arg's canonical spelling; the reserved tokens `{actors}`
  `{object}` `{with}` `{at}` `{about}` `{to}` `{roll}` are never
  param-substituted, and an UNDECLARED `{token}` in a body string is a
  load-time error naming the token, the function, and the reserved set
  (deliberately, not a warning: this compiler has no warning channel, and
  silent pass-through would bake a typo'd parameter name into every call's
  player-facing prose). Recursion is a load error that NAMES the cycle (a
  colored DFS over the static call graph, branch bodies included — even for
  never-called functions, since expansion would not terminate); the graph's
  depth is capped at 8 (a chain of 9 is a load error); arity is checked at
  every call site's parse, nested calls included. Parameters shadow: inside
  a body, a bare token matching a parameter resolves to the parameter —
  before entity names and before role names (quote or rename to reach the
  name underneath). Role refs in bodies resolve per call site — fine in
  recipes; in slot/expiry triggers the event bindings are empty, so ops
  that need their target reject the step at runtime and iteration-style
  ops apply to nothing (world-form conditions and literal outcomes are the
  trigger vocabulary). Ordering: parse → expand → guard — the trigger
  guards and the endings check run AFTER expansion, so a body's PROGRESS/
  REFUSE or an undeclared ENDING id is a load error exactly as if authored
  at the call site.
- **Script op lines**: `NAME(params…) -> OUTCOMES` under `ops:` —
  the same three accepted head forms as functions (`NAME(p, …) ->`, `NAME ->`,
  bare `NAME p … ->`) and the same body grammar (one source line, outcome
  list, `{param}` interpolation with the exact-spelling rule, `IF`, `ROLL`,
  `DRAW`, any built-in op, function calls). A declared op is a named,
  parameterized body of outcomes callable in ANY outcome position — recipes,
  all three trigger kinds, IF arms, other op bodies — written and read
  exactly like a built-in op call: `ENCHANT(ROWAN)`. A call expands at load
  by the same pass functions use; the engine and the log never see a call,
  and §4.4's built-in table stays closed — which is why every load guard
  (ENDING membership, DRAW decks, RUN scoping, the trigger ban sweep, the
  dice lints) sees through op bodies by construction: the guards run
  post-expansion, on the body the call became. Name rules: an op name is
  UPPER_SNAKE, and it may be none of — a built-in op (the table is closed),
  a reserved word (declared verbs included: the ops table builds after the
  load's verb registration, so a script's own lexemes are as taken as
  built-ins), a duplicate op, or a declared function's name (the
  function-name grammar admits UPPER names, so the two namespaces are
  checked against each other — not case-disjoint by construction). Op names
  are deliberately NOT added to `reserved`: calls only ever parse in
  outcome position, so a prototype or entity may legitimately be named like
  an op — a bare `ENCHANT` in an outcome list stays SPAWN sugar; only
  `ENCHANT(` is the call. Arity is checked at EVERY call site at load (wrong
  arity is a load error naming the op, the expected count, and the actual
  count — nested sites included, the wrapping body's name prefixed); this
  op-grade check is deliberately NOT retro-fitted onto functions. An op
  body may call other ops and functions alike; `ops:` and `functions:`
  declare one flat namespace that builds names first and bodies second, so a
  body's references resolve regardless of section order — a function or op
  declared after its caller is legal. A FUNCTION body may never
  call an op — a specific load error ("functions expand at load before ops
  exist"), not a generic unknown-call. Recursion (op→op) is a load error
  naming the cycle, over the same colored DFS the functions use; the depth
  cap is the functions' 8, counting op→op hops only (a function's graph is
  already acyclic and capped by its own build, so function calls are leaves
  of the op graph). The `unknown 'NAME(…)'` error teaches declared ops as
  part of the outcome vocabulary.
- **Deck lines** (§4.4): `ID spec NAME[, NAME…] [defaultcard NAME] [reset]`
  (the trailing clauses in either order). The id is UPPER_SNAKE, unreserved,
  and unique. Spec entries are prototype/entity NAMES only — repeats weight
  the draw — and must resolve at load (naming the deck and the entry
  otherwise), as must `defaultcard`. The fixed draw order is the seed-fixed
  position permutation of §4.4; `reset` wraps when exhausted, `defaultcard`
  serves forever, neither makes every past-the-period draw a §9.2 rejection.
  Deck entries should be stackable, or be consumed or renamed on the same
  step — otherwise a non-stackable entry whose previous copy still lives
  makes the SPAWN half of its next serve reject, and the deck pins shut: the
  rejection discards the draft, the cursor never advances, and every
  subsequent serve of that entry fails identically. That is the documented
  authoring rule, not engine machinery — a test pins the stuck state (the
  message, the frozen cursor, the unchanged snapshot) so no engine change
  can silently "fix" it by skipping or auto-consuming.
- **Ending lines**: `ID: "Title" [(flavour)]` — the ending table (§4.4).
  The id is UPPER_SNAKE and unique; the title is a quoted string; the
  flavour is a display word (any lowercase term) defaulting to
  `melancholy`. The table is what the `ENDING` outcome (§4.4) must name: an
  ENDING with an undeclared id — in a recipe or in any trigger kind — is
  a load-time error.

## 11. The play interface — lenient input, the line REPL, undo

A `Game` is script-compiled content + base state + the append-only log
(`Game::from_script`). `handle_line` is one turn — pure enough to drive from
tests. The shipped interface is a plain **line REPL** (`bin/chorus`: a
`read_line` loop over `Game`; the `» ` prompt is drawn only when stdin is a
TTY, so a piped session's stdout stays byte-clean). No TUI ships in this
crate — `Game` is the headless seam a drawn UI would sit on. An action turn
ends with the `« … »` canonical echo — every record's sentence, `|`-joined —
printed under the prose; a game-over turn returns the ending notice instead
of executing (§4.4).

**Lenient input** (`read the forgetting`) is a *front door*, not NLU (§2):
it maps onto the canonical grammar, and the engine-composed canonical
sentence is what gets logged (§9.1). Rules:

- **Canonical passthrough**: input starting `d:h @` or containing `->`
  goes through the strict parser unchanged (clock defaults to the world
  clock when absent).
- Otherwise: optional leading `d:h` clock (default: **begin now** — the
  world clock, minute-precise; windows may begin mid-halfhour) — an
  out-of-range halfhour
  field wraps into whole days the way durations compose (`0:118` = 118
  halfhours = day 2, halfhour 22; the canonical `d:h @` form keeps its
  strict `bad halfhour` bound); the
  first dictionary verb token ends the actor list — two-token preposition
  forms like `use on` are recognized; tokens before the verb are actors,
  defaulting to the `player:` character. A missing verb fails with the
  whole dictionary vocabulary listed, plus verb suggestions (a multi-word
  verb also matches on its first word).
- Role keywords `at | with | about | to` start role phrases; bare names
  before one are objects; a repeated role keyword accumulates into one
  group (`with x … with y` is one `with`).
- Names resolve case-insensitively, underscores as spaces, articles AND the
  possessives `my/her/his/their` dropped, longest match first (up to 4
  tokens) among **alive** entities;
  unknown words fail with suggestions (prefix match, then edit distance ≤ 2).
  A name named by several roles of one clause binds distinct §4.2 pool
  copies in birth order — the first role the head, each further role the
  next living copy, addressed by `#id` — and the line refuses when the
  living pool is smaller than the roles naming it; a name doubled inside
  one role still meets the engine's same-role placement refusal.
- A `with X` phrase whose verb has no `with` slot routes X to the verb's
  real companion slot: the `to` arm takes the FIRST item (when `to` is
  defined, free, and the item fits its query); otherwise every item that
  fits the actors slot query routes to the actors while they have room for
  them; anything unroutable stays in `with`, and the engine rejects the
  phrasing as typed.
- Parse failures are collected in sentence order and only the FIRST
  speaks: an unknown word takes the suggestion path, a pool too small the
  refusal.

**The interface shows its options outright**: the line REPL opens
with `Game::banner` — the title, the script's `goal:` line (`§ …`, the
deadline is part of the game), a status line (`you are MARCH · at GUARD VAN ·
day 0, 21:00` — the player's clock is the same minute rendered as day +
time-of-day), `here:` (the cards sharing the place, pool copies grouped
`×N`), the **try row** (`suggestions()`, below), the configured verbs, and
the way to `help`. After every turn — play, `wait`, `undo` — `after_turn`
prints the clock with the stations still processing (`now: day 0, 22:00 ·
processing: explore until 23:00`, open windows in due order) and a fresh
try row with `wait` appended while anything is cooking; the turn that ends
the story gets the one line that says so (`undo` reopens, `quit` leaves),
printed once — later turns answer with the plain notice. The meta views are typed words — `look`
(place + clock, cards here, **exits** read off the living direction
relations the same way `rel(<dir>, …)` gates travel, what the player
carries, the processing stations), `cards` (the visible table, copies
grouped), `inspect <name>` (a card's face; a card that left play says
"no longer in play (left play at action N)"; a bare `inspect` prints the
usage and the table, never guessing a card), `journal` (canonical),
`story` (prose), `undo` (names what it took back — `undone: « … »` or
`undone: the wait`), `wait`, `help`, `quit`. A piped session echoes every
line as `» line` (the stand-in for a TTY's prompt + typing; on a TTY the
`» ` prompt is drawn instead and input is not echoed).
`suggestions()` (the try row) is `Game` API the REPL prints: verb × card
bindings that fit the slots — multi-person verbs name their companions
before the verb (`march the guard work`), verbs lighting a non-default
non-REFUSE recipe first, flavor bindings appended after — every entry
lenient-parseable and typeable verbatim (lowercase display names), the
list capped at 3; candidate pools rank same-container cards first, ties in
the frozen name order (ASCII-lowercase, then byte order); an actors slot
with no query defaults its pool to the player's own kind (§4.1's frozen
role). A step rejection renders the verb's usage line from the same slot
arithmetic, and the noun a slot wants is its kind — `<someone>` for
`kind:character` (§4.1's frozen role), `<kind>` for any other declared kind,
`<thing>` when the slot queries no kind. Verbs with an
open station and cards locked in a window are excluded; `recipe_lights`
skips REFUSE recipes and spent `max` recipes (they land in flavor), and a
REFUSED action prints the verb's authored `shown` hints under the refusal.

**Lenient input meets its own suggestions**: name runs normalize
with the same key the tables use — `MARCH READ THE_MANIFEST`, `the vane
ledger` and `read ledger` (a unique name-word) all resolve; when a word is
ambiguous among several names, the one fitting the verb's slot query wins
(`guard` is THE GUARD to `talk`, GUARD VAN to `explore`), and a unique fit
resolves, otherwise the word fails with display-form suggestions (`did you
mean: the vane ledger?`) — prefix, name-words containing it, then
edit distance ≤ 2, per word. A misspelled verb suggests the whole line with
the verb corrected (`xeplore the car` → `explore the car`). Each verb's
`says` template may donate a prose alias — the first word standing alone
before a placeholder or a full stop (`{actors} question {to}.` →
`question` is TALK, `search` is EXPLORE, `convene` is USE_ON) — unless the
word prefixes a registered two-token verb (`use` never aliases: `use on`
is USE_ON), is a registered verb itself, or a role keyword. A bare name
after a verb with no object slot goes to the first companion slot every
item fits (`question the guard` → `talk to THE_GUARD`). Input carrying an
outcome arrow (`->`) is refused: outcomes are the log's grammar, not the
player's — nothing is silently dropped. An unknown word that names a card
no longer in play says so, and one naming an unminted prototype says the
table has nobody by that name yet.

**The table is a projection, not the world** (`Game::visible()`): places, the
player, cards sharing the player's location, cards the player carries, the
CONTENTS of any place the log has named (an explored place shows its
insides), and
anything the log has named — through a role, an outcome ref, or a SPAWN's
INTO destination — are
on the table; everything else is out of sight until the player goes and
looks. The projection is computed per render, never stored: undo and replay
re-derive it for free (§7).

**Unknown characters are authored, not special-cased**: a suspect boards as a
masked twin (`THE_GUARD is character with duty:2, unmet:1 at GUARD_VAN`) and a
first-talk recipe swaps the mask for the named prototype
(`talk.meet_guard when to.aspect.unmet >= 1 AND to.aspect.duty >= 1 ->
DESTROY THE_GUARD, SPAWN BRACK INTO GUARD_VAN, NARRATE "\"Brack. Railway
security…\""`). Until that exchange the player sees the archetype, not the
name — engine state untouched, everything in the log (§9.2).

**Undo** = truncate the log to where the last turn began and re-fold the
surviving prefix from the base state (§7). The turn boundary is
bookkeeping, not log archaeology: a turn may be `[advance…, begin]` or
a bare advance, and no suffix rule can tell them apart once appended.
Rejections and parse errors log nothing,
so there is nothing to undo after them. Once the story has ended (§4.4)
input returns the ending notice, and the
suggestion list empties — while the views and meta words keep working, because
undo past the ending record reopens the book.

## 12. Remaining open questions

1. **Non-character subjects**: can any card be a subject for flavor events
   ("THE_PALE_LADY WHISPERS_TO ANA"), or only `character` kinds?
2. The player's container: a CS-like "table" where loose cards live — one bag,
   or pockets/locations?
3. The `to` role has its first content use (the Nightjar Flyer's
   interrogations, §5.3) but no core verb declares it yet (§4.3):
   recipient semantics (gifts, summons, threats) still await a core verb.
4. ~~Script-level verb declaration: new verb lexemes currently require
   `Dictionary::register_verb` in code (§10); should scripts declare them?~~
   RESOLVED: yes — the `verbs:` block declares them (§4.3), the
   load registers them into its effective dictionary under the §5.1 nameable
   grammar.
5. The unattended pump: the CS slice runs its time loop with no background
   tick — the `job→needs` induce bootstraps it (`on complete` + RUN
   hand-off, §6.7), and the loop, once resident and self-rechaining at
   100 %, suppresses later bootstraps exactly as CS's does. A true
   unattended pump remains future work.

## 13. Tooling — the reach analyzer, the lint, the bins

The crate ships more than the engine. Three tooling surfaces live beside it,
each with its own frozen contract; none touches the grammar, the log
format, or play semantics.

### 13.1 The reach analyzer (static card reachability)

`src/reach/` answers "can the player obtain card X?" STATICALLY, over
(compiled content, initial state) — design-time analysis: no engine, no
mutation, deterministic. Nine seam modules (the analyzer proper — none
of them imports the engine; the `fuzz` / `scale` campaign modules that
share the directory are §13.3's differential layer, `midgame` composes
the engine's own replay entry for the `--from-state` face (§7), and they
are the only reach files allowed to):

- **census** — every SPAWN/REPLACE/DRAW site over recipes, all three
  trigger kinds, decks, prototypes and initial entities. IF-branch
  discipline: sites live in segments, and an authored REFUSE kills exactly
  the sites whose ancestor chain contains its segment — its own arm and
  anything nested inside it (an untaken arm never mints). Throwing
  ops (MOVE/RELATE/UNRELATE/REPLACE) demote their whole outcome list.
  Unbounded counts carry the `MANY` sentinel; derive-mints behind
  object-mandatory verbs carry a name-blind sentinel.
- **world** — the weighted, distinct-fact card-world the predicates
  evaluate over (exactly the §4.5 match, over the flattened entity array);
  `path_open` demands every path condition static AND true (anything
  non-static demotes); `writable_paths` unions the aspect/state keys a
  query demands across §4.5 alternation groups; the risky-freeze model
  bounds freeze distance in first-crossing units.
- **fixpoint** — committability as a NAIVE round-robin re-test (verbs in
  content order, then sites in live order) until nothing changes. The
  schedule is part of the contract: `count(X) <= k` gates make
  `path_open` anti-monotone, so proven-set membership is
  schedule-dependent and a worklist or event-driven rewrite CAN change
  the output — such rewrites are forbidden. Slot support models roles as
  distinct `name#copy` instances (first-fit over availability order);
  expiry-arming recurses with cycle guards. Completion triggers prove only
  when a firing completion is CONSTRUCTIBLE, in three rungs appended at the
  ladder tail. The window rung: some committable verb's committed-role slot
  (the committed roles lock; actors never do — no duration condition, the
  engine registering and completing duration-0 windows like any other) must
  bind the home descriptor (max ≥ 1, query fit, min satisfiable around the
  home) under a COMPLETABLE recipe — selectable under first-match (no
  earlier when-less recipe, no earlier static-true `when` always beating
  it), with no decided REFUSE, no deterministic completion-time error on
  the always-applying path (an application ERROR defers from the table to
  the completion and refuses the window; a census-frozen draw cap of 0
  never durably serves), and no unconditional non-stackable mint colliding
  with a living initial card or a sibling mint (roll/role-gated ops sit out
  every part — the window-refuse retry is existential). The aspect rung:
  the aspect form requires such a binder recipe to DECLARE the aspect
  (§6.7 matches the committed recipe's `aspects`). The draft rung, judged
  PER WITNESS PAIR (complete triggers fire on every completion, so
  reachability is EXISTENTIAL over the (verb, recipe) witnesses): the
  completion refusal is atomic — the whole scratch dies with any refusal —
  and the site proves by walking any witness pair whose completion draft
  holds none of the body's unconditional non-stackable mints, demoting
  only when EVERY pair is dirty; a pair's draft occupants are that pair's
  own — the locked home itself, the pair's completing recipe's
  first-application mints, and the same-home aspect-compatible sibling
  triggers' mints firing beside it (one shared scratch per completion pass
  in declaration order, filtered by the pair's recipe aspects — never a
  global union); symmetrically, a completing recipe whose verb force-locks
  such a trigger's home (a min≥1 committed slot whose every world fit is
  named the home — a 0..1 slot can be left empty, so it never forces)
  demotes when the body mints a draft name — its own mints die with the
  scratch. The first clean witness renders the support line; no freeze
  condition applies (a completion firing is scheduled by the window that
  binds the home, not by the slot clock). Every demotion guard yields its
  structured verdict as it fires:
  `site_ladder` is ONE pass over the rungs in evaluation order, each
  failing guard producing its `CardBlocker` (judgment and reason are the
  same computation — no post-hoc re-derivation); `site_proven` is the
  ladder's `.ok()` face and the `--why` diagnostics its Err face, so the
  demotion rules have one home. New rungs append at the ladder tail;
  existing rungs stay stable.
- **paths** — the post-fixpoint report: dead verbs are derived here, along
  with the mint universe (aspect values
  there are bounds, not facts), deck caps derived from proven once-trigger
  draws (only `order[0..cap]` can ever be served), the first-hit
  provenance chain, recursive path trees (guards/supports/requires, depth
  backstop 12), and guarded "candidate" reasons (ELSE fallback, same-list
  kill, duplicate-collision risk, throwing ops, shadowing, non-static IF
  paths, when-guards). Three semantics: `existence` (default), `usable`
  (also bindable into a committable verb slot), `demand` (a §4.5 query
  matched against the actual mint descriptors, target ignored). Verdicts
  tier obtainable / unobtainable / unproven / unusable × existence
  proven / candidate / none. The diagnostic faces live here too, both
  over the settled facts with ZERO new analysis: `card_blockers` (the
  `--why` table — the ladder's Err side over every live unproven site
  serving the target, the same pass `site_proven` runs) and
  `verb_report` (one verb's duration, slots, committability proof and
  per-recipe mint-site verdicts). World notes converge: a note that NAMES
  a card — a maximal `[A-Za-z0-9_]` token equal to a post-fixpoint
  universe name (initial ∪ prototypes ∪ deck cards ∪ spawn mints; the
  derive sentinel cannot tokenize) — rides only on THAT name's report; a
  note naming no card stays global.
- **render** — the report text, byte-frozen (4-space indent steps,
  `path i/n:`, `verdict: X (existence: y)`); design-time predicate/query
  spelling, not the canonical printer. Beside the frozen faces sit the
  two diagnostic renderers (`render_why`, `render_verb`); their bytes are
  pinned by the test suite, not frozen.
- **world_report** — the `--world` global dashboard: one whole-world
  report over the facts census/world/fixpoint/paths already computed —
  ZERO new analysis, every section a re-render of post-fixpoint fields.
  Sections, each omitted whole when its data is empty (overview and
  freeze always render): the overview (live/refused site counts,
  verb/prototype/deck counts, trigger counts by the three kinds); the
  freeze line (frozen, the bounded freeze distance in first-crossing
  minutes, the risky owners by name); dead verbs, each with the first
  min>0 slot whose query can never match — the very predicate that
  condemned the verb — rendered in the design-time spelling; deck caps
  from proven draws (`MANY` renders unbounded); the collision names; the
  destroyed-by map (name → the owners that destroy/replace it); the SET
  pump map (set sites grouped by key → owners, annotated refused /
  shadowed / non-static path); the refused-site owners; and the world
  notes. Determinism is the contract: every section walks
  IndexMap/IndexSet insertion order (content order underneath) and
  nothing sorts or rehashes, so two runs over the same (compiled
  content, initial state) render byte-identical text.
- **export** — the machine-readable exits, both ZERO new analysis over
  the settled facts. `--json` serializes the per-card `CardReport`
  inside the text head's meta envelope (`file` / `demand` / `usable`
  around `report`): the `serde::Serialize` derives live on the paths.rs
  report types (derives only, no logic), so the JSON keys are the fields
  in declaration order (the text report's own section order) and the
  report enums serialize as their `as_str` strings — never the Rust
  variant names; serialization is deterministic (struct order, `Vec`
  order, no maps). `--why` MERGES the blocker table into the envelope as
  a `why` array, each row's blocker `CardBlocker::as_str`'s own text —
  the very string `render_why` prints; without the flag the key is
  absent, mirroring the text face rendering nothing. `--graph` renders
  the whole-content mint dependency graph as Graphviz DOT: nodes are the
  universe ∪ initial card names (kind-annotated ellipses; first
  occurrence wins the kind) plus one box per distinct site owner string.
  Every PROVEN site contributes its edges through the analyzer's OWN
  bare-name serving predicate (`analyze_name`'s — the graph re-derives
  no analysis of its own): a derive-mint behind an object-mandatory verb
  never mints the bare name (12320), so its site contributes no edge at
  all — its `requires` fillers ride mints it never produces, exactly the
  blind filter `record_mint_source` applies to the requires side — and a
  draw serves a name only when it is not derive-blinded and
  `facts.deck_cap` can reach it: the freeze model's value, rewritten to
  0/1 for trigger-held draws after the fixpoint (seed 81401's shape),
  judged per name by `deck_serves_cap` — the cursor-aware verdict the
  card face sees, against `facts.deck_cap`; one serving predicate, one
  home (at cursor 0 it is `served_by_cap`, the script faces' identity;
  the site's raw cap is never read). `mints → requires` edges carry the
  verb's slot fillers (or an expiry trigger's named card — the same
  split `path_for` renders), and `census.destroyed_by` the dashed
  `destroys` kill edges. Determinism is the contract: node lines sort by
  node id, edge lines by (src, dst, label), both under `cmp_utf16` with
  exact duplicates collapsed — two runs over the same (compiled content,
  initial state) render byte-identical text.
- **cost** — the `--cost` face: the cost/count reading of the settled
  facts, ZERO new analysis (a seam module beside `export`).
  Three answers over the PROVEN provenance DAG (`facts.proven_sources`
  — the `mint_sources` candidates under the chain's OWN serving
  predicate, the deck-cap test folded in — with the `fillers` split
  `path_for` renders): **minimum verb applications** — the memoized
  shortest path where a recipe-owned proven site costs 1 (one
  application, its fillers in hand), a trigger-owned proven site costs
  0 (slot triggers are the clock's; expiry/complete triggers are
  ARRANGED — costed 0 with a note, their required fillers still
  recurring), initial entities cost 0, depth-backstopped like
  `path_for` (12), tie-break stable (the first proven source in proof
  order — the chain's own first-fit), reported with the cheapest
  witness chain on one line; **minimum clock minutes** — the same DAG
  under the clock weights (recipe hops = the verb's duration, slot
  hops = `first_cross`, arranged hops 0); **copies bound** — the max
  simultaneous copies the settled world supports from `avail`'s
  counts (`MANY` → `unbounded`; a count past `FLATTEN_WEIGHT_CAP` →
  `{sum}+ (world cap)` — `cards_of`'s bookkeeping cannot count past
  the cap, so the figure is a floor). The number is a LOWER-BOUND
  planning estimate and every costed report says so: it assumes each
  needed recipe applies once with its fillers in hand, and gate
  timing / the anti-monotone count gates are honored only through
  provenness (a site they kill is never a proven source). A frozen
  world's note says the freeze boundary caps the clock. The face
  composes with `--usable`/`--why`, the `all` command and the
  bounded-time faces
  (the `--by` window bounds the DAG — only sites proven inside it
  participate, and a note says so; `--from-state` anchors for free —
  same facts, the clock re-anchoring with the replayed minute and the
  deck windows shrinking with the cursor); it stays out of `--json`
  and is ignored under `--demand` (the demand match's provenance is
  not costed) — documented, not rejected.
- **bridge** — the lint seam (§13.2): the settled facts re-rendered as
  WARNING-class findings for `bin/lint-scripts --reach`, ZERO new analysis
  (a seam module beside `cost`). One `Analyzer` per script; six
  finding families in a fixed class order, every walk an IndexMap/IndexSet
  insertion order, deduplicated per owner, every finding owner-addressed
  (reach has no line numbers) and WARNING-class (a design smell, never an
  error): dead verbs (`dead_verbs`, the killing slot walked by the same
  `build_post_fixpoint` hopeless predicate as the `--world` dead-verbs
  row); self-gated lists (live unproven sites re-run through
  `site_ladder` over the read-only world rebuild — the `--why` pass's own
  ladder, no re-derivation — with only the `SelfGated` rung surfaced);
  risky owners (`risky_owners`); deck-cap cursor freezes (a live draw whose
  census cap froze below `MANY` on a repeatable owner — a once trigger's
  single draw is its design, not a smell); the frozen world (`frozen` +
  `freeze_dist` — the boundary minute `now + freeze_dist` with the risky
  slot triggers standing on it, exactly the argmin owners of
  `freeze_dist`'s own min over the risky slot triggers' first crossings —
  the minute the `--world` freeze line renders); and REFUSE-doomed
  sites (the refused census, owners named — the world note carries the
  count). The findings reuse the lint's own `LintFinding` type, so the bin
  prints one uniform stream.

The two bounded-time faces key the SAME settled facts on one horizon
(`WorldFacts::horizon`) instead of adding analysis. `--by <minute>`
bounds every time-keyed judgment at the authored absolute minute M
(engine clock, §7): the horizon is `min(freeze_dist, M − now + 1)` —
`INFINITY` unbounded, the freeze boundary and the window taking the
tighter, the +1 turning "fired by minute M" into the freeze model's
strict `first_cross < horizon` — and every time-keyed rule reads the
horizon it induces, never the raw bound. A slot trigger whose first
crossing lands beyond M has not fired in the window and its sites fall
to candidate (`TriggerBeyondBy`); duration>0 recipes demote at ANY
finite horizon (`BeyondByDuration`, the unfrozen spelling of the
seed-296 rule); trigger-held deck caps rewrite `first_cross < horizon`
to fired-by-M 1/0 and the expiry-arming recursion follows the same
horizon; expiry/complete triggers are never demoted by M directly
(arranged, not timed). The report head gains `  [by minute N]`.
`--from-state <log.jsonl>` rebuilds the analysis initial world by the
engine's OWN fold (`reach::midgame::replay_state` = `load_log` + `fold`,
§7 — a third engine-composing reach file beside `fuzz` / `scale`, zero
session/engine edits): the replayed entities and clock enter for free
(every `first_cross` re-anchors), the log's fired set demotes spent
once triggers (`AlreadyFired` — they never fire again; their past
mints ride the replayed entities as initial facts), and non-reset deck
cursors bound the remaining draws (`deck_serves_cap` serves the window
`[cursor, cursor + cap)` — the graph face's predicate too;
`add_product` registers the same window; identity at cursor 0). Read
but deliberately unmodeled: open windows (the lock table), the
executed-recipe map, the rng counter — named by a name-free fidelity
note that rides on every replayed card report (global, the
note-convergence rule). An ended log is the analyze error (exit 1).
The bound and the replay mark live on the Analyzer HOST (one world per
run — the bound cannot drift between cards); on the default face every
rule reads `by: None`'s horizon, which IS the freeze distance — the
frozen output byte-identical throughout.

CLIs: the argv surface is the cargo/uv-style Commands + Options grammar:
`reach <script.txt> <COMMAND> [OPTIONS]` — the script first, then
one of six commands, `card <NAME>` (the default face: one card's proven
paths and bindable slots), `all` (prototypes ∪
LIVE initial entities ∪ `deck.order` — not `defaultcard` — in UTF-16
order), `demand <QUERY>` (a §4.5 query matched against the actual mint
descriptors, any target never read), `verb <NAME>` (one verb's report),
`world` (the whole-world dashboard) and `graph` (the whole-content mint
dependency graph as Graphviz DOT) — `world` and `graph` need only the
script. The command IS the face: a `-`-spelled unknown word is an
unknown FLAG (exit 2, stderr, naming the word and listing the legal
flags), a non-`-` word an unknown command (naming the word and listing
`card, all, demand, verb, world, graph`). `--help`/`-h` anywhere in argv
prints the full help (the command table, the option table, three real
examples) on stdout and exits 0; no arguments prints the short usage
(`usage: reach <script.txt> <COMMAND> [OPTIONS]` + the `reach --help`
hint) on stderr, exit 2; a script with no command word, or an unknown
command word, is the same exit-2 family; an unknown `-`-word in flag
position — a typo'd `--usablee`, a stray `--` (there is no
end-of-options separator) — is the unknown-flag error naming it; extra
positionals beyond `<script> <COMMAND> [value]` are the exit-2 error
naming the first extra; `card`/`demand`/`verb` with no — or an empty, the
frozen truthiness — value is the exit-2 error naming the command. Argv
errors exit 2 with the short usage; compile/analyze errors exit 1
(stderr only); every value flag or value command with a missing value
names itself, and a value slot refuses a `-`-leading token (that token is
a flag, not a value — `--from-state --why` names the flag instead of
reading a log named `--why`), the single deliberate exemption being `--by`
(negative minutes are values).

The options compose: `--usable` (existence plus bindability
into a committable verb slot), `--why` appends one `— why` section to
each card report (the `card` and `all` faces alike): every live unproven
site serving the card, with its owner and the ladder's FIRST blocker —
without the flag the output is byte-identical, and a card whose every
serving site is proven renders the one-line empty face. `--cost` appends
one `— cost` section to each card report (the `card` and `all` faces
alike, after `--why`): the three answers — minimum applications with its
witness chain, minimum minutes, the copies bound — with the lower-bound
honesty stated on every costed report, and a target with no proven DAG
rendering the one-line exclusion face. It composes with `--usable`,
`--why`, the `all` command and the bounded-time faces, stays out of
`--json` and is ignored under the `demand` command (documented, not
rejected). The `verb` command renders one verb's report (duration, slots,
committability verdict with its proof lines, per-recipe mint-site
verdicts — the settled facts re-rendered, zero new analysis); `--why`
does not apply to it, and an unknown verb name is the analyze error
(exit 1). `--json` rides the card faces only (`card <NAME> --json`;
`all --json` one JSON array in the walk order; `demand "<q>" --json` the
envelope with the query in `demand`), `--why --json` merging the blocker
table into the envelope; the `world`, `verb` and `graph` faces ignore it
— their faces stay their text and DOT selves.
`--by <minute>` bounds the card faces (it composes with `all`, `card`,
`demand` and `--why`; `world`, `verb` and `graph` ignore it, and the
`--json` envelope carries the BOUNDED report with no `by` marker — the
report fields say it). A missing or non-integer value is the argv error
naming the flag (exit 2). `--from-state <log.jsonl>` plays the midgame
face on every face (the state is the Analyzer's input — the fidelity-note
head is the card face) and composes with `--by`, the window anchoring
to the replayed clock; a missing value is the argv error, a
missing/unreadable/unparsable log or a drifting fold is the analyze
error (exit 1), and a log that ENDS the game is refused the same way —
an ended world has nothing left to reach.
`--color` is the one output-styling switch: OFF by default —
the identity transform, every face byte-identical to its unstyled
output; ON, the bin wraps the ALREADY-RENDERED lines in ANSI SGR
sequences through the shared lib module `chorus::cli` (`Styles` —
line-level prefix matching over the frozen text; the library renderers
are untouched, coloring lives at the bin layer): the verdict word
green+bold (`OBTAINABLE`) / yellow+bold (`UNPROVEN`) / red+bold (the
negative verdicts), each face's first line and the `— why` / `— cost`
section headers bold, `note:` rows dim+italic, `path i/n:` heads and
their `·` steps dim, `bindable via` rows cyan, `candidate source` rows
dim, and the `world` panel's freeze value red (frozen) or green (not
frozen), its `risky:` row yellow and its dead / refused counts red when
non-zero. Only foreground colors (crossterm's own fixed named-color
encoding) and the bold/dim/italic attributes are ever emitted — no
background colors, no 256-color palette values. The machine faces (the
`--json` envelopes and the `graph` DOT) are NEVER colorized — under
`--color` they stay their exact bytes.
A card minted only by `on complete` clauses reports like any other — its
provenance chain names the engine-minted `complete:{home}` trigger id —
and the analyzer never rejects a script for owning one.
`reach-fuzz` differentially fuzzes the analyzer against
the real engine's settle (`--seeds/--steps/--states/--from`, defaults
200/7/400/1; FAIL and soft-inconclusive lines on stderr, progress and the
summary on stdout; exit 1 iff a failure; failing seeds printed
space-separated). `reach-scale` runs the census at CS scale (default the
full 1,566 × 2,084 world) on worker threads — at most
`min(available_parallelism, 4)` workers by default, each rebuilding the
whole deterministic world — with a `--worker i k P R` child protocol
dumping per-name JSON stats.

The fuzz generator (`gen_scenario`, v2) builds one random script per
seed over the FULL trigger face — slot, expiry and completion triggers
(the complete form drawing its home and its optional §6.7 aspect gate) —
plus §6.7 RUN candidates (a recipe body may RUN only the same verb's
strictly earlier recipes, acyclic by construction; a completion body
draws from the global table of duration>0 recipes, bounding every
completion-fed chain to one hop per advance) and §6.4 recipe `aspects`
presence lists. The generator's draw order is deliberate (every
short-circuit and evaluation-order choice carries an inline note) but
VERSIONED, not frozen: the v2 space added the completion/RUN/aspects
draws, re-banded the outcome-op table (RUN's 0.10 width funded by SET,
ROLL and NARRATE; every other op keeps its exact v1 width) and moved the
trigger-kind roll ahead of the body draws, so scenario streams differ
across generator versions by design. What stays pinned is purity: the
generator is a pure function of its seed — the same seed builds the same
scenario, draw for draw, under ONE version.

### 13.2 The script lint

`lint_script` / `lint_script_findings` (and `bin/lint-scripts`) run AFTER
`compile_script`: a script that does not compile never lints. Findings:

- **ERROR** — a verb declares slots but no recipes (every action rejects at
  selection, §6.6); a recipe unreachable after a catch-all (first match
  wins, §6.4); a recipe shadowed by a whitespace-normalized identical
  `when`; a `count(role) op k` gate the slot's §6.1 cardinality can never
  satisfy (all six comparators); an unknown `{token}` template token (the
  six roles + `{roll}`; function bodies also admit their params); a deck
  entry neither stackable by name nor by kind (the §10 livelock rule as a
  compile-side diagnostic); an aspect read — slot query, `when`, or derive
  source — with no writer anywhere (declared `with` clauses, derive
  targets, `SET(…, aspect:x)`; every dotted prefix of a written path counts
  as written, so writing `a.b.c` satisfies reads of `a` and `a.b`, while a
  read matches exactly — a shallow write never satisfies a deeper read); an
  entity name outside the name-lexeme class (every ref to it must be
  quoted).
- **WARNING** — a `when` PROVABLY never-matching: a three-valued,
  interval-narrowing evaluation over the §6.1 slot ranges, false for every
  §6.6 selection binding. Conservative at the boundary — staying Unknown is
  what keeps the warning sound: `exists`/`count(<query>)` world reads,
  `every`, non-integer comparisons and the cross-verb RUN re-evaluation
  stay Unknown.
- **WARNING (the reach bridge)** — `--reach` appends the reach analyzer's
  global-dynamic findings (§13.1's `bridge` module) to the same stream,
  each line `[reach]`-prefixed so the two layers read apart and the
  summary naming them separately: six WARNING-class families over the
  settled WorldFacts, zero new analysis — dead verbs, self-gated lists,
  risky owners, deck-cap cursor freezes (a once trigger's single draw is
  its design), the frozen world (the boundary minute and the risky slot
  triggers standing on it — the argmin owners of `freeze_dist`'s min),
  and REFUSE-doomed sites. Owner-addressed (the per-site lint's line
  numbers have no reach counterpart), deduplicated per owner,
  deterministic (insertion-order walks — two runs byte-identical). A reach
  finding is advisory by the same rule — it never changes the exit code —
  and without the flag the output is byte-identical to the plain lint.

Errors are counted, warnings advise; findings never change the exit code
(constant 0 — a dead `when` is sometimes the point); compile errors remain
`compile_script`'s hard failure and unreadable input exits 1. The bin
defaults to every `scripts/*.txt` relative to the CWD (a missing dir names
itself and the cwd, exit 1). Script op bodies (§10) are scanned
like function bodies — an op body's raw `SET(…, aspect:x)` counts as a
write and its braced tokens get the same typo scan — but the lint never
sees through a CALL SITE: a recipe line reading `BOOST(OBJECT)` contributes
no finding itself, the write is counted once from the declared op body.
`--reach` may sit anywhere in the arguments and composes with the default
list and named files alike (the sole-argument invocation still reads the
default list).

### 13.3 The bins

`bin/chorus` is the line REPL of §11. The other four are the fixed-purpose
CLIs above (`reach`, `reach-fuzz`, `reach-scale`, `lint-scripts`) — all
five keep byte-exact stdout text and exit codes; the
`reach` CLI speaks the cargo/uv-style Commands + Options grammar of §13.1
(`reach <script.txt> <COMMAND> [OPTIONS]`; `--help`/`-h` prints the full
help on stdout at exit 0; every argv error exits 2 on stderr with the
short usage and the offending word named), its one styling switch
`--color` (default OFF — the identity transform) wrapping the frozen text
faces through the shared lib module `cli` while the machine faces
(`--json`, `graph`) never color. `lint-scripts` has the
single-purpose Options form (`lint-scripts [OPTIONS] [FILES...]`, no
subcommands: `--help`/`-h` anywhere prints the full help on stdout at exit
0; an unknown `-`-flag is the argv error — exit 2 on stderr naming the
flag, never a silent fallthrough into the file list — while any non-flag
word stays a file path, the empty list meaning the sorted `scripts/`
default) with the same `--color` switch wrapping its faces through `cli`
(the per-file title bold, the finding rows' bullet + severity marker in
the severity color — error red, `[warning]` yellow, `[reach]` magenta —
the message text untouched, the summary's non-zero counts in their
severity colors and an all-zero summary green), the exit-code discipline
unchanged (findings 0, hard input errors 1, argv errors 2). The fuzz
and scale campaign engines are library modules (`reach::fuzz`,
`reach::scale`, with the run-then-settle adapter in `engine::settle`), so
the bins and the test suite import one implementation. `lcg` (crate root)
is the deterministic campaign-side RNG those modules draw from —
deliberately NOT the engine's randomness, which is §4.4's counter alone.
Recorded campaign runs are reproducible only against the generator
version that produced them. The bins freeze exactly five dependency
crates (`Cargo.toml`: indexmap, serde, serde_json, regex, crossterm —
the last only for the ANSI writer behind `cli`, riding with
`default-features = false`; `Cargo.lock` is committed).
