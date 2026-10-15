# 04 验证 — 报告 A（04-ui-examples.md）与报告 B（05-tests-docs.md）

验证方法：对每条 finding/关键声明，独立打开引用的 file:line 核对证据；重点逐字核对
`src/core/dice.ts` 的 splitmix64/rollDie、`src/content/content.ts` 的 hashDeckId/permutePositions/drawFrom、
`test/` 下 Game 的 import 与方法依赖（grep 全目录）、toThrow 数量级（192 处/13 文件）。
全程只读，未运行构建/测试，未修改任何源码。

---

## 汇总表 — 报告 A（04-ui-examples.md，UI-01…UI-15）

| ID | 结论 | 一句话说明 |
|---|---|---|
| UI-01 | PARTIAL | 结论成立且更强（Game 被 7 个测试文件锚定，不可丢弃），但"9 个测试文件"错误——实际恰好 7 个，且"等"字多余（所列即全集） |
| UI-02 | VERIFIED | play.ts:60 与 engine.ts:154-155 两条 game-over 路径、两版文案逐字属实；endings.test 同时钉住两版文案（:378 与 :186/:219） |
| UI-03 | VERIFIED | tui.tsx:374 与 457-459 两份 inspect 路由、BUTTONS(:23)、runPlain 不支持按钮/undo/wait/story 全部属实 |
| UI-04 | PARTIAL | 结论（picker 标题恒 undefined、应改用 meta.title）成立，但脚本行号错误：`script:` 在 the-nightjar-flyer.txt:47，不是 :2（:2 是注释 `#`） |
| UI-05 | VERIFIED | card()（render.ts:118-135）与 Log.journal()（store.ts:19-22，报告写 20-22，差 1 行）全仓库零调用者，均属实 |
| UI-06 | VERIFIED | play.ts:133-136 + tui.tsx:374 空 `(.*)` → startsWith('') 恒真返回首卡，属实；且无任何测试钉住 bare-inspect 行为（补充） |
| UI-07 | VERIFIED | lenient.ts:193 `Math.min(4, ...)` 无注释魔法数，逐字属实 |
| UI-08 | VERIFIED | 四文件的行数、import 清单（render 1-9 / lenient 6-14 / play 4-16 / tui 6-10）、同步性、tui 行数划分全部属实 |
| UI-09 | VERIFIED | 四个怪癖的全部行号（418-419、127-137/158-175/271-274、250、300/435）逐一属实 |
| UI-10 | VERIFIED | play.ts:184-192 的 ceil 量化（:189）与 now+60（:188）、lenient.ts:59-66 lone-clock `clauses: []` 分支全部属实 |
| UI-11 | VERIFIED | lenient.ts:37 正则逐字一致、:46 原始 parser message、:5 文件头示例全部属实 |
| UI-12 | VERIFIED | play.ts:210/204-205/249-282/284-289 属实；endings.test.ts:356、script-play.test.ts:384(visible)/412(suggestions) 三处行号精确命中 |
| UI-13 | VERIFIED | demo.ts:27-38 的 id 记账（:38）、idGen（:33）、fold-verify（:49-53）、store.ts:11-13 nextRecordId 全部属实 |
| UI-14 | PARTIAL | 五处行号引用全部正确，但实际共 **5 处**（play 3 + tui 2），不是"6 处"；提炼 helper 的建议本身成立 |
| UI-15 | VERIFIED | play.ts:73 的 `« … »` 拼装与 tui.tsx:62-68 quoteLine 的三个反向解析操作（`/^«\s*/`、`/\d+:\d+/`、`split(' | ')`）逐字属实 |

## 汇总表 — 报告 B（05-tests-docs.md，TEST-01…TEST-15）

