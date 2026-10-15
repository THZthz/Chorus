# 02 — engine/ 与 content/ 分析报告的独立核实（rust-port）

核实对象：`docs/rust-port/analysis/02-engine-content.md`（22 条 ENG-NN finding）。
核实方式：逐条打开被引 file:line，对照本 worktree 源码与测试（只读；未运行构建/测试）；对每条判断 (a) 证据真实性 (b) finding 是否成立 (c) 建议是否保持行为语义。
行号核对基准：本 worktree（engine.ts 1298 行、content.ts 592 行、script.ts 620 行、fold.ts 17 行、store.ts 32 行、parse.ts 853 行、format.ts 107 行——报告概述中的行数全部准确）。

## 汇总表

| ID | 结论 | 一句话说明 |
|----|------|-----------|
| ENG-01 | VERIFIED | 编译后突变、三个构建点、entities 参数只喂 deck 全部属实；建议（并入 compileContent）行为保持。 |
| ENG-02 | PARTIAL | 截断式 undo 与 initialState 生产不可达属实；但"4 个测试文件"实为 2 个文件 4 处调用（decks.test.ts ×2、dice.test.ts ×2）。 |
| ENG-03 | VERIFIED | complete/event 两分支的 ApplyCtx 字面量确系同构复制，`kind==='event'` 连续两次（1002/1004）属实；合并仅限 ctx 构造，分支其余差异必须保留。 |
| ENG-04 | VERIFIED | 四组顺序敏感点全部真实（含 formatArg pair 逐字节进句子）；修正：byName 外层映射从未被迭代，只有池数组需保序。 |
| ENG-05 | VERIFIED | BigInt 链路、负 seed 二补码、FNV-1a、独立向量全部属实；补充：`(sides*r)>>64` 在 Rust 需 u128 中间量，非纯 u64 wrapping。 |
| ENG-06 | VERIFIED | registerWindows 的 sentence→parse 往返（351 行）与 fold 共用（1027 行）、失败静默不开窗口全部属实；"853/107 行模块"= parse.ts/format.ts 确认。 |
| ENG-07 | VERIFIED | 六个 JS 值语义渗透点逐行确认（含 `state:!x` 即 `v!==true`、`startMin ?? `、`===` 类型不等即 false）。 |
| ENG-08 | VERIFIED | SPAWN/REPLACE 的 JSON 字符串 pair、`delete attrs['container']`、日志为唯一持久化格式全部属实。 |
| ENG-09 | VERIFIED | 三份手写副本、三种错误文案、mintIsStackable 注释均属实；测试确以精确文案断言（decks.test.ts:573 钉死整句），全测试 192 处 toThrow（"150+"成立）。 |
| ENG-10 | VERIFIED | 六组顺序锚点逐条对上源码；parallel.test.ts 存在可作模板。 |
| ENG-11 | VERIFIED | 拒绝时 `records:[record], state: stateIn`（257-261）、id 让给拒绝行、重_roll 怪癖注释（443-447）、foldUpTo 过滤（fold.ts:16）全部属实。 |
| ENG-12 | VERIFIED | wait 行唯一动钟 + 连续性断言（956-963）、complete 锚定 w.start（975-978）、SET expires（619）、ADVANCE 落 wait 跨度（1085-1086/1099-1100）全部属实。 |
| ENG-13 | VERIFIED | ended 硬门（154-156）、首 ENDING 生效（735）、编译期校验、flavour 开放词表全部属实。 |
| ENG-14 | VERIFIED | 排列纯函数、游标在 draft（825-826）、DRAWN 折叠断言（844-871）、耗尽规则（472-477）全部属实。 |
| ENG-15 | VERIFIED | 漂移真实（0 基 vs 1 基，编译侧被 script-play.test.ts:175 钉死为 `study.recipe0`）；但"顺便修掉"是行为变更——**移植时保持原状，修复另立议题**。 |
| ENG-16 | VERIFIED | role 强转无校验、typo 槽位静默失效属实（正则在 script.ts:171，报告引 172 为同行检查的 throw 行，差一行不影响实质）；编译期拒绝是行为变更——**移植时保持原状**。 |
| ENG-17 | VERIFIED | 五种重复声明处理不一致 + 大小写 id 碰撞链路（431/138/types.ts:159）全部属实；统一为报错是行为变更——**移植时保持原状**。 |
| ENG-18 | VERIFIED | 四个死函数（grep 全仓零调用/仅测试用）、不可达回退、恒真条件、`i=i`/`void i` 全部属实；不搬是行为保持的（无任何调用方）。 |
| ENG-19 | VERIFIED | 1239/1279 两处 `*30` 绕过常量属实（补充：1287 行 `clockFromAbsMin(absSlot * 30)` 是第三处）；meta else 落 parseClock、lintScript 双重解析属实；meta 枚举化需保留现有错误文案。 |
| ENG-20 | VERIFIED | node:fs 唯一出现点、全 src 无 Math.random/Date.now（grep 空）、非原子写、无行号全部属实；ActionRecord 确为纯 plain object，serde 可直映射。 |
| ENG-21 | VERIFIED | sortRefs 用 JS `<`（UTF-16 码元序）、play.ts:238 localeCompare 仅 UI、Rust UTF-8 字节序分歧面描述全部准确。 |
| ENG-22 | VERIFIED | 脚本=数据、正则行段全对上、examples/ 恰 2 个 .ts、test/engine.test.ts:1002 确为 toThrow、0 基 recipe id 被测试钉死——结论与理由全部成立。 |

