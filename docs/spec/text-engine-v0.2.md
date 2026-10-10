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
   render cards without changing the data model.

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
`container` (where the card sits), `expires` (`+24h` style; the engine
materializes `expiresAt` = current clock + duration at set time), and the
boolean flag namespace `state.*`: `state.sealed`, `state.locked`,
`state.hidden`, `state.portable`, `state.unique`. Queries read them via
`state:sealed` / `state:!sealed` (§4.5); `SET`/`REVEAL` toggle them.

**Relations** are facts *between* two cards (the social graph): typed edges
declared in the dictionary as `symmetric` (`parent_child`) or `directed`
(`parent_of`). They change only via `RELATE`/`UNRELATE` outcomes and live in
their own table (§7). Condition predicates read them: `rel(parent_child, actors)`.

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
| `WORK`      | ACTORS WORK [AT place] [WITH item]     | labor → resources              |
| `TALK`      | ACTORS TALK [AT place] [ABOUT idea]    | social outcomes, group actions |
| `EXPLORE`   | ACTORS EXPLORE place                   | discover contents of a place   |
| `TRAVEL_TO` | ACTORS TRAVEL_TO place                 | deliberate movement            |
| `LOCATE_AT` | ACTORS LOCATE_AT place                 | set/dump position (utility)    |
| `DREAM`     | ACTORS DREAM [WITH idea]               | visions, rest                  |
| `USE_ON`    | ACTORS USE_ON target [WITH item]       | craft, rites, keys, tools      |
| `ACQUIRE`   | ACTORS ACQUIRE item                    | buy/gain via cost              |
| `WAIT`      | ACTORS WAIT                            | pass time (auto duration)      |

Custom verbs are declarable with the same shape — the core set is a default,
not a closed wall.

### 4.4 OUTCOME operators (fixed vocabulary, **normative arities**)

