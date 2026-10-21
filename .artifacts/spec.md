# Chorus² — Engine Architecture Specification

## 1. Mission

Chorus² is a personal experimental engine: a general-purpose runtime for "aspect-card state-machine" story games — Cultist Simulator, Book of Hours, and any game that tells stories through situations × cards × timed recipes.

The engine commits to one shape: a small kernel, declarative content, registry extension, and a single total selection order. The principles behind the shape are normative, not aspirational: state is a serializable value and there is no action log; selection is a total order of explicit priority, and declaration order only breaks ties; time moves only when the host asks (§4.6).

The first release ships conditional expressions, multi-instance situations, cross-run legacy, and no real-time mode — `turn` is the explicit minimum time unit. A text DSL is the primary authoring surface (§5); original sample content leads, and a Cultist Simulator content converter follows as the expressiveness stress test (plan.md).

## 2. The Six Kernel Laws

- **L1 Explicit time.** Time advances only when the host asks. The content header declares the minimum unit `turn`; an advance step requires a duration that is a positive multiple of `turn`. The engine never runs a clock on its own; it settles only what is due.
- **L2 The snapshot is the save.** The state is a plain serializable value. Save = serialize a snapshot; undo = a snapshot stack; load = restore. No action log, no replay law, no byte-level promises.
- **L3 Closed nouns, open verbs.** The nouns (card / aspect / situation / recipe / slot / deck / legacy / ending / zone) are closed; effects and predicates grow through the registry without kernel changes.
- **L4 One recognize-and-execute loop.** Every mechanism compiles to (condition, effects) pairs, and one selection law serves every trigger source. The selection law is `min_by (priority, decl_index)`: priority is ascending — the smaller number is selected first, and the default 100 is the last resort — and declaration order breaks ties.
- **L5 Lightweight determinism.** The seeded RNG lives in the state. Same content + same seed + same host operation sequence ⇒ same state. A free corollary of L1/L2, guaranteed by nothing else.
- **L6 Mechanics vs presentation.** The engine returns a structured event stream; the host renders. Narrative text is content data, never an engine output format.

## 3. Crate Map and Dependency Law

| crate | responsibility | public interface (one line) |
|---|---|---|
| `story-core` | value types and state: Entity / Zone / Situation / State / Clock / Rng / Snapshot, serde | "serializable game-state values + pure functions" |
| `story-content` | compiled content DB and IR types (recipes / slots / effect trees / expressions / registry name tables) + diagnostics | compile IR into a validated content database with diagnostics; read-only queries |
| `story-lang` | the text DSL (`.sw`) → IR → `story-content` | compile source text into a content database, diagnostics on failure |
| `story-engine` | the kernel: selection law, settlement, slots, mint pipeline, effect registry, snapshots | boot from content, registry, seed, legacy; commit / bind / advance / state / snapshot / restore |
| `story-cli` | thin dev driver (load one root file and follow includes, dumb terminal loop) | optional, not a product |

Dependency law: `core ← content ← engine`; `lang → {core, content}`; **engine never depends on lang**; the compiler and the runtime are separate programs. Diagnostic types live in `story-content` and are shared by every crate.

## 4. Kernel Semantics

### 4.1 The State Model

The state is a single plain value: every fact the engine knows lives in it, every operation transforms it, and serializing it is saving the game. It carries:

- **Now and randomness.** A clock reading (`now`, starting at 0 at boot — the `clock` operand reads it) and one seeded RNG counter. The seed is fixed at boot (§4.8); the same content, seed, and host operation sequence produce the same state (L5).
- **Entities.** A live card is an entity: name, prototype, kind tag, aspect map, zone, mint policy, and an optional expiry moment. Aspect keys are flat — the dot in `beacon.heat` is a spelling convention, not hierarchy. Aspects are the only card-carried integers the authored language sees; the entity record also carries kernel-internal attributes, which serve host-registered effects and engine bookkeeping. No authored operand or effect writes an attribute.
- **Zones.** A zone is a first-class place: a pure address (a named grouping of card locations), no aspects, not destructible, not mintable. Four kinds exist, each with a fixed visibility-and-bindability default:

  | kind | public | bindable |
  |---|---|---|
  | Table | yes | yes |
  | Hidden | no | no |
  | SituationInterior | no | no |
  | Of(entity) | yes | yes |

  Declared zones are Table; the other kinds arise from kernel operation — situation interiors, card carriers (`into <card>`), and host-created hidden zones. A zone scope includes the subtree of nested zones.
