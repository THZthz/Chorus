# 04 — UI 与 examples 移植分析

范围：`src/ui/play.ts`、`src/ui/render.ts`、`src/ui/lenient.ts`、`src/ui/tui.tsx`、`examples/demo.ts`、`examples/play.ts`。只读分析，无任何源码改动。

## UI 架构概述

- 分层（自底向上）：`render.ts`（纯函数：ActionRecord → 叙事文本）→ `lenient.ts`（纯函数：玩家英语 → canonical `Step`）→ `play.ts` 的 `Game` 类（会话状态机，无任何 react/ink/node 导入）→ `tui.tsx`（Ink 组件 + readline fallback，唯一的 react/ink/node 依赖点）。
- 事件流：stdin 一行 →（tui `Prompt` 行编辑/历史/tab 补全）→ `App.submit` → `Game.handleLine` → `parsePlayerInput`（lenient）→ `executeStep`（引擎）→ `log.appendAll` + `state` 替换 → `proseLines` 渲染 → `Entry` → ink `<Static>` 永久滚回区 + 底部 live 区。
- UI 层消费的引擎 API 面：`compileScript`、`executeStep(content, dict, state, step, { firstRecordId })`、`Log`（`records`/`nextRecordId`/`appendAll`）、`fold`、`cloneState`、`parseSentence`、`matchQuery`、`evalPred`、`clockFromAbsMin`/`formatClock`、`Dictionary.isVerb/verbs`（play.ts:4-16 的 import 即完整清单）。
- `Game.handleLine` 是整个回合循环，注释自述"testable without a TTY"（play.ts:2）；9 个测试文件直接驱动它，它是事实上的公共 API（`src/index.ts:21` 导出）。
- `Game` 里混着三类东西：回合记账（handleLine/undo/wait）、引擎领域的投影（`visible()` 可见性、`suggestions()` 动作枚举）、纯展示组装（story/journal/cards）。三者命运不同（见文末清单）。
- 引擎是规则唯一权威：拒绝、REFUSE 草稿回滚、§9.35 game-over、§9.38 oldest-name 全在 engine/fold；`Game` 只做 turnLengths 记账与 fold 重放。
- `render.ts` 与 `lenient.ts` 均**确认纯净**：import 只有相对路径类型与纯 core 函数，无 react/ink/node:fs/process；全部同步、无 Promise。可直接逐函数移植。
- `tui.tsx`（470 行）约 300 行是 ink/终端专属（Prompt 行编辑、ScriptPicker、App 布局、render/Instance 生命周期），约 90 行是游戏逻辑展示（`verbList`/`statusOf`/`hereOf`/`quoteLine`/`runButton`/`submit`）；`play.ts` 292 行中 **0 行** react/ink 专属。
- 入口：`examples/play.ts`（CLI：读脚本文件 → `Game.fromScript` → `runTui`，无参时 ScriptPicker 列出 `scripts/*.txt`）；`examples/demo.ts`（无头：6 个 canonical 步骤 → journal + prose + fold 校验，是移植正确性的现成回归样例）。
- 关键结论：**"最小 line REPL" 已经以 `runPlain`（tui.tsx:450-469）形态半存在**；Rust 版应以其为骨架补齐元命令，而不是移植 ink 部分。

## Findings

### UI-01 [类别: modularity] [严重度: high]
`Game` 类虽住在 `src/ui/`，实为游戏会话核心，不可当作"UI 可简化"处理。
`play.ts:23-292`：`handleLine`（56-74）是回合循环；`undo`（161-167）实现 spec §7 的 state=fold(surviving log) 语义；`turnLengths`（44）是 undo 截断点记账。`visible()`（82-107）是"玩家能看到什么"的**规则**投影（log 命名传播、INTO 排除、容器链），`suggestions()`（199-291）是 90 行的 verb×card 绑定枚举器，直接调 `evalPred`/`matchQuery`。`test/` 下 9 个文件 import `Game`（decks/dice/endings/nightjar/nightjar-explorer/script-play/stackable 等）。移植时 `Game` 应归入引擎/会话层；"TUI 允许简化"**不**授权丢弃它——否则 undo 语义与全部行为测试失去锚点。

