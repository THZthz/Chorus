# 03 — reach.ts 与 tools/ CLI 移植前分析

对象：`src/reach/reach.ts`（1969 行）、`tools/reach-cli.ts`、`tools/reach-scale-cli.ts`、`tools/reach-fuzz-cli.ts`、`tools/lint-cli.ts`（及其直接依赖的 `test/reach-scale-lib.ts`、`test/reach-fuzz-lib.ts`——两个 CLI 的验收语义都在这两个库里）。只读分析，未改动任何源码。

## 算法概述

reach 计算**静态卡牌可达性**："玩家能否获得卡 X"，对编译后的 content + 初始 State 做 Duck-typing 式的穷举证明，不运行引擎。输入 `(CompiledContent, State, target | demand-query, opts)`，输出 `CardReport`：verdict（obtainable / unobtainable / unproven / unusable）、existence 层级（proven / candidate / none）、首条证明链 `chain`、全部证明路径树 `paths`、候选守卫源 `candidates`、notes。判定保守方向是"不可得"绝不误判：结构缺失才判 none，任何守卫/时序不确定性降级为 candidate。四个阶段：

1. **站点普查**（`collectSites`/`walkSites`, L205-454）：按 `forEachAuthoredOutcome` 同构递归（content.ts:303）遍历所有 verb recipe 与 trigger 体，枚举 SPAWN/REPLACE/DRAW 生产站点，标注 IF 段（seg）、authored REFUSE 段死、throwing op（MOVE/RELATE/UNRELATE/REPLACE + DRAW…INTO）整表降级、first-match shadow、同表 kill、重复 mint 碰撞（dupRisk）等 RawSite 标志；同时收集 SET 站点、destroyedBy、非栈名集合。
2. **风险触发器/冻结分析**（`computeWorldFacts` 内 L712-953 的大块）：初值碰撞、同槽双触发器互撞、跨槽时序、确定性 throw 冻结（throwFrozen）、递归槽冻结线 `freezeDist`/`frozen`——把"世界在第一个确定性拒绝处永久冻结"建模为站点/卡组的容量上界。
3. **不动点**（L1061-1078）：`committable verbs ↔ proven sites` 互相喂养——verb 可提交需要槽位从已可用卡填充（`slotSupport` 的首适配回溯），站点被证明则经 `addProduct` 增量注入可用世界（`avail`/`flatCache`，权重 min(count,32)）。证明次序即 Map 插入次序。随后派生 `universe`（一切可能出现卡的静态描述符）、`canEverMatch`、`deadVerbs`、`deckCap`。
4. **逐卡分析 + 路径导出**（`analyzeCard` L1297-1505，`pathFor` 递归展开需求子树，深度上限 12）：三种语义（存在 / usable / demand 查询），chain 只取第一条证明路径（byte-stable 约定）。

整份 per-(content, initial) 的中间产物缓存在 `WorldFacts`（L587-613，WeakMap 双层键 L615），同世界多卡共享——这是近期"world facts cached per (content, initial)"优化的落点。

## CLI 职责一览

| CLI | 输入 | 输出 | 退出码 |
|---|---|---|---|
| `reach-cli.ts`（npm run reach） | argv: `<script.txt> <CARD> [--usable] [--demand "<q>"]` 或 `--all` | 逐卡报告到 stdout（verdict、路径树缩进渲染、候选、notes）；usage 到 stderr | usage 错误 2；编译/查询异常未捕获→1；成功隐式 0 |
| `reach-scale-cli.ts`（npm run bench:reach） | flags `--protos/--recipes/--cores`；`--worker i k P R` 为隐藏 worker 模式 | 构建 CS 规模合成世界（reach-scale-lib），按 `idx % k === i` 分片普查，父进程汇总 p50/p95/verdict 分布 | worker 失败→Promise.all reject→未处理拒绝非零；正常 0 |
| `reach-fuzz-cli.ts`（npm run fuzz:reach） | flags `--seeds/--steps/--states/--from` | 差分模糊：`runFuzzSeed(seed)` 纯函数生成场景→`analyzeCard` vs 引擎穷举前向探索（settleStep）；FAIL 行到 stderr，进度到 stdout | 有 failure 1，否则 0；打印坏种子列表 |
| `lint-cli.ts`（npm run lint:scripts） | argv 文件列表，缺省 `readdirSync('scripts')` | lintScript 警告逐行 stdout + 汇总 | 设计上恒 0（警告不失败）；文件缺失/无 scripts 目录→未捕获异常→1 |

