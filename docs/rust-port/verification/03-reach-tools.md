# 03 — reach.ts 与 tools/ CLI 分析报告的逐条核实

对象：`docs/rust-port/analysis/03-reach-tools.md`（15 条 REACH-NN + 5 条 CLI-NN）。
核实方式：只读对照源码（`src/reach/reach.ts` 1969 行全文、`tools/reach-cli.ts`、`tools/reach-scale-cli.ts`、`tools/reach-fuzz-cli.ts`、`tools/lint-cli.ts`、`src/content/content.ts`、`src/content/script.ts`、`src/core/dice.ts`、`src/core/types.ts`、`test/reach-fuzz-lib.ts`、`test/reach-scale-lib.ts`、`tsconfig.json`、`package.json`）。未改动任何源码，未运行构建/测试。验收前提：reach 分析输出须与 TS 版逐位一致。

## 汇总表

| ID | 结论 | 一句话说明 |
|---|---|---|
| REACH-01 | VERIFIED | 分层切缝线与 walkSites↔forEachAuthoredOutcome 镜像契约全部属实（L205-227 vs content.ts:303，访问序逐点一致） |
| REACH-02 | PARTIAL | setSites 类型三处重复属实；但 siteProven 是 **17** 个参数非 18，且 notes/firstCross **在** WorldFacts 上（真正的例外是 avail/mintSources/fillers/triggerMintNames/cardsOf/site/content） |
| REACH-03 | VERIFIED | 四条 Map 序依赖全部成立；initNames 后写胜在引擎中局 State（同名 stackable mint）必现，非纯理论 |
| REACH-04 | VERIFIED（并加强） | Object.entries 三处属实；且 checkName 只查保留字、原型名语法 `(\S+) is KIND` **不拦数字开头名**，类整数键在脚本语言层面即可触达 |
| REACH-05 | VERIFIED | trigFlat 排序同键无关（L777 跳过）与 UTF-16 vs 字节序差异均属实 |
| REACH-06 | VERIFIED | 三类哨兵（MANY=1e9 / Infinity / '\u0000derived'）全部逐点核实，三性质描述准确 |
| REACH-07 | VERIFIED | 三个身份键控缓存属实；synth id 恒为 `~name`、aspectsOverride 可不同 → Rust 按 id 做 matchMemo 键会假命中，风险真实 |
| REACH-08 | PARTIAL | startSlot/cyclic 死代码、未用解构、void content 均属实；但 siteReason 的 `!initNames.size` 分支**可达**（无 entities 段的脚本），不是死代码，移植必须保留 |
| REACH-09 | VERIFIED | 全部重复点逐一数过：4×/3×/3× 助手调用、7 处致盲谓词、6 处 deck 展开、2 处 JSON.parse |
| REACH-10 | PARTIAL（**标红**） | 瓶颈观察属实，但"不动点单调、语义不变"**错误**：`count(X) <= k` 类门使 pathOpen 反单调，proven **集合**本身依赖测试日程，worklist 化可能改变输出 |
| REACH-11 | VERIFIED | 全部属实；一处需修正：mintSources 与 provenSources 谓词**不**完全相同（前者不做 servedByCap，因 deckCap 在不动点后才存在），统一索引须在查询期补该过滤 |
| REACH-12 | VERIFIED | 遮蔽、internal 不变式均属实（个别行号小瑕：L1930 处并无 `!`，其调用点有） |
| REACH-13 | VERIFIED | firstFreeze 键=值恒等，Math.min 无效操作，实为 Set——属实 |
| REACH-14 | VERIFIED | 整数域、双mod 取模、cap=4 公式、两处 LCG（fuzz L35-38 / scale L28-31）逐字核实；u32 wrapping 映射位精确 |
| REACH-15 | VERIFIED | FNV-1a64(UTF-8)+splitmix64+BigInt 掩码+降序 Fisher–Yates（content.ts:430-461、dice.ts:26-34）全部属实，u64 wrapping 可直接对应 |
| CLI-01 | VERIFIED | --demand 末位静默消失（L20）、parseQuery 未捕获 exit 1（reach.ts:1310）均属实 |
| CLI-02 | VERIFIED | 分支序、名字集、排序、快照文案均属实（小修正：--all 只收 deck.order，**不含** defaultcard） |
| CLI-03 | VERIFIED | spawn npx、每 worker 重建全量世界、tmpfile 无 run 区分、stdio 丢弃、Promise.all 拒绝——逐点属实 |
| CLI-04 | VERIFIED | 契约描述属实；runFuzzSeed 确为纯函数（模块无可变全局）；小瑕：其定义在 lib **L457**（报告引的 L19 是头注释），且 inconclusive 'soft' 行也走 stderr |
| CLI-05 | VERIFIED | readdirSync('scripts') 相对 CWD、头注释"恒 0"与缺失输入 exit 1 的矛盾属实 |

