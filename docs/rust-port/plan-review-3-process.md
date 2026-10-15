# plan.md 评审（视角三：施工波次、依赖顺序与验收策略）

评审对象：`docs/rust-port/plan.md`（plan v1）。依据：verification/01–04.md + worktree 源码抽查（只读，未改任何源码、未运行构建/测试）。

## 总评：需修订后批准

计划的语义对齐规范（§3）、怪癖冻结（§1.2）、差分策略（§5.4/§6.2）质量很高，波次主干（W0→W1→W2→W3→W4→W6，W5 开发并行/验收滞后）方向正确。但**验收策略在四处与源码事实冲突**：(1) W1 的测试移植清单含 functions.test，而该文件 import 了 content/engine/store/settle（W2/W3 才存在），W1 的"全绿"门按现文不可达成；(2) nightjar.test（21 个 test，P0）与 script-play.test 的 Game/lenient 部分未列入任何波次；(3) W4 的 Rust 侧 fuzz 重放所需的 reach_fuzz_lib.rs 未落位到任何波次；(4) §7 没有 per-wave review 门与 reviewer 清单，而本计划大量关键行为恰好**无测试钉住**（lint 1 基、bare inspect、BTreeMap 陷阱类），只靠测试绿不构成放行依据。这四处均为 must-fix，修订量小（清单搬家 + 补一节规则），不影响总体架构。

对任务书各问的要点结论（详见逐条）：
- **W1 内部顺序**：范围列举顺序（types→…→lexer→…→predicate→dice→clock）按字面读有 3 处依赖倒置（should-fix PR3-05）；单 crate 编译不会失败，但与 §3"实现顺序即清单序"的纪律和 subagent 认领相抵触。
- **W2 依赖 W1**：成立且计划已声明主干（fn_expand 接缝类型）；隐含还需 dict（coreDictionary/assertNameable/isVerb）、sentence/roles、js.rs 的 jsjson+ordinary_own_property_keys（W1 已列"jsjson 完成"，成立）。
- **W3 与 W6**：无实质重叠/返工——Game 被 7 个测试文件锚定必须在 W3，W6 只是薄 dispatcher（note PR3-09）。
- **W4 dump 脚本**：计划明确"只新增 tools/golden-dump.ts、不改 src/、不与 cargo 并发"，§7 规则 10 一致；缺口在 S1–S14 转写的"vitest 校验"步骤不可执行（should-fix PR3-08）。
- **W5 依赖**：计划声明 content/engine API 正确，但漏了 tests/common 基建（settle.rs 必需，explorer/scale/fuzz lib 未落位，见 PR3-03）。

---

### PR3-01 [must-fix] functions.test.ts 不可能在 W1 移植：其依赖在 W2/W3 才存在（§4-W1"测试移植"、§4-W1"验收"）

实证：test/functions.test.ts:9–20 import 了 `content/content.js`（compileContent）、`content/script.js`（compileScript）、`engine/engine.js`（executeStep）、`engine/fold.js`、`engine/store.js`（loadLog/saveLog）、`./snap.js`、`./settle.js`；正文大量经 compileScript→executeStep→settleStep 驱动（:86–87、:464、:518）。这些分别属 W2（content）、W3（engine/store/settle/snap）。W1 只有 core，"上述 4 个测试文件移植版全绿"与 W1 验收里"宏展开 call site === 手工展开孪生全过"（该孪生测试就在 functions.test 里，经 compileScript 加载展开）**按现文不可执行**——第一波就会卡在波次门上。
修订建议：把 functions.test 移到 W3（其 27 个 test 全部依赖编译与引擎）；W1 的宏展开验收改为不经 compileScript 的直测子集（手工构造 FnTable 调 collectFnCalls/expandFnCalls/format），并把这一替代路径写明。W1 测试清单相应改为 sentence/clock/query-predicate 3 个文件。

### PR3-02 [must-fix] nightjar.test 与 script-play.test 的 Game/lenient 部分未分配波次（§4-W3 测试移植清单、§5.3 P0）

