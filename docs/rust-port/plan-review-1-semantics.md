# plan.md 评审意见 — 视角：语义保真（评审 1）

- 评审对象：`docs/rust-port/plan.md`（plan v1）
- 依据材料：`docs/rust-port/analysis/01–05.md`、`docs/rust-port/verification/01–04.md`（以验证报告为准）
- 评审方式：只读。逐条对照四份验证报告的"计划必须吸收/应驳回"清单与 plan.md；对 §3 语义对齐规范抽查 14 条，逐行打开 worktree 真实源码核对（dice.ts、clock.ts、ref.ts、format.ts、lexer.ts、query.ts、types.ts、predicate.ts、parse.ts、engine.ts、content.ts、script.ts、store.ts、play.ts、lenient.ts、fold.ts 及 test/dice.test.ts、test/decks.test.ts、test/nightjar.test.ts、test/explorer-lib.ts、test/reach-fuzz-lib.ts、test/reach-adversarial.test.ts、tools/）。未修改任何源码，未运行构建/测试。

---

## 总评：**需修订后批准**

计划的语义保真骨架是扎实的：四份验证报告的全部 must 清单（01：2+6 条；02：10 条；03：9 条；04：10 条，共 37 条）逐条都能在计划中找到落点，无一缺失；全部"应驳回/标红"的顺手修复项（recipe id 漂移、slot role 校验、argSpelling 合并、reach worklist、load 期键校验、ending flavour 收紧等）都被正确挡在门外，且未发现计划夹带新的行为变更——仅有的几处分歧（原子写、loadLog 行号、孤立代理项 lossy、REPL 取代 runPlain）均为验证报告明示允许并已"记录为有意分歧"的项。§3 二十条要点抽查 14 条，公式/常量/行号/顺序描述与源码逐项相符，未发现一处技术性错误。

唯一够成 must-fix 的问题在 W4：黄金差分是全计划唯一的**跨语言**逐字节证据，但其场景清单没有显式钉住 §3.14 自己承认"无现成测试"的引擎排序锚点（(end, beginId, clause) 全序并列、同 halfhour 多实体到期序等）——W3 的补测是 Rust 单侧自证，不能证明与 TS 逐位一致。修订成本很低（在 golden-dump 场景清单中点名 2–3 个锚点场景），修订后即可批准。

---

## 逐条意见

### PR1-01 [must-fix] W4 黄金差分未显式钉住引擎排序锚点，"逐位一致"对这些锚点缺少跨语言证据

- 位置：plan.md §4-W4（269–285 行）、§3.14（195–202 行）、§6.1 风险 2。
- 事实：§3.14 与验证 02（ENG-10）明说"同 halfhour 多实体到期序这类**无现成测试**的锚点，按 ENG-10 补测义务新写"。但 W3 新写的排序测试是 Rust 实现 vs 手推期望值的**单侧自证**；全计划唯一的跨语言逐字节证据是 W4 的 JSONL `cmp`。W4 现有四类场景（nightjar WIN_PATH、sampledWalk、S1–S14 转写、fuzz 200×25）不保证命中：
  1. due 窗口 `(end, beginId, clause)` 排序键的**并列打破**（engine.ts:1073）——S1 只有 60/30 不同时长窗口，无同 end 并列；
  2. 同 halfhour 多实体到期、按 entities 插入序出日志（engine.ts:1065→1238）；
  3. authored `on expire of` 按声明序首条匹配 vs 内建 DESTROY #id（1252–1265）——S2 覆盖了"替换内建"，但未验证覆盖多触发器并列。
- REFUSE 丢弃 advance 记录 + id 回收（engine.ts:257–262/1070）有 fuzz 覆盖（reach-fuzz-lib.ts:149 生成 REFUSE outcome），但建议在 manifest 场景名里显式可辨识。
- 要求：W4 范围第 3 点的场景清单显式列出上述锚点（每个至少一条 golden），或规定 W3 新排序测试的期望值一律从 TS golden-dump 导出（而非手推）。否则"200 个 fuzz 种子零字节差"通过也不能宣称排序语义逐位一致。