**总计：17 VERIFIED / 3 PARTIAL / 0 REFUTED。** PARTIAL 三条的核心主张均成立，仅在参数计数、死代码判定或单调性前提上需要修正；其中 REACH-10 的修正足以改变移植计划（见标红）。

---

## REACH-01 [modularity] — VERIFIED

**原声明要点**：reach.ts 四层关注点（census L146-454 / 静态谓词 L456-575 / world+fixpoint L577-1292，内含 L712-953 约 240 行 6 子规则风险冻结块 / analyzeCard+paths+render L1294-1968）；`walkSites`（205-227）是 `forEachAuthoredOutcome`（content.ts:303）的手工镜像，Rust 应共享同一遍历器。

**核实过程**：
- 行号逐段核对：站点普查段落起于 L137 注释/L146 `SegFlags`，止于 L454；静态谓词 L456-575；`computeWorldFacts` L625-1292；风险块 `{`在 L712、`}`在 L953（242 行）；`analyzeCard` L1297-1505，路径/显示辅助到 L1968。均与声明一致。
- 风险块 6 子规则可辨：初值碰撞（L732-737）、同槽互撞（L738-763）、跨槽时序（L764-782）、gated-branch（L793-815）、throwFrozen（L846-870）、slot-blocking/冻结豁免（L816-830、871-952）。
- 镜像比对：`forEachAuthoredOutcome`（content.ts:303-311）为 visit(o) → IF 则 arms 依序递归 → fallback 递归；`walkSites`（reach.ts:205-227）为 visit(o) → arms 依序递归（带 `nextSeg()` 新 seg）→ fallback 递归。除 seg 编号与 flag 收集外遍历形状完全一致；reach.ts L30-35 注释也明确以该穷举为 'none' 判定的健全性依据。

**结论**：成立。切缝线真实存在（各段均有分隔注释），镜像契约属实。

**修正表述**：无需修正。补充：镜像差异不止"多收集 flag"——`walkSites` 对每个 IF 臂分配新 seg 且 REFUSE/throwing 记在**所在 seg** 上，这是行为的一部分；共享遍历器时回调需同时供给两种消费形态。

**移植建议**：**must**（共享遍历器）；模块切分 should。

## REACH-02 [modularity] — PARTIAL

**原声明要点**：`siteProven` 有 18 个位置参数（L1638-1656）；其实参几乎都在 WorldFacts 上，"notes、mintSources、fillers、firstCross 等是例外"；collectSites 的 setSites 行类型在 L246、L258、L1645 三处逐字重复。

**核实过程**：数 L1638-1656 参数：content, site, avail, committable, initNames, setWrites, setSites, destroyedBy, frozen, freezeDist, notes, mintSources, fillers, dirtyOwners, firstCross, triggerMintNames, cardsOf = **17 个**。对照 WorldFacts（L587-613）：notes（L588）与 firstCross（L592）**在**接口上；真正的例外是 content、site、avail、mintSources、fillers、triggerMintNames、cardsOf。三处类型重复属实（L246 返回类型、L258 局部变量、L1645 参数）。

**结论**：主旨成立（超长参数表 + 三处重复类型，应收敛为 ctx struct / 命名类型），两处细节有误。

**修正表述**：17 参非 18；例外清单应为 avail/mintSources/fillers/triggerMintNames/cardsOf（构建期内部态）——notes/firstCross 反而已在 WorldFacts 上。Rust 落法：`WorldFacts` 已是天然宿主，把 avail/mintSources/fillers/triggerMintNames 也挂上去（或一个 `FixpointCtx` 内部 struct），`siteProven(&Ctx, site)` 两参。

**移植建议**：should（可读性/防错位）；不改行为。

## REACH-03 [semantics] — VERIFIED

**原声明要点**：initNames 后写胜（L667-671）；avail 插入序 → slotSupport 候选序（L1017-1027）→ 首适配 assign（L1032-1046）→ chain/paths byte-stable；proven 序 = 证明次序，provenSourceFor 首命中（L1152-1157）、demand/名字模式按 proven 序（L1327、L1384）；其余 Map 多为集合判断可放宽，L732 处理序需逐个核对。