§5.3 把 nightjar 列入 P0（"不通过不得宣称行为一致"），但 §4-W3 的移植清单是 dice 剩余/engine(74)/parallel(13)/decks 剩余/stackable(21)/conditionals(36)/endings(25)/nightjar-explorer(4)——**没有 nightjar.test**（实测 21 个 test，覆盖 lenient 路由、拒答文案、once-trigger、timetable、win path 等远多于 W4 golden 重放的骰序/牌序三点）。W4 验收只覆盖 nightjar.test:153/163/347–348 的值，不能替代移植该文件。同理，§4-W2 明言 script-play.test 只移植"编译器子集（不含 Game 部分，Game 在 W3）"，但 W3 清单未列 script-play 剩余（lenient input 两个 describe + game loop describe，:246–421）——W3 验收虽引用了 script-play:384/401–407/412 的断言，但清单与验收不同步。
修订建议：§4-W3 清单补"nightjar.test（21）、script-play.test 剩余（lenient + game loop）"；建议 §5.2 加一张"vitest 文件 → 波次 → Rust 文件"完整映射表（25 个测试文件逐一落位），消除"部分移植"（dice/decks/script-play 跨波）的二义性。

### PR3-03 [must-fix] reach_fuzz_lib.rs / reach_scale_lib.rs 未落位，W4/W5 的验收门缺执行前提（§2.2、§4-W4、§4-W5）

§2.2 把 test/reach-fuzz-lib.ts、reach-scale-lib.ts 映射到 tests/common/，但 §4 没有任何波次认领：W3 只建 explorer_lib.rs + snap/settle/seed；W4 的"fuzz 种子批 seed 1..200、每种子 K=25 settleStep"（§4-W4 第 4 条）要求 **Rust 侧**重放同一 driver——即 genScenario + 测试侧 LCG + settle 等价物（reach_fuzz_lib.rs 的内容）必须在 W4 前移植，否则"200 个 fuzz 种子 ×25 步零字节差"这一 W4 验收无法执行；W5 的 reach-fuzz.test（100 种子）与"reach-scale 同参世界 TS/Rust 各建一次"同样需要这两个 lib。§4-W5 的前置只写"W3（content/engine API）；W2 的 CompiledContent"，未含 tests/common 基建依赖。
修订建议：W4 范围显式加"移植 tests/common/reach_fuzz_lib.rs（genScenario/LCG 动作选择器，与 golden-dump.ts 内的固定动作选择器同源）"；W5 范围显式加"移植 reach_scale_lib.rs（buildCsScale 等价物）"，并把 W5 前置补"tests/common/settle.rs（W3）"。

### PR3-04 [must-fix] §7 缺 per-wave review 门与 reviewer 检查清单（§7 全节、§5.1）

§7 规则 3 只有一句"评审即按此对照"，没有定义：reviewer 角色、每波 tag 前是否强制 review、检查什么。这不是流程奢侈——本计划的一等风险恰好是**无测试钉住的行为**：lint 1 基文案（Q1，验证 02 明言"无测试钉住"）、bare inspect（Q11，"无测试锚定"）、functions.test.ts:138 的罗列序"恰为字母序是巧合"（BTreeMap 化测试不会红，验证 01 附加发现）、snap 排序细节（TEST-13 只有三处锚点）、错误消息无断言覆盖的分支（loadLog 带行号等"记录为有意分歧"项）。"每波 cargo test 全绿"对这些零约束力，review 是唯一防线。
修订建议：§7 增补一条"波次收口 review 门"（tag 前执行，产出一段 review 记录），检查清单至少含：(a) 按 `// ts:` 行锚对本波新移植文件与 TS 原文逐段对照（core/engine 全量，其他抽样）；(b) 迭代序审计：grep 本波文件的 `HashMap`/`HashSet`/`BTreeMap`，逐一核对是否处于序可观察点（对照 §3.7）；(c) §1.2 QUIRK 注释在本波范围模块内逐条在位；(d) docs/rust-port/messages.md 增量行数 = 本波移植 toThrow 数；(e) grep 门禁（rand::、panic 点 unwrap/expect）通过；(f) `git diff --stat` 证实 src/、test/、scripts/ 零改动；(g) "有意分歧"登记表（loadLog 行号、REPL 新代码等）逐条有据。

### PR3-05 [should-fix] W1 范围列举顺序含依赖倒置（§4-W1 标题行）

