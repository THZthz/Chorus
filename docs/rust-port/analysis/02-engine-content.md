# 02 — engine/ 与 content/ 移植前分析（rust-port）

范围：`src/engine/{engine,fold,store}.ts`、`src/content/{content,script}.ts`，以及它们直接依赖的 `src/core`（types/dice/clock/ref/predicate/query/sentence/format/parse/dict）。只读分析，所有行号取自本 worktree。

## 模块职责概述

- **content.ts（592 行）**：内容模型 + 编译器。`ContentRaw`（脚本的对象形态）→ `compileContent` → `CompiledContent`（verbs/triggers/prototypes/templates/endings/decks 全部一次性解析、lint、失败即抛）。deck 的固定排列 `permutePositions` 是 `(seed, deckId)` 的纯函数，`drawFrom` 是 play/fold 共享的取牌函数。
- **script.ts（620 行）**：脚本文本（小型 DSL）→ `parseScript`（缩进敏感的逐行解析器）→ `Script`；`compileScript` = parse + compileContent + 初始 State 装配（实体/容器/关系/player 校验）；`lintScript` 是不抛错的告警通道。**它是逻辑（解析器+编译器），不是数据；内容数据就是脚本文本本身。**
- **engine.ts（1298 行）**：解释器。`executeStep` 四阶段（advance → 校验/选谱 → REFUSE 预演 → BEGIN），`applyOutcome` 是全部 17 个 outcome op 的唯一执行点（play 与 fold 共用），`applyRecord` 是 fold 侧重放入口，`resolveDue/fireWindow/fireSlot` 是 advance pass 的确定性核心。
- **fold.ts（17 行）**：`state = fold(log)`，`foldUpTo` = undo。纯薄封装。
- **store.ts（32 行）**：`Log`（内存 append-only 记录表）+ JSONL 存取。**只有它碰 Node API（node:fs）。**

**依赖关系**：`engine → content（只读 CompiledContent）→ core`；`fold → engine.applyRecord`；`store → engine（ActionRecord 类型）`。core 不依赖 engine/content（无循环）。`State` 定义在 core/types，engine/content 只通过它共享可变性模型。对外 API 面由 `src/index.ts` 全量 re-export；运行时入口是 `ui/play.ts` 的 `Game`（compileScript + executeStep + fold/undo）。

**可变性模型**：engine 采用 draft 纪律——每步 `cloneState(stateIn)` 得 draft（REFUSE 预演再 clone 一层 scratch），拒绝/拒绝则整体丢弃 draft，`ExecResult.state` 在拒绝时原样返回输入。`cloneState`（types.ts:115-134）必须逐字段深拷（byName 数组、windows.locked 有注释明确的 law 6 约束）。`CompiledContent` 编译后应视为不可变——但见 ENG-01 的例外。

---

## Findings

### ENG-01 [modularity: medium] CompiledContent 编译后被 compileScript 原地突变
证据：`src/content/script.ts:426-428` — `for (const e of script.entities) { if (e.stackable) { content.stackableNames.add(e.name); content.stackableKinds.add(e.kind) } }` 直接改已编译对象；而 prototype 的同名集合在 `content.ts:202-206` 编译期构建；`lintScript` 又第三次复刻（script.ts:545-552）。
影响：stackable 集合有三个构建点、两个所有者。Rust 应把 entities 一次性传入 `compileContent`（它的签名已预留 `entities` 参数，content.ts:194，但只用于 deck 解析），消除编译后突变，否则易出现 play/fold 视图不一致。

### ENG-02 [modularity: low] Log 的 append-only 不变量由调用方维护
证据：`src/ui/play.ts:164` — `this.log.records.length = length`（undo 直接截断数组）；`Log.initialState`（store.ts:9）生产代码从不读，只有 4 个测试文件用（decks.test.ts:431 等）。
影响：store.ts 虽只有 32 行，但 Log 的真实所有者是 play。Rust 里 Log 应提供 `truncate(len)` 并把 `initialState` 移出或显式标注为测试夹具。

### ENG-03 [duplication: low] applyRecord 的 complete/event 两分支近乎复制
证据：`src/engine/engine.ts:980-995`（complete 的 ApplyCtx + outcomes 循环）与 `engine.ts:1006-1021`（event 的同构代码）；且 `if (record.kind === 'event')` 连续出现两次（1002、1004）。
影响：Rust 合并为一个 `makeFoldCtx(record, state, …)` helper，避免两处分派点漂移。

