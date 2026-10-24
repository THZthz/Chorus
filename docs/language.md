# Chorus² — Content Language Reference (`story-lang`)

The authoring format for Chorus² content: extension `.sw`, hand-written by authors. This document is the full concrete reference — every declaration, every effect, every diagnostic. The conceptual model lives in spec.md §5; the runtime contract in kernel-api.md.

Design goals, in brief: the grammar is mechanically regular (one declaration per shape, one spelling per concept); files are diff- and grep-first (one declaration per block, one fact per line, indentation as nesting, explicit ids); prose is first-class; diagnostics are a public interface — they carry `file:line:col`, a stable number, and a fix phrased at the declaration site:

```
lighthouse.sw:42:11: error [E203] unknown aspect key `restor` (read in sup.eat slot query)
  note: nearest declared key is `restore` (declared at lighthouse.sw:12)
```

Effects apply atomically onto a draft; a mid-application failure rolls the resolution unit back, rng and deck cursors included (kernel-api.md §9). There is no notion of a log or a replay. A complete, playable mini-game is writable without host glue — `emberwatch.sw` proves it (§10).

## 1. File Format and Lexer

- `#` starts a line comment. Strings are double-quoted; the only escapes are `\n`, `\"`, `\\`; strings are single-line. Identifiers are `lower_snake`; display names are data (`title "..."`), not identifiers. Indentation is 2 spaces; a tab is a lex error. Block order is free; compilation is two-pass (collect declarations, then resolve references); there is no use-before-declare.
- **Duration literals:** `<n>m`, `<n>h`, or the compound `<n>h<m>m` — an hours part, a minutes part, or both (value = total minutes). Examples: `45m`, `2h`, `1h30m`. In expression position a duration literal denotes its minute count. The recipe `duration` field additionally accepts a bare non-negative integer: `duration 0` equals `0m`, the instant recipe.
- **Files and `include`.** A compilation unit is one root file plus its transitive includes. `include "relative/path.sw"` may stand anywhere a top-level declaration may; the path resolves against the including file's directory. Inclusion is textual: the included declarations splice in at the include point, and the two-pass collect/resolve then runs over the whole splice — no use-before-declare across files either. An include cycle is [E231].
- **One id namespace.** Cards, verbs, decks, zones, endings, predicates, and event names share one global namespace across every included file; recipes hang off it as `<verb>.<recipe>`. Aspect and legacy keys are their own declared registries (§2.3, §2.7). Declaring the same id twice — within one file or across two includes — is [E232], reported at the second declaration with both sites named.

## 2. Declarations

### 2.1 The `game` Header

```
game "Emberwatch"
  seed 7          # content-fixed seed; boot must be called with the same seed
  turn 10m        # minimum unit the host may advance by
  home tower      # default drop zone
```

- `seed <N>` fixes the run seed in content: `boot` must receive the same seed, and a mismatch is a blocking boot diagnostic (`SeedMismatch`). `seed host` delegates the seed to the `boot` argument.
- `turn` constrains advance granularity only: the advance duration must be a positive multiple of `turn`. Recipe durations are non-negative minute values — `0` is legal and instant — card lifetimes are positive, and there is no duration-quantization rule.

### 2.2 `zones`

```
zones
  tower
  shore
  cabin in tower        # optional nesting; the zone scope includes the subtree
```

A zone is a pure address (a named grouping of card locations): no aspects, not destructible, not mintable. Card-carried containment uses `into <card>` on effects. Declared zones are `Table` zones; the defaults for every zone kind are fixed in spec.md §4.1.

### 2.3 The `aspects` Registry

```
aspects
  heat: 0        # key: default value; integers
  dread: 0
```

Every key read anywhere must be declared here; an undeclared key in any query, condition, or effect is [E203]. Read default: a card without the key falls back to the declared value (usually 0). Writes clamp at ≥ 0; the effect-level suffix `unclamped` switches the clamp off (§4.3).

### 2.4 `cards` — One Namespace for Entities and Prototypes

```
cards
  card coal
    kind matter              # optional display/filter tag; matchable via kind: terms
    stackable                # | unique | group <g>   exactly one
    aspects coal 1           # comma-separated key value pairs
    start in tower count 4   # entity face: present at boot; omitted = pure mint template
    desc "Black and glinting."
    # on complete [<aspect>] blocks — see §5
    # meta lines — see §2.8

  card tea
    kind provision  stackable
    aspects restore 1, stamped 1
    lifetime 3h              # armed per mint, from the mint moment
    start in tower count 1
```

