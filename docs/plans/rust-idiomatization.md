# Plan — Rust-idiomatization: retiring the TS-alignment shape (no behavior change by default)

Status: ACTIVE plan (drafted 2026-10-14). Supersedes nothing; sits after the
v0.4/v0.5 feature plans, which are landed history. Written after a full-tree
read-through; file:line claims verified on the same day and may drift —
re-locate before relying on them.

**The new ruling this plan implements**: the TS implementation is no longer a
behavior reference. The normative baseline is `docs/spec/text-engine-v0.2.md`
(frozen, untouched during the port) and the executable contract is the
existing 711-test suite. Code that exists only to mirror TS shape may now be
re-shaped — but only where nothing observable moves.

References: `README.md` (the frozen Q1–Q17 quirk table), `docs/spec/text-engine-v0.2.md`
(all § numbers below are that file), `src/js.rs` (the compat layer), the
v0.4/v0.5 plans (§0 determinism laws, reused verbatim below).

## 0. Baseline and invariants (read first)

### 0.1 Sources of truth

1. **Spec**: `docs/spec/text-engine-v0.2.md` is the ONLY normative document.
   Where the spec is silent, the 711 tests are the contract. Where both are
   silent, behavior is incidental and free to change.
2. **Tests**: 711 green today (79 unit + 632 integration; verified 2026-10-14).
   `cargo test -j 4` ≈ 45 s wall; clippy `--all-targets` is already zero-warning.
3. **The determinism laws** (v0.4 §0, unchanged): fold(log) byte-equality;
   engine-written rows round-trip; no re-derivation at replay; step/pass
   atomicity; no RNG outside the §9.41 counter; draft discipline (deep clone).
4. **README's no-compat rule** (header): formats and APIs may break between
   commits — so renames/moves are cheap; only *behavior* is expensive.

### 0.2 What the spec pins (A-class anchor list)

Everything below is **normative** — implementation may be renamed/re-homed,
never re-behaved. This is the whitelist that keeps A separate from B/C.

