# Plan — Rust-idiomatization: retiring the TS-alignment shape (no behavior change by default)

Status: ACTIVE plan — **v3** (2026-10-15): the conclusions of the three
read-only verification reports (`idiomatization-verification-{A,B,C}.md`)
are merged; the fix list of §2 Phase 3 is SETTLED. Sits after the v0.4/v0.5
feature plans, which are landed history. Written after a full-tree
read-through; file:line claims verified on the day and may drift —
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
2. **Tests**: 711 green today — **MEASURED basis**: a real `cargo test -j 4`
   run (per-suite result lines summed: 79 unit + 632 integration), 2026-10-14.
   A static `grep '#\[test\]'` yields only 615 — the grep is NOT the baseline;
   P0 re-baselines from a fresh measured run and the gate number follows the
   run. `cargo test -j 4` ≈ 45 s wall; clippy `--all-targets` is already
   zero-warning.
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
| A3 | Grammar, canonical output forms, and the round-trip law (`format(parse(row)) === row` for engine-written rows; paren-less SPAWN/NARRATE/REPLACE/ENDING/DRAW; pre-verbal AT; sorted actors; bare-name refs) | §5.1, §5.2, §9.15, §9.14 |
| A4 | Duration rendering: single exact unit first (d, h, halfhour, minutes), compound only when no unit fits (`1halfhour` = 30 min; `61m` → `1h1m`) | §5.1, §9.45 |
| A5 | Log format: JSONL one record/line; field set incl. `tGame`/`advancedMin`/`startMin`/`window`/`recipes`/`sentence`; concrete outcomes fully materialized (ids minted, roles concretized, `aspects=`/`attrs=` pairs, REPLACE → SPAWN+DESTROY pair with `attrs=` always) | §7, §9.15, §9.18, §9.33, v0.5 note(:180-181) |
| A6 | SET deltas logged as absolutes; IF concretizes to the taken branch only; the log never contains IF/FN/call sugar | §9.31, §9.39, §9.40 |
| A7 | Execution model: validate → select → apply → time → events; step-atomic REFUSE (logged, zero time) vs §9.19 rejections (nothing logged); monotonic clock; clause re-resolution against the draft | §6.6, §6.5, §9.19, §9.20 |
| A8 | BEGIN/COMPLETE/WAIT rows; windows table is the lock; completion anchors on the window's start via `beginId`; advance passes ordered by (end minute, begin id, clause), completions before slot events, no cascade, events span dead air | §6.7, §9.28 |
| A9 | Events: authored triggers before expiries (expiry matching in declaration order); `once` fired-ness as a fold projection; fold trusts EVENT rows; expiry `expire:<NAME>` id; authored-expiry replaces destroy entirely | §6.6, §9.29, §9.32, §9.34 |
| A10 | Names unique among the living; stackable pools address the OLDEST copy in birth order; engine-written destroys address by `#id` | §9.14, §9.38 |
| A11 | Endings: `endings:` table, first-ENDING-wins, post-ending rejection naming the title, undo reopens; ENDING load-time existence check | §9.35, §10 |
| A12 | Functions: compile-time expansion, canonical-equals-expansion, depth 8 cap, recursion-cycle load error, arity check, param shadowing, undeclared `{token}` load error | §9.40, §10 |
| A13 | Load-time checks EXIST (wording is B): trigger PROGRESS/REFUSE guards; role-operands-in-trigger-IF; `count()`/`total()` arg lint; ROLLED/DRAWN authoring errors; deck spec/defaultcard resolution; reserved words | §10, §6.3, §9.34, §9.39, §9.41, §9.42 |
| A14 | Play surface: lenient input resolution rules (case-insensitive, underscores, articles, longest match ≤ 4 tokens, prefix-then-edit-distance-≤2 suggestions); `Game.visible()` projection; undo = drop trailing events + last action, re-fold; `suggestions()` lights non-default non-REFUSE recipes | §11 |
| A15 | Prose shape: verb-template line per clause; `PROGRESS` ticks indented with `(+offset)`; NARRATE unindented; REFUSE its own line; `{roll}` rewritten at execution | §5.3, §9.41(42) |
| A16 | Q7 is spec behavior, not a quirk to fix: a REFUSED step's NARRATE may carry the rehearsed dice; the counter never moved; re-attempt throws the same dice | §9.41 (refused-rolls bullet) |
| A17 | `1halfhour` lexes as ONE duration token (Q9's observable side) | §5.1, §9.45 (the ruling's own words, spec line 1721: "Bare literals lex as ONE token in outcome args") |

