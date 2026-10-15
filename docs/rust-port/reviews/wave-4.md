# W4 波次门评审记录（§7 规则 13，PR3-04）

- 评审对象：`rust-port-w3..HEAD`（7 commit：golden-dump.ts 工具 → 244 条 goldens + manifest → IntoTarget::Ref newtype 修复 → anchor-station-locks 连续性修复 → reach_fuzz_lib.rs → tests/golden.rs 重放 harness → engine_ordering 17 HAND 替换 + messages 行 11 钉准）
- 评审人：wave integrator 指派的独立 reviewer（非本波作者）
- 评审日期：2026-10-14
- 证据方式：本波重点是**差分证据的真实性**，故 (a)(b)(d)(f) 按工具/场景/harness 三层全量读原文对照（tools/golden-dump.ts 2017 行全文 ↔ test/reach-adversarial.test.ts 1017 行 ↔ test/reach-fuzz-lib.ts 507 行 ↔ test/explorer-lib.ts ↔ src/engine/engine.ts 锚点行 ↔ rust/tests/golden.rs 2126 行 ↔ rust/tests/common/reach_fuzz_lib.rs 855 行 ↔ rust/tests/engine_ordering.rs 17 测），(c) 按serde 线形全量审查；(e) **亲自重跑** `cargo test -j 4`（545 passed / 0 failed）并**亲自用 `npx tsx tools/golden-dump.ts --out <tmp>` 单独重生成全部 goldens**（不与 cargo 并发），`diff -r` 与入库 244 条逐字节相同，临时目录已删除。工作树复核：评审后 `git status` 无 src/test/scripts/examples 触碰（docs/rust-port/ 下若干 untracked 文档为既存状态，非本波产物）。

## 总评：**PASS-with-notes**

545/545 全绿（W3 的 537 净增 8：golden.rs +8——六个 family 检查 + 孤儿 driver 探针 + 首分歧行报告探针），TS 侧零改动，goldens 亲测确定性复现（244 条 / 5,498,805 字节 JSONL 重生成零差异）。差分证据链经逐层核验为**真**：工具只读、场景忠实（断言复算门在写任何字节前全过）、判据是字节相等而非 sha/归一化、244 条全覆盖、HAND 替换未放松任何断言。遗留 should-fix 1 条（dump 的 S10 门漏抄原测试的 optional 对照块，不影响 golden 语料）+ note 3 条，均不阻塞。可打 `rust-port-w4` tag。

---

## (a) 工具只读性 — PASS