- **One declaration, two faces.** With `start`, the card is instantiated at boot (`count` default 1; lifetimes arm at t=0); every declaration also serves as a mint template.
- **Uniqueness modes, exactly one:**
  - no flag (default): one live copy per name; minting onto a live same-name card is the hard `NameInUse` effect failure;
  - `stackable`: multiple live copies of one name;
  - `unique`: one live copy; a mint onto a live same-name card is skipped (`SpawnSkipped`) instead of failing; **only `unique` cards are addressable by name path**;
  - `group <g>`: while any member of the group lives, no other member mints (a bound card counts as live).
  - `unique` together with `stackable` or `group` is [E221].
- **Mint aliasing.** `mint <card>` declares an alias: the named card's body is the template for every mint of this name. Boot (`start`) instances always materialize from the card's own declared body.
- `lifetime <duration>`: every mint arms decay from the mint moment. A bound (locked) card's timer is deferred while locked (spec.md §4.6).
- `relates <kind> <card>, ...`: boot edges (§2.9) — at boot, every live instance of this card relates toward every live instance of each named card. An unknown kind is [E248]; an unknown card is [E244]; a named card with no live boot instance warns at boot and the edge is skipped.
- Name paths in expressions (`beacon.heat`) and name targets in effects address `unique` cards only [E239].
- A card may carry `on complete [<aspect>]` books — §5.

### 2.5 `predicates`

```
predicates
  predicate beacon_lit   = beacon.heat >= 1
  predicate starving(p)  = p.hunger >= 3 and p.restore < 1
  predicate can_fuel(n)  = total(world, aspect:coal) + total(interior, aspect:coal) >= n
```

Named, optionally parameterized; the body is any condition expression. Parameters are expressions — integer scalars or card references — type-checked at inline expansion: a parameter read as `p.<aspect>` must be bound to a card reference; a parameter used in arithmetic must be bound to an integer expression.

Load-time checks: recursion (a cycle is [E230]), arity, and key registration. Conditions and slot queries call predicates by name; the compiler inlines them at the call site — the kernel never sees a predicate.

A predicate used in query position must be unary over the candidate card: its single parameter binds to the candidate, and the body may read that card's aspects. Checked at expansion.

### 2.6 `verbs` and `recipes`

A verb is a site — the declaration face of a situation.

```
verb tend committable instances 1
  title "Tend the Beacon"
  releases tower          # optional; where interior survivors go at completion (default home)

  recipe feed
    label "Feed the fire"
    priority 20             # default 100; smaller is selected first
    craftable               # | chain-only | craftable chain-only (omittable; defaults per verb mode)
    max 1                   # lifetime open-count cap for the whole run
    duration 30m            # required, per recipe; 0 = instant
    declares stamped        # optional: aspects this recipe declares, for card books (§5)
    when beacon.heat <= 5
    guard "The furnace already roars." when beacon.heat >= 6
    slots
      fuel: 1..2 consuming aspect:coal
      kindle: 0..2 greedy open aspect:kindle
    effects
      set beacon.heat += total(slot:fuel, aspect:coal) + total(slot:kindle, aspect:kindle)
      narrate "The furnace drinks. Heat reads {beacon.heat}."
```

