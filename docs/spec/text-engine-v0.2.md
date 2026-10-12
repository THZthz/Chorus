# Chorus Text Engine — Spec v0.2

A text-only prototype of a Cultist Simulator / Book of Hours-like card game.
Everything the game *is* — cards, actions, history — is expressed as structured
text ("sentences"). Sentences are canonical data: parseable, storable in a
database, replayable. Rendering (text cards now, drawn cards later) is a
projection of that data, never the source of truth.

> v0.2 = v0.1 + implementation rulings (§9.10+). Changes: grammar formalized
> and reconciled with examples (pre-verbal `AT`, args/arity grammar, reserved
> words), USE_ON signature fixed, single time model (auto duration + ADVANCE
> as authored extra), step-atomic REFUSE, recipe ids, EVENT representation,
> aspects/attrs split, name-uniqueness rules.
>
> v0.2.1 = spec–implementation alignment review. Changes: reserved word `AS`,
> name lexeme allows leading `_`, canonical paren-less outcome forms (`SPAWN`,
> `NARRATE`), role refs as outcome args, concrete SPAWN materialization pairs,
> prototype `derive` (replaces the `CLUE(secret-2)` sugar), recipe auto-ids,
> monotonic clock, fold clock ownership, `advancedMin` on the record, slot
> `shown`/`hidden` data-model-only, script format (§10) and play interface
> (§11) documented.
>
> v0.2.2 = the processing cycle. Changes: `PROGRESS` outcome (authored
> read-bar ticks, §6.7), the processing cycle formalized (commit → lock →
> ticks → outcomes → unlock; locks guaranteed by serialized discrete time),
> prose renders progress and gains `{to}`/`{with}` tokens (§5.3), the `to`
> role reaches the record (§7), distinct cards per slot, `once` triggers
> (§9.29), hour-quantized step clocks with dead-air event crossings (§9.28),
> first story content exercising it (`scripts/the-nightjar-flyer.txt`), and
> an exploratory story test (test/nightjar-explorer.test.ts).
>
> v0.2.3 = the halfhour clock. The clock tick shrinks from the hour to the
> **halfhour** (30 minutes): `day:halfhour` with halfhour 0..47, the
> `halfhour` duration unit (§5.1), triggers fire per halfhour crossing
> (`at hour N` remains as an alias for slot 2N, §10), and quantization/
> dead-air semantics re-anchor on the halfhour tick (§9.28).
>
> v0.2.4 = the card mapping (§8.1). The operation-level correspondence between
> text-engine operations and card operations, written down: entity → card,
> slots → roles, dropping → role binding, commit → the canonical sentence,
> the processing window → the locked station, outcomes → the cards' fates.
> Documentation of existing semantics — no grammar, log, script, or API
> change.
>
> v0.2.5 = SET arithmetic (§9.31). The SET outcome gains signed delta values
> (`+1`, `-2`) and `aspect:` keys: `SET(x, aspect:level, +1)` adds to the
> entry's current value and the engine logs the computed absolute form, so
> the fold replays literals and never redoes arithmetic. Dotted arg keys
> (`state.sealed`) now lex as written in §5.1.
>
> v0.2.6 = authorable decay (§9.32–§9.34). Triggers gain a second form —
> `ID on expire of NAME -> OUTCOMES` — so an expiry can REPLACE the card or
> apply arbitrary outcomes instead of the built-in destroy; the REPLACE
> outcome (`REPLACE ref WITH prototype`, paren-less) mints the successor into
> the dying card's current location (container or zone, §10) and concretizes
> to plain SPAWN+DESTROY; REFUSE
> becomes a load-time error in all trigger outcomes; play and fold now agree
> exactly on `state.fired` for every EVENT row.
>
> **Compatibility: none, by design.** Nothing in this document is a
> stability guarantee — the grammar, the log and script formats, and the
> API may break at any version boundary (v0.2.2 changed event semantics;
> v0.2.3 re-scaled the clock). Old logs are not replay contracts: re-author
> or migrate them yourself; the engine ships no migration path (§9.30).

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
| `place`     | Locations, nestable (VILLA ⊃ STUDY)       | HOUSE, GREENHOUSE, STREET        |
| `item`      | Portable physical things                  | candle, key, funds, tincture     |
| `text`      | Readable artifacts                        | book, letter, codex              |
| `idea`      | Intangibles produced by thought           | lore fragment, clue, memory      |
| `state`     | Conditions attached to a character or run | dread, affliction, vision        |
| `rite`      | A procedure-as-card the player owns       | a summoning rite                 |
| `resource`  | Abstract player stats as cards            | vitality, passion, reason        |

### 4.2 Entity properties (what a card carries)

| Property  | Shape                                     | Examples                            |
|-----------|-------------------------------------------|-------------------------------------|
| aspects   | weighted principle tags, dot sub-keys     | `lantern:2`, `language.latin:2`     |
| attrs     | engine/content scalars & flags            | `container:#study`, `state.sealed`, `expires:+24h`, `portable` |
| relations | typed edges to *other* cards              | `(BERTHA, parent_child)`            |

**Aspects are principle tags only** — the matching currency of slots and
recipes. **Knowledge is aspects on a character** (`occult:3`,
`language.latin:2`) — there is no separate skill system.