- 通读 tools/golden-dump.ts 全文 2017 行：imports 仅 `node:crypto/node:fs/node:path/node:url` + src/*（content/dict/dice/clock/parse/query/engine/fold/reach/render/play/types/ref/sentence）+ test/*（settle/reach-fuzz-lib/snap/seed/explorer-lib）——全部是**值导入**；写面仅 `mkdirSync(outDir)` 与 `writeFileSync(join(outDir, …))`（`--out` 可改向，默认 rust/tests/goldens，属 §7 规则 10 产物豁免）；fold-prefix 两模式只 `readFileSync` + stdout，零写。
- `git diff --stat rust-port-w3..HEAD -- src/ test/ scripts/ examples/` → **空** ✓。W4 的 TS 触面 = tools/golden-dump.ts 一个新文件，Rust 触面 = rust/{src/core/{sentence,parse,fn_expand,format}, src/engine/engine.rs, src/session/game.rs, tests/{golden.rs, common/{mod.rs, reach_fuzz_lib.rs}}, tests/goldens/**} + docs/rust-port/messages.md 一行——无越界。

## (b) golden 生成忠实性 — PASS（附 should-fix-1 / note-1）

**S1–S14 场景构造与断言复算**：逐段对照 reach-adversarial.test.ts——内容 rig（verbs/prototypes/decks/triggers/seed）、初始实体、analyzer verdict/existence/notes 断言、engine 逐步走线（mkBegin 裸 BEGIN、runner 的 `nextId += n+1` 记账、manual 定 id 步）、explore 边界（maxSteps/maxStates 逐条同参）全部一致；断言复算门（PR1-02/PR3-08）在 emit 前全过、失败即 fail() 中止（`check()` → `golden-dump: assertion failed`），s01–s14/s06-plain/s13-low/s13-high 共 16 条 scenario goldens 与原测试的走线逐值对应（含 S2 expiresAt===30、S4 `'object' takes 2..2, got 1`、S8 永冻、S11 窗几何 `[[FORGE_LEFT,0,30],[FORGE_RIGHT,30,90]]`、S13 rng 0/1 计数冻结）。shared dict 的累积注册序（S1→S14 依次 isVerb/registerVerb）被 dump 与 golden.rs 双侧镜像，golden.rs 的 s_dict() 一次性注册全 21 动词的等价性论证（无跨场景动词词素）成立。verdict/引擎状态口径按验证 03 勘误：复算的是 verdict/状态断言而非骰序 ✓。
- **should-fix-1**：dump 的 S10 块（:996-1035）**漏抄原测试的 optional 对照块**（reach-adversarial.test.ts:789-809：min:0 with-slot 的 bootstrap 分子 → GOAL obtainable + fwOpt 见证，两条分析器侧断言）。该块不产引擎记录，对 golden 语料零影响，但"复算原 S1–S14 expect"的门在此差两条。建议下次工具触面补上（或登记豁免）。

**四个排序锚点场景命中性**（读 engine.ts 对应行确认分支语义，再读 golden 产物确认踩中）：
- `anchor-due-tie`：两个 30min 窗同为 [0,30]、beginId 1/2 → engine.ts:1073 全序 `(end, beginId, clause)` 的第二比较项；golden JSONL 实测 complete 序 PACE_A(1) → PACE_B(2) ✓。姊妹场景 `anchor-due-tie-clause` 单步双子句 [0,30]/[30,30] 踩第三比较项（clause 0 先于 1）✓。
- `anchor-expiry-insertion-order`：ZOMBIE_CARD 先于 APPLE_CARD 插入（非名字序）、同 expiresAt=30 → engine.ts:1065 的 `[…state.entities.values()]` 快照序 + :1238 的 `for (const exp of expiring)`；golden 实测 trigger 序 `expire:ZOMBIE_CARD → expire:APPLE_CARD → wait` ✓。
- `anchor-on-expire-first-match`：同一名字两条 authored 声明 + 无 authored 的 DUST 共存 → engine.ts:1252 的 `content.triggers.find(…)` 首条匹配 + :1265 内建 DESTROY #id；golden 实测 `decay_first → expire:DUST → wait`，且 decay 行带 authored 文案、DUST 行带 `DESTROY #dust` ✓。
- `anchor-refuse-id-recycle`：REFUSE 丢弃 advance pass → engine.ts:257-262（`makeBeginRecord(opts.firstRecordId…)` + advancedMin=0 + state=stateIn）与 :1070；golden 实测记录 id 序 `1, 2, 2, 2, 3`——拒绝行 id=2 两次复用 + 真推进以 2,3 关窗，foldUpTo 的 `<` 依赖面被显式钉住 ✓。
其余 15 个锚点（complete-before-slot-event / advance-chronological / expiry-hoist-no-cascade / wait-partition / window-completion-failure / first-ending / roll-last-wins / roll-per-clause / advance-extension / flash-zero-duration / station-locks / overlapping-windows / expiry-locked-drop / slot-cadence-once）与 §3.14 矩阵行号一一对应，rig 与 rust/tests/engine_ordering.rs test-for-test 镜像。

**fuzz 动作选择器与 LCG 逐算子对照**：
- LCG：TS `s=(seed^0x9e3779b9)>>>0`、`((s=Math.imul(s,1664525)+1013904223)>>>0)/0x100000000` ↔ Rust `Lcg::new` XOR 0x9e37_79b9、`wrapping_mul(1664525).wrapping_add(1013904223)`（u32 wrapping ≡ imul+>>>0 的 mod-2³²）、`next_f64 = f64/2³²`（幂二除法位精确）——逐算子等价。
- 选择器：TS `chooseStep = steps[Math.floor(r()*steps.length)]` ↔ Rust `choose_step = steps[r.next_int(len)]`，`next_int = floor(next_f64*n)` 同式；工具内嵌 `SELECTOR_SOURCE` 与入库 rust/tests/goldens/action-selector.ts **逐字节相同**（python 比对 True），与 reach_fuzz_lib.rs 的 `choose_step` 行为一致。
- 调用纪律：dump fuzzFamily ↔ drive_fuzz_seed（:820-855）——attempts 先自增、每 attempt 对全候选表抽一次、被拒 consume 抽签即 continue、K=25/cap=400、id 记账 `id += records.len()`——一致。candidates 候选空间（waits [0,1,2,12,24,48]、effSlots last-def-wins、dup→#id、combos cap 8/opts slice 6、multi-clause 配对）与 reach-fuzz-lib.ts:294-397 逐行同构。
- **经验证**：200/200 fuzz goldens 字节重放通过本身就是 genScenario 抽签序逐位一致的强证明（任何一处漂移都会改变 raw/initial 而炸字节比对）。

**walk / nightjar / demo 家族**：walk 循环（'NIGH' 裸态 LCG、0.2 采样门、`floor(r()*n)%n`、ending-undo `turnLengths` 截断、`executed%20` 收割）与 explorer-lib.ts:275-306 逐行一致（dump 侧多捕获 records 属生成所需）；nightjar 三条走 autoSettleLine ≡ nightjar.test.ts:18-32 的 fresh() 适配器（handleLine→windows>0→wait），nightjar-dream 的 PORTENT×2/VOID×1/exhausted 断言前置复算 ✓；demo 与 examples/demo.ts 六步 + `x${id}_${++n}` idGen（Rust 侧 `id_now` 按步捕获 + 跨步共享 `n`，闭包语义等价）✓。

## (c) IntoTarget 修复正确性 — PASS

- TS 原文形状（sentence.ts:29）：`{ t: 'into'; ref: Ref | { role: Role } }`——`ref` 的值**就是** Ref 对象本体或 `{role}`。修复后 Rust `IntoTarget::Ref(Ref)`（untagged newtype）序列化为裸 Ref（`{"t":"into","ref":{"form":…}}`）；原 struct 变体会产 `{"ref":{"ref":…}}` 双包——修复方向与终形均正确，且 demo golden record 3 是真实的第一分歧探测器（commit 473e11a 记录属实）。
- **未波及其他 Arg 变体——全量 serde 形状审查**：`Arg` 13 变体逐一对照 sentence.ts:20-39：`ref`（tag `t`=`ref` + 内层 `ref` 字段）、`role`、`int/delta/dur`（i64 ≡ TS 整数域，PR2-04 口径）、`str/bool`、`key`、`asId`、`pair`（k/v，v 走 `AttrValue` untagged 裸原生值 ≡ `string|number|boolean`）、`vexpr`/`param`（authored-only、`sign` Option-skip ≡ undefined 丢弃）——线形全部不变。`Ref` 三变体（`form` 内部 tag、Name 的 `kind` skip、Param）未动；`Outcome` 的 `{op,args}` 序 + arms/fallback/call skip 未动。内部 tag `t` 前置 + 声明序字段 ≡ TS 构造字面量序。全波 Rust src 触面 diff 逐行过目：parse/fn_expand/format/engine/game 的改动**纯构造点适配**（`IntoTarget::Ref(x)` 新型），无逻辑变化。
- 该修复的可观察后果由 golden harness 承重：244 条逐字节通过 = 修复后线形与 TS 全语料一致（含所有 `into` 出现的记录）。

## (d) 重放 harness 判据 — PASS

- **真的逐字节**：`assert_bytes_eq` 是纯 `==` 字符串比较（golden 文件原文 vs `to_jsonl(records)`），失败报告首分歧行号 + 两侧原文（§6.2 第 1 步形态，且有专测 `assert_bytes_eq_reports_the_first_divergent_line` 钉报告器本身）；**非 sha**（manifest 的 sha256 显式只读不验，PR2-09 无 sha2 依赖）、**非归一化**（无任何 trim/排序/键序归一）。空日志 = 单个 `\n` 字节的 saveLog 形也精确复刻（to_jsonl join+push('\n')）。
- **双判据都在**：① play 侧同 driver 从头重放 → JSONL 字节相等；② fold 侧 `fold(load_log(golden 文件))` over driver base → snap == manifest snap。加上侧栏第三层：steps/journal/prose/suggestions 四类 sidecar 逐字节（拒绝文本不进 JSONL，steps sidecar 是其唯一跨语言钉）。
- **244 条全覆盖**：manifest goldenCount=244（fuzz 200 + anchor 19 + scenario 16 + walk 5 + script 3 + demo 1，与 dump 六家族产出严格相等）；六个 `check_family(kind)` 按 kind 遍历 manifest 全条目（`checked>0` 地板），每条走 record-count + bytes + fold-snap + sidecars 四重断言；`every_golden_has_a_driver` 对 244 条逐一 driver_for——无孤儿、无遗漏。
- tReal 恒 null（engine.ts:27/317 确定性口径）在两侧 driver 均成立。

## (e) 亲测 — PASS

- `cargo test -j 4`（reviewer 亲跑，rust/ 下，无 vitest/tsx 并发）：**545 passed / 0 failed**。分套计数：lib 79 / clock 12 / common_smoke 9 / conditionals 36 / decks 46 / dice 49 / endings 25 / engine 74 / engine_ordering 17 / fn_expand_direct 9 / functions 27 / gate 2 / **golden 8** / lint 14 / nightjar 21 / nightjar_explorer 4 / parallel 13 / query_predicate 25 / script_play 29 / sentence 18 / session_smoke 7 / stackable 21 / doctests 0 —— 合计恰 545，与声明一致；golden.rs 3.99s（fuzz 200 种子在其中）。
- **黄金重生成**（cargo 结束后单独跑）：`npx tsx tools/golden-dump.ts --out /tmp/golden-regen-XXXX` → `goldens: 244 ({script:3,walk:5,scenario:16,anchor:19,fuzz:200,demo:1}), jsonl bytes total: 5498805`；`diff -r` 对入库 rust/tests/goldens **IDENTICAL（exit 0，零差异）**——黄金语料无隐藏非确定性，manifest 的 sha/bytes 记账与实文件一致。临时目录已删除。

## (f) engine_ordering HAND 替换 — PASS

- `grep -c HAND` → **0**；`#[test]` 计 17 不变；模块头改写为 "PR1-01 (closed)" 并声明 dump-measured 来源。
- 全波 diff（69+/52−）逐行过目：**纯注释/消息串改写**——仅有的两处断言行变化是 panic 消息串（"HAND: …" → "golden anchor-refuse-id-recycle: …"），断言值 2/2 不变；其余全部是 HAND 注释 → `golden: <场景名> (W4 dump 实测…)` 溯源注释。没有任何期望值被放松或改动——"hand values survived the cross-language replay unchanged" 的 commit 声明与 diff 相符。
- 抽查 4 个场景名（任务点名四个）：anchor-due-tie / anchor-expiry-insertion-order / anchor-on-expire-first-match / anchor-refuse-id-recycle **全部在 manifest anchors 列表**，golden.rs 的 anchor_driver 逐一有同参 driver（rig/步序/firstRecordId 与 engine_ordering.rs 镜像）；其余 15 名也逐一在位（含换行拆写的 anchor-window-completion-failure :666）。

## (g) 有意分歧 5 条核对

| # | 分歧声明 | 核对结论 |
|---|---------|---------|
| ① | 2^53−1 哨兵（golden 的 EGG expiresAt=9007199254740991） | **成立**。dump:1367-1368 与 golden.rs:1653-1654 双侧同值带注释；2^53−1 在 TS f64 与 Rust i64 中均精确可表示（JSONL 序列化两侧同串）；engine_ordering.rs:473/486 诚实登记该 rig 自身用 i64::MAX−1 的夹具差异（同为"永不过期"哨兵，golden 重放面用的是 2^53−1，跨语言证据不受影响） |
| ② | walk 收成 5（explorer-lib 说"up to ten"） | **成立**。dump walkFamily 注释明示"measured at 5 in this build (walk 20..100)"，`check(specs.length >= 1)` 只保底；manifest walk 条目 = 20/40/60/80/100；Rust walk_driver 镜像同循环（500 上限）并在够不到 harvest 时 panic——545 绿即两侧落在同一确定性收成 |
| ③ | fuzz 软锁种子 | **成立**。dump :1836-1839 注释声明"soft-locked world 合法耗尽 attempt cap"；实测 200 种子中 50 个 applied<25（如 seed 63 applied 0、seed 21 applied 4，全部 attempts=400）；applied/attempts 对确定性并经 steps sidecar（`applied N attempts 400`）在两侧逐字节钉住 |
| ④ | messages 行 11 结论 | **成立**。行 11 的 W4 增补写明：244 条 golden 均良构、无一触发 drift 文案；golden 差分钉的是 wait 行 fold 分区纪律本身（每条 fold-snap 相等），文案锚留给后续日志漂移测试——结论与 harness 实况一致（所有 golden fold 过、无 drift throw） |
| ⑤ | station-locks 连续性修正 | **成立**。f4c5f2d 的修正：r5 从 **r4.state** 继续，golden 日志是**一局连续棋**（两分歧分支拼的 log fold 不回其 snap，law 1）；en-route 锁释放语义不变（advance 解析 3:0/1:0、放 MAGICK BOOK、TOUCH 才开）；dump:1604-1610 与 golden.rs:1878-1882 注释一致，且 harness 的 fold-snap 判据对该 golden 实际通过（经验证实） |

## 附带核对（W3 评审遗留项清账）

- W3 should-fix-1/2（messages.md 行 115/118 的 11/10 矛盾、行 142 的 engine.ts:1785→:819）：**已清**（现树行 118/142 均带勘误注记）。
- W3 should-fix-3（Phase-2 validate_slots 参数 ref 的错误通道）：**已按建议处理**——engine.rs:383-395 落通道分歧注释（TS 双通道 vs Rust 单 Result 的合并声明 + 四条不可达论证）。

## 问题分级

**must-fix：0 条。**

**should-fix：1 条（不阻塞收口）**

1. golden-dump.ts 的 S10 复算门漏抄原测试的 optional 对照块（reach-adversarial.test.ts:789-809：min:0 with-slot bootstrap → `GOAL obtainable` + `fwOpt.everNames.has('GOAL')` 两条分析器侧断言）。对 golden 语料与重放判据零影响（该块不产引擎记录），但 PR1-02/PR3-08"复算原 expect"的门在 S10 少两条——下次工具触面补两行 check，或在工具头注释登记豁免。

**note：3 条**

1. golden.rs 的 s11 driver 丢掉了 dump 侧 `RIGHT completed at 90` 的中间态断言（`c.advance(…,"0:3")` 返回值未检）——该性质已被 JSONL 字节比对（RIGHT 的 complete 行在语料中）完全子sumed，仅对称性差异。
2. golden.rs 的 nightjar_main 将 WIN_PATH 九行硬编码而非复用 `common::explorer_lib::WIN_PATH`（suggestions_driver 用的是后者）——两处今日逐字相同；WIN_PATH 是冻结测试夹具，但单一事实源更稳，顺手可合。
3. walk_driver 的 Rust 循环条件只有 `i < 500`，无 dump 的 `specs.length < 10` 双条件——对本波 harvest（≤100）行为恒等（TS 侧 500 轮内从未达 10 条），仅形状差异，记录备查。

**结论：PASS-with-notes——可打 `rust-port-w4` tag。** should-fix-1 建议随 W5 首个工具触面 commit 顺手清账；全部不阻塞。