**总计：21 VERIFIED / 1 PARTIAL / 0 REFUTED。**

---

## ENG-01 [modularity: medium] CompiledContent 编译后被 compileScript 原地突变

**原声明要点**：script.ts:426-428 直接改已编译对象的 stackable 集合；prototype 集合在 content.ts:202-206 编译期构建；lintScript 第三次复刻（script.ts:545-552）；compileContent 的 `entities` 参数（content.ts:194）只用于 deck 解析。

**核实过程**：逐一打开四处。script.ts:426-428 确为 `for (const e of script.entities) { if (e.stackable) { content.stackableNames.add(...); content.stackableKinds.add(...) } }`，作用于 compileContent 的返回值（423 行）。content.ts:202-206 构建 prototype 侧集合；`entities` 参数在 compileContent 体内唯一使用点是 217 行 `compileDecks(..., entities)`。lintScript 在 544-552 行自建第三份集合。三份集合的成员语义一致。

**结论**：VERIFIED。

**修正表述**：无需修正。补充一点：Set 成员判断与插入顺序无关，故把 entity 侧集合构建搬进 compileContent（在 prototype 循环之前或之后均可）不改变任何 `has()` 结果——建议严格行为保持。

**对移植计划**：**should**。Rust 中 `compileContent` 直接接收 entities 并一次性构建两个集合，结构上消除突变点；无行为风险。

---

## ENG-02 [modularity: low] Log 的 append-only 不变量由调用方维护

**原声明要点**：play.ts:164 `this.log.records.length = length` 直接截断；`Log.initialState`（store.ts:9）生产代码从不读，"只有 4 个测试文件用（decks.test.ts:431 等）"。

**核实过程**：play.ts:164 确认（undo 路径 161-167）。grep `initialState`：src 内仅 store.ts:9（构造器存储），生产零读取；测试命中 4 处——decks.test.ts:431、decks.test.ts:531、dice.test.ts:669、dice.test.ts:721，即 **2 个文件 4 处调用**，不是"4 个测试文件"。

**结论**：PARTIAL（实质成立，证据中文件计数有误：2 文件 / 4 调用点）。

**修正表述**：把"4 个测试文件"改为"2 个测试文件中的 4 处调用"。其余（append-only 由调用方维护、initialState 是测试夹具）准确。

**对移植计划**：**should**。Rust Log 提供 `truncate(len)`；initialState 移出 Log 或标注 `#[cfg(test)]`/测试夹具。均为内部 API 设计，无行为影响。

---

## ENG-03 [duplication: low] applyRecord 的 complete/event 两分支近乎复制

**原声明要点**：engine.ts:980-995 与 1006-1021 同构；`if (record.kind === 'event')` 连续两次（1002、1004）。

**核实过程**：complete 分支的 ApplyCtx 字面量在 980-994，outcomes 循环 995；event 分支的 ApplyCtx 字面量 1005-1020，循环 1021。两处 ctx 除 `now` 来源外逐字段相同（bindings 展开、idGen 闭包 `r${record.id}s${++n}`、extraAdvanceMin: 0）。1002 行 `state.fired.add`、1004 行分支体——连续两次 kind 判断确认。

**结论**：VERIFIED。

**修正表述**：注意两分支并非完全可合并——complete 有 window 查找/删除（975-976）、REFUSE 早退（977）、startMin 来自窗口表（978）；event 有 fired.add（1002）与 tGame 锚定（1003）。可安全共享的只是 **ctx 构造**（一个 `makeFoldCtx(record, state, startMin, n) -> ApplyCtx`），分派骨架必须保留。

**对移植计划**：**should**。Rust 抽取共享的 ctx 构造函数即可；合并过度会引入行为风险。

---

## ENG-04 [portability: high] Map/对象插入顺序被行为依赖

**原声明要点**：四处真实顺序敏感点——过期快照序（1065-1067 → fireSlot 1238-1271 → 事件记录直接进日志）；busyReason 首个匹配（280/290）决定拒绝文案；`JSON.stringify(aspects/attrs)` 键序逐字节进句子（511-525、585-589 + format.ts:91）；编译期 `Object.entries` 序与 `.recipe${i}`（204/208/219/274/276、229）。对照项：resolveDue 全序排序（1073）、fired 仅成员判断、existsQuery/countQuery 顺序无关（predicate.ts:446/484）。