**核实过程**：
- L667-671：`initNames.set(e.name, e)` 对 State.entities 迭代序后写胜，无误。补充事实：**初始** compileScript 的 State 不可能有重名活实体（`addEntity` types.ts:147-160 对重名非全 stackable 直接 throw；全 stackable 时 id=`s_name` 相同、Map 覆盖），所以后写胜只在**引擎中局 State**（同名 stackable mint 的多个 id 不同副本）生效——fuzz/scale 正是拿这类 State 调 analyzeCard，依赖真实可达。
- L1018 `for (const facts of avail.values())` 按插入序展开 `name#copy`（L1022-1023），assign（L1032-1046）首适配取候选序中首个可行组合；`how` 文本进入 supports/fillers → 输出字节。
- L1152-1157 首命中；L1327（demand）、L1384（名字）遍历 `proven`，首条定 chain、全序定 paths。
- 其余 Map 逐一核对：`byList` 序 → `risky`（L731-737）→ `riskyOwners` 仅做 `has`（L902/914/965）；`trigBySlot`/`mintCounts`/`mintSources`/`deckCap` 均只做成员判断或 get——与"可放宽/需核对"的 hedge 一致。

**结论**：成立，是全文件最重要的确定性依赖。Rust 必须用 IndexMap/IndexSet 或显式序号复刻：initNames（按 State.entities 序）、avail（按 push 序）、proven/committable（按证明序）、State.entities 本身。

**修正表述**：bullet 1 补一句"该序在含同名 stackable mint 的中局 State 上生效；初始 compileScript State 因 addEntity 的重名约束不会出现"。

**移植建议**：**must**；并配"同一 (content, State) 两次分析逐字节一致 + 跨语言 golden 对照"。

## REACH-04 [semantics] — VERIFIED（加强）

**原声明要点**：编译期 Map 由 Object.entries 构建（content.ts:204、218、274）；JS 类整数键先于字符串键按数值序枚举；当前 E0001/V0 前缀规避；Rust serde_json 默认不保序、preserve_order 也与 JS 类整数键重排不一致；须固定键序或加 load 校验。

**核实过程**：三处 Object.entries 属实（L204 stackable 集、L219 verbs、L274 prototypes）。**加强证据**：`checkName`（script.ts:198-202）只查保留字；原型行语法 `/^(\S+)\s+is\s+.../`（script.ts:205、258-259）对名字无 `[A-Z]` 开头约束——`123 is item` 是合法原型行。即类整数键**不是理论，脚本语言层面即可触达**（verb 名另受 `dict.isVerb` 约束，是次暴露面）。serde_json 默认 `Map` 为 BTreeMap（按键排序，同样非 JS 序），preserve_order 用 IndexMap 保文本序——与报告描述一致。

**结论**：成立且比原报告更严重。

**修正表述**："当前内容用前缀规避"应改为"捆绑内容规避了，但语言不设防"。另外建议中的"load 期校验（拒绝类整数键）"本身是**行为变更**（TS 接受这类脚本）；要保语义只能在 Rust 读取处显式复刻 JS 的 OrdinaryOwnPropertyKeys 序（先类整数升序、后字符串插入序）。

**移植建议**：**must**（键序复刻或显式重排）；"校验拒绝"方案应降级为需显式签字的行为分歧。

## REACH-05 [semantics] — VERIFIED

**原声明要点**：`trigFlat.sort((x,y)=>x.c-y.c)`（L774）依赖稳定排序但结果与同键序无关（L777 跳过相等 c）；reach-cli `--all` 的 `[...names].sort()`（reach-cli.ts:73）是 UTF-16 码元序，Rust 字节序仅在非 BMP vs U+E000..FFFF 区间不同。

**核实过程**：L774、L777 属实；对 (i,j) 对循环，标记只落在 c 较大的一方（L780 `risky.set(trigFlat[j]...)`），相等 c 跳过，结果确为同键序无关（稳定性非依赖）。L73 属实。UTF-16 vs UTF-8 字节序（=码点序）差异描述准确： supplementaries 在 UTF-16 下排于 U+E000..FFFF 之前、在 UTF-8 下排后。

**结论**：成立。

**修正表述**：无需修正。

**移植建议**：trigFlat **ignore**（不依赖稳定性，照抄数值比较即可）；`--all` 排序 **must** pin（比较器按 UTF-16 码元，或加 ASCII 断言）。