- **Modes, exactly one:** `committable` (the player may open it) / `autonomous` (an engine heartbeat, opening on checkpoints driven by host advances) / `uncommittable` (only `start` opens it — a popup). Player commits against the latter two are refused before opening.
- `instances <n|any>`: live-instance cap (default 1). Multi-instance is first-class: one verb may hold several situation instances at once.
- **Recipe fields** (order-free within the block): `label`, `priority`, eligibility flags, `max`, `duration`, `declares`, `when`, `guard`, `allow shadow`, `slots`, `effects`.
- **Selection happens at instance opening.** Verify slot minimums against the source's bindings (or state-countable greedy fill for `start`/autonomous — spec.md §4.3), scan for the first eligible recipe whose `when` holds, then open the window, lock cards, and arm the timer.
- **`duration` is recipe-level and mandatory.** Verbs declare no timing. Every recipe declares `duration <literal>` — the minutes the instance runs after opening; `0` is the instant recipe, completing inline inside its opener's action (spec.md §4.2). A recipe without a `duration` line is a compile error [E237]. Duration is orthogonal to selection: it never enters candidacy, `when`, or guard evaluation, and it arms only at materialization.
- **`declares <aspect>, ...`** lists aspect keys this recipe declares, comma-separated; every key must be in the aspects registry [E203]. An aspect-qualified card completion book fires at a recipe that declares that aspect, or — when the walking card's own declaration carries the qualifier — at any completing recipe whose interior pool contains it (§5).
- Two recipes of one verb that share a priority and an identical (or missing) `when` raise the [W110] shadowing warning; `allow shadow`, written on the shadowed recipe (the second declaration), suppresses it. Disjoint `when`s never shadow.
- `guard "<prose>" when <cond>`: guards are checked at opening, in declaration order, before any time is spent and before any card locks; a hit cancels the whole commit with the guard-refused diagnostic.
- **Slots are recipe-level.** `name: range [flags] [query]` — range `N` | `N..M` | `N..` (`min > max` is [E240]); flags `greedy` (auto-pull; requires a finite max — [E241]), `consuming` (burned at settlement), `open` (re-bindable mid-run), `take` (cards return to their bound-from zone at release); the query may call derived predicates. Explicit fills come first; greedy tops up after selection. Bindings lock until completion. Slot names appear in conditions as the `slot:<name>` scope and in prose as `{<name>}`.
- Recipe ids are unique within their verb; global references read `<verb>.<recipe>`.

**Eligibility sets:**

| craftable | chain-only | player commit | `start` effect | autonomous heartbeat |
|---|---|---|---|---|
| yes | no | yes | yes | yes |
| no | no | no | yes | yes |
| any | yes | no | yes | no |

Default faces: on a `committable` verb, recipes default to `craftable=true, chain-only=false`; on an `autonomous` verb, to `craftable=false, chain-only=false` (heartbeat-private); on an `uncommittable` verb, to `craftable=false, chain-only=true`. `craftable=true` together with `chain-only=true` is the compile contradiction [E222].

### 2.7 `decks` / `legacies` / `endings`

```
decks
  deck omens
    entries portent, portent, gull, calm   # repetition = weighting; a seeded permutation fixes the order
    fallback calm                          # relief path 1: serve this card when the permutation is spent
  deck tides
    entries ebb, flood
    wrap                                   # relief path 2: cycle the permutation when no fallback is declared

legacies
  dawns_seen: 0

endings
  ending survived "The Beacon Holds" grand
    text "Dawn finds the lamp trimmed and burning."
```

- Deck draws walk a dedicated permutation cursor. Exhaustion resolves in a fixed order: `fallback` wins while it can serve; `wrap` cycles the base permutation when no `fallback` is declared — the cursor returns to the head and dealing continues; the exhaustion effect failure is the answer only when a deck declares neither.
- Legacy keys must be declared; an undeclared key read or written is [E204]. Values are 64-bit integers.
- Ending tiers (`grand`, `somber`, `dark` above) are free-form display labels — plain strings the engine never ranks.

### 2.8 `meta` Annotations

Any declaration line — `game`, a zone, an aspect, a card, a predicate, a verb, a recipe, a slot, a deck, a legacy key, an ending, a trigger rule, an `effectset` — may be followed by deeper-indented `meta` lines:

```
card coal
  kind matter  stackable
  meta ui_icon "coal_lump"

zones
  tower
    meta shows_fog "true"
```

- `meta <key> <value>`: the key is `lower_snake` and never declared; the value is a double-quoted string. Any number of lines attach to the declaration above them. A repeated key on one declaration is [E236] — as is a repeated `on complete` qualifier on one card (§5).
- The compiled content carries each declaration's annotations as a key–value string map; the kernel never reads it. Tools hang UI, layout, and documentation data without grammar changes or kernel knowledge.

### 2.9 The `relations` Registry

```
relations
  carries exclusive          # directed (default) + exclusive
  married symmetric exclusive
  adjacent symmetric
  north_of                   # directed, no flags
```

Relation kinds are declared names with flags, forming their own registry — disjoint from card `kind` tags (different namespaces, like aspect keys; no collision rule). A duplicate kind is [E232], reported at the second declaration with both sites named. Flags are order-free; `directed` is the default and has no keyword; an unknown flag is ordinary syntax [E201].