- **Situations.** An open situation is one instance of one recipe: its source (player commit, autonomous heartbeat, or `start` with its parent), its interior zone, its slot bindings — each binding remembering the zone the card was bound from — and its start and end moments.
- **Locks.** A lock is derived, never stored twice: the engine walks situation bindings, and there is no redundant lock table. Locking is total — while an instance holds a card, that card cannot join any other situation until the instance releases it, whatever asks first. Content models multi-actor scenes as separate cards or as sequential commits.
- **Bookkeeping.** Deck permutation cursors, per-recipe run counters, the set of spent once-rules, legacy values, and the ending flag.

A snapshot is the full state plus an envelope (format version, content fingerprint); §4.8 fixes the semantics.

### 4.2 Situation Lifecycle

**Materialization.** All three instance sources walk one path: mint the interior zone → bind slots (greedy top-up included) → lock → bump the recipe's run counter → enter the due queue at `now + duration`. An instant recipe never enters the queue (below).

**Busy belongs to the card, not the verb.** Bound cards stay locked for the instance's life; the only verb-level constraint is the `instances` cap. A player commit hitting a full cap is refused before opening; a `start` or autonomous poll at a full cap skips (`StartSkipped`).

**Player intervention.** `commit` always creates a new instance; the only intervention into a running instance is `bind` on an `open` slot — no re-selection, no clock movement. A mid-run bind affects only completion books and expressions evaluated at effect time; selection never re-runs and slot minimums are never re-validated.

**Completion sequence.** At the end moment (an instant recipe runs the same sequence inline, below):

1. The recipe's effects apply in order.
2. Consuming slots settle: surviving bound cards are destroyed.
3. Trigger books fire, in a fixed order: the recipe's own completion book first; then each locked card whose declaration carries a completion book walks it — deduplicated across slots (a card bound in two slots walks once), in bind order, each book under the selection law. A card book qualified by an aspect fires only when the completing recipe declares that aspect; an unqualified card book fires at every completion. Card books stop at the first ending: once one fires, no later card book walks. Cards bring their own completion behavior into any situation that binds them — the genre's core "when this card is used, X" pattern — without rewriting the binding recipe. The kernel abort path (below) is the one exception: it ignites the recipe's completion book alone, because card-carried books belong to completion, not abortion.
4. All bindings release: `take` slots return cards to the zone each was bound from, the rest unlock in place; interior leftovers move to the recipe's `releases` zone — which overrides `home` — and the interior is destroyed.
5. The instance is removed and completion is reported (`SituationCompleted`).

The kernel abort path — an effect failure inside a running instance — walks steps 3–5, skipping 1–2. It is kernel-only; nothing authored invokes it.

**Instant recipes (`duration 0`).** A zero-duration instance completes inline at the end of the action that opened it: `commit` materializes the instance and immediately runs the full completion sequence — effects → consuming settlement → trigger books (card books included) → release → interior cleanup — before returning, so the open and completion events return together and the player sees the result with zero latency. A `start` effect opening a zero-duration recipe resolves inline within the current resolution unit; nested instant chains are legal up to the per-advance budget (§4.6), each inline completion — card books included — counting against it as one resolution; exceeding the budget is the livelock rollback path. An autonomous verb whose recipes are all instant pulses exactly once per autonomous checkpoint: one selection, one inline open-complete cycle. Guards, greedy pulls, locking, and release apply exactly as in any materialization, and the whole open-complete cycle sits inside one atomic action — the commit's or the resolution unit's checkpoint — so nothing can interleave with it. An `ending` fired inside an inline completion closes the run normally (§4.6).

