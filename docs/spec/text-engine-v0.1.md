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

Roles: `ACTOR` (subject) · `OBJECT` (direct) · `WITH` (instruments) ·
`AT` (where) · `TO` (recipient/destination).

| Verb        | Signature                             | Purpose                        |
|-------------|---------------------------------------|--------------------------------|
| `READ`      | ACTOR READ OBJECT [AT place]          | study a text → ideas           |
| `WORK`      | ACTOR WORK [AT place] [WITH item]     | labor → resources              |
| `TALK_WITH` | ACTOR TALK_WITH character [AT place]  | social outcomes                |
| `EXPLORE`   | ACTOR EXPLORE place                   | discover contents of a place   |
| `TRAVEL_TO` | ACTOR TRAVEL_TO place                 | deliberate movement            |
| `LOCATE_AT` | ACTOR LOCATE_AT place                 | set/dump position (utility)    |
| `DREAM`     | ACTOR DREAM [WITH idea]               | visions, rest                  |
| `USE_ON`    | ACTOR USE_ON item OBJECT              | craft, rites, keys, tools      |
| `ACQUIRE`   | ACTOR ACQUIRE item                    | buy/gain via cost              |
| `WAIT`      | ACTOR WAIT duration                   | let time pass                  |

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
sentence   = [ timestamp "@" ] ( action | event ) ;
action     = clause { "," clause } [ "->" outcomes ] ;
event      = "EVENT" trigger { "," clause } [ "->" outcomes ] ;
clause     = subject verbo [ "TO" ref ] [ objects ] [ "WITH" reflist ] [ "AT" ref ] ;
subject    = ref ;                       (* role: ACTOR *)
objects    = ref { "," ref } ;           (* role: OBJECT *)
reflist    = ref { "," ref } ;           (* role: WITH *)
outcomes   = outcome { "," outcome } ;
outcome    = operator "(" args ")" | ref ;   (* bare ref = SPAWN into hand *)
ref        = "#" id | kind ":" name | name ; (* "#c7f3" · "text:THE_PALE_LADY" · "ANA" *)
name       = token | """ string """ ;
```

- A comma-separated action is **one player step** compiled from several atomic
  verbs (e.g. walk somewhere, then act there) and committed as one log entry.
- `EVENT` sentences are outcomes of the clock (time fires recipes), not
  initiated by the player.

### 5.2 Worked examples

Your example, canonicalized (`LOCATE_AT` becomes the context phrase `AT` —
location is *where*, not a second verb):

```
2026-04-05T14:00 @ ANA AT HOUSE READ text:THE_FORGETTING -> CLUE_SECRET_HIDEOUT
```

One step, two atomic verbs (travel then act), one log entry:

```
2026-04-05T14:00 @ ANA TRAVEL_TO VILLA, ANA AT STUDY READ text:THE_PALE_LADY
    -> CLUE_SECRET_HIDEOUT, ADVANCE(2h)
```

Time fires an event:

```
2026-04-05T16:00 @ EVENT dusk_falls -> SPAWN state:DREAD, NARRATE "The lamps come on."
```

Rite via `USE_ON` with instruments:

```
2026-04-06T23:00 @ ANA USE_ON rite:RITE_OF_ASCENSION MY_FOLLOWER WITH item:CHALICE AT HOUSE
    -> DESTROY(item:CHALICE), SPAWN idea:MARK_OF_ASCENSION INTO place:HOUSE
```

### 5.3 Prose rendering (L3 template, not stored truth)

Each verb owns a template:

```
READ: "{actor} reads {object} at {place}."
```

→ *"Ana reads The Pale Lady at the Study — she finds a clue to the hideout."*

The database stores the canonical sentence; prose is generated on demand
(and localizable).

---

## 6. Engine layer — verbs, slots, recipes (content model)

A verb definition (game data, not code):

```
verb READ:
  slots:
    actor:  kind:character
    object: kind:text  state:sealed!
  at:       optional, aspect:study      # some places read better
  duration: 1h
  recipes:
    - when:   object.aspect.secret >= 3
      out:    SPAWN idea:CLUE(secret-2 level), NARRATE "{actor} teases out a hidden thread."
    - when:   at.aspect.study >= 1
      out:    NARRATE "{actor} studies {object} carefully."
    - default:
      out:    NARRATE "{actor} skims {object} without result."
```

Execution model (CS-shaped):
1. Player fills a verb's slots with cards (queries validate).
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
  t_game      TEXT NOT NULL,          -- in-game timestamp, ISO-8601
  t_real      TEXT,                   -- wall clock (optional, debugging)
  actor       JSON,                   -- refs
  verb        TEXT NOT NULL,
  objects     JSON,
  instruments JSON,
  at          TEXT,                   -- place ref
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
▶ READ   [ actor: ______ ]  [ object: ______ ]   (takes 1h)
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

## 10. Remaining open questions

1. **Subjects**: only `character` may be ACTOR (assumed), or can any card be a
   subject ("THE_PALE_LADY WHISPERS_TO ANA") for flavor events?
2. Timestamps: ISO in-game datetime (assumed) vs integer ticks?
3. The player's container: a CS-like "table" where loose cards live — one bag,
   or pockets/locations?