后三者是移植后的**验收工具**：fuzz 提供正确性差分（种子可复现），scale 提供性能基线，reach-cli 提供逐字节输出对照。

---

## Findings

### REACH-01 [类别: modularity] [严重度: high]
reach.ts 一个文件承载四个可分层的关注点：站点普查（L146-454）、静态谓词求值/渲染（L456-575）、世界事实+不动点（L577-1292，其中 L712-953 是约 240 行的内联"风险触发器/冻结"块，含 6 个子规则：初值碰撞、同槽互撞、跨槽时序、gated-branch、throwFrozen、slot-blocking 豁免）、逐卡分析+路径导出+显示渲染（L1294-1968）。建议 Rust 侧按 `census / world(freeze) / fixpoint / paths / render` 分模块，每块已有清晰分隔注释可直接当切缝线。特别地 `walkSites`（reach.ts:205-227）是 content.ts:303 `forEachAuthoredOutcome` 的**手工镜像**（多了 seg 编号与 flag 收集）——两边递归形状必须逐字一致，否则站点穷举性（'none' 判定的健全性依据，L30-35）失效；Rust 里应让二者共享同一个遍历器（回调或迭代器），消除镜像漂移风险。

### REACH-02 [类别: modularity] [严重度: medium]
`siteProven` 有 **18 个位置参数**（L1638-1656），其实参几乎全部已在 `WorldFacts` 上（notes、mintSources、fillers、firstCross 等是例外，是"构建期只读伴随物"）。移植时应把构建期伴生状态并成一个上下文 struct（Rust 侧一个 `&Ctx`），否则 18 参调用极易错位。同理 `collectSites` 的返回类型是匿名结构，其 `setSites` 行类型在 L246、L258、L1645 三处逐字重复拼写——应提为命名接口。

### REACH-03 [类别: semantics] [严重度: high]
**JS Map 插入序是输出确定性的一等依赖**，Rust 必须用 IndexMap/IndexSet（或显式序号）逐点复刻：
- `initNames` 对同名（stackable）实体**后写胜**（L667-671），代表描述符取 `State.entities` 插入序的最后一个副本——影响 demand 匹配与 usable 检查的描述符。
- `avail`（L630）的插入序决定 `slotSupport` 候选序（L1017-1027 先遍历 `avail.values()` 再按序展开 `name#copy`），进而决定首适配回溯 `assign`（L1032-1046）选出**哪一条**提交性证明——chain/paths 的 byte-stable 完全系于此。
- `proven`/`committable` 的 Map 迭代序 = 证明次序：`provenSourceFor` 返回首个命中（L1152-1157），`analyzeCard` 的 demand/名字模式按 `proven` 序推路径（L1327、L1384）——首条 chain 与路径排序都依赖它。
- `byList`/`trigBySlot`/`mintCounts`/`mintSources` 等其余 Map 同理（多数只做集合判断可放宽，但 L732 `[...byList].map` 的处理序会进入 `risky` 后再无序化，需逐个核对）。