**Chains.** Follow-ups have exactly one form: the `start` effect. Explicit priority makes RUN/FORK-style alternatives redundant; first-match-stops is a free corollary of the selection law; fan-out is several `start` effects in one list; a named hand-off is `start via <recipe>`, still checked against `when` and the instance cap. Failure taxonomy: (a) no eligible recipe, or (b) every `when` false → the chain ends silently (`ChainEnded`); (c) the instance cap blocks, or (d) a matched recipe whose materialization fails (a slot minimum cannot be filled) → `StartSkipped`. Fan-out is best-effort: multiple `start` effects are independent, and one branch's failure never rolls back its siblings or the parent resolution. A `start` fired inside a completion sees the parent situation's interior and its still-locked bindings; release happens at completion step 4.

**Endings.** An `ending` effect closes the run at that instant: the ending flag is set, the remaining due queue is discarded — not resolved — and every subsequent `commit` or `advance` refuses with the story-ended diagnostic. The host's undo stack (§4.8) restores the pre-ending snapshot.

### 4.3 The Selection Law

The winner of any recipe scan is `min_by (priority, decl_index)` over the candidates whose `when` holds: priority is ascending — the smaller number wins first, the default 100 is the last resort — and declaration order breaks ties. `decl_index` is globally unique, so the winner is unique by construction. The commit pipeline runs in this order: eligibility → minimum-fill test → when-scan (priority order) → guard check → greedy top-up after selection → materialize.

**Candidacy.**

- **Player commits.** Explicit bindings must satisfy every slot minimum; a recipe whose minimums outrun its bindings is not a candidate. Greedy pulls never establish candidacy and never satisfy a minimum.
- **Starts and autonomous polls** submit no bindings. Their recipes qualify when every slot minimum is satisfiable by greedy candidates counted in the current state: unbound cards, in bindable zones, matching the slot filter. Greedy still tops up at materialization and may still fall short; the shortfall skips the open (`StartSkipped`) with no rollback of the parent resolution. The matched-but-materialization-failed case is therefore real, and it is handled at exactly one point.

**Failure results.** Nothing matches a commit → `NoRecipe`: zero state change, zero time. The winner's guard refuses → `GuardRefused` with its prose, same zero cost; guards are checked at opening, in declaration order, before any time is spent and before any card locks. A guard refusing during a `start` ends the chain.

**Orthogonals.** `max` counters increment at instantiation — an in-flight instance occupies its count, and a verb at its `instances` cap refuses parallel opens. Duration never enters selection: it never touches candidacy, `when`, or guard evaluation, and it arms only at materialization (`ends_at = now + duration`).

**Evaluation contexts per source.** Player commit: the submitted bindings, `interior` empty. `start`: no bindings of its own; `interior` is the parent situation's interior when fired inside a completion, else empty. Autonomous poll: no bindings, `interior` empty. `slot:` scopes resolve against the context's bindings (§4.7).

### 4.4 Slots

A slot is declared per recipe: a name, a range (exact, bounded, or open-ended), a filter query, and flags — `greedy` (auto-pull at materialization), `consuming` (bound cards burn at settlement), `open` (re-bindable mid-run), `take` (cards return to their origin zone at release).

- **Bind eligibility** is total-ordered: alive → zone bindable → not locked by any instance (§4.1) → filter match → slot not full and card not duplicated in it. The host asks through one named face per question (kernel-api.md §1).
- **Greedy pull** runs at materialization, after selection: candidates ordered by zone class (Table zones first, public zones in declaration order), then mint order (oldest first), pulled toward max, best effort. Greedy never establishes candidacy and never satisfies a minimum (§4.3).
- **Open slots.** `bind` adds a card mid-run — no recipe re-selection, no clock movement, a bound event emitted.
- **Minimums are checked only at selection time**, never re-validated mid-run: a slot whose bound cards fall below minimum through mid-run destruction does not re-run anything; the instance completes normally.
- **Destruction drops immediately:** a dead entity leaves every binding; there is no dead-reference re-resolution. Consuming settlement is completion step 2. Release happens at completion step 4 and on the abort path: `take` slots return cards to `bound_from`, the rest unlock in place. A released card whose expiry is past due re-enters the due queue and expires at the next resolution pass (§4.6).

