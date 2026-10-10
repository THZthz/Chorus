# Chorus Text Engine — Spec v0.1

A text-only prototype of a Cultist Simulator / Book of Hours-like card game.
Everything the game *is* — cards, actions, history — is expressed as structured
text ("sentences"). Sentences are canonical data: parseable, storable in a
database, replayable. Rendering (text cards now, drawn cards later) is a
projection of that data, never the source of truth.

---

## 1. Goals

1. **Text-only.** No images. Every card and action is a sentence or a text block.
2. **Recordable.** Every action is stored in a database with an in-game
   timestamp, in a form that can be parsed back (lossless round-trip).
3. **Closed vocabulary.** A dictionary of KINDS, ASPECTS, VERBS and OUTCOME
   operators. Content (books, characters, places) is *data* that uses this
   dictionary, not new syntax.
4. **Expressive enough** for the CS/BoH loop: verbs with slots, slot
   requirements, recipes (condition → outcome), time cost, decay, containers.
5. **Drawable later.** Each element has fixed, labeled fields, so a UI can
   render cards without changing the data model.

## 2. Non-goals (v0.1)

- Natural-language understanding of freeform player typing.
- Graphics or UI implementation.
- Game balance / actual content (separate content files).

---

## 3. Architecture: three layers

```
┌─────────────────────────────────────────────────────────┐
│ L3  Rendering    cards · verb stations · journal        │  ← projection
├─────────────────────────────────────────────────────────┤
│ L2  Sentences    "ANA AT STUDY READ THE_PALE_LADY -> …" │  ← canonical log
├─────────────────────────────────────────────────────────┤
│ L1  Dictionary   KINDS · ASPECTS · VERBS · OUTCOMES     │  ← ontology
└─────────────────────────────────────────────────────────┘
```

- **L1 Dictionary** defines what words exist.
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

### 4.2 ASPECTS (tags, the matching currency)

Two sorts:

- **Structural** — the engine knows their meaning:
  `portable`, `locked`, `sealed`, `perishable`, `unique`, `hidden`.
- **Principles** — game-defined, weighted (`aspect:level`):
  e.g. `lantern:2`, `winter:1`, `language:latin`, `secret:4`.

Aspects are how slots and recipes match cards (§7). The *set* of principles is
game content; the *mechanism* is engine.

### 4.3 VERBS (v1 core set)

A verb's lexeme may embed its preposition (matching your `LOCATE_AT` style).
Each verb declares which roles it needs:

Roles: `ACTORS` (subject) · `OBJECT` (direct) · `WITH` (instruments) ·
`AT` (where) · `TO` (recipient/destination) · `ABOUT` (topic).

**ACTORS is an unordered set** — one or more characters. `A, B TALK …` and
`B, A TALK …` are the *same* action. The engine composes refs in canonical
(sorted) order and the log stores them sorted, so equality is structural.

| Verb        | Signature                             | Purpose                        |
|-------------|---------------------------------------|--------------------------------|
| `READ`      | ACTORS READ OBJECT [AT place]         | study a text → ideas           |
| `WORK`      | ACTORS WORK [AT place] [WITH item]    | labor → resources              |
| `TALK`      | ACTORS TALK [AT place] [ABOUT idea]   | social outcomes, group actions |
| `EXPLORE`   | ACTORS EXPLORE place                  | discover contents of a place   |
| `TRAVEL_TO` | ACTORS TRAVEL_TO place                | deliberate movement            |
| `LOCATE_AT` | ACTORS LOCATE_AT place                | set/dump position (utility)    |
| `DREAM`     | ACTORS DREAM [WITH idea]              | visions, rest                  |
| `USE_ON`    | ACTORS USE_ON item OBJECT             | craft, rites, keys, tools      |
| `ACQUIRE`   | ACTORS ACQUIRE item                   | buy/gain via cost              |
| `WAIT`      | ACTORS WAIT                           | pass time (via ADVANCE outcome)|

Custom verbs are declarable with the same shape — the core set is a default,
not a closed wall.

### 4.4 OUTCOME operators (fixed vocabulary)

| Operator | Args                        | Effect                              |
|----------|-----------------------------|-------------------------------------|
| `SPAWN`  | ref-spec [, INTO container] | new card appears (hand, or a place) |
| `DESTROY`| ref                         | card removed                        |
| `SET`    | ref, attr, value            | mutate attribute (e.g. state:sealed→false) |
| `MOVE`   | ref, INTO container         | relocate (place→hand, hand→place)   |
| `REVEAL` | ref                         | unlock hidden text / mark discovered|
| `ADVANCE`| duration                    | move the in-game clock              |
| `NARRATE`| "text"                      | a journal line only; no state change|