| # | Pinned behavior | Spec anchor |
|---|---|---|
| A1 | splitmix64 dice: `1 + floor(sides * splitmix64(seed ⊕ (c+i)) / 2^64)`, exact 64-bit end-to-end; counter semantics; `ROLLED(total, ctrAfter)` rows; `seed:` meta default 0 | §4.4, §9.41, §10 |
| A2 | Deck draw = seed-fixed Fisher–Yates over SPEC POSITIONS keyed `splitmix64(seed ⊕ FNV-1a-64(deckId) ⊕ i)`; no rng-counter consumption; `DRAWN` cursor assertion | §9.42, §10 |
| A3 | Grammar, canonical output forms, and the round-trip law (`format(parse(row)) === row` for engine-written rows; paren-less SPAWN/NARRATE/ENDING/DRAW; pre-verbal AT; sorted actors; bare-name refs) | §5.1, §5.2, §9.15, §9.14 |
| A4 | Duration rendering: single exact unit first (d, h, halfhour, minutes), compound only when no unit fits (`1halfhour` = 30 min; `61m` → `1h1m`) | §5.1, §9.45 |
| A5 | Log format: JSONL one record/line; field set incl. `tGame`/`advancedMin`/`startMin`/`window`/`recipes`/`sentence`; concrete outcomes fully materialized (ids minted, roles concretized, `aspects=`/`attrs=` pairs, REPLACE → SPAWN+DESTROY pair with `attrs=` always) | §7, §9.15, §9.18, §9.33 |
| A6 | SET deltas logged as absolutes; IF concretizes to the taken branch only; the log never contains IF/FN/call sugar | §9.31, §9.39, §9.40 |
| A7 | Execution model: validate → select → apply → time → events; step-atomic REFUSE (logged, zero time) vs §9.19 rejections (nothing logged); monotonic clock; clause re-resolution against the draft | §6.6, §6.5, §9.19, §9.20 |
| A8 | BEGIN/COMPLETE/WAIT rows; windows table is the lock; completion anchors on the window's start via `beginId`; advance passes ordered by (end minute, begin id, clause), completions before slot events, no cascade, events span dead air | §6.7, §9.28 |
| A9 | Events: authored triggers (declaration order) before expiries; `once` fired-ness as a fold projection; fold trusts EVENT rows; expiry `expire:<NAME>` id; authored-expiry replaces destroy entirely | §6.6, §9.29, §9.32, §9.34 |
| A10 | Names unique among the living; stackable pools address the OLDEST copy in birth order; engine-written destroys address by `#id` | §9.14, §9.38 |
| A11 | Endings: `endings:` table, first-ENDING-wins, post-ending rejection naming the title, undo reopens; ENDING load-time existence check | §9.35, §10 |
| A12 | Functions: compile-time expansion, canonical-equals-expansion, depth 8 cap, recursion-cycle load error, arity check, param shadowing, undeclared `{token}` load error | §9.40, §10 |
| A13 | Load-time checks EXIST (wording is B): trigger PROGRESS/REFUSE guards; role-operands-in-trigger-IF; `count()`/`total()` arg lint; ROLLED/DRAWN authoring errors; deck spec/defaultcard resolution; reserved words | §10, §6.3, §9.34, §9.39, §9.41, §9.42 |
| A14 | Play surface: lenient input resolution rules (case-insensitive, underscores, articles, longest match ≤ 4 tokens, prefix-then-edit-distance-≤2 suggestions); `Game.visible()` projection; undo = drop trailing events + last action, re-fold; `suggestions()` lights non-default non-REFUSE recipes | §11 |
| A15 | Prose shape: verb-template line per clause; `PROGRESS` ticks indented with `(+offset)`; NARRATE unindented; REFUSE its own line; `{roll}` rewritten at execution | §5.3, §9.41(42) |
| A16 | Q7 is spec behavior, not a quirk to fix: a REFUSED step's NARRATE may carry the rehearsed dice; the counter never moved; re-attempt throws the same dice | §9.41 (refused-rolls bullet) |
| A17 | `1halfhour` lexes as ONE duration token (Q9's observable side) | §5.1, §9.45 (the ruling's own words, spec line 1721: "Bare literals lex as ONE token in outcome args") |

### 0.3 What only tests (or nothing) pin — the B/C boundary rule