Slot names appear in conditions as the `slot:<name>` scope and in prose as `{<name>}` tokens; slot queries may call derived predicates — one mechanism serves both places (§4.7).

### 4.5 The Effect Vocabulary

The authored set is closed: `spawn`, `destroy`, `consume`, `swap`, `set`, `move`, `legacy`, `narrate`, `roll`, `draw`, `start`, `signal`, `ending`, `if/else`. `end_situation` is kernel-only (the abort path). Host-registered ops are the registry extension node — host-only, never authored (§6); `effectset` calls expand into this set at compile time (§7, language.md §6). Closure is a kernel law, not an authoring cap. `signal` is the only ignition of an event book; signaling an event name no book declares is a compile error.

**Target addressing.** One target grammar serves `set`, `move`, `destroy`, `swap`, and `consume` (spelling in language.md §4.2; matching notes in kernel-api.md §8). Five forms, one semantics each:

- **Name target** — a unique card, spelled `card.aspect` in `set`. It addresses exactly one card; a name target on a card that is not unique is a compile error.
- **Slot target** — `slot:<name>` addresses the slot's bound cards and acts per bound card: `set slot:morsel aspect:stamped += 1` writes once per bound card.
- **Scoped query** — an explicit scope with query terms; it addresses the scope's candidates oldest-first within the scope.
- **Bare query** — terms without a scope; it resolves interior-oldest-first, then world-oldest-first.
- **Handle target** — a spawn handle addresses the cards minted under it in the same effect list, oldest-first.

Multi-card targets feed per-card effects: a `set` writes every card its target resolves to; the counts of `destroy` / `consume` / `swap` walk the resolved order.

**Built-in semantics.**

- **spawn** — the only mint pipeline: prototype defaults → effect overrides → uniqueness dedup → assign zone → allocate id → arm expiry at the resolution's stepped now. Uniqueness is a mint-instant check: a mint consults only the entities live at that instant, a skipped mint leaves no residue in the state, and a failed mint never burns a deck position. Mint aliasing (§5) defines the body used by all mints of a name; boot instances always materialize from the card's own declaration.
- **destroy** — target-addressed removal, with a count or all.
- **consume** — target-addressed destruction with the explicit fallback order: interior-oldest-first, then world-oldest-first.
- **swap** — destroy + spawn atomized; the replacement's zone defaults to the swapped card's own; uniqueness dedup excludes the card currently dying (swapping oneself = retire and re-mint, legal).
- **move** — relocates live entities to a zone or into a card carrier; no minting, no timer changes.
- **set / legacy** — integer arithmetic with `=` / `+=` / `-=`, clamped at ≥ 0 unless suffixed `unclamped`.
- **roll** — `count d sides + bonus`, all three full integer expressions, evaluated at effect time and validated there: count ≥ 0, sides ≥ 1; count 0 applies the bonus only and leaves the RNG counter unmoved. The result binds its name for subsequent conditions and prose tokens in the same effect list (last wins).
- **draw** — the deck's compile-time-fixed permutation plus the state cursor; exhaustion walks `fallback` → `wrap` → the exhaustion failure (§5, §4.6).
- **start** — materializes a new instance per §4.2; inside a completion it observes the parent's interior and locked bindings.
- **signal** — ignites the named event book; the only ignition point.
- **ending** — sets the ending flag; the run closes per §4.2; further endings in the same advance are no-ops.
- **narrate** — zero state; the engine evaluates token expressions into a string and emits a narration event; the host renders (L6).

**Atomicity.** Effects apply onto a draft of the state; an effect failure rolls the current resolution unit back to its checkpoint — RNG counter and deck cursors included. Within a unit, everything happens or nothing does. What happens next depends on the entry point: `commit` returns the refusal to the caller; an `advance` unit aborts its situation through the release path and the loop continues (§4.6).