### ENG-04 [portability: high] Map/对象插入顺序被行为依赖，Rust 禁用 std HashMap 迭代
证据（均为真实顺序敏感点）：
- `engine.ts:1065-1067` — 过期快照 `[...state.entities.values()]`：同一 halfhour 内多个到期实体的**事件记录顺序 = entities Map 插入序**（fireSlot 的 expiring 循环，1238-1271），直接进入日志。
- `engine.ts:280/290` — `busyReason` 对 `windows.values()` 取第一个匹配，决定拒绝文案指向哪个窗口。
- `engine.ts:511-525、585-589` — `JSON.stringify(aspects/attrs)` 把对象键序写进具体日志行与 journal 句子（formatArg pair 直接引用该字符串，format.ts:91）——**逐字节日志一致性依赖对象键插入序**。
- `content.ts:204/208/219/274/276` — `Object.entries(raw.verbs/prototypes/endings/templates)`：编译顺序、minted recipe id（`.recipe${i}`，content.ts:229）依赖声明顺序。
对照（无需处理的）：`resolveDue` 对 due 窗口显式排序 (end, beginId, clause)（engine.ts:1073，全序）；`state.fired` Set 只做成员判断；existsQuery/countQuery 与顺序无关（predicate.ts:446/484）。
影响：Rust 的 `State.entities/byName/windows`、`CompiledContent.*` 必须用 `IndexMap`/`Vec`/`BTreeMap(key=插入序号)` 保序；aspects/attrs 序列化需 `serde_json` + `preserve_order`（IndexMap）。

### ENG-05 [portability: high] RNG 全链路 BigInt 精确语义，Rust 需逐位复刻
证据：`src/core/dice.ts:26-42` — splitmix64 全常数/移位在 BigInt 上（MASK64、GOLDEN_GAMMA 等）；`rollDie(seed, counter, sides)` 的 key 是 `asUintN(64, BigInt(seed)) ^ asUintN(64, BigInt(counter))`（负 seed 按二补码），`1 + Number((BigInt(sides) * r) >> 64n)`；`content.ts:430-461` — `hashDeckId` 是 FNV-1a 64 over UTF-8 字节（TextEncoder），`permutePositions` 的 key = `seed ⊕ hash(deckId) ⊕ i`（全 asUintN）。测试向量独立生成（dice.ts:16-17，test/dice.test.ts）。
次点：seed 文本用 `Number()` 解析（script.ts:114），`^-?\d+$` 允许超 2^53 的字面量——精度丢失后 BigInt(seed) 与作者意图不符。Rust：u64 wrapping 运算 + `as u64` 转换即可精确复刻，但必须对负 seed 走同一二补码路径，并考虑 seed 解析溢出行为。

### ENG-06 [portability: high] engine 内部依赖 format∘parse 往返（law 2 是引擎承重墙）
证据：`engine.ts:347-356` — `registerWindows` 把刚 `formatSentence` 出的 `record.sentence` 重新 `parseSentence` 取回 clauses（"the SAME code path play and fold run — the divergence guard"），失败吞异常得空 clauses（则开不出窗口）；fold 侧同函数（engine.ts:1027）。
影响：Rust 有两条路——(a) 忠实移植 format+parse 并保证互逆（现状，风险集中在两个 853/107 行模块）；(b) 让 play 在内存里直接把 clauses 传给 registerWindows、fold 从 BEGIN 行重放（需验证与现状逐字节一致，sentence 字段仍要生成）。**移植时必须明确选一条并在报告中记录**；现状语义是：往返失败的 BEGIN 行静默不开窗口。

### ENG-07 [portability: medium] JS 值语义渗透到游戏判定
证据：
- undefined/null 三态：`resolveRef` 返回 `Entity | null`（engine.ts:114-125）；`aspectGet`/`evalExpr` 以 undefined 表示缺失，`evalPred` cmp 对 undefined 读 false（predicate.ts:453），strict 模式下 path 无绑定则抛（predicate.ts:499-504）——同一种"缺失"两种纪律，Rust 需 Option<T> 且逐点对应。
- `??` 保 0/falsy：`step.startMin ?? absMin(...)`（engine.ts:159）——startMin=0 必须生效。
- truthy 判定：`state:!x` 即 `v !== true`（query.ts:81）——字符串 "false"、0 都算未封印。
- `Infinity` 作槽位 max（script.ts:390、engine.ts:94 `slot.max === Infinity`）：不进 JSON（记录不含 slot），Rust 用 `Option<u32>`/`u32::MAX`。
- `===` 严格相等作用于 `AttrValue = string|number|boolean`（query.ts:84 的 state:= 比较）——Rust 枚举派生 PartialEq 需区分类型不匹配时的 false。

