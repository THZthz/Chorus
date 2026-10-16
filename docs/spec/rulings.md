# Chorus Text Engine — §9 Decisions (implementation rulings)

This file carries §9 of [text-engine-v0.6.md](text-engine-v0.6.md) plus the
spec's version changelog. Ruling numbers §9.1–§9.50 are stable citable
identifiers: every §9.x reference across `src/`, `tests/`, the README, and
the tools resolves here, unchanged. Rulings append; existing ones are never
renumbered.

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
    never the step context (§5.2, §9.15). The authored `expires <duration>`
    modifier (§4.4, §9.36) concretizes the same way: the modifier itself is
    authored-sugar only and never reaches the log — the mint's
    `expires`/`expiresAt` pair lands INSIDE the concrete row's `attrs=`
    materialization, so the fold replays byte-identically with no new log
    surface. The modifier is its own arg shape, deliberately not a sniffed
    bare value: the `key=value` spelling is NOT the modifier
    (`SPAWN X expires=2h` is a loud parse error — the modifier is the space
    form `expires 2h`), and a §9.40 parameter expanded into a pair position
    lands as a bare value the grammar cannot author and rejects the step
    loud (§9.19) rather than reading as — or against — the modifier.
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
    re-author or migrate them by hand. The established architecture is not
    frozen either: it may be changed when there is need, but the spec must
    be amended in the same change to describe the architecture as it now is.

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
    entity — rejects the whole step with nothing logged (§9.19). (Revised by
    §9.38: a stackable successor may share a stackable pool — `REPLACE X WITH
    X` then succeeds, append-then-pop.) (A replaced
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

### Implementation rulings (v0.2.7, endings)

35. **Game-over discipline**: the `ENDING` outcome ends the story, and the
    story stays ended — until undone. Precisely:
    - **First wins.** The first ENDING to *apply* in a step sets
      `state.ended = {id, title, flavour}`; later ENDINGs in the same step
      (other clauses' recipes, or events crossing in the same advance) are
      logged no-ops. The action's outcomes apply before its events, so an
      action ENDING outranks an event ENDING within one advance.
    - **The ending rides along.** The step itself completes normally:
      remaining outcomes apply, the clock advances by the verb's duration
      (plus authored `ADVANCE`), and events crossing the elapsed span still
      fire — one atomic step, as ever (§6.6). A `REFUSE` later in the same
      step discards the ending with the whole draft (§6.5): no ending, no
      state change, attempt logged.
    - **Post-ending steps are rejected, nothing logged.** `executeStep` on a
      state whose `ended` is set rejects before validation — the message
      names the ending's title — distinct from `REFUSE` (which logs) and
      from §9.19 rejections (which cannot happen *after* a logged ending,
      since nothing further is ever logged). `Game.handleLine` returns the
      same notice without executing, `Game.suggestions()` returns empty, and
      the TUI shows the banner (`— NO MORE · melancholy —`) while the views
      and buttons keep working.
    - **Events may end the game.** Both trigger forms (§10) may carry
      ENDING — a season/time-trigger chain ending in despair, or an authored
      `on expire` trigger that consumes the card and ends the story (the
      authored outcomes replace the built-in destroy entirely, §9.32, so the
      author destroys the dying card explicitly or it re-fires on every
      later crossing). The event row is an ordinary record; its ENDING is
      the first one applied if the action did not already end the story.
    - **Undo is not special-cased.** `ended` derives from the fold like
      everything else (§7): undo drops the ending record (and its trailing
      events), re-folds, and the game is open again. The concrete log form
      is `ENDING <id>` — it round-trips — and title/flavour resolve from
      content inside `applyOutcome`, the one code path play and fold share,
      so the two agree byte-for-byte. The fold itself never enforces
      game-over (it trusts logged rows, §6.6); the rejection lives in
      `executeStep` only.
    - **Load-time existence check.** An ENDING whose id is not in the
      `endings:` table (§10) is a load-time error, in recipes and in both
      trigger forms — unlike expiry-target names (§9.32), ending ids are
      content structure, not runtime entities.

### Implementation rulings (v0.3, expiry units and the twelve core verbs)

36. **`SET x, expires, …` accepts every §5.1 duration form** (§4.2): the
    quoted-string path goes through the same single duration parser as the
    lexer, so `SET(x, expires, "2halfhour")` materializes `expiresAt` =
    set-time clock + 30 exactly as the bare literal always did — bare
    duration literals were never broken, because the lexer pre-converts them
    to minutes before SET sees them (§5.1), and quoted and bare forms of the
    same literal materialize the same `expiresAt` — the authored `expires`
    attr keeps its authored form (minutes for bare, the string for quoted,
    §9.15), so only that attr and the logged sentence differ. Before v0.3
    the quoted path accepted only `h`/`m`/`d` and rejected the `halfhour`
    form. A value that parses as no duration (`"2fortnights"`) is an
    application failure: the whole step is rejected with nothing logged
    (§9.19). A zero duration (`"0m"`) materializes `expiresAt` = the set-time
    clock and so fires at the first halfhour boundary at or after that minute
    that the clock actually crosses (§6.6) — during the setting step itself
    when it crosses one, otherwise during the next advance's span; unchanged
    from how `"0h"` always behaved. (Extended by the SPAWN `expires
    <duration>` modifier, §4.4: the same rule at mint birth — one write
    shared with SET, so the same forms, the same zero-duration boundary,
    the same loud rejection of a non-duration — anchored to the MINT
    clock, `ctx.now`: the step clock at the table, the window's START
    clock inside a completion (§6.7), a slot/expire trigger's crossing
    tick — never a follow-up derivation; concretizes inside the `attrs=`
    pair per §9.18.)
37. **The core verb set is twelve** (§4.3): `STUDY` (READ's signature —
    `ACTORS STUDY object [AT place]`) and `MANSUS` (EXPLORE's signature —
    `ACTORS MANSUS place`) join the dictionary, so a story authors them with
    an ordinary `verbs:` block (§10) and lenient input recognizes the
    lexemes (§11). Registration changes only the vocabulary — cardinalities,
    queries, duration, recipes, and prose templates remain content (§6.1,
    §5.3); a verb with no template contributes no template line, and its
    recipe `NARRATE`s still speak. Verb lexemes are reserved words (§9.14):
    the seed world's study room is `THE_STUDY` since v0.3 for exactly this
    reason.

### Implementation rulings (v0.3.1, stackable cards)

38. **Names may pool: stackable cards** (§4.5, §5.1, §6.3, §10). Dread,
    Funds, and Suspicion are pools of identical cards, not unique cards. An
    entity line or prototype line may carry the `stackable` keyword (before
    `with`/`deriving`); it lands in compiled content as two derived sets —
    `stackableNames` (the declared names) and `stackableKinds` (the KINDS of
    those declarations). Every mint site (SPAWN, REPLACE; DRAW from v0.4)
    computes its `stackable` flag from the mint's RESOLVED name and kind: a
    mint is stackable iff its name is declared stackable — which declares its
    kind too — or its KIND carries a stackable declaration. The rule is
    kind-level, not form-level: a `kind:K` mint of a stackable kind is the
    explicit spelling, but a plain `SPAWN PROTO` whose prototype's kind
    carries any stackable declaration mints stackable as well. The flag rides
    on the card; the fold recomputes the same rule from the logged ref —
    concrete SPAWN rows always carry the resolved `kind:K`, so play and fold
    decide from the identical (name, kind) pair without new log surface.
    Concretely:
    - **The uniqueness law, revised.** Non-stackable mints are unchanged
      (§9.14, §9.19: a duplicate living name rejects the whole step). A
      stackable mint never collides with its own pool, but MAY NOT collide
      with a distinct NON-stackable living name — one shared namespace: a
      non-stackable holder blocks stackable mints of the same name, and a
      mixed pool is therefore impossible (a non-stackable mint demands an
      empty pool). The name frees only when the LAST copy dies.
    - **Pool addressing = oldest copy.** `state.byName` maps a name to its
      living ids in BIRTH order (insertion order — never id-string order,
      which sorts `e10_1` before `e2_1`). A name ref with N > 1 living copies
      addresses the HEAD of that array — the OLDEST copy — everywhere name
      refs resolve: DESTROY, SET, MOVE, REPLACE, ref positions in every
      outcome, slot resolution, lenient input, `exists(NAME)` (any copy), and
      the play surface (the card listing shows every copy; `inspect` reads
      the oldest).
    - **Authored destroys stay name-addressed.** `DESTROY <poolname>` (and
      any authored name ref) resolves per the pool rule and LOGS THE NAME
      ref, so destroying the oldest, then the next-oldest, is repeatable
      authoring; the fold replays the same resolution deterministically.
    - **Engine-written destroys address by id.** The built-in expiry path
      keys its destroy to the EXPIRING entity — it concretizes to
      `DESTROY #id` — because under a pool a name-addressed destroy would
      kill the oldest copy while the card whose `expiresAt` actually crossed
      survived and re-fired forever. (Replaces the v0.2.6 name-form row.)
    - **The authored-expiry asymmetry (deliberate, documented).** An
      `on expire of NAME` trigger (§9.32) matches by NAME: a crossing of ANY
      copy fires it, once per crossing, even within one advance (each
      crossed halfhour slot re-checks the snapshot). But its outcomes are
      plain authored text: anything referencing NAME addresses the OLDEST
      copy, which under a pool may not be the copy whose expires crossed.
      The crossing copy is therefore not consumed by its own crossing; being
      past its `expires` it re-fires on every later crossing (§9.32), and
      once it becomes the pool's head, the trigger's destroy takes it.
      Authors who need exactly-the-crossing-copy dead use the built-in path
      (no authored trigger) or keep per-copy expires exclusive. The engine
      does NOT silently inject a by-id destroy into authored outcomes —
      §9.32's "authored outcomes replace the built-in destroy entirely"
      contract is preserved.
    - **REPLACE X WITH X succeeds for stackables** (§9.33 revised): the
      spawn appends while the dying card is still alive, then the destroy
      pops it — the pool never dips, and the next pool op addresses the
      next-oldest (now head). A NON-stackable successor colliding with any
      living holder still rejects the whole step (§9.19). Concrete rows
      already carry `AS #id`, so the fold replays the pair exactly.
    - **[doc] Lenient input addresses the oldest copy only** — typed prose
      has no `#id` form of its own; the canonical sentence may name `#id`
      explicitly as the escape hatch. Journal prose renders two pool copies
      bound into one role as "SUSPICION and SUSPICION". `SET <poolname>, …`
      touches only the oldest copy — pool-wide writes need role bindings
      (each bound card gets its own concrete SET, §9.31).
    - **Draft discipline covers the pools** (law 6): `cloneState` DEEP-COPIES
      the `byName` arrays, so a discarded draft's pool destroy (an array
      splice) can never mutate the real state's pool. The fold/JSONL equality
      test snapshot includes the multimap, and the three test-local `snap()`
      helpers are consolidated into one shared helper (test/snap.ts) so this
      cannot drift again.
    - **Companion: counting the pool.** The §4.5 query language gains
      `name:x` (exact-case, negatable), and `count(...)` is overloaded in the
      shared value-expression evaluator to accept any §4.5 query —
      `count(name:SUSPICION) >= 3` — world-scoped like `exists(...)`, so
      F4's `ROLL(count(name:X), 6)` will parse without new grammar. A
      load-time lint fences the arg: `count()`/`total()` arguments must be a
      declared role or a query-starter, so `count(DREAD) >= 3` — the most
      tempting typo — is a load error naming the valid forms, not a silent 0.

### Implementation rulings (v0.3.2, conditionals)

39. **IF / ELSE IF / ELSE in outcome lists** (§4.4, §5.1, §6.8). One
    predicate grammar, three rulings:
    - **Concretization logs the taken branch only.** The IF node is authored
      form: at execution the arms test in order against the DRAFT, and the
      log receives only the taken branch's outcomes, flattened in position —
      the same computation-once/log-the-result shape as SET deltas (§9.31)
      and recipe-id recording. An alternative design — log the IF and
      re-decide at fold — was rejected: the fold would re-run conditions
      against a replaying state (divergence risk) and log formats must carry
      results, not choices. The corollary is accepted: untaken branches are
      invisible to the journal (like §5.3 templates, authored text is not
      log truth). A REFUSE inside a taken branch refuses the whole step
      (§6.5); the recorded attempt carries the branch's outcomes collected
      up to the REFUSE.
    - **The asymmetric failure discipline is deliberate.** The same
      predicate grammar returns false in `when:` positions when an operand
      cannot resolve (recipe selection reads the input state, where roles
      may legitimately be empty), but REJECTS the step (§9.19) when an IF
      condition operand cannot resolve: a `role.aspect.x`/`role.attr.x` path
      over a role with NO bound card is an unresolvable ref, not a
      selection. The boundary, precisely: counts over anything read 0,
      aggregates 0, world existentials false (these are branch selection,
      not failures), a missing aspect on a bound card reads false, a missing
      NUMERIC attr entry on a bound card compares as 0 (the §9.31 SET-delta
      rule: `attr:x >= 3` fails and `attr:x < 3` holds from missing; a
      present non-numeric entry still never compares), and a
      card-reading operand over an empty role rejects. A role whose cards
      all died mid-list reads as absent — silent false (the §6.8 dead-stamp
      case): the card existed at the clause's re-resolution, so nothing
      failed to resolve; it died since. Quantifiers over an empty role keep
      their §6.3 truth in BOTH disciplines — `every` vacuously true, `any`
      false — so a quantifier over an optional role wants the same guard.
      Authors who branch on an OPTIONAL
      role guard it first (`IF count(object) >= 1 AND object.aspect.x …`).
      F4 slots its unbound-`roll` rejection into the same channel.
    - **Conditions parse from the sentence token stream, one grammar.**
      `when:` predicates arrive as strings and self-tokenize; IF conditions
      arrive inside the outcome list, so they are consumed as a balanced
      unit up to the depth-0 `->` (predicates carry commas inside
      `rel()`/`every()` — naive comma-splits are wrong; a depth-0 `,` or
      bare `ELSE` where the arrow belongs is a parse error). Both front-ends
      share the same grammar functions — the Tok stream is re-serialized
      into the predicate parser's own token alphabet (word-adjacent lexemes
      space-separated, everything else glued exactly as the string tokenizer
      would see it: `aspect:hunter>=1`, `state:!sealed`, dotted paths), so
      the `exists()`/`count()` inner-query reconstruction and every rule
      behave identically. This exposed a latent dead branch: the
      split-token shape of path operands (`object` `.` `aspect` `.` `duty`
      as separate tokens) now really parses.
    - **The load-time guards recurse.** The trigger PROGRESS/REFUSE guards
      (§9.34) and the ENDING existence check (§9.35) were shallow top-level
      scans; nested IF branches would have hidden PROGRESS, REFUSE, or an
      unknown ending from all three. They now walk branch bodies recursively
      through one shared entry point — the walk F3's function expansion
      feeds (expansion happens BEFORE the guards, so expanded bodies are
      checked like literal ones).
    - **Role operands in trigger IF conditions are a load error** (the
      §9.34 philosophy extended to conditions): slot/expiry event bindings
      are empty, so `IF count(actors) >= 1` or `IF object.aspect.duty >= 1`
      in a trigger would silently read 0/false and never fire. The error
      names the operand; the world forms (`exists`, `exists(<query>)`,
      `count(<query>)`) are the trigger vocabulary. Recipes keep full role
      access — their steps carry real bindings.
    - **ELSE binds greedy.** The chain is consumed inside the IF node (the
      `,`-loop of the outcome list must not strand an ELSE), so a nested
      ELSE belongs to the nearest IF and an ELSE after an IF's closed body
      belongs to the outer one — the classic dangling-else resolution, now
      grammatical. `IF`/`ELSE` join the reserved words (§5.1); no existing
      content collided.