## REACH-06 [semantics] — VERIFIED

**原声明要点**：`MANY=1e9`（L135）参与权重帽（L645）、capacity 导出门（L1231）、servedByCap（L1929-1934）；`Infinity` 用于 freezeAt/freezeDist（L888、L971）与 firstCross 比较（L909-911、L1178）；`'\u0000derived'` 哨兵键（L1336、1341、1878-1888、1902-1907）三性质。

**核实过程**：全部行号逐一命中。L645 `Math.min(f.count, 32)`；L1231 `site.cap !== MANY` 门；servedByCap L1929-1935；L888/L971 的 `: Infinity`；L910 `fc < freezeAt`、L1178 `firstCross(t.slot) < freezeDist`；哨兵键四处命中（draw/demand 盲名 L1336/L1341，addProduct L1878-1891/L1902-1910，实际区间略宽于引用）。三性质核实：占 avail/byName 一个键（push L1878/L1906）、槽查询可中（描述符 kind/aspect 完好）、`exists(NAME)`/`count(name:)` 门不中（byName.has(裸名) 假）。

**结论**：成立。

**修正表述**：无需修正。

**移植建议**：**must** 逐一对应（MANY 用 i64、Infinity 用 Option<i64>/i64::MAX 保比较方向、哨兵键用枚举但保三性质）。

## REACH-07 [portability] — VERIFIED

**原声明要点**：trigIndexCache（L233）、matchMemo（L487）、worldCache（L615）三个身份键控 WeakMap 缓存；"调用方不得变异"契约仅内部注释（L580-586）；synth 实体 id 恒为 `~name`（L1879/L1907），aspectsOverride 不同仍同 id，Rust 缓存键须含 aspects。

**核实过程**：三处缓存与注释行号属实（契约注释实际 L579-586）。`synth`（L688-697）`id: '~${name}'` 属实。撞键场景真实存在：universe 同时装入"纯原型描述符 `synth(name, kind, p)`"（L1088）与"aspectsJson 覆盖的站点描述符 `synth(..., override)`"（L1102），同名不同 aspects、同 id；JS 靠对象身份分键，Rust 按 id 键 memo 必假命中（matchQuery 是纯函数，键必须含全部影响取值的字段）。

**结论**：成立，是该文件重构风险最高的一条。

**修正表述**：无需修正。

**移植建议**：**must**。Rust 落法：matchMemo 键 = (Query 值, Entity 的完整描述符值含 aspects) 或干脆不做 memo 先朴素移植（matchQuery 本身便宜）；worldCache 用显式 `Analyzer<'c>` 持有 (content, initial)。

## REACH-08 [dead-code] — PARTIAL

**原声明要点**：外层 startSlot/cyclic（L677-678）被块内同名（L769-770）遮蔽且 cyclic 全文无调用；analyzeCard 解构未用 destroyedBy/freezeDist/dirtyOwners（L1304-1305）；siteChain 的 content 参数未用（void content L1953）；siteReason 的 `site.verb && !initNames.size`（L1964）疑似陈旧。

**核实过程**：grep 全文：`cyclic` 仅 L678/L770 两处定义、零调用；`startSlot` 仅 L677/L769 两处定义（外层无其他引用）——外层与内层两对均死。tsconfig 无 `noUnusedLocals`，属实。analyzeCard 体内（L1297-1505）grep destroyedBy/freezeDist/dirtyOwners 仅命中解构行本身。`void content` L1953 属实。**但**：compileScript 不自动注入任何实体（state 仅由 `script.entities` 构造，script.ts:429-437；`player:` 只是校验不是注入），`entities:` 段可省略——零实体脚本合法，此时 `initNames.size === 0`，该分支**可达**。

**结论**：前三项死代码属实；第四项不是死代码，是退化输入下的活分支。

**修正表述**："疑似陈旧条件，应复查"应改为"仅对零实体初始世界触发；语言允许零实体脚本，移植必须保留该分支，否则该退化输入下输出改变"。

**移植建议**：死代码删除 **should**（删前跑差分）；siteReason 分支 **must 保留**。

## REACH-09 [duplication] — VERIFIED

**原声明要点**：站点三元链重复调用助手（L320-322）；致盲谓词 7 处（L991、1192、1389、1407、1433-1441、1877、1902）；deck 名展开 6 处（L369-371、389-391、418-420、722-724、987-988、1328-1336）；JSON.parse try/catch 两处（L1099、L1549）。