### REACH-04 [类别: semantics] [严重度: medium]
编译期 Map 由 `Object.entries` 构建（content.ts:204、218、274：prototypes/verbs）。JS 对象键序：字符串键按插入序，但**类整数键**（"123"）先于全部字符串键且按数值升序。当前内容用 `E0001`/`V0` 等前缀规避了这一点，但语言层面 `SPAWN 1` 式命名即可触发差异。Rust 侧 serde_json 默认**不保序**（需 `preserve_order` feature，其 IndexMap 同样有类整数键问题吗——serde_json preserve_order 保留原始 JSON 文本序，与 JS 解析器的类整数键重排在"JSON 对象含类整数键"时不一致）。移植时必须固定：raw content 的键迭代序 = JS 语义，并加一条"键名不得为类整数"的 load 期校验或在读取处显式重排。

### REACH-05 [类别: semantics] [严重度: low]
排序语义：`trigFlat.sort((x,y) => x.c - y.c)`（L774）依赖 ES2019+ 的稳定排序；其后的成对循环对相等 `c` 跳过（L777），实际结果不依赖同键元素间序，但 reach-cli `--all` 的 `[...names].sort()`（reach-cli.ts:73）是**默认字典序 = UTF-16 码元序**：Rust `String::sort`（字节序/码点序）仅在非 BMP vs U+E000..FFFF 区间与之一致性不同。卡名当前全 ASCII，验收输出要逐字节对照时应在 Rust 侧显式 pin 排序键（如按 UTF-16 编码比较或声明 ASCII 限定）。

### REACH-06 [类别: semantics] [严重度: medium]
哨兵值模型，Rust 需逐一对应：
- `MANY = 1e9`（L135）是 count/cap 的"无穷"：参与 `Math.min(f.count, 32)` 权重帽（L645）、`cap !== MANY` 时才导出 `deck.capacity`（L1231）、`servedByCap` 的 `cap > idx`（L1929-1934）。Rust 用 `i64` 精确即可（1e9 < 2^53），但导出/显示处的哨兵相等判断要保留。
- 真正的 `Infinity`：`freezeAt`/`freezeDist` 的"无冻结"（L888、L971）与 `firstCross` 距离比较（L909-911、L1178）——建议 Rust 用 `Option<i64>` 或 `i64::MAX` 并保留比较方向。
- `'\u0000derived'` 哨兵名（L1336、1341、1878-1888、1902-1907）：作为 avail/byName 的**键**参与 exists/count 门与槽查询匹配（描述符可见、裸名门不可见）。Rust 建议用枚举 `MintKey::Bare(name) | Blinded`，但必须保持"占一个键、查询可中、名字门不中"三性质。

### REACH-07 [类别: portability] [严重度: medium]
三个模块级**对象身份键控的可变缓存**：`trigIndexCache`（WeakMap, L233）、`matchMemo`（WeakMap<Query, WeakMap<Entity, boolean>>, L487，注释声明"永不失效"因为 Query/Entity 编译后不可变）、`worldCache`（WeakMap<CompiledContent, WeakMap<State, WorldFacts>>, L615）。契约"调用方在多次 analyzeCard 之间不得改动 content/initial"（L580-586）只写在内部注释里，API 层无强制——JS 里被篡改会静默拿到陈旧事实。Rust 无 WeakMap：应重构为显式 `Analyzer<'c>`（持有 content 引用与惰性 WorldFacts）或以内容 id/指针做键的缓存；matchMemo 的"实体身份"在 Rust 需换成稳定键（Entity.id 对合成实体恒为 `~name`，同 id 不同 aspects 的合成描述符会在 WeakMap 语义下分键、在 id 键下撞键——注意 L1879/L1907 的合成实体 id 恒为 `~${name}`，若 aspectsOverride 不同仍共享 id，Rust 缓存键须含 aspects 或维持指针等价物）。

### REACH-08 [类别: dead-code] [严重度: low]
- L677-678：外层 `startSlot`/`cyclic` 定义后被 L769-770 块内同名定义遮蔽，且 `cyclic` **全文无任何调用**——两处均为死代码（tsconfig 未开 noUnusedLocals 所以能编译）。
- `analyzeCard` 解构了 `destroyedBy`、`freezeDist`、`dirtyOwners` 但函数体内未使用（L1304-1305）。
- `siteChain` 的 `content` 参数未用，以 `void content` 压制（L1953）——直接删参。
- `siteReason` 的 `site.verb && !initNames.size` 分支（L1964）：仅在初始世界**完全没有实体**时才提示"verb committability unknown"，实际初始态恒有玩家实体，疑似陈旧条件，按上下文意图应复查。