| ID | 结论 | 一句话说明 |
|---|---|---|
| TEST-01 | PARTIAL | 8 处 run() 的 file:line 全部精确命中；但漂移描述**说反了**：idGen 闭包存在于 6/8 个版本（dice/decks/conditionals/functions/endings/stackable + engine 的 runWith），只有 parallel 版**不带**——"以 parallel 版为最全蓝本"不成立，合并版需 parallel 的 bare-clock 分支 + settleStep 系的 idGen |
| TEST-02 | VERIFIED | sentence.test.ts:11-17 与 clock.test.ts:12（:11 注释自认 mirroring）属实 |
| TEST-03 | VERIFIED | 四处 ent()（reach.test.ts:13、fuzz-lib 67-76、adversarial 30-39、scale-lib 122-124）与字段清单逐字属实 |
| TEST-04 | VERIFIED | 三处 LCG（fuzz-lib:35-38、scale-lib:28-31、explorer-lib:276-277）与三种种子约定（0x9e3779b9 / 0xc5 / 'NIGH'=0x4e494748）逐字属实 |
| TEST-05 | VERIFIED | 零 snapshot/mock/fake-timer、test.fails 仅注释、四处引用正则（dice:315、engine:98/261、dice:384）全部精确命中；toThrow 全仓 192 处/13 文件，"错误消息即契约"量级成立 |
| TEST-06 | VERIFIED | nightjar.test.ts:153 `[[5,2],[8,4],[14,7]]`、:163 `['SCANDAL','RUMOUR']`、:346-348 VOID/PORTENT、dice:161-173、decks:141-144 全部逐字属实 |
| TEST-07 | VERIFIED | sig() 定义于 fuzz-lib:246，rng 参与签名（`g: s.rng` :253，警告注释 :250-252，报告写 :252-254，差 1-2 行）、beginId 排除注释 :268-271 属实 |
| TEST-08 | VERIFIED | 六处 rig（dice:65-79、endings:23-29、engine:387-395/554-562/742-754、parallel:34-38）形状与行号全部属实 |
| TEST-09 | VERIFIED | reach-adversarial.test.ts:12 注释属实；全文件 `fails(` 零调用属实 |
| TEST-10 | PARTIAL | dice:30-33 与 adversarial:45-48 精确命中；但 parallel 的 dict 在 :16、registerVerb 在 :29（:33 是注释行）；模块级共享字典模式与 reserved-word 风险（lint.test.ts:36-39）属实 |
| TEST-11 | VERIFIED | settleStep 定义于 settle.ts:15-34、`Math.max(...windows.end)` 恰在 :25；act/done/refusal 实际 40-67（报告写 36-61，refusal 收尾略超出）——实质属实 |
| TEST-12 | VERIFIED | 两个 grep gate（dice:935-943、decks:831-838）行号与内容精确属实；Rust 侧换 CI gate 的建议合理 |
| TEST-13 | VERIFIED | snap.ts:9-21 的排序规则（`a<b?-1:1`、fired `.sort()`、windows 浅拷贝排序）逐字属实 |
| TEST-14 | VERIFIED | 五处引用（dice:181、engine:1002-1003/1007-1008、dice:547-554、content.ts:476）全部精确命中 |
| TEST-15 | VERIFIED | test/ 零处 import ui/tui 属实（grep 0 命中）；import 面核实：ui 侧 play=7 文件、lenient=4、render=3 |

---

## 报告 A 逐条验证

### UI-01 — Game 是会话核心，不可当作 UI 丢弃（重点核实项）

**原声明**：`test/` 下 9 个文件 import `Game`（decks/dice/endings/nightjar/nightjar-explorer/script-play/stackable 等）；概述称"9 个测试文件直接驱动它"。

**核实过程**：
- `grep -rn "from '../src/ui/play.js'" test/` → 恰好 **7** 个文件：decks、dice、endings、nightjar、nightjar-explorer、script-play、stackable。报告所列 7 个名字即全集，"等"字多余，"9"错误。
- 方法级依赖（grep 每个方法在 test/ 的调用点，已排除 Log 同名方法）：
  - `handleLine`：全部 7 文件
  - `undo`：6 文件（nightjar/endings/decks/script-play/nightjar-explorer/dice）
  - `wait`：6 文件（同上，含 stackable、不含 nightjar-explorer）
  - `game.journal()`：6 文件 11 处（nightjar:354、endings:343、decks:491、script-play:371、nightjar-explorer:323、dice:663/694/700/701/706/715）
  - `story()`：3 文件（endings:344、script-play:372、nightjar-explorer:324）
  - `look()`：2（script-play:373、nightjar-explorer:321）
  - `cards()`：2（stackable:175、nightjar-explorer:322）
  - `inspect()`：2（script-play:401/406/407、stackable:176）
  - `suggestions()`：2（endings:356、script-play:412）；`visible()`：1（script-play:384）