**核实过程**：L320 spawnNameRef×4、L321 replaceWithRef×3、L322 drawDeckId×3（每次重走 o.args）；7 处致盲谓词逐一命中；6 处 deck 展开逐一命中；两处 try/catch 命中。

**结论**：成立。纯抽取重构不改行为（前提：`siteMintsBareName` 之类谓词按调用点原谓词逐字搬运，不"顺手统一"语义）。

**修正表述**：无需修正。

**移植建议**：should（Rust 侧顺手做，零风险）；deck 展开抽迭代器注意六处谓词细节不同（addMint/stacky/coMints/mintSetOf/recordMintSource/demand-descs 的过滤各异，抽"枚举"不抽"过滤"）。

## REACH-10 [performance] — PARTIAL（**标红**）

**原声明要点**：`while (changed)`（L1061-1078）每轮对全部未证明 verb 重跑 slotSupport、全部未证明 site 重跑 siteProven；建议 worklist/事件驱动；声明"语义不变（不动点单调……改变测试次序会改变 proven 插入序……须保持先到先证明的语义或显式排序）"。

**核实过程**：
- 瓶颈属实：L1061-1078 每轮全量重测；`exprExistsNames`（L1575）/`writablePaths`（L568）确实可用于建依赖倒排。
- **单调性前提错误**：`evalStatic` 的 cmp 分支（L513-518）对 `countQuery` 操作数在 avail 增长下——`>=`/`>` 保真（单调），但 `<=`/`<` **反单调**（真→假可翻转），`=`/`!=` 双向非单调。`pathOpen`（L527-529）要求全部路径谓词为真，因此一个站点在第 k 轮通过、在第 k+1 轮（世界增长后）会**不通过**；而 TS 的 proven 一经 set 永不移除。语言层面 `CmpOp` 含全部六种（query.ts:7），`when count(name:X) <= 1` 式门合法可创作。**故最终 proven 集合依赖测试日程，而不仅是插入序**。
- 后果一（集合）：事件驱动 worklist 把"依赖变化才重测"的站点测得**比 round-robin 晚**；对反单调门，round-robin 在低水位轮次抓住的证明，worklist 会永久丢失 → verdict 不同。
- 后果二（序）：即便集合相同，worklist 的证明完成序 ≠ live 序 round-robin 的证明序，`proven` 插入序变 → chain 首条与 paths 排序变（REACH-03 已证其是一等输出依赖）。原报告的 hedge 只覆盖了后果二。

**结论**：瓶颈观察成立；"语义不变"不成立。**此优化若按建议的事件驱动方式实现，可能改变输出，移植计划必须标红。**

**修正表述**："不动点单调"改为"对 `>=`/`>`/exists 门单调，对 `<=`/`<`/`=`/`!=` 的 count 门**非**单调；fixpoint 解日程依赖"。安全结论：**朴素 round-robin（verbs 按 content.verbs 序 → sites 按 live 序，逐轮至无变化）是唯一已知逐位一致的日程**；worklist 只有在完整复刻该日程（等价于带跳过表的 round-robin）时才保序保集合，收益趋零。

**移植建议**：**must 朴素移植**；worklist 化列入"禁止在差分保护下想当然"清单（见文末准入判据）。

## REACH-11 [performance] — VERIFIED

**原声明要点**：provenSourceFor/provenSources 线性扫（L1147-1159、L1189-1199）；pathFor 每需求名每节点调 provenSources（L1255）；mintSources（L985）与之谓词重复、只含裸名；canEverMatch 已 memo（L1113-1128）；pathFor 的 `new Set([...seen, n.name])`（L1256）。

**核实过程**：全部命中。一处不精确：`mintSources` 的收录谓词（L991）跳过致盲名，但**不做 servedByCap**（deckCap 在不动点结束后才构建，L1169-1182）；`provenSources`（L1194）带 cap 过滤。两者不是同一谓词，"统一为一个名字索引"须把 cap 过滤留在查询期。

**结论**：成立，附谓词差异修正。

**修正表述**：按名字建索引时：per-name 站点表按证明序追加（与遍历 `proven` 同序），查询期再套 `servedByCap(deck, name, deckCap[...])`，才与现实现逐位一致。

**移植建议**：索引化 **should**（差分保护下可做，序保持即可）；`new Set` 展开 **ignore**（深度帽 12 下无碍，照抄最稳）。

## REACH-12 [clarity] — VERIFIED