The spec repeatedly says *what must be named/reported* without pinning *the
bytes*: "naming the valid forms" (§6.3), "names the cycle" (§9.40), "the
message names the ending's title" (§9.35). So **error-message EXISTENCE and
content-requirements are A; exact message WORDING is B** (191
`assert_err_msg` sites pin today's regexes character-for-character). The same
rule sorts every JS-semantics fossil: if the log bytes or accepted input
change, it is B; if only internal shape moves, it is C.

## 1. The classification (core deliverable)

Quantities: **A ≈ 17 items** (§0.2 table), **B ≈ 19 rows** (14 quirk decision
items + 5 non-quirk test-pinned conventions), **C ≈ 12 items** (§1.3).

### 1.1 Class B — test-pinned, spec-silent (the DECISION CHECKLIST)

Default for every row: **keep current behavior**. Ticked rows are fixed in
phase 3, each as its own commit that also edits exactly the listed tests and
updates the README Q-table status column. Nothing else in a B commit.

**Quirk rows** (README appendix, all frozen today):

| Q | What it is | Fix option (if ticked) | Impact / visibility | Verdict |
|---|---|---|---|---|
| Q1 | recipe ids mint 0-based at compile (`.recipe0`) but 1-based in the lint mirror (`.recipe1`) | pick one base for both | recipe ids ride BEGIN/COMPLETE rows (`recipes:` field) and log provenance — unifying breaks byte-identity of every recorded log and the engine/lint id assertions (engine.rs, lint.rs) | keep (cheap, harmless); fixing = log churn for zero player value |
| Q2 | the predicate string tokenizer silently swallows illegal chars (`aspect:x >= @3` ≡ `aspect:x>=3`) | tokenizer errors on non-grammar chars | authoring-facing silent-typo trap; no repo content trips it; lint/query_predicate tests may need new error cases | **RECOMMENDED FIX** — same silent-typo class the spec already rejects elsewhere (§9.40's undeclared `{token}` ruling) |
| Q3 | `arg()` cannot read a param key (`SET(myparam = 3)` parse error) while SPAWN args support one | teach the SET/args path the `k=v` form or reject uniformly | grammar acceptance change; functions.rs / sentence.rs tests; author-facing only | keep |
| Q4 | slot roles are never validated — a typo'd slot compiles and silently never matches | load-time error for a slot key outside the six roles | the worst silent-failure trap in authoring; spec §6.1's grammar says `role` — the fix is spec-ALIGNED; no bundled content has typo'd slots; lint.rs gains cases | **RECOMMENDED FIX** |
| Q5 | duplicate declarations inconsistent: entities/endings error; prototypes/templates/verbs silently overwrite; `ANA`/`ana` case-collide | uniform load-time duplicate error (case-sensitive exact) | author-facing; script.rs tests + a fixture sweep; spec §10 pins uniqueness only among the living, so this is genuinely B | **RECOMMENDED FIX** (same silent-overwrite class as Q4) |
| Q6 | a BEGIN whose format∘parse round-trip fails silently opens no window | loud fold error (or debug_assert + loud) | only reachable with hand-corrupted logs; fold/store tests; fixing makes the fold stricter — safe for valid logs | keep (unreachable in practice), or cheap loud-error tick |
| Q7 | REFUSED step's NARRATE carries rehearsed dice | — | **spec-blessed (A16, §9.41)** — not a decision item; README row stays with status "spec-pinned" | keep (no option) |
| Q8 | JS `\s` ≠ Rust `\s`; every regex spells the explicit JS class | switch to Rust `\s` would change which script bytes parse as whitespace (U+FEFF/NEL) | parse-domain change rippling into every tokenizer test; zero player value | keep |
| Q9 | `DUR_RE` alternation order is load-bearing | — | implementation note, not behavior (A17 owns the behavior) — not a decision item | keep (comment only) |
| Q10 | quoted-string `\X` takes the next char literally (`"\n"` = letter n); trailing backslash still throws | give `\` real escape semantics | changes interpretation of authored journal prose; byte-visible in logs; dice/engine string tests | keep |
| Q11 | bare `inspect` shows the first visible card (`startsWith('')`) | print the card list / usage instead | player-visible, repl/cli tests pin the current card dump | cheap fix if ticked; cosmetic |
| Q12 | ROLES are soft-reserved in outcome-arg position but not reserved words | reserve them (breaking role-named entities in arg position) | grammar change; would break scripts that name entities `at`/`to`; parse tests | keep |
| Q13 | aspects are one level deep; `'a.b.c'` lands as `{a:{'b.c':v}}`; last-write-wins clobber both directions | real nested tree | data-model change, byte-visible only for 3-level keys (no bundled content uses one); types.rs tests pin the flattening | keep |
| Q14 | same-ref comparison errors on a param ref; relation keys embed `\|` (id collision theoretical) | return false / change key encoding | key format is snap-visible (relations compare by key string) — fixing moves test snapshots for a case the engine cannot produce (`#hex` ids only) | keep |
| Q15 | CLI shapes: trailing `--demand` silently vanishes; linter exits 1 on missing directory; unknown meta key blamed on "bad clock" | arg-parse fixes + honest errors | cli.rs/lint.rs pin exit codes and usage text — fix requires their updates; player-facing polish | cheap fix if ticked (below Q2/Q4/Q5 in value) |
| Q16 | a `-0` delta formats as `+0`; `SET(x,-0)` is authorable | canonicalize `-0` → `0` at parse | byte-visible in logs only for `-0` inputs; format.rs tests | keep |
| Q17 | reach `site_reason`'s empty-init-names branch is reachable, not dead code | — | a "not dead code" note, not a defect — not a decision item | keep (no option) |

**Non-quirk B rows** (conventions the tests pin and the spec is silent on;
all default keep — none is a fix candidate, listed for completeness):

| # | What | Why B |
|---|---|---|
| B-18 | Error-message wording everywhere (250 `ChorusError::new` sites; 191 `assert_err_msg` regex pins) | spec pins naming-requirements (§0.3), not wording |
| B-19 | `cmp_utf16` collation for sorts/keys/snap (UTF-16 code-unit order; differs from Rust byte order only for non-BMP names) | spec §5.1 says "sorted" without a collation |
| B-20 | `OrdinaryOwnPropertyKeys` key order + `jsjson` escape set (log byte order for integer-like attrs keys, `\u00xx` lowercase escapes, DEL raw) | spec §7 examples never exercise those keys/chars |
| B-21 | `cmp_ascii_ci` suggestion-pool sort (ASCII-lowercase then byte order; ICU stays out) | spec §11 wants suggestions, not their order |
| B-22 | `js_num_parse`'s f64 middle step above 2^53 (dice seed literals — the suite pins a 2^53−1 seed, §9.41) | pinned by dice vectors; the *saturation* tail (i64 bounds) is C (no log reaches it) |

### 1.2 How to read a B fix's cost

Each ticked row names: the tests whose assertions change (expected count),
whether the delta is player-visible (author-time errors: yes; log bytes: only
Q10/Q13/Q16), and whether the golden corpus (§2, phase 0) digest changes. A B
commit that cannot enumerate all three does not ship.

### 1.3 Class C — zero observable footprint (free Rust-ization)

All items below must keep the 711 tests green **without editing any
assertion**; test-file `use`/path lines may be touched mechanically.

1. **Dissolve `src/js.rs` (662 lines) into honestly-named modules.** The
   module's doc says "JS semantics infrastructure" — under the new baseline
   the JS provenance is history; the *behaviors* remain (B-19/B-20/Q8), the
   umbrella does not. Proposal:
   - `core/text.rs` — the JS whitespace set (`JS_WS_CLASS`, `js_is_ws`,
     `js_char_is_ws`, `js_trim`) + `cmp_utf16` + `cmp_ascii_ci`, re-documented
     as "the engine's frozen collation and whitespace classes" (Q8/B-19 cross-links).
   - `core/jsonw.rs` — `JsJson`, `jsjson`, `jsjson_obj_in_js_order`,
     `is_integer_like_key`, `ordinary_own_property_keys`,
     `clone_map_in_js_order` (B-20's home).
   - `js::ChorusError`/`Result` → `error.rs` at the crate root (see item 2).
   - `Lcg` → `lcg.rs` (see item 6).
   - `static_regex`/`grp_text`/`grp_opt`/`grp0_*` → `core/regex_util.rs`.
   - `js_num_parse` → `core/num_parse.rs` (renamed `parse_i64_like_f64`),
     doc moved with it (B-22).
   Preferred: do the mechanical import sweep in the same commit (README's
   no-compat rule makes shims pointless). Touched-file counts, corrected
   (criterion: `grep -rl 'js::'`, i.e. every file naming the module path):
   **25 src files + 17 test files** (an earlier draft's "~15 src / ~26 test"
   counted export-name imports only and had the split backwards).
2. **Error channel**: `ChorusError { msg: String }` is a deliberate
   anti-drift choice whose rationale ("variants + Display double maintenance")
   was TS-parity. Options:
   - Default: re-home to `error.rs`, keep the plain message struct, refresh
     the doc to argue the case on its own merits (single construction site per
     message; 191 regex pins are the real maintenance surface).
   - Optional (tick): a `thiserror` enum (NEW DEPENDENCY — declared here,
     `thiserror = "2"`, the only dependency change this plan may make) with
     ~8 site-grouped variants whose `Display` emits today's exact strings;
     `assert_err_msg` untouched. Cost: 250 construction sites migrated;
     benefit: match-site ergonomics, `#[from]` for io/serde.
3. **`id_gen` channel → trait.** `ExecOptions { first_record_id,
   id_gen: Option<&mut dyn FnMut() -> String> }` (engine.rs:98) becomes
   `trait IdGen { fn next_id(&mut self) -> String; }` with a blanket impl for
   `FnMut` closures and a `DeterministicIdGen` (the `#<id>_<n>` mint) as the
   default; `ExecOptions: Default` over `first_record_id`. Call sites: engine,
   store tests, game.rs (`id_gen: None` disappears), tests/common/settle.rs.
4. **`ActionRecord.kind: String`** (engine.rs:57) — the open-world replay
   fallthrough (unknown kind → action branch) is deliberate and spec-silent.
   Idiomatic shape that preserves it exactly: `enum RecordKind { Action,
   Event, Complete, Wait, Other(String) }` with serde impls and the fold
   routing `Other` down the action branch, byte-identical round-trip.
   **MANDATORY PRECONDITION, verified call sites**: raw string comparisons
   against `kind` are pinned in the suite — `tests/dice.rs:755/832/1294/1331/1524`
   (`x.kind == "complete"` / `== "event"`), `src/engine/engine.rs:2882`
   (`assert_eq!(r.records[0].kind, "action")`) and `:2889`
   (`x.kind.as_str()`) — so the enum MUST ship with `pub fn as_str(self)
   -> &'static str` plus `PartialEq<str>` / `PartialEq<&str>` (and the
   mirrored `PartialEq<RecordKind> for &str`) impls so every `== "complete"`
   style comparison compiles UNCHANGED. If those compat impls cannot keep all
   such sites compiling without edits, the item degrades to default KEEP
   `String` (re-documented). Default: do it with the impls; zero test edits.
   (Decision inside phase 2, either way zero behavior.)
5. **Dead-shape cleanups**: `Prepared.bindings` + its `#[allow(dead_code)]`
   (engine.rs:114) — delete the field; `slot.max: Option<i64>` (None = the JS
   `Infinity` max) is already idiomatic Rust — keep, only rewrite the comment
   to state the domain fact without the JS origin; `Log.initial_state` (a
   documented test fixture) — keep.
6. **`Lcg`** (js.rs:359): test-side driver RNG for reach-fuzz/scale/explorer.
   Its three seed entries (`new` XOR / `new(0xc5)` / `from_raw_state('NIGH')`)
   and frozen sequences stay BIT-EXACT — the fuzz campaigns and
   fixpoint.rs's seed-referenced findings (`fuzz seed 262`) are keyed to them,
   and re-validating campaigns buys nothing. Re-home to `lcg.rs`, rename
   entries `FuzzRng::seeded/for_scale/with_raw_state` at most. Not a candidate
   for `rand` or `#[cfg(test)]`-gating (bins use it too).
7. **Internal naming modernization**: js_-prefixed names keep their meaning
   but lose the framework reference where the name is misleading
   (`js_trim` → `trim_js_ws`-style honesty is fine; `ordinary_own_property_keys`
   keeps a provenance comment but may become `object_key_order`). Rename
   freely inside phase 1's sweep; behavior untouched by definition.
8. **Documentation debt**: every "divergence" comment that argues from TS
   (store.rs module doc, engine.rs saturation notes at :1172/:1566/:1722/:2628,
   census.rs:480, reach.rs:78, script.rs:10) is reworded to state the
   behavior as a choice of THIS engine, keeping the factual content
   (saturation direction, atomic write, error shape). README's layout section
   and the quirk appendix's "lives at" column follow the js.rs split.
9. **`prelude`** (lib.rs): follow the moves; otherwise untouched.
10. **Module split mirroring TS files**: audited — the current cut
    (core/content/engine/reach/session) is coherent on its own terms; no
    merges. `engine/fold.rs` (52 lines) stays: it is the reviewed seam.
11. **Edition**: stay on 2021. A 2024 bump is mechanical but buys nothing
    here and re-reviews the whole tree; not in scope (listed so it is a
    decision, not an accident).
12. **Dependencies**: the frozen four (indexmap, serde, serde_json, regex)
    stay. The only permitted addition is the optional `thiserror` of item 2.

## 2. Phased construction map

**Memory discipline (16 GB box, every phase)**: `cargo -j 4`; never run
`cargo test` and `cargo clippy` concurrently (both drive codegen units);
no node in any loop (none exists since the TS retirement); profile.dev
`debug = 0` and profile.test `line-tables-only` stay as-is in Cargo.toml.
One commit per numbered step; each commit states its class (C0/C1/C2/B-fix).

**Gate for every phase (acceptance, no exceptions)**:
`cargo test -j 4` → 711 + (new tests only where this plan adds them) green;
`cargo clippy -j 4 --all-targets` zero warnings; the golden digest (P0)
byte-identical unless the commit is a ticked B fix that enumerates its diff.

### Phase 0 — the measurement net (C0, no src changes)

- Scope: add `tests/golden.rs` + a committed digest: every bundled script
  (`scripts/*.txt`, `tests/demo-world.txt`) × a fixed deterministic command
  transcript (replay + a scripted handle_line sequence + reach/lint CLI runs)
  → sha256 of stdout bytes, JSONL bytes, and `snap()` state; one line per
  case in `tests/golden/digest.txt`. The test regenerates and compares.
  **Determinism clause**: every case is keyed by its REPO-RELATIVE script
  path (never the absolute path); tempdirs for JSONL round-trips live under
  `target/` or the OS temp dir and their paths must never enter a digest
  line — only the bytes written INTO them are hashed. Verified on the
  current tree (2026-10-14): no wall-clock can reach the digest — `t_real`
  is `None` on the whole chain (Step/records are built with `t_real: None`
  in every harness and the engine never fills it), and `snap()` emits pure
  JSON with no timestamps — so identical inputs yield identical digest bytes
  on any machine, any day.
- This is the C-class zero-behavior PROOF instrument: before/after any
  refactor, the digest must not move. It complements (not replaces) the 711
  tests — it catches drift in the corners no assertion reaches (exact CLI
  bytes, log files on disk).
- Acceptance: digest committed; suite green; clippy green. No test assertion
  of the existing 711 changes (new file only).

### Phase 1 — dissolve js.rs (C: items 1, 7, 8, 9)

- Prerequisite: phase 0 digest exists.
- Scope: the module split, the mechanical `use` sweep (src + tests imports
  ONLY — assertion lines are frozen), the divergence-comment rewording
  (item 8), README layout/"lives at" column updates.
- Acceptance: gate green; `git diff tests/` shows import lines only
  (review-enforced); digest byte-identical.
- Size: 25 src files + 17 test files touched (§1.3.1 counts), all
  mechanically.

### Phase 2 — internal API shapes (C: items 2-default, 3, 4, 5, 6, 10)

- Prerequisite: phase 1 landed (solo reviewable diffs).
- Scope: `IdGen` trait + `ExecOptions: Default`; `RecordKind` decision
  (enum-with-Other default); `Prepared.bindings` deletion; `Lcg` re-home;
  optional `thiserror` ONLY if ticked (dependency addition lands here,
  declared in §1.3.2).
- Acceptance: gate green; digest byte-identical; new unit tests allowed ONLY
  as property tests over unchanged behavior (e.g. RecordKind round-trips
  every `kind` string byte-for-byte) — never as behavior changes.

### Phase 3 — B decisions (ONLY ticked rows) + close-out

- Prerequisite: phases 0–2 landed; the user's tick list (§1.1 verdict column).
- Scope: one commit per ticked quirk: the fix + exactly the enumerated test
  updates + the README Q-table status column ("fixed 2026-10-xx, see plan
  §1.1") + the golden digest regeneration WITH the diff explained in the
  commit message. Recommended defaults if the user wants a minimal list:
  Q4 (slot-role validation — spec-aligned, kills the worst silent trap),
  Q5 (uniform duplicate-declaration errors), Q2 (tokenizer stops swallowing
  illegal chars); Q15 as the cheap cosmetic extra.
- Close-out commit: README quirk appendix statuses final; this plan's status
  flipped to LANDED with the digest commit hash.
- Acceptance: per-commit test-diff enumeration matches reality; suite green;
  clippy green; spec file untouched (verified by `git diff docs/spec/` empty).

## 3. Risks and verification

- **How C-class zero-behavior is proven** (the §0.1 contract, three legs):
  1. the 711 tests pass with assertion lines untouched (diff review: only
     `use` paths move in tests/ during phases 1–2);
  2. the phase-0 golden digest is byte-identical before/after (catches
     stdout/JSONL/snap drift the assertions might not reach — same judgment
     as the dice suite's two-runs-write-identical-bytes gate);
  3. serde wire shapes are the sharpest risk in phases 1–2 (field order, tag
     spellings, `skip_serializing_if`): the pinned-field-order doc comments on
     `ActionRecord`/`Ref`/`Arg` move WITH their types, and the JSONL
     byte-identity tests (dice.rs two-runs gate, demo replay, snap) are the
     tripwire. A refactor commit that touches a `Serialize` derive must say
     so in the message and justify byte-neutrality.
- **B-fix regression risk**: each ticked fix widens or sharpens an accepted
  input language; the danger is rejecting something a real script needs.
  Mitigations: every bundled script + the demo + nightjar must still compile
  (the corpus includes them); the fix's commit enumerates each new rejection
  with an example; load errors are author-time only — no log bytes move for
  Q2/Q4/Q5/Q15, so the digest diff should be empty or exactly the CLI stderr
  lines the fix intends.
- **Residual risks, ranked**: (1) serde field-order drift in a mechanical
  sweep — caught by leg 2/3 above; (2) B-fix scope creep into engine
  semantics — contained by one-quirk-one-commit and the enumeration rule;
  (3) import-only test edits masking assertion drift — contained by the
  `git diff tests/` review gate; (4) LCG sequence temptation — forbidden by
  §1.3.6 outright.

## 4. Explicitly NOT doing

- No engine-semantics rewrite: A1–A17 are frozen; the fixpoint stays the
  naive round-robin (worklist rewrites remain forbidden, fixpoint.rs:4).
- No spec edits: `docs/spec/text-engine-v0.2.md` is untouched; where a quirk
  fix would contradict it (none in the tick list), that fix is dropped, not
  the spec.
- No deleting the README Q1–Q17 table: the appendix stays, gaining a status
  column that phase 3 keeps in sync with the tick decisions (Q7/Q9/Q17 rows
  get "spec-pinned / implementation note / not-a-defect" annotations).
- No new dependencies beyond the declared optional `thiserror`; no async, no
  runtime, no JSON-stack replacement, no edition bump, no removal of the
  five-bin stdout/exit-code freezes.
- No test-assertion edits outside ticked B commits — including "while we're
  here" modernizations of test bodies.

## 5. Revision record v1 → v2 (review "revise-then-approve", 2026-10-14)

1. **must-fix 1 — §1.3.4 RecordKind**: verified the pinned raw-string
   comparisons (`tests/dice.rs:755/832/1294/1331/1524`,
   `src/engine/engine.rs:2882/2889`); the item now lists `as_str()` +
   `PartialEq<str>`/`PartialEq<&str>` compat impls as a MANDATORY
   precondition keeping every `== "complete"` site compiling unchanged, with
   automatic downgrade to default-keep-`String` if they cannot. No other
   edits to the item.
2. **should-fix 2 — §1.3.1 / Phase 1 counts**: corrected to **25 src files +
   17 test files** (`grep -rl 'js::'` criterion, the module-path mention
   count; the v1 numbers counted export-name imports only and had the split
   backwards). Both the §1.3.1 note and Phase 1's Size line now carry the
   corrected figures and the grep criterion.
3. **should-fix 3 — A17 anchor**: added §9.45 with the ruling's own wording
   location (spec line 1721, "Bare literals lex as ONE token in outcome
   args").
4. **should-fix 4 — Phase 0 determinism clause**: digest keys are
   repo-relative script paths; tempdir/absolute paths must never enter a
   digest line (only the bytes written into them are hashed); recorded the
   verified facts that `t_real` is `None` across the whole chain and `snap()`
   emits timestamp-free pure JSON, so digests are machine- and day-stable.
5. Bundled-script list for P0: review found no issue — unchanged.