- 行号引用核实：play.ts:56-74 handleLine、161-167 undo、:44 turnLengths、82-107 visible、199-291 suggestions、:2 注释、src/index.ts:21 导出——全部命中。

**结论**：PARTIAL（计数错误；核心结论成立且被加强）。

**修正表述**：Game 被 **7** 个测试文件 import（全集，无"等"）；除 handleLine/undo/wait 外，journal(6 文件)/story(3)/look(2)/cards(2)/inspect(2)/suggestions(2)/visible(1) 均有测试直接断言。

**移植建议**：**must** —— Game 整体归入引擎/会话层移植；handleLine、undo+turnLengths、wait 的 ceil 量化、journal/story/look/cards/inspect/suggestions/visible 全部不可丢，除非同步重写上述测试。

### UI-02 — game-over 双实现、双文案

**原声明**：play.ts:60 UI 短路（endingNotice 全文案）；engine.ts:154-155 引擎 rejected（无 flavour）；endings.test 断言了 UI 文案。

**核实过程**：play.ts:60、engine.ts:154-156、render.ts:22 逐字核对一致。补充发现：endings.test.ts **同时**钉住两版——:378 `endingNotice(...) .toBe('the story has ended — No More (melancholy).')`（UI 版），:186/:219 `rejected .toMatch(/the story has ended — No More/)`（引擎版）。

**结论**：VERIFIED。**修正表述**：不是"只钉了 UI 版"，两版文案各自都有测试锚点——二选一去重时无论保哪版都要动另一处测试。

**移植建议**：**must** —— 保留两条路径的各自文案与通道（UI info notice / 引擎 rejected），原样移植；不要合并。

### UI-03 — 命令路由两份且分歧

**核实过程**：tui.tsx:374 与 457-459 两处 `/^inspect\b\s*(.*)$/` 属实；BUTTONS 于 :23；App.submit(368-381) 有数字按钮路由（runButton 349-366），runPlain(450-469) 只有 inspect + handleLine；输入 `2`/`undo` 在 runPlain 下确实落入 lenient 的 "no verb found" 分支（lenient.ts:78-85）。

**结论**：VERIFIED。**移植建议**：**should** —— Rust REPL 单一 dispatcher；无测试依赖（tui 零测试引用）。

### UI-04 — examples/play.ts 标题解析漂移

**核实过程**：examples/play.ts:21 的 `/^title:\s*(.+)$/m` 属实；script.ts:105-108 的 meta 正则 `^(script|player|goal|start|seed):` 与 `m[1] === 'script'` → `meta.title` 逐字属实；`grep title scripts/the-nightjar-flyer.txt` 零命中 → title 恒 undefined 成立。**但** `script: Murder on the Nightjar Flyer` 在脚本 **第 47 行**，不是报告所写 :2（:2 是 `#`）。

**结论**：PARTIAL（行号错，结论对）。

**移植建议**：**must**（结论部分）—— Rust 版标题一律取 `compileScript` 的 `meta.title`，删除平行解析；引用行号按本报告更正。

### UI-05 — card() 与 Log.journal() 死代码

**核实过程**：全仓 grep `\bcard(`（排除 render.ts 定义处）→ 仅 test/explorer-lib.ts:139 一条**注释**；`.journal(` 在 src/ 的唯一命中是 tui.tsx:364 的 `game.journal()`（Game 自己的方法，非 Log 的）；test/ 中 11 处 `.journal(` 全部是 `game.journal()`。store.ts 实际行号 19-22（报告写 20-22）。

**结论**：VERIFIED。**移植建议**：**should** —— 两者不带；Game.journal()/render.journal 保留。

### UI-06 — bare-inspect 显示首卡

**核实过程**：play.ts:133-136、tui.tsx:374 属实；`startsWith('')` 恒真、`find` 返回首个可见卡。补充：grep 未发现任何测试调 `game.inspect('')` 或经 handleLine 输入裸 `inspect`，该行为**无测试锚定**。

**结论**：VERIFIED。**移植建议**：**should** —— 若保留卡片视图需文档化；丢弃则随之消失且不破坏测试（但 inspect 本身有测试，见可简化清单的驳回项）。

### UI-07 — Math.min(4, …) 魔法数