**Structural properties are attrs**, not aspects. Engine-known attr keys:
`container` (where the card sits — the container's entity id), `expires`
(`2h`/`90m`/`1d` style; the engine materializes `expiresAt` = current clock +
duration at set time, stored as **absolute minutes**) and the boolean flag
namespace `state.*`: `state.sealed`, `state.locked`, `state.hidden`,
`state.portable`, `state.unique`. Queries read them via `state:sealed` /
`state:!sealed` (§4.5); `SET`/`REVEAL` toggle them.

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
| `WORK`      | ACTORS WORK [AT place]                 | labor → resources              |
| `TALK`      | ACTORS TALK [AT place] [ABOUT idea]    | social outcomes, group actions |
| `EXPLORE`   | ACTORS EXPLORE place                   | discover contents of a place   |
| `TRAVEL_TO` | ACTORS TRAVEL_TO place                 | deliberate movement            |
| `LOCATE_AT` | ACTORS LOCATE_AT place                 | set/dump position (utility)    |
| `DREAM`     | ACTORS DREAM [WITH idea]               | visions, rest                  |
| `USE_ON`    | ACTORS USE_ON target [AT place] [WITH item] | craft, rites, keys, tools |
| `ACQUIRE`   | ACTORS ACQUIRE object                  | buy/gain via cost              |
| `WAIT`      | ACTORS WAIT                            | pass time (auto duration)      |

Signatures say which roles a verb may take; cardinalities and queries are
content (§6.1). The seed content narrows some of these (e.g. seed `ACQUIRE`
takes an object of any kind; seed `WORK` declares no `with` slot). Custom
verbs are declarable with the same shape via `Dictionary.registerVerb` — the
core set is a default, not a closed wall.

### 4.4 OUTCOME operators (fixed vocabulary, **normative arities**)

| Operator  | Args (normative)                | Effect                                        |
|-----------|---------------------------------|-----------------------------------------------|
| `SPAWN`   | ref [, INTO ref] [, AS #id] [, k=v …] | mint a new entity (from a prototype if one is defined); `AS #id` is engine-written so folds replay exactly; `aspects=`/`attrs=` JSON pairs are engine-written materialization (§9.18) |
| `DESTROY` | ref                             | card removed (deadAction stamped)             |
| `REPLACE` | ref `WITH` prototype            | decay a card into its successor (§9.33): mint from the prototype into the dying card's **current location** — its `container` (place-contained or character-carried) or, for a zone-tagged card, its `zone` (§10: the grouping has no card, so the tag passes to the successor) — then soft-destroy it — spawn first, then destroy, so the old name is released as usual. Paren-less authored form `REPLACE X WITH Y`; the engine logs the concrete `SPAWN … AS #id` + `DESTROY #id` pair, so there is no REPLACE in the log (§9.33). Prototype derivation (`name from object`) is unavailable — REPLACE has no clause. A name collision with any living entity — the dying card itself included, still alive at spawn time — rejects the whole step (§9.19) |
| `SET`     | ref, key, value                 | mutate an attr (`state.sealed`, `container`, `expires`…) — or the aspects map when the key is `aspect:`-prefixed (`aspect:level`, §9.31); a signed integer value (`+1`, `-2`) is a delta on the entry's current value and logs the computed absolute (§9.31); `SET x, expires, 2h` also materializes `expiresAt` |
| `MOVE`    | ref, INTO ref                   | relocate (place→actor, actor→place)           |
| `REVEAL`  | ref                             | unlock hidden text / mark discovered          |
| `ADVANCE` | duration (`2h`, `90m`, `1d`)    | authored extra time shift (§9.12)             |
| `NARRATE` | string                          | journal line only; raw template stored, interpolated at render |
| `PROGRESS`| duration, string                | a read-bar tick: authored progress text at `duration` into the processing window (§6.7); journal-only, no state or time effect |
| `RELATE`  | ref, ref, type [, int]          | create a relation edge between two cards      |
| `UNRELATE`| ref, ref, type                  | remove a relation edge                        |
| `REFUSE`  | string                          | abort the whole step (§6.5): draft discarded, attempt still logged |

Shorthand: a bare ref in an outcome list means `SPAWN ref` (a **new** entity,
from its prototype if defined, into the step's `at` place or the first actor's
`container`). Moving an *existing* card is `MOVE`. Outcomes apply in order.

**Role refs are valid outcome args**: any ref position may name a role —
`ACTORS`, `OBJECT`, `WITH`, `AT`, `ABOUT`, `TO` (case-insensitive; rendered
uppercase) — and expands to that role's bound cards (`DESTROY WITH`,
`MOVE ACTORS INTO OBJECT`, `SPAWN MARK INTO at`). Role refs are concretized to
`#id`s in the logged outcome (§9.15, §9.18).

### 4.5 Query mini-language (slot queries — one card at a time)

```
kind:text  aspect:secret>=3  aspect:language.latin>=2  state:!sealed  -aspect:winter
```

- `kind:` — exactly one kind
- `aspect:name[.sub][>=|<=|=]level` — principle match (bare `aspect:x` = level ≥ 1)
- `state:key[=|!]value` — attr match against the `state.*` namespace (`state:!sealed` → `attrs['state.sealed']`)
- `-` prefix — negation

Slot queries test **one card at a time**. Anything about a *group* (size,
relations between members, pooled power) is a recipe condition (§6.3) — that
is deliberate: the UI can grey-out invalid drops from slots alone, while
group logic stays narrative.

---

## 5. L2 — Sentence grammar

### 5.1 EBNF (canonical form)

```ebnf
log        = { sentence } ;
sentence   = [ clock "@" ] ( action | event ) ;
clock      = int ":" int ;                       (* "day:halfhour", day 0-based, halfhour 0..47; 1 halfhour = 30 min *)
action     = clause { ";" clause } [ "->" outcomes ] ;
event      = "EVENT" name [ "->" outcomes ] ;
clause     = actors [ "AT" ref ] verbo [ "TO" ref ] [ objects ] [ "WITH" reflist ] [ "ABOUT" ref ] [ "AT" ref ] ;
(*          the trailing "AT" is input sugar only (§9.10); canonical output is pre-verbal *)
actors     = ref { "," ref } ;                   (* 1..n, order-free; canonical = sorted *)
objects    = ref { "," ref } ;
reflist    = ref { "," ref } ;
verbo      = VERB ;                              (* must be a verb registered in the dictionary *)
outcomes   = outcome { "," outcome } ;           (* commas inside ( ) belong to args *)
outcome    = OPERATOR "(" [ args ] ")"           (* SET/RELATE/UNRELATE/REFUSE/ADVANCE/PROGRESS: parens required *)
           | "SPAWN" ref [ "INTO" target ] [ "AS" "#" id ] { key "=" value }
           | ( "DESTROY" | "REVEAL" | "MOVE" ) parenlessTail      (* input sugar *)
           | "REPLACE" ( ref | role ) "WITH" ref (* paren-less only; WITH is syntax, not a role *)
           | "NARRATE" string
           | ref ;                               (* bare ref = SPAWN ref, engine adds AS #id *)
(*          canonical OUTPUT: SPAWN, NARRATE and REPLACE paren-less, every other op paren'd
            — e.g. SPAWN X INTO #house AS #c1 · NARRATE "…" · MOVE(#ana, INTO #study) *)
args       = arg { "," arg } ;                   (* arg shapes validated per §4.4 arity *)
arg        = ref | role | int | signedInt | string | bool | duration
           | "INTO" target | "AS" "#" id | key "=" value | dottedName | aspectKey ;
signedInt  = ( "+" | "-" ) int ;                 (* SET delta value only (§9.31) *)
aspectKey  = "aspect:" name { "." name } ;       (* SET key into the aspects map (§9.31) *)
target     = ref | role ;
role       = "ACTORS" | "OBJECT" | "WITH" | "AT" | "ABOUT" | "TO" ;  (* case-insensitive *)
dottedName = name { "." name } ;                 (* SET(x, state.sealed, true) *)
duration   = int ( "halfhour" | "h" | "m" | "d" ) ;
                                                 (* 1halfhour = 30 minutes *)
ref        = name | kind ":" name | "#" id | string ;  (* canonical output: name *)
```

Lexical rules:

- `name` = `[A-Za-z_][A-Za-z0-9_]*`. **Reserved words** (`EVENT TO WITH AT
  ABOUT INTO AS AND OR NOT` + every registered verb lexeme) cannot be used as
  names; content needing them uses the quoted-string ref form (`"PLAN B"`).
- **Entity names are unique among alive entities** (enforced at SPAWN /
  registration). Canonical ref output is the bare name; `kind:NAME` and `#id`
  are accepted input forms (sugar; typed refs are kind-checked at resolution).
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

One step, two clauses (travel then act); no `ADVANCE` — the clock advances
automatically by the sum of clause durations (§9.12). This record's `t_game`
is the step's **start** clock:

```
0:14 @ ANA TRAVEL_TO VILLA; ANA AT STUDY READ THE_PALE_LADY
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
and an `aspect:` key writes the aspects map (§9.31):

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
`attrs=` pairs (§9.15, §9.18; the pairs are space-separated inside the SPAWN).
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
single-clause steps. Whether a live UI shows each tick as it comes due or
prints the whole read-bar at completion is a pure display choice; the
canonical record is identical either way.

---

## 6. Engine layer — properties, slots, recipes (content model)

### 6.1 Slot cardinality: how many cards a role takes

A slot declares a **count**, an optional visibility flag, and a query every
member must satisfy. One slot per line (script syntax, §10):

```
slot  := role ":" count [ query ]
count := int [ ".." [ int ] ]                    (* "1", "2", "1..2", "2..", "0..1" *)
```

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
(`SlotDef.shown`, default `shown`); the script line grammar does not carry it
in v0.2 — hidden slots are declared in TS content only.

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
aggregate    total(actors, aspect:lantern) >= 3 # pooled power of the set
paths        total(actors, aspect:lantern) < object.aspect.demand   # value vs value
             count(actors) > object.attr.seats                      # attr paths too
logic        AND · OR · NOT (uppercase; NOT binds tightest)
comparisons  >= · <= · > · < · = · !=
```

v0.2 limits: path operands read `role.aspect.key` and `role.attr.key`
(numeric attrs); the scalar conditions inside `every`/`any` read **aspects
only** (`aspect:key >= n`).

**`exists(...)` — the world-existential check** (Chorus's answer to Cultist
Simulator's `extantreqs`/`tablereqs`, docs/reverse/cultist-simulator.md):
true iff at least one **alive** card anywhere in the state matches — every
alive entity in the world, not just the cards bound into this step's roles,
and regardless of where they sit. It reads the same input state as every
other condition (recipe-selection time, §6.4) and is an ordinary primary
condition: `NOT exists(...)`, `exists(...) AND ...`, `OR` chains all compose.

- `exists(NAME)` — an alive entity bears exactly this name (names are case
  exact and unique among the living, §9.14; a destroyed card stamps
  `deadAction`, releases its name, and stops existing).
- `exists(<query>)` — an alive entity matches the §4.5 slot query: a
  conjunction of `kind:` / `aspect:` / `state:` terms with `-` negation.
- Disambiguation: if the first token inside the parens is a query-term
  starter (`kind:` / `aspect:` / `state:` / a leading `-`), the contents
  parse as a §4.5 query; otherwise they parse as a single NAME. Spaces around
  a comparison inside a query are glue: `aspect:hunter >= 1` reads as
  `aspect:hunter>=1` (§4.5 terms are whitespace-separated).

Idiom — a rite that only burns while the coast is clear (no HUNTER card
exists anywhere in play) and the sealed door still stands:

```
- id:    rite.unwatched
  when:  NOT exists(aspect:hunter>=1) AND exists(THE_SEALED_DOOR)
  out:   PROGRESS(30m, "The door holds; the street is empty."), SPAWN ...
```

### 6.4 Recipes: first match wins, most specific first, **stable ids**

A recipe is an optional stable `id` (used for log provenance; when omitted the
compiler mints `<verb>.recipe<n>` / `<verb>.default`), `when` (or `default`),
`out`, and optional `shown`/`hint`. Data-model shape and script line (§10):

```
recipe      := [id] ["when:" cond | "default"] "out:" outcomes
               ["shown:" true|false] ["hint:" "text"]      (default: shown: false)
script line := [id] ["hidden" | "shown" ["hint" «"text"»]] ["when" cond] "->" outcomes
             | "default" "->" outcomes
```

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
`aspects=`/`attrs=` pairs — into the log (§9.15, §9.18): the log never
contains sugar.

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
   time is monotonic (§9.5, §9.20).
2. **Selection**: for each clause in order, the verb's first recipe whose
   `when` holds fires (or `default`). No recipe → rejection, nothing logged.
3. **Application**: outcomes apply in clause order into a draft; role refs
   re-resolve against the draft before each clause applies, so cards an
   earlier clause destroyed are never applied stale. A `REFUSE` discards the
   whole draft (§6.5); an outcome that *cannot* apply (unresolvable ref,
   name collision) rejects the whole step with nothing logged — distinct
   from `REFUSE`, which always logs its attempt (§9.19). Otherwise the draft
   becomes the new state.
4. **Time**: the engine advances the clock by the **sum of executed clauses'
   verb durations** plus authored `ADVANCE` extras (WAIT recipes,
   night-skipping rites) and records the total as the record's
   `advancedMin`. `t_game` on the record is the start clock. On replay
   (fold, §7) only action records move the clock — `t_game` + `advancedMin`;
   EVENT rows never advance or rebase it (§9.21).
5. **Provenance**: the record stores the fired recipe ids (one per clause,
   clause order) and the canonical sentence.
6. **Events**: after the clock advances, the engine resolves, in
   chronological order, across the **whole elapsed span** — from the state's
   previous clock to the step's end, so quantized dead air (§9.28)
   cannot swallow a crossing: authored time triggers (content: id + slot,
   the `hour`/`halfhour` syntax of §10 [+ `once`], firing on every crossing
   of that halfhour-of-day, or on the first crossing only when `once`,
   §9.29) and entity expiries (`expiresAt` crossed — authored `on expire of
   NAME` triggers fire instead when one names the expiring entity, §9.32;
   with no match, trigger id `expire:<NAME>` and one engine-written
   `DESTROY`).
   Within one crossed halfhour, authored triggers fire before expiries. Each
   fires its own EVENT log row with its outcomes applied. One pass — events
   never cascade within the same advance (v0.2 simplification; new expiries
   resolve at the next advance). **The fold trusts logged EVENT rows** — it
   never re-derives them; a once-trigger's fired-ness is a projection
   rebuilt from those rows.

Perishables/decay v0.2: an entity may carry `expires:+24h`; the engine
materializes `expiresAt` when the attr is set; the crossing fires an expiry
EVENT — the authored `on expire of NAME` trigger's outcomes if one matches
(decay chains: influence → weaker influence, health → fatigue, summons →
defunct, usually via `REPLACE`), else the built-in destroy.

### 6.7 The processing cycle (commit → lock → ticks → outcomes → unlock)

Committing cards to a verb station is not the whole action: the action
*processes* over the verb's duration, and the engine narrates that
processing.

1. **Commit & lock.** The step validates and a recipe fires per clause
   (§6.6). For the step's whole processing window — `t_game` to
   `t_game + advancedMin` — the verb station and every role-bound card are
   **locked**: nothing else may act on them, or with them, meanwhile. (A role
   takes **distinct cards**; the same card cannot fill one role twice.)
2. **Ticks.** The fired recipes may schedule `PROGRESS` outcomes — authored
   event descriptions at offsets inside the window (the "read bar"). An
   offset counts from the record's start clock `t_game`; the display plots
   ticks on the window. The log stores them in emission order, so a replay
   re-derives exactly what the player saw and when.
3. **Outcomes & unlock.** At the window's close the recipe's remaining
   outcomes apply: `DESTROY`/`SET`/`SPAWN` consume, mutate, or mint cards
   (the event's type decides which), `NARRATE` closes the scene, and the
   surviving cards unlock back to the table.

The window is **narrated, not simulated**: all outcomes still apply
atomically at commit (§6.6), and effects anchored to the clock (such as
`SET x, expires, …`) use the record's start clock. A `REFUSE` collapses the
cycle before any of it: no window elapses, no tick is due, and the attempt
is logged as in §6.5. `PROGRESS` belongs to verb recipes only — the script compiler rejects it in
trigger outcomes (events have no window). `REFUSE` belongs to verb recipes
too, for the same reason sharpened: a refusal aborts a *step* (§6.5), and an
event has no step to refuse — so `REFUSE` in any trigger outcome (both
forms) is a load-time error (§9.34).

**Why the engine has no lock machinery**: time is discrete per action
(§9.2) — committing a step advances the clock across its entire window, so
no other step can even begin inside it. The lock is the processing window
projected by the UI, guaranteed by serialized time rather than enforced by
flags. Concurrent verb stations (several windows in flight at once) remain
an open question (§12).

`PROGRESS(duration, string)` is journal-only, like `NARRATE`: no state
effect, no time effect, raw template stored and interpolated at render
(§5.3). Offsets are authored discipline — a tick past the window is harmless
but meaningless. A cadence ("every hour") is simply several authored ticks;
a real-time UI may schedule their display however it likes.

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
  kind         TEXT NOT NULL DEFAULT 'action',  -- 'action' | 'event'
  trigger      TEXT,                            -- event trigger id (events only; an authored `on expire` trigger's id, else 'expire:<NAME>')
  t_game       TEXT NOT NULL,                   -- start clock "day:halfhour", day 0-based
  t_real       TEXT,
  advanced_min INTEGER NOT NULL DEFAULT 0,      -- minutes this record advanced the clock (0 for refused/events)
  actors       JSON,                            -- refs: sorted, de-duplicated union across clauses (order not semantic)
  verb         TEXT NOT NULL,                   -- FIRST clause's verb lexeme ('' for event rows; kind column discriminates)
  objects      JSON,                            -- FIRST clause's object/with/at/about roles (multi-clause steps
  instruments  JSON,                            --   keep per-clause detail in `sentence` — it is lossless)
  at           TEXT,
  about        TEXT,
  to           TEXT,                            -- FIRST clause's `to` role (empty for event rows)
  outcomes     JSON,                            -- concrete outcomes (ids minted, sugar resolved, roles concretized)
  recipes      JSON,                            -- fired recipe ids, clause order
  sentence     TEXT NOT NULL                    -- canonical form, lossless
);
```

- **State = fold(log)** → free replay, free provenance (`born_action`),
  potential undo (re-fold without the tail). The fold replays each record's
  concrete outcomes; refused records are state no-ops; the clock is owned by
  action records alone (`t_game` + `advanced_min`, §6.6). A once-trigger's
  fired-ness is likewise a projection: the fold marks it from each EVENT
  row's `trigger` (§9.29).
- **The log is a working file, not a stable contract** (§9.30): it replays
  under *this* spec version only. When a change breaks old logs, re-author
  or migrate them by hand.
- **Journal view** = the `sentence` column in order — the game *is* a book.
- The shipped store is JSONL — one record per line (`saveLog`/`loadLog`); the
  SQL shape above is the queryable target when one is wanted.

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
hidden gates print nothing). Still a sketch: nothing renders stations in
v0.2.

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
| the outcomes at unlock (§4.4 — applied at commit, narrated at the window's close, §6.7) | the cards' fates: `DESTROY` consumed · `SPAWN` minted onto the table (`INTO` = which tray) · `MOVE` slides to another container · `SET` repaints the face · `REVEAL` turns a hidden card face-up · `RELATE`/`UNRELATE` re-lay the table (§4.2) · `NARRATE`/`PROGRESS` touch no card |
| `REFUSE` (§6.5) | the committed cards bounce back, no time passes, and the attempt is still journaled |
| a step rejected before logging — bad slots, no matching recipe, unresolvable outcome, clock rewind (§6.6, §9.19, §9.20) | the station refuses the drop outright: the table is untouched and nothing is journaled — distinct from `REFUSE`, which journals the attempt |
| an EVENT row (§5.1, §6.6) — clock-fired, no station | cards arriving and leaving on their own: an expiry consumes a card, a trigger's outcomes mint, mutate, or consume cards — journaled like any other record |
| the table projection (`Game.visible()`, §11) | what the player actually sees on the table: `container` = which tray/pile a card sits in, zones = groupings with no card (§10), N/S/E/W relations = where place cards sit (§4.2) |
| undo (§11) | a take-back: drop the trailing EVENT rows and the last action row, then re-fold — every card returns exactly where it was (§7) |
| `born_action` (§7) | a card's provenance — "entered play at action N" |

Two invariants keep the mapping safe:

- **Cards never mutate anything.** The card layer is L3, a projection over
  (state, log) (§3, §7): a drop event would call the same `executeStep`
  `handleLine` (§11) calls, and the canonical sentence — not the drop — is
  what is stored. There is no second, card-level rulebook to keep in sync.
- **The mapping is display-independent.** Text cards, drawn cards, or both at
  once are projections of the one log; the requirement-visibility (§6.2) and
  table-projection (§11) rules are display-only and change no matching,
  outcome, or record. The journal is the whole table session, replayable (§7).

---

## 9. Decisions

### Confirmed with the designer (v0.1)

1. **Player input**: UI slots — the player places cards into verb slots
   (Cultist Simulator style); the **engine composes the canonical sentence**
   and appends it to the log. No natural-language parsing. (v0.2 reality:
   there is no UI yet — the lenient play interface of §11 stands in for the
   slot UI, under the same invariant: it is a front door that maps onto the
   grammar; the logged sentence is always the canonical one.) §8.1 is the
   operation-level card mapping.
2. **Time model**: **discrete, per-action.** No background ticking.
3. **Outcomes**: **hand-authored recipes only.** Deterministic and testable.
4. **Prose layer**: afterthought. Canonical sentences are the deliverable.
5. **Timestamps**: recorded as `day:halfhour` (48 halfhours per day, one
   halfhour = 30 minutes), day starts at 0. `(day, halfhour)` is
   raw monotonic mechanics time — human-time framing is L3's business.
6. **Multiple actors**: the subject is an unordered set (`A, B TALK …` ≡
   `B, A TALK …`); clauses are separated by `;` so `,` stays free for lists.
   Actors are stored sorted — equality between actions is structural.
7. **Entity properties** = aspects (weighted, dot sub-keys) + attrs +
   relations. Knowledge is just aspects (`occult:3`); relations are typed
   edges, mutated only by `RELATE`/`UNRELATE`.
8. **Cardinality vs exceptions**: slots declare the general count rule
   (validated by the UI); *conditional* exceptions are REFUSE recipes.
   Refused attempts cost no time but are logged — failure is part of the story.
9. **Requirement visibility**: `shown`/`hidden` on slots and gate recipes,
   **display-only** (never affects matching or the log). Defaults: slots
   shown, gates hidden.

### Implementation rulings (v0.2, from spec review)

10. **`AT` position**: pre-verbal (`ANA AT HOUSE READ X`) is canonical —
    "scene, then action". Post-verbal `AT` is accepted input sugar.
11. **USE_ON**: one `object` role (the target: rite, lock, ingredient);
    instruments go in `with`. The old two-object form is gone.
12. **Time has one owner**: the engine advances the clock by clause durations;
    `ADVANCE` outcomes are authored extras only. Recipes never hand-write the
    verb's own duration.
13. **REFUSE is step-atomic** (draft discarded); no partial application.
14. **Names unique among alive entities**; canonical refs are bare names;
    `kind:name`/`#id` accepted as input sugar; reserved words must be quoted.
15. **Outcome arities are normative** (§4.4); commas inside parens belong to
    args. The engine writes minted ids (`AS #id`) and resolved sugar into the
    log, so the log replays exactly. Canonical output is paren-less for
    `SPAWN` and `NARRATE` only; input additionally accepts paren-less
    `DESTROY`/`REVEAL`/`MOVE`.
16. **EVENTs**: trigger registry lives in content (id + halfhour slot) alongside
    expiry; events get their own log rows; the fold trusts logged events and
    never re-derives them; no cascading within one advance.
17. **Structural flags are attrs, aspects are principles only** — one home
    each; `state:` queries read attrs.

### Implementation rulings (v0.2.1, spec–implementation alignment)

18. **Concrete outcomes are fully materialized**: role refs are concretized
    to `#id`s, and `SPAWN` gains its engine-written `INTO`, `AS #id`, and
    `aspects=`/`attrs=` JSON pairs — the fold replays from the record alone,
    never the step context (§5.2, §9.15).
19. **Application failures reject the whole step**: an outcome that cannot
    apply (unresolvable ref, duplicate name among the living, bad op args)
    rejects with nothing logged — unlike `REFUSE`, which always logs its
    attempt (§6.5).
20. **The clock is monotonic**: a step whose start clock is before
    `state.now` is rejected, nothing logged — §9.5 enforced by the engine.
21. **The fold clock is action-owned**: replaying a log sets the clock from
    action records only (`t_game` + `advancedMin`); EVENT rows never advance
    or rebase it, so a log ending past an event keeps the action's end time.
22. **Recipe ids default** to `<verb>.recipe<n>` / `<verb>.default` when a
    recipe omits its stable id (§6.4).
23. **Parameterized spawns are prototype derive rules** (`name from object`,
    `aspects from object.aspect-N`), not call-site sugar — minted names look
    like `CLUE_THE_FORGETTING` (§6.4).
24. **The `to` role is grammar-complete but content-free** in v0.2: the
    clause grammar and role refs accept it; no core verb declares a `to`
    slot yet (§12 open question).

### Implementation rulings (v0.2.2, the processing cycle)

25. **`PROGRESS` is journal-only** (§4.4, §6.7): canonical paren'd form
    `PROGRESS(30m, "…")`, recorded in emission order, arity `(duration,
    string)` enforced at application (§9.19); ticks never touch state and
    never advance the clock. Offsets count from the record's start clock;
    `PROGRESS` in trigger outcomes is a load-time error.
26. **The processing cycle needs no engine locks** (§6.7): serialized
    discrete time (§9.2) guarantees the verb station and its role cards are
    untouchable for the window; what happens at unlock is ordinary outcomes
    (destroyed / mutated / minted / returned).
27. **The `to` role reaches the record**: `ActionRecord` gains `to` (§7) and
    prose gains `{to}` and `{with}` tokens (§5.3). First content use is the
    Nightjar Flyer's interrogations (`MARCH TALK TO COLE ABOUT
    THE_AETHER_SEAL`); ruling 24 still holds for the *core* verb set.
28. **Step clocks are halfhour-quantized; the world is not** (found by the
    exploratory test): a step may never start before `state.now` (§9.5), and
    a sub-halfhour action (e.g. a 20m wait) leaves `now` mid-tick, so the
    next step resumes on the **next halfhour tick** — the lenient interface
    (§11) assigns exactly that. The dead air is real time: event resolution
    spans the whole elapsed interval (§6.6), so a tick's trigger fires even
    when no action window covers that instant. (v0.2.3: the tick shrank from
    the hour to the halfhour.)
29. **`once` triggers** (§6.6, §10): slot triggers repeat on every crossing
    of their halfhour-of-day unless authored `once`; a once-trigger fires on
    its first crossing only. Fired-ness lives in the state as a projection
    rebuilt from EVENT rows at fold — event-sourced, undo-safe.

### Implementation rulings (v0.2.3, the halfhour clock)

30. **No backward compatibility, deliberately** (also the header note):
    grammar, log and script formats, and API carry no stability guarantee
    across versions — v0.2.3 alone broke every pre-existing log by
    re-scaling `day:hour` to `day:halfhour`. The engine ships no migration
    shims and no version detection; when a change breaks old files,
    re-author or migrate them by hand.

### Implementation rulings (v0.2.5, SET arithmetic)

31. **SET deltas are logged as absolutes** (§4.4, §5.2): a signed integer
    value (`+1`, `-2`) is a delta on the entry's current value — an attr by
    default, the aspects map under an `aspect:`-prefixed key (`aspect:secret`,
    `aspect:language.latin`). A missing entry counts as 0; results may go
    negative (no clamping — a counter may read below zero); deltas are
    integers only (no duration deltas, no floats). The engine computes at
    execution time and writes the concrete absolute form into the log —
    `SET(DREAD, aspect:level, +1)` at level 2 logs `SET(DREAD, aspect:level, 3)`
    — so the fold replays literals and never redoes arithmetic (§9.15). A
    delta over a role ref applies per bound card and logs one concrete SET
    per card (`#id`s, each with its own result). Adding to a non-numeric
    entry is an application failure, not a silent no-op (§9.19). Deltas apply
    to the draft, so a later REFUSE in the same step discards them whole
    (§6.5); absolute values behave exactly as before.

### Implementation rulings (v0.2.6, authorable decay)

32. **Expiry triggers are authorable** (§6.6, §10): a trigger line
    `ID on expire of NAME -> OUTCOMES` fires when an entity named NAME
    expires, and its outcomes become the EVENT row's outcomes with
    `record.trigger = ID` — instead of the built-in `expire:<NAME>` destroy.
    The authored outcomes replace the built-in destroy *entirely*: if they
    leave the entity alive, it re-fires on every later crossing (expire
    deliberately, e.g. with `DESTROY NAME`, or give it a fresh `expires`).
    First matching trigger in declaration order wins. There is **no
    compile-time existence check** on NAME: decay targets are often spawned
    at runtime, so a wrong name surfaces as an inert trigger, not a load
    error. The expiry loop still works from the pre-pass expiry snapshot, so
    ordering is unchanged (authored time triggers first, then expiries;
    snapshot; no cascades within one advance) and an expiry outcome that
    cannot apply rejects the whole step per §9.19.
33. **REPLACE concretizes to plain SPAWN + DESTROY** (§4.4): the engine
    mints from the prototype into the dying card's current location — its
    `container` (place-contained or character-carried) or, for a zone-tagged
    card, its `zone` (§10) — the CS decayTo inheritance; *then* stamps the
    dying card's `deadAction` and releases its name, and
    writes the existing concrete forms — `SPAWN name [INTO #container] AS #id
    [aspects=…] [attrs=…]`, `DESTROY #id` — into the log. Zero new
    log-format surface; the fold replays the pair exactly (REPLACE always
    writes the `attrs=` pair, even empty — the pair is the authoritative
    materialization (§9.18), so the fold cannot grow a location the mint
    never had). The mint is by
    **plain prototype** (or a plain `kind:NAME` ref — everything SPAWN
    accepts without a clause): REPLACE has no clause, so prototype
    derivation (`name from object`, §9.23) is unavailable. Because spawn
    precedes destroy, `REPLACE X WITH X` — or any collision with a living
    entity — rejects the whole step with nothing logged (§9.19). (A replaced
    place's children keep pointing at the dead place id; re-parenting them is
    the author's job.)
34. **Trigger outcome guards and fired-set agreement**: `REFUSE` in any
    trigger outcome (both forms) is a load-time error — a refusal aborts a
    step and logs the attempt (§6.5), but an event has no step; the previous
    silent behavior was worse than undefined, it *diverged*: play applied
    the outcomes before the REFUSE and logged the row, while the fold's
    §6.5 no-op rule replayed nothing. And `state.fired` is now written by
    exactly one rule in both paths — one entry per EVENT row's `trigger`
    (authored id or `expire:<NAME>`) — fixing a pre-existing play/fold
    divergence where expiry rows were marked at fold but not at play.

## 10. The script format — a game as one text file

A whole game is one text file: meta lines, then section blocks of
sentence-like declarations. `#` starts a comment; blank lines are ignored.
`compileScript` parses and compiles it once, failing loudly at load time
(unknown kind, unregistered verb or relation type, bad grammar).

```
script: Murder on the Nightjar Flyer   # title
player: MARCH                          # default actor for lenient input (§11)
goal: unmask Lord Vane's killer …      # shown up front in the player interface
start: 0:42                            # initial clock

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

verbs:
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

- **Entity line**: `NAME is KIND [with aspect:level, …] [at|in PLACE]`
  (trailing `with`/`at`/`in` clauses in either order). Script ids are
  `s_<name>`; containers resolve by name after all entities are read.
  Names must be unique among the living (§9.14) and are written as-is —
  prefer `UPPER_SNAKE`.
- **Relation line**: `A is RELTYPE of B`, `RELTYPE` registered in the
  dictionary; symmetric/directed storage follows its definition (§4.2).
- **Verb lexemes are fixed by the dictionary**: a `verbs:` block configures
  slots, duration, and recipes for a *registered* verb (§4.3, §6.1); new
  verb lexemes are a code-level `Dictionary.registerVerb`, not a script
  declaration.
- **Recipe lines**: §6.4's script form (`[id] [hidden|shown [hint "…"]]
  [when COND] -> OUTCOMES` or `default -> OUTCOMES`).
- **Trigger lines** — two forms:
  - `ID [once] at (hour|halfhour) N -> OUTCOMES` — `hour` N is 0..23
    (slot 2N), `halfhour` N is 0..47. Fires on every crossing of that
    halfhour-of-day — or on the first crossing only with `once` (§9.29).
  - `ID on expire of NAME -> OUTCOMES` — fires when the entity named NAME
    expires (§9.32); NAME gets no compile-time existence check (the entity
    may be spawned at runtime), and its outcomes may reference NAME directly
    (names are unique among the living). With no such trigger, expiry
    destroys the card (§6.6).

## 11. The play interface — lenient input, the terminal UI, undo

A `Game` is script-compiled content + base state + the append-only log
(`Game.fromScript`). `handleLine` is one turn — pure enough to drive from
tests; `runTui` wraps it in an Ink terminal UI (a plain readline loop when
stdin or stdout is not a TTY).

**Lenient input** (`read the forgetting`) is a *front door*, not NLU (§2):
it maps onto the canonical grammar, and the engine-composed canonical
sentence is what gets logged (§9.1). Rules:

- **Canonical passthrough**: input starting `d:h @` or containing `->`
  goes through the strict parser unchanged (clock defaults to the world
  clock when absent).
- Otherwise: optional leading `d:h` clock (default: the **next halfhour
  tick** at or after the world clock, §9.28); the
  first dictionary verb token ends the actor list — two-token preposition
  forms like `use on` are recognized; tokens before the verb are actors,
  defaulting to the `player:` character.
- Role keywords `at | with | about | to` start role phrases; bare names
  before one are objects.
- Names resolve case-insensitively, underscores as spaces, articles
  dropped, longest match first (up to 4 tokens) among **alive** entities;
  unknown words fail with suggestions (prefix match, then edit distance ≤ 2).
- A `with X` phrase whose verb has no `with` slot routes X to the verb's
  real companion slot — `to` when defined and free, else the actors while
  they have room and X fits the slot query; anything else stays in `with`
  and the engine rejects the phrasing as typed.

**The player interface shows its options outright**: the goal declared by
the script's `goal:` meta line opens the session, a `try:` row suggests
concrete executable actions (`Game.suggestions()` — verb × card bindings
that fit the slots and light up a non-default non-REFUSE recipe, validated
through the lenient parser), the verb vocabulary and the cards sharing the
player's location stay visible, and the meta views are number buttons — look
(player's container + contents), cards (the visible table), inspect (a
card's face: kind, aspects, whereabouts, provenance), journal
(canonical), story (prose), undo, quit — backed by
`Game.look/cards/inspect/journal/story/undo`. Input completes verbs and card names
with tab.

**The table is a projection, not the world** (`Game.visible()`): places, the
player, cards sharing the player's location, cards the player carries, and
anything the log has named — through a role or a spawn/destroy outcome — are
on the table; everything else is out of sight until the player goes and
looks. The projection is computed per render, never stored: undo and replay
re-derive it for free (§7).

**Unknown characters are authored, not special-cased**: a suspect boards as a
masked twin (`THE_GUARD is character with duty:2, unmet:1 at GUARD_VAN`) and a
first-talk recipe swaps the mask for the named prototype
(`talk.meet_guard when to.aspect.unmet >= 1 AND to.aspect.duty >= 1 ->
DESTROY THE_GUARD, SPAWN BRACK INTO GUARD_VAN, NARRATE "\"Brack. Railway
security…\""`). Until that exchange the player sees the archetype, not the
name — engine state untouched, everything in the log (§9.30).

**Undo** = drop the trailing EVENT rows and the last action row, then
re-fold from the base state (§7). Rejections and parse errors log nothing,
so there is nothing to undo after them.

## 12. Remaining open questions

1. **Non-character subjects**: can any card be a subject for flavor events
   ("THE_PALE_LADY WHISPERS_TO ANA"), or only `character` kinds?
2. The player's container: a CS-like "table" where loose cards live — one bag,
   or pockets/locations?
3. The `to` role has its first content use (the Nightjar Flyer's
   interrogations, §9.27) but no core verb declares it yet (§9.24):
   recipient semantics (gifts, summons, threats) still await a core verb.
4. Script-level verb declaration: new verb lexemes currently require
   `Dictionary.registerVerb` in code (§10); should scripts declare them?
