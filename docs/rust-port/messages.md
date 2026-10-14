# 错误消息对照表（messages.md）

plan §2.5 / TEST-14 / 验证 04 must#4：错误消息文本是契约。三列对照：消息模板 ↔ 首个钉住它的测试 ↔ TS 源行；Rust 构造点随波登记。模板必须逐字符相同（含空格与标点）；TS `${…}` 插值在表中以 `${…}` 原样记。随波增补，W6 收口覆盖全仓 192 条 `toThrow`（验证 01 CORE-16/TEST-05 勘误口径）。

状态图例：**built** = Rust 构造点已存在；**pinned** = 模板已在 Rust 测试侧逐字冻结（构造点在后续波）；**registered** = 仅登记文本与出处（W2/W3 落地时升为 pinned/built）。

| # | 状态 | 消息模板 | 首个钉住的测试 | TS 源行 | Rust 构造点 |
|---|------|---------|---------------|---------|------------|
| 1 | pinned | ``line ${ln + 1}: seed must be an integer, got '${m[2].trim()}'`` | test/dice.test.ts:181（`/seed must be an integer/`） | src/content/script.ts:113 | （W2 content/script.rs；模板冻结于 rust/tests/dice.rs `script_seed_error`） |
| 2 | built | ``deck '${deck.id}' is exhausted — no 'reset' and no 'defaultcard' (§9.42)`` | test/decks.test.ts:410（refusal `/deck 'SINGLES' is exhausted — no 'reset' and no 'defaultcard'/`；原文含 `(§9.42)` 后缀） | src/content/content.ts:476 | rust/src/core/deck_rng.rs `draw_from` |
| 3 | registered | ``ROLL: dice count must be an integer >= 0, got ${count ?? 'nothing'}`` | test/dice.test.ts:271/346（refusal `/ROLL: dice count must be an integer >= 0/`） | src/engine/engine.ts:764 | （W3 engine/engine.rs） |
| 4 | registered | ``ROLL: sides must be an integer >= 2, got ${sides ?? 'nothing'}`` | test/dice.test.ts:315/328（refusal `/sides must be an integer >= 2/`） | src/engine/engine.ts:767 | （W3 engine/engine.rs） |
| 5 | registered | `'roll' is unbound — no ROLL has run in this outcome list before this point` | test/dice.test.ts:423/433（refusal `/'roll' is unbound/`） | src/core/predicate.ts:477 | （W3 core/predicate.rs） |

备注：

- 行 1 的 `line` 前缀是 1 基行号（`ln + 1`，`ln` 为 `text.split('\n')` 的 0 基下标）；dice.test.ts 的 rig 里 `seed:` 行在 0 基下标 4，消息即 `line 5: …`（rust/tests/dice.rs 已按此冻结）。
- 行 3/4/5 为 W0 预登记（文本自 engine.ts:764/767、predicate.ts:477 原文抄录），W3 移植构造点时逐字核对并升为 built。
- 表行增量须与各波移植的 `toThrow` 数对账（§7 规则 13(d)）；W0 波移植的 dice.test.ts:181 一条对应行 1（decks.test.ts:410 是 refusal/toMatch 断言，非 toThrow，行 2 因此单独登记）。