**核实过程**：lenient.ts:193 `for (let len = Math.min(4, toks.length - j); len >= 1; len--)`，无注释。结论 VERIFIED。**移植建议**：**should** —— Rust 加常量名（如 `MAX_NAME_WORDS = 4`）+ 注释。

### UI-08 — 纯净度分层

**核实过程**：render.ts 135 行 / lenient.ts 248 行 / play.ts 292 行 / tui.tsx 469 行（报告写 470，含末换行）✓；render.ts:1-9、lenient.ts:6-14 import 全为相对类型或 core 纯函数 ✓；play.ts:4-16 import 即 API 清单 ✓；tui.tsx:6-10 是唯一 react/ink/node 依赖 ✓；store.ts:3 有 node:fs 而 Log 类不触 fs ✓；游戏路径全同步（play.ts 无 async）✓；异步只在 pickScript(298-319)/runTui(434-447)/runPlain 的 for await(456) ✓。

**结论**：VERIFIED。**移植建议**：**must** —— 分层结论直接作为移植边界。

### UI-09 — 三个终端怪癖

**核实过程**：418-419 deferred-wrap 注释、127-137 runChunk、158-175 runTabs、271-274 ScriptPicker 的 `\r` 处理、250 UTF-16 光标注释、300/435 两处 isTTY 判定——全部逐字属实。结论 VERIFIED。**移植建议**：**ignore**（最小 REPL）/ **should**（若做富 TUI）。

### UI-10 — wait 的 ceil 量化

**核实过程**：play.ts:184-192（ceil 于 :189、now+60 于 :188）；lenient.ts:59-66 lone-clock 返回 `clauses: []`。结论 VERIFIED。**移植建议**：**must**。

### UI-11 — canonical 逃生舱判定

**核实过程**：lenient.ts:37 `/^\d+:\d+\s*@/.test(trimmed) || trimmed.includes('->')`、:46 `(err as Error).message`、:5 文件头示例——逐字属实。结论 VERIFIED。**移植建议**：**must**。

### UI-12 — suggestions() 内嵌启发式与测试锚点

**核实过程**：play.ts:210（REFUSE 墙）、204-205（busy 排除）、268-282（round-robin + `mixed.length < 64`）、284-289（可解析性过滤）全部属实；test 引用三处行号**精确命中**：endings.test.ts:356（suggestions 为空）、script-play.test.ts:384（visible）、:412（suggestions）。

**结论**：VERIFIED。**移植建议**：**should** —— 若降级 suggestions/visible，必须同步改写 **test/endings.test.ts** 与 **test/script-play.test.ts**（报告已正确标出）。

### UI-13 — demo 的 id 记账复制

**核实过程**：demo.ts:27-38（id 记账、`id += r.records.length` 在 :38、idGen 在 :33）、fold-verify :49-53、store.ts:11-13 nextRecordId——全部属实。结论 VERIFIED。**移植建议**：**should** —— demo 作为移植回归样例保留，记账用 nextRecordId 简化。

### UI-14 — 找玩家惯用法重复

**核实过程**：`grep "meta.player" src/ui` + find 形态过滤，命中恰 **5** 处：play.ts:84、:111、:202、tui.tsx:38、:53。报告引用的 5 个行号全对，但"6 处"错误（若把 lenient.ts:206 的 playerRefs 算入才是 6，而那超出报告自列的范围）。

**结论**：PARTIAL（计数 6→5）。**移植建议**：**should** —— 提炼 helper；如要凑第 6 处，把 lenient.ts:206 一并纳入。

### UI-15 — `« … »` 组装后正则拆解

**核实过程**：play.ts:73 拼装格式与 tui.tsx:62-68 的 `replace(/^«\s*/,'')`、`match(/\d+:\d+/)`、`split(' | ')` 逐字属实。结论 VERIFIED。**移植建议**：**should** —— 结构化 Turn（附 sentences 数组），展示层自行格式化。

---

### 报告 A 三个清单的逐项核对（与测试依赖的冲突检查）

