# 05 — 测试与文档：移植后如何证明行为一致（只读分析）

> 范围：`.worktrees/rust-port/test/` 全部 25 个文件（10,351 行，约 460 个 `test()` 调用点）、
> `docs/spec/text-engine-v0.2.md`（1,914 行，Spec v0.4）、`docs/plans/v0.4|v0.5`、
> `docs/reverse/cultist-simulator.md`、`README.md`。所有结论只读得出，未运行构建/测试。

## 一、测试体系概述

- ** vitest 是壳，不是语义**。全部断言是显式的 `expect`（deep-equal + `toThrow(/regex/)`），
  **没有一处 `toMatchSnapshot` / `vi.mock` / fake timers**（已 grep 验证）。Node 专属 API 只有
  `fs/os/path`（临时目录 JSONL 往返）。这意味着 Rust 侧（`cargo test`）可等价重建，
  vitest 本身不构成移植障碍。
- **断言的核心范式是 fold 等价**：`snap(fold(records, base)) === snap(playedState)`，再加一层
  `saveLog/loadLog` JSONL 往返。这是 plan v0.4 §0「determinism laws 1–6」的测试化身
  （v0.5-parallel-verbs.md:13-35 原文列出六条）。**这六条法律就是移植验收的法律**。
- **snap.ts 是唯一的共享状态快照**：手写 JSON 序列化（Map 键排序），覆盖 entities / relations /
  byName 池 / now / **rng 计数器** / decks 游标 / fired / ended / **windows**（snap.ts:7-23）。
  它是逐字节比较的 oracle，Rust 侧必须逐字段复刻（含排序规则）。
- **两条确定性故事线**：引擎 RNG（splitmix64 计数器，见下）+ 日志驱动 fold（永不重推导，law 3）。
  测试同时钉住两端：`ROLLED(total, ctrAfter)` 让 fold 精确落位计数器
  （dice.test.ts:617-727 undo 重构三部曲）。
- **黄金端到端锚点**：`scripts/the-nightjar-flyer.txt`（seed 1899 校准）——nightjar.test.ts 钉住
  具体骰子序列 `[[5,2],[8,4],[14,7]]`（:153）、牌序 `['SCANDAL','RUMOUR']`（:163）、9 步胜路。
  这是移植最强的单个验收测试：seed+输入序列+全部输出逐位钉死。
- **探索性测试**：nightjar-explorer.test.ts（3 pass：穷举 sweep / 种子随机 walk / typed surface），
  每步断言 fold 等价 + 句子无损往返 + 世界形状（checkStep，:92-138）。它对引擎行为是不变式断言，
  数量少（4 个 test）但覆盖面是全引擎最大的。
- **测试分两层**：内容无关的 engine 层（seed.ts 世界 fixture + 内联 rig content）与内容驱动层
  （nightjar script）。seed.ts（195 行）是 spec §6.4 旗舰示例的数据化，刻意 vitest-free。
- **lib 辅助（6 个）全部 vitest-free**，scripts 与 tests 同源共用（explorer-lib.ts:1-4 声明的纪律），
  这是它们可以直接移植到 Rust 的前提。
- reach 系（reach.test / adversarial / fuzz / scale，共 ~1,900 行）测的是**静态可达性分析器**
  （src/reach，工具而非玩法语义），但 reach-fuzz-lib 内含一个**完整的受控场景生成器 + 穷举前向
  探索 oracle**，是现成的差分测试骨架（见 §三）。

## 二、RNG 机制详述（移植的第一验收对象）

### 2.1 引擎 RNG：计数器式 splitmix64 纯函数