### PR1-02 [should-fix] W4-3 的 S1–S14 转写判据与事实不符，且部分场景不可用脚本文本表达

- 位置：plan.md §4-W4 第 3 点（275 行）。
- 事实：转写校验写的是"用 vitest 里既有断言（**骰序/事件序**）校验转写无误"。实测 reach-adversarial.test.ts 有 14 个 test、断言均为 analyzer verdict / 引擎状态断言（如 :135–138、:214–216、:282–300 的 verdict/chain/aliveNames），并非骰序/事件序断言（那在 dice/parallel.test）。另外部分场景用 `probeClause` 以 #id 绑定 stackable 副本（:77–79），mid-game 铸出的 id 无法出现在脚本文本里。
- 建议：(a) 等价性判据改为"对转写/golden 驱动后的记录与状态**重跑原 S1–S14 的 expect 断言**"；(b) 对不可脚本文本化的场景，优先用 manifest 已允许的 canonical 步 driver（canonical 句可携带 #id ref），而非强行转写脚本。

### PR1-03 [should-fix] W5 reach-scale 验收"NameStat 逐字段一致"按字面含计时字段，不可满足

- 位置：plan.md §4-W5 验收（296 行）。
- 事实：NameStat 含 `ms` 字段（reach-scale-lib.ts:163–165，performance.now 计时）；跨语言逐字段一致对 ms 不可能成立。按字面执行该验收会永远红或被静默忽略。
- 建议：改为"verdict 分布一致 + NameStat 非计时字段（name/verdict/existence/roots/nodes/depth）逐字段一致；ms 仅作量级参考"。

### PR1-04 [should-fix] W5"共享单一遍历器"缺验证 03 对 REACH-01 的修正细节，照做会丢 seg 语义

- 位置：plan.md §4-W5 范围（289 行）、§2.2 reach 行。
- 事实：验证 03 REACH-01 修正表述补充："walkSites 对每个 IF 臂分配新 seg 且 REFUSE/throwing 记在**所在 seg** 上，这是行为的一部分；共享遍历器时回调需同时供给两种消费形态"。计划只写"walkSites 与 forEachAuthoredOutcome 共享单一遍历器（REACH-01）"，未带此修正；执行者若只读计划，可能把 seg 分配/记账丢进共享遍历器之外。
- 建议：在 W5 范围补一句该修正（或在 §7.2 要求执行者对照验证 03 各条"修正表述"）。

### PR1-05 [note] §2.5 开头两条 bullet 完全重复，且第一条尾部有乱码

- 位置：plan.md:163–164。两条 `pub struct ChorusError …` 逐字重复，163 行结尾"温др"为乱码。删 163 行即可。

### PR1-06 [note] §2.2 模块映射表 TS 列笔误

- 位置：plan.md:116。TS 列写"src/engine/store.rs"，应为 `src/engine/store.ts`。

### PR1-07 [note] §3.9 尾反斜杠段留有草稿式自问自答，最终裁定正确但易误读

- 位置：plan.md:190（"拼入字面 `"undefined"` 四字符? 不——TS 是把…"）。核对源码（lexer.ts:30–33：`out += src[j+1]` 在 j+1 越界时拼入字符串 "undefined"，随后 33 行抛 unterminated）：计划的最终裁定（Rust `push_str("undefined")` 后照常抛错，中间值不可观察）正确。建议改写为一句裁定，删去问句草稿。

### PR1-08 [note] 验证 04 must#9 的 sig() 字段清单未在计划中具名

- sig()（reach-fuzz-lib.ts:246，`g: s.rng` 等）仅靠 §2.2 的 reach-fuzz-lib.ts → tests/common/reach_fuzz_lib.rs 整文件映射隐式覆盖。建议在 §3.12 或 W4 提一句"sig 字段清单属冻结面"，与 genScenario/runFuzzSeed 并列。

---

## 附一：四份验证报告"计划必须吸收"清单逐条落实核对（评审项 1）

结论：**37/37 全部有落点，无缺失**。关键映射：

