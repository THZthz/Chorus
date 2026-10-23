# Chorus² — Kernel API and Implementation Specification

The implementation contract for `story-core` and `story-engine`: host-facing signatures, value shapes, the event stream, the runtime diagnostics catalog, budgets, and the test obligations. Architecture and semantics live in spec.md; the content language in language.md; the crate map and dependency law in spec.md §3.

## 1. Host-Facing API

```rust
pub struct Engine {
    content: Arc<Content>, registry: EffectRegistry, state: State, seed: u64,
    // Boot-derived from content + seed, read-only after boot: declared-zone
    // addresses in declaration order, greedy candidate ordering, zone nesting,
    // the home zone, the turn unit, and the per-deck deal permutations
    // (`State` carries only the deck cursors).
    zone_ids: BTreeMap<String, ZoneId>, zone_order: BTreeMap<ZoneId, u32>,
    zone_parent: BTreeMap<ZoneId, Option<ZoneId>>, home: ZoneId, turn: i64,
    deck_orders: BTreeMap<DeckId, Vec<CardId>>,
}

impl Engine {
    pub fn boot(content: Arc<Content>, registry: EffectRegistry, seed: u64,
                legacy_in: Option<BTreeMap<String, i64>>) -> (Self, Vec<Diagnostic>);
    pub fn commit(&mut self, action: CommitAction) -> Result<Vec<Event>, Diagnostic>;
    pub fn bind(&mut self, sit: SituationId, slot: SlotName, e: EntityId) -> Result<Vec<Event>, Diagnostic>;
    pub fn advance(&mut self, dt: Time) -> Result<Vec<Event>, Diagnostic>;  // dt = k * turn, k >= 1
    // recognition face (read-only, for UI):
    pub fn state(&self) -> &State;
    pub fn preview_commit(&self, action: &CommitAction) -> Result<RecipeId, Diagnostic>;
    pub fn can_bind(&self, sit: SituationId, slot: SlotName, e: EntityId) -> Result<(), Diagnostic>;
    pub fn slot_candidates(&self, recipe: RecipeId, slot: SlotName) -> Vec<EntityId>;
    // persistence face:
    pub fn legacy_out(&self) -> &BTreeMap<String, i64>;
    pub fn snapshot(&self) -> Snapshot;
    pub fn restore(&mut self, snap: &Snapshot) -> Result<(), Diagnostic>;
    // tool faces (design-time drivers; spec.md §4.10):
    // `Engine` and `EffectRegistry` are `Clone`; every `Engine` field beyond
    // `state` is boot-derived and read-only after boot.
    pub fn with_state(&self, state: State) -> Engine;
    pub fn into_state(self) -> State;
    pub fn state_mut(&mut self) -> &mut State;
    // context-free evaluation face (`sit = None`; §5, T9):
    pub fn eval_expr(&self, expr: &Expr) -> i64;
    pub fn eval_cond(&self, cond: &Expr) -> bool;
    pub fn matches(&self, e: EntityId, terms: &[QueryTerm]) -> bool;
    pub fn scope_entities(&self, scope: &Scope) -> Vec<EntityId>;
    // forkable settlement face — the ordinary settlement of commit/advance;
    // at each chance point the callback supplies the outcome, or `None`
    // runs the kernel's deterministic path:
    pub fn commit_forked(&mut self, action: CommitAction,
        fork: &mut dyn FnMut(&ChancePoint) -> Option<ChanceOutcome>)
        -> Result<Vec<Event>, Diagnostic>;
    pub fn advance_forked(&mut self, dt: Time,
        fork: &mut dyn FnMut(&ChancePoint) -> Option<ChanceOutcome>)
        -> Result<Vec<Event>, Diagnostic>;
}
pub struct CommitAction { pub verb: VerbId, pub bindings: Vec<(SlotName, EntityId)> }
```