**原声明要点**：demand 分支 `const paths = writablePaths(demandQ)`（L1367）遮蔽外层 paths（L1301）；非空断言密布（L1322、1381、1860、1930）；两处 `throw new Error('internal: …')`（L505、521）。

**核实过程**：遮蔽属实（L1301 `paths: ReachPath[]` vs L1367）。L1322/L1381 的 `provenSourceFor(...)!`、L1860 的 `site.deck!` 属实；**L1930 无 `!`**（servedByCap 体内无断言；其调用点 L1154/L1194/L1348/L1392 才有）——一处行号小瑕。L505/L521 属实。

**结论**：成立。

**修正表述**：行号 L1930 → L1154 等 servedByCap 调用点。

**移植建议**：should（`!`→`expect` 逐个登记来源；改名 paths→demandWritePaths）。

## REACH-13 [clarity] — VERIFIED

**原声明要点**：`firstFreeze.set(d, Math.min(firstFreeze.get(d) ?? d, d))`（L878、L885）键即值，Math.min 无效，实为 Set。

**核实过程**：两处命中；`d = firstCross(t.slot)` 与键相同，值恒为 d；`freezeAt = Math.min(...firstFreeze.keys())`（L888）是唯一消费点。

**结论**：成立。Rust 用 `HashSet<i64>` + `min(keys)`（空则"无冻结"哨兵）逐位等价。

**修正表述**：无需修正。

**移植建议**：should（照 Set 化，行为零差异）。

## REACH-14 [portability] — VERIFIED

**原声明要点**：全整数域；firstCross 双 mod（L684-687）负数安全、JS/Rust % 同为截断；slotSupport cap=4 与 `Math.min(count===MANY?cap:count, cap)`（L1015-1023）逐字保留；两个 LCG（fuzz L35-38、scale L28-31）`Math.imul`+`>>>0` → Rust u32 wrapping。

**核实过程**：全部命中。firstCross 实为 `((s*30 - now) % 1440 + 1440) % 1440` 再 `d===0?1440:d`（后者也是语义一部分，移植勿漏）。LCG 两处逐字一致：`s=(seed^0x9e3779b9)>>>0; s=(Math.imul(s,1664525)+1013904223)>>>0`。位精确性论证成立：Math.imul 是 32 位环绕乘，其有符号结果 + 1013904223 在 f64 中精确，`>>>0` 的 ToUint32 取模后与 u32 wrapping mul-add 完全一致。`r()/0x100000000` 与 `Math.floor(r()*n)` 需按 f64 复刻。

**结论**：成立。

**修正表述**：补"firstCross 的 `d===0 → 1440` 分支勿漏"。

**移植建议**：**must**（逐字对应 + 逐种子 golden 对照）。

## REACH-15 [portability] — VERIFIED

**原声明要点**：DRAW 事实建立在 content.ts:428-461 的 permutePositions（FNV-1a 64 UTF-8 + splitmix64 + BigInt 掩码 Fisher–Yates）上；Rust u64 wrapping 直接对应；建议 golden deck 序列对照。

**核实过程**：`hashDeckId`（content.ts:430-437）：偏移 0xcbf29ce484222325、素数 0x100000001b3、`& (2^64-1)`、`TextEncoder` UTF-8 字节——Rust 对应 u64 wrapping + `id.as_bytes()`。`permutePositions`（453-461）：`BigInt.asUintN(64, BigInt(seed) ^ hash ^ BigInt(i))` → `splitmix64(key) % (i+1)` → 降序 Fisher–Yates 交换；`splitmix64`（dice.ts:26-34）全 BigInt 掩码（GOLDEN_GAMMA/MIX1/MIX2 与移位）。u64 wrapping + `(a^b^c)` 逐步对应无误；`Number(x % BigInt(i+1))` 精确。

**结论**：成立，"最硬的一项"判断合理。

**修正表述**：无需修正。

**移植建议**：**must**（u64 wrapping + JS 侧 golden deck 序列差分测试）。

## CLI-01 [semantics] — VERIFIED

**原声明要点**：`--demand` 为末位 argv 时 `args[demandIdx+1]` undefined（L20），demand 静默消失（有卡名则按名字模式跑）；parseQuery 异常编译后才抛（reach.ts:1310），未捕获 exit 1 带堆栈。

