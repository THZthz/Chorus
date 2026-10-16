# Chorus Text Engine — Spec v0.6

A text-only prototype of a Cultist Simulator / Book of Hours-like card game.
Everything the game *is* — cards, actions, history — is expressed as structured
text ("sentences"). Sentences are canonical data: parseable, storable in a
database, replayable. Rendering (text cards now, drawn cards later) is a
projection of that data, never the source of truth.

> **Compatibility: none, by design.** Nothing in this document is a stability
> guarantee — the grammar, the log and script formats, and the API may break
> at any version boundary (v0.2.2 changed event semantics; v0.2.3 re-scaled
> the clock). Old logs are not replay contracts: re-author or migrate them
> yourself; the engine ships no migration path (§9.30). The established
> architecture is not frozen either: it may be changed when there is need —
> but the spec must be amended in the same change, so this document always
> describes the architecture as it actually is.

The numbered implementation rulings — the spec's decision ledger, §9.1–§9.50 —
and the full version-by-version changelog live in
[rulings.md](rulings.md). Every §9.x citation in comments, tests, and tools
resolves there, unchanged.

Version arc: v0.2 formalized the grammar and opened the ruling ledger;
v0.2.x added the alignment review, the processing cycle, the halfhour clock,
the card mapping, SET arithmetic, authorable decay, and endings; v0.3.x
rolled up the twelve core verbs, stackable cards, conditionals, functions,
seeded dice, decks, and the CS five; v0.4 completed the Cultist vocabulary
and added compound duration literals; v0.5 made the processing window real
(parallel verb stations, BEGIN/COMPLETE/WAIT rows); v0.6 landed the
engine-findings fixes (authored `SPAWN … expires`, the RUN `when:`
projection).