- `boot` validates the content-fixed seed (`SeedMismatch` is blocking; `seed host` content delegates the seed to the argument), folds `legacy_in` over the declared defaults, arms boot instances (timers anchored at t=0), and runs one autonomous checkpoint at t=0. Its diagnostics split into warnings (play may proceed) and errors (the host must not proceed).
- `can_bind` addresses open slots of a live situation; `slot_candidates` addresses a recipe, not a verb, because two recipes of one verb may reuse a slot name under different filters.
- `preview_commit` runs the selection pipeline — eligibility, minimum-fill, when-scan, guard — and reports the would-be selection including the guard outcome, without mutating state or spending time.
- Invariants: every entry point returns `Result<_, Diagnostic>`; content-driven failure is always a value, never a panic. `commit` is all-or-nothing. `advance` is atomic per resolution unit (§9).

```rust
pub enum ChancePoint {
    Roll { count: u32, sides: u32, bonus: i64, bind: String },
    Draw { deck: DeckId, count: u32, into: Option<Into> },
}
pub enum ChanceOutcome { Roll(i64), Draw(Vec<CardId>) }
```

- **Tool faces** (spec.md §4.10): `with_state` / `into_state` / `state_mut` move the `State` of an engine in and out without re-booting or snapshot serialization — `with_state` clones only the boot-derived fields and takes the state by value.
- The **context-free evaluation face** evaluates expressions, conditions, query terms, and scopes with `sit = None` through the kernel's own machinery (§5, T9) — goal and invariant checking stay in the kernel, never duplicated in tools.
- The **forkable settlement face** pauses at each chance point: one fires only for `count ≥ 1` and `sides ≥ 1` — count 0 applies the bonus only and never pauses (T11). `ChanceOutcome::Roll` carries the full bound value, bonus included; a forked `Draw` mints the supplied cards in order and advances the deck cursor by the requested count, falling through to the kernel's fallback/wrap path past the permutation's end. Per-unit rollback is unchanged (T4).

## 2. State and Value Types

```rust
pub struct State {
    pub now: Time, pub rng_counter: u64,
    pub next_entity: u64, pub next_situation: u64, pub next_zone: u64,
    pub entities: BTreeMap<EntityId, Entity>,
    pub zones: BTreeMap<ZoneId, Zone>,
    pub situations: BTreeMap<SituationId, Situation>,
    pub relations: Relations,                   // subject -> kind -> objects; spec.md §4.9
    pub decks: BTreeMap<DeckId, u64>,           // permutation cursors
    pub recipe_runs: BTreeMap<RecipeId, u32>,   // max counters (+1 at instantiation)
    pub fired_once: BTreeSet<TriggerRuleId>,
    pub legacy: BTreeMap<String, i64>,
    pub ended: Option<EndingId>,
}

pub type Relations = BTreeMap<EntityId, BTreeMap<String, BTreeSet<EntityId>>>;

pub struct Entity {
    pub id: EntityId, pub name: String, pub prototype: CardId,
    pub kind: Option<String>,                    // matches kind:<k> query terms
    pub aspects: BTreeMap<String, i64>,
    pub attrs: BTreeMap<String, i64>,            // kernel-internal: host ops and bookkeeping only
    pub zone: ZoneId, pub mint: MintPolicy, pub expires_at: Option<Time>,
}

pub struct Zone { pub id: ZoneId, pub kind: ZoneKind, pub public: bool, pub bindable: bool }
pub enum ZoneKind { Table, Hidden, SituationInterior(SituationId), Of(EntityId) }

pub struct Situation {
    pub id: SituationId, pub verb: VerbId,
    pub source: SituationSource, pub recipe: RecipeId,
    pub interior: ZoneId,
    pub bindings: BTreeMap<SlotName, Vec<Binding>>,
    pub started_at: Time, pub ends_at: Time,
}
pub enum SituationSource { Commit, Autonomous, Start { parent: SituationId } }
pub struct Binding { pub entity: EntityId, pub bound_from: ZoneId, pub seq: u64 }   // seq: bind order within the situation
```