- A **directed** edge is stored under its own subject and tests that direction only.
- A **symmetric** edge is stored once, under the canonical subject (the smaller entity id — mint order), and every read is direction-agnostic: `rel(married, a, b)` and `rel(married, b, a)` are the same question.
- An **exclusive** kind is a write-time rule: directed + exclusive keeps at most one `from` per object — a second `relate` *steals* (the prior edge is removed first, each victim emitting `Unrelated`, then the new edge emits `Related`); symmetric + exclusive gives each endpoint valence 1 (partner semantics — a new edge steals every edge of that kind touching either endpoint). Non-exclusive kinds accumulate freely.
- Boot edges: a card line `relates <kind> <card>, ...` relates every boot instance of the card toward every live instance of each named card, applied after all boot mints and before the t=0 checkpoint, in card declaration order. A named card with no live boot instance is a boot warning (play proceeds) and the edge is skipped; boot emits no events. Edges to cards minted after boot come only from authored `relate` (§4.3).

Every kind read or written anywhere must be declared here; an undeclared kind is [E248].

## 3. Expressions, Queries, and Scopes

```
condition  := or-term ("or" or-term)*
or-term    := and-term ("and" and-term)*
and-term   := "not" and-term | comparison | predicate-call | "(" condition ")"
comparison := arith (">=" | "<=" | ">" | "<" | "==" | "!=") arith
arith      := term (("+" | "-" | "*" | "/") term)*      # integers; division floors
```

**Operands:** unique-card name paths (`beacon.heat`), `count(<scope>, <query>)`, `total(<scope>, aspect:<key>)`, `alive(<card>)`, `rel(<kind>, <card-ref>, <card-ref>)`, `clock`, `legacy.<key>`, roll bindings (`sight`, inside the effect list that declared them), and duration literals (their minute count). Division by zero evaluates to 0 (totality preserved); a constant zero divisor is a compile error [E242]. `rel(...)` tests the logical edge between the two endpoints — symmetric kinds hold either way; a dead endpoint reads false; an unknown kind is [E248]. It composes with `and`/`or`/`not` and predicates: predicate card parameters substitute into any card-ref position and are type-checked there ([E245] on a mismatch), so `predicate holds_alice(c) = rel(carries, alice, c)` is a slot filter.

**Card refs.** A relation endpoint (`rel(...)` argument, `related:` scope, `{related:...}` token) is one of:

```
<card-ref> ::= <unique-card>                    # a unique card name [E239]
             | subject                          # the context card
             | first(<scope> [<query terms>])   # first match in scope order
```

`subject` is the query-position candidate inside slot filters and queries, the bound card in a card completion book, the dying card in an expiry book; outside a subject context it reads as a dead endpoint. `first(...)` re-resolves at every evaluation: the scope's first candidate that matches the terms, in the scope's own deterministic order — this is how non-unique cards become endpoint-addressable (`first(related:carries of alice name:sword)`). An unresolved ref is a *dead endpoint*: `rel` reads false and a `related:` scope reads empty — totality, no special cases.

**Scopes:** `interior`, `table`, `world`, `zone:<Z>`, `slot:<name>`, `bound`, `anywhere`, `related:<kind> (of | to) <card-ref>`.

- `interior` — the interior of the situation whose resolution is running;
- `slot:<name>` — the slot's bound cards in the running situation;
- `bound` — every card bound in the running situation across all slots, **counted per binding**: a card bound in two slots counts twice;
- `zone:<Z>` — the zone with its nested subtree;
- `table` — the public live cards plus every situation's non-greedy slot bindings; greedy bindings and situation interiors are excluded;
- `world` — public live cards;
- `anywhere` — all live cards;
- `related:<kind> of <card-ref>` — the endpoint's edges of one kind (what alice carries);
- `related:<kind> to <card-ref>` — edges pointing at the endpoint (who relates `north_of` to the hall); for symmetric kinds both spellings read the same partners.

`related:` is a root scope like `slot:`: it *replaces* the scope position (there is no scope algebra) and its results are filtered by ordinary query terms — `count(related:carries of alice, kind:weapon)`. It is **location-transparent**: it enumerates related live cards wherever they live — situation interiors, hidden zones, carriers included — because relations are facts, not locations. Enumeration order is mint order (oldest first), the same law as every scope.

In contexts without a running situation — `on expire` rules, `on event` rules, commit selection, autonomous polls — `interior`, `slot:`, and `bound` read as empty sets.