### Implementation rulings (v0.3.3, functions)

40. **Functions are compile-time macros; the engine never sees a call**
    (§4.4, §10). The `functions:` section declares `NAME(params…) ->
    OUTCOMES`; every outcome position may call `NAME(arg, …)`. Content
    compile builds the function table FIRST — before any verb or trigger
    body parses — and threads it explicitly into the outcome parser (never
    Dictionary state): known function names win before op parsing at the
    top of `outcome()`, via a name-then-`(` lookahead. Because every call
    expands to its body during that same load pass, the engine, the log,
    and every downstream consumer see plain outcomes only — a call site IS
    its expanded body, byte for byte (**canonical equals expansion**). This
    deviation from "the log never contains sugar" is safe by inspection:
    script-sourced outcome strings are never formatted (§0 law 2 rounds
    trip engine-written rows only), and the trigger guards run AFTER
    expansion, so a body's PROGRESS/REFUSE or an undeclared ENDING id is a
    load error exactly as if authored at the call site. Ordering:
    parse → expand → guard.
    - **Calls parse only in outcome position.** A function may share its
      name with an entity or a verb without ambiguity — clause positions,
      ref positions inside another outcome's args, and lenient input never
      dispatch calls. Only an op-named function (the `dict.isOp` check —
      functions and ops share the caller's vocabulary) and an IF/ELSE-named
      one (undispatchable: conditionals intercept first) are load errors.
      An UNKNOWN `NAME(` in outcome position is a compile error listing the
      whole vocabulary — ops, then declared functions — with a forgotten-IF
      hint when the name is a predicate helper (`exists(...) -> (…)` without
      `IF` used to fail as a bare ref with a trailing `(`; it now says so).
    - **Two substitution mechanisms** (a call site's parameters must reach
      both the AST and the prose):
      - *AST-level*: a parameter may stand anywhere a ref or value literal
        is expected — whole-ref positions (`SPAWN X`, `DESTROY X`),
        role-ref positions, generic arg positions (int, delta sign — `+n`
        carries its authored sign into the fold —, string, bool, dur, key,
        INTO, pair key or value), NARRATE's string slot, and IF-condition
        cmp operands. The body parses ONCE into a template holding param
        placeholders; expansion splices the call-site arg into each
        placeholder, in position. A kind mismatch the AST can represent (a
        string bound where a ref belongs) splices through and surfaces as
        the usual runtime step rejection — same-as-authored semantics; one
        it cannot represent (a non-ref/role in an INTO slot, a non-number
        in a condition) is a load error naming the parameter. Parameters do
        NOT reach `kind:`-qualified names (a load error — bind the whole
        ref), query terms or scalar conditions inside predicates (the
        grammar rejects them), or `when:` lines (functions are
        outcome-position only).
      - *String interpolation*: `{param}` inside a quoted string in a
        function body is rewritten AT EXPANSION to the call-site arg's
        canonical spelling — `sus(THE_GUARD)` renders "…linger on
        THE_GUARD."; ints/durations/signs spell as they author. The
        reserved tokens `{actors}` `{object}` `{with}` `{at}` `{about}`
        `{to}` and `{roll}` (§5.3 prose + F4's die) are NEVER
        param-substituted and pass through untouched. An UNDECLARED
        `{token}` in a body string is a **load-time error** — the chosen
        behavior over a warning, deliberately: this compiler has no warning
        channel (compileContent throws or succeeds), and the alternative —
        silent pass-through — bakes a typo'd parameter name into every
        call's player-facing journal prose forever. The error names the
        token, the function, and the reserved set.
    - **Recursion, depth, arity — all load errors.** A recursion cycle is
      rejected by a colored DFS over the static call graph extracted from
      the templates (branch bodies included), and the error NAMES the cycle
      (`functions recurse: a -> b -> a`) — even for never-called functions,
      since expansion would not terminate. The same graph's depth (1 + the
      deepest callee chain) is capped at 8 for legitimate chains; a chain
      of 9 is a load error. Arity is checked at every call site's parse,
      nested calls included.
    - **Parameters shadow.** Inside a body, a bare token matching a
      parameter resolves to the parameter — before entity names and before
      role names — so `f(GUARD) -> DESTROY GUARD` destroys the BOUND card,
      never an entity that happens to be named GUARD. The same rule makes a
      param named like a role (`object`) take over that position inside the
      body; quote or rename to reach the name underneath.
    - **Role refs in bodies resolve per call site** — fine in recipes,
      where steps bind real cards; in slot/expiry triggers the event
      bindings are empty, so a trigger-called body's role-dependent
      outcome behaves exactly like the same role ref authored in the
      trigger: ops that need their target reject the step at runtime
      (`MOVE ACTORS INTO OBJECT` → "unresolvable destination", §9.19),
      iteration-style ops (`DESTROY OBJECT`, SET by role) apply to nothing.
      World-form conditions (`exists`, `count(name:…)`) and literal
      outcomes are the trigger vocabulary, unchanged by expansion.
    - **Bodies are ordinary outcome lists otherwise**: nesting (a body
      calling functions) expands recursively — forward references included,
      since all bodies parse before any call site expands — and IF/ELSE in
      a body works end to end, its conditions carrying parameter operands.
      ROLL and DRAW are legal in bodies by the same generality: the
      expander is op-agnostic, so v0.4's dice and decks slot in without
      grammar changes.

### Implementation rulings (v0.3.4, seeded dice)

41. **The dice are a seeded counter, and the counter is state** (§4.4, §9.30).
    The engine's ONLY randomness (law 5) is `State.rng` — an integer counter
    of how many die values the whole history has consumed — sampled by a pure
    function of two integers: die `i` of a roll at counter `c` is

        1 + floor( sides * splitmix64(seed XOR (c+i)) / 2^64 )

    with `seed` the script's `seed:` meta line (§10, default 0, stored in
    compiled content) and splitmix64's constants and shifts computed on
    BigInt END-TO-END (the ES2022 target has them; a float64 cannot hold
    `sides * u64` and would quantize the low bits away — the test suite pins
    a python-computed vector table plus a 2^53−1 seed to keep it that way).
    A roll of m dice consumes exactly m counters (`c .. c+m-1`). Consequences:
    - **Concretization**: ROLL logs `ROLLED(<total>, <ctrAfter>)` — the total
      AND the counter after. Play consumes the counters on the DRAFT; the
      fold's ROLLED apply sets `state.rng = ctrAfter` and derives nothing
      else (law 3: replay never re-rolls). ROLLED needs real parse+format
      entries (two plain ints; the generic paren'd path formats and re-parses
      it), because a fallback that emitted anything unparseable would break
      round-trip for every recorded roll.
    - **Undo/replay**: since the counter is state reconstructed by fold, undo
      past a roll lands the counter exactly where the folded history left it
      (roll, undo, act differently, save — `fold(loadLog(log)) === played
      state`, dice included). No dice bookkeeping outside the log exists.
    - **Refused rolls**: a REFUSE after a ROLL discards the draft — the real
      counter never moved, and the fold skips the refused record entirely, so
      it never moves there either (play/fold agreement is asserted). The
      refused attempt's journal prose may still show its `{roll}` number, and
      a re-attempt throws the SAME dice (same counters) — deterministic,
      documented, accepted.
    - **Seed drift (§9.30 asymmetry)**: changing `seed` shifts every FUTURE
      die but never past rows — the fold replays recorded ROLLED totals and
      counters verbatim, whatever the content's seed now says.
    - **Authoring ROLLED is a load error** (the paren-form-error precedent):
      the parse entry exists so concrete rows re-parse; the compile lint is
      what keeps scripts from forging dice history.
    - **Validation**: `sides` < 2 or `count` < 0 reject the whole step
      (§9.19 — nothing logged, clock unmoved); `count` = 0 is legal (total =
      bonus, counter unmoved). The same strict discipline as IF conditions
      applies to the operands themselves: a count/sides/bonus expression that
      cannot resolve (a missing aspect over its bound role) rejects the step
      rather than silently rolling nothing.
    - **Triggers may roll**: slot and expiry outcome lists may ROLL and tier
      on `roll`; the EVENT record carries the concrete outcomes incl. the
      ROLLED row and replays exactly. Trigger ROLL arguments must be
      ROLE-FREE (the §9.39 trigger lint extended to the dice expressions —
      `ROLL(count(actors), 6)` is a load error naming the argument), while
      world forms (`count(name:X)`) are the trigger vocabulary.