**验证 01（core）**
| 项 | 计划落点 |
|---|---|
| CORE-04 dice 位级 + 向量门 | §3.1/§3.2、W0 |
| CORE-16 192 条 toThrow + 消息表 | §1.1.4（plan.md:16，已用勘误值 192/13 文件）、§2.5、messages.md（W0 建、W6 收口） |
| CORE-05 cmp_utf16 两处 | §3.6、§2.2 ref 行；sameRef 剔除声明在 §1.2（plan.md:51） |
| CORE-06 IndexMap/插入序 | §3.7、§2.2 types 行、§2.3 |
| CORE-08 吞字符/\s/DUR_RE | Q2/Q8/Q9、§3.8；23 处全集在 §2.3 逐条列出 + W1 注意项（含 clock.ts:52） |
| CORE-12 arg() 不查 param 键 | Q3、W1 验收 |
| CORE-17 三值边界 | §3.10、W1 验收"三值边界四测" |
| CORE-20 逐字节展开 | §3.16、W1 验收 |
| 附加发现（functions.test.ts:138 字母序巧合） | §2.5（plan.md:166） |

**验证 02（engine/content）**
| 项 | 计划落点 |
|---|---|
| ENG-04 插入序（byName 外层放宽） | §2.2 types 行（"池数组保序即够，ENG-04 修正"）、§3.7 |
| ENG-05 RNG（u128 修正、seed>2^53 保持 JS 取整） | §3.2（显式吸收 u128 修正）、§3.5、W0 |
| ENG-06 往返方案 (a) + 静默 catch | §3.13、Q6 |
| ENG-10/11/12 顺序矩阵 | §3.14、W3 验收（含补测义务原文） |
| ENG-09/15/16/17 文案逐字 + 双基数 | §2.5（三条名冲突消息调用点各自提供）、Q1/Q4/Q5、W2 验收 |
| ENG-07 值语义映射 | §3.10（"逐点对应不统一"原文吸收） |
| ENG-08/20 pair 不透明 String + serde | §3.7/§3.15、§2.2 store 行 |
| ENG-21 UTF-16 排序 | §3.6 |
| ENG-13/14 ending/deck | §1.2（flavour 禁收紧）、W2 drawFrom 三规则、W4 前置 DRAWN 断言 |
| ENG-22 脚本=数据 | §2.2 include_str! 行、W2 |
| 应驳回 7 条（ENG-15/16/17 顺手修、meta 枚举、byName IndexMap、u128、ENG-02 计数） | 全部按修正后吸收（Q1/Q4/Q5、Q15+W2"不做 meta 枚举化"、§2.2、§3.2）；ENG-02/03/18 修正分别落在 §2.2 store 行、W3 验收 ENG-03、§1.2 死代码清单（1058 活分支） |

**验证 03（reach/tools）**
| 项 | 计划落点 |
|---|---|
| Map 序 + OrdinaryOwnPropertyKeys | §3.7（含 REACH-04 加强：原型名不拦数字开头）、§1.2 禁 load 校验 |
| 单一遍历器 | W5（细节缺口见 PR1-04） |
| 置换/LCG 位精确 + golden | §3.4/§3.12、W0/W4/W5 |
| 哨兵模型（MANY/Infinity/\u0000derived/firstCross d==0→1440/cap=4/firstFreeze Set） | §3.11 逐项 |
| 朴素 round-robin（REACH-10 标红） | §1.2 明确禁止 + W5 + §6.1 风险 4（准入判据全文采纳） |
| matchMemo 键安全 / worldCache Analyzer | W5（先不做 + 键含 aspects 条款） |
| CLI-02 stdout golden + UTF-16 排序 | §1.1.7（含 defaultcard 修正）、W5 验收 |
| siteReason 分支（REACH-08 修正） | Q17、W5 |
| CLI-03/04 契约 | W5（含 soft 行 stderr、17 参 REACH-02 修正值） |

