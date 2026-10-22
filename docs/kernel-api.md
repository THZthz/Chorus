# Chorus² — Kernel API and Implementation Specification

The implementation contract for `story-core` and `story-engine`: host-facing signatures, value shapes, the event stream, the runtime diagnostics catalog, budgets, and the test obligations. Architecture and semantics live in spec.md; the content language in language.md; the crate map and dependency law in spec.md §3.

## 1. Host-Facing API

```rust
pub struct Engine { content: Arc<Content>, registry: EffectRegistry, state: State }

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
}
pub struct CommitAction { pub verb: VerbId, pub bindings: Vec<(SlotName, EntityId)> }
```

- `boot` validates the content-fixed seed (`SeedMismatch` is blocking; `seed host` content delegates the seed to the argument), folds `legacy_in` over the declared defaults, arms boot instances (timers anchored at t=0), and runs one autonomous checkpoint at t=0. Its diagnostics split into warnings (play may proceed) and errors (the host must not proceed).
- `can_bind` addresses open slots of a live situation; `slot_candidates` addresses a recipe, not a verb, because two recipes of one verb may reuse a slot name under different filters.
- `preview_commit` runs the selection pipeline — eligibility, minimum-fill, when-scan, guard — and reports the would-be selection including the guard outcome, without mutating state or spending time.
- Invariants: every entry point returns `Result<_, Diagnostic>`; content-driven failure is always a value, never a panic. `commit` is all-or-nothing. `advance` is atomic per resolution unit (§9).

## 2. State and Value Types

```rust
pub struct State {
    pub now: Time, pub rng_counter: u64,
    pub next_entity: u64, pub next_situation: u64, pub next_zone: u64,
    pub entities: BTreeMap<EntityId, Entity>,
    pub zones: BTreeMap<ZoneId, Zone>,
    pub situations: BTreeMap<SituationId, Situation>,
    pub decks: BTreeMap<DeckId, u64>,           // permutation cursors
    pub recipe_runs: BTreeMap<RecipeId, u32>,   // max counters (+1 at instantiation)
    pub fired_once: BTreeSet<TriggerRuleId>,
    pub legacy: BTreeMap<String, i64>,
    pub ended: Option<EndingId>,
}

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
pub struct Binding { pub entity: EntityId, pub bound_from: ZoneId }
```

Key points: aspect keys are flat; **locks are derived** (walk `situations.bindings`; there is no redundant lock table); zones are first-class (the `public × bindable` policy bits carry visibility and bindability, defaults per spec.md §4.1); `clock` reads `state.now`, which starts at 0 at boot. `attrs` never receive writes from authored effects — the compiled IR contains no attribute-writing node.

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
    Set { target: Target, key: AspectKey, op: ArithOp, expr: Expr, unclamped: bool },
    Move { target: Target, into: Into },
    Legacy { key: LegacyKey, op: ArithOp, expr: Expr },
    Narrate { template: String },
    Roll { count: Expr, sides: Expr, bonus: Expr, bind: String },
    Draw { deck: DeckId, count: Expr, into: Option<Into> },
    Start { verb: VerbId, via: Option<RecipeId>, count: Expr },
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
}
```

Extension without kernel changes: host Rust registers `Host` closures by name; `boot` verifies that every referenced op is registered (`UnregisteredOp`). `include` and `effectset` are story-lang compile steps; `Content` stores only the expanded IR.

- **Target application** (per target form): `Name` → the one entity; `Slot` → the context's bindings for that slot, per-binding application; `Query` → candidates in the scope, oldest-first (mint order) within the scope; `BareQuery` → interior-oldest-first, then world-oldest-first; `Handle` → the unit's pool, oldest-first. Counts walk the resolved order; `Set` writes every resolved card.
- **Handles**: a `spawn ... as h` appends the minted ids to `ctx.handles[h]`; the pool lives for the enclosing effect list and is cleared when the list ends. A handle target the compiler cannot see an enclosing spawn for is [E238] (an orphan handle-shaped prose token is [E205]).
- **Roll**: `splitmix64(seed ^ rng_counter)` per die, counter in `State`, incremented once per die. Count and sides are evaluated as integer expressions at effect time; count < 0 or sides < 1 is an effect failure (unit rollback). Count 0 applies the bonus only and leaves `rng_counter` unmoved. The result lands in `ctx.roll` (last-wins).
- **Ending**: sets `state.ended`; further endings in the same advance are no-ops.

## 5. Engine-Side Queries and Expressions

```rust
enum Scope { Interior, Bindings, Slot(SlotName), Zone(ZoneId), World /* public */, Anywhere }
enum Expr { Lit(i64), Bool(bool), Aspect { scope, query, key }, Count, Exists, Attr,
            BoundCount, Legacy, Roll, Counter, Clock, Alive(String), Arith, Cmp, Logic, Not }
```

The engine-side grammar mirrors the DSL's (language.md §3). `Interior` is the interior zone of the situation whose resolution is currently running — for a `start` fired inside a completion, the parent situation's interior with its still-locked bindings. `Slot(s)` is the context's binding set for that slot name. `Zone(z)` includes the nested subtree. `Bindings` (the `bound` scope) counts per binding — a card bound in two slots counts twice; it reads empty outside a running situation, as do `Interior` and `Slot`. `kind:<k>` terms compare against `Entity.kind`; the compiler validates the kind against the collected card declarations [E203].

**Totality.** Evaluation cannot fail: absent aspect and attr reads are 0; dead entities read 0; empty-set `count` = 0 and `exists` = false; an unbound slot scope is empty; an unbound roll is 0; `x / 0` = 0; a constant zero divisor is a compile error. Derived predicates are inlined at story-lang compile time — the kernel never sees one.

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
- **Trigger-rule sources at completion.** The completion step walks, in order: the recipe's `OnComplete` book; then one entry per distinct locked card whose declaration carries a completion book — deduplicated across slots, ordered by first bind — each book walked under the selection law. An aspect-qualified card book is included only when the completing recipe's `declares` list contains the qualifier. Inside a card book, `ctx` binds `subject` to the carrying card. The kernel abort path (steps 3–5 on failure) walks the recipe's book alone.
- **Start candidacy and materialization.** For `start` and autonomous sources, the minimum-fill test counts greedy candidates in the current state: entities that are unbound, in bindable zones, and filter-matching for the slot. A recipe passing the test but failing materialization (greedy top-up still short of a minimum) yields `StartSkipped { reason: MaterializationFailed }` with no rollback of the parent resolution.
- **Locking.** `can_bind` refuses `EntityLocked` for any entity bound by any live instance, regardless of which situation asks.
- **Context-free books.** `on expire` and `on event` rules evaluate with `sit = None`: `Interior`, `Slot`, and `Bindings` scopes read as empty sets.

## 9. Testable Invariants

Each statement is directly assertable by a test:

- **T1 Totality.** Any expression evaluates to a value in any state: absent aspects/attrs read 0; dead entities read 0; empty scopes count 0 / exist false; unbound slot scopes are empty; unbound rolls read 0; `x / 0` = 0.
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
- **T12 Card books.** At completion the recipe's book fires first, then card books deduplicated across slots in bind order; an aspect-qualified book fires only under a matching `declares`; inline completions run card books inside one budget unit.
- **T13 Handle scope.** A handle pool is empty outside its effect list; a static reference outside its list is [E238].
- **T14 Per-binding writes.** `set slot:<name> aspect:<k> += 1` writes exactly once per bound card; `total(bound, aspect:<k>)` counts a twice-bound card twice.

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

Every entry in §9 (T1–T14) carries an executable assertion as well.