| Operator  | Args (normative)                | Effect                                        |
|-----------|---------------------------------|-----------------------------------------------|
| `SPAWN`   | ref [, INTO ref] [, AS #id]     | mint a new entity (from a prototype if one is defined); `AS #id` is engine-written so folds replay exactly |
| `DESTROY` | ref                             | card removed (deadAction stamped)             |
| `SET`     | ref, key, value                 | mutate attr (`state.sealed`, `container`, `expires`…) |
| `MOVE`    | ref, INTO ref                   | relocate (place→actor, actor→place)           |
| `REVEAL`  | ref                             | unlock hidden text / mark discovered          |
| `ADVANCE` | duration (`2h`, `90m`, `1d`)    | authored extra time shift (§9.12)             |
| `NARRATE` | string                          | journal line only; raw template stored, interpolated at render |
| `RELATE`  | ref, ref, type [, int]          | create a relation edge between two cards      |
| `UNRELATE`| ref, ref, type                  | remove a relation edge                        |
| `REFUSE`  | string                          | abort the whole step (§6.5): draft discarded, attempt still logged |

Shorthand: a bare ref in an outcome list means `SPAWN ref` (a **new** entity,
from its prototype if defined, into the step's `at` place or the first actor's
`container`). Moving an *existing* card is `MOVE`. Outcomes apply in order.

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
clock      = int ":" int ;                       (* "day:hour", day 0-based, hour 0..23 *)
action     = clause { ";" clause } [ "->" outcomes ] ;
event      = "EVENT" name [ "->" outcomes ] ;
clause     = actors [ "AT" ref ] verbo [ "TO" ref ] [ objects ] [ "WITH" reflist ] [ "ABOUT" ref ] ;
actors     = ref { "," ref } ;                   (* 1..n, order-free; canonical = sorted *)
objects    = ref { "," ref } ;
reflist    = ref { "," ref } ;
verbo      = VERB ;                              (* must be a verb registered in the dictionary *)
outcomes   = outcome { "," outcome } ;           (* commas inside ( ) belong to args *)
outcome    = OPERATOR "(" [ args ] ")" | ref ;   (* bare ref = SPAWN ref, engine adds AS #id *)
args       = arg { "," arg } ;                   (* arg shapes validated per §4.4 arity *)
arg        = ref | int | string | duration | "INTO" ref | "AS" "#" id | key "=" value ;
duration   = int ( "h" | "m" | "d" ) ;
ref        = name | kind ":" name | "#" id | string ;  (* canonical output: name *)
```

Lexical rules:

- `name` = `[A-Za-z][A-Za-z0-9_]*`. **Reserved words** (`EVENT TO WITH AT
  ABOUT INTO AND OR NOT` + every registered verb lexeme) cannot be used as
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

Time fires an event (own log row, at the crossing hour):

```
0:16 @ EVENT dusk_falls -> SPAWN DREAD, NARRATE "The lamps come on."
```

Rite via `USE_ON` with instruments (actors operate the rite together):

```
1:23 @ ANA, MY_FOLLOWER AT HOUSE USE_ON RITE_OF_ASCENSION WITH CHALICE
    -> DESTROY(CHALICE), SPAWN MARK_OF_ASCENSION INTO HOUSE
```

### 5.3 Prose rendering (L3 template, not stored truth)

Each verb owns a template using **role names**:

```
READ: "{actors} read {object} at {at}."
TALK: "{actors} talk about {about} at {at}."
```

→ *"Ana reads The Pale Lady at the Study — she finds a clue to the hideout."*
Actor sets join in prose as *"Ana and Bertha"*, following the canonical
(sorted) order. `NARRATE` outcomes store raw templates; interpolation happens
only here.

---

## 6. Engine layer — properties, slots, recipes (content model)

### 6.1 Slot cardinality: how many cards a role takes

A slot declares a **count**, an optional visibility flag, and a query every
member must satisfy. One slot per line:

```
slot  := role ":" count ["," ("shown"|"hidden")] ["," query]
count := int [ ".." [ int ] ]                    (* "1", "2", "1..2", "2..", "0..1" *)
flag  := "shown" (default) | "hidden"
```

```
verb READ:   slots: actors: 1..2, kind:character     # one or two readers
verb DREAM:  slots: actors: 1, kind:character        # strictly solo
verb USE_ON: slots: actors: 1..5, kind:character     # a coven may be needed
                    object: 1, kind:rite
                    with:    0..2, kind:item
```

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
pairwise     rel(parent_child, actors)          # ∃ a pair of actors with this relation
aggregate    total(actors, aspect:lantern) >= 3 # pooled power of the set
paths        total(actors, aspect:lantern) < object.aspect.demand   # value vs value
logic        AND · OR · NOT (uppercase; NOT binds tightest)
```

### 6.4 Recipes: first match wins, most specific first, **stable ids**

A recipe is an optional stable `id` (used for log provenance), `when` (or
`default`), `out`, and optional `shown`/`hint`:

```
recipe := ["id:" name] ("- when:" cond | "- default") "out:" outcomes
          ["shown:" true|false]  ["hint:" "text"]     (default: shown: false)
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
      out:   SPAWN CLUE(secret-2), NARRATE "{actors} tease out a hidden thread."

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

`SPAWN CLUE(secret-2)` is content sugar for a prototype-parameterized spawn:
the prototype `CLUE` is minted with `secret = object.aspect.secret - 2`. The
engine resolves it at execution and writes the concrete outcome (`SPAWN
CLUE_HIDEOUT AS #c1`) into the log — the log never contains sugar.

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
   Invalid → rejected at the UI, nothing logged.
2. **Selection**: for each clause in order, the verb's first recipe whose
   `when` holds fires (or `default`). No recipe → rejection, nothing logged.
3. **Application**: outcomes apply in order into a draft; REFUSE discards the
   draft (§6.5). Otherwise the draft becomes the new state.
4. **Time**: the engine advances the clock by the **sum of executed clauses'
   verb durations**. `t_game` on the record is the start clock. `ADVANCE`
   outcomes are authored *extra* shifts (WAIT recipes, night-skipping rites).
5. **Provenance**: the record stores the fired recipe ids (one per clause,
   clause order) and the canonical sentence.
6. **Events**: after the clock advances, the engine resolves, in
   chronological order: authored time triggers (dictionary content: id +
   hour) and entity expiries (`expiresAt` crossed). Each fires its own EVENT
   log row with its outcomes applied. One pass — events never cascade within
   the same advance (v0.2 simplification; new expiries resolve at the next
   advance). **The fold trusts logged EVENT rows** — it never re-derives them.

Perishables/decay v0.2: an entity may carry `expires:+24h`; the engine
materializes `expiresAt` when the attr is set; the crossing fires an expiry
EVENT whose outcomes destroy/mutate it.

---

## 7. Database schema (append-only log; projections derived)

The log is the source of truth. `entity`/`relation` are projections built by
fold (§6.6); a SQL shape is given for when a queryable store is wanted.

```sql
CREATE TABLE entity (
  id           TEXT PRIMARY KEY,      -- "#c7f3"
  kind         TEXT NOT NULL,         -- dictionary KIND
  name         TEXT NOT NULL,         -- unique among alive entities
  aspects      JSON,                  -- {"secret":4,"language":{"latin":2}}
  attrs        JSON,                  -- {"container":"#study","state.sealed":true,"expiresAt":"3:14"}
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
  id          INTEGER PRIMARY KEY,
  kind        TEXT NOT NULL DEFAULT 'action',  -- 'action' | 'event'
  trigger     TEXT,                            -- event trigger id (events only)
  t_game      TEXT NOT NULL,                   -- start clock "day:hour", day 0-based
  t_real      TEXT,
  actors      JSON,                            -- refs, stored sorted (order not semantic)
  verb        TEXT NOT NULL,                   -- verb lexeme ('' for event rows; kind column discriminates)
  objects     JSON,
  instruments JSON,
  at          TEXT,
  about       TEXT,
  outcomes    JSON,                            -- concrete outcomes (ids minted, sugar resolved)
  recipes     JSON,                            -- fired recipe ids, clause order
  sentence    TEXT NOT NULL                    -- canonical form, lossless
);
```

- **State = fold(log)** → free replay, free provenance (`born_action`),
  potential undo (re-fold without the tail).
- **Journal view** = the `sentence` column in order — the game *is* a book.

---

## 8. Rendering sketches (text now, drawable later)

Card (every field is from the dictionary — nothing ad hoc):

```
┌──────────────────────────────┐
│ THE PALE LADY          [text]│
│ secret:3 · language.latin:2  │
│ "The first page is missing…" │
└──────────────────────────────┘
```

Verb station (requirement badges per §6.2 — shown gates print their hint,
hidden gates print nothing):

```
▶ USE_ON  [ actors: 1..5 characters ]  [ object: a rite ]  [ with: 0..2 items ]
    ⚠ The rite wants pooled lantern — it will starve.        (shown gate)

▶ READ    [ actors: 1..2 characters ]  [ object: a text ]   (takes 1h)
                                                          (magick gate: hidden)
```

Journal: canonical sentences, or the prose templates — same data, two views.

---

## 9. Decisions

### Confirmed with the designer (v0.1)

1. **Player input**: UI slots — the player places cards into verb slots
   (Cultist Simulator style); the **engine composes the canonical sentence**
   and appends it to the log. No natural-language parsing.
2. **Time model**: **discrete, per-action.** No background ticking.
3. **Outcomes**: **hand-authored recipes only.** Deterministic and testable.
4. **Prose layer**: afterthought. Canonical sentences are the deliverable.
5. **Timestamps**: recorded as `day:hour`, day starts at 0. `(day, hour)` is
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
    log, so the log replays exactly.
16. **EVENTs**: trigger registry lives in content (id + hour) alongside
    expiry; events get their own log rows; the fold trusts logged events and
    never re-derives them; no cascading within one advance.
17. **Structural flags are attrs, aspects are principles only** — one home
    each; `state:` queries read attrs.

## 10. Remaining open questions

1. **Non-character subjects**: can any card be a subject for flavor events
   ("THE_PALE_LADY WHISPERS_TO ANA"), or only `character` kinds?
2. The player's container: a CS-like "table" where loose cards live — one bag,
   or pockets/locations?