### 4.6 Time, Advance, and Trigger Books

**Settlement.** An advance step targets `now + dt`; the loop repeatedly takes the least due item under the total-order key `(moment, phase, id)`, with phase order Completion < Expiry < Autonomous and `id` the mint order. Before resolving an item, the engine sets `now` to the item's moment — checkpoints, effects, spawn anchoring, and event timestamps observe the stepped now; when the loop drains, `now` reaches the target. Same-moment arbitration: completion before expiry (author intent outranks default decay), expiry before autonomous (auto-starts see the cleaned world).

Each item resolves completely: effects, then the designated trigger books; `start`s fired along the way materialize immediately, and a new instance enters the due queue at the stepped now plus its duration — unless it is instant, in which case the opener completes it inline and nothing enters the queue (§4.2).

**Per-unit failure policy.** Every resolution unit checkpoints before its first effect. An effect failure rolls the unit back to that checkpoint; the unit's situation aborts through the release path; a `ResolutionFailed` event is emitted; and the advance loop continues with the next item. Scheduled, autonomous, and chain resolutions can therefore never stall a run — the failure is an event on the advance path, not a diagnostic. The only whole-advance rollback is the livelock budget: more resolutions in one advance call than the budget allows rolls the entire call back to its entry checkpoint and returns the livelock diagnostic (the constant in kernel-api.md §6).

**Expiry.** The anchor is the mint moment; expiry is a one-shot timer cleared at settlement; extending life means re-minting or an expiry book that replaces default destruction. An expiry item that finds its card locked is deferred: `on expire` books do not fire while the card is bound, and on release a past-due card re-enters the due queue and expires at the next resolution pass. Expiry rules address the dying card through the implicit reference `subject`; their effects fully replace default destruction.

**Autonomous checkpoints.** At every distinct settled moment (after the completion and expiry phases finish) and once at the end of the advance. In verb declaration order, each autonomous verb not at its instance cap runs the selection law over empty bindings (`interior` empty); a hit materializes at most one instance — for an all-instant verb, that one hit opens and completes inline within the checkpoint; a miss is silent. Boot runs one checkpoint at t=0.

**Trigger books.** Four attach points: the recipe's completion book (completion step 3); card-carried completion books (same step, after the recipe's book — §4.2); the card's expiry book (expiry settlement); the top-level event book (ignited only by `signal`). The kernel ignites the designated book, never a broadcast. A book walks its rules under the selection law, and the first rule whose `when` holds fires; `once` marks the rule spent in the state's once-set. Rules have no refuse semantics; effect failure follows the per-unit policy above.

**Granularity.** `turn` constrains advance granularity only: the advance duration must be a positive multiple of `turn`. Recipe durations are non-negative minute values — 0 is legal and instant — card lifetimes are positive, and no quantization rule exists.

**Endings** close the run at that instant (§4.2): the remaining due queue is discarded, subsequent entry points refuse, and undo restores the pre-ending snapshot.

### 4.7 Queries and Expressions

**Scopes.** `interior` is the interior zone of the situation whose resolution is currently running — for a `start` fired inside a completion, the parent situation's interior with its still-locked bindings. `slot:<name>` is the context's binding set for that slot. `bound` is every card bound in the running situation across all slots, counted per binding — a card bound in two slots counts twice. `zone:<Z>` is a zone with its nested subtree. `world` is the public live cards. `anywhere` is all live cards. Queries match on aspect values, name, and kind (the tag a card declaration carries).

**Contexts without a situation.** In expiry rules, event rules, player-commit selection, and autonomous polls, `interior`, `slot:`, and `bound` read as empty sets. Only the resolution of a running situation gives them content.

**Totality law.** Evaluation cannot fail: absent aspect and attribute reads are 0; dead entities read 0; empty-set counts are 0 and existence tests false; an unbound slot or bound scope is empty; an unbound roll is 0; division by zero evaluates to 0, while a constant zero divisor is a compile error. Compile time catches syntax and type errors; runtime always has an answer.