**必须保留表**：逐项核实通过——lenient 全部具名函数存在且为纯函数（norm:180、aliveNames:165、resolveGroup:185、playerRefs:204、suggest:212、suggestVerb:224、levenshtein:235；with→to/actors 路由 127-147；lone-clock 59-66；passthrough 37-48）；render.ts:11-114 的具名函数全部存在（journal:11、endingBanner:16、endingNotice:21、pretty:26、flattenAspects:31、proseLines:45、prose:85、interpolateClause:92、interpolateRecord:102、joinAnd:112）；"错误消息文案被测试断言"成立（全仓 192 处 toThrow + refusal/toMatch 断言；endings.test.ts:378 逐字钉 endingNotice 文案）。无冲突。

**可简化表**：
- 命令路由（tui.tsx:368-381, 450-468）：无测试依赖，可合并。✓
- **`inspect` 行——有冲突**："丢弃卡片视图可整体去掉"未标注测试影响：`inspect()` 被 **script-play.test.ts:401/406/407** 与 **stackable.test.ts:176** 直接断言。丢弃须同步改写这 2 个测试文件（bare-inspect 行为本身无测试，UI-06 部分不受影响）。
- **`look`/`cards`/`story`/`journal` 行——有冲突**："保留 story（或 journal）一个即可"未标注测试影响：story 被 3 文件（endings:344、script-play:372、nightjar-explorer:324）、**journal 被 6 文件 11 处**、look 被 2 文件（script-play:373、nightjar-explorer:321）、cards 被 2 文件（stackable:175、nightjar-explorer:322）断言。缩减视图必须同步改写 **endings、script-play、nightjar-explorer、stackable、decks、dice、nightjar** 中的相应断言。
- suggestions()/visible()：报告已正确标注测试同步要求（UI-12，行号精确）。✓
- processing()（play.ts:171-178）：唯一调用者 tui.tsx:44，无测试依赖。✓

**可丢弃表**：逐项核实**全部成立**——tui.tsx 全部组件/runPlain/quoteLine/三个怪癖：test/ 零处 import ui/tui；render.card() 死代码；examples/play.ts 的 listScripts/title 正则不被任何测试 import。此清单无一条与测试冲突。

---

## 报告 B 逐条验证

### TEST-01 — run() 八连抄与漂移

**原声明**：8 文件各自定义 run()；"engine 版带 idGen 闭包、parallel 版额外处理裸时钟纯推进（:21-23）、其余不带"；建议以 parallel 版为最全蓝本。

**核实过程**：8 处定义行号**全部精确命中**（conditionals:34、decks:33、dice:35、endings:44、functions:28、engine:28+32、parallel:19、stackable:88）；parallel 的 bare-clock 分支恰在 :21-23 ✓。但逐个读函数体：**dice、decks、conditionals、functions、endings、stackable 六个版本的 settleStep 调用都带 idGen 闭包**（`` `x${firstRecordId}_${++n}` ``），engine 版经 runWith(:32) 也带——"其余不带"为假。漂移的真实维度是：parallel 带裸时钟分支而**唯一不带 idGen**；stackable 用 `poolDict` 而非 `dict`；engine 用 runWith 间接层。因此"parallel 版是最全蓝本"不成立——它缺 idGen，没有任何单一版本是超集。

**结论**：PARTIAL（行号全对，漂移描述反转）。

**修正表述**：8 个 run() 中 7 个带 idGen（engine 经 runWith），唯一不带的是 parallel；parallel 的独有分支是裸时钟纯推进（:21-23），stackable 的独有差异是 poolDict。合并后的 Rust 唯一 helper 应为：**parallel 的裸时钟分支 + settleStep 系的 idGen 通道 + 可注入字典**。

**移植建议**：**should** —— "只建一个 run 助手"的结论保留，蓝本描述按上句修正。

### TEST-02 — canonical() 双胞胎
sentence.test.ts:11-17、clock.test.ts:11-12（:11 注释"mirroring sentence.test.ts"）属实。VERIFIED。**must**（进共享 lib；这是 §5.2 文法契约的自检）。

### TEST-03 — ent() 四连抄
四处定义（reach.test.ts:13、fuzz-lib:67-76、adversarial:30-39、scale-lib:122-124）与 `{id,kind,name,aspects,attrs,bornAction,deadAction,stackable}` 字段清单逐字属实。VERIFIED。**should**。