42. **The `roll` binding lives on the apply context, one slot per clause**
    (§6.3, §5.3). The last ROLL of an outcome list binds `roll` (last-wins;
    a second ROLL rebinds); the slot is reset at each `;` clause of a
    multi-clause step, survives function expansion (expansion is compile-time
    — an expanded body reads its call site's list), and is NOT state: a
    REFUSE discards it with the draft. It is readable in:
    - **IF conditions** — the `{t:'roll'}` ValExpr, parsed only by the Tok
      front-end. A `when:` line using `roll` is a LOAD error (selection
      provably runs before any outcome could have rolled); the runtime
      rejects a TAKEN branch reading an unbound `roll` (§9.19), which is the
      enforcement backstop for the lint's false negatives.
    - **NARRATE strings** — `{roll}` is rewritten AT EXECUTION, so the
      concrete row carries the number (the journal never re-derives dice).
      The rewrite applies to NARRATE args after the list's ROLL; a NARRATE
      before it keeps the literal token. `{roll}` in PROGRESS or REFUSE
      strings is a LOAD error — that prose is stored raw and rendered
      without an execution pass, so the token would bake in as literal text.
      Function bodies treat `{roll}` as reserved (never param-substituted,
      §9.40) precisely so it survives expansion to here.
    - **Compile-time may-bind lint (best-effort)**: a `roll` reference with
      no ROLL anywhere before it on any path of its list is a load error
      (every play would reject); a ROLL hidden inside an earlier IF's
      branches makes later references lint-clean (no false positives — that
      branch might be the taken one). False NEGATIVES are allowed and the
      runtime rejection covers them.
    - `ROLL` and `ROLLED` join the reserved words (§5.1), and a function
      named ROLL collides with the op (§9.40's isOp rule) as usual.

### Implementation rulings (v0.3.5, decks)

43. **A deck is a seed-fixed permutation over POSITIONS of its spec**
    (§4.4, §9.30, §10). The `decks:` section declares `ID spec NAME, …
    [defaultcard NAME] [reset]`; entries are prototype/entity NAMES only
    (no kinds — the mint's kind resolves at load from the prototype or the
    declared entity line), resolved once at load by a check modeled on the
    §9.35 endings check (load error naming the deck and the unknown entry;
    `defaultcard` resolves the same way; an empty spec is a load error).
    Repeats are legal and weight the draw.
    - **The permutation, not the spec order, is the draw sequence**: the
      fixed order is a Fisher–Yates shuffle over the POSITIONS `0..n-1` of
      the spec, where position `i` draws its shuffle value from
      `splitmix64(seed XOR hash(deckId) XOR i)` — the §9.41 mixer with an
      FNV-1a-64 hash of the deck id standing in for the dice counter. Pure
      in (seed, deck id): fixed at load, identical at play and fold, and
      consuming NO rng counter (law 5 — deck streams and the dice stream are
      independent key spaces of the same mixer, so drawing never shifts a
      die). Shuffling POSITIONS (not names) preserves the multiset: `A, A,
      B` keeps its 2:1 weighting in every period. The permutation lives on
      compiled content, never on State; `State.decks` holds one cursor per
      deck, cloned like every state field (law 6).
    - **Serving and exhaustion**: a draw serves `order[cursor]` and advances
      the cursor ON THE DRAFT (a rejection or REFUSE discards it — law 4).
      Past the period, `reset` wraps (the sequence is periodic with period
      `len(spec)` — there are no mid-run reshuffles); without `reset`,
      `defaultcard` serves forever; with neither, the draw FAILS — a §9.19
      step rejection (nothing logged, cursor unmoved). Play and fold share
      one serving function, so the two can never drift.
    - **Concretization**: DRAW logs `DRAWN(<DECK>, <name>)` followed by the
      standard concrete `SPAWN … [INTO #c] AS #id [aspects=…] [attrs=…]` —
      the SPAWN half owns every mint rule (prototype aspects and `derive`,
      §9.38 stackability, §9.18 materialization), so deck cards are ordinary
      entities once drawn and a stackable deck entry pools naturally. The
      DRAWN row carries the SPEC entry name (not a derived mint name) so its
      assertion reads the permutation. DRAWN needs real parse+format entries
      (two name refs — an unsafe spelling formats re-quoted and re-parses as
      ref's quoted-string form, via the generic paren'd path)
      because a fallback that emitted anything unparseable would break
      round-trip for every recorded draw. Authoring DRAWN is a load error,
      exactly like ROLLED; `DRAW` and `DRAWN` join the reserved words (§5.1),
      and a function named DRAW or DRAWN collides with the op (§9.40).
    - **Seed drift (§9.30 asymmetry, with a deck-specific edge)**: changing
      `seed` shifts every FUTURE draw (and every die) but never rewrites
      past rows. The dice replay past rolls from the log alone; a deck's
      cursor row must RE-DERIVE its expectation (cursor + permutation), so a
      seed or spec change under an old log makes the DRAWN assertion FAIL
      LOUD instead of silently diverging — re-fold such logs against the
      content that played them, or accept the error as the drift detector
      it is.
    - **DRAW's INTO must resolve** (stricter than SPAWN's lenient default):
      a role with no bound card — every event binding, for one — or an
      unresolvable ref rejects the step rather than materializing the card
      nowhere. **Triggers have EMPTY bindings** (§9.39's rule): a no-INTO
      DRAW in a trigger mints containerless (the documented default — the
      same shape as a trigger SPAWN), so a trigger that wants a drawn card
      to LAND authors an explicit `INTO <place-or-character>`. The deck id
      itself IS load-checked (a `DRAW 'NOSUCH'` cannot even load), in
      recipes and triggers, IF branches included (§9.35's walk); INTO
      targets stay runtime-resolved like every ref position — expansion
      before guards means a function body's param-INTO binds per call site.
    - **Legal homes**: DRAW is legal in recipes, triggers, IF branches
      (an untaken branch consumes no draw — the cursor rides the draft),
      and function bodies (`deckeffects`-style multi-draw is just multiple
      DRAWs in v0.4 — no sugar). A `when:`-selected recipe that never runs
      never draws.
44. **The livelock rule — deck entries should be stackable, or be consumed
    or renamed on the same step** (§9.38 x §9.42). A deck entry minted
    NON-stackable whose previous copy still lives makes the SPAWN half of
    its next serve reject (`SPAWN: name 'X' already in use`); the rejection
    discards the draft, the cursor never advances, and every subsequent
    serve of that entry fails IDENTICALLY — the deck is pinned shut with no
    error beyond the per-step rejection. That is the documented authoring
    rule, not engine machinery: declare pool entries `stackable` (gossip,
    dread, funds), or have the same step consume/rename the previous copy,
    and the deck cycles forever as intended. A test pins the stuck state
    (the message, the frozen cursor, the unchanged snap) so a future engine
    change cannot silently "fix" it by skipping or auto-consuming.
45. **Duration literals may combine units** (§5.1, §4.2). A duration is one
    or more `int (halfhour|h|m|d)` pairs summing to minutes (`1h30m` = 90,
    `1d2h30m` = 1590). Bare literals lex as ONE token in outcome args; the
    quoted-string `expires` path (§9.36) and the script `duration:` entries
    run through the same single parser — every path accepts compounds.
    Canonical rendering still prefers a single exact unit (d, then h, then
    halfhour, then raw minutes — `90m` renders `3halfhour`, never `1h30m`)
    and decomposes into compound form only when no single unit fits
    (`100m` → `1h40m`, `1441m` → `1d1m`), so every canonical form re-parses
    (the §5.2 round-trip). Output is byte-identical wherever a single exact
    unit fits; minutes that fit no unit (`61m`, `100m`) now render compound
    instead of raw — the one visible rendering change.

### Implementation rulings (v0.3.6, the Cultist Simulator five)

These five rulings close the dossier's "needs authoring translation" list
(`linked` chains, xtriggers, induces, any-of slots, `maxexecutions`).
Numbering starts at 47: the implementation already cites a §9.46 (expires
anchoring / WAIT-row span) that this document has not yet backfilled — 46
stays reserved for it.

47. **`RUN` — a completion may chain into named follow-up recipes**
    (§4.4, §6.4, §6.7). New outcome op:

        RUN "recipe-id" [, "recipe-id" …]

    - **Arg form: quoted-string refs.** Recipe ids are lowercase and dotted
      (`study.recipe0`; minted ids are `<verb>.recipe<i>`) and the lexer's
      NAME class has no dot, so a bare id is unlexable — the args are
      quoted-string refs (the `DRAWN` precedent). `RUN` joins the reserved
      words (§5.1; the ROLL/DRAW precedent for authored ops). The paren-less
      list's commas are owned one candidate deep: a comma continues the list
      only when a quoted string follows, so `RUN "a", NARRATE "x"` composes
      in an outcome list; when the next outcome itself opens with a string,
      parenthesize out (`RUN("a"), SPAWN NAME`).
    - **Semantics: an ordered candidate walk.** When the carrying window's
      completion applies its outcomes, RUN walks its candidates IN ORDER and
      pins the FIRST whose `when:` matches the post-outcome draft —
      evaluated LENIENT, `strict: false`, exactly like selection (§6.4) —
      and whose `max` budget (§9.50) is not spent; it then opens ONE
      follow-up window on that recipe's verb with the parent's committed
      role refs PROJECTED onto that verb's declared slots: a role the
      candidate verb does not declare is dropped, so the synthesized clause
      keeps the same shape a player step could produce (a player cannot
      bind an undeclared role, and actors survive wherever the candidate
      declares actors) — a card bound only in a dropped role is carried by
      no window and so never locks — beginning at the completion clock.
      The projection happens at synthesis only: the parent window's own
      refs and locks are untouched, the follow-up's BEGIN row remains the
      record, and the fold replays it through the one registration path
      unchanged. The `when:` IS
      re-evaluated: CS's linked hand-off is requirement-disambiguated (81
      all-chance-100 multi-entry routers; dream_router's six; the victory
      forks), so a pinned-id-no-check rule would make them inexpressible.
      The candidate `when:` evaluates against the post-outcome draft AND
      the parent window's committed role bindings, MERGED with the
      projected clause's own — where the candidate verb declares the role
      the projected binding wins (it is what the follow-up will carry);
      where the projection drops the role the parent binding stands, so a
      cross-verb gate reads the parent situation (the CS linked semantics).
      Same-verb chains keep their ordinary matches — the projection is the
      identity over the declared roles — but the declared-role bindings
      CHANGE SOURCE: they now come from the projected clause re-resolved
      against the post-outcome draft, so a same-verb gate whose bound card
      the parent's own outcomes replaced or killed reads the follow-up's
      truth, not the begin-time id snapshot. A role the projection drops is
      GATE-VISIBLE ONLY: the merged bindings let the candidate `when:` read
      it, but the follow-up never carries it — an outcome SET over that
      unbound role expands to no targets and silently no-ops (the §9.31
      role-ref law), and the dropped cards lock nothing. The merge is
      eval-context only: the BEGIN row still carries
      the projected clause alone.
      No candidate matches: nothing opens — the chain ends, silently (that
      is CS's chain ending, not a refusal). Under content drift an
      unresolvable candidate silently continues the walk (the §9.30
      posture) where the window's own commit-time recipe lookup fails
      loud.
    - **Chance is authored around it** — `ROLL` + `IF` in the parent
      (`IF roll <= 90 -> (RUN "…success") ELSE -> (RUN "…failure")`,
      §9.41/§9.39). Candidates with IDENTICAL requirements are this form,
      not the list form: the list walk would always pin the first, so pure
      chance forks (210 of CS's 335 multi-entry parents) author one RUN
      per IF arm — one arm evaluates, one RUN applies. A second RUN OP
      reached in one completion rejects the window (a station holds one
      window, §6.7); the list is the only multi-candidate shape, and
      deeper chains author RUN inside the follow-up's own outcomes.
    - **The guard survives.** The pinned recipe's top-level `REFUSE` still
      gates, inline against the draft at the parent's completion — a
      Phase-3-shaped trial (PROGRESS skipped, scratch rng/id consumption
      discarded, §6.6) whose refusal rejects the PARENT window (§6.7's
      window-refuse: the COMPLETE row carries the marker, partial effects
      discarded).
    - **Concretization: zero-concrete op.** RUN is the first op that logs
      NOTHING of itself — the follow-up's BEGIN row is the record. The
      fire result grows an extra-records slot; the follow-up window is
      inserted after the parent window leaves the open table, is
      registered through the ONE registration path play and fold share
      (so the insert and the fold's BEGIN replay cannot drift), and the
      pass loop picks the new due window up on its next iteration (the
      `advance_extra` precedent). Content that drops a RUN stops
      producing follow-up BEGINs in future play; old logs replay
      untouched.
    - **Livelock guard.** Cycles are legal (needs→discardseason→drawseason
      →needs is one and must stay legal), but a cycle of zero-duration
      follow-ups generates unbounded due work inside one pass: an advance
      pass that completes more than 1024 windows fails loud — a
      documented guard with a pinned test, the §9.44 posture.
    - **Load checks.** In a recipe body every candidate must name a recipe
      of the PARENT verb (unknown or cross-verb id: load error, IF
      branches included — the §9.35 walk). In an `on complete` body
      (§9.48) candidates may cross verbs, which requires GLOBAL recipe-id
      uniqueness: a cross-verb resolution consults a global id table, and
      a collision across verbs is a load error.
    - **Out of slice (reserved)**: CS `linked`-entry `challenges` (21
      entries, the Exile `long.*` rolls — authorable today as ROLL + IF
      with the dossier's convention tables) and the 8 inline recipe
      definitions (compile to ordinary declared recipes);
      `additional`/`expulsion` never occur in core linked entries
      (0/1473).

48. **`on complete` — completion triggers on a card; recipe `aspects`**
    (§6.7, §10; covers CS xtriggers + induces). New entity/prototype
    trigger line — ID-less, a new home beside the section-scoped,
    always-ID'd `triggers:` forms:

        on complete [<aspect>] -> ( outcomes )

    and the recipe-line token it gates on (positional, beside
    `[id] [hidden|shown] [hint] [when] ->`):

        aspects <name> [, <name>]*          # presence list, level 1

    The recipe-line id detector's keyword exclusion grows accordingly:
    a leading token is an id only when it matches the id class and is not
    one of `when`, `hidden`, `shown`, `max`, `aspects` (§9.50 for `max`).

    - **Semantics.** When a window completes, AFTER the recipe's own
      outcomes and BEFORE any RUN: each card bound in the window's
      committed locked roles — the same set §6.7 makes busy, the
      player-dropped station card included; deduped (a card bound through
      two roles fires once); fired in the lock list's order; actors stay
      free — fires its `on complete` triggers. The bare form fires on
      every completion binding the card; the aspect form fires iff the
      completed recipe declares that aspect. Outcomes apply on the draft;
      a downstream rejection discards the whole completion (law 4/§9.19).
    - **Deliberate deviations from the other trigger kinds.** Completion
      triggers produce NO EVENT row and never touch `state.fired`: their
      outcomes are logged as concrete rows appended to the COMPLETE row's
      outcome list in fire order (the fold replays them without
      recomputing gates; play and fold share the firing function — the
      §9.42 one-serving-function rule). Their bodies see the WINDOW's
      re-resolved bindings — the empty-bindings rule of slot/expire
      triggers (§9.42's DRAW-INTO rule) does NOT apply, and the role-free
      load lint does not run on them. A failure inside the body refuses
      the WINDOW, not the event triggers' whole-step throw. `REFUSE` in
      the body is a load error (§9.34).
    - **RUN inside the body is legal and may cross verbs** (§9.47's global
      id table). The busy check for such a follow-up EXCLUDES the parent's
      own window — otherwise a same-verb hand-off would see its own
      station busy and silently skip forever (an invisible content bug;
      the ordering hazard is specified away, not left to implementation
      order). Any OTHER busy station skips the candidate silently. This
      is what makes CS's `induces job→needs` expressible: a work
      completion containing the job card hands off to the time verb — a
      one-shot bootstrap, after which the needs cycle (60 s, 100 % links)
      keeps the time station resident and later induces correctly
      suppress.
    - **The gate's emitter is authored.** An aspect with no declaring
      recipe never fires. CS's recipe `aspects` cover 71 of its 95 xtrigger
      events; for the rest the consuming recipe declares what it emits.
      Where ONE recipe consumes many carriers (CS `translate`, requirements
      `{text: 1}`, serving ~95 texts), a declaration would cross-fire —
      the converter SPLITS it into per-carrier `when:`-selected variants,
      each declaring its language (the 8 `scholar*` events). Six Exile
      corpse carriers (`remnant`/alzabo, bound nowhere) are out of slice
      with the rest of `long.*`. The CS value duality needs no mechanism:
      1517 of 1519 xtrigger values are ELEMENT ids — the trigger lives on
      the carrier itself (`REPLACE` self) — and the 2 recipe ids are RUN
      targets.
    - **`aspects` is content, not state** — it never enters the log;
      content drift changes which triggers fire in FUTURE play only
      (§9.30 posture). `ROLL` inside consumes counters normally (§9.41);
      `ENDING` inside ends the story (§9.35, first wins).

49. **Alternation in the query mini-language** (§4.5). CS verb slots are
    ANY-OF maps (`work`: ability OR job OR ritual OR desire OR benefactor
    at level ≥ 1). The query grammar gains top-level alternation over
    conjunction groups:

        query := and ( '|' and )*        and := term+        term := '-'? atom

    - **Terms are unchanged** — every existing atom form composes inside a
      group: levels (`aspect:lore>=6`; CS element slots reach 6, recipe
      requirements 8), sub-aspect keys, attr state, exact-case names, and
      per-term negation (`-aspect:lunatic` is CS's negative requirement).
      `|` binds loosest; a leading/trailing `|` or an empty group is a
      parse error.
    - **Legal homes**: every §4.5 consumer — slot `query:` strings and the
      `exists()` / `count()` inner queries (§9.38) — one parser, one
      behavior. There is deliberately NO canonical printer: queries are
      authored strings, never serialized and never logged, so the §5.2
      round-trip does not apply to them.
    - **Selection stays per-card**: the DNF evaluates against ONE card
      (the UI's drop-grey-out is unchanged); group logic remains a recipe
      concern (§4.5's deliberate split). CS `forbidden` maps need no
      alternation — all 16 multi-key maps are conjunctions of negations,
      already expressible.

50. **`max N` — recipe execution limits** (§6.4). CS's `maxexecutions`
    (26 recipes: 25 × max 1, 1 × max 3). New positional recipe-line token:

        max N                              # N ≥ 1, integer

    - **Semantics.** At selection (§6.4's walk) a recipe whose execution
      count is ≥ N is skipped — the walk continues; `max 1` is the recipe
      analog of a once-trigger. The same gate applies to RUN's candidate
      walk (§9.47): an exhausted candidate is simply non-matching.
    - **The count.** `State.executed: IndexMap<recipe-id, i64>`, cloned
      like every state field (law 6), maintained by the ONE
      window-registration path play and fold share: a registered window
      increments each clause's commit-time recipe id on the draft. A
      table-refused step opens no window and counts nothing; a window
      that later refuses still counted (it registered at begin). The fold
      rebuilds the map through the same registration path from BEGIN
      rows — selection reads only the state map, never the log (the
      `fired` precedent, §9.29).
    - **Interaction.** `hintonly`/`shown` recipes respect `max` the same
      way (a spent recipe stops being suggested). A max change under an
      old log changes FUTURE selection only (§9.30 posture).

*Closing note (not a ruling).* The five land as: one outcome op (`RUN`),
one trigger line + one recipe token (`on complete` / `aspects`), one
grammar extension (`|`), one recipe token + one State map (`max`). The
background-tick question (dossier §6) is answered for the slice WITHOUT a
pump: the `job→needs` induce bootstraps the time loop, and the loop — once
resident, self-rechaining at 100 % — suppresses later bootstraps exactly as
CS's does. A true unattended pump remains future work.


---

## Appendix — version notes

The spec's version-by-version changelog, carried in the spec header through v0.6, moved here verbatim (a heading per version; wording unchanged).

### v0.2

> v0.2 = v0.1 + implementation rulings (§9.10+). Changes: grammar formalized
> and reconciled with examples (pre-verbal `AT`, args/arity grammar, reserved
> words), USE_ON signature fixed, single time model (auto duration + ADVANCE
> as authored extra), step-atomic REFUSE, recipe ids, EVENT representation,
> aspects/attrs split, name-uniqueness rules.

### v0.2.1

> v0.2.1 = spec–implementation alignment review. Changes: reserved word `AS`,
> name lexeme allows leading `_`, canonical paren-less outcome forms (`SPAWN`,
> `NARRATE`), role refs as outcome args, concrete SPAWN materialization pairs,
> prototype `derive` (replaces the `CLUE(secret-2)` sugar), recipe auto-ids,
> monotonic clock, fold clock ownership, `advancedMin` on the record, slot
> `shown`/`hidden` data-model-only, script format (§10) and play interface
> (§11) documented.

### v0.2.2

> v0.2.2 = the processing cycle. Changes: `PROGRESS` outcome (authored
> read-bar ticks, §6.7), the processing cycle formalized (commit → lock →
> ticks → outcomes → unlock; locks guaranteed by serialized discrete time),
> prose renders progress and gains `{to}`/`{with}` tokens (§5.3), the `to`
> role reaches the record (§7), distinct cards per slot, `once` triggers
> (§9.29), hour-quantized step clocks with dead-air event crossings (§9.28),
> first story content exercising it (`scripts/the-nightjar-flyer.txt`), and
> an exploratory story test (test/nightjar-explorer.test.ts).

### v0.2.3

> v0.2.3 = the halfhour clock. The clock tick shrinks from the hour to the
> **halfhour** (30 minutes): `day:halfhour` with halfhour 0..47, the
> `halfhour` duration unit (§5.1), triggers fire per halfhour crossing
> (`at hour N` remains as an alias for slot 2N, §10), and quantization/
> dead-air semantics re-anchor on the halfhour tick (§9.28).

### v0.2.4

> v0.2.4 = the card mapping (§8.1). The operation-level correspondence between
> text-engine operations and card operations, written down: entity → card,
> slots → roles, dropping → role binding, commit → the canonical sentence,
> the processing window → the locked station, outcomes → the cards' fates.
> Documentation of existing semantics — no grammar, log, script, or API
> change.

### v0.2.5

> v0.2.5 = SET arithmetic (§9.31). The SET outcome gains signed delta values
> (`+1`, `-2`) and `aspect:` keys: `SET(x, aspect:level, +1)` adds to the
> entry's current value and the engine logs the computed absolute form, so
> the fold replays literals and never redoes arithmetic. Dotted arg keys
> (`state.sealed`) now lex as written in §5.1.

### v0.2.6

> v0.2.6 = authorable decay (§9.32–§9.34). Triggers gain a second form —
> `ID on expire of NAME -> OUTCOMES` — so an expiry can REPLACE the card or
> apply arbitrary outcomes instead of the built-in destroy; the REPLACE
> outcome (`REPLACE ref WITH prototype`, paren-less) mints the successor into
> the dying card's current location (container or zone, §10) and concretizes
> to plain SPAWN+DESTROY; REFUSE
> becomes a load-time error in all trigger outcomes; play and fold now agree
> exactly on `state.fired` for every EVENT row.

### v0.2.7

> v0.2.7 = endings (§4.4, §9.35, §10). The `ENDING` outcome (paren-less,
> `ENDING ID`) ends the story: the `endings:` script section declares the
> table (UPPER_SNAKE id, quoted title, optional flavour — default
> `melancholy`), the first ENDING to apply in a step wins and the step itself
> completes normally, steps on an ended state are rejected with nothing
> logged, time triggers and authored expiries may ENDING (the CS
> despair-death shape), and undo reopens the game because `state.ended`
> derives from the fold like everything else.

### v0.3

> v0.3 = the additive roll-up since v0.2.4, collected under one number because
> every change below it is additive. Since v0.2.4: SET signed deltas and
> `aspect:` keys (v0.2.5, §9.31), authorable expiry triggers + the REPLACE
> outcome + trigger guards with play/fold `state.fired` agreement (v0.2.6,
> §9.32–§9.34), the ENDING outcome + the `endings:` table + game-over
> (v0.2.7, §9.35), and in v0.3 itself: `SET x, expires, …` accepts every
> §5.1 duration form in the quoted-string path too (§4.2, §9.36), and the
> core verb set grows from ten to twelve — `STUDY` (READ's signature) and
> `MANSUS` (EXPLORE's signature) (§4.3, §9.37).

### v0.3.1

> v0.3.1 = stackable cards (§9.38). An entity or prototype declaration may
> carry `stackable`, so a name may be held by SEVERAL living copies — Dread
> and Funds are pools of identical cards, not unique cards. The engine keeps
> each name as an ordered pool (oldest first, §9.38): every name reference
> resolves to the OLDEST copy, engine-written expiry destroys are keyed by
> `#id` so the expiring copy dies (not the pool head), authored destroys stay
> name-addressed, the slot query language gains a `name:` term (§4.5), and
> `count(...)` accepts any §4.5 query — `count(name:SUSPICION) >= 3` — with a
> load-time lint fencing the `count(DREAD)` role-typo (§6.3). Prototype
> declarations that mint several same-named cards over a run are the primary
> users; §9.33's REPLACE self-collision note is revised for stackables.

### v0.3.2

> v0.3.2 = conditionals (§4.4, §5.1, §6.8, §9.39). Outcome lists gain
> IF / ELSE IF / ELSE branching: `IF cond -> ( outcomes )` with a greedy
> ELSE IF chain and an optional ELSE, branch bodies in parens. Conditions use
> the full §6.3 grammar, parsed from the sentence token stream (§9.39);
> selection runs against the DRAFT, so earlier outcomes in the same list are
> visible to later IFs; the log records ONLY the taken branch's outcomes,
> flattened in position — the record never contains an IF op. Asymmetric
> failure discipline: a condition operand that cannot resolve rejects the
> step (§9.19) where a `when:` predicate would read false. The trigger
> guards and the endings check recurse into branch bodies, and
> role-dependent operands in trigger IF conditions are a load-time error.
> `IF` and `ELSE` join the reserved words (§5.1).

### v0.3.3

> v0.3.3 = functions (§9.40, §10). The `functions:` script section declares
> parametric outcome macros — `NAME(params…) -> OUTCOMES` — and any outcome
> position may call them: `sus(THE_GUARD)`, `watch(3)`. Calls are COMPILE-TIME:
> every call expands to the function's body at load, with two parameter
> substitution mechanisms (AST-level splicing and string interpolation),
> before the trigger guards and the endings check run. The engine never sees
> a call, and therefore neither does the log — canonical equals expansion.
> Recursion cycles and call chains deeper than eight are load errors, as are
> arity mismatches and undeclared `{token}` interpolation.

### v0.3.4

> v0.3.4 = seeded dice (§4.4, §5.1, §9.41, §10). The `ROLL(count, sides[,
> bonus])` outcome is the engine's ONLY randomness: a counter-based pure
> function of the script's `seed:` meta line (default 0) and a state counter,
> sampled with splitmix64 computed BigInt end-to-end (no float64 ever sees a
> die). A roll consumes exactly its dice in counters; the log records the
> engine-written `ROLLED(<total>, <ctrAfter>)` — authored ROLLED is a load
> error — and the fold lands the counter from the row without re-deriving
> anything, so undo reconstructs dice state from the folded log. The list's
> last ROLL binds `roll` for IF conditions and rewrites `{roll}` in NARRATE
> prose at execution (PROGRESS/REFUSE `{roll}` is a load error); `ROLL` and
> `ROLLED` join the reserved words.

### v0.3.5

> v0.3.5 = decks (§4.4, §5.1, §9.42, §10). The `decks:` script section
> declares a deck as an ordered multiset of prototype/entity names —
> `RUMOURS spec RUMOUR, RUMOUR, HEARSAY, SCANDAL defaultcard RUMOUR reset` —
> and the `DRAW <DECK> [INTO target]` outcome serves the deck's next card.
> The draw order is a seed-fixed PERMUTATION OVER POSITIONS of the spec
> (repeats weight the draw), derived at load purely from (seed, deck id) with
> the §9.41 mixer — deterministic, identical at play and fold, and consuming
> NO rng counter. The log records the engine-written `DRAWN(<DECK>, <name>)`
> followed by the concrete SPAWN; the fold advances the deck cursor from the
> DRAWN row, asserting the recorded name against the compiled permutation so
> content drift fails loud. Authored DRAWN is a load error; `DRAW` and
> `DRAWN` join the reserved words.

### v0.4

> v0.4 = the Cultist-complete roll-up. The engine vocabulary the v0.4 plan
> set out is now all here — same-name pools (v0.3.1, §9.38), conditionals
> (v0.3.2, §9.39), functions (v0.3.3, §9.40), seeded dice (v0.3.4, §9.41),
> decks (v0.3.5, §9.42) — and this number belongs to the showcase that plays
> all of it in one bundled story. `scripts/the-nightjar-flyer.txt` now
> gathers a stackable SUSPICION pool through careless searches and the
> conductor's rounds (the story's recurring triggers, alongside the dawn
> watch: his `patrol_check` function rolls and calls the `watch(n)`
> escalation — `count(name:SUSPICION) >= n` — whose ending puts the case
> off the Flyer), rolls its rude searches against nerve pools with
> `{roll}` prose, draws gossip from the RUMOURS deck in a talk recipe and in
> the patrol trigger, and branches a talk recipe on `exists(...)`. The
> story's `seed:` is chosen and documented so the scripted win path
> exercises every mechanic without tripping the ending.

### v0.4.1

> v0.4.1 = compound duration literals (§5.1, §9.45). A duration may stack
> §5.1 units — `1h30m`, `1d2h30m` — summing to minutes; bare literals lex
> as one token in outcome args, and quoted-string `expires` (§9.36) and
> script `duration:` entries share the same parser. Canonical rendering
> keeps the single exact unit when one fits and decomposes (`1h40m`) only
> otherwise — byte-identical wherever an exact unit fits, with minutes that
> fit no unit (`61m` → `1h1m`) the one visible rendering change.

### v0.5

> v0.5 = parallel verb stations (§6.7, `docs/plans/v0.5-parallel-verbs.md`, in git history).
> The processing window becomes real: committing an action **begins** a
> window `[t, t+duration]` on its verb's station and does NOT move the
> clock; several stations may run overlapping windows, and every card bound
> in a committed role is busy until its window closes (actors stay free).
> The recipe is selected at commit (its id rides the BEGIN row); the
> outcomes apply at the window's close, on a **COMPLETE** row anchored to
> the window's start clock (so `expires` replays identically). The clock
> moves only on explicit advances — a bare clock input (`1:12`), an action
> whose typed clock lies ahead, or WAIT/ADVANCE — and each advance is one
> atomic pass that resolves due completions (ordered by end, then begin id)
> before the halfhour's slot events, ending in a WAIT row: the only record
> that moves `now`, whose spans partition time. Top-level REFUSE gates
> still reject at the table; a REFUSE inside an IF branch — and any
> outcome that cannot apply — defers to the completion, where the window
> refuses (the completion analog of §9.19: attempt logged, effects
> discarded, window closed, never a retry loop). Events still ignore
> locks; a completion drops bindings whose cards died mid-window. Old logs
> are not replay contracts (§9.30): BEGIN/COMPLETE/WAIT rows, `startMin`
> and the windows table replace the commit-completes-at-once model.

### v0.6

> v0.6 = the engine-findings fixes (`docs/plans/v0.6-engine-findings.md`, in git history). The
> `SPAWN` outcome gains the authored `expires <duration>` modifier (§4.4,
> §9.36, §9.18): a mint arms its own decay at birth — the SET-expires pair,
> anchored to the mint clock — so an authored outcome can address the card
> it just minted even when the name pools (a name ref would hit the OLDEST
> pool member, §9.38); the modifier is authored-sugar only and rides the
> concrete row's `attrs=` pair. The RUN candidate walk's `when:` now
> evaluates against the parent window's committed bindings merged with the
> projected clause's own, projected winning where the candidate verb
> declares the role (§9.47) — CS's linked targets gated on the situation
> contents are expressible again.