**Derived predicates** are inlined at compile time — the kernel never sees one — and the same predicate mechanism serves conditions and slot queries (§5).

### 4.8 Legacy, Snapshots, and Diagnostics

**Legacy.** Content-declared keys with declared defaults; conditions read, effects write; values are 64-bit integers. Boot folds the host's incoming legacy over the declared defaults; a persistence face hands the values back to the host for the next run.

**Snapshots.** The full state plus an envelope: format version and content fingerprint. A fingerprint mismatch discards the snapshot and the host recompiles and reboots. The undo stack is host-held (the state clones cheaply), pushed before every player-visible action — including before an action that fires an ending, which is how undo restores the pre-ending state (§4.2).

**Seed.** Content may fix the run seed; boot must then be called with the same seed, and a mismatch is a blocking boot diagnostic. Content may instead delegate the seed to the boot argument.

**Diagnostics philosophy.** Content-driven failure is always a value, never a panic. Diagnostics come in four families: lookup (unresolvable references), eligibility refusal (commit and bind refusals handed back to the player), effect failure (runtime content faults such as exhaustion and name collisions), and advance/snapshot/boot. Compile diagnostics carry file, position, a stable number, and a fix phrased in the author's language at the declaration site — the compile table lives in language.md §9, the runtime kind catalog in kernel-api.md §7. Boot diagnostics split into warnings (play may proceed) and errors (it must not); the warning stream replaces a separate lint tool. The per-resolution failure event is deliberately an event, not a diagnostic: a failed resolution unit is normal advance traffic (§4.6). The commit path validates fully before mutating; the advance path leans on per-unit checkpoints.

## 5. The Content Language: Conceptual Model

The authoring surface is one text format, extension `.sw`; concrete syntax, grammar, and diagnostics live in language.md. This section fixes the concepts the kernel depends on.

**Declarations.** Every concept is a declaration block: the game header, zones, the aspects registry, cards, predicates, verbs (with recipes), decks, legacies, endings, top-level event books, effect macros, and meta annotations. One global id namespace spans cards, verbs, decks, zones, endings, predicates, and event names; aspect and legacy keys form their own declared registries, and every key read anywhere must be declared. Compilation is two-pass — collect declarations, then resolve references — so there is no use-before-declare, and duplicate ids are reported at the second site. Files compose through textual includes under the same namespace. Prose is first-class: narration, descriptions, and ending text are written in place with `{token}` interpolation.

**Cards and prototypes.** One card declaration has two faces: with a `start` placement it also instantiates at boot; every declaration is a mint template. Mint aliasing redirects a name's mints to another card's body; boot instances always materialize from their own declaration. Uniqueness is per card: multiple live copies by opt-in; single-copy by default with a hard collision failure; or soft-skip uniqueness and group exclusivity, checked against the entities live at the mint instant, a bound card counting as live. Lifetime arms per mint from the mint moment. A kind tag rides along for queries and tools. A card may carry its own completion books (§4.2).

**Verbs, recipes, slots.** A verb is a site — the declaration face of a situation. It declares a mode (`committable` — the player may open it; `autonomous` — an engine heartbeat opening on checkpoints; `uncommittable` — only `start` opens it), a live-instance cap, and an optional `releases` zone for interior survivors. A recipe carries a label, a priority, eligibility flags (`craftable`, `chain-only`; their combination on one recipe is a compile contradiction), a lifetime open cap, its own mandatory duration — verbs declare no timing, zero is the instant recipe, and a recipe without a duration line is a compile error — an optional `declares` list of aspects on which aspect-qualified card books key, a `when` condition, guards, slots, and effects. Two recipes sharing a priority and an identical (or missing) `when` raise a shadowing warning that the shadowed recipe can suppress. Selection happens at instance opening under the law of §4.3; candidacy requires explicit minimum fill for player commits and state-countable greedy fill for engine sources.

**Predicates.** Named, optionally parameterized condition definitions. Parameters are expressions — integer scalars or card references — type-checked at expansion; a predicate used in query position is unary over the candidate card. The compiler inlines predicates at the call site; recursion is a compile error.