### TEST-04 — LCG 三处复制
fuzz-lib:35-38、scale-lib:28-31、explorer-lib:276-277 逐字属实；三种种子入口（`^0x9e3779b9` / `lcg(0xc5)`@:33 / `0x4e494748='NIGH'`）属实。VERIFIED。**must**（若做差分）——LCG 逐位移植（`u32` wrapping mul/add 即可）并保留三个种子入口。

### TEST-05 — vitest 耦合面与错误消息契约
全 test/ 25 文件 10,351 行、460 个 test()、零 toMatchSnapshot/vi.mock/useFakeTimers、test.fails 仅注释——全部精确属实。四处引用正则（dice:315、engine:98、engine:261、dice:384）逐字命中。补充量化：**toThrow 共 192 处，分布于 13 个文件**（dice 34、decks 30、conditionals 29、functions 25、engine 仅 8——engine 的消息多经 refusal()/toMatch 断言）。VERIFIED。**must** —— 错误消息文本逐字移植（采纳报告的"建议前者"）。

### TEST-06 — seed-1899 黄金 trace
nightjar.test.ts:153/163 精确属实；:346 为注释、实际断言在 :347-348（`byName.get('PORTENT')` 长 2、`VOID` 长 1）——实质属实。seed 1899 确在脚本元行（the-nightjar-flyer.txt:51 `seed: 1899`）。dice:161-173 的 [14,3]/[11,3]/[12,3] 与 decks:141-144 四置换向量属实。VERIFIED。**must** —— P0 验收集核心。

### TEST-07 — sig() 字段清单
sig 定义于 fuzz-lib:246；`g: s.rng`（:253）+ "collapsing … silently drop a subtree" 注释（:250-252，报告写 :252-254，差 1-2 行）；beginId/clause 排除注释（:268-270，代码 :271）属实；与 snap.ts 字段清单互相印证属实。VERIFIED。**should**。

### TEST-08 — rig 字面量重复
六处引用全部命中（dice:65-79 inspectContent、endings:23-29 waitContent、engine:387-395 probeContent、:554-562 counterContent/TUNE、:742-754 decayContent/DECAY、parallel:34 geo），形状与声明一致。VERIFIED。**ignore/should**。

### TEST-09 — test.fails 幽灵注释
reach-adversarial.test.ts:12 注释属实，全文件零 `fails(` 调用。VERIFIED。**ignore** —— 不要为它留设计。

### TEST-10 — 模块级共享字典
dice:30-33、adversarial:45-48 精确命中；**parallel 的 dict 实际在 :16、registerVerb 在 :29**（报告写 :33，该行是注释）。实质模式（每文件一个模块级 dict + 代码注册 rig 动词）属实；reserved-word 风险真实（lint.test.ts:36-39 的 `'TO' is a reserved word` 断言）。VERIFIED except 行号 → PARTIAL（仅 parallel 一处引用偏移）。**should** —— Rust 每测试新建字典（与 TS 每文件隔离等价）。

### TEST-11 — settle.ts 易漏
settleStep 定义于 :15-34 ✓、`Math.max(...windows.end)` 恰在 :25 ✓、act(:40-42)/done(:48-50)/refusal(:57-67) —— 报告写 :36-61，refusal 收尾超出 6 行（范围小偏差，实质属实）；"它不在 src 里、移植清单容易漏"的警告成立。VERIFIED。**must** —— settle.ts/snap.ts/seed.ts 明确列入移植基建清单。

### TEST-12 — grep gate 换形态
dice:935-943、decks:831-838 逐行属实（readdirSync 遍历 src 断言无 `Math.random`；实测 src/ 零命中）。VERIFIED。**should** —— Rust CI grep gate + dice 路径禁浮点。

### TEST-13 — snap 键序
snap.ts:9-21 逐字属实（三处 `([a],[b]) => a<b?-1:1`、fired `.sort()`、windows 排序浅拷贝）。"跨实现比对优先用 JSONL 日志字节"的建议合理（record 形状简单）。VERIFIED。**must**（Rust 内部 fold 等价用 snap）/ **should**（跨实现用日志）。

### TEST-14 — 错误消息对照表
五处引用全部精确命中（dice:181、engine:1002-1003、:1007-1008、dice:547-554、content.ts:476）。VERIFIED。**must** —— 与 TEST-05 同；按文件整理"消息→测试"对照表。