**核实过程**：
- 1065-1067：`[...state.entities.values()]` 过滤出 expiring，数组序 = entities Map 插入序；fireSlot 1238 按 `for (const exp of expiring)` 顺序生成 event 记录并 push 进日志——同一 halfhour 多实体到期时记录顺序确由 Map 插入序决定。属实。
- 280/290：`for (const w of state.windows.values())` 返回首个匹配的文案。属实；爆炸半径窄——同一 verb 双窗口被站点不变量阻止，单卡双窗口仅在单步多子句提交同一卡时可能出现（busyReason 在 Phase 4 注册窗口之前运行，挡不住同步内重复），但"顺序敏感"的定性成立。
- 511-525/585-589 + format.ts:91：pair 的 `v` 是 JSON 字符串，`formatArg` 直接 `quote(a.v)` 嵌入句子——aspects/attrs 的对象键插入序逐字节进 journal 与日志行。aspects 由 aspectsFromPairs 按声明序插入（query.ts:102-114），键非整数样序，JSON.stringify 保插入序。属实。
- content.ts 各 Object.entries 与 229 行 `.recipe${i}`（0 基，i 为 recipes 数组下标）确认。verbs Map 迭代序还影响 play.ts suggestions 的遍历序与编译期报错顺序。
- 对照项全部属实：1073 行显式 `(end, beginId, clause)` 排序；fired 只有 has/add（1002/1231/1235/1269，无迭代）；predicate.ts:446（existsQuery）/484（countQuery）只做存在/计数。

**结论**：VERIFIED。

**修正表述**：一处范围收紧——`State.byName` 的**外层映射在 src 内从未被迭代**（只有 get/set/delete 与池数组 push/splice/indexOf），行为只依赖**池数组的插入序**（oldestNamed 取头部）。Rust 中 `HashMap<String, Vec<String>>`（Vec 保序）即可，不必强行 IndexMap；用 IndexMap 也无害。entities 与 windows 的外层映射必须保序（entities：过期快照与 UI 遍历；windows：busyReason 首匹配）。CompiledContent 的 verbs/prototypes/templates/endings 建议保序（报错顺序 + UI 遍历序）。

**对移植计划**：**must**。IndexMap/Vec 纪律贯穿 State.entities、State.windows、CompiledContent 全部映射与 JSON 序列化（serde_json `preserve_order` 或手写有序序列化）；验收基准是日志逐字节相等。

---

## ENG-05 [portability: high] RNG 全链路 BigInt 精确语义

**原声明要点**：dice.ts:26-42 splitmix64 全 BigInt；rollDie key 为 `asUintN(64, BigInt(seed)) ^ asUintN(64, BigInt(counter))`（负 seed 二补码），`1 + Number((BigInt(sides)*r) >> 64n)`；hashDeckId FNV-1a 64 over UTF-8（content.ts:430-461）；permute key = seed ⊕ hash ⊕ i；向量独立生成；seed 解析 `Number()` 可超 2^53 丢精度。

**核实过程**：dice.ts:19-42 全部对上（MASK64/GOLDEN_GAMMA/MIX1/MIX2、asUintN XOR、BigInt 乘 + 64 右移）；dice.ts:16-17 注释与 test/dice.test.ts 头注（"Vectors below were computed independently (python3…)"）确认向量独立性。content.ts:430-437 FNV-1a（0xcbf29ce484222325 / 0x100000001b3，TextEncoder 字节）；456 行 key 合成全 asUintN。script.ts:113-114：`/^-?\d+$/` 通过后 `Number(m[2].trim())`，>2^53 字面量确实会静默取整。

**结论**：VERIFIED。

**修正表述**：一处实现细节修正——`(BigInt(sides) * r) >> 64n` 是 128 位积取高 64 位，Rust 不能用纯 u64 wrapping 表达，需 `(sides as u128 * r as u128) >> 64`（或 `carrying_mul`）；其余（wrapping_add/mul、rotate/xor、`seed as u64` 的二补码 cast）确为 u64 直译。另注意 `State.rng` 是 JS Number（f64 整数区），回放计数器直接落 `ROLLED` 行的整数值，Rust 用 u64/i64 存储即可逐值一致。

**对移植计划**：**must**。以 test/dice.test.ts 的独立向量作为 Rust 实现的验收集；seed 解析需决定超精度字面量的行为——**保持 JS 的 Number 取整结果**（如 `f64` 中转或显式文档化），不得"修复"为任意精度，否则同脚本同 seed 骰子序列分叉。

---

## ENG-06 [portability: high] engine 内部依赖 format∘parse 往返

**原声明要点**：registerWindows（engine.ts:347-356）把 `record.sentence` 重新 parseSentence 取 clauses，失败吞异常得空数组（不开窗口）；play 与 fold 共用（fold 侧 1027 行）；"853/107 行模块"。

**核实过程**：344-346 注释原文 "the SAME code path play and fold run (the divergence guard)"；351 行 `try { clauses = parseSentence(record.sentence, dict).clauses } catch { clauses = [] }` 确认；applyRecord 的 action 分支 1027 行调用同一函数确认。行数：parse.ts 853、format.ts 107，确认所指两模块。测试侧：test/parallel.test.ts 头注明言 fold 逐字节重建窗口，是往返路径的现行验收。

**结论**：VERIFIED。

**修正表述**：需强调选项 (b)（内存直传 clauses）并**非严格行为保持**：现状中往返失败的 BEGIN 行静默不开窗口；直传后该退化行为消失（窗口会按原始 clauses 打开）。除非先证明 round-trip 全_total_（对一切可达 sentence 无失败），否则 (b) 与现状在退化输入上分叉。默认走 (a) 忠实移植。