Shorthand: a bare ref in an outcome list means `SPAWN` that ref into the
player's hand — so `-> CLUE_SECRET_HIDEOUT` works exactly as in your example.

### 4.5 Query mini-language (used by slots & recipes)

```
kind:text  aspect:secret>=3  state:!sealed  -aspect:winter
```

- `kind:` — exactly one kind
- `aspect:name[>=|<=|=]level` — principle match (bare `aspect:x` = level ≥ 1)
- `state:key[=|!]value` — attribute match
- `-` prefix — negation

---

## 5. L2 — Sentence grammar

### 5.1 EBNF

```ebnf
log        = { sentence } ;
sentence   = [ clock "@" ] ( action | event ) ;
clock      = day ":" hour ;                  (* day starts at 0: "0:14" *)
action     = clause { ";" clause } [ "->" outcomes ] ;
event      = "EVENT" trigger { ";" clause } [ "->" outcomes ] ;
clause     = actors verbo [ "TO" ref ] [ objects ] [ "WITH" reflist ]
             [ "AT" ref ] [ "ABOUT" ref ] ;
actors     = ref { "," ref } ;               (* ACTORS, 1..n, order-free *)
objects    = ref { "," ref } ;               (* role: OBJECT *)
reflist    = ref { "," ref } ;               (* role: WITH *)
outcomes   = outcome { "," outcome } ;
outcome    = operator "(" args ")" | ref ;   (* bare ref = SPAWN into hand *)
ref        = "#" id | kind ":" name | name ; (* "#c7f3" · "text:THE_PALE_LADY" · "ANA" *)
name       = token | """ string """ ;
```

- `;` separates **clauses** within one player step (several atomic verbs
  committed as one log entry, e.g. walk somewhere, then act there). `,` is
  reserved for ref lists — this is what keeps multi-actor subjects unambiguous.
- **ACTORS is an unordered set**: `A, B TALK AT DINNING_HALL ABOUT PLAN` and
  `B, A TALK AT DINNING_HALL ABOUT PLAN` are the same action. The log stores
  the actor list sorted, so the two forms are byte-identical when recorded.
- `EVENT` sentences are outcomes of the clock (time fires recipes), not
  initiated by the player.

### 5.2 Worked examples

Your example, canonicalized (`LOCATE_AT` becomes the context phrase `AT` —
location is *where*, not a second verb):

```
0:14 @ ANA AT HOUSE READ text:THE_FORGETTING -> CLUE_SECRET_HIDEOUT
```

Two characters, order-free — both lines record identically:

```
2:19 @ ANA, BERTHA TALK AT DINNING_HALL ABOUT CLUE_SECRET_HIDEOUT
    -> SPAWN idea:PLAN_INFILTRATION, ADVANCE(1h)

2:19 @ BERTHA, ANA TALK AT DINNING_HALL ABOUT CLUE_SECRET_HIDEOUT
    -> SPAWN idea:PLAN_INFILTRATION, ADVANCE(1h)
```

One step, two atomic verbs (travel then act), one log entry:

```
0:14 @ ANA TRAVEL_TO VILLA; ANA AT STUDY READ text:THE_PALE_LADY
    -> CLUE_SECRET_HIDEOUT, ADVANCE(2h)
```

Time fires an event:

```
0:16 @ EVENT dusk_falls -> SPAWN state:DREAD, NARRATE "The lamps come on."
```

Rite via `USE_ON` with instruments:

```
1:23 @ ANA USE_ON rite:RITE_OF_ASCENSION MY_FOLLOWER WITH item:CHALICE AT HOUSE
    -> DESTROY(item:CHALICE), SPAWN idea:MARK_OF_ASCENSION INTO place:HOUSE
```

### 5.3 Prose rendering (L3 template, not stored truth)

Each verb owns a template:

```
READ: "{actors} read {object} at {place}."
TALK: "{actors} talk about {topic} at {place}."
```

→ *"Ana reads The Pale Lady at the Study — she finds a clue to the hideout."*
Actor sets join in prose as *"Ana and Bertha"*, following the canonical
(sorted) order.