Key points: aspect keys are flat; **locks are derived** (walk `situations.bindings`; there is no redundant lock table); zones are first-class (the `public × bindable` policy bits carry visibility and bindability, defaults per spec.md §4.1); `clock` reads `state.now`, which starts at 0 at boot. `attrs` never receive writes from authored effects — the compiled IR contains no attribute-writing node. `relations` stores one row per logical edge — subject by entity id, kinds lexicographic, objects by entity id — a symmetric kind's row under the canonical subject `min(from, to)`; pure edge ops (`relate_edge`, `unrelate_edge`, `sever_pair`, `drop_entity_edges`, `edge_exists`, `edges_into`, `edges_touching`) live beside `State` in `story-core` and take the declared kind's flags from `Content.relation_kinds`. Destruction drops every edge touching the dying card, silently; `move` never touches edges.

A snapshot is the full `State` plus an envelope:

```rust
pub struct Snapshot { pub format: u32, pub content_fingerprint: u64, pub state: Vec<u8> }
```

`restore` checks format (`FormatVersion`) then fingerprint (`ContentMismatch`); a mismatch discards the snapshot — the host recompiles and reboots. The undo stack is host-held (`State: Clone`), pushed before every player-visible action.

## 3. The Event Enum

The v1 event set is closed (L6: structured events out; the host renders):

```rust
pub enum Event {
    /// Renderable prose.
    Narration { text: String },
    /// A card joined an open slot mid-run.
    Bound { sit: SituationId, slot: SlotName, entity: EntityId },
    /// An instance finished its completion sequence.
    SituationCompleted { sit: SituationId, recipe: RecipeId },
    /// A situation adopted a `continue via` target instead of completing:
    /// the same instance goes on executing the target recipe with its timer
    /// re-armed (spec.md §4.2).
    Continued { situation: SituationId, recipe: RecipeId },
    /// A start chain ended silently: no eligible recipe, every `when` false,
    /// or the winner's guard refused.
    ChainEnded { verb: VerbId },
    /// An open was skipped: instance cap or materialization shortfall.
    StartSkipped { verb: VerbId, reason: StartSkipReason },
    /// A mint was skipped by uniqueness policy.
    SpawnSkipped { name: String },
    /// A resolution unit failed and rolled back; its situation aborted through
    /// the release path. Advance traffic, deliberately not a Diagnostic.
    ResolutionFailed { sit: SituationId, reason: String },
    /// A logical relation edge was written by an authored `relate` (one per
    /// written edge, authored orientation). Boot edges emit nothing.
    Related { from: EntityId, kind: String, to: EntityId },
    /// A logical relation edge was removed by an authored `unrelate` or an
    /// exclusive steal (one per removed edge). A kinded `unrelate a from
    /// b as k` reports the authored orientation; steal victims and kindless
    /// severing report the stored orientation. Destruction drops edges
    /// silently — never an `Unrelated`.
    Unrelated { from: EntityId, kind: String, to: EntityId },
}
pub enum StartSkipReason { InstanceCap, MaterializationFailed }
```

## 4. The Effect Registry

```rust
pub trait Effect { fn apply(&self, state: &mut State, ctx: &mut EffectCtx) -> Vec<Event>; }

pub enum Target {
    Name { card: CardId, aspect: Option<AspectKey> },  // aspect present on Set; unique-checked at compile
    Slot(SlotName),
    Query { scope: Scope, terms: Vec<QueryTerm> },     // explicit scope
    BareQuery(Vec<QueryTerm>),                          // interior-then-world fallback
    Handle(HandleName),
}