**Contents**: §1 [Goals](#1-goals) · §2 [Non-goals](#2-non-goals-v02) · §3 [Architecture](#3-architecture-three-layers) · §4 [L1 — Dictionary](#4-l1--dictionary) · §5 [L2 — Sentence grammar](#5-l2--sentence-grammar) · §6 [Engine layer](#6-engine-layer--properties-slots-recipes-content-model) · §7 [Database schema](#7-database-schema-append-only-log-projections-derived) · §8 [Rendering sketches](#8-rendering-sketches-text-now-drawable-later) · §9 [Decisions](rulings.md) · §10 [The script format](#10-the-script-format--a-game-as-one-text-file) · §11 [The play interface](#11-the-play-interface--lenient-input-the-terminal-ui-undo) · §12 [Remaining open questions](#12-remaining-open-questions)

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
(any §5.1 duration form — `2h`, `90m`, `1d`, `2halfhour`, compound `1h30m`
(§9.45) — quoted or bare;
the engine materializes `expiresAt` = current clock +
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
| `STUDY`     | ACTORS STUDY object [AT place]         | the CS study station (READ's signature, v0.3) |
| `WORK`      | ACTORS WORK [AT place]                 | labor → resources              |
| `TALK`      | ACTORS TALK [AT place] [ABOUT idea]    | social outcomes, group actions |
| `EXPLORE`   | ACTORS EXPLORE place                   | discover contents of a place   |
| `MANSUS`    | ACTORS MANSUS place                    | enter a place — the Mansus crossing (EXPLORE's signature, v0.3) |
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
| `SPAWN`   | ref [, INTO ref] [, AS #id] [, expires duration] [, k=v …] | mint a new entity (from a prototype if one is defined); `AS #id` is engine-written so folds replay exactly; `aspects=`/`attrs=` JSON pairs are engine-written materialization (§9.18); the authored `expires <duration>` modifier arms the mint at birth with the SET-expires pair (§9.36), so an authored outcome can decay the card it just minted even when the name pools — the concrete row carries the pair inside its `attrs=` materialization (§9.18) |
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
| `ENDING`  | id                              | end the story (§9.35): sets `state.ended` to `{id, title, flavour}` resolved from the `endings:` table (§10); the id must be declared — load-time error otherwise (recipe or trigger); the first ENDING to apply in a step wins, later ones are logged no-ops; the step itself completes normally — outcomes apply and the clock advances, the ending rides along |
| `IF`      | cond `->` `(` outcomes `)` { `ELSE IF` cond `->` `(` outcomes `)` } [ `ELSE` `->` `(` outcomes `)` ] | branching (§6.8, §9.39): arms evaluate in order against the draft, first true condition wins, `ELSE` is the default, no true arm and no `ELSE` applies nothing; the log records only the TAKEN branch's outcomes, flattened in position — no IF ever appears in the log |
| `ROLL`    | count, sides [, bonus]          | roll dice (§9.41): paren-only; each argument is a §6.3 value expression (`3`, `total(actors, aspect:nerve)`, `count(name:X)`, `object.aspect.dc`, …). Rolls `count` dice of `sides` faces plus `bonus`, consuming exactly `count` rng counters; binds `roll` for the rest of the outcome list. The log records the engine-written `ROLLED(<total>, <ctrAfter>)` — no ROLL ever appears in the log. `sides` < 2 or `count` < 0 reject the whole step (§9.19); `count` = 0 is legal (total = bonus, counter unmoved) |
| `ROLLED`  | total, ctrAfter                 | **engine-written only** (§9.41): the record of one ROLL — the fold sets `state.rng = ctrAfter` and derives nothing else. Authoring one is a load error |
| `DRAW`    | deck [, INTO target]            | draw the next card of a `decks:` declaration (§9.42): paren-less, the deck id is a plain name (a declared-deck check runs at load, over recipes and triggers, IF branches included); INTO is the SPAWN target grammar, and must RESOLVE — a role over empty event bindings or a dead ref rejects the step rather than materializing the card nowhere. With no INTO the mint reuses SPAWN's default (the step's `at` card, else the first actor's `container`; in triggers, where bindings are empty, the card is minted containerless unless an explicit INTO is authored). The deck entry may name a prototype or a declared entity; repeats in the spec weight the draw. The log records the engine-written `DRAWN(<DECK>, <name>)` + the concrete `SPAWN … AS #id` pair — no DRAW ever appears in the log |
| `DRAWN`   | deck, name                      | **engine-written only** (§9.42): the record of one DRAW — the fold advances that deck's cursor, asserting `name` is exactly what the compiled permutation serves at the cursor (content drift between play and fold fails loud). Authoring one is a load error |
| `RUN`     | `"recipe-id"` `, …`             | chain the completion (§9.47): walk the candidates in order, pin the first whose `when:` matches the post-outcome draft (lenient, §6.4) and whose `max` (§9.50) is unspent, and open it as a follow-up window on its verb with the parent's bindings projected onto the candidate verb's declared roles; no match — the chain ends silently; zero-concrete (the follow-up's BEGIN row is the only record). In recipe bodies: same verb, load-checked; in `on complete` bodies: cross-verb legal (global id table), the parent's own station is not busy. A second RUN op in one completion rejects the window |
| `REFUSE`  | string                          | abort the whole step (§6.5): draft discarded, attempt still logged |

Shorthand: a bare ref in an outcome list means `SPAWN ref` (a **new** entity,
from its prototype if defined, into the step's `at` place or the first actor's
`container`). Moving an *existing* card is `MOVE`. Outcomes apply in order.

**Role refs are valid outcome args**: any ref position may name a role —
`ACTORS`, `OBJECT`, `WITH`, `AT`, `ABOUT`, `TO` (case-insensitive; rendered
uppercase) — and expands to that role's bound cards (`DESTROY WITH`,
`MOVE ACTORS INTO OBJECT`, `SPAWN MARK INTO at`). Role refs are concretized to
`#id`s in the logged outcome (§9.15, §9.18).

**Declared functions may head an outcome position**: `NAME(arg, …)` dispatches
to a `functions:` declaration and expands to its body AT LOAD — one vocabulary,
two namespaces of convenience: functions are not ops (§4.4's table is closed),
declaring one named like an op is a load error (§9.40, §10).

### 4.5 Query mini-language (slot queries — one card at a time)

```
kind:text  aspect:secret>=3  aspect:language.latin>=2  state:!sealed  -aspect:winter
name:SUSPICION  -name:SUSPICION        (§9.38: exact-case, like name refs and exists)
```

- `kind:` — exactly one kind
- `aspect:name[.sub][>=|<=|=]level` — principle match (bare `aspect:x` = level ≥ 1)
- `state:key[=|!]value` — attr match against the `state.*` namespace (`state:!sealed` → `attrs['state.sealed']`)
- `name:NAME` — exact-case name match (§9.38); composes with the other terms,
  and `-name:x` negates like any term
- `-` prefix — negation

Slot queries test **one card at a time**. Anything about a *group* (size,
relations between members, pooled power) is a recipe condition (§6.3) — that
is deliberate: the UI can grey-out invalid drops from slots alone, while
group logic stays narrative.

Alternation (v0.3.6, §9.49): `|` separates conjunction groups —
`aspect:ability | aspect:job aspect:ritual` matches ability, or job AND
ritual — with every term form unchanged; legal wherever a query parses
(slots, `exists()`, `count()`).

---

## 5. L2 — Sentence grammar

### 5.1 EBNF (canonical form)

```ebnf
log        = { sentence } ;
sentence   = [ clock "@" ] ( action | event ) ;
clock      = int ":" int ;                       (* "day:halfhour", day 0-based, halfhour 0..47; 1 halfhour = 30 min *)
action     = clause { ";" clause } [ "->" outcomes ] ;
event      = "EVENT" trigger-id [ "->" outcomes ] ;
trigger-id = name { ":" name }                    (* segments reassembled — the
                                                     built-in expiry id is `expire:<NAME>`
                                                     (§6.6); an unsafe segment prints quoted *)
clause     = actors [ "AT" ref ] verbo [ "TO" ref ] [ objects ] [ "WITH" reflist ] [ "ABOUT" ref ] [ "AT" ref ] ;
(*          the trailing "AT" is input sugar only (§9.10); canonical output is pre-verbal *)
actors     = ref { "," ref } ;                   (* 1..n, order-free; canonical = sorted *)
objects    = ref { "," ref } ;
reflist    = ref { "," ref } ;
verbo      = VERB ;                              (* must be a verb registered in the dictionary *)
outcomes   = outcome { "," outcome } ;           (* commas inside ( ) belong to args *)
outcome    = OPERATOR "(" [ args ] ")"           (* SET/RELATE/UNRELATE/REFUSE/ADVANCE/PROGRESS: parens required *)
           | "SPAWN" ref [ "INTO" target ] [ "AS" "#" id ] [ "expires" duration ] { key "=" value }
                                                 (* expires: the authored mint-decay modifier (§4.4, §9.36) —
                                                    any §5.1 duration form, bare or quoted; consumed into the
                                                    attrs= materialization (§9.18), never logged itself. The bare
                                                    ref sugar stays one ref — the modifier rides the SPAWN op *)
           | ( "DESTROY" | "REVEAL" | "MOVE" ) parenlessTail      (* input sugar *)
           | "REPLACE" ( ref | role ) "WITH" ref (* paren-less only; WITH is syntax, not a role *)
           | "NARRATE" string
           | "ENDING" ref                        (* paren-less only; names an `endings:` entry (§9.35) — a name ref, quoted ids round-trip; existence is a load-time check *)
           | "IF" cond "->" "(" outcomes ")"
             { "ELSE IF" cond "->" "(" outcomes ")" }
             [ "ELSE" "->" "(" outcomes ")" ]    (* §6.8, §9.39: bodies REQUIRE parens; the ELSE chain is consumed greedily by the IF — a nested ELSE binds to the nearest IF; cond is the §6.3 grammar, read from the token stream up to the depth-0 "->" *)
           | "ROLL" "(" valexpr "," valexpr [ "," valexpr ] ")"
                                                 (* §9.41: paren-only; each argument is a §6.3 cmp-operand value expression (int, count(role), count(<query>), total(role, aspect:path), role.aspect.x, role.attr.x — a body parameter inside a function template), split on depth-0 commas so nested parens survive; arity 2..3 *)
           | "ROLLED" "(" int "," int ")"        (* §9.41: ENGINE-WRITTEN only — the recorded roll; authoring one is a load error *)
           | "DRAW" name [ "INTO" target ]       (* §9.42: paren-less only — the deck id is a plain name ref (declared decks are pinned at load); INTO is the SPAWN target grammar and must resolve *)
           | "DRAWN" "(" name "," name ")"       (* §9.42: ENGINE-WRITTEN only — the recorded draw, `DRAWN(<DECK>, <name>)`; an unsafe name formats re-quoted and re-parses (ref's string form); authoring one is a load error *)
           | ref ;                               (* bare ref = SPAWN ref, engine adds AS #id *)
(*          canonical OUTPUT: SPAWN, NARRATE, REPLACE and ENDING paren-less, every other op paren'd
            — e.g. SPAWN X INTO #house AS #c1 · NARRATE "…" · MOVE(#ana, INTO #study) · ROLLED(14, 3) · DRAWN(RUMOURS, HEARSAY) *)
args       = arg { "," arg } ;                   (* arg shapes validated per §4.4 arity *)
arg        = ref | role | int | signedInt | string | bool | duration
           | "INTO" target | "AS" "#" id | key "=" value | dottedName | aspectKey ;
signedInt  = ( "+" | "-" ) int ;                 (* SET delta value only (§9.31) *)
aspectKey  = "aspect:" name { "." name } ;       (* SET key into the aspects map (§9.31) *)
target     = ref | role ;
role       = "ACTORS" | "OBJECT" | "WITH" | "AT" | "ABOUT" | "TO" ;  (* case-insensitive *)
dottedName = name { "." name } ;                 (* SET(x, state.sealed, true) *)
duration   = int ( "halfhour" | "h" | "m" | "d" ) { int ( "halfhour" | "h" | "m" | "d" ) } ;
                                                 (* 1halfhour = 30 minutes; compound
                                                    literals like `1h30m` sum their pairs (§9.45) *)
ref        = name | kind ":" name | "#" id | string ;  (* canonical output: name *)
```

Lexical rules:

- `name` = `[A-Za-z_][A-Za-z0-9_]*`. **Reserved words** (`EVENT TO WITH AT
  ABOUT INTO AS AND OR NOT IF ELSE ROLL ROLLED DRAW DRAWN` + every registered
  verb lexeme) cannot be used as names; content needing them uses the quoted-string
  ref form (`"PLAN B"`).
- **Entity names are unique among alive entities** (enforced at SPAWN /
  registration) — except cards minted from a `stackable` declaration, which
  pool under one name (§9.38). Canonical ref output is the bare name;
  `kind:NAME` and `#id` are accepted input forms (sugar; typed refs are
  kind-checked at resolution). A name ref under a pool addresses the OLDEST
  copy (§9.38).
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

A slot declares a **role**, a **count**, an optional visibility flag, and a
query every member must satisfy. One slot per line (script syntax, §10):

```
slot  := role ":" count [ query ]
role  := "actors" | "object" | "with" | "at" | "about" | "to"   (* §4.3's six roles, exact lowercase *)
count := int [ ".." [ int ] ]                    (* "1", "2", "1..2", "2..", "0..1" *)
```

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
             exists(name:SUSPICION)                       # ∃ a living copy of the name (§9.38)
aggregate    total(actors, aspect:lantern) >= 3 # pooled power of the set
             count(name:SUSPICION) >= 3         # the whole living pool, not a role (§9.38)
paths        total(actors, aspect:lantern) < object.aspect.demand   # value vs value
             count(actors) > object.attr.seats                      # attr paths too
logic        AND · OR · NOT (uppercase; NOT binds tightest)
comparisons  >= · <= · > · < · = · !=
```

v0.2 limits: path operands read `role.aspect.key` and `role.attr.key`
(numeric attrs); the scalar conditions inside `every`/`any` read **aspects
only** (`aspect:key >= n`). A missing numeric attr entry on a bound card
compares as 0 (§9.39); a missing aspect on a bound card reads false.

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
  exact, §9.14; a destroyed card stamps `deadAction`, releases its name, and
  stops existing).
- `exists(<query>)` — an alive entity matches the §4.5 slot query: a
  conjunction of `kind:` / `aspect:` / `state:` / `name:` terms with `-`
  negation.
- Disambiguation: if the first token inside the parens is a query-term
  starter (`kind:` / `aspect:` / `state:` / `name:` / a leading `-`), the
  contents parse as a §4.5 query; otherwise they parse as a single NAME.
  Spaces around a comparison inside a query are glue: `aspect:hunter >= 1`
  reads as `aspect:hunter>=1` (§4.5 terms are whitespace-separated).

**`count(...)` is overloaded the same way** (§9.38): `count(role)` counts the
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
one predicate language, two disciplines (§9.39): a `when:` reads an
unresolvable operand as false, an IF condition rejects the step.

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

### 6.7 The processing cycle (begin → lock → ticks → complete → unlock)

Committing cards to a verb station is not the whole action: the action
*processes* over the verb's duration, and the engine narrates that
processing. Since v0.5 the window is **real**: several stations run in
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
   from the window table via the row's `beginId`, the same source play
   used, so the fold never re-derives them from a display clock.

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
completion analog of §9.19).

**Time is explicit.** Beginning at `now` moves nothing; the clock advances
only through advance passes — a bare clock input, an action whose typed
clock lies ahead, WAIT/ADVANCE — and each pass resolves due completions
first (ordered by end minute, then begin row, then clause), then the
halfhour's slot events (§6.6), completions at minute m preceding slot-m
events. The pass ends in a WAIT row, the only record that moves `now`,
whose spans partition time. One pass is atomic (§9.19); expiries created
by the pass resolve at the next one.

**Why there is no lock-flag machinery beyond the windows table**: the
table *is* the lock — play and fold rebuild it identically from BEGIN and
COMPLETE rows (§7), and every busy check is a lookup in it. Concurrent
verb stations, left open in earlier revisions, are the model since v0.5.

`PROGRESS` belongs to verb recipes only — the script compiler rejects it in
trigger outcomes (events have no window). `REFUSE` belongs to verb recipes
too, for the same reason sharpened: a refusal aborts a *step* or a window,
and an event has neither — so `REFUSE` in any trigger outcome (both forms)
is a load-time error (§9.34).

`PROGRESS(duration, string)` is journal-only, like `NARRATE`: no state
effect, no time effect, raw template stored and interpolated at render
(§5.3). Offsets are authored discipline — a tick past the window is harmless
but meaningless. A cadence ("every hour") is simply several authored ticks;
a real-time UI may schedule their display however it likes.

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
(§9.39).

**Evaluation — in order, against the DRAFT.** Arms test in source order;
the first true condition wins; with no true arm, `ELSE` applies; with no
true arm and no `ELSE`, nothing applies and the step proceeds — no error.
Unlike recipe selection, which reads the input state (§6.4), IF conditions
read the draft as it stands at the IF's turn, so earlier outcomes in the
same list are visible to later IFs. Normative:

- A recipe selected by `when count(name:S) < 3` fires with the pool at two,
  and its own `SPAWN SUSPICION, IF count(name:SUSPICION) >= 3 -> (ENDING
  ARRESTED)` sees the spawn it just made — the same step that spawned the
  third copy arrests (§9.39).
- An IF after `DESTROY OBJECT` in the same list reads a dead-stamped entity:
  the card reads as absent, so a comparison over it is false — silently
  (draft reads death; see §9.39 for the exact boundary vs. the rejection
  rule).

**Concretization — the taken branch only.** The log records the taken
branch's outcomes, flattened into the IF's position (the SET-delta
precedent, §9.31: the engine computes once at execution and logs the
result). The record never contains an IF op; the untaken branches are
authored discipline the log cannot recover (the §5.3 journal-loss
corollary). A `REFUSE` inside a taken branch refuses the whole step as
usual (§6.5).

**Triggers.** Both trigger forms (§10) may author IF, with the WORLD forms
of §6.3 — `exists(...)`, `exists(<query>)`, `count(<query>)` — since event
bindings are empty. A role-dependent operand in a trigger IF condition
(`count(role)`, `total(role,…)`, `role.aspect.x`, `role.attr.x`,
`rel(…role…)`, `every`/`any(role, …)`) is a **load-time error**: it would
silently read false/0 there (§9.39). The trigger PROGRESS/REFUSE guards
(§6.7, §9.34) and the endings existence check (§9.35) recurse into branch
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

The numbered rulings — §9.1–§9.50, the spec's decision ledger — live in
[rulings.md](rulings.md). The numbers are stable identifiers: every §9.x
citation in comments, tests, and tools resolves there, unchanged.

## 10. The script format — a game as one text file

A whole game is one text file: meta lines, then section blocks of
sentence-like declarations. `#` starts a comment; blank lines are ignored.
`compileScript` parses and compiles it once, failing loudly at load time
(unknown kind, unregistered verb or relation type, bad grammar, duplicate
declaration). Meta lines:
`script:` (title), `player:` (default actor), `goal:` (shown up front),
`start: d:h` (initial clock), and `seed: N` — the §9.41 dice seed, an
integer, default 0.

```
script: Murder on the Nightjar Flyer   # title
player: MARCH                          # default actor for lenient input (§11)
goal: unmask Lord Vane's killer …      # shown up front in the player interface
start: 0:42                            # initial clock
seed: 7                                # dice seed (§9.41, default 0)

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

decks:
  RUMOURS spec RUMOUR, RUMOUR, HEARSAY, SCANDAL defaultcard RUMOUR reset

endings:
  ARRESTED: "Taken by the constabulary"
  DESPAIR_DEATH: "No More" (melancholy)
  LANTERN_VICTORY: "The Light in the Ruins" (grand)

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

- **Entity line**: `NAME is KIND [stackable] [with aspect:level, …]
  [at|in PLACE]` (trailing `with`/`at`/`in` clauses in either order; the
  `stackable` keyword sits before them, §9.38). Script ids are `s_<name>` —
  the name lowercased, so two names differing only in case mint the same id
  (`ANA`/`ana` → `s_ana`); since names are case-exact (§9.14), the id
  collision is its own load error, naming both names. Containers resolve by
  name after all entities are read. Names must be
  unique among the living (§9.14) — except a `stackable` entity line, whose
  name is a pool (§9.38) — and are written as-is — prefer `UPPER_SNAKE`.
- **Duplicate declarations are load errors in every section** — entities,
  prototypes, templates, verbs, decks, endings, functions alike (the
  functions/decks rule generalized): a second declaration of the same key
  fails at load naming the key, it never silently overwrites. (Verbs: a
  repeated verb header, not a body line, is the duplicate.)
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
  - `ID on expire of NAME -> OUTCOMES` — fires when an entity named NAME
    expires (§9.32); NAME gets no compile-time existence check (the entity
    may be spawned at runtime), and its outcomes may reference NAME directly.
    Under a pool, a crossing of ANY copy fires the trigger, and its
    name-addressed outcomes hit the OLDEST copy — the §9.38 asymmetry. With
    no such trigger, expiry destroys the crossing card by id (§6.6, §9.38).
- **Function lines** (§9.40): `NAME(params…) -> OUTCOMES` — parametric
  outcome macros. Params are comma-separated identifiers in parens, bare
  space-separated identifiers without them, or absent (an empty `()` is the
  same as none). A body may call other functions, use `IF`, and interpolate
  `{param}` in its strings; the call sites — in recipes or triggers — write
  `NAME(arg, …)` and expand at load time (§9.40). Functions may share names
  with entities and verbs (calls only ever parse in outcome position); an
  op-named or IF/ELSE-named function is a load error.
- **Deck lines** (§9.42): `ID spec NAME[, NAME…] [defaultcard NAME] [reset]`
  (the trailing clauses in either order). The id is UPPER_SNAKE, unreserved,
  and unique. Spec entries are prototype/entity NAMES only — repeats weight
  the draw — and must resolve at load (naming the deck and the entry
  otherwise), as must `defaultcard`. The fixed draw order is the seed-fixed
  position permutation of §9.42; `reset` wraps when exhausted, `defaultcard`
  serves forever, neither makes every past-the-period draw a §9.19 rejection.
  Deck entries should be stackable, or consumed/renamed on the same step —
  the §9.42 livelock rule.
- **Ending lines**: `ID: "Title" [(flavour)]` — the ending table (§9.35).
  The id is UPPER_SNAKE and unique; the title is a quoted string; the
  flavour is a display word (any lowercase term) defaulting to
  `melancholy`. The table is what the `ENDING` outcome (§4.4) must name: an
  ENDING with an undeclared id — in a recipe or in either trigger form — is
  a load-time error.

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
  tick** at or after the world clock, §9.28) — an out-of-range halfhour
  field wraps into whole days the way durations compose (`0:118` = 118
  halfhours = day 2, halfhour 22; the canonical `d:h @` form keeps its
  strict `bad halfhour` bound); the
  first dictionary verb token ends the actor list — two-token preposition
  forms like `use on` are recognized; tokens before the verb are actors,
  defaulting to the `player:` character.
- Role keywords `at | with | about | to` start role phrases; bare names
  before one are objects.
- Names resolve case-insensitively, underscores as spaces, articles
  dropped, longest match first (up to 4 tokens) among **alive** entities;
  unknown words fail with suggestions (prefix match, then edit distance ≤ 2).
  A name named by several roles of one clause binds distinct §9.38 pool
  copies in birth order — the first role the head, each further role the
  next living copy, addressed by `#id` — and the line refuses when the
  living pool is smaller than the roles naming it; a name doubled inside
  one role still meets the engine's same-role placement refusal.
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
so there is nothing to undo after them. Once the story has ended (§9.35)
the interface wears its banner, input returns the ending notice, and the
suggestions row empties — while the views and buttons keep working, because
undo past the ending record reopens the book.

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