**核实过程**：L20 属实；rest 循环 `i++` 越界安全；L30 usage 门 `(!card && !all && !demand)`——demand 丢失且无卡名时反而 exit 2，有卡名时静默按名字模式，与描述一致。reach.ts:1310 `parseQuery(opts.demand)` 在 compileScript（CLI L36）之后，CLI 无 try/catch。另核实：`--demand "<q>"` 无卡名时 `report(card!)` 的 card 为 undefined，analyzeCard 因 demandQ 优先而忽略 target——靠巧合工作。

**结论**：成立。

**修正表述**：无需修正。注意：建议的"校验前移 exit 2 / 单行错误"是**可观察 CLI 行为变更**（现状是静默降级/堆栈），应作为显式分歧签字，而非悄悄改。

**移植建议**：核心报告输出 **must 保真**；错误路径改进 should（需显式记录为有意分歧）。

## CLI-02 [semantics] — VERIFIED

**原声明要点**：`--all` 与卡名同给时卡名被忽略（L69-76 分支序）；名字集 = prototypes ∪ 活初始实体 ∪ deck 卡（L70-72）；renderPath 缩进/文案是验收快照格式（L44-66）；slotProofLines 注释要求 byte-identical（reach.ts L1919-1924）。

**核实过程**：全部属实。一处收紧：L72 只加 `d.order` 的名字，**不含 defaultcard**——报告"deck 卡"略宽。slotProofLines 的 byte-identical 注释在 L1917-1918。

**结论**：成立。

**修正表述**：名字集精确为 prototypes ∪ 活初始实体 ∪ deck.order（无 defaultcard）。

**移植建议**：**must**（golden 冻结 stdout 全部文案含缩进）。

## CLI-03 [portability] — VERIFIED

**原声明要点**：worker 经 `spawn('npx', ['tsx', process.argv[1]!…])`（L43）；每 worker 重建整份 CS 世界；IPC 靠 `tmpdir()/reach-scale-${i}.json`（L30、50-51）无 PID/run 区分，并发互踩；worker stdout 丢弃、stderr 透传（L44）；Promise.all 失败未处理拒绝；`--worker i k P R` 分片 `idx % k === i` 与 NameStat JSON 是 A/B 对照接口。

**核实过程**：逐点命中（L24-32 worker 模式、L27 每 worker `buildCsScale`、L28 分片、L30 写文件、L42-46 spawn/stdio/Promise.all、L50-51 读+删）。

**结论**：成立。

**修正表述**：无需修正。

**移植建议**：Rust 线程+channel 应该做；**must** 保留 `--worker i k P R` 语义与 NameStat 字段（name/ms/verdict/existence/roots/nodes/depth）作为跨语言 A/B 接口。

## CLI-04 [clarity] — VERIFIED

**原声明要点**：flags 默认 200/7/400/1；FAIL→stderr、进度/汇总→stdout；`runFuzzSeed(seed)` 纯函数（引 lib:19）是种子可复现根基；退出码 1 iff 有 failure、坏种子空格列表（L34）；Date.now（L23）vs performance.now（scale L34）不一致但无害。

**核实过程**：CLI 侧全部命中（L13-16、L28、L31、L33-35）。`runFuzzSeed` 定义在 **lib L457**（报告引的 L19 是头注释里对该函数的描述）；其体（L457-507）与 `genScenario`（`rng(seed)` 局部）均无模块级可变状态，纯函数成立。小补充：inconclusive 的 `soft` 行走 console.warn（L30）也在 stderr。

**结论**：成立，两处引用级小瑕。

**修正表述**：runFuzzSeed 位置 L457；stderr 除 FAIL 外还有 soft 行（若 CI 按"stderr 仅 FAIL"解析需知道这点）。

**移植建议**：**must** 保持"每种子自带 LCG、无全局状态、流分离、退出码语义"；计时器差异 ignore。

## CLI-05 [semantics] — VERIFIED

**原声明要点**：缺省 `readdirSync('scripts')` 相对 CWD（L16）；文件/目录缺失未捕获 exit 1，与头注释"Exit code stays 0"（L2-3）矛盾。

**核实过程**：逐点命中；readFileSync（L20）对缺失文件同样抛。

**结论**：成立。

**修正表述**：无需修正。与 CLI-01 同样的注意点：把"输入不存在"改为显式退出码是行为分歧，需签字。

**移植建议**：should（语义整理）；现状行为若作为 golden 对照基线则先照抄再改。

---

## 计划必须吸收（must）