### ENG-08 [portability: medium] JSON 字符串作为日志内部的强类型载体
证据：SPAWN 把派生后的 aspects/attrs 以 JSON 字符串对写入具体行（engine.ts:509-518 解析、524-525/585-589 生成；`delete attrs['container']` 动态删键，517）。fold 端 `JSON.parse` 后 Object.assign。
影响：ActionRecord 的 JSONL 是唯一持久化格式（State 有 Map，从不序列化——这是移植利好）；Rust 端 pair 值建议保留 `String` 原样存取（不做结构化往返）即可逐字节一致；若结构化，必须保序且容忍开放键。

### ENG-09 [portability: medium] 名字冲突/stackable 规则有三份手写副本
证据：SPAWN 池检查 `engine.ts:490-493`、REPLACE 内联重复 `engine.ts:566-569`、`addEntity` 底层兜底 `types.ts:147-155`——同一规则（非 stackable 独占名 / stackable 只可与全 stackable 池共享），三处错误文案不同（'SPAWN: name … already in use' / 'REPLACE: …' / 'duplicate name … among alive entities'）；另 `mintIsStackable`（engine.ts:403-405）注释明确要求 play 与 fold 从同一 (name, kind) 复算。
影响：Rust 抽成单一 `ensure_mintable(state, name, stackable) -> Result<_, Msg>`，且错误消息文本必须保留（测试大量 `toThrow(/…/)` 断言，test/ 全套 150+ 处）。

### ENG-10 [semantics: medium] 事件/效果触发顺序的确定性锚点（逐条对应 Rust 断言）
证据（均在 engine.ts）：
1. advance pass：due 窗口按 (end, beginId, clause) 关闭，与 slot 边界交错时"completion ≤ 边界先发，slot-(<m) 事件在后"（1071-1097 与注释 1080-1083）；slot cursor 不跳过窗口跨度（同一边界只打一次，1084-1086）。
2. 每 pass 的过期快照一次、无级联（1064-1067 注释）；本 pass 造的 expires 下个 advance 才解析。
3. 每 slot：authored triggers 声明序 → expiries（快照序）（fireSlot，1218-1273）；authored `on expire of` 按名匹配首条（1252）。
4. 骰子：per-clause 绑定 `sctx.roll = undefined` 每子句复位（234-235），list 内 last-wins（ROLL 写 ctx.roll，775）；一掷 m 掷消耗计数 c..c+m-1（774-776）；ROLLED 行在 fold 只落计数（787-791）。
5. 首个 ENDING 生效、后续为 no-op（735）；REFUSE 丢弃整个 draft（含 rng/decks/fired 增量）。
6. 窗口完成失败只拒绝该窗口、窗口仍关闭（fireWindow 1161-1168）——防 livelock（plan v0.5 §0.4）。
影响：这些是移植的验收清单，建议每条在 Rust 侧写成一个排序测试（现有 test/parallel.test.ts 可作模板）。

### ENG-11 [semantics: medium] 被拒 BEGIN 连带丢弃整个 advance pass（含 id 复用）
证据：`engine.ts:257-262` — refused 时 `return { records: [record], state: stateIn }`，advance 已产生的 complete/event/wait 记录全部丢弃，记录 id `firstRecordId` 让给拒绝行；下次尝试以相同 id 重放同一 advance（确定性保证重放一致）。另有文档化的 re-roll 怪癖：拒绝行的 NARRATE 带预演骰点数字，但真计数器未动（443-448 注释）。
影响：Rust 端记录 id 分配必须同样"失败回收"，不可改为单调全局序列，否则 undo/foldUpTo（按 `r.id < uptoExclusive` 过滤，fold.ts:15-17）语义破坏。

### ENG-12 [semantics: low] clock/计时：只有 wait 行移动 now；complete 锚定窗口 start
证据：`applyRecord` wait 分支断言时间连续性（engine.ts:956-963，"wait spans must partition time"）；complete 分支优先从 windows 表读 `w.start` 锚定（975-978），tGame 仅显示；`SET expires → expiresAt = ctx.now + toMinutes(v)`（619）吃的就是这个锚；ADVANCE 在完成时扩展 pass 目标并落到 wait 行跨度（1085-1086、1099-1100，plan §2 C9）。
影响：Rust 需保留"wait 行是唯一时钟原语"这一不变量；fold 对 wait 行的连续性断言要原样保留（它是日志漂移的警报器）。