### REACH-09 [类别: duplication] [严重度: low]
- `spawnNameRef`/`replaceWithRef`/`drawDeckId` 在站点三元链里对同一 Outcome 重复调用 2-4 次（L320-322，每次重走 `o.args`）——先求一次存局部变量。
- derive 致盲谓词 `site.derives && objectAlways(...)` 与 `drawBlindsBare(...)` 的组合在 L991、1192、1389、1407、1433-1441、1877、1902 七处重复书写——值得抽 `siteMintsBareName(site)` 单一谓词。
- deck 卡名展开（`order` + `defaultcard`）在 L369-371、389-391、418-420、722-724、987-988、1328-1336 六处重复遍历——抽 `deckCardNames(deck)` 迭代器。
- 外层/内层 `startSlot`+`cyclic` 重复（见 REACH-08）；`JSON.parse(aspectsJson)` 的 try/catch 在 L1099 与 L1549 两处——可在普查期 parse 一次存结果。

### REACH-10 [类别: performance] [严重度: medium]
不动点每轮（`while (changed)` L1061-1078）对**每个未证明 verb** 重跑 `slotSupport`（全 avail 事实 × 槽位 + 回溯），对**每个未证明 site** 重跑 `siteProven`（含 selfGated、pathOpen、门时序等全套检查），无论上一轮失败以来世界是否变化。CS 规模（2084 verbs、约 2000+ 站点）下轮数受证明链深约束但每轮是 O(V×world + S×checks)。世界视图已有 O(1) 增量维护（`push`/`flatCache` L638-664），但"谁需要重测"没有 worklist：Rust 移植建议改为事件驱动（新证明的名字 → 只重测 avail 集合/门谓词引用该名字的 verb 与 site；`exprExistsNames`/`writablePaths` 已能给出每个站点依赖哪些名字，可预建倒排索引）。这是 7x 优化之后下一层的主要空间，且**语义不变**（不动点单调，证明次序不变则输出不变——注意：改变测试次序会改变 `proven` 插入序，见 REACH-03，须保持先到先证明的语义或显式排序）。

### REACH-11 [类别: performance] [严重度: low]
- `provenSourceFor`/`provenSources` 每次线性扫全部 proven 站点（L1147-1159、L1189-1199），而 `pathFor` 每个需求名字每节点调用一次 `provenSources`（L1255）——路径树导出是 O(节点数 × proven)。`mintSources`（L985）已在不动点期维护"名字→已证明站点"，但与 `provenSources` 的谓词重复且只含裸名情形；统一为一个名字索引可消除。
- `canEverMatch` 的 `universe.some(...)` 线性扫描已按 Query 对象 memo（L1113-1128），CS 规模下每个去重查询一次，可接受。
- `pathFor` 每分支 `new Set([...seen, n.name])`（L1256）O(分支²) 分配，深度帽 12 下无碍；Rust 用持久化集或小 Vec 线性查即可。

### REACH-12 [类别: clarity] [严重度: low]
- `analyzeCard` demand 分支内 `const paths = writablePaths(demandQ)`（L1367）遮蔽外层 `paths: ReachPath[]`（L1301）——易误读，改名。
- 非空断言 `!` 密布（如 L1322、1381、1860 `site.deck!`、1930），两处 `throw new Error('internal: …')` 不变式（L505、521）——Rust 侧对应 `expect`/`unreachable!`，建议在移植清单里逐个登记不变式来源。
- 错误处理面窄：`JSON.parse` 双点 try/catch（REACH-09）、`parseQuery` 异常会从 CLI 冒泡（见 CLI-01）。