**Verification footnotes (report A, 2026-10-14 — 17/17 VERIFIED, no downgrades;
the four notes below are merged as-is and change no A/B/C placement):**

1. **A2's §9.42**: the spec's own cross-references cite the deck ruling as
   §9.42 (spec :326/:398/:1825) while the ruling's printed number is 43 — a
   spec-internal misprint; this plan follows the spec's citation convention.
2. **A3's paren-less set** is pinned at spec :401 — SPAWN, NARRATE, REPLACE,
   ENDING (plus DRAW at :398/:326); v2's list omitted REPLACE (added above).
3. **A5's `window` field name**: the spec pins `startMin` (:181), the
   `beginId` anchor (:834) and the windows-table concept (:858); the JSON
   spelling `window` is engine naming, not spec text.
4. **A9's ordering**: "(declaration order)" is spec text only for
   expiry-trigger matching (§9.32, :1211); the mutual order of multiple TIME
   triggers within one halfhour slot has no spec text — that sub-behavior is
   B (test-pinned only). The row stays A on its remaining, explicit content.

### 0.3 What only tests (or nothing) pin — the B/C boundary rule

The spec repeatedly says *what must be named/reported* without pinning *the
bytes*: "naming the valid forms" (§6.3), "names the cycle" (§9.40), "the
message names the ending's title" (§9.35). So **error-message EXISTENCE and
content-requirements are A; exact message WORDING is B** (191
`assert_err_msg` sites pin today's regexes character-for-character). The same
rule sorts every JS-semantics fossil: if the log bytes or accepted input
change, it is B; if only internal shape moves, it is C.

## 1. The classification (core deliverable)

Quantities: **A = 17 items** (§0.2 table, all VERIFIED by report A),
**B = 19 rows** (14 quirk rows + 5 non-quirk conventions; v3 settles the
dispositions: **10 fix items** in three batches, 9 no-fix/keep),
**C = 12 items** (§1.3; 8 VERIFIED, 4 RISKY with the risk merged into the
items — 0 REFUTED).

### 1.1 Class B — test-pinned, spec-silent (the DECISION CHECKLIST)

**v3: the checklist is SETTLED** by `idiomatization-verification-B.md` (every
row's impact re-measured by grep; three fix directions re-ruled). The fix set
is ten items in three batches — see §2 Phase 3. Everything else is
**explicitly no-fix** (behavior untouched; README rows get a "no-fix,
verified" annotation). Q7/Q9/Q17 were never decision items (spec-pinned /
implementation note / not a defect).

**Quirk rows** (README appendix, all frozen today; "pins" = the measured
existing-test footprint, corrected by report B where v2 overstated):

| Q | What it is | Verification-B final ruling (impact re-measured) | Disposition |
|---|---|---|---|
| Q1 | recipe ids mint 0-based at compile (`.recipe0`) but 1-based in the lint mirror (`.recipe1`) | fix by flipping the LINT side to 0-based (script.rs:1269) — zero log churn, golden digest untouched. v2 overstated the pins: only script_play.rs:345 + content.rs:1215 (v2's "engine.rs, lint.rs" was wrong); bundled scripts have no id-less non-default recipes | FIX — batch 2 |
| Q2 | the predicate string tokenizer silently swallows illegal chars (`aspect:x >= @3` ≡ `aspect:x>=3`) | fix is spec-ALIGNED (§6.3/§4.5 grammar has no `@`; the only whitespace license is "spaces around a comparison are glue" — the fix must, and only must, keep skipping whitespace); aligns predicate.rs with query.rs, which already errors loudly. Pins: 1 unit test (predicate.rs:1160) | FIX — batch 1 |
| Q3 | `arg()` cannot read a param key (`SET(myparam = 3)` parse error) while SPAWN args support one | fix = TEACH: extend the error message and accept the `key=value` arg form in the arg path. The v2 "reject uniformly" option is REFUTED: spec §9.40 pins the pair "key or value" acceptance face — the op must NOT start rejecting. Zero existing pins; new tests only | FIX (teach only) — batch 2 |
| Q4 | slot roles are never validated — a typo'd slot compiles and silently never matches | fix is spec-ALIGNED (§4.3's six-role list + §10 "failing loudly at load time … bad grammar"); one check at script.rs:1065 via the existing `Role::from_lower`. Zero breakage: every slot key in bundled + test scripts is legal (actors 24 / object 16 / at 8 / with 4 / about 2 / to 1); new error cases only | FIX — batch 1 |
| Q5 | duplicate declarations inconsistent: entities/endings error; prototypes/templates/verbs silently overwrite; `ANA`/`ana` case-collide | fix = uniform load-time duplicate errors (functions and decks already error — content.rs:1038/:856 — the fix converges on the majority). MUST also add the **minted-id collision check**: script.rs:1154 mints `s_{name.to_lowercase()}`, so `ANA`/`ana` collide by ID and case-SENSITIVE name dedup cannot catch it — the fix is a "minted id `s_ana` already in use" load error, NOT case-insensitive dedup (that would conflict with case-exact names, §9.14). Pins kept: decks.rs:1165, endings.rs:196; silent overwrites unpinned | FIX (+ id check) — batch 1 |
| Q6 | a BEGIN whose format∘parse round-trip fails silently opens no window | fix = loud fold error. v2 mispointed the pins: the pin is ONE unit test, engine.rs:3263 (`register_windows_roundtrips…silently_skips_degenerate_rows`), not fold/store integration tests | FIX — batch 3 |
| Q7 | REFUSED step's NARRATE carries rehearsed dice | — | spec-blessed (A16, §9.41) — not a decision item; README row status "spec-pinned" |
| Q8 | JS `\s` ≠ Rust `\s`; every regex spells the explicit JS class | NO-FIX: cost/benefit inverted — the divergence is only U+FEFF/U+0085, bundled content uses neither, and the change touches 6 src files + hand scanners with the widest regression surface | NO-FIX (verified) |
| Q9 | `DUR_RE` alternation order is load-bearing | — | implementation note (A17 owns the behavior) — no fix object |
| Q10 | quoted-string `\X` takes the next char literally (`"\n"` = letter n); trailing backslash still throws | fix = real escape semantics (lexer.rs:161). v2 overstated: pins are 1 unit test (lexer.rs:309); bundled content uses only `\"`, byte-identical under the new semantics | FIX — batch 3 |
| Q11 | bare `inspect` shows the first visible card (`startsWith('')`) | fix = usage/card-list on bare `inspect`. Pins: repl.rs:60 (+ mirror :102-104) ONLY — v2's "cli" half was wrong (cli.rs has no inspect pin). game.rs:305's "no test pins it" comment is stale and is corrected in the same commit | FIX — batch 2 |
| Q12 | ROLES are soft-reserved in outcome-arg position but not reserved words | NO-FIX (fix direction REFUTED): §5.1's reserved-word list is explicit and excludes the role names, and §9.40's shadowing order ("before entity names and before role names") pins role precedence as design semantics — "fixing" would VIOLATE §5.1 | NO-FIX (refuted) |
| Q13 | aspects are one level deep; `'a.b.c'` lands as `{a:{'b.c':v}}`; last-write-wins clobber both directions | NO-FIX (fix direction REFUTED): §4.2 pins the data model itself ("weighted principle tags, dot sub-keys", two levels); a nested tree exceeds the spec's model (a query.rs:410 unit test also pins the flattening) | NO-FIX (refuted) |
| Q14 | same-ref comparison errors on a param ref; relation keys embed `\|` | NO-FIX (unreachable): the sameRef half DIED with the TS port (dead code never ported — the README row text is outdated); the remaining `\|`-key half is unreachable (engine-minted `#hex` ids only), so re-encoding is pure churn over 4–6 snapshot pins. P3 corrects the README row text as an errand | NO-FIX (unreachable) |
| Q15 | CLI shapes: trailing `--demand` silently vanishes; linter exits 1 on missing directory; unknown meta key blamed on "bad clock" | fix; pins: cli.rs :37 (usage exit2) / :50 (trailing demand) / :89 (bad demand exit1) / :145 (missing dir exit1); lint.rs not involved (v2 wrong); the "bad clock" misattribution has zero pins | FIX — batch 2 |
| Q16 | a `-0` delta formats as `+0`; `SET(x,-0)` is authorable | NO-FIX (fix direction REFUTED, vacuous): i64 has no `-0` and parse.rs:971 already normalizes `-0` to `Delta{0}` — "canonicalize at parse" is a no-op; the real alternatives would break the delta-vs-absolute grammar distinction (§9.31) or need a sign-bit model change. Zero pins exist | NO-FIX (refuted) |
| Q17 | reach `site_reason`'s empty-init-names branch is reachable, not dead code | — | a "not dead code" note, not a decision item |

**Non-quirk B rows** (conventions the tests pin and the spec is silent on):

| # | What | Final ruling |
|---|---|---|
| B-18 | Error-message wording everywhere (250 `ChorusError::new` sites; 191 `assert_err_msg` regex pins — both counts exactly confirmed) | keep; "fixing" is pure churn over 191 regex pins with no direction gain |
| B-19 | `cmp_utf16` collation for sorts/keys/snap (UTF-16 code-unit order; differs from Rust byte order only for non-BMP names) | keep; the name grammar is ASCII-only, so the difference is reachable only via quoted-string refs — near-unobservable; stays B (A3 pins "sorted", not the collation) |
| B-20 | `OrdinaryOwnPropertyKeys` key order + `jsjson` escape set (log byte order for integer-like attrs keys, `\u00xx` lowercase escapes, DEL raw) | keep; integer-like keys are UNREACHABLE through the authored grammar (name/dottedName are letter-first) — the machinery is effectively C, and the escape set already matches `JSON.stringify`/serde_json. serde_json's `preserve_order` feature is load-bearing here: do not touch |
| B-21 | `cmp_ascii_ci` suggestion-pool sort (ASCII-lowercase then byte order; ICU stays out) | keep; fixing is player-visible suggestion-order churn for nothing |
| B-22 | `js_num_parse`'s f64 middle step above 2^53 | FIX — batch 3. The f64 middle step silently rewrites authored integers past 2^53; a direct i64 parse is MORE spec-faithful (§10: `seed:` is "an integer"). v2's justification was wrong: the pins are js.rs:472 + clock.rs:201 only (the dice suite's 2^53−1 seed is exactly representable and does not depend on the f64 step). The overflow tail (beyond i64) must be DEFINED in the fix — saturate (the documented divergence direction) or a loud error; decide at implementation, document either way |

### 1.2 How to read a B fix's cost

Each batch item names: the tests whose assertions change (expected count,
re-measured by report B), whether the delta is player-visible, and whether
the golden corpus (§2, phase 0) digest moves. A B commit that cannot
enumerate all three does not ship. Cross-batch coupling: NONE (the parse
sites are disjoint — report B §4); two cautions: a Q5 test fixture must trip
exactly ONE error (a fixture that also typos a slot would double-fire into
Q4's new check), and only Q11/Q15 move CLI golden lines (enumerated per
commit).

### 1.3 Class C — zero observable footprint (free Rust-ization)

All items below must keep the tests green **without editing any assertion**;
test-file `use`/path lines AND qualified-path occurrences inside test bodies
may be touched mechanically (report C: 12 body lines + 11 signature lines
carry qualified `chorus::js::…` paths — zero semantic edits).

1. **Dissolve `src/js.rs` (662 lines) into honestly-named modules.** The
   module's doc says "JS semantics infrastructure" — under the new baseline
   the JS provenance is history; the *behaviors* remain (B-19/B-20/Q8), the
   umbrella does not. Proposal:
   - `core/text.rs` — the JS whitespace set (`JS_WS_CLASS`,
     **`JS_NON_WS_CLASS`** (added in v3 — v2's relocation list omitted it,
     js.rs:33), `js_is_ws`, `js_char_is_ws`, `js_trim`) + `cmp_utf16` +
     `cmp_ascii_ci`, re-documented as "the engine's frozen collation and
     whitespace classes" (Q8/B-19 cross-links).
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
   counted export-name imports only and had the split backwards). Report C
   notes: js.rs's in-module `#[cfg(test)]` unit tests move with their code
   (assertion text untouched) so the measured test-count arithmetic stays
   honest.
2. **Error channel**: `ChorusError { msg: String }` is a deliberate
   anti-drift choice whose rationale ("variants + Display double maintenance")
   was TS-parity. Options:
   - Default: re-home to `error.rs`, keep the plain message struct, refresh
     the doc to argue the case on its own merits (single construction site per
     message; 191 regex pins are the real maintenance surface).
   - Optional (user opt-in): a `thiserror` enum (NEW DEPENDENCY — declared here,
     `thiserror = "2"`, the only dependency change this plan may make) with
     ~8 site-grouped variants whose `Display` emits today's exact strings;
     `assert_err_msg` untouched. Cost: 250 construction sites migrated;
     benefit: match-site ergonomics, `#[from]` for io/serde.
3. **`ExecOptions`: KEEP the struct, ADD `Default` — the trait is ABANDONED
   (v3).** Report C's census found the footprint v2's call-site list missed:
   README:146's usage example pins the literal
   `ExecOptions { first_record_id: …, id_gen: None }`, and 6 more test files
   hold ≈13 `id_gen:` literals (demo.rs :62/:117 — the only closure sites,
   engine_ordering.rs ×3, session_smoke.rs ×3, parallel.rs ×2, endings.rs,
   nightjar_explorer.rs, plus tests/common/settle.rs's helper typed on
   `Option<&mut dyn FnMut() -> String>`). An `IdGen` trait buys nothing that
   justifies touching those (or the README example). The adopted change is
   strictly additive: keep the struct and the `id_gen` field exactly as-is,
   add `impl Default for ExecOptions`. Every existing literal — README's
   included — compiles unchanged.
4. **`ActionRecord.kind`: KEEP `String` — the v2 downgrade clause FIRES.**
   Report C's full census: ≈69 raw-string comparison sites across 15 test
   files (v2 listed 7 — a 10× undercount; do not use v2's list as a review
   checklist), plus two hard blockers no compat-impl set can survive:
   tests/parallel.rs:316-333 builds a typed `Vec<(String, Option<String>)>`
   from `.kind.clone()` and compares it against `String` literals, and three
   `ActionRecord` fixture literals hard-code `kind` as a `String`
   (dice.rs:214, tests/engine.rs:177, engine_ordering.rs:621). Under the v2
   precondition ("if the compat impls cannot keep all such sites compiling
   without edits, degrade to KEEP String"), the item degrades. What remains
   of it in phase 2: re-document the open-world fallthrough comment
   (engine.rs:53-60) as a choice of this engine. (For the record: an enum
   would also have needed `as_str(&self)`, not `as_str(self)` — engine.rs:2889
   calls it through a reference.)
5. **Dead-shape cleanups**: `Prepared.bindings` + its `#[allow(dead_code)]`
   (engine.rs:114) — delete the field; `slot.max: Option<i64>` (None = the JS
   `Infinity` max) is already idiomatic Rust — keep, only rewrite the comment
   to state the domain fact without the JS origin; `Log.initial_state` (a
   documented test fixture) — keep.
6. **`Lcg`** (js.rs:359): test-side driver RNG. Usage, CORRECTED per report
   C: ONLY tests/common uses it (explorer_lib ×3, reach_fuzz_lib ×10,
   reach_scale_lib ×7, nightjar_explorer ×1; 7 constructor call sites) — v2's
   "(bins use it too)" was FALSE and is deleted. It still cannot be
   `#[cfg(test)]`-gated, for the right reason: integration-test crates
   compile the lib WITHOUT `cfg(test)`, so gating would break them. Its three
   seed entries (`new` XOR / `new(0xc5)` / `from_raw_state('NIGH')`) and
   frozen sequences stay BIT-EXACT — the fuzz campaigns and the seed-keyed
   findings are keyed to them (`fuzz seed 262` at paths.rs:253; census.rs:671
   1963; world.rs:575/624/807), and re-validating campaigns buys nothing.
   Re-home to `lcg.rs`; renaming entries
   (`FuzzRng::seeded/for_scale/with_raw_state`) is optional and moves only
   qualified-path lines under the widened budget.
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
    stay. serde_json's `preserve_order` feature is load-bearing for B-20's
    pinned key order — do not touch (report C). The only permitted addition
    is the optional `thiserror` of item 2.

## 2. Phased construction map

**Memory discipline (16 GB box, every phase)**: `cargo -j 4`; never run
`cargo test` and `cargo clippy` concurrently (both drive codegen units);
no node in any loop (none exists since the TS retirement); profile.dev
`debug = 0` and profile.test `line-tables-only` stay as-is in Cargo.toml.
One commit per numbered step; each commit states its class (C0/C1/C2/B-fix).

**Gate for every phase (acceptance, no exceptions)**:
`cargo test -j 4` → the measured baseline (711 at drafting; §0.1) + (new
tests only where this plan adds them) green;
`cargo clippy -j 4 --all-targets` zero warnings; the golden digest (P0)
byte-identical unless the commit is a settled B fix that enumerates its diff
(only Q11/Q15 may move digest lines — CLI golden lines only).

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
- Acceptance: gate green; `git diff tests/` shows import lines and
  qualified-path occurrences ONLY — zero semantic edits (23 such lines exist,
  report C; review-enforced); digest byte-identical.
- Size: 25 src files + 17 test files touched (§1.3.1 counts), all
  mechanically.

### Phase 2 — internal API shapes (C: items 2-default, 3, 4-remnant, 5, 6, 10)

- Prerequisite: phase 1 landed (solo reviewable diffs).
- Scope: `impl Default for ExecOptions` (struct and `id_gen` field untouched
  — §1.3.3); `Prepared.bindings` deletion; `Lcg` re-home (§1.3.6); the
  RecordKind remnant = re-document only, `kind` STAYS `String` (§1.3.4);
  optional `thiserror` ONLY if the user opts in (dependency addition lands
  here, declared in §1.3.2).
- Acceptance: gate green; digest byte-identical; new unit tests allowed ONLY
  as property tests over unchanged behavior — never as behavior changes.

### Phase 3 — B fixes, SETTLED work list (verification-B final ruling) + close-out

- Prerequisite: phases 0–2 landed. The ten items below are the approved fix
  set (§1.1 dispositions); batches have no cross-coupling (disjoint parse
  sites), batch risk ascends. One commit per item: fix + exactly the pinned
  tests listed + new error cases + README Q-row status + golden digest
  accounting in the message.

- **Batch 1 — compile-time rejections (zero log/digest movement, zero
  existing-test breakage):**
  1. **Q4** slot-role validation — script.rs:1065, one check via
     `Role::from_lower`; new error cases only (all bundled/test slot keys
     verified legal).
  2. **Q5** uniform duplicate-declaration load errors (script.rs
     :516/:534/:564/:581) **plus the minted-id collision check** (script.rs:1154
     mints `s_{lowercase}`): a second `ANA is …` after `ana is …` is a
     "minted id `s_ana` already in use ('ANA')" load error. Existing error
     tests kept: decks.rs:1165, endings.rs:196. Fixtures trip exactly ONE
     error each (a fixture that also typos a slot would double-fire into Q4).
  3. **Q2** tokenizer errors on grammar-illegal characters (predicate.rs:298);
     whitespace stays glue (§6.3); predicate.rs:1160 rewritten.
- **Batch 2 — acceptance widening / CLI / cosmetic:**
  4. **Q3** teach `arg()` the `key=value` form (parse.rs:1021): the error
     message becomes teach-type and the form is ACCEPTED — spec §9.40 pins
     the pair "key or value" acceptance face; the op must not start
     rejecting. New tests only.
  5. **Q1** flip the LINT mirror to 0-based (script.rs:1269), unifying on the
     compile side; digest untouched (no bundled id-less non-default recipes);
     assertions verified: script_play.rs:345, content.rs:1215.
  6. **Q11** bare `inspect` prints usage/the card list (bin/chorus.rs +
     game.rs); repl.rs:60/:102-104 updated; game.rs:305's stale "no test pins
     it" comment fixed in the same commit.
  7. **Q15** CLI shapes — bin/reach.rs argv (`--demand` requires a value),
     lint-scripts.rs missing-directory message, script.rs honest unknown-meta
     error; cli.rs :37/:50/:89/:145 updated; new error texts enumerated in the
     commit message (these move CLI golden lines).
- **Batch 3 — semantic micro-changes (each rewrites exactly its pinned unit
  test):**
  8. **Q10** real escape semantics in quoted strings (lexer.rs:161);
     lexer.rs:309 rewritten; bundled `\"` stays byte-identical (verified).
  9. **Q6** loud fold error on a BEGIN whose round-trip fails (engine.rs:652);
     engine.rs:3263 unit test flips to expect the error.
  10. **B-22** direct-i64 parse of authored integer literals
      (`js_num_parse` → the f64 middle step removed); the >i64 overflow tail
      is DEFINED here — saturate (the documented divergence direction,
      engine.rs:2628 precedent) unless implementation argues otherwise — and
      documented; js.rs:472 + clock.rs:201 rewritten.
- **Errands riding P3**: README's Q14 row text corrected (the sameRef half
  died with the port; only the `|`-key half remains, unreachable) — same
  commit may not change behavior; the README Q-table status column updated
  per item's commit.
- **Explicit NO-FIX list** (behavior untouched; README rows annotated "no-fix,
  verified" at close-out): Q8 (cost/benefit inverted), Q12 (violates §5.1's
  reserved-word list; role precedence is §9.40's pinned design), Q13
  (violates §4.2's pinned data model), Q14 (unreachable), Q16 (fix direction
  vacuous), B-18/B-19/B-20/B-21 (kept).
- **Close-out commit**: README Q-table statuses final (fixed / no-fix
  verified / spec-pinned per row); this plan's status flipped to LANDED with
  the digest commit hash.
- Acceptance: per-commit test-diff enumeration matches reality; suite green
  (measured baseline, §0.1); clippy green; spec untouched (`git diff
  docs/spec/` empty); golden digest byte-identical in EVERY batch-1/2/3
  commit except Q11/Q15, where ONLY the CLI golden lines move and each moved
  line is enumerated in the commit message.

## 3. Risks and verification

- **How C-class zero-behavior is proven** (the §0.1 contract, three legs):
  1. the tests pass with assertion lines untouched (diff review: in phases
     1–2 only `use`/path lines and qualified-path occurrences move in
     tests/);
  2. the phase-0 golden digest is byte-identical before/after (catches
     stdout/JSONL/snap drift the assertions might not reach — same judgment
     as the dice suite's two-runs-write-identical-bytes gate);
  3. serde wire shapes are the sharpest risk in phases 1–2 (field order, tag
     spellings, `skip_serializing_if`): the pinned-field-order doc comments on
     `ActionRecord`/`Ref`/`Arg` move WITH their types, and the JSONL
     byte-identity tests (dice.rs two-runs gate, demo replay, snap) are the
     tripwire. A refactor commit that touches a `Serialize` derive must say
     so in the message and justify byte-neutrality.
- **B-fix regression risk**: each of the ten settled fixes widens or
  sharpens an accepted input language; the danger is rejecting something a
  real script needs. Mitigations: every bundled script + the demo + nightjar
  must still compile (the corpus includes them); the fix's commit enumerates
  each new rejection with an example. No fix in the settled list moves log
  bytes (report B §4) — the only digest movement is Q11/Q15's CLI golden
  lines, enumerated per commit.
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
- No test-assertion edits outside the settled B-fix commits — including
  "while we're here" modernizations of test bodies.
- No fixing the settled NO-FIX rows: Q8, Q12, Q13, Q14, Q16, B-18/B-19/
  B-20/B-21 (§1.1) — their README rows get annotations, their behavior and
  tests stay byte-still.

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

## 6. Revision record v2 → v3 (verification reports A/B/C merged, 2026-10-15)

1. **A class stands at 17/17 VERIFIED (report A), untouched.** The report's
   four notes merged as footnotes under §0.2: A2's §9.42 spec-internal
   misprint; A3's paren-less list gains REPLACE (spec :401); A5's `window`
   JSON spelling flagged as engine naming; A9's "(declaration order)"
   narrowed to expiry triggers — the time-trigger within-slot order is B.
2. **C class amended per report C (8 VERIFIED stand; the 4 RISKY findings are
   merged into the items):**
   - §1.3.4 RecordKind **DOWNGRADED to keep `String`** — the v2 downgrade
     clause fired: measured ≈69 comparison sites (v2's list undercounted 10×)
     and two unbeatable blockers (parallel.rs:316's typed `Vec<(String, …)>`
     fixture and 3 `ActionRecord` literals hard-coding String). Phase 2 keeps
     only the comment re-documentation.
   - §1.3.3 **trait abandoned**: `ExecOptions` keeps its struct and
     `id_gen: Option<&mut dyn FnMut>` field; only `impl Default` is added
     (README:146's example and 6 test files' ≈13 `id_gen:` literals are the
     footprint — all compile unchanged).
   - §1.3.1 / Phase 1 **split gate widened**: test-body qualified-path lines
     (12 body + 11 signature, report C) may move mechanically; the old
     "import lines only" gate was unsatisfiable. `JS_NON_WS_CLASS` added to
     the relocation list; the js.rs unit-tests-move-with-code note added.
   - §1.3.6 **Lcg reasons corrected**: only tests/common uses it (the false
     "bins use it too" deleted); cfg(test)-gating stays forbidden because
     integration crates compile the lib without cfg(test); seed anchors
     re-pointed (paths.rs:253, census.rs:671, world.rs:575/624/807).
   - §1.3.12: serde_json `preserve_order` flagged load-bearing/do-not-touch.
3. **B class re-ruled per report B; Phase 3's work list REWRITTEN and
   SETTLED** (§2 Phase 3, §1.1 dispositions): fix set = batch 1 Q4 → Q5
   (**+ the minted-id `s_` lowercase collision check, per the report**) →
   Q2; batch 2 Q3 (TEACH-type only — spec §9.40 pins pair key-or-value
   acceptance, must not reject) → Q1 (flip the LINT side to 0-based) → Q11 →
   Q15; batch 3 Q10 → Q6 → **B-22**. Moved to explicit NO-FIX with reasons
   cited from the report: Q8 (cost/benefit inverted), Q12 (violates §5.1's
   reserved-word list), Q13 (violates §4.2's data model), Q14 (unreachable),
   Q16 (fix direction vacuous), B-18/B-19/B-20/B-21 (kept). Impact-cell
   corrections folded into the §1.1 table (Q1/Q3/Q6/Q10/Q11/Q16 pins
   re-measured; Q5's Q4-double-fire caution in §1.2).
4. **Errands (v2→v3, item 4 of the review)**: README's Q14 row text outdated
   (sameRef half died with the port) — corrected in P3; game.rs:305's "no
   test pins it" comment stale (repl.rs:60 pins) — fixed with Q11's commit;
   test-count wording switched to the MEASURED basis (711 from a real
   `cargo test -j 4` run; the 615 static `#[test]` grep is not the baseline)
   in §0.1.