### ENG-13 [semantics: low] endings：状态字段驱动，无专门记录种类
证据：ENDING op 写 `state.ended`（engine.ts:735），日志只留具体 `ENDING <id>` 行；`executeStep` 顶部用 `stateIn.ended` 硬门（154-156）；undo 过头自然重开（注释 151-153）。ending 表在编译期校验（content.ts:207-214、checkEndingRefs 358-367）。
影响：Rust 保持"ending 是 fold 的投影"即可；flavour 是开放小写词表（content.ts:212），勿收紧为枚举。

### ENG-14 [semantics: low] deck：排列纯函数、游标在 State、DRAWN 折叠断言
证据：排列 `(seed, deckId)` 固定于编译期、不耗 rng 计数（law 5，content.ts:151-164 注释、453-477）；游标 `state.decks` 在 draft 上推进（engine.ts:825-826）；fold 端 DRAWN 行对照排列断言，漂移即响（844-871）；耗尽规则 reset 环绕 / defaultcard 永续 / 抛错（content.ts:472-477）。
影响：Rust 移植后 DRAWN 断言是内容漂移的唯一探测器，必须最先落地并有测试。

### ENG-15 [duplication: low] recipe id 铸造规则两处镜像已漂移（真实不一致）
证据：编译期 `content.ts:229` 铸 `.recipe${i}`（0 基）；lint 镜像 `script.ts:485` 铸 `.recipe${i + 1}`（1 基）。同一"无 id 带 when 的第一条 recipe"编译为 `read.recipe0`，而 lint 告警里叫 `read.recipe1`。
影响：只影响 lint 文案（用户按告警 id 去 script 里找不到对应物），不影响游戏语义；移植时应统一为一条铸造函数，顺便修掉。

### ENG-16 [clarity: medium] 槽位 role 从不校验，typo 静默失效
证据：`script.ts:391` — `role: role as SlotDef['role']`，正则只要求 `[a-z]+`（script.ts:172）；`compileContent` 原样存入 slotByRole（content.ts:221-227）；`validateSlots` 只遍历规范 ROLES（engine.ts:85-87）。
影响：verb 块里写 `objct: 1 kind:item` 加载成功且永不匹配——玩家看到的是莫名的 "does not accept 'object'" 或无解槽位。Rust 应在编译期拒绝未知 role（行为变更，需记录为有意修复或保持原状）。

### ENG-17 [clarity: low] 重复声明处理不一致 + 大小写 id 碰撞
证据：entities 重复报错（script.ts:136-139）、endings 重复报错（151-153）；但 prototypes（143）、templates（157）静默覆盖，verbs 重复头静默重置（163）。更实际的是：实体 id `s_${name.toLowerCase()}`（431）而查重区分大小写（138）——`ANA` 与 `ana` 同过检查、同 id，addEntity 的 `entities.set` 静默覆盖前者（types.ts:159），byName 双池指向同一实体。
影响：加载期损坏，难排查。Rust 应统一"重复=错误"并对 id 生成做大小写碰撞检查（同样属有意行为变更，需记录）。

### ENG-18 [dead-code: low] 未使用/不可达代码
证据：
- `clock.ts:78-84 crossedSlots`、`types.ts:221-223 attrGet`、`types.ts:182-184 getEntity`、`ref.ts:51-53 sameRef` — src 内零调用；`sentence.ts:75-77 emptyClause` 仅测试用。
- `engine.ts:1149` — `idGen: idGen ?? (() => ...)`：参数类型非可空，回退分支不可达（连同 `let mint = 0`）。
- `engine.ts:1058` — `toMin < stateIn.now`：唯一调用方 executeStep 已在 161-164 拒绝该情形。
- `script.ts:408` — `i = i` 空语句 + `script.ts:411 void i`（token 消耗设计被放弃的残迹）；`script.ts:390` — `cm[1] !== undefined` 恒真。
影响：移植时直接不搬或简化，但需对照测试确认无隐藏依赖。

### ENG-19 [clarity: low] 魔数与解析器结构点
证据：`engine.ts:1239`（`exp.at > absSlot * 30`）与 `engine.ts:1279`（`now: absSlot * 30`）绕过 `MIN_PER_HALFHOUR` 常量（1091 处则用了常量）；script.ts 的 meta 解析 `else meta.start = parseClock(...)`（116）——未来新增 meta 键会静默落进时钟错误；`lintScript` 双重解析（479-480 先 parseScript 再 compileScript，后者内部又 parse 一次）。
影响：Rust 用枚举的 meta key、常量替换魔数、parse 一次传两处。