**Expressions.** Integer and boolean arithmetic over totals, counts, aspect reads, clock, legacy values, roll bindings, and existence tests, evaluated over the scopes of §4.7; duration literals denote their minute count. Division floors; totality holds everywhere (§4.7).

**Decks.** An ordered entry list where repetition weights; a seeded permutation fixes the deal order behind a per-deck cursor. Exhaustion has two declared relief paths — a fallback card, then cycling the permutation — and only a deck declaring neither can raise the exhaustion failure on draw.

**Endings.** An id, a display title, and a free-form tier label — a plain display string the engine never ranks.

**Trigger books.** Authored at four points: inside a recipe (its completion book), inside a card (its completion book, optionally aspect-qualified, and its expiry book), and at top level (event books, ignited only by `signal`). Rules carry priority, a `when` condition, an optional once flag, and effects.

**Compile-step machinery.** Includes, `effectset` macros, and `meta` annotations are story-language compile steps: the kernel compiles one expanded content database and never sees any of them (§7).

The worked sample `emberwatch.sw` — a one-heartbeat lighthouse mini-game — lives beside these documents and is indexed in language.md §10.

## 6. Non-Goals

Scope decisions, each with its reason:

- **No event sourcing or durable action log.** The snapshot is the save (L2); a log would be a second representation of the same state with no promise attached.
- **No byte-exact output or pinned error texts.** Rendering belongs to the host (L6); diagnostics are structured values (§4.8).
- **No fixed tick grid and no tick triggers.** Time moves only when the host asks, in multiples of `turn`, and durations carry no quantization (L1, §4.6).
- **No verb-level slots.** Slots belong to recipes; the verb declares only its instance cap and `releases` zone, so every slot decision is a per-recipe decision.
- **No one-instance-per-verb law.** `instances` is a plain numeric cap; multi-instance is first-class (§4.2).
- **No marker-card access gates.** Reachability is expressed with ordinary conditions — `when` clauses and guards.
- **No twin or shadow card copies.** `swap` and re-minting cover replacement (§4.5); every live card is exactly one entity.
- **No persistent register of past manifestations.** Uniqueness is a mint-instant check (§4.5); the state records only live cards.
- **No reach/fuzz/lint tooling and no standalone lint binary.** Compile diagnostics plus the boot warning stream are the authoring feedback loop (§4.8).
- **No authored op blocks.** Host-registered effects are the only registry extension, so the kernel's effect vocabulary stays closed and auditable (§4.5).
- **No pairwise card relations.** Zones and carriers own placement; pairwise card facts are expressed as matched aspect tokens when needed.

## 7. Extension Seams

Every seam below is either designed in this spec or permanently excluded; none blocks delivery (plan.md).

1. **Include and modules — designed.** Files splice textually under the one id namespace; a compilation unit is one root file plus its transitive includes (language.md §1).
2. **`effectset` macros — designed.** Compile-time expansion into the closed effect vocabulary, parameters typed like predicate parameters (language.md §6).
3. **Deck wrap — designed.** The declared cycling relief path lets long-run decks deal forever without kernel knowledge (language.md §2.7).
4. **Meta annotations — designed.** Any declaration carries free-form key–value data; the compiled content stores it, the kernel never reads it, and tools hang UI, layout, and documentation data without grammar changes (language.md §2.8).
5. **Localization — designed.** Every prose string flows through the token-interpolation layer and is addressable by a template id: its declaration site plus field, with an occurrence index disambiguating repeats within one recipe. An override table keyed by template id substitutes alternate text at load, before interpolation; no engine change (language.md §7).
6. **Real-time hosting — already expressible.** A host may advance on any cadence it likes — once per wall-second, once per frame: L1 gives the engine no wall clock, so cadence is the host's law alone.
7. **Hidden information — seam designed; one permanent exclusion.** Hidden zones plus the `anywhere` scope already model information the rendering layer withholds (narrative fog is host policy, L6). Per-character knowledge hands are permanently out: the engine models a single viewpoint per state.