### TEST-15 — 测试→src import 面
grep 证实：test/ 零处 import ui/tui；ui 侧 import 为 play=7 文件、lenient=4（explorer-lib/nightjar/script-play/stackable）、render=3（endings/engine/nightjar-explorer）；core/content/engine/reach 均有 import。"Game 的 headless handleLine 是现成对接口"成立（7 文件使用）。VERIFIED。**must**（headless 接口保留）。

---

## 报告 B 章节级勘误（非 finding，但计划引用时需更正）

1. **§四 优先级表**：`parallel.test.ts（310 / 14）` → 实际 **310 行 / 13 个 test()**（其余 19 行的行数/测试数全部精确核实无误；总量 25 文件 / 10,351 行 / 460 test() 亦精确）。
2. **§2.4**：`store.ts:22-25 saveLog` → 实际 **24-27**（逐 record JSON.stringify、按行拼接的描述属实）。
3. **§五**：spec"python-computed vector table"短语在 **:1576**（报告写 :1571-1574；同一段落、span 末差 2 行）；spec 总行数 1,914 ✓。
4. **§一**："reach 系共 ~1,900 行" → 四个 reach 测试文件实际 591+1017+25+89 = **1,722 行**（"约"在容差内，若含 fuzz/scale lib 则远超）。
5. 其余文档引用抽查全部属实：README:16-19（No backward compatibility）、:23-33（布局）、:44-65（play 截例）；plans/v0.4-cultist-mechanics.md:26-31（snap 合并史）、:324（Settled by fiat）；v0.5-parallel-verbs.md:13-35（六条 laws）、:121-127（§5 `(endMin, beginRecordId)`）、:83/172（C14 与 §7 验证记录）；reverse doc 的 1,566/2,084/67（:389/:40/:155）与 §6(:324)/§8(:416)。

## 两个专项声明的裁决（任务指定重点）

**① splitmix64 与 dice.ts 逐字一致性：确认一致。**
dice.ts:19-22 定义 MASK64/GOLDEN_GAMMA/MIX1/MIX2；:26-31 finalizer 为 `z=(x+γ)&M; z=((z^(z>>30))*M1)&M; z=((z^(z>>27))*M2)&M; return (z^(z>>31))&M`——与报告 B 表格逐字一致，含"每步 & (2^64-1)"与"finalizer 单次应用、非完整 splitmix64 序列"的定性。rollDie（:38-42）：`BigInt.asUintN(64, BigInt(seed)) ^ BigInt.asUintN(64, BigInt(counter))`（负数二补码，:39）+ `(BigInt(sides) * r) >> 64n`——报告描述与 Rust `u128` widening mul / `as u64` 转换建议语义严格对应。测试锚点（dice:85-89 三原始值、:94-113 恰 **27** 个向量元组、decks:141-144 四置换）全部实测存在。

**② decks 的 Fisher-Yates：确认一致。**
content.ts:453-461 `permutePositions`：位置 i（自 n-1 降至 1）取 `j = Number(splitmix64(asUintN(seed) ^ hashDeckId ^ asUintN(i)) % BigInt(i+1))`，即报告的 `splitmix64(seed ⊕ hash(deckId) ⊕ i) mod (i+1)`；hashDeckId（:430-437）为 FNV-1a-64（offset `0xcbf29ce484222325`、prime `0x100000001b3`、TextEncoder UTF-8 字节）；drawFrom（:472-477）与耗尽错误（:476）属实；seed 默认 0（:293）、seed 存于 content（:169）属实。

**③ toThrow 数量级**：报告未给出总数；实测 **192 处 toThrow / 13 个文件**（另有无尽数量的 refusal()/toMatch 消息断言），"错误消息遍布全部 P0/P1 文件、文本即契约"的定性成立。

**④ 报告 A 清单 vs 测试依赖**：可丢弃表 6 项全部安全；可简化表中 **inspect 行与 look/cards/story/journal 行漏标测试影响**（详见上文清单核对），须按本报告补充受影响测试文件后再执行。

---

## 计划必须吸收（must）

