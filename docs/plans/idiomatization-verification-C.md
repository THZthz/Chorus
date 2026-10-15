# Verification — rust-idiomatization.md §1.3, the C-class list (12 items)

Date: 2026-10-14. Method: read-only greps over `tests/`, `src/` (incl. `src/bin/`),
`README.md`, `Cargo.toml` plus targeted file reads. No source file was modified.
Every item in §1.3 was checked (no sampling). Line numbers below were re-located
on this tree today.

Verdict scale: **VERIFIED** (no footprint, suggestion feasible as written) /
**RISKY** (hidden footprint found, or the plan's stated counts/mechanism do not
hold without edits beyond what the plan budgets) / **REFUTED** (a test
assertion, CLI output, or README doc pins the current shape outright).

## Summary table

| # | Item | Verdict | One-line finding |
|---|------|---------|------------------|
| 1 | Dissolve js.rs | **RISKY** (low) | 25 src + 17 test counts exact; but 12 test *body* lines use qualified `js::` paths, so Phase 1's "git diff tests/ shows import lines only" gate is unsatisfiable as written; `JS_NON_WS_CLASS` (js.rs:33) is missing from the proposal's relocation list |
| 2 | Error channel (ChorusError) | VERIFIED | 250 src construction sites (255 incl. tests), 191 `assert_err_msg` — both match the plan; no serde, no literal constructions outside js.rs, tests/bins only use path imports |
| 3 | id_gen → trait | **RISKY** | README:146 pins the literal `ExecOptions { first_record_id: …, id_gen: None }`; the plan's call-site list omits 6 test files holding ~13 `ExecOptions` literals (only demo.rs passes closures) |
| 4 | RecordKind enum | **RISKY** | Listed 7 pinned sites are real but the true surface is ≈69 raw-string comparison sites in 15 test files + 3 `ActionRecord` fixture literals + parallel.rs:316's typed `Vec<(String, …)>` clone, which no compat-impl set can keep compiling — the plan's own downgrade rule fires; also `as_str(self)` must be `as_str(&self)` to compile at engine.rs:2889 |
| 5 | Prepared.bindings deletion | VERIFIED | Field written at engine.rs:407/430, never read (all four `.bindings` reads are `ApplyCtx.bindings`); zero test/bin/README footprint; struct is private |
| 6 | Lcg re-home | **RISKY** (low) | The claim "(bins use it too)" is FALSE — zero `Lcg` references in any bin or non-js.rs src; only tests/common/* use it (which still forbids cfg(test)-gating, so the conclusion stands); "fuzz seed 262" lives at paths.rs:253, not fixpoint.rs |
| 7 | js_-name modernization | VERIFIED | Zero js_*-prefixed usage in tests — renames fully internal; sole exception: renaming `ordinary_own_property_keys` touches tests/common/snap.rs (import + 3 body calls + doc line 22) |
| 8 | Documentation debt | VERIFIED | All 7 cited sites exist and are comments only: engine.rs:1172/1566/1722/2628, census.rs:480, src/bin/reach.rs:78, script.rs:10; README's js.rs footprint is exactly line 169 (Q8 "lives at") |
| 9 | prelude follows moves | VERIFIED | lib.rs prelude re-exports `crate::js::{ChorusError, Result}` and `ExecOptions`/`ActionRecord`; nothing else pins module paths |
| 10 | Module cut stays | VERIFIED | `core/content/engine/reach/session` confirmed in lib.rs; `src/engine/fold.rs` is exactly 52 lines |
| 11 | Edition stays 2021 | VERIFIED | Cargo.toml:4 `edition = "2021"`; no-op decision |
| 12 | Frozen four deps | VERIFIED | [dependencies] = indexmap, serde, serde_json, regex — exactly four; no thiserror today |

**Tally: 8 VERIFIED · 4 RISKY · 0 REFUTED.** No test, CLI output, or README
document pins any C-item's *behavior*; every RISKY finding is about the plan's
stated migration surface (counts, signatures, gates), not about observability.

## Per-item detail

### 1. Dissolve js.rs — RISKY (low)

- **Counts check out.** `grep -rl 'js::' src` = **25 files**, `… tests` =
  **17 files** — exactly the plan's corrected figures. `src/js.rs` is exactly
  **662 lines**. All exports named in the proposal exist (cmp_utf16,
  JS_WS_CLASS, js_is_ws, js_char_is_ws, js_trim, ChorusError/Result,
  js_num_parse, static_regex, grp_text/grp0_text/grp0_start/grp_opt,
  is_integer_like_key, ordinary_own_property_keys, clone_map_in_js_order,
  JsJson/jsjson/jsjson_obj_in_js_order, cmp_ascii_ci, Lcg).
- **Footprint.** Tests import only: `Result`, `ChorusError`, `cmp_utf16`,
  `Lcg`, `ordinary_own_property_keys` (12 import lines across 12 files). The
  js-domain internals (jsjson, static_regex, grp_*, js_trim, js_num_parse,
  JS_WS_CLASS, JS_NON_WS_CLASS) have **zero** test usage — they are
  src-internal (14 src files). Bins use only qualified paths
  (`src/bin/reach-fuzz.rs:31,34` js_char_is_ws; `src/bin/reach.rs:26,101`
  cmp_utf16/ChorusError; `src/bin/chorus.rs:167,186` Result) — internal code
  paths, mechanical.
- **The risk.** (a) **12 test body lines** use qualified `chorus::js::…`
  paths and break when the module disappears: reach_fuzz_lib.rs
  :846/858/912/931/933/935/958/960/973 (`sort_by(chorus::js::cmp_utf16…)`)
  script_play.rs:1026-1027 (same), nightjar_explorer.rs:336
  (`chorus::js::Lcg::from_raw_state`), plus 11 signature/annotation lines
  (`-> chorus::js::Result<…>` at dice.rs:144/1238, nightjar.rs:37/51,
  lint.rs:35, demo.rs:32/92/95, conditionals.rs:115, harness.rs:22).
  §1.3's header budget ("use/**path** lines may be touched mechanically")
  covers these, but **Phase 1's acceptance gate "git diff tests/ shows
  import lines only (review-enforced)" is unsatisfiable as written** —
  either widen the gate wording to "import + qualified-path lines" or the
  phase cannot pass its own review. (b) The proposal's relocation list omits
  **`JS_NON_WS_CLASS`** (js.rs:33) — it must ride to `core/text.rs` with
  `JS_WS_CLASS` (it has no test usage; omission is housekeeping, not
  behavior). (c) js.rs's 9 in-module `#[cfg(test)]` tests move with the
  split; assertion text is untouched but the plan should say "unit tests move
  with their code" to keep the 711-count arithmetic honest.

### 2. Error channel — VERIFIED

- `ChorusError::new` sites: **250 in src**, 255 including tests — matches the
  plan's "250 construction sites". `assert_err_msg` in tests: **191** —
  matches.
- **serde face:** js.rs:53 derives only `Debug, Clone, PartialEq, Eq` — no
  Serialize/Deserialize; the error never enters JSONL, snap() output, or any
  logged row. No test constructs `ChorusError { msg: … }` literally (the only
  literal is the `new` impl itself at js.rs:60).
- Bins touch it only via paths (`src/bin/reach.rs:26,101`); Display prints
  `msg` unchanged, so stdout/stderr bytes cannot move. Re-homing to
  `error.rs` + prelude follow (item 9) is footprint-free. The optional
  thiserror variant (Display emitting today's exact strings) is feasible by
  the same count; no test edits needed under either option.

### 3. id_gen channel → trait — RISKY

- **Full call-site census** (`grep -rn 'id_gen:'` + `ExecOptions`):
  src — engine.rs:98 (def) + 13 `id_gen:` lines (unit tests at
  :2849/:2868/:3338 and internal `&mut dyn FnMut` plumbing :771/:1207/:1391/
  :1809/:2023/:2063/:2138/:2194/:2270/:2441/:2512/:2539/:2565), store.rs:179/
  :196 (unit tests, both `None`), game.rs:136 (`None`), lib.rs:37 (prelude
  re-export). tests — **the plan lists only "engine, store tests, game.rs,
  tests/common/settle.rs", but 6 more test files construct ExecOptions
  literals:** demo.rs:62/:117 (the only `Some(&mut …)` closure sites),
  endings.rs (1), engine_ordering.rs (3 literals :150/:170/:354),
  nightjar_explorer.rs (1), parallel.rs (2), session_smoke.rs (3) — ≈13
  `id_gen:` lines beyond settle.rs — plus the settle.rs channel helper
  (:31-:44, typed on `Option<&mut dyn FnMut() -> String>`).
- **README footprint:** README.md:146 shows
  `ExecOptions { first_record_id: log.next_record_id(), id_gen: None }` in
  its usage example — **this pins the current construction form in the
  docs**. The plan's item 8 covers README's layout/Q-table but never
  enumerates this line; the trait change (and `ExecOptions: Default`)
  makes the snippet stale and it must be updated in the same commit.
- **Feasibility:** if the trait keeps the field shaped
  `Option<&'a mut dyn IdGen>`, all `id_gen: None` literals compile
  unchanged and the two demo.rs closure sites coerce through the proposed
  blanket `FnMut` impl (struct-literal fields are coercion sites). So the
  migration is doable with zero test edits under that shape — but NOT under
  a shape where `id_gen: None` disappears (the plan's stated Default
  ergonomics), which would edit ~13 test lines. Verdict: hidden footprint
  (README + unlisted files) → RISKY; amend the call-site list and the README
  line before phase 2.

### 4. ActionRecord.kind → RecordKind enum — RISKY

- **The 7 listed sites are real:** tests/dice.rs:755/:832
  (`filter(|x| x.kind == "complete")`), :1294/:1331/:1524
  (`find(|x| x.kind == "event")`), src/engine/engine.rs:2882
  (`assert_eq!(r.records[0].kind, "action")`) and :2889
  (`map(|x| x.kind.as_str())` → `["complete", "wait"]`).
- **But the enumeration is far from complete.** Full census of raw-string
  sites on `ActionRecord.kind` (excluding the unrelated TurnKind::/,
  Entity.kind, SiteKind, SentenceKind surfaces): **≈69 comparison sites
  across 15 test files** — dice.rs (5), tests/engine.rs (11:
  :128/:446/:453/:484/:734/:1439/:2122/:2151/:2218/:2629/:2641),
  engine_ordering.rs (8: :185/:286/:329/:370/:774/:861/:980/:1064),
  nightjar.rs (6: :204/:328/:382/:392/:554/:678), stackable.rs (6:
  :736/:751/:772/:883/:898/:1070), conditionals.rs (5: :100/:1050/:1052/
  :1084/:1105), common_smoke.rs (4: :191/:195/:196/:253), parallel.rs (7
  plus the block below: :253/:432/:473/:563/:605/:676/:688), endings.rs (4:
  :674/:726/:771/:780), nightjar_explorer.rs (5: :105/:123/:136/:141/:155),
  session_smoke.rs (2: :159/:185), functions.rs (2: :89/:933), settle.rs (2:
  :119/:130), decks.rs (1: :480), script_play.rs (1: :953); plus src/engine/
  engine.rs fold routing :1966/:1985/:2040/:2048 (`record.kind == "wait"/
  "complete"/"event"`) and unit tests :2983/:3108/:3176/:3247. The compat
  impls (`PartialEq<&str>`/`PartialEq<str>` + mirrored, `as_str`) DO
  generalize to all of these — the design survives, but the "verified call
  sites" list understates the surface ~10× and must not be used as the
  review checklist.
- **Two hard blockers for "zero test edits":**
  1. **`as_str(self)` does not compile at engine.rs:2889** — `x` is
     `&ActionRecord`, so a by-value `as_str(self)` moves out of a reference
     (E0507; `RecordKind::Other(String)` is not Copy). The signature must be
     `pub fn as_str(&self) -> &'static str`. With `&self` it also satisfies
     the `Vec<&str>`/`Vec<String>` collects at tests/engine.rs:128,
     conditionals.rs:1050, common_smoke.rs:191, engine_ordering.rs:185,
     parallel.rs:253/676/688 (`collect` converts `&str` via `From`).
  2. **tests/parallel.rs:316-333 cannot compile under any compat-impl set:**
     `let shapes: Vec<(String, Option<String>)> = … (x.kind.clone(), …)`
     then `assert_eq!(shapes, vec![("complete".to_string(), …)])`. Once
     `kind: RecordKind` (which must stay `Clone` for the derived
     `ActionRecord: Clone`), `.clone()` yields `RecordKind` and both the
     annotation and the `String`-typed literal comparison break. This is a
     fixture/annotation line — neither an assertion nor a use/path line —
     so it is outside the plan's edit budget.
- **Three ActionRecord fixture literals also hard-code `kind` as String:**
  tests/dice.rs:212-214 (`kind: "complete".to_string()`), tests/engine.rs:
  176-177 (same), tests/engine_ordering.rs:620-621 (`bare_complete`, same).
  Struct-literal fields need an exact-typed expression, so each requires a
  textual edit (e.g. `kind: "complete".into()` + `From<&str>`).
- **serde face:** kind IS observable — it is the second field of every JSONL
  row (store.rs:64 `serde_json::to_string(r)` per row) and the doc comment at
  engine.rs:53-55 pins field order + explicit `null`s. The plan's mitigation
  (hand-written serde impls: serialize as the raw string, `Other(s)`
  pass-through, unknown-string deserialize) preserves bytes exactly, and
  snap() does **not** include records (tests/common/snap.rs builds
  `entity_json` from `Entity.kind`, a different field — snap.rs:107). The
  fold's open-world fallthrough (unknown kind → action branch) is preserved
  by routing `Other` down the same branch; engine.rs:1966-2048 compile
  unchanged under `PartialEq<&str>`.
- **Conclusion:** the observable face is protectable, but per the plan's own
  mandatory precondition ("if those compat impls cannot keep all such sites
  compiling without edits, the item degrades to default KEEP String") the
  **downgrade rule fires as written**: parallel.rs + 3 fixtures exceed the
  zero-edit budget. Either accept a narrowly-enumerated 4-file mechanical
  test-fixture edit (amending the budget), or default to KEEP `String`.

### 5. Dead-shape cleanups — VERIFIED

- `Prepared` is a private struct (engine.rs:107); its `bindings` field
  (:114-115, with the quoted `#[allow(dead_code)]`) is written at :407 and
  :430 and **never read**: all four `.bindings` read sites (:723/:868/:1156/
  :1218) are `ctx.bindings` / `sctx.bindings` on `ApplyCtx`, a different,
  live field. `fold_bindings` (engine.rs:246) is fold-side and independent.
- Footprint: zero occurrences of the field in tests/, src/bin/, README.
  Deleting it touches engine.rs only. `slot.max: Option<i64>` and
  `Log.initial_state` are keep-decisions — nothing to verify beyond that
  they exist as described (they do).

### 6. Lcg re-home — RISKY (low)

- **The "(bins use it too)" justification is FALSE.** `grep -rn 'Lcg' src`
  outside js.rs returns **nothing** — none of the five bins references it
  (src/bin/reach-fuzz.rs uses only `js_char_is_ws`). Real users:
  tests/common/explorer_lib.rs (3 refs), tests/common/reach_fuzz_lib.rs (10),
  tests/common/reach_scale_lib.rs (7), tests/nightjar_explorer.rs (1) —
  7 `Lcg::` constructor call sites total. The operative conclusion (do NOT
  `#[cfg(test)]`-gate) is still correct, but for the right reason:
  integration-test crates compile the lib without `cfg(test)`, so gating
  would break them. The plan's stated reason should be corrected.
- **Seed findings:** the tree does key committed findings to Lcg seeds, but
  "fuzz seed 262" is at **src/reach/paths.rs:253**, not fixpoint.rs
  (fixpoint.rs:4 carries only the worklist ban). Other seed-comment anchors:
  census.rs:671 (1963), world.rs:575 (7475), world.rs:624 (686), world.rs:807
  (65415). Bit-exactness requirement confirmed: js.rs's unit test
  `lcg_reproduces_the_frozen_sequence_bit_for_bit` (:449) pins `Lcg::new(0)`,
  `from_raw_state(0x4e49_4748)` ('NIGH'), and `Lcg::new(0xc5)` sequences.
- The proposed optional rename (`FuzzRng::seeded/for_scale/with_raw_state`)
  would edit test body call sites (the 7 above + js.rs's own unit test) —
  allowed only under the widened "path lines" budget (see item 1).

### 7. Internal naming modernization — VERIFIED

- `grep -rn 'js_trim|js_num_parse|js_is_ws|js_char_is_ws|jsjson|JS_WS_CLASS|
  JS_NON_WS_CLASS|static_regex|grp_text|grp_opt|grp0_'` in tests: **zero
  hits**. All js_* usage is src-internal (14 src files). Renaming is
  behavior-invisible by construction.
- Sole exception to "no test impact": `ordinary_own_property_keys` (the
  item's example rename to `object_key_order`) is imported and called three
  times in **tests/common/snap.rs** (:32 import, :152/:163/:179 calls, doc
  at :22). Those are mechanical name lines but body calls, inside the same
  gate-wording caveat as item 1.

### 8. Documentation debt — VERIFIED

All cited "divergence" sites exist and are comments (no string literals, no
stdout): engine.rs:1172 (saturating subtraction), :1566 (saturating add),
:1722 (dice sum), :2628 (checked_step doc), census.rs:480 (demand-descriptor
JSON.stringify note), **src/bin/reach.rs:78** ("recorded divergence — stderr
text, not the stdout freeze" — the plan's bare "reach.rs:78" is the bin),
script.rs:10 (regex-domain freeze). README's only js.rs reference is line 169
(Q8's "lives at" column); README's layout section (lines 27-29) lists
src/core / src/engine contents without naming js.rs, so "README follows the
split" reduces to the Q8 cell + optionally adding the new core modules to the
layout list. All comment-only, zero-footprint.

### 9. prelude — VERIFIED

src/lib.rs prelude re-exports `crate::js::{ChorusError, Result}` (:36) and
`execute_step, ActionRecord, ExecOptions, Step` (:37). Nothing outside lib.rs
requires these re-exports to keep their current module paths (tests import
via `chorus::engine::engine::…` or `chorus::js::…` directly). Following the
moves plus rewording the prelude's "JS-semantics error channel" comment is
footprint-free.

### 10. Module split mirroring — VERIFIED

lib.rs declares exactly `content, core, engine, js, reach, session` (js is
item 1's target); `src/engine/fold.rs` is exactly **52 lines**. Nothing pins
merges or further splits. No action, no footprint.

### 11. Edition — VERIFIED

Cargo.toml:4 `edition = "2021"`. The item is a decision to stay; tree state
matches; no footprint.

### 12. Dependencies — VERIFIED

`[dependencies]` is exactly indexmap = "2", serde = "1" (derive),
serde_json = "1" (preserve_order), regex = "1" — the frozen four; no
thiserror, no others. The item's "only permitted addition is optional
thiserror" is consistent with today's tree. Note `preserve_order` on
serde_json is load-bearing for B-20's pinned key order — worth an explicit
"do not touch" in the plan text.

## REFUTED / RISKY list

**REFUTED: none.** No test assertion, CLI stdout/stderr freeze, or README
document pins any C-item's internal shape; every proposed change has a
byte-neutral path.

**RISKY (4), in priority order:**

1. **Item 4 (RecordKind)** — the plan's "zero test edits, default DO IT"
   verdict contradicts its own downgrade rule: parallel.rs:316-333
   (typed `Vec<(String, Option<String>)>` + `.kind.clone()` + String-typed
   literal comparison) cannot compile under any as_str/PartialEq compat set,
   and 3 ActionRecord fixture literals (dice.rs:214, tests/engine.rs:177,
   engine_ordering.rs:621) hard-code String. Also `as_str(self)` must be
   `as_str(&self)` (engine.rs:2889 moves out of a reference), and the
   "verified call sites" list (7) understates the real ≈69-site surface
   10×. Fix: amend the signature, replace the site list with the compat-impl
   criterion + full census, and either budget the 4-file fixture edit or
   take the KEEP-String default.
2. **Item 3 (id_gen trait)** — README:146 pins the `ExecOptions { …,
   id_gen: None }` example (unlisted in the plan), and 6 test files with
   ≈13 ExecOptions literals are absent from the call-site enumeration. Doable
   with zero test edits only if the field stays `Option<&mut dyn IdGen>`;
   the stated `ExecOptions: Default` ergonomics would edit those lines.
3. **Item 1 (js.rs split)** — Phase 1's acceptance gate "git diff tests/
   shows import lines only" is unsatisfiable: 12 test body lines + 11
   signature lines carry qualified `chorus::js::` paths (reach_fuzz_lib.rs
   ×9, script_play.rs ×2, nightjar_explorer.rs:336, etc.). Widen the gate to
   "import + qualified-path lines" and add `JS_NON_WS_CLASS` to the
   relocation list.
4. **Item 6 (Lcg)** — factual corrections only: bins do NOT use Lcg
   (tests/common/* do, which equally forbids cfg(test)-gating), and "fuzz
   seed 262" is paths.rs:253, not fixpoint.rs. No migration risk once the
   justification is fixed.