### REACH-13 [类别: clarity] [严重度: low]
`firstFreeze.set(d, Math.min(firstFreeze.get(d) ?? d, d))`（L878、L885）：键就是 `d`，首次存入值必为 `d`，此后 `min(d, d)=d`——该 Map 事实上是 Set，`Math.min` 是无效操作。非 bug，但误导读者以为有按距离取小逻辑；Rust 直接用 `HashSet<i64>` + `min(keys)`。

### REACH-14 [类别: portability] [严重度: low]
数值/位运算语义盘点（结论：可直接映射，需注意点如下）：全文件算术均为整数域（分钟数、计数 ≤ 1e9、32 帽），f64 与 i64 双安全；`firstCross` 的 `((s*30 - now) % 1440 + 1440) % 1440`（L684-687）负数安全且 JS/Rust `%` 截断方向一致；`slotSupport` 的 `cap = 4` 展开帽与 `Math.min(count===MANY?cap:count, cap)`（L1015-1023）必须逐字保留（决定首适配证明的形状）。test 侧两个 LCG（reach-fuzz-lib.ts:35-38、reach-scale-lib.ts:28-31）用 `Math.imul` + `>>> 0`：Rust 为 `u32` 的 wrapping mul/add/xor——生成场景逐种子 bit-exact 是差分验收有效的前提。

### REACH-15 [类别: portability] [严重度: medium]
reach 的 DRAW 事实（`deck.order`、`order[position]`、capacity）全部建立在 content.ts:428-461 的 `permutePositions` 上：FNV-1a 64（UTF-8 字节）+ splitmix64 + BigInt 64 位掩码的 Fisher–Yates。Rust 用 `u64` wrapping 直接对应，但它是"逐位一致"清单里最硬的一项：deck 位置错了，`servedByCap`、路径导出、chain 文案全错。移植后应加一条用 JS 侧 golden deck 序列做的对照测试。

### CLI-01 [类别: semantics] [严重度: low]
reach-cli.ts：`--demand` 若为最后一个 argv，`args[demandIdx + 1]` 为 undefined（L20），demand 模式**静默消失**（若同时给了卡名则按普通卡名模式跑）；`parseQuery` 的异常在编译之后才抛出（L1310 调用点在 reach.ts），未捕获→exit 1 带堆栈。Rust 移植应：参数校验前移（demand 值缺失即 usage exit 2）、parseQuery 失败给单行错误 + 明确退出码。

### CLI-02 [类别: semantics] [严重度: low]
reach-cli.ts：`--all` 与显式卡名同时给出时卡名被静默忽略（L69-76 分支序）；`--all` 的名字集 = prototypes ∪ 活初始实体 ∪ deck 卡（L70-72），排序见 REACH-05。`renderPath` 的 4 空格/层缩进、`path i/n:`、`verdict: X (existence: y)` 等文案是事实上的验收快照格式，应作为 golden 输出冻结（L44-66、reach.ts 的 `slotProofLines` L1919-1924 注释明确要求与旧格式 byte-identical）。

### CLI-03 [类别: portability] [严重度: medium]
reach-scale-cli.ts：worker 用 `spawn('npx', ['tsx', process.argv[1]!, …])`（L43）拉起——依赖 PATH 上的 npx、Windows 需 shell、且**每个 worker 重建整份 CS 世界**（编译成本 × 核数）；IPC 靠 `tmpdir()/reach-scale-${i}.json`（L30、50-51），文件名无 PID/run 区分，**并发两次运行会互踩**；worker stdout 被丢弃、stderr 透传（L44）；父进程 Promise.all 失败→未处理拒绝。Rust 移植天然该改为线程 + channel（同一进程共享一次编译），但 `--worker i k P R` 的分片语义（`idx % k === i`）与 `NameStat` JSON 结构是和 JS 版做 A/B 性能对照的接口，移植期应保留等价物。