**对移植计划**：**must**。移植方案选 (a)：完整移植 format+parse 并先建逐字节 round-trip 测试（law 2）；`catch { clauses = [] }` 的静默语义必须原样保留，不得"顺手"改成报错。

---

## ENG-07 [portability: medium] JS 值语义渗透到游戏判定

**原声明要点**：resolveRef 返回 `Entity | null`（114-125）；evalPred cmp 对 undefined 读 false（predicate.ts:453）；strict 无绑定抛（predicate.ts:499-504）；`step.startMin ?? absMin(...)`（159）；`state:!x` 即 `v !== true`（query.ts:81）；Infinity 作槽位 max（script.ts:390、engine.ts:94）；`===` 严格相等（query.ts:84）。

**核实过程**：逐行确认——engine.ts:114-125（`?? null`、param 抛、oldestNamed）；predicate.ts:452-453 `if (l === undefined || r === undefined) return false`；499-504 strict throw；engine.ts:159（`??` 保 0）；query.ts:81 `if (t.op === '!') return v !== true`（字符串 "false"/0 均算未封印，描述准确）；script.ts:390 `countSpec.endsWith('..') ? Infinity : min`、engine.ts:94 `slot.max === Infinity`；query.ts:84 `t.op === '=' ? v === t.value : …`（类型不等即 false）。

**结论**：VERIFIED。

**修正表述**：无需修正。补充：槽位记录（含 Infinity）从不进 ActionRecord/JSONL（窗口行不含 slot），报告已注明，确认无误——Rust 用 `Option<u32>`/`u32::MAX` 安全。

**对移植计划**：**must**。作为 Rust 逐点映射表吸收（undefined→Option 三态、"缺失"的两种纪律——`when:` 读 false vs strict 抛——必须逐点对应，不可统一）。

---

## ENG-08 [portability: medium] JSON 字符串作为日志内部的强类型载体

**原声明要点**：SPAWN 解析/生成 JSON pair（509-518、524-525、585-589），`delete attrs['container']`（517）；fold JSON.parse + Object.assign；ActionRecord JSONL 是唯一持久化格式，State 从不序列化。

**核实过程**：509-518（含 517 行动态删键）、524-525（concrete pair 推入）、585-589（REPLACE 始终物化 attrs pair）逐行确认；"fold 端 JSON.parse 后 Object.assign"即 511-517（applyOutcome 为 play/fold 共用路径，描述准确）。saveLog 只序列化 records（store.ts:24-27）；State 全 Map/Set 无序列化路径。ActionRecord 字段全为 plain object/array（Ref/Outcome/Arg 均无 Map/Set），确认 serde 可直映射。

**结论**：VERIFIED。

**修正表述**：无需修正。

**对移植计划**：**must**。Rust 端 pair 值保留 `String` 原样存取（不做结构化往返），即可逐字节一致且免于键序/开放键问题；若未来结构化，必须保序 + 容忍开放键（报告已给出同样条件）。

---

## ENG-09 [duplication: medium] 名字冲突/stackable 规则有三份手写副本

**原声明要点**：SPAWN（engine.ts:490-493）、REPLACE（566-569）、addEntity（types.ts:147-155）三处同一规则、三种文案；mintIsStackable（403-405）要求 play/fold 从同一 (name, kind) 复算；测试 150+ 处 toThrow 钉死文案。