**验证 04（ui/tests）**
| 项 | 计划落点 |
|---|---|
| Game 不可丢（7 文件修正值） | §1.3（plan.md:58，明写"验证 04 UI-01 修正计数"）、W3"一个不丢" |
| RNG 逐字 + 向量表第一验收物 | §3.1/2/4、W0 |
| 消息契约 | §2.5、messages.md |
| nightjar seed-1899 + 双跑字节恒等 | W4 验收（[[5,2],[8,4],[14,7]] 等实测核对相符）、W3 |
| game-over 双路径双文案 | §1.3、W3 验收 |
| test 侧基建（settle/snap/seed，不在 src/） | §2.2 末行（TEST-11 警示原文）、W3 |
| grep gate | W0 雏形 + W6 定稿、§5.4 |
| 差分骨架（LCG/genScenario/runFuzzSeed） | §3.12、W4（sig 缺口见 PR1-08） |
| headless 接口 | W3 game.rs、W4 driver |
| TEST-01 修正蓝本 | W3 settle.rs（"不用 parallel 单版"原文）、§5.2 表 |
| 勘误（parallel 13、store.ts:24–27、UI-04 :47、UI-14 5 处、可简化表测试影响） | W3（13）、§3.15（24–27）、§1.3（title 取 meta.title）；inspect/look/cards/story/journal 全保留即规避了可简化表冲突 |

## 附二：怪癖冻结清单完整性 + 夹带检查（评审项 2）

**完整性：通过。** 验证报告中标红/驳回的全部"顺手修复"项均在 §1.2 Q1–Q17 或"明确禁止"节：recipe id 漂移（Q1）、slot role 不校验（Q4）、谓词吞非法字符（Q2）、parse.ts arg() 死分支（Q3）、argSpelling str/pair 合并（禁令，§1.2/§2.2/§3.16 三处一致）、reach worklist 化（禁令 + W5 + §6.1）、load 期拒绝类整数键（禁令，改为复刻 OrdinaryOwnPropertyKeys）、重复声明/大小写（Q5）、meta 枚举化（Q15 + W2）、ending flavour 收紧（禁令）、sameRef 排序替换（禁令）、CORE-07 大整数显式拒绝（§3.5/§3.19 明令不拒绝）、死代码清单（含 1058 活分支、emptyClause 反向要求）、UI-04 examples title 正则（随文件不移植，REPL 取 meta.title 属 §1.3 授权的新代码）、bare inspect（Q11）、test.fails 幽灵注释。

**夹带检查：未发现行为变更夹带。** 计划内全部结构性改动（ROLES 下沉、entities 传入 compileContent、ctx 收敛、47→HALFHOURS_PER_DAY-1、*30→常量、原子写、loadLog 行号、lossy 代理项、demo nextRecordId、REPL 取代 runPlain）逐一对到验证报告的 should/允许/授权条款；三处"有意分歧"（原子写、行号、lossy）均在计划内自我记录，符合验证 02 ENG-20 的条件。

## 附三：§3 语义对齐规范抽查（评审项 3）——14 条，全部与源码相符