The database stores the canonical sentence; prose is generated on demand
(and localizable).

---

## 6. Engine layer — verbs, slots, recipes (content model)

A verb definition (game data, not code):

```
verb READ:
  slots:
    actors: kind:character          # a set; every member must satisfy the query
    object: kind:text  state:sealed!
  at:       optional, aspect:study      # some places read better
  duration: 1h
  recipes:
    - when:   object.aspect.secret >= 3
      out:    SPAWN idea:CLUE(secret-2 level), NARRATE "{actors} tease out a hidden thread."
    - when:   at.aspect.study >= 1
      out:    NARRATE "{actors} study {object} carefully."
    - default:
      out:    NARRATE "{actors} skim {object} without result."
```

Execution model (CS-shaped):
1. Player fills a verb's slots with cards (queries validate). The ACTORS slot
   holds a set; recipes may pin cardinality (`actors: 2` for a séance) and
   every member must satisfy the slot query.
2. Engine picks the first matching recipe.
3. Recipe consumes the verb's duration, applies outcomes in order, and the
   whole step is appended to the log as canonical sentences.

Perishables/decay v0.1: an entity may carry `expires: +24h`; when `ADVANCE`
crosses it, the engine emits an `EVENT` whose outcomes destroy/mutate it.

---

## 7. Database schema (append-only log + entity table)

```sql
CREATE TABLE entity (
  id           TEXT PRIMARY KEY,      -- "#c7f3"
  kind         TEXT NOT NULL,         -- dictionary KIND
  name         TEXT NOT NULL,
  aspects      JSON,                  -- {"secret":4,"language":"latin"}
  attrs        JSON,                  -- {"container":"#study","state":"sealed"}
  born_action  INTEGER,               -- action id that created it
  dead_action  INTEGER                -- action id that destroyed it (NULL = alive)
);

CREATE TABLE action (
  id          INTEGER PRIMARY KEY,
  t_game      TEXT NOT NULL,          -- in-game clock "day:hour", day 0-based
  t_real      TEXT,                   -- wall clock (optional, debugging)
  actors      JSON,                   -- refs, stored sorted (order is not semantic)
  verb        TEXT NOT NULL,
  objects     JSON,
  instruments JSON,
  at          TEXT,                   -- place ref
  about       TEXT,                   -- topic ref (TALK etc.)
  outcomes    JSON,
  sentence    TEXT NOT NULL           -- canonical form, lossless
);
```

- **State = fold(log)** → free replay, free "why does this exist?" provenance
  (`born_action`), potential undo (re-fold without the tail).
- **Journal view** = the `sentence` column in order — the game *is* a book.

---

## 8. Rendering sketches (text now, drawable later)

Card (every field is from the dictionary — nothing ad hoc):

```
┌──────────────────────────────┐
│ THE PALE LADY          [text]│
│ readable · secret:3 · latin  │
│ "The first page is missing…" │
└──────────────────────────────┘
```

Verb station:

```
▶ READ   [ actors: ______ ]  [ object: ______ ]   (takes 1h)
```

Journal: canonical sentences, or the prose templates — same data, two views.

---

## 9. Decisions (confirmed)

1. **Player input**: UI slots — the player places cards into verb slots
   (Cultist Simulator style); the **engine composes the canonical sentence**
   and appends it to the log. No natural-language parsing.
2. **Time model**: **discrete, per-action.** Each action advances the clock by
   its duration. No background ticking; decay and events resolve at action
   boundaries via `ADVANCE`.
3. **Outcomes**: **hand-authored recipes only.** Deterministic and testable.
   Consequence: the recipe-matching engine (§6) is the core interpreter to build.
4. **Prose layer**: afterthought. Canonical sentences are the deliverable;
   templates get filled in per verb later.
5. **Timestamps**: recorded as `day:hour`, day starts at 0 (`0:14` = noon of
   the first day). Durations spill into the next day.
6. **Multiple actors**: the subject is an unordered set (`A, B TALK …` ≡
   `B, A TALK …`); clauses are separated by `;` so `,` stays free for lists.
   Actors are stored sorted — equality between actions is structural.

## 10. Remaining open questions

1. **Non-character subjects**: can any card be a subject for flavor events
   ("THE_PALE_LADY WHISPERS_TO ANA"), or only `character` kinds?
2. The player's container: a CS-like "table" where loose cards live — one bag,
   or pockets/locations?