列举序"types/roles/dict/ref/sentence/lexer/parse/fn_expand/format/query/predicate/dice-接线/clock"按字面读有三处倒置：types 引 `Role`（sentence）与 `Ref`（ref），却排在 roles/ref 之前；sentence 引 `Pred/ValExpr`（predicate，sentence.ts:4），却排在 predicate 之前；lexer 调 `parseDuration`（clock.ts，lexer.ts:3），却排在末位的 clock 之前。单 crate 内模块序不影响编译，但 §3 声明"实现顺序即清单序"、且波内任务要拆给不同 subagent（见 PR3-07），字面照读会做无用返工。
修订建议：显式给出 W1 内部移植序：roles→clock→types→ref→query→predicate（类型先行的部分）→sentence→lexer→parse/fn_expand→format→dice 接线；或加一句"W1 范围列表无顺序含义，按上序移植"。

### PR3-06 [should-fix] "每波全绿"与"core 域 toThrow"定义不精确（§5.1、§4-W1 验收）

(a) §5.1"每波 cargo test 全绿才进下一波"未写明"全绿"= **迄今已移植的测试**全绿（未移植的 TS 测试不在 Rust 侧存在，不算红）；P0 全集（§5.3）是终局门而非每波门。这句话本身不与"W1 时 engine 未移植"矛盾，但建议一句话写死，避免执行者误以为要在 W1 补齐 P0。(b) W1 验收"192 条中 core 域的 toThrow 逐条过"的"core 域"无定义：按验证 04 的分布（dice 34、decks 30、conditionals 29、functions 25、engine 8），core 模块的错误消息大多被 **decks/conditionals/functions** 的断言钉住，而这三文件在 W3（functions 见 PR3-01）。建议改为"W1 所移 3 个测试文件内的 toThrow 逐条过 + messages.md 增补对应行"。

### PR3-07 [should-fix] 多 subagent 并行的构建/提交纪律缺失（§2.4、§7 规则 5/9）

§2.4/规则 5 只约束了单进程维度（-j 4、禁 vitest×cargo 并发、双 cargo 不并行），没有回答"同仓多 subagent"问题：多个 agent 同时在 .worktrees/rust-port 里改码、各自跑 cargo、各自 commit。cargo 的 target 目录文件锁会把并发 cargo 命令串行化（安全但空等），真正的风险是**并发编辑同文件冲突与波次门的归属**（谁宣布"本波全绿"）。16GB 下并行编译两个 rustc 链也不现实。
修订建议：§7 增补：波内允许多 subagent 按模块认领（认领表注明文件边界，互不重叠）；**cargo 构建/测试全局串行**，由 wave integrator 一人执行并宣布波次门；commit 仍按规则 9 的模块粒度、由认领者提交各自文件（不重叠则无冲突）。§4 各波的建议子任务切分见 PR3-11。

### PR3-08 [should-fix] W4 场景转写的校验方法不可执行（§4-W4 第 3 条）