### UI-02 [类别: semantics] [严重度: medium]
§9.35 game-over 检查在 UI 与引擎各实现一次，且两条路径的用户可见文案不同。
`play.ts:60`：`if (this.state.ended) return { kind: 'info', lines: [endingNotice(...)] }`，文案为 `the story has ended — TITLE (flavour).`（render.ts:22）；`src/engine/engine.ts:154-155`：`if (stateIn.ended) return rejected(\`the story has ended — ${stateIn.ended.title}\`, ...)`，无 flavour、且走 error 通道。UI 短路使引擎的 rejected 文案在正常游玩中不可达。移植时须二选一作为权威（建议保留 UI 层 notice 的完整文案，因为 endings.test.ts 断言了它），并记录该重复。

### UI-03 [类别: duplication] [严重度: medium]
命令路由（`inspect` 前缀、数字按钮）实现了两份且行为分歧。
`tui.tsx:374-378`（`App.submit` 的 `/^inspect\b\s*(.*)$/`）与 `tui.tsx:457-459`（`runPlain` 里同样的正则）。分歧：`submit`（368-381）还支持数字按钮（look/cards/story/journal/undo/wait/quit，BUTTONS 定义于 23 行），`runPlain` 不支持——非 TTY 下输入 `2` 会直落 `handleLine` 被 lenient 报 "no verb found"；`runPlain` 也不支持 undo/wait/story。Rust REPL 应把路由收敛到一处（Game 方法或 REPL dispatcher），避免移植两份分歧逻辑。

### UI-04 [类别: duplication] [严重度: medium]
`examples/play.ts` 的标题解析与编译器实际指令漂移——真实 bug。
`examples/play.ts:21`：`/^title:\s*(.+)$/m` 从原始文本抓标题；但编译器识别的 meta 指令是 `script|player|goal|start|seed`，标题指令是 `script:`（`src/content/script.ts:105-108`：`if (m[1] === 'script') meta.title = m[2].trim()`）。唯一打包脚本 `scripts/the-nightjar-flyer.txt:2` 用的正是 `script: Murder on the Nightjar Flyer`——picker 永远显示不出标题（`title` 恒为 undefined）。Rust 版应复用编译产物的 `meta.title`，删掉这份平行解析。

### UI-05 [类别: dead-code] [严重度: low]
`render.ts:118-135` 的 `card()`（box-drawing 卡片渲染）全仓库无调用者（grep 仅命中定义处；`Game.cards()` 用的是自己的单行格式，play.ts:125-128）。另外 `Log.journal()`（`src/engine/store.ts:20-22`）也无调用者——`Game.journal()` 用的是 `render.journal`（play.ts:155-157）。移植时两者可不带。

### UI-06 [类别: clarity] [严重度: low]
裸 `inspect`（无参数）会静默显示第一张可见卡的牌面。
`play.ts:133-136`：`q = ''` 时精确匹配失败、回退 `startsWith('')` 恒真 → 返回 `table[0]`；`tui.tsx:374` 的正则允许空捕获（`(.*)`）。有卡片视图的版本若保留此行为需写成文档；无卡片视图则随 inspect 一起消失。

### UI-07 [类别: clarity] [严重度: low]
`lenient.ts:193`：`resolveGroup` 的多词名字匹配上限 `Math.min(4, ...)` 是无注释魔法数（名字最长按 4 词尝试）。移植时若改名解析策略需显式记录该上限；建议在 Rust 版加常量名与注释。

### UI-08 [类别: portability] [严重度: medium]
纯净度分层（移植成本评估）：
- `render.ts`（135 行）、`lenient.ts`（248 行）：**纯函数、全同步、零平台依赖**，import 全为相对路径类型或 core 纯函数（render.ts:1-9, lenient.ts:6-14）。可机械移植。
- `play.ts`（292 行）：无 react/ink 导入；唯一间接平台依赖是 `engine/store.js` 的 `Log`（store.ts 顶部有 `node:fs`，但 `Log` 类本身不触 fs）。逻辑本身可直接移植。
- `tui.tsx`（470 行）：唯一 react/ink/node 依赖点（tui.tsx:6-10：react hooks、ink render/Static/useInput/useWindowSize、`node:readline/promises`、`node:process` stdin/stdout）。约 300 行不可移植，约 90 行游戏展示逻辑（`verbList` 26-28、`statusOf` 35-48、`hereOf` 51-58、`quoteLine` 62-68、`runButton`/`submit` 349-381）随 TUI 简化一并重写。
- 异步语义：**游戏路径完全同步**（handleLine/wait/undo 无 Promise）；异步只存在于 tui 的 `runTui`/`pickScript` Promise 包装（tui.tsx:298-319, 434-447）与 `runPlain` 的 `for await`（450-468）。Rust 版核心可做成纯同步函数，REPL 外壳再包 IO。