### CLI-04 [类别: clarity] [严重度: low]
reach-fuzz-cli.ts 契约干净、应原样保留：flags 默认值 200/7/400/1；`FAIL`→stderr、进度/汇总→stdout 的流分离（CI 日志解析依赖）；`runFuzzSeed(seed)` 纯函数（reach-fuzz-lib.ts:19）是种子可复现的根基——Rust 版必须保持"每种子自带 LCG、无全局状态"；退出码 1 iff 有 failure，坏种子空格分隔列表（L34）。小瑕疵：计时用 `Date.now` 而 scale CLI 用 `performance.now`（L23 vs reach-scale-cli.ts:34），不一致但无害。

### CLI-05 [类别: semantics] [严重度: low]
lint-cli.ts：缺省时 `readdirSync('scripts')` 相对 CWD（L16）——从别处运行会 ENOENT 未捕获→exit 1，与文件头"Exit code stays 0"（L2-3）的承诺在"文件/目录缺失"这一失败模式下矛盾；文件缺失同理。设计意图是"警告不失败"，Rust 移植应把"输入不存在"与"有发现"分开：前者显式报错退出码，后者恒 0。

---

## 移植风险清单（最难移植的 5 个点）

1. **Map/对象迭代序 = 输出确定性**（REACH-03/04）：`initNames` 后写胜、`avail` 插入序驱动首适配证明选择、`proven` 序决定 chain 与路径排序；编译期 `Object.entries`（含类整数键重排）。Rust 需 IndexMap 纪律 + 一份"同一 content 两次分析输出逐字节一致"的跨语言 golden 对照。
2. **walkSites 与 forEachAuthoredOutcome 的镜像契约**（REACH-01）：站点穷举的完备性是 'unobtainable' 判定健全性的唯一依据；两边遍历形状（arm/else/嵌套 IF、函数展开后列表）必须共享同一实现。
3. **deck 置换与 LCG 的位精确**（REACH-15/REACH-14）：BigInt splitmix64 置换、`Math.imul` LCG、`(seed, deckId)` 纯函数——差分模糊与 scale 基线两个验收工具的有效性都押在上面。
4. **哨兵值与时序冻结模型**（REACH-06/13）：`MANY=1e9`、`Infinity` 冻结距离、`'\u0000derived'` 键哨兵、once/capacity 的 `cap` 推导（L405-431）——这些不是显示细节，而是 exists/count 门与 deck 容量判定的输入。
5. **全局身份键控缓存 → 显式结构的重构**（REACH-07）：`worldCache`/`matchMemo` 的"身份相等 + 永不失效 + 调用方不得变异"契约在 Rust 无对应物；重构成 `Analyzer` 结构时若 matchMemo 键选错（如用 `Entity.id`，合成实体 id 恒为 `~name` 但 aspectsOverride 可不同）会引入假命中。

## 性能关键路径

- **每次 (content, initial) 一次**（worldFor 缓存，L615-623）：`collectSites`（全 content 遍历）→ 风险/冻结分析（含同槽两两 O(T_slot²) 与跨槽两两 O(T²) 配对，L747-757、775-782——触发器多时是平方热点）→ 不动点（REACH-10 的 worklist 缺失是主瓶颈）→ universe/deadVerbs 构建（原型数 × 合成实体）。
- **每卡一次**（analyzeCard，CS 规模 1566+ 卡普查）：命中 WorldFacts 后主要是 `proven` 线性扫描（REACH-11）+ 路径树展开（`pathFor`，深度帽 12）+ demand 模式的 `universe.some`（memo 化）。
- **微热点**：`matchQ` WeakMap memo（L487，"fixpoint 在 CS 规模重复询问同一 (query, card) 数千次"——注释原文，L484-486）；`flatCache` 的 O(1) 增量 push（L653-664，7x 优化的落点）；`mintStacks` 的 Set 查询（普查期每 mint 一次，L351-367）。
- 验收基线：`npm run bench:reach`（1566×2084 全核）与 `test/reach-scale.test.ts`（CI 缩小版）——Rust 版应以 summarize 的 p50/p95/max 口径对齐对比。