**Query terms:** space-conjoined `aspect:<k>`, `aspect:<k><op><n>` (e.g. `aspect:heat>=2`), `name:<x>`, `kind:<k>`, `-<term>` (negation), and unary predicate calls; an empty query matches everything in scope. The set of known kinds is collected from card `kind` lines; a `kind:` term naming an undeclared kind is [E203], as for aspect keys.

The grammar is deliberately closed and machine-checkable — the property that lets `story-analyze` parse its query goals in this same condition grammar (spec.md §8).

## 4. Effects

### 4.1 The Closed Vocabulary

The authored set: `spawn`, `destroy`, `consume`, `swap`, `set`, `move`, `legacy`, `narrate`, `roll`, `draw`, `start`, `continue`, `signal`, `ending`, `relate`, `unrelate`, `if/else`. Nothing else is authored as an effect kind: the situation-abort effect is kernel-only, and host-registered ops are host-only. `effectset` calls (§6) are author-side abbreviation and expand to this set at compile time.

Effect failure (consume shortage, exhausted deck, name-in-use, invalid roll operands, …) rolls the current resolution unit back to its checkpoint; a retried identical `commit` re-rolls identical dice, because state was restored bit-for-bit (kernel-api.md §9). `relate` and `unrelate` are total effects: kinds are compile-checked [E248], endpoints resolve to whatever they resolve to, and there are no failure modes.

### 4.2 Targets

One target grammar serves `set`, `move`, `destroy`, `swap`, `consume`, `relate`, and `unrelate`:

```
target := <unique-card> "." <aspect>        # name path — set only (the write key is the aspect)
        | <unique-card>                     # name form (set spells it with .<aspect>)
        | slot:<name>                       # the slot's bound cards
        | (world | interior | anywhere | zone:<Z>) <terms>   # scoped query
        | <terms>                           # bare query: interior-oldest-first, then world-oldest-first
        | <handle>                          # a spawn handle's pool (§4.3)
```

Semantics (spec.md §4.5):

- A **name target** addresses exactly one card; a name target on a card that is not `unique` is [E239].
- A **slot target** acts per bound card: `set slot:morsel aspect:stamped += 1` writes once per bound card.
- A **scoped query** addresses the scope's candidates oldest-first within the scope.
- A **bare query** resolves interior-oldest-first, then world-oldest-first.
- A **handle** addresses the cards minted under it in the same effect list, oldest-first.

A `set` writes every card its target resolves to; counts in `destroy` / `consume` / `swap` walk the resolved order.

### 4.3 Effect Reference

```
spawn <card> [count <n>] [into (interior | <id>)] [as <handle>]
destroy <target> [all | count <n>]          # default all
consume <target> [<n>]                      # default 1
swap <target> [count <n>] to <card> [into <id>]
move <target> into (interior | <id>)
set <card>.<aspect> (= | += | -=) <expr> [unclamped]
set <multi-target> aspect:<key> (= | += | -=) <expr> [unclamped]
legacy <key> (= | += | -=) <expr>
narrate "<prose>"
roll <count>d<sides> [+ <bonus>] as <name>
draw from <deck> [count <n>] [into (interior | <id>)]
start <verb> [via <recipe>] [count <n>]
continue via <recipe>                       # recipe effects only [E247]
relate <target> to <target> as <kind>
unrelate <target> from <target> [as <kind>]
signal <event>
ending <id>
if <condition> <effect-lines> [else <effect-lines>]
<effectset-name>(<args>)                    # §6
```