### UI-09 [类别: portability] [严重度: low]
`tui.tsx` 记录了三个终端怪癖，仅当保留富 TUI 时相关，最小 REPL 可整体丢弃：deferred-wrap——永不写终端最后一列，否则 ink 行计数失步（tui.tsx:418-419）；ink 不拆分 coalesced 块中的 `\r`/`\t`，需自行按行/按 tab 切分提交（tui.tsx:127-137, 158-175, 271-274）；光标是 UTF-16 索引、星形字符会劈开（tui.tsx:250）。非 TTY 判定 `!stdin.isTTY || !stdout.isTTY`（tui.tsx:300, 435）在 Rust 版对应 `atty`/`IsTerminal`。

### UI-10 [类别: semantics] [严重度: low]
`wait()` 的半时量化 round-up 是必须保真的语义。
`play.ts:184-192`：目标时刻 `Math.ceil(dueAt / 30) * 30`（有窗口取最早 end，无窗口 `now + 60`），再借 `handleLine("d:hh")` 复用 lenient 的 lone-clock 纯推进分支（lenient.ts:59-66，`clauses: []` 的 Step 让引擎解析所有到期窗口）。移植若丢掉 ceil 或 lone-clock 分支，窗口完成时刻与 `1:12` 手输行为都会漂移。

### UI-11 [类别: semantics] [严重度: low]
canonical 逃生舱的触发条件是宽松子串：`/^\d+:\d+\s*@/` **或** `trimmed.includes('->')`（lenient.ts:37）。玩家句子任意位置含 `->` 即整行走严格 `parseSentence`（错误消息风格也随之变为 parser 的原始 message，lenient.ts:46）。这是有意的直通设计（文件头示例 lenient.ts:5），但移植时需原样保留判定，否则 canonical passthrough 行为改变。

### UI-12 [类别: semantics] [严重度: low]
`suggestions()` 内嵌游戏设计启发式，非引擎正确性：带 REFUSE outcome 的 recipe `when` 视为"墙"不是"邀请"（play.ts:210）；busyVerbs/busyCards 排除（204-205）；story/flavor 两桶 + round-robin 交错 + 64 条上限（249-282）；最终每条建议还要过一遍 `parsePlayerInput` 可解析性过滤（284-289）。注意 `test/endings.test.ts:356` 与 `test/script-play.test.ts:384,412` 断言了 `suggestions()`/`visible()`——若 Rust 版丢弃此功能，须同步删除/改写这些测试，且测试中的行为断言（结束后建议为空等）本身就是语义记录。

### UI-13 [类别: duplication] [严重度: low]
`examples/demo.ts:27-38` 手工维护 `id += r.records.length` 复制了 `Log.nextRecordId`（store.ts:11-13）的记账；同时演示了 `ExecOptions` 的两条 id 通道并存（数字 `firstRecordId` 与字符串 `idGen: () => \`x${id}_${++n}\``）。demo 作为移植回归样例很有价值（尤其 49-53 行的 fold-verify），Rust 版可直接用 nextRecordId 简化。

### UI-14 [类别: duplication] [严重度: low]
"找玩家实体"惯用法重复 6 处：`play.ts:84, 111-112, 202`（`alive.find(e => e.name === this.meta.player)`）与 `tui.tsx:38, 53`。Rust 版应提为 `fn player_entity(&self) -> Option<&Entity>` 一个 helper。

### UI-15 [类别: clarity] [严重度: low]
`« … »` canonical 回显是"组装后再正则拆解"的脆弱耦合：`Game.handleLine` 在 play.ts:73 把回显拼进 `Turn.lines`（`« ` + sentences joined by ` | ` + ` »`），tui 的 `quoteLine`（tui.tsx:62-68）再用 `/^«\s*/`、`/\d+:\d+/`、`split(' | ')` 反向解析来折叠超长行。移植时若保留回显，建议把结构化数据（sentences 数组）放进 `Turn`，展示层自行格式化。