**核实过程**：三处文案逐字确认——`SPAWN: name '${name}' already in use`（492）、`REPLACE: name '${protoName}' already in use`（568）、``duplicate name '${e.name}' among alive entities``（types.ts:153）。三处判定条件逻辑一致（pool 非空 && (!stack || 池非全 stackable)），REPLACE 在垂死卡仍存活时检查（池含自身，stackable 自替换合法——556-557 注释）。mintIsStackable 与注释确认。toThrow 计数：test/*.ts 合计 **192 处**（"150+"成立）；decks.test.ts:573 以整句相等断言 SPAWN 文案，stackable.test.ts:240-248 断言 addEntity 文案。

**结论**：VERIFIED。

**修正表述**：无需修正。补充：Rust 统一为单一判定函数时，**三条消息文本必须由调用点各自提供**（不能统一成一条），否则 192 处断言中的相应用例全红。

**对移植计划**：**must**（规则合一 + 文案逐字保留，作为验收）。

---

## ENG-10 [semantics: medium] 事件/效果触发顺序的确定性锚点

**原声明要点**：六组——advance pass 的 (end, beginId, clause) 全序与 completion≤边界先发（1071-1097、注释 1080-1083）、slot cursor 不跳窗口跨度（1084-1086）；过期快照一次无级联（1064-1067）；fireSlot 声明序 triggers → 快照序 expiries、authored on-expire 首条匹配（1218-1273、1252）；per-clause 骰绑定（235、775、774-776、787-791）；首 ENDING 生效（735）+ REFUSE 丢 draft；窗口完成失败只拒该窗、窗口仍关（1161-1168）。

**核实过程**：逐条对上——1073 排序键、1076 `nextW.end <= slotAbs`（completion 先于同分钟 slot）、1081-1086 注释（cursor 不跳）、1099-1100 wait 收尾；1062-1067 快照 hoisted 注释；1228 声明序循环、1238 expiry 循环、1252 `.find(...)` 首匹配；233-235 每子句 `sctx.roll = undefined`、772-776 计数 c..c+m-1、782-791 ROLLED 仅落计数；735 `if (!ctx.state.ended)`；1154-1168 catch→REFUSE 行 + `out.windows.delete(w.key)`。parallel.test.ts 存在（v0.5 验收测试，头注即排序主题）。

**结论**：VERIFIED。

**修正表述**：注释实际在 1081-1086（报告写 1080-1083，差数行，实质无误）。

**对移植计划**：**must**。六条逐条落成 Rust 排序测试（以 parallel.test.ts / engine.test.ts 的 expiry 用例为模板）——报告的"验收清单"定性准确，其中多条（如同 halfhour 多实体到期序）目前**没有**专门测试直接钉住，属移植时的补测义务。

---

## ENG-11 [semantics: low] 被拒 BEGIN 连带丢弃整个 advance pass（含 id 复用）

**原声明要点**：257-262 拒绝时 `records:[record], state: stateIn`，advance 记录全丢、`firstRecordId` 让给拒绝行；重_roll 怪癖（443-448 注释）；foldUpTo 按 `r.id < uptoExclusive` 过滤（fold.ts:15-17）。

**核实过程**：257-261 确认（`makeBeginRecord(opts.firstRecordId, …)` + `advancedMin = 0` + 返回 `stateIn`）；advance 的 id 从 firstRecordId 起分配（1070 `let id = firstRecordId`），随 draft 一起丢弃。重_roll 注释实际在 437-447（rewriteRollProse 文档块，"a NARRATE in a REFUSED step is journal-visible with its number even though the counter never moved"）。fold.ts:16 确认。确定性重放成立：全部"随机性"是 (seed, counter) 纯函数且计数器在 draft 上，拒绝即还原。

**结论**：VERIFIED。

**修正表述**：注释行号微偏（437-447 vs 报告 443-448），实质无误。

**对移植计划**：**must**。Rust 记录 id 分配必须复刻"失败回收"（拒绝行复用 firstRecordId、下轮同 id 重放），禁止改单调全局序列；`foldUpTo` 的 `<` 过滤语义依赖它。

---

## ENG-12 [semantics: low] clock/计时：只有 wait 行移动 now

**原声明要点**：wait 分支断言时间连续（956-963）；complete 优先读 `w.start` 锚定（975-978）；SET expires 吃该锚（619）；ADVANCE 扩展 pass 目标并落 wait 跨度（1085-1086、1099-1100）。

**核实过程**：956-963（`state.now !== start` → throw "wait spans must partition time"）；975-978（window 表命中→删表项→`startMin = w ? w.start : absMin(parseClock(record.tGame))`；未命中行注释明言 tGame 回退只为 bare-row 探测/前窗口日志）；619 行 `e.attrs['expiresAt'] = ctx.now + toMinutes(value)` 且 fireWindow ctx `now: w.start`（1147）；1085-1086 `target += fired.advanceExtra`、1099-1100 wait 行跨度即 `[fromMin, target]`。全部属实。

**结论**：VERIFIED。

**修正表述**：无需修正。

**对移植计划**：**must**。"wait 是唯一时钟原语"与连续性断言原样保留；complete 的 tGame 回退分支也必须保留（测试用于 bare-row 折叠探测）。

---

## ENG-13 [semantics: low] endings：状态字段驱动，无专门记录种类

**原声明要点**：ENDING 写 `state.ended`（735）；executeStep 顶部硬门（154-156）；undo 过头自然重开（注释 151-153）；编译期校验（content.ts:207-214、checkEndingRefs 358-367）；flavour 开放小写词表（content.ts:212）。

**核实过程**：735 `if (!ctx.state.ended) ctx.state.ended = { id, title, flavour }`；154-156 `if (stateIn.ended) return rejected(...)`；151-153 注释（"undo past the ending record reopens the game"）；content.ts 208-214 循环（UPPER_SNAKE、标题非空、flavour `/^[a-z]+$/`）；checkEndingRefs 358-367 递归 IF 分支。全部属实。

**结论**：VERIFIED。

**修正表述**：无需修正。

**对移植计划**：**must**（保持"ending 是 fold 投影"）；**勿收紧 flavour 为枚举**——收紧即行为变更（新脚本会加载失败），报告的告诫正确。

---

## ENG-14 [semantics: low] deck：排列纯函数、游标在 State、DRAWN 折叠断言

**原声明要点**：排列固定于编译期、不耗 rng（content.ts:151-164 注释、453-477）；游标在 draft 推进（engine.ts:825-826）；fold DRAWN 对照排列断言（844-871）；耗尽规则（content.ts:472-477）。

**核实过程**：CompiledDeck 文档 151-156（law 5：与骰子流独立键空间、不耗计数器）；permutePositions 453-461（Fisher–Yates over positions，key 见 ENG-05）；engine.ts:825-826（drawFrom + `ctx.state.decks.set(...)` on draft）；DRAWN 分支 844-871（对照 `drawFrom(deck, cursor)`，名字不配即 loud error）；drawFrom 472-477（reset 环绕 / defaultcard 永续 / 抛错）。全部属实。

**结论**：VERIFIED。

**修正表述**：无需修正。

**对移植计划**：**must**。DRAWN 断言是最先落地的内容漂移探测器，须配套测试（decks.test.ts 已有 loadLog/fold 重放用例可对照）。

---

## ENG-15 [duplication: low] recipe id 铸造规则两处镜像已漂移

**原声明要点**：编译期 content.ts:229 铸 `.recipe${i}`（0 基）；lint 镜像 script.ts:485 铸 `.recipe${i + 1}`（1 基）；只影响 lint 文案。

**核实过程**：229 行 `id: r.id ?? (r.when ? \`${name.toLowerCase()}.recipe${i}\` : …)`（i 为 `v.recipes.map` 下标，0 基）；485 行 `r.id ?? (r.when ? \`${verb.toLowerCase()}.recipe${i + 1}\` : …)`（1 基）。同一无 id 带 when 的首 recipe 两处命名确差 1。编译侧 0 基被测试钉死：script-play.test.ts:175 `expect(studyGate.id).toBe('study.recipe0')`。lint 侧 1 基文案**无测试钉住**（lint.test.ts 全部使用显式 id 或 `read.default`）。

**结论**：VERIFIED。

**修正表述**：finding 本身完全成立。但报告建议"移植时应统一为一条铸造函数，**顺便修掉**"——**此处标红**：修掉即改变 lint 输出文本，属用户可见行为变更。虽然无测试钉住 1 基文案（修复不会红测试），语义一致性也不能作为移植期顺手变更的理由。

**对移植计划**：移植时**必须原样保留**（编译 0 基 + lint 1 基，一条铸造函数的两个参数化变体即可做到"单一来源"而**不改输出**）；统一基数**另立议题**。

---

## ENG-16 [clarity: medium] 槽位 role 从不校验，typo 静默失效

**原声明要点**：script.ts:391 `role: role as SlotDef['role']`；正则只要求 `[a-z]+`（script.ts:172）；compileContent 原样存 slotByRole（content.ts:221-227）；validateSlots 只遍历规范 ROLES（engine.ts:85-87）；typo 槽位永不匹配。

**核实过程**：391 行强转确认；verb 块内槽位行的正则 `^([a-z]+):\s*(.+)$` 在 **171 行**（报告引 172——172 是同一检查的 throw 行，差一行，实质无误）；content.ts:221-227 `slotByRole.set(s.role, compiled)` 无白名单校验；engine.ts:85 `for (const role of ROLES)`——`objct` 槽位永不被查询，而缺 object 槽时提供 object 报 `does not accept 'object'`（89 行），与报告描述的症状一致。

**结论**：VERIFIED。

**修正表述**：行号勘误（正则 171 非 172），其余准确。

**对移植计划**：**移植时保持原行为**（未知 role 照常编译、静默失效）——**标红**：报告自己已注明"行为变更，需记录为有意修复或保持原状"，核实确认必须选"保持原状"，编译期拒绝**另立议题**。

---

## ENG-17 [clarity: low] 重复声明处理不一致 + 大小写 id 碰撞

**原声明要点**：entities 重复报错（136-139）、endings 重复报错（151-153）；prototypes（143）、templates（157）静默覆盖；verbs 重复头静默重置（163）；实体 id `s_${name.toLowerCase()}`（431）而查重区分大小写（138）——`ANA`/`ana` 同过检查、同 id，addEntity `entities.set` 静默覆盖（types.ts:159），byName 双池指向同一实体。

**核实过程**：逐行确认——136-139（`entityNames.has(name)` → throw，区分大小写）；151-153（endings throw）；143 `prototypes[key] = proto`（覆盖）；157 `templates[m[1]] = m[2]`（覆盖）；163 `verbs[verbName] = { slots: [], … }`（重置）；431 `idOf = (name) => \`s_${name.toLowerCase()}\``；第二个实体的名字进不同 byName 池 → addEntity 名字检查不触发，159 行 `s.entities.set(e.id, e)` 覆盖同 id 前者。链路完全成立。

**结论**：VERIFIED。

**修正表述**：无需修正。

**对移植计划**：**移植时保持原行为**——**标红**：报告建议"统一重复=错误 + 大小写碰撞检查"并自注"属有意行为变更"。核实结论：移植期一律保持现状（覆盖/重置/大小写碰撞全保留），统一校验**另立议题**。

---

## ENG-18 [dead-code: low] 未使用/不可达代码

**原声明要点**：crossedSlots（clock.ts:78-84）、attrGet（types.ts:221-223）、getEntity（types.ts:182-184）、sameRef（ref.ts:51-53）src 零调用；emptyClause（sentence.ts:75-77）仅测试用；engine.ts:1149 idGen 回退不可达；engine.ts:1058 `toMin < stateIn.now` 死条件；script.ts:408 `i = i`、411 `void i`、390 `cm[1] !== undefined` 恒真。

**核实过程**：grep 全仓（src/test/tools/scripts）——crossedSlots/attrGet/sameRef/getEntity 各只有定义处，零调用；emptyClause 仅 test/explorer-lib.ts:116、test/reach-fuzz-lib.ts:374 使用。fireWindow 的 `idGen` 参数类型为非可空 `() => string`（1127 行），1149 行 `idGen ?? (...)` 回退与 1144 行 `let mint = 0` 确不可达。resolveDue 全仓唯一调用方为 executeStep:183，其入参 startMin 已在 161-164 保证 `>= stateIn.now`，1058 行首析取支确死（第二析取支 `toMin === now && !hasDue` 存活，**不可整行删除**）。script.ts:408 `i = i`、411 `void i` 确认；390 行 `cm` 来自 `/^(\d+)(?:\.\.(\d+)?)?$/` 匹配，`(\d+)` 必参与，`cm[1] !== undefined` 恒真。

**结论**：VERIFIED。

**修正表述**：一处精度补充——engine.ts:1058 只**首个析取支**死，`(toMin === stateIn.now && !hasDue)` 是活路径，移植时须保留该分支。

**对移植计划**：**should**。死函数不搬（零调用方，无行为风险）；不可达回退不搬；恒真条件与 `i=i`/`void i` 简化——均为 Rust 内部清洁，不影响可观察行为。

---

## ENG-19 [clarity: low] 魔数与解析器结构点

**原声明要点**：engine.ts:1239（`exp.at > absSlot * 30`）与 1279（`now: absSlot * 30`）绕过 MIN_PER_HALFHOUR（1091 用了常量）；script.ts:116 meta else 落 parseClock；lintScript 双重解析（479-480）。

**核实过程**：1239、1279 确认；1091 `slotAbs / MIN_PER_HALFHOUR` 确认；**补充第三处**：makeEventRecord 1287 行 `clockFromAbsMin(absSlot * 30)` 同样是裸 30。script.ts:116 确认（未匹配 script/player/goal/seed 的 meta 键会走 `parseClock` 抛 "bad clock"——是响亮报错但归因误导，报告"静默落进时钟错误"指归因错位，成立）。lint.test.ts:479-480 `parseScript` + `compileScript`（后者内部再 parse）确认。

**结论**：VERIFIED。

**修正表述**：补充 1287 行第三处魔数；meta 键枚举化的告警——现状未知 meta 键的报错文案是 "bad clock '…'（expected "day:halfhour"）"，Rust 枚举化若改抛 "unknown meta key" 即变更错误文本，须保留原文案或验证无测试/用户依赖。

**对移植计划**：**should**（常量替换与 parse 一次传两处均为行为保持；meta 枚举化须保持错误文本不变，否则降级为另立议题）。

---

## ENG-20 [portability: low] store.ts：仅有的 Node 依赖 + 非原子写

**原声明要点**：store.ts:3 node:fs 唯一出现点；全 src 无 Math.random/Date.now；saveLog 直接覆写；loadLog 无行号无校验；JSONL 行纯 JSON，serde 可一一对应。

**核实过程**：grep `node:fs` 在 src 内仅 store.ts:3；grep `Math.random|Date.now` 在 src 零命中。saveLog 24-27（writeFileSync 整文件覆写）、loadLog 29-32（split('\n').filter(Boolean).map(JSON.parse)，无行号）确认。ActionRecord 逐字段核对：id/kind/trigger/tGame/tReal/advancedMin/startMin/window/actors/verb/objects/instruments/at/about/to/outcomes/recipes/sentence——全部 plain（Ref/Arg/Outcome 无 Map/Set）。

**结论**：VERIFIED。

**修正表述**：无需修正。一点边界说明：原子写（tmp+rename）改变的是崩溃窗口内的文件系统行为，不触及游戏语义，属可接受的移植改进；loadLog 加行号会改变损坏日志的错误文案——当前无测试钉住该文案，可做但应记录。

**对移植计划**：**should**（原子写 + 带行号错误）；serde 直映射是 **must** 的实现载体（日志逐字节相等的另一面是 serde 序列化配置：字段序、无字段省略）。

---

## ENG-21 [portability: low] 字符串排序语义

**原声明要点**：sortRefs（ref.ts:47-49）用 JS `<`（UTF-16 码元序）排 actors/objects/with，决定 canonical 句子与 BEGIN 行 actors 序；play.ts:238 localeCompare 仅 UI；Rust UTF-8 字节序仅在 U+E000..FFFF vs 星外平面字符上不同。

**核实过程**：ref.ts:47-49（`refSortKey` 字符串比较）确认；sortRefs 用于 formatClause（format.ts:30/34/35）与 makeBeginRecord actors（engine.ts:321）——确实进入 canonical 句子与日志行。play.ts:238 `a.name.localeCompare(b.name)` 在 suggestions 池排序，仅 UI。分歧面判断正确：两序仅在不同平面字符与 U+E000..U+FFFF 的相对位置上不同；名字可含任意引号字符串（formatRef 允许），理论分歧真实存在。

**结论**：VERIFIED。

**修正表述**：无需修正。

**对移植计划**：**must**（保守方案成立）：Rust 侧排序按 UTF-16 码元序实现（比较时转 `Vec<u16>` 或手写 code-unit 比较），并在类型层面注明原因；不要用 `str::cmp`（字节序）静默替代。

---

## ENG-22 [modularity: low] content 数据的组织结论

**原声明要点**：script.ts 是解析器/编译器逻辑（约 15 个语义敏感正则，行段 205-251/256-303/307-317/324-334/343-368/372-379/383-392/394-413）；examples/ 下 2 个 .ts 演示；内容本体是脚本文本；加载期 fail-loud 是测试断言的一部分（test/engine.test.ts:1002 等）；派生物必须加载期重算。

**核实过程**：八个行段与 parseEntityLine/parsePrototypeLine/parseTriggerLine/parseFunctionLine/parseDeckLine/parseEndingLine/parseSlotLine/parseRecipeLine 一一对应，确认；examples/ 恰有 demo.ts、play.ts 两个文件；打包内容为 scripts/the-nightjar-flyer.txt（lint.test.ts:9 以文本读入——印证"脚本文本=数据"）。test/engine.test.ts:1002 核实确为一条 `.toThrow(/PROGRESS belongs/)`；全测试 192 处 toThrow。编译侧派生物（0 基 recipe id、seed 排列、stackable 集合）均有测试钉住（script-play.test.ts:79-84/175 等），预编译会制造第二真相源的论证成立。

**结论**：VERIFIED。

**修正表述**：无需修正。

**对移植计划**：**must**（架构决策照单吸收）：Rust 保持"脚本文本=数据"（include_str! 或运行时读入），parseScript/compileContent 移植后加载期完成全部校验；CompiledContent 为纯数据结构；错误文本是测试 API 的一部分，逐字保留。

---

## 计划必须吸收（must）

1. **插入顺序纪律（ENG-04）**：State.entities / State.windows / CompiledContent 全部映射保序（IndexMap/Vec）；JSON 序列化保序；byName 外层可放宽为"池数组保序"。验收 = 日志逐字节相等。
2. **RNG/排列 64 位精确语义（ENG-05）**：splitmix64、FNV-1a、seed⊕counter 二补码 XOR、128 位积取高 64 位；负 seed 走 `as u64`；以 dice.test.ts 独立向量为验收；seed 超 2^53 解析保持 JS Number 取整结果。
3. **format∘parse 往返是承重墙（ENG-06）**：按方案 (a) 完整移植并保互逆；`catch { clauses = [] }` 静默不开窗口语义原样保留；先建 round-trip 逐字节测试。
4. **顺序/原子性矩阵（ENG-10/11/12）**：advance pass 交错序、过期快照无级联、per-clause 骰绑定、拒绝整 pass 丢弃 + id 回收、wait 唯一动钟 + 连续性断言、complete 锚定 w.start、窗口完成失败仍关窗——逐条落 Rust 排序测试。
5. **错误/告警文案逐字保留（ENG-09/15/16/17 + 192 处 toThrow）**：三条名冲突消息按调用点分发；0 基 recipe id（编译）与 1 基（lint）的漂移**原样保留**。
6. **JS 值语义映射表（ENG-07）**：undefined 三态、`??` 保 0、`v !== true`、`===` 类型不等即 false、`Option<u32>` 化 Infinity——逐点对应不统一。
7. **JSON pair 不透明字符串（ENG-08）+ serde 直映射（ENG-20）**：pair 值 String 原样存取；序列化配置钉死字段序。
8. **UTF-16 码元排序（ENG-21）**：sortRefs 的 Rust 实现按 code unit 比较。
9. **ending/deck 语义（ENG-13/14）**：ending 为 fold 投影、flavour 不收紧；DRAWN 断言最先落地并配测试。
10. **架构（ENG-22）**：脚本文本=数据，加载期全量校验，CompiledContent 纯数据结构。

## 应驳回 / 降级（不得随移植顺手变更）

1. **ENG-15 "顺便修掉" recipe id 漂移**——驳回"顺手修"：lint 文本是用户可见行为；移植保持 0 基/1 基现状（可用单一铸造函数参数化而不改输出），统一基数另立议题。
2. **ENG-16 编译期拒绝未知 slot role**——驳回随移植实施：行为变更，现状（静默失效）必须保留，另立议题（报告已自注，核实确认取"保持原状"）。
3. **ENG-17 统一重复声明为错误 + id 大小写碰撞检查**——驳回随移植实施：同上，现状保留，另立议题。
4. **ENG-19 meta 键枚举化的错误文案**——降级为条件采纳：仅当保留 "bad clock …" 原文案时可在移植内做，否则另立议题。
5. **ENG-04 对 byName 外层映射的 IndexMap 硬要求**——降级为"池数组保序即可"（外层映射全仓无迭代点）；IndexMap 仍为安全默认。
6. **ENG-05 "纯 u64 wrapping 即可复刻"**——修正为需要 128 位中间量的一处（`(sides*r)>>64`），不影响 finding 成立。
7. **ENG-02 "4 个测试文件"**——勘误为 2 个文件 4 处调用；finding 实质成立。