- `spawn` mints from the card's template (or its `mint` alias); `count` default 1; the zone defaults to `interior` in a situation context, else `home`.
- **Handles.** `spawn <card> ... as <handle>` binds the minted card(s) to `<handle>` for the enclosing effect list only: subsequent effects in that list address the pool via `<handle>`, oldest-first for multi-card pools, and narrate tokens may reference `{<handle>}` (display names) and `{<handle>.<aspect>}` (the oldest card's read). A handle is visible at and after its spawn line, including nested `if` branches and subsequent `effectset` expansions, and nowhere else; a handle target with no enclosing spawn is [E238], an orphan handle-shaped token is [E205] (§7).
- `set` writes with `=` / `+=` / `-=`, clamped at ≥ 0 unless suffixed `unclamped`; `legacy` writes the same arithmetic and always clamps at ≥ 0.
- **Effect counts are full integer expressions** — the `count <n>` of `spawn` / `destroy` / `swap` / `draw` / `start` and the trailing `<n>` of `consume` take the §3 grammar (`spawn coin count 1 + total(world, aspect:coal)` is legal), evaluated at effect time; a negative count is the effect failure, and a constant negative is [E246]. The expression ends at end-of-line: a following `into`, `as`, `to`, or `via` clause is no operator.
- **`roll` takes full integer expressions** in count, sides, and bonus — `roll 1d6 + warden.watch as sight`, `roll total(bound, aspect:dice)d6 as storm`, `roll 0d6 + legacy.favor as grace` are all legal. Operands are validated at effect time: count ≥ 0, sides ≥ 1; a violation is an effect failure (unit rollback). Count 0 applies the bonus only and leaves the rng counter unmoved. The result binds `<name>` for subsequent `if` conditions and `{<name>}` tokens in the same effect list (last wins).
- `draw`'s count defaults to 1; the target defaults to `interior` when the effect runs in a situation context, else `home`. Exhaustion walks `fallback` → `wrap` → the exhaustion failure (§2.7).
- `start` materializes a new instance; failure semantics are spec.md §4.2's taxonomy — silent chain end, or skip on cap/materialization shortfall, never a parent rollback.
- `continue via <recipe>` continues the running situation into the target recipe instead of completing (spec.md §4.2): the surviving bindings become unbound interior storage, the target's greedy slots pull, and the timer re-arms at `now + duration`. The recipe is spelled locally (a recipe of the same verb) or globally `<verb>.<recipe>`, and may be the executing recipe itself. Branch semantics are ordinary: a `continue` fires only when its branch executes, and the completion then re-checks the recipe's `continue` arms in emission order from the executed one — the first arm whose `when` holds adopts. It is legal only in recipe effects — anywhere in a trigger book is [E247], and an unknown target is [E244].
- `signal <event>` ignites the `on event <event>` book (§5) — the only way an event book ever fires; an event name no book declares is [E243].
- `relate <from> to <to> as <kind>` writes one logical edge per subject × object pair (§2.9): both endpoints take the §4.2 target grammar, multi-card endpoints address all matches, and application runs over every pair in resolved order. Self-pairs are skipped silently; each written edge emits `Related`; an exclusive kind steals first (§2.9).
- `unrelate <from> from <to> [as <kind>]` removes edges: with a kind, that kind's edge between each pair; without, every logical edge between each pair, either direction, once per edge ("sever everything" — symmetric rows are removed once, not twice). Each removed edge emits `Unrelated`; an edge already absent is a silent no-op.
- `if` takes a condition and deeper-indented effect lines; `else` sits at the `if` line's indent with its own effect lines.

## 5. Trigger Books

Four book kinds attach at four fixed points; the kernel ignites a designated book, never a broadcast (spec.md §4.6).

```
on complete            # inside a recipe: fires at its completion step 3

on complete [<aspect>]  # inside a card: fires when any completion binds this card

on expire              # inside a card: fires instead of default destruction

on event <name>        # top level: fires only when an effect executes signal <name>
  rule <id> [priority <n>] [once] [when <cond>]
    effects
      ...
```

Every book holds rules of one shape: `rule <id> [priority <n>] [once] [when <cond>]` with an `effects` block. A book walks its rules under the selection law — `min_by (priority, decl_index)` — and the first rule whose `when` holds fires; `once` marks the rule spent for the run.

- **Recipe books** fire at the recipe's completion step 3, with the situation's bindings still locked and its interior alive.
- **Card books** (`on complete [<aspect>]` inside a `card`) let a card bring its own completion behavior into any situation that binds it — the genre's "when this card is used, X" pattern — without rewriting every binding recipe. Sequencing at completion: the recipe's own book first, then every interior card's book — the bound cards, deduplicated across slots (a card bound twice walks once), in bind order, then the unbound interior storage cards in mint order. The aspect-qualified form fires when the completing recipe `declares` that aspect, or when the situation's interior aspect pool — the read-value sums over the interior, bound cards included — contains it and the walking card's own declaration (its element id and aspect keys) carries the qualifier; an unqualified book fires at every completion. Inside a card book, `subject` is the bound card carrying the book. Card books run within inline completions of instant recipes too, counting inside that one resolution against the per-advance budget (spec.md §4.2). Two blocks with the same qualifier on one card are [E236].
- **Expiry books** fire instead of default destruction; their rules address the dying card as `subject`; while the card is bound they do not fire (spec.md §4.6).
- **Event books** live at top level; `signal <name>` is their only ignition [E243].

Rules have no refuse semantics; effect failure follows the per-unit policy (§4.1). In expiry and event books — contexts without a running situation — `interior`, `slot:`, and `bound` read as empty sets.

## 6. Effect Macros — `effectset`

```
effectset stoke(n)          # top level, beside predicates; parameters type like predicate parameters
  effects
    consume aspect:coal n
    set beacon.heat += n
```

A call is an effect line — `stoke(2)` or `stoke(1 + total(world, aspect:coal))` — legal anywhere an effect line is: a recipe's `effects`, a trigger rule's `effects`, either `if` branch, or another `effectset` body.

- **Compile-time expansion.** The compiler expands every call before IR lowering: the body splices in with each parameter replaced by the argument's token stream, and the result parses as ordinary effects (§4). Parameters type like predicate parameters — integer expressions or card references, checked at expansion.
- **No recursion, bounded depth.** A macro whose expansion reaches itself is [E235]; the depth cap is 16, and exceeding it is the same diagnostic. Every body is parsed once at declaration with its parameters as opaque holes, so a malformed line is a compile error even if the macro is never called.
- **Diagnostics.** Unknown macro name at a call site [E233]; argument count ≠ parameter count [E234].
- **The kernel never sees a macro.** The compiled content stores expanded effect lists only — `effectset` is pure abbreviation, exactly as predicates inline away (§2.5).

## 7. Prose and Token Interpolation

Every prose string — `narrate`, card `desc`, ending `text`, guard prose — supports uniform `{…}` tokens:

- a slot name renders the slot's bound cards' display names;
- a unique-card name path (`{beacon.heat}`) renders a value read;
- a roll binding (`{sight}`) renders the roll result;
- a spawn handle (`{fresh}`, `{fresh.heat}`) renders its pool's display names, or the oldest card's read (§4.3);
- `{legacy.<key>}` renders a legacy value;
- `{related:<kind> (of | to) <card-ref>}` renders the related cards' display names, comma-joined; an empty set renders the empty string. The card-ref may be a unique name, `subject` (in a subject context), or `first(<scope> [<terms>])` over the checkable term forms (`aspect:`/`kind:`/`name:`/negation).

An unknown token is [E205] — including an unknown relation kind or an unresolvable card-ref inside a `related:` token. One exception: a `kind:` term inside the token's `first(...)` that names an undeclared kind is the query-term checker's [E203]. Every prose string is addressable by a template id (its declaration site plus field, with an occurrence index for repeats within one recipe); a load-time override table keyed by template id substitutes alternate text before interpolation — the localization seam (spec.md §7).

## 8. Idioms

Compositions the mechanism set makes natural:

- **Per-site lifetime via variant cards.** A card that should exist only at one site is a short-lived variant — a `fresh_news` card with `lifetime 30m` beats a per-spawn override; mint the variant, let decay clean up.
- **Mid-run narration via chains.** Longer arcs are chains of shorter recipes: each `start` hands off to the next beat, and the autonomous heartbeat keeps world texture moving between player actions.
- **Reveal.** Move a card out of a Hidden zone to surface it, or flip a 0/1 aspect as the reveal flag; the rendering layer reads state and decides what showing means (spec.md §7).
- **Time-of-day patterns.** `clock` arithmetic in `when` conditions: a dawn gate is `clock >= 6h`; an evening window is `clock >= 22h and clock < 24h`; duration literals carry the minute counts.
- **Disjunctive slot filters via unary predicates.** A slot that accepts any of several aspect arms is one predicate over the candidate card: `predicate fits(p) = p.a >= 1 or p.b >= 1`, called bare in the slot query (`fits()`); conjunction and negation terms compose around the call.

**Relations vs zones vs carriers vs aspects** — the authoritative guidance for the four fact/placement tools (the decision table of spec.md §4.9):

| Authoring need | Use | Why |
|---|---|---|
| Map adjacency, up/down/left/right | Directed relation kinds (`north_of` …); `adjacent` symmetric when direction-free | Direction is data; zones are containers, not neighbors; reverse questions are incoming enumerations (`related:north_of to hall`) |
| Equip / carry as a *fact* (equipped, ownership, curse-marked, "knows") | Directed **exclusive** relation (`carries`) | Survives movement; one-bearer semantics with observable steal |
| Physical inventory (items live on the body, bindable, move together, scatter home on death) | Carrier zone (`into <card>`) | Location semantics: public+bindable `Of(entity)` zone, zone scopes see it, occupants scatter home on the carrier's death |
| Social ties, membership, knowledge, debts | Plain directed kinds, non-exclusive (`member_of`, `knows`) | The general case; one-to-many and many-to-one are sets and scans, not special forms |
| Group with ranked ties ("alice favors bob 3") | Relation for the tie (`favors`), **aspect on the card for the rank** | The edge answers "is there a tie", the aspect answers "how strong"; a rank varying per dynamic pair has no first-class home — declare the keys you know, or keep the rank on the source card |
| Per-card flags and counters | Aspects | Single-card facts are not edges |

The two carry-like tools compose deliberately: `move torch into alice` (physical) plus `relate alice to torch as wields` (fact) is a normal pair of lines; using only the relation is correct when location is irrelevant. Slot filters can test relations through a unary predicate: `predicate unclaimed(p) = not rel(carries, alice, p)`.

## 9. Compile Diagnostics

Compile diagnostics carry `file:line:col`, a stable number, and a note phrased at the declaration site (nearest spelling, suggested fix). The table:

| # | Fires when |
|---|---|
| E201 | The line's syntax is malformed (unknown declaration, missing separator, unexpected token). |
| E202 | The text is not lexable: a tab, an uppercase identifier, a bad string, escape, or duration spelling. |
| E203 | An aspect key or card kind is read in a query, condition, predicate, or effect but is not declared (the aspects registry, card `kind` lines). A `kind:` term inside a prose token's `first(...)` counts too (§7). |
| E204 | A legacy key is read or written but is not declared. |
| E205 | A prose `{token}` resolves to nothing known (slot, name path, handle, roll binding, legacy key). |
| E221 | A card declares `unique` together with `stackable` or `group`. |
| E222 | A recipe declares `craftable` together with `chain-only`. |
| E230 | A predicate's expansion reaches itself. |
| E231 | An include cycle. |
| E232 | An id is declared twice in the one namespace, or a relation kind is declared twice in the relations registry (reported at the second site, both named). |
| E233 | An `effectset` call names no declared macro. |
| E234 | An `effectset` call's argument count differs from its parameter count. |
| E235 | An `effectset` expansion reaches itself or exceeds the depth cap (16). |
| E236 | A declaration-level key repeats: a `meta` key, an `on complete` qualifier, or an `on expire` block on one declaration. |
| E237 | A recipe has no `duration` line. |
| E238 | A `<handle>` target has no enclosing `spawn ... as <handle>` in the same effect list (an orphan handle-shaped prose token is [E205], §7). |
| E239 | A name-path operand or target addresses a card that is not `unique`. |
| E240 | A slot range's minimum exceeds its maximum. |
| E241 | A `greedy` slot has no finite maximum. |
| E242 | An expression divides by a constant zero. |
| E243 | `signal` names an event no `on event` book declares. |
| E244 | An unknown id is referenced: card, verb, recipe, zone, deck, ending, or predicate. |
| E245 | A predicate or effectset parameter is typed or arity-checked against its use (used both ways, wrong argument form, wrong argument or parameter count). |
| E246 | A magnitude is non-positive where that is required (turn, lifetime, `max`, counts) or out of range (a duration literal past 64 bits). |
| E247 | `continue` appears outside recipe effects (a trigger book: recipe, card, expiry, or event). |
| E248 | A relation kind is read or written but is not declared (the relations registry): a `rel(...)` operand, a `related:` scope, `relate`/`unrelate`, a card `relates` line. (Inside a `{related:...}` prose token the unresolvable kind is the token checker's [E205].) |
| W110 | Two recipes of one verb share a priority and an identical (or missing) `when` (warning; `allow shadow` on the shadowed recipe suppresses it). |

Runtime diagnostics (refusals, effect failures, boot and snapshot kinds) are the engine's catalog: kernel-api.md §7.

## 10. The Worked Sample

`emberwatch.sw`, beside this document, is the acceptance sample: a lone lighthouse keeper feeds the beacon and survives to dawn. It exercises the core loop — the aspects registry with read defaults, cards with lifetimes, recipe-level slots (consuming, greedy, open), per-recipe durations, guards, derived predicates, roll, deck draw, query consume, legacy write/read, three endings, zone scope, a multi-instance verb, and a chain-only verb opened by `start`. It deliberately stops at the core loop: the full mechanism matrix is the engine test suite's contract (kernel-api.md §10). Keep the sample in lockstep with this reference.