1. **Map 序纪律**（REACH-03/04）：IndexMap/IndexSet 或显式序号复刻 initNames（State.entities 序、后写胜）、avail（push 序）、proven/committable（证明序）、State.entities 本身；编译期 `Object.entries` 的 JS 类整数键重排要么复刻（OrdinaryOwnPropertyKeys）要么显式重排——"load 校验拒绝"是行为变更，须签字。
2. **walkSites ↔ forEachAuthoredOutcome 单一遍历器**（REACH-01）：'none' 判定健全性所系。
3. **deck 置换与 LCG 位精确**（REACH-14/15）：u64/u32 wrapping 逐步对应 + JS golden（deck 序列、逐种子场景）。
4. **哨兵与时序冻结模型逐字对应**（REACH-06/13）：MANY、Infinity 比较方向、'\u0000derived' 三性质、firstCross 的 `d===0→1440`、cap=4 展开公式。
5. **不动点朴素 round-robin 移植**（REACH-10 标红）：verbs（content.verbs 序）→ sites（live 序）逐轮循环，禁止直接改事件驱动。
6. **缓存重构的键安全**（REACH-07）：matchMemo 键须含 aspects（或先不做 memo）；worldCache 改显式 Analyzer。
7. **输出快照冻结**（CLI-02）：reach-cli 全部 stdout 文案逐字节 golden；`--all` 排序 pin UTF-16（REACH-05）。
8. **siteReason 的 `!initNames.size` 分支保留**（REACH-08 修正）：零实体脚本可达。
9. **验收工具契约保留**（CLI-03/04）：`--worker i k P R` 分片、NameStat 字段、runFuzzSeed 纯函数、退出码语义。

## 应驳回（或降级）

1. **REACH-10 的"语义不变"声明**：驳回。fixpoint 对 `<=`/`<`/`=`/`!=` count 门非单调，proven 集合日程依赖；事件驱动 worklist 不保证逐位一致，必须先朴素移植。
2. **REACH-02 的"18 参数"与例外清单**：降级为 17 参；notes/firstCross 实在 WorldFacts 上（真例外：avail/mintSources/fillers/triggerMintNames/cardsOf）。
3. **REACH-08 的 siteReason"疑似死代码"**：驳回死代码判定（零实体脚本可达），改为"必须保留"。
4. **REACH-04 的"load 期校验"建议**：降级——拒绝类整数键名本身改变 TS 已接受输入的行为。
5. **REACH-11 的"谓词重复"**：降级为"部分重复"（mintSources 无 servedByCap，deckCap 不动点后才存在）。
6. 引用级小瑕（不改结论）：CLI-04 的 runFuzzSeed 行号（L19→L457）、REACH-12 的 L1930、CLI-02 的 deck defaultcard 未入 --all 名字集、CLI-04 的 soft 行也在 stderr。

## 性能优化准入判据

**原则**：任何优化的准入证物 = 朴素移植版与 TS 版先逐位对齐（fuzz 全种子 + reach-cli golden + scale 抽样），之后每个优化在**同一差分套件**下重跑；输出一个字节不同即回退。

**A 类：差分保护下可做（不触碰不动点日程与 proven 序）**
- provenSourceFor/provenSources 按名字索引（REACH-11）——前提：per-name 表按证明序追加、servedByCap 过滤留在查询期。
- canEverMatch/matchMemo 值键 memo（REACH-07/11）——键含完整描述符（含 aspects）。
- pathFor 的 seen 集改持久化结构（REACH-11）——遍历序不变。
- REACH-09 的纯抽取、REACH-13 的 Set 化、REACH-02 的 ctx struct、REACH-12 的改名——语义保持的重构。
- flatCache 增量 push 的 Rust 对应——照 TS 现有算法搬运即可。

**B 类：必须先朴素移植、之后仅在等价性证明下尝试**
- **不动点 worklist/事件驱动（REACH-10）**：除非能证明（a）依赖索引覆盖 siteProven 的全部输入，（b）日程与 round-robin 逐测试等价（等价于带跳过表的 round-robin，收益趋零），否则禁止。反单调 count 门使"晚测"丢证明。
- **世界视图重建/缓存换洗**（flatCache 的替代形态）：权重语义（min(count,32)）与 push 序任何改变都进入 slotSupport 候选序。
- 触发器两两配对（O(T²) 热点）的剪枝：剪枝谓词必须是被保守证明的恒等谓词，且 risky/dupRisk 的**置位集合**逐位不变（消费点全是成员判断，集合不变即安全——这条可以差分验证）。

**C 类：与输出无关、随时可做**：进程拓扑（CLI-03 线程化）、计时器、错误路径整理（须作为显式 CLI 行为分歧记录）。