pub enum EffectNode {
    Spawn { prototype: CardId, count: Expr, into: Into, handle: Option<HandleName> },
    Destroy { target: Target, count: DestroyCount },    // All | AtMost(Expr)
    Consume { target: Target, count: Expr },
    Swap { target: Target, count: Expr, to: CardId, into: Option<Into> },
    Set { target: Target, key: AspectKey, op: SetOp, expr: Expr, unclamped: bool },
    Move { target: Target, into: Into },
    Legacy { key: LegacyKey, op: SetOp, expr: Expr },   // always clamps at ≥ 0
    Narrate { template: String },
    Roll { count: Expr, sides: Expr, bonus: Expr, bind: String },
    Draw { deck: DeckId, count: Expr, into: Option<Into> },
    Start { verb: VerbId, via: Option<RecipeId>, count: Expr },
    Continue { recipe: RecipeId },                      // recipe effects only; [E247] elsewhere
    Relate { from: Target, to: Target, kind: String },  // total; one Related per written edge
    Unrelate { from: Target, to: Target, kind: Option<String> },  // None severs every edge per pair
    Signal { event: String },
    Ending { id: EndingId },
    Branch { cond: Expr, then: Vec<EffectNode>, els: Vec<EffectNode> },
    Op { name: String, args: Vec<Expr> },               // host-only; never emitted by story-lang
}

pub enum RegistryEntry {
    Compiled(Vec<EffectNode>),
    Host(Arc<dyn Fn(&mut State, &mut EffectCtx) -> Vec<Event> + Send + Sync>),
}

pub struct EffectCtx {
    pub sit: Option<SituationId>,       // None in context-free books (expire, event)
    pub now: Time,                      // the stepped now
    pub roll: Option<i64>,              // last-wins roll binding
    pub handles: BTreeMap<HandleName, Vec<EntityId>>,  // mint-ordered pools; cleared at list end
    pub continued: Option<RecipeId>,    // the `continue via` this list executed; last-wins
}
```

Extension without kernel changes: host Rust registers `Host` closures by name; `boot` verifies that every referenced op is registered (`UnregisteredOp`). `include` and `effectset` are story-lang compile steps; `Content` stores only the expanded IR.

- **Target application** (per target form): `Name` → the one entity; `Slot` → the context's bindings for that slot, per-binding application; `Query` → candidates in the scope, oldest-first (mint order) within the scope; `BareQuery` → interior-oldest-first, then world-oldest-first; `Handle` → the unit's pool, oldest-first. Counts walk the resolved order; `Set` writes every resolved card.
- **Handles**: a `spawn ... as h` appends the minted ids to `ctx.handles[h]`; the pool lives for the enclosing effect list and is cleared when the list ends. A handle target the compiler cannot see an enclosing spawn for is [E238] (an orphan handle-shaped prose token is [E205]).
- **Roll**: `splitmix64(seed ^ rng_counter)` per die, counter in `State`, incremented once per die. Count and sides are evaluated as integer expressions at effect time; count < 0 or sides < 1 is an effect failure (unit rollback). Count 0 applies the bonus only and leaves `rng_counter` unmoved. The result lands in `ctx.roll` (last-wins).
- **Ending**: sets `state.ended`; further endings in the same advance are no-ops.

## 5. Engine-Side Queries and Expressions

```rust
enum Scope { Interior, Bindings, Slot(SlotName), Zone(ZoneId), Related { kind, of: CardRef, incoming },
             Table, World /* public */, Anywhere }
enum CardRef { Name(CardId), Subject, Query { scope, terms } }   // relation endpoints
enum Expr { Lit(i64), Bool(bool), NamePath { card, key }, Subject(key),
            Aspect { scope, query, key }, Count { scope, query }, Exists { scope, query },
            Attr { scope, query, key }, Legacy(LegacyKey), Roll, Counter, Clock,
            Alive(String), Rel { kind, from: CardRef, to: CardRef }, Arith, Cmp, Logic, Not }