### ENG-20 [portability: low] store.ts：仅有的 Node 依赖 + 非原子写
证据：`store.ts:3`（node:fs 唯一出现点，全 src 无 Math.random/Date.now）；`saveLog` 24-27 直接整文件覆写（无 tmp+rename）；`loadLog` 29-32 无行号、无 schema 校验。
影响：日志是唯一事实源，Rust 应原子写（临时文件 + rename）；加载错误带行号。JSONL 行是纯 JSON（ActionRecord 全部为 plain object/array），Rust serde 结构体可一一对应——Map 字段不进日志是现成的设计红利。

### ENG-21 [portability: low] 字符串排序语义
证据：`ref.ts:47-49 sortRefs` 用 JS 字符串 `<`（UTF-16 码元序）排序 actors/objects/with，决定 canonical 句子与 BEGIN 行 actors 顺序；`play.ts:238 localeCompare` 仅 UI 建议（TUI 可简化，不在本报告范围）。
影响：Rust `str` 按 UTF-8 字节序，与 UTF-16 码元序仅在 U+E000..FFFF vs 星外平面字符上不同——名字可含任意引号字符串（formatRef 允许），存在理论分歧面。保守做法：按 code unit 排序（Rust 手写比较或先转 Vec<u16>）。

### ENG-22 [modularity: low] content 数据的组织结论（问题 4 的正面回答）
证据：`script.ts` 全文是解析器/编译器逻辑（约 15 个语义敏感正则：entity 行 205-251、prototype 256-303、trigger 307-317、function 324-334、deck 343-368、ending 372-379、slot 383-392、recipe 394-413）；内容本体是脚本文本（examples/ 下 2 个 .ts 演示、test/ 里内联脚本）。`CompiledContent` 全部字段可从脚本确定性重建。
结论：**Rust 中内容应保持"脚本文本 = 数据文件"**：`include_str!` 嵌入或运行时读入，移植 parseScript/compileContent 后在加载期完成全部校验（现有行为就是 fail-loud-at-load）。不建议预编译成 JSON/二进制——理由：(a) 加载期错误文本是测试断言的一部分（test/ 150+ 处 toThrow，如 test/engine.test.ts:1002）；(b) deck 排列、stackable 集合等派生物必须在加载期从同一 seed 重算，预编译会制造第二真相源。CompiledContent 在 Rust 中就是纯数据结构（IndexMap/Vec），无可变逻辑。

---

## 移植风险清单（最难移植的 5 个点）

1. **插入顺序即语义（ENG-04）**：entities 的 Map 序决定同 halfhour 过期事件顺序；aspects/attrs 对象键序逐字节进日志与 journal。任何 HashMap/HashMap 型 serde 序列化都会让"fold(loadLog(saveLog(log))) 逐字节相等"（law 1）失败。需要 IndexMap 纪律贯穿 State、CompiledContent、JSON 序列化三层。
2. **BigInt 骰子/排列链路（ENG-05）**：splitmix64、FNV-1a(deckId)、seed⊕counter 的二补码 XOR、`(sides * r) >> 64`——全是 64 位精确整数语义，Rust 需 u64 wrapping 逐算子对齐，并以 test/dice.test.ts 的独立向量为验收。
3. **引擎内的 format∘parse 往返（ENG-06）**：registerWindows 靠 sentence 字符串往返取回 clauses，play 与 fold 共用此路径；移植时要么完整移植 lexer/parse/format 并保持互逆，要么重构为内存直传——两者都必须先建逐字节 round-trip 测试（law 2）再动手。
4. **顺序/原子性判定矩阵（ENG-10/11/12）**：advance pass 的窗口-slot 交错序、过期快照不级联、per-clause 骰绑定、拒绝时整 pass 丢弃且 id 回收、wait 行唯一动钟、complete 锚定窗口 start——每条都是行为语义，漏一条就是静默分叉，且大多没有专门测试直接钉住。
5. **错误消息与加载期校验文本（ENG-09/15/16/17 + test 断言）**：150+ 处 `toThrow(/…/)` 把错误字符串钉成了 API；名冲突规则三副本、recipe id 镜像漂移、slot role 不校验等现状怪癖要么原样保留要么有意修复并记录——默认应"先逐字保留，后统一清理"。