## TUI 简化清单（最小 line-based REPL：读一行 → 推进 → 打印）

### 必须保留（行为语义，逐行移植）
| 项 | 位置 | 理由 |
|---|---|---|
| `parsePlayerInput` 及全部辅助（`norm`/`aliveNames`/`resolveGroup`/`playerRefs`/`suggest`/`suggestVerb`/`levenshtein`） | lenient.ts 全文 | 玩家输入唯一入口；fuzzy 名字解析、§9.38 oldest-copy、`with`→`to`/actors 路由（lenient.ts:127-147）、lone-clock 纯推进（59-66）、canonical passthrough（37-48）、建议列表——全是可观察语义 |
| `Game.handleLine`（含错误/建议拼装、`« »` 回显格式） | play.ts:56-74 | 回合循环本体；错误消息文案被测试断言 |
| `Game.undo` + `turnLengths` 记账 | play.ts:44, 69, 161-167 | §7 state=fold 语义；undo 截断点无法从 log 后缀恢复（注释 40-43 明示） |
| `Game.wait` 的 ceil-to-halfhour 量化 | play.ts:184-192 | 见 UI-10 |
| `proseLines`/`interpolateClause`/`interpolateRecord`/`joinAnd`/`pretty`/`flattenAspects`/`journal`/`endingNotice`/`endingBanner` | render.ts:11-114 | §6.7 叙事规则与 §9.35 结尾行规则；纯函数零成本移植；endings.test 有文案快照 |
| 入口骨架：读脚本文件 → `Game.fromScript` → 循环；demo 的 fold-verify | examples/play.ts:38-46, demo.ts:29-53 | demo 是现成的移植正确性冒烟测试 |

### 可简化
| 项 | 位置 | 建议与理由 |
|---|---|---|
| 命令路由 | tui.tsx:368-381, 450-468 | 两份合一：REPL 统一 dispatcher 处理 `inspect` 前缀 + 数字/名称元命令；runPlain 已证明该形态可行（UI-03） |
| `inspect` | play.ts:132-146 | 保留则注意 bare-inspect 显示首卡的行为（UI-06）；丢弃卡片视图可整体去掉 |
| `look`/`cards`/`story`/`journal` 四视图 | play.ts:110-157 | REPL 里保留 story（或 journal）一个即可；`look` 可缩为一行。均为只读投影，不影响引擎状态 |
| `suggestions()`/`visible()` | play.ts:82-107, 199-291 | 完整建议枚举是便利功能，可降级为"打印动词表"（tui.tsx:26-28 的 verbList 一行事）；但删除需同步处理引用它们的测试（UI-12） |
| `processing()` | play.ts:171-178 | 可并入 wait 提示或 status 行，非独立视图 |

### 可丢弃（TUI 授权范围内）
| 项 | 位置 | 理由 |
|---|---|---|
| 全部 ink/react 组件：`Prompt`（行编辑/历史/tab 补全/paste chunk）、`ScriptPicker`/`pickScript`、`App`/`EntryView`/`InputLine`/`Static` 布局、`runTui` 的 render/Instance 生命周期与 `requestExit` 时序 | tui.tsx:70-319, 321-447 | 纯终端框架；Rust REPL 用行读取即可。tab 补全、历史、按钮均非游戏语义 |
| 三个终端怪癖处理 | tui.tsx:127-137, 158-175, 418-419, 250 | ink 专属 workaround（UI-09） |
| `runPlain` | tui.tsx:450-469 | 被新 REPL 取代（其骨架即新 REPL 的原型） |
| `quoteLine` 截断/折叠 | tui.tsx:62-68 | 展示层技巧；随结构化 Turn（UI-15）消失 |
| `card()` | render.ts:118-135 | 死代码（UI-05） |
| `examples/play.ts` 的 `listScripts`/title 正则 | examples/play.ts:11-26 | ScriptPicker 不复存在；UI-04 的平行解析是 bug，Rust 版标题一律取 `compileScript` 的 `meta.title`，脚本路径走 CLI 参数 |