```

The engine-side grammar mirrors the DSL's (language.md §3). `Interior` is the interior zone of the situation whose resolution is currently running — for a `start` fired inside a completion, the parent situation's interior with its still-locked bindings. `Slot(s)` is the context's binding set for that slot name. `Zone(z)` includes the nested subtree. `Bindings` (the `bound` scope) counts per binding — a card bound in two slots counts twice; it reads empty outside a running situation, as do `Interior` and `Slot`. `Table` is the public live cards plus every situation's non-greedy slot bindings; greedy bindings and situation interiors are excluded. `kind:<k>` terms compare against `Entity.kind`; the compiler validates the kind against the collected card declarations [E203].

**Totality.** Evaluation cannot fail: an aspect read falls back to the key's declared read default — a card without the key and a dead or absent entity alike — and an attr read is 0; empty-set `count` = 0 and `exists` = false; an unbound slot scope is empty; an unbound roll is 0; `x / 0` = 0; a constant zero divisor is a compile error. `Rel` with a dead or absent endpoint reads false, and a `Related` scope on one enumerates nothing — `resolve_card_ref` (`Name` → the unique card's live entity, `Subject` → the context's subject, `Query` → the scope's first term-matching candidate in scope order) is total the same way. Symmetric kinds read direction-agnostically; enumeration is deduplicated, ascending entity id (mint order), with no reverse index — incoming enumeration scans rows whose object set contains the endpoint. Derived predicates are inlined at story-lang compile time — the kernel never sees one.

## 6. Time, Settlement, and Budgets

- **advance(dt) settlement.** `target = now + dt`; the loop repeatedly takes the least due item under the total-order key `(moment, phase, id)`, with `phase: 0=Completion < 1=Expiry < 2=Autonomous` and `id` the mint order. Before resolving an item, the engine sets `state.now = item.moment`; checkpoints, effects, spawn anchoring, and event timestamps observe the stepped now. When the loop drains, `now = target`.
- **Per-unit failure policy.** Every resolution unit checkpoints (a `State` clone) before its first effect. An effect failure rolls the unit back to that checkpoint; a failed completion unit's situation aborts through the release path (bindings released — `take` slots to `bound_from`, the rest in place — interior leftovers to `releases` / `home`, interior destroyed) and `Event::ResolutionFailed { reason }` is emitted for it, while a failed expiry unit — no situation to name — rolls back silently and the timer is spent; the loop continues with the next item.
- **Budgets and constants:**
  - `MAX_RESOLUTIONS_PER_ADVANCE = 4096` — exceeding it rolls the entire advance back to its entry checkpoint and returns `Diagnostic::Livelock`. The only whole-advance rollback.
  - `MAX_RESOLUTION_DEPTH = 128` — the native recursion depth of inline resolution (instant `start` chains, signal cycles); a breach takes the same Livelock rollback path instead of exhausting the stack.
  - `EFFECTSET_EXPANSION_DEPTH = 16` — compile-time cap, shared with predicate inlining depth policy; exceeding is [E235].
- **Expiry.** The anchor is the mint moment (the resolution's stepped now); expiry is a one-shot timer cleared at settlement. An expiry item that finds its card locked is deferred: on release the card re-enters the due queue if past due and expires at the next resolution pass; `on expire` books do not fire while the card is bound.
- **Autonomous checkpoints.** At every distinct settled moment (after phases 0/1 finish) and once at the end of the advance. In verb declaration order, each autonomous verb not at its `instances` cap runs the selection law over empty bindings; a hit materializes at most one instance; a miss is silent. Boot runs one checkpoint at t=0.
- **Seed handling.** `seed <N>` content requires `boot` to receive the same seed (`SeedMismatch`, blocking); `seed host` content takes the boot argument. The seed participates in the RNG stream and in deck permutation setup.

## 7. Runtime Diagnostics Catalog

`Diagnostic { kind, message, context }`, in four families. Compile diagnostics (E-numbers) live in language.md §9 and are produced by `story-content` / `story-lang`; this catalog is the runtime face.

- **Lookup:** UnknownVerb / UnknownEntity / UnknownRecipe / UnknownZone / UnknownDeck / UnknownEnding / UnresolvableRef / UndeclaredEvent
- **Eligibility (commit and bind refusals):** VerbAutonomous / StoryEnded / InstanceCapReached / EntityDead / EntityLocked / ZoneNotBindable / SlotFilterFail / SlotUnderMin / SlotOverMax / NoRecipe / GuardRefused { prose }
- **Effects:** NameInUse / DeckExhausted / UnregisteredOp / BadLifetime / BadDuration / EffectFailure
- **Advance / Snapshot / Boot:** Livelock / FormatVersion / ContentMismatch / Deserialize / SeedMismatch

`ResolutionFailed { reason }` is deliberately not a diagnostic: it is an `Event` on the advance failure path (§6). The commit path validates fully before mutating; the advance path leans on per-unit checkpoints. The boot warning stream replaces a separate lint tool.

## 8. Matching Notes: Targets and Trigger Sources

Implementation notes pairing with spec.md §4.5–§4.7 and language.md §4–§5:

- **Target resolution.** `Name` requires the card to be `unique` (compile-checked [E239]); resolve the live entity by name. `Slot` resolves against the running context's bindings and applies per binding. `Query` collects candidates inside its scope ordered by mint order (oldest first). `BareQuery` resolves interior-first, then world. `Handle` indexes `ctx.handles`; the pool is empty outside its list by construction.
- **Trigger-rule sources at completion.** The completion step walks, in order: the recipe's `OnComplete` book; then one entry per interior card whose declaration carries a completion book — the bound bindings first, ordered by first bind (deduplicated across slots), then the unbound interior storage cards in mint order, deduplicated by entity across both groups — each book walked under the selection law. An aspect-qualified card book is included when the completing recipe's `declares` list contains the qualifier, or when the situation's interior aspect pool — the read-value sums over the interior, bound cards included — contains it AND the walking card's own declaration — its element id and aspect keys, mutations included — carries the qualifier (the two catalyst passes with their per-stack scoping: recipe declares, then situation aspects over stacks whose own elements declare the trigger). Inside a card book, `ctx` binds `subject` to the carrying card. The kernel abort path (steps 3–5 on failure) walks the recipe's book alone.
- **Continuation.** The completion path takes the `EffectNode::Continue` the executing recipe's effect list executed — recorded on the taken path, so branch semantics are ordinary and a `continue` inside an untaken branch never fires — and runs the adoption transition after the books: non-consuming bindings released into the interior as unbound storage, then the recipe's `continue` arms (the static pre-order of the list) re-check their `when`s in order against the released pool — the interior sums, the transition's one gate, decided pre-materialization; a recipe whose `max` is spent misses like a failed `when` — starting at the executed arm; the first arm that passes is adopted — recipe and verb both — and then materializes without a further gate: greedy slots pulled from the world, one `max` count spent (adoption materializes the new recipe), the timer re-armed, `Continued` emitted; a miss on every arm completes the situation. Each step charges the budget; the inline resolution of a 0-duration target recurses natively and is depth-capped like every inline chain (§6).
- **Start candidacy and materialization.** For `start` and autonomous sources, the minimum-fill test counts greedy candidates in the current state: entities that are unbound, in public bindable zones, and filter-matching for the slot. A recipe passing the test but failing materialization (greedy top-up still short of a minimum) yields `StartSkipped { reason: MaterializationFailed }` with no rollback of the parent resolution.
- **Locking.** `can_bind` refuses `EntityLocked` for any entity bound by any live instance, regardless of which situation asks.
- **Context-free books.** `on expire` and `on event` rules evaluate with `sit = None`: `Interior`, `Slot`, and `Bindings` scopes read as empty sets.
- **Relations.** `Relate` resolves both targets (shared target grammar), then applies subjects × objects in resolved order: self-pairs skipped silently, exclusive kinds steal first (`Unrelated` per victim in map order, stored orientation, before the `Related`), one `Related` per written logical edge (authored orientation). `Unrelate` with a kind removes that edge per pair; without, `sever_pair` removes every logical edge between the pair, either direction, once per edge. `destroy_entity` calls `drop_entity_edges` — one edit covering destroy, consume, swap, and default expiry — silently. Boot applies card `relates` lines after all boot mints and before the t=0 checkpoint, in card declaration order, objects resolving to every live same-name entity; missing objects are boot warnings (`UnresolvableRef`, warning severity) and no events are emitted. Slot eligibility and greedy candidacy never read relations; a slot filter may test them through a `QueryTerm::Pred` whose body evaluates with the candidate as `ctx.subject`.

## 9. Testable Invariants

Each statement is directly assertable by a test:

- **T1 Totality.** Any expression evaluates to a value in any state: aspect reads fall back to the declared read default and attrs read 0 — a card without the key and a dead or absent entity alike; empty scopes count 0 / exist false; unbound slot scopes are empty; unbound rolls read 0; `x / 0` = 0.
- **T2 Zero-divisor.** A constant zero divisor is refused at compile time [E242].
- **T3 Commit atomicity.** Any commit refusal (`NoRecipe`, `GuardRefused`, `InstanceCapReached`, `StoryEnded`, `VerbAutonomous`, …) leaves the state bit-identical and spends zero time.
- **T4 Per-unit atomicity.** An effect failure rolls the resolution unit back to its checkpoint — `rng_counter` and deck cursors included — aborts the unit's situation through the release path, emits `ResolutionFailed` (a completion unit; a failing expiry unit rolls back silently and spends its timer), and the advance continues.
- **T5 Livelock.** More than `MAX_RESOLUTIONS_PER_ADVANCE` resolutions — or nesting deeper than `MAX_RESOLUTION_DEPTH` — in one call rolls the entire call back to its entry checkpoint and returns `Livelock`.
- **T6 Determinism.** Same content + seed + operation sequence ⇒ identical state, including a retried identical `commit` after a refusal (bit-restored dice).
- **T7 Ending.** After any ending: the due queue is discarded unresolved; every subsequent `commit`/`advance` returns `StoryEnded`; restoring the pre-ending snapshot resumes play.
- **T8 Locks are total.** `can_bind` returns `EntityLocked` for any entity bound by any live instance, from any asking situation.
- **T9 Context-free scopes.** Evaluating `interior`, `slot:`, or `bound` inside `on expire` rules, `on event` rules, commit selection, or autonomous polls yields empty sets.
- **T10 Start candidacy.** A `start`/autonomous open whose slot minimum exceeds the state-countable greedy candidates never materializes; a recipe that matches but materializes short yields `StartSkipped { MaterializationFailed }` with siblings and parent untouched.
- **T11 Roll validation.** Count 0 applies the bonus only and leaves `rng_counter` unmoved; count < 0 or sides < 1 is an effect failure rolling the unit back.
- **T12 Card books.** At completion the recipe's book fires first, then interior card books — bound bindings deduplicated across slots in bind order, then unbound interior storage in mint order; an aspect-qualified book fires under a matching `declares`, or under an interior pool holding the aspect when the walking card's own declaration carries it too; inline completions run card books inside one budget unit.
- **T13 Handle scope.** A handle pool is empty outside its effect list; a static reference outside its list is [E238].
- **T14 Per-binding writes.** `set slot:<name> aspect:<k> += 1` writes exactly once per bound card; `total(bound, aspect:<k>)` counts a twice-bound card twice.
- **T15 Continuation.** A `continue via` step adopts the target recipe, releases the non-consuming bindings into the interior as unbound storage, re-arms the timer, and emits `Continued`; a target whose `when` misses the interior sums — or whose `max` is spent — completes normally; a timer-bounded self-continue loops once per re-arm, and an instant self-continue loop ends in the Livelock rollback.
- **T16 Table scope.** `table` counts the public live cards plus every situation's non-greedy slot bindings, and excludes greedy bindings and situation interiors.
- **T17 Relation totality.** `rel(...)` reads false and `related:` scopes read empty for dead or absent endpoints (an unresolved `first(...)` included); `count`/`total` read 0, existence tests read false, in every book context including context-free ones.
- **T18 Relation persistence.** Same content + seed + operations give identical `state.relations` (canonical orientation included); `snapshot()` → `restore()` round-trips relations; undo (the host-held pre-action snapshot) restores them bit-for-bit after a steal and after a destruction.
- **T19 Destroy drops edges.** Destroying either endpoint removes every logical edge touching it, via `destroy`, `consume`, `swap`, and default expiry destruction; the surviving endpoint's other edges survive; the drops are silent.
- **T20 Symmetric and exclusive maintenance.** A symmetric `relate` stores one canonical row yet reads as `rel` both ways, and `unrelate` clears it; an exclusive directed `relate` removes prior holders (`Unrelated` per victim, map order, before the `Related`); a symmetric exclusive `relate` steals prior partners of either endpoint; non-exclusive kinds accumulate holders.
- **T21 Event fidelity.** Exactly one `Related` per written and one `Unrelated` per removed logical edge from authored effects and steals; self-pairs, no-op unrelates, and destroy-drops emit nothing.

## 10. M2 Test Matrix

Behavioral coverage contract: at least one positive and one refusal path per mechanism. The acceptance sample `emberwatch.sw` covers the core loop and deliberately stops there; this matrix owns the rest.

**Mechanisms the sample does not exercise:**

1. mint aliasing
2. take slots
3. group uniqueness
4. start via / count
5. releases override
6. on-expire books
7. destroy / swap
8. anywhere scope
9. zone nesting
10. fallback exhaustion
11. unique-mint skip
12. into \<card\>
13. negated query terms
14. signal / on-event books
15. multi-advance fan-out
16. the guard-refused refusal
17. wrap cycling
18. effectset expansion
19. include splicing
20. meta pass-through

**Instant-recipe behaviors:**

21. `duration 0` inline resolution on commit — the open and completion events return together;
22. the recursion budget across an instant `start` chain (the livelock rollback path);
23. the single autonomous pulse of an all-instant verb.

**New-mechanism entries (targets, card books, handles, dynamic rolls):**

24. shared targets — slot-target per-card `set` write; scoped-query oldest-first; bare-query interior→world fallback;
25. card-carried completion books — unqualified, aspect-qualified under `declares`, dedup across slots in bind order, inline-completion interplay;
26. spawn handles — single mint, multi pool oldest-first, scope expiry;
27. dynamic roll operands — expression count/sides, count-0 rng behavior, invalid-operand rollback;
28. `bound` scope — per-binding double-count, empty outside a situation;
29. `kind:` terms — positive, negated, unknown-kind compile refusal;
30. start/autonomous candidacy by state-countable greedy fill, with the `StartSkipped { MaterializationFailed }` path.
31. mid-run `bind` — the open-slot happy path: the `Bound` event, the interior move, release to `bound_from`, and open-slot capacity interplay.
32. `continue via` — adoption, interior storage release, and timer re-arm; the `when`-miss fall-through; per-step books; timer-bounded self-continue; the instant self-continue Livelock bound.
33. the `table` scope — world cards and non-greedy bindings counted, greedy bindings and interiors excluded.
34. the widened card-book walk — unbound interior storage books at completion, and the interior-pool catalyst pass for aspect-qualified books, scoped per stack to cards whose own declaration carries the qualifier.

**Relations entries (spec.md §4.9):**

35. `relate`/`unrelate` basic — the written and removed edges read and speak; multi-target endpoints apply per pair in resolved order;
36. symmetric two-way read over single canonical storage — one row under the min endpoint, `rel` holds both ways, enumeration matches from either end;
37. exclusive steal with events — directed per-object eviction and symmetric valence-1 partnership, `Unrelated` per victim before the `Related`;
38. `rel()` positive/negative/dead-endpoint faces, inside a slot-filter predicate (candidate as subject) and an expiry book (`subject`);
39. the `related:` scope in `count`/existence/`total` and as an effect target (per-card `set` walk), enumerating over cards living in situation interiors and carrier zones;
40. boot `relates` lines — edges present at t=0, multi-count objects resolving to every live copy, missing-object warning with play proceeding;
41. snapshot round-trip and undo of relations, after a steal and after a destruction;
42. move independence — relations survive `move` into zones and carriers, and die on destroy, silently;
43. `first(...)` endpoints — oldest-copy resolution, the `slot:` endpoint, empty reads false/empty;
44. `unrelate` without a kind severs all kinds between the pair, either direction, once per logical edge.

Every entry in §9 (T1–T21) carries an executable assertion as well.