**Math.random 在 src/ 中零出现**，且被两个「grep gate」测试钉死
（dice.test.ts:935-943、decks.test.ts:831-838 遍历 src/*.ts 源码找 offender）。
引擎唯一的随机性（spec §9.41 ruling 41，plan law 5）：

```
die i of a roll at counter c  =  1 + floor( sides * splitmix64(seed XOR (c+i)) / 2^64 )
```

具体实现（全部 file:line）：

| 构件 | 位置 | 内容 |
|---|---|---|
| `splitmix64(x)` finalizer | `src/core/dice.ts:26-31` | `z = x + 0x9e3779b97f4a7c15; z = (z^(z>>30)) * 0xbf58476d1ce4e5b9; z = (z^(z>>27)) * 0x94d049bb133111eb; return z^(z>>31)`，每步 `& (2^64-1)`。常量在 dice.ts:19-22（MASK64 / GOLDEN_GAMMA / MIX1 / MIX2）。注意这是 **finalizer 单次应用**，不是完整 splitmix64 序列。 |
| `rollDie(seed, counter, sides)` | `src/core/dice.ts:38-42` | key = `BigInt.asUintN(64, seed) XOR BigInt.asUintN(64, counter)`（**负数取二补码**，dice.ts:39）；`(BigInt(sides) * r) >> 64n` 即 floor 除 2^64。 |
| 计数器状态 | `src/core/types.ts:95`（`rng: number`）、`:112`（初值 0）、`:133`（cloneState 复制） | `State.rng` = 全历史消耗的 die 总数；一掷 m 骰恰消耗 c..c+m-1。 |
| 播放侧应用 | `src/engine/engine.ts:773-780` | total 从 bonus 起，循环 `rollDie(content.seed, c+i, sides)`；`ctx.state.rng = c + count` **在草稿上**推进；具体化 `ROLLED(total, c+count)`。 |
| 回放侧应用 | `src/engine/engine.ts:781-789` | fold 的 ROLLED：`state.rng = ctrAfter`，**其余什么都不做**（law 3：回放永不重掷）。 |
| seed 来源 | `src/content/content.ts:110-112`（`ContentRaw.seed?: number`）、`:169`（编译后 `seed: number`）、`:293`（默认 0）、script 元行 `seed: N`（§10）；坏 seed 是载入错误（dice.test.ts:177-182） | seed 存在于 content，不在 State。 |
| 骰子向量钉 | `test/dice.test.ts:85-89`（splitmix64 原始值 3 个）、`:94-113`（27 行 (seed,counter,sides)→die 表，**python3 大整数独立计算**，含负 seed、2^53−1、2^40 计数器） | 语言无关的常量表，Rust 直接照抄断言。 |

**BigInt 纪律**（dice.ts:7-9 注释）：`sides * u64` 放不进 float64，全程 BigInt。
Rust 对应物是 **64 位乘法取高 64 位**（`u64` widening multiply → `(a as u128 * b as u128) >> 64`，
即 `mulhi`），XOR 前对 i64 seed/counter 做 `as u64` 二补码转换——语义与 `BigInt.asUintN` 严格对应。

### 2.2 牌堆 RNG：同 mixing 的 (seed, deckId) 固定置换

- Fisher–Yates **对位置 0..n-1** 洗牌，位置 i 取 `j = splitmix64(seed ⊕ hash(deckId) ⊕ i) mod (i+1)`
  — `permutePositions`，`src/content/content.ts:453-461`。
- `hashDeckId` 是 FNV-1a-64：offset `0xcbf29ce484222325`、prime `0x100000001b3`，逐 UTF-8 字节
  （content.ts:430-437，`TextEncoder` → Rust 用 `.as_bytes()` 等价）。
- 纯函数 of (seed, deckId)，载入时定死、不消耗 rng 计数器、不存于 State
  （State 只有游标 `decks: Map<id, cursor>`，types.ts:95 附近；服务函数 `drawFrom` content.ts:472-477
  处理 reset 回绕 / defaultcard / 耗尽拒绝）。
- 向量钉：`test/decks.test.ts:141-144`——`permutePositions(4,0,'RUMOURS')=[0,2,1,3]`、
  `(4,42,'RUMOURS')=[3,0,1,2]`、`(7,0,'SEVEN')=[0,6,3,1,4,5,2]`、`(7,-3,'NEG')=[4,0,2,6,3,1,5]`
  （同样 python3 独立预计算）。
- **seed 漂移不对称**（spec §9.30/§9.41/§9.42）：改 seed 只影响未来骰/抽；过去的 ROLLED 行照样回放，
  但 DRAWN 行会**响亮失败**（游标断言重推导，decks.test.ts:732+ 有专门 describe）。

### 2.3 测试侧 RNG（另一套，别混淆）

fuzz/walk 用的是 **NR 型 LCG**：`s' = (imul(s,1664525) + 1013904223) mod 2^32`，种子先 XOR
`0x9e3779b9`——`reach-fuzz-lib.ts:35-38`、`reach-scale-lib.ts:28-31`（LCG 常量 0xc5）、
`explorer-lib.ts:276-277`（种子 `'NIGH'`=0x4e494748）。它与引擎 RNG 无关，是**场景脚手架**；
但 `runFuzzSeed(seed)` / `buildCsScale(n,m)` / `sampledWalkStates()` 都是纯函数，
若要在 Rust 侧重放相同场景做差分，LCG 必须逐位移植（三个 `Math.imul`/`>>>0` 操作都好搬）。

### 2.4 逐字节证据链

- 同 seed 两次全新运行写出**字节相同**的 JSONL（dice.test.ts:917-933）；不同 seed 必然不同。
- `store.ts:22-25`：`saveLog` = 每条 record 一次 `JSON.stringify`，按行拼接。Rust 的 record
  序列化必须字段序稳定（建议直接移植字段顺序或用带序 struct）。

## 三、差分/模糊测试基础设施（能否当 TS vs Rust 双实现差分骨架？）

**结论：能，且有三条现成轨道。**

1. **reach-fuzz-lib.ts（507 行）** —— 「随机场景 → 两个 oracle → 差分断言」的完整范式：
   - `genScenario(seed)`（:78-223）：LCG 驱动生成随机 content（2–4 个动词、when/IF/ROLL 守卫、
     SPAWN/REPLACE/DRAW/SET/DESTROY/REFUSE、slot+expiry 触发器、deck、stackable/derive），
     注释里记录了两个真实 fuzz seed 教训（:315-327 seed 12606 重复 role、:338-346 seed 60447
     stackable #id 绑定）——这些是**引擎语义的隐性规格**，移植时值得逐条读。
   - `explore()`（:399-444）：BFS 穷举有界前向探索，状态签名 `sig()`（:246-282）**完整覆盖 State**
     ——含 rng 计数器（:252-254 注释警告：折叠相差一掷的世界会静默丢子树）、windows 细节、
     fired、decks。这是「哪些 State 字段参与世界区分」的权威清单。
   - `runFuzzSeed(seed)`（:457-507）：静态分析器 `analyzeCard` vs 引擎真相。属性：
     `unobtainable ⇒ 永不出现`（无条件下声音）；`obtainable ⇒ 某次出现`（探索到上界时降级
     inconclusive）。失败可由 seed 复现。
   - **TS vs Rust 用法**：把 `analyzeCard` 换成 Rust 分析器与 TS 分析器互差只是次级收益；
     主收益是把**引擎**当作第三个实现——同一 genScenario(seed) 喂给 TS 引擎和 Rust 引擎，
     逐 record 字节比对。生成器是纯函数、vitest-free，直接可移植。
2. **reach-adversarial.test.ts（1,017 行，S1–S14）**：随机欠采样的手工对抗场景（并行窗口重叠、
   expiry 链、REPLACE 链、stackable min:2、derive-of-derive、deck 冻结、三重同槽触发、
   冻结计数器的 roll 守卫…），每个都「推导自引擎语义 → 用 settleStep/explore 前向确认」。
   这 14 条是**边界语义的浓缩规格**，是最该进移植验收集的测试之一。
3. **reach-scale-lib.ts（184 行）**：`buildCsScale(1566, 2084)`（:33）生成 CS 规模合成世界，
   同参数字节相同（:11-12 注释：「two builds produce independent, comparable worlds」）——
   这句话本身就是双实现差分的邀请函：TS 和 Rust 各建一次，world facts/verdicts 必须一致。

另外两条**更直接的引擎级差分轨道**（不依赖 reach 系）：

- **黄金日志差分**：TS 侧用 explorer-lib 的 `WIN_PATH`（:235-245）、`sampledWalkStates()`
  （:275-306，ending-aware 随机游走）和 nightjar 胜路生成 JSONL 日志存盘；Rust 侧
  `fold(loadLog(path))` 必须 snap 相等，且 Rust 重放同输入序列必须产出字节相同的日志。
  日志相等 ⇒ RNG/顺序/结果逐位一致（这就是 §7 的定义）。
- **向量表差分**：dice.test.ts:85-113 与 decks.test.ts:141-144 的常量表语言无关，Rust CI 直接照抄。

## 四、测试 → 行为域 → 移植验收优先级

优先级定义：**P0** = 最低验收集（不通过不得宣称行为一致）；**P1** = 第二波（语义完整度）；
**P2** = 工具层（reach 分析器），最后移植；TUI（src/ui/tui.tsx）无直接测试，可按需求简化。

| 文件（行数 / test 数） | 行为域 | 优先级 |
|---|---|---|
| `dice.test.ts`（952 / 48） | RNG 本体向量、ROLL 语义、roll 绑定、ROLLED 往返、undo 计数器重构、JSONL 字节恒等、grep gate | **P0** |
| `engine.test.ts`（1,211 / 74） | READ 阶梯/recipe 首匹配、REFUSE 原子性、slot 纪律、fold 确定性、SET 增量、REPLACE、expiry、事件次序、record 形状 | **P0** |
| `snap.ts` + `settle.ts` + `seed.ts`（279 行 lib） | 快照 oracle、v0.5 settle 适配器、世界 fixture | **P0**（作为基建随 P0 移植） |
| `parallel.test.ts`（310 / 14） | v0.5 窗口/站锁/完成序 (endMin, beginId)/wait span 分区/开窗 fold | **P0**（完成序是确定性核心） |
| `sentence.test.ts`（141 / 18）+ `clock.test.ts`（96 / 12） | §5.2 worked examples、canonical 往返 `format(parse(x))===x`、§5.1/§9.45 时长文法 | **P0**（字节级文法契约） |
| `nightjar.test.ts`（364 / 21） | seed-1899 校准的故事验收：具体骰序/牌序/胜路/escalation 不触发 | **P0**（最强黄金 trace） |
| `decks.test.ts`（849 / 43） | 置换向量、DRAW/DRAWN、游标 fold、耗尽、livelock、content-drift 响亮失败 | **P0** |
| `nightjar-explorer.test.ts`（336 / 4）+ `explorer-lib.ts`（306） | 穷举 sweep 的引擎不变式、walk 的逐步 fold 等价、无损往返、世界形状 | **P0/P1**（invariant 引擎，建议尽早） |
| `stackable.test.ts`（664 / 21） | 池语义、oldest-first 寻址、engine-destroy-by-id、REPLACE 自替、fold 含池 | P1 |
| `conditionals.test.ts`（617 / 36） | IF/ELSE IF/ELSE、草稿可见性、具体化（日志无 IF op）、非对称失败纪律 | P1 |
| `functions.test.ts`（577 / 27） | 编译期展开等价（call site === 手工展开孪生）、递归环/深度 8/arity、触发器调用点 | P1 |
| `endings.test.ts`（401 / 25） | ENDING 首者胜、ended 派生自 fold、undo 重开、事件可 ENDING | P1 |
| `query-predicate.test.ts`（238 / 25） | §4.5 查询文法、§6.3 谓词、exists 世界存在性、count(query) | P1 |
| `script-play.test.ts`（428 / 26） | script 编译器（§10）、lenient 输入、game loop（handleLine 层，无 TUI） | P1 |
| `lint.test.ts`（169 / 14） | 载入期错误升级与 lint 警告（**错误消息文本即契约**） | P1 |
| `reach.test.ts`（591 / 32） | 静态可达性判断（existence/demand/usability）+ 路径导出 | P2 |
| `reach-adversarial.test.ts`（1,017 / 14） | S1–S14 对抗场景（同时是引擎边界语义规格） | P2（但其**引擎侧确认**部分建议升 P1） |
| `reach-fuzz.test.ts`（25 / 1×100 seeds）+ `reach-fuzz-lib.ts`（507） | 分析器 soundness 随机差分（~2s/100 seeds） | P2（结构留作跨实现差分骨架） |
| `reach-scale.test.ts`（89 / 6）+ `reach-scale-lib.ts`（184） | 分析器 CS 规模扩展性（CI 切片 400×400；全量 1,566×2,084 走 `npm run bench:reach`） | P2 |

## 五、docs 中的语义约定

- **spec（text-engine-v0.2.md，标题 Spec v0.4）**：版本化靠「Implementation rulings」累积编号
  （v0.2 → v0.2.7 → v0.3.x → v0.4 的 §9.41/§9.42/§9.43-45），每条 ruling 都是可测试的规范语句，
  且多处直接点名测试文件（如 §9.41 「the test suite pins a python-computed vector table」，
  :1571-1574）。关键约定：
  - §5.1 EBNF + §5.2 worked examples = 文法与 canonical 往返契约（sentence.test 逐例对应）；
  - §6.4 recipe 首匹配 + 稳定 id；§6.5 REFUSE 步原子；§6.7 处理周期（begin→lock→ticks→complete）；
  - §7 日志/fold；§9.30 seed 漂移不对称；§9.41 骰子（本文 §二）；§9.42 牌堆；§9.38 池寻址；
  - §10 script 格式（`seed:` 元行在此定义）；§12 open questions。
- **README.md**：「No backward compatibility, by design」(:16-19)——格式可在任意 commit 破坏，
  Rust 移植不受旧日志兼容约束；README 的布局段(:23-33)与 play 截例(:44-65)可作为 Rust CLI 的
  输出对照参考（仅文档级）。
- **plans/v0.4**：§0 六条 determinism laws（fold 逐字节、行往返、不重推导、步原子、无 RNG 例外、
  cloneState 深拷贝）——移植验收法律的权威出处；「Settled by fiat」(:324-341) 记录了被否决的
  替代方案（为什么池寻址是插入序而非 id 序等），防止移植时「顺手改进」破坏语义。
- **plans/v0.5**：完成序总序 `(endMin, beginRecordId)`（§5）、wait span 分区规则、beginId 锚定
  （C14：完成锚定禁止从 tGame 推导）、14 条 claim 验证记录（§7）。窗口折叠/完成序是 Rust
  移植最容易出错处，此文件是主要规格。
- **reverse/cultist-simulator.md**：CS 逆向档案（schema/数字/IL 验证），§6 映射表 + 8 gaps
  （v0.4/v0.5 已闭合其中 1/2/6/7）；reach-scale-lib 的 1,566/2,084/67 直取自此档案。
  对移植的意义：定义「CS 忠实度」的行为参照，以及哪些数字（挑战曲线 30/70/90、lifetime 表）
  若将来实现，有权威出处。§8 记录了两次被纠正的错误结论（挑战表曾反转），提示**照抄文档时
  以测试为准**。

## 六、Findings

### TEST-01 [duplication: medium] `run()` settle 适配器在 8 个测试文件中近乎重复
`conditionals.test.ts:34`、`decks.test.ts:33`、`dice.test.ts:35`、`endings.test.ts:44`、
`functions.test.ts:28`、`engine.test.ts:28,32`、`parallel.test.ts:19`、`stackable.test.ts:88`
各自定义 `run(content, state, src, firstRecordId)`，主体都是 `settleStep` 包装；且已出现漂移：
engine 版带 idGen 闭包、parallel 版额外处理裸时钟纯推进（:21-23）、其余不带。v0.4 plan 曾因
同样的漂移把三个内联 snap 合并成 snap.ts（v0.4-cultist-mechanics.md:26-31），`run` 是下一个
该合并的。对移植的影响：Rust 侧应只建**一个** `run` 助手，以 parallel 版为最全蓝本。

### TEST-02 [duplication: low] `canonical()` 往返助手双胞胎
`sentence.test.ts:11-19` 与 `clock.test.ts:12`（后者注释自认「mirroring sentence.test.ts」）。
parse→format→re-parse→re-format 的字节恒等检查应进共享 lib。

### TEST-03 [duplication: low] `ent()` 实体工厂四连抄
`reach.test.ts`、`reach-fuzz-lib.ts:67-76`、`reach-adversarial.test.ts:30-39`、
`reach-scale-lib.ts:122-124` 各写一份 `{id,kind,name,aspects,attrs,bornAction,deadAction,stackable}`
字面量工厂。字段新增时（如 windows 那次）有漏改风险。

### TEST-04 [duplication: low] LCG 随机数函数三处复制
`reach-fuzz-lib.ts:35-38`、`reach-scale-lib.ts:28-31`、`explorer-lib.ts:276-277`：同一
`(imul(s,1664525)+1013904223)>>>0` 常量组。移植 LCG 时只需搬一处；但三处种子约定不同
（XOR 0x9e3779b9 / 0xc5 / 'NIGH'），需在 Rust 侧保留各自的种子入口以重放相同场景。

### TEST-05 [portability: high] 测试对 vitest 的耦合面极小，但错误消息是契约
全 test/ 无 snapshot/mock/timer（已 grep）。vitest 专属仅 `describe/test/expect` +
`test.fails` 仅有注释提及（见 TEST-09）。断言词汇 = deep-equal + 字符串包含 +
`toThrow(/regex/)`。**Rust 重建的唯一硬骨头是 toThrow 的正则**：如 `/sides must be an integer >= 2/`
（dice.test.ts:315）、`/already in use/`（engine.test.ts:261）、`/'actors' takes 1..2/`
（engine.test.ts:98）、`/recipe attune\.default: 'roll' is read but no ROLL can have run before it/`
（dice.test.ts:384）。**错误消息文本因此成为行为规格的一部分**——Rust 端要么逐字移植消息文本，
要么系统性地把正则放宽为错误码（后者改动量大且丢信息；建议前者）。

### TEST-06 [semantics: high] seed-1899 校准的具体数值是移植黄金 trace
nightjar.test.ts:153 `expect(rolled).toEqual([[5,2],[8,4],[14,7]])`、:163
`expect(drawn).toEqual(['SCANDAL','RUMOUR'])`、:346 `VOID, PORTENT, PORTENT`——这些数字是
seed=1899（脚本内 `seed:` 元行）下整条链（触发器 ROLL、函数展开、deck 置换、游标推进、
v0.5 完成序）的联合输出。Rust 引擎若能原样通过 nightjar.test 的移植版，等于同时验证了
RNG、牌堆、处理周期、事件序四大机制在该坐标下逐位一致。反过来说：任何「合理的」实现差异
（如完成序 tie-break 写错）都会在此处爆出。dice.test.ts:161-173 的 `[14,3]/[11,3]/[12,3]`
与 decks.test.ts:141-144 的置换向量是更小的同类锚点，应最先通过。

### TEST-07 [semantics: medium] sig() 给出「参与世界区分的 State 字段」权威清单
reach-fuzz-lib.ts:246-282 的状态签名含 now、**rng**、ended、entities(name,kind,aspects,attrs-\>container)、
relations、fired、decks、windows(verb,start,end,recipe,refs,locked)，并明确排除 beginId/clause
（「exploration-order artifacts, not world facts」，:268-271）。Rust 侧实现任何状态去重/等价比较时
照此清单，否则要么丢子树（漏 rng，:252-254 注释原话）要么去重失效（多含 beginId）。
snap.ts:7-23 是同一清单的序列化版，两者互相印证。

### TEST-08 [duplication: low] rig verb 字面量大量重复
`{slots:[{role:'actors',min:1,max:1,query:'kind:character'}],duration,recipes:[{out}]}`
以 `waitContent`/`inspectContent`/`tuneContent`/`decayContent`/`probeContent`/`geo` 等名字
在 dice.test.ts:65-79、endings.test.ts:23-29、engine.test.ts:387-395、:554-562、:742-754、
parallel.test.ts:34-38 等处重复。低风险（都是测试局部 rig），移植时可顺手统一为 `rigVerb()`。

### TEST-09 [dead-code: low] reach-adversarial 头注释提及的 test.fails 从未使用
reach-adversarial.test.ts:12 「asserted with test.fails inversions」——grep 全文件无 `fails(`
调用。注释描述的机制不存在（可能随重构移除），移植时不要为其留设计。

### TEST-10 [clarity: medium] 模块级共享 Dictionary 的注册泄漏模式
各测试文件的模块级 `const dict = coreDictionary()` 后跟 `dict.registerVerb('INSPECT')` 等
（dice.test.ts:30-33、reach-adversarial.test.ts:45-48 `compile()`、parallel.test.ts:33）。
vitest 每文件一个模块实例，文件内测试共享注册结果；这隐式依赖「registerVerb 幂等/重复注册无害」
与文件级隔离。Rust 若用全局字典需决定：每测试新建（与 TS 每文件等价）或全局注册一次——
**不同选择会影响 reserved-word 冲突类测试的行为**（如 lint.test.ts 的 reserved 断言）。

### TEST-11 [portability: medium] settle.ts 是 v0.5 语义的测试侧化身，需原样移植
settle.ts:15-34 的 `settleStep`（执行一步 + 纯推进关窗）与 :36-61 的 `act/done/refusal`
编码了 v0.5 回合形状 `[advance…, begin, complete…, wait]` 与 REFUSE 位置约定。大量 P0 测试
经它执行——它不在 src 里，**移植清单容易漏掉它**。settle.ts:25 `Math.max(...windows.end)`
的「推进到最后一个窗口的 end」规则就是预置的等价推进语义。

### TEST-12 [semantics: medium] grep-gate 测试在 Rust 侧要换形态
dice.test.ts:935-943 与 decks.test.ts:831-838 在运行时读 src 源码断言无 `Math.random`。
Rust 等价物：CI grep gate（禁 `rand` crate、`rand::random`、浮点随机），并建议加一条
**dice 路径禁浮点**的检查（BigInt 纪律 → Rust 的 `u128` widening mul；一个
`f64` 参与就会破坏 dice.test 向量表，向量表本身就是探测器）。

### TEST-13 [clarity: low] snap() 的键序规则要精确复刻
snap.ts:9-21 用 `([a],[b]) => a<b ? -1 : 1` 排序 Map 条目；`fired` 用 `.sort()`（字符串字典序）；
windows 是排序后的浅拷贝数组（:19-21）。Rust 用 `BTreeMap` 天然有序，但要确认 JSON 键/值的
**序列化形状**（字符串化后的对象键序、整数格式）与 TS `JSON.stringify` 一致——snap 是字符串比较，
`1` vs `1.0`、键序差异都会造成假阳性。若做跨实现比对，更稳的锚是 **JSONL 日志字节**（record 形状
简单得多）而非 snap 字符串；snap 保留给各实现内部的 fold 等价。

### TEST-14 [duplication: medium] error-message 正则与消息文本遍布全部 P0/P1 文件
（与 TEST-05 同根，单独立项以便排期。）抽样：`/seed must be an integer/`（dice.test.ts:181）、
`/PROGRESS belongs/`（engine.test.ts:1002-1003）、`/REFUSE belongs/`（:1007-1008）、
`/ROLLED is engine-written/`（dice.test.ts:547-554）、`/deck 'D1' is exhausted/`（src 侧
content.ts:476）。建议 Rust 移植按文件整理一张「消息→测试」对照表，作为 spec §9 各 ruling 的
附录（消息文本是 ruling 的可观察面）。

### TEST-15 [modularity: low] 测试→src 的 import 面较宽，利于划定移植边界
统计各测试文件的 import 目标：core（dict/parse/format/query/predicate/clock/dice/ref/sentence/types）、
content（content/script）、engine（engine/fold/store）、reach（reach）、ui（play/lenient/render）。
**没有任何测试 import `ui/tui.tsx`** —— TUI 完全在测试面之外（README 亦称可简化），Game
(`ui/play.ts`) 的 headless `handleLine` 是现成的 Rust 侧对接口。

## 七、移植验收策略建议

**总原则：日志字节是最终裁判。** 「行为一致」在本项目有现成定义（spec §7 + plan laws 1-6）：
相同初始态 + 相同输入序列 ⇒ 相同 log ⇒ `fold(log)` 相同状态（含 rng/decks/fired/ended/windows）。
全部验收手段都是这条定义的不同投影。

**第 0 步（小时级）**：移植向量表。dice.test.ts:85-113 的 30 个常量 + decks.test.ts:141-144 的
4 个置换 + dice.test.ts:85-89 的 splitmix64 原始值。Rust `splitmix64`/`rollDie`/`permutePositions`
用 `u128` widening mul + `as u64` 二补码 XOR 直接对表。这是移植其余一切的前提。

**第 1 步**：移植 oracle 基建（snap.ts → Rust snap（注意 TEST-13 的序列化形状）、settle.ts、
seed.ts、store 的 record serde）。随后跑 engine.test 的 fold 等价子集（engine.test.ts:194-250）
——`fold(loadLog(saveLog(log))) === played` 在单实现内成立即说明 fold/serde 自洽。

**第 2 步（核心：黄金日志差分）**：
1. 在 TS 侧写一个脚本（复用 explorer-lib / WIN_PATH / nightjar 胜路，或直接用 `Game.fromScript`
   + 固定输入行列表）把 N 条确定性游玩轨迹的 JSONL 存盘，连同初始 State（script 文本即可）入库；
2. Rust 侧 `fold(loadLog(path))` 断言 snap 相等（验证 fold）；
3. Rust 侧用同一输入序列**从头玩**，断言产出的 JSONL 与 TS 侧字节相同（验证 play；
   dice.test.ts:917-933 已示范「同 seed 双跑字节恒等」的判据）。
   建议轨迹集：nightjar 胜路 9 步（seed 1899）、sampledWalkStates 的确定性 walk（LCG 移植后
   场景完全可重放）、reach-adversarial S1–S14 的手工序列（它们覆盖并行窗口/ expiry/ deck 冻结
   等边角）。
4. nightjar.test 的移植版（具体骰序/牌序断言）作为收尾门。

**第 3 步**：移植 P1 文件（stackable/conditionals/functions/endings/query-predicate/script-play/lint），
错误消息按 TEST-14 对照表逐字搬。lint 类测试（lint.test.ts 全部、各处的 toThrow）迁移成本最高
（正则全等），若决定放宽为错误码，需在 spec 里以 ruling 形式记档，否则 spec 与测试漂移。

**第 4 步（差分骨架）**：移植 reach-fuzz-lib 的 `genScenario` + LCG（TEST-04 的一处实现），
双实现跑同一 seed 流：每 seed 两侧各 `compileContent` + 跑 K 个 settleStep，逐 record JSON 比对；
异常时 `runFuzzSeed(seed)` 的可复现性直接平移成「报 seed 号」的协作协议。explore/sig 可移植为
Rust 侧的内部不变式工具（TEST-07 的字段清单）。reach-scale 的同参字节相同性质可用来做
分析器（若移植）的互检。

**TUI**：无测试依赖，Rust 按 README 的定位简化即可；保留 `Game.handleLine` 等价的 headless
接口以服务第 2 步的差分。

**顺序总结**：向量表 → fold/serde 自洽 → 黄金日志差分（nightjar + adversarial + walk）→
P1 全量 → fuzz 骨架长期跑。P0 集合（§四表）即最低验收集；reach 系 P2 不阻塞玩法语义验收。