1. **Game 不可丢弃**：7 个测试文件（decks/dice/endings/nightjar/nightjar-explorer/script-play/stackable）直接驱动 handleLine/undo/wait/journal/story/look/cards/inspect/suggestions/visible；Game 归入引擎/会话层整体移植（UI-01，计数按 7 更正）。
2. **RNG 逐字移植**：splitmix64 finalizer 常量与移位（dice.ts:19-31）、rollDie 的 asUintN 二补码 XOR + 64 位高位乘（:38-42，Rust 用 u128 widening mul）、permutePositions 的 FNV-1a-64 ⊕ key + mod(i+1)（content.ts:430-461）、drawFrom 回绕/默认卡/耗尽拒绝（:472-477）。
3. **向量表是第一验收物**：dice.test.ts:85-89（3 值）+ :94-113（27 行）+ decks.test.ts:141-144（4 置换），语言无关，Rust 直接照抄。
4. **错误消息文本是契约**：192 处 toThrow/13 文件 + refusal/toMatch 断言；逐字移植消息文本（TEST-05/14），并按文件整理"消息→测试"对照表。
5. **nightjar seed-1899 黄金 trace**（:153/:163/:347-348）+ 双跑 JSONL 字节恒等判据（dice:917-933）作为端到端验收。
6. **game-over 双路径双文案都保留**：UI notice（endings:378 钉全文案）与引擎 rejected（:186/:219 钉短文案）。
7. **test 侧基建随 P0 移植**：settle.ts（:15-34，Math.max 推进规则 :25）、snap.ts（:7-23，键序规则）、seed.ts、store 的 record serde；这三件不在 src/，最易漏（TEST-11/13）。
8. **grep gate 换形态**：Rust CI 禁 rand/浮点参与 dice 路径（TEST-12）。
9. **差分骨架**：LCG（常数 1664525/1013904223、三种种子入口）、genScenario/runFuzzSeed/sig 字段清单（TEST-04/07）。
10. **headless 接口**：保留 handleLine 等价物服务黄金日志差分（TEST-15）。

## 应驳回（或按修正后采纳）

1. **驳回原表述**：UI-01"9 个测试文件 import Game（……等）"→ 实为 **7 个，所列即全集**（结论方向不变、证据更强）。
2. **驳回原表述**：UI-04"the-nightjar-flyer.txt:2 用的正是 script: …"→ 实际在 **:47**（:2 是注释）；结论（title 恒 undefined）不变。
3. **驳回原表述**：UI-14"重复 6 处"→ 实为 **5 处**（play 3 + tui 2；含 lenient.ts:206 才是 6）。
4. **驳回子项**：报告 A 可简化表"inspect：丢弃卡片视图可整体去掉"——`inspect()` 被 script-play.test.ts:401/406/407 与 stackable.test.ts:176 断言，**丢弃必须同步改写这 2 个测试文件**，原句未标注此条件。
5. **驳回子项**：报告 A 可简化表"look/cards/story/journal 保留 story（或 journal）一个即可"——四个视图分别被 6/3/2/2 个测试文件断言（journal 11 处），**缩减必须同步改写 endings、script-play、nightjar-explorer、stackable、decks、dice、nightjar 中的相应断言**，原句"均为只读投影"不能推出"可自由缩减"。
6. **驳回子项**：TEST-01"engine 版带 idGen 闭包、其余不带；以 parallel 版为最全蓝本"→ 实为 **7/8 带 idGen、唯一不带的是 parallel**；正确蓝本 = parallel 的裸时钟分支 + settleStep 系的 idGen + stackable 的可注入字典。
7. **更正引用**：TEST-10 的 parallel.test.ts:33 → :16/:29；报告 B §四 parallel 测试数 14 → **13**；store.ts saveLog :22-25 → :24-27；spec python 短语 :1571-1574 → :1576；sig rng 注释 :252-254 → :250-253；settle act/done/refusal :36-61 → :40-67。

## 总量统计

- 报告 A：**VERIFIED 12 / PARTIAL 3（UI-01、UI-04、UI-14）/ REFUTED 0**；三个清单中"可丢弃"表全部成立，"可简化"表 2 个子项需补测试同步条件。
- 报告 B：**VERIFIED 13 / PARTIAL 2（TEST-01、TEST-10）/ REFUTED 0**；另 5 处章节级引用行号/计数勘误（无一对结论造成实质影响）。
- 两报告的所有 RNG 算法描述与实现**逐字一致**，可作为移植规格直接使用。