"若 reach-adversarial.test.ts 的场景构造未导出，则……转写为等价脚本文本入 goldens/scenarios/*.txt，**并用 vitest 里既有断言校验转写无误**"——既有 vitest 测试不会去跑新转写的脚本文本；若为此向 test/ 加临时测试文件则违反 §7 规则 10（TS 侧零修改，唯一例外是 tools/golden-dump.ts）。这一步目前没有合法载体。
修订建议：把转写校验并入 golden-dump.ts 本身：工具内对每个转写场景复算原测试的骰序/事件序断言（断言逻辑抄自 reach-adversarial.test.ts，工具只读 import），全部通过才写 goldens/。同时明确 goldens/scenarios/*.txt 属规则 10 的"goldens/ 产物"豁免。

### PR3-09 [note] W3 提前 lenient/render/Game 不与 W6 重叠；但 W6 验收含手工冒烟，不可机检（§4-W3、§4-W6）

Game 被 7 个测试文件锚定（验证 04 UI-01），必须随 W3 落地，W3→W6 无返工面；W6 的 bin/chorus.rs 是新代码（§1.3 明确不入冻结面），范围干净。唯一问题：W6 验收"REPL 手工冒烟 = demo 脚本 6 步 + 胜路输入行"依赖人工判定，与全计划"可机检验收"的风格不符。建议改为脚本化：管道喂入固定输入行序列，stdout 与 examples/demo.ts 的 journal/prose 黄金输出 diff（该黄金可在 W4 一并生成，属 golden 产物）。

### PR3-10 [note] §6.2 二分协议基本可操作，两点补强（§6.2）

五步（diff 首分歧行→前缀 fold 对拍→按模块插桩→输入最小化→回退规则）方向正确、与 commit/tag 粒度（规则 9）配合可 git bisect。补强：(a) 步骤 2 的"TS/Rust 各 fold 得 State 对拍"需要 TS 侧有任意日志前缀的 fold+snap dump 能力——现定义的 golden-dump.ts 只有 4 类固定 driver，建议把"fold-prefix 模式"预先做进该工具（仍是只读工具，同一豁免）；(b) 建议规定 Rust 侧插桩 dump 的形态（临时分支或 env 门控、修复后删除、不入库）与失败报告必备字段（seed、步号、首个分歧 record id/kind），便于跨 agent 协作排查。

### PR3-11 [note] 各波子任务切分建议（§4；配合 PR3-07 的串行构建纪律）

16GB 内存下**编辑可并行、构建必须串行**；以下切分均以"一个 integrator 持有 cargo"为前提，T 编号即建议认领粒度：
- **W0**（1–2 个任务）：T0.1 crate 脚手架 + Cargo.toml profile + 门禁雏形；T0.2 js.rs（cmp_utf16/LCG/ChorusError）+ messages.md 建立；T0.3 core/dice.rs + core/deck_rng.rs + 全部向量测试（波次门：T0.3 全绿）。
- **W1**（按 PR3-05 序切 4）：T1.1 roles+clock+types（State/IndexMap 纪律落型）；T1.2 ref+dict+query；T1.3 predicate+sentence（predicate 类型先定，sentence 后接）；T1.4 lexer+parse+fn_expand+format；T1.5 js.rs 完成（jsjson 对拍 Node）+ 测试基建（assert_err_msg/deep-equal/canonical）+ 3 个测试文件（functions.test 移出，见 PR3-01）。T1.1→T1.2→T1.3→T1.4 强串行，T1.5 部分可并行。
- **W2**（3）：T2.1 parseScript 行段解析器 + meta/seed + lintScript；T2.2 compileContent（fn_expand 接线、recipe 铸造、decks、stackable 三集合一、OwnPropKeys 接入）；T2.3 lint.test + script-play 编译器子集 + nightjar include_str 冒烟。T2.1/T2.2 可两人先后并行，T2.3 收口。
- **W3**（最大波，切 6–7）：T3.1 store.rs serde + Log；T3.2 fold.rs；T3.3 engine.rs 主干（executeStep/applyOutcome/applyRecord）；T3.4 resolveDue/fireWindow/fireSlot 顺序机制 + §3.14 排序测试；T3.5 tests/common（snap/settle/seed/explorer_lib）；T3.6 lenient.rs+render.rs+game.rs；T3.7 测试移植分三包：A=engine+parallel+functions（W3 起）、B=decks 剩余+dice 剩余+stackable+conditionals、C=endings+nightjar+nightjar-explorer+script-play 剩余（依 PR3-02 补入）。T3.3→T3.4 串行；T3.5 完成后 T3.7A/B 与 T3.6 可交错。
- **W4**（2+1）：T4.1 golden-dump.ts + goldens 生成 + manifest（纯 TS 侧，与 cargo 天然串行错开）；T4.2 Rust 重放 harness + reach_fuzz_lib.rs（PR3-03）；T4.3 失败二分循环（按 §6.2）。T4.1/T4.2 可并行开发。
- **W5**（4）：T5.1 census+world（Analyzer<'c>/WorldFacts）；T5.2 fixpoint（朴素 round-robin，禁优化）；T5.3 paths+render；T5.4 四个 bin + reach_scale_lib.rs + reach.test/reach-adversarial/reach-fuzz/reach-scale 测试移植。T5.1→T5.2→T5.3 强依赖链；T5.4 的 fuzz/scale 差分部分排在 W4 验收后（与计划一致）。
- **W6**（1–2）：T6.1 bin/chorus.rs + 脚本化 REPL 冒烟（PR3-09）；T6.2 demo.rs + prelude/messages.md/grep 门禁收口 + README 怪癖附录。

## 结论

**需修订后批准**。must-fix 4 条（PR3-01–04），均为清单/规则层面的修订（测试文件搬家、基建落位、补 review 门），不需动架构。修订完成后计划即可进入执行。