1. **§3.1 splitmix64**：dice.ts:19–31 常量 GOLDEN_GAMMA=0x9e3779b97f4a7c15、MIX1=0xbf58476d1ce4e5b9、MIX2=0x94d049bb133111eb、移位序列 30/27/31 与计划逐字一致；"finalizer 单次应用"定性正确；三个原始值与 test/dice.test.ts:86–89 逐字相符。
2. **§3.2 rollDie**：dice.ts:38–42 `asUintN(seed)^asUintN(counter)`、`(BigInt(sides)*r)>>64n`——u128 widening 修正确为必要，计划已吸收。
3. **§3.4 置换**：content.ts:430–437 FNV-1a（offset/prime/UTF-8 字节）、453–461 降序 Fisher–Yates、`key=asUintN(seed)^hash^asUintN(i)`、`%(i+1)`；drawFrom:472–477 三规则；四个置换向量与 decks.test.ts:141–144 逐值相符。
4. **§3.5 seed 解析**：script.ts:113–114 `/^-?\d+$/` + `Number()` 属实。
5. **§3.6 UTF-16 排序**：ref.ts:47–49 比较器、types.ts:228 `a > b` 对称交换、sameRef:51–53 为相等比较——计划"受影响面仅此三处"准确。
6. **§3.10 三值语义**：predicate.ts:452–454（undefined→false）、499–504（strict 仅零绑定抛）、493/492–498（path 首存活卡 + dead-stamp）、428–435（every/any 空集）；query.ts:81（`v !== true`）、84（类型不等即 false）——逐行相符。
7. **§3.13 往返**：engine.ts:351 `try { … } catch { clauses = [] }`、fold 侧 1027 共用——属实。
8. **§3.14 顺序矩阵**：1073 排序键、1076 completion≤slot、1081–1086 cursor 注释、1062–1067 快照 hoisted、1228 声明序、1238 快照序、1252 首条匹配、233–235 per-clause 复位、735 首 ENDING、257–262 拒绝返回 stateIn + 1070 `let id = firstRecordId`、956–963 wait 连续性断言、975–978 `w ? w.start : absMin(parseClock(tGame))`、1154–1168 失败仅拒该窗且 `out.windows.delete(w.key)`、1058 首析取支死/`(toMin===now && !hasDue)` 活——全部逐行核对无误（含三处 `*30`：1239/1279/1287）。
9. **§3.16 宏展开**：引文确在 parse.ts:654–655；FN_RESERVED_TOKENS 七 token（:682）、MAX_FN_DEPTH=8（:686）、substString 单趟（770–777）、delta 折入（722–728）、substExpr 三分支（825–838）、unknownCallError 用 byName 插入序（845/849）——相符。
10. **§3.17 拼串形**：predViewFromToks:141–161 词邻插空格、:269 span substring、:273 `replace(/\s*(>=|…)\s*/g,'$1')`——相符。
11. **§3.18 clock**：clock.ts:11 HALFHOURS_PER_DAY=48、:35 上界 47、parse.ts:122 同值、:52 matchAll、MIN_PER_HALFHOUR=30（:9）——等值重构声明成立。
12. **§3.19 数值**：format.ts:84 `-0 → +0` 分支确认（`-0 < 0` 为 false 走 `+`）；lexer.ts:56–58 `Number(num[0])`。
13. **§3.20 Game/lenient**：play.ts:188–189（now+60、ceil×30）、:73 `« … »`、:44/:69/:161–167 turnLengths、:60 endingNotice、lenient.ts:37 逃生舱正则、:65 lone-clock `clauses:[]`——相符。
14. **Q3/Q12 冻结点**：parse.ts:575 提前 return 使 :587 死条件成立、spawnArgs:535–536 param 键可达（SPAWN/SET 不对称属实）；startsRef:227–233 不拦角色名 vs arg():576 asRole 先拦（Q12 拦截顺序承重属实）。

## 附四：W4 黄金差分判据评估（评审项 4）

- **生成器不确定性**：已排除主要风险——`tReal` 全仓无赋值点（仅 engine.ts:27/317），Game 路径恒 null（grep 证实）；golden 相关路径无 Date.now/Math.random（仅 scale 的 performance.now 与 fuzz-cli 的耗时打印，不进 golden）；nightjar seed 1899 在脚本元行；sampledWalkStates/genScenario 均为局部 LCG 纯函数（explorer-lib.ts:276–277、reach-fuzz-lib.ts:35/78）。**残留风险**：(a) dump 脚本内的"固定 LCG 动作选择器"是新代码，属事实冻结面，建议把其源码一并入库（manifest 引用），防止 TS 侧重生成时静默漂移；(b) S1–S14 转写判据问题见 PR1-02。
- **manifest 锚点覆盖**：含 name/driver/sha256/snap，snap 侧已提示 TEST-13 键序规则；但引擎排序锚点未被任何 golden 显式覆盖——见 PR1-01（本次评审唯一 must-fix）。
- **fuzz 覆盖**：genScenario 生成 REFUSE outcome（reach-fuzz-lib.ts:149），id 回收链路有随机覆盖；200×25 + 骰/牌/窗口并发使整体证据面良好，补上 PR1-01 的显式锚点后判据即充分。
