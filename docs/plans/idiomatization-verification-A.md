# 验证报告 — rust-idiomatization.md §0.2 A 类清单（17 项）

日期：2026-10-14。验证者：只读复核 agent（未改动任何 src/tests/spec 文件；未跑 cargo）。
基准：`docs/spec/text-engine-v0.2.md`（下称 spec，行号以当前工作区文件为准，1915 行）；
现状核对：`src/`、`tests/`、`README.md` 的静态阅读。

方法：对每一项 (1) 逐字打开计划引用的 spec 节/行核对明文；(2) 静态核对计划描述的
代码/测试现状；(3) 按三档结论。三个重点复核（Q7→A16、Q9→A17、A1/A2 常数）单列于文末。

## 汇总表

| 项 | 结论 | spec 原文关键句（行号） | 偏差说明 |
|----|------|------------------------|----------|
| A1 | VERIFIED | "1 + floor( sides * splitmix64(seed XOR (c+i)) / 2^64 )" (:1570)；"seed: … default 0" (:1572, :1739-1740)；"consumes exactly m counters" (:1577)；"ROLLED(<total>, <ctrAfter>)" (:1578) | spec 的 "BigInt END-TO-END" (:1573) 是对精确 64 位算术的表述；Rust 用 u64/u128 定宽精确算术等价实现，python 独立向量表 + 2^53−1 种子钉死（tests/dice.rs:3-4,304）。非偏差 |
| A2 | VERIFIED | "Fisher–Yates shuffle over the POSITIONS … splitmix64(seed XOR hash(deckId) XOR i) — the §9.41 mixer with an FNV-1a-64 hash" (:1653-1655)；"consuming NO rng counter" (:1657)；DRAWN 游标断言 (:327, :1675-1678) | 计划写 "§9.42"：spec 自身交叉引用即以 §9.42 指牌堆裁定（:326, :398, :1825），而该裁定印刷编号为 43——spec 内部编号错位，计划跟随 spec 引用惯例，非计划错误 |
| A3 | VERIFIED | "canonical OUTPUT: SPAWN, NARRATE, REPLACE and ENDING paren-less" (:401)；DRAW paren-less (:398, :326)；"Post-verbal AT … canonicalized to the pre-verbal position" (:429-431)；"canonical = sorted" (:381)；"canonical output: name" (:414)；round-trip：§5.2 标题 "all parse; all round-trip" (:442)、"§0 law 2 rounds trip engine-written rows only" (:1486-1487) | 计划的 paren-less 列举（SPAWN/NARRATE/ENDING/DRAW）漏列 REPLACE——REPLACE 亦明文 paren-less（:401, :313 "Paren-less authored form"）。列举不全，非错误 |
| A4 | VERIFIED | "prefers a single exact unit (d, then h, then halfhour, then raw minutes — `90m` renders `3halfhour`) … decomposes into compound form only when no single unit fits" (:1724-1727)；"`61m`" (:1729)；"1halfhour = 30 minutes" (:412) | 无 |
| A5 | VERIFIED | "The shipped store is JSONL — one record per line" (:992)；§7 列 t_game/advanced_min/recipes/sentence (:966-979)；"BEGIN/COMPLETE/WAIT rows, `startMin` and the windows table" (:180-181)；"via the row's `beginId`" (:834)；物化 ruling 18 (:1126-1129)、"REPLACE always writes the `attrs=` pair" (:1225-1227) | `window` 作为字段名是引擎对 spec "windows table"/窗口键的命名——spec 明文钉住概念与 `beginId` 锚定，未钉该 JSON 拼写；其余五个字段名 spec 明文（camelCase 见 :181 与 §6.6:775 "advancedMin"） |
| A6 | VERIFIED | "SET deltas are logged as absolutes" (:1187)；"the log receives only the taken branch's outcomes" (:1409-1411)；"no IF ever appears in the log" (:323, :921)；"the engine never sees a call, and therefore neither does the log" (:109-110)、"canonical equals expansion" (:1484-1485) | 无 |
| A7 | VERIFIED | §6.6 六步 1 Validation…6 Events (:758-802)；REFUSE "no state change, no time passes … attempt is still appended to the log" (:747-751)；§9.19 "rejects with nothing logged" (:1130-1133)；§9.20 单调钟 (:1134-1135)；"role refs re-resolve against the draft before each clause applies" (:766-768) | 无 |
| A8 | VERIFIED | BEGIN 行 (:815-818)、COMPLETE 行 (:828)、"The pass ends in a WAIT row" (:854-855)；"the windows table *is* the lock" (:858)；"(ordered by end minute, then begin row, then clause), then the halfhour's slot events" (:852-853)；"completions at minute m preceding slot-m events" (:853)；"One pass is atomic … resolve at the next one" (:855-856)；死区 ruling 28 (:1167-1170) | 无 |
| A9 | VERIFIED | "Within one crossed halfhour, authored triggers fire before expiries" (:792-793)；"The fold trusts logged EVENT rows" (:794)；once 投影 (:1173-1174)；"expire:<NAME>" (:376-378)；"authored outcomes replace the built-in destroy entirely" (:1207-1209) | "(declaration order)" 全 spec 仅一处明文（:1211，§9.32，针对 expiry 触发器匹配）；同一 halfhour 槽内多个时间触发器的相互顺序无明文。括注对 expiry 触发器成立、对时间触发器为过度声明（该子行为实为 B：仅测试钉住） |
| A10 | VERIFIED | "Names unique among alive entities" (:1111)；"unique among alive entities … A name ref under a pool addresses the OLDEST copy" (:423-428)；"byName … in BIRTH order … addresses the HEAD … the OLDEST copy" (:1348-1353)；"it concretizes to `DESTROY #id`" (:1359-1360) | 无 |
| A11 | VERIFIED | ruling 35 全部子 bullet：First wins (:1252-1256)；post-ending rejection "the message names the ending's title" (:1263-1266)；undo 重开 (:1278-1285)；load-time existence check (:1286-1289)；endings: 表 (:1834-1839) | 无 |
| A12 | VERIFIED | "every call expands to the function's body at load" (:107-109)；"canonical equals expansion" (:1484-1485)；"capped at 8 … a chain of 9 is a load error" (:1536-1538)；recursion "the error NAMES the cycle" (:1532-1534)；arity (:1538-1539)；"Parameters shadow" (:1540-1545)；"An UNDECLARED `{token}` … load-time error" (:1525-1531) | 无 |
| A13 | VERIFIED | PROGRESS 触发器禁令 (:1154)；REFUSE 触发器禁令 (:1239-1240)；trigger IF role operand load error (:1459-1465)；count/total lint "a load error naming the valid forms" (:646-651)；"Authoring one is a load error"（ROLLED :325、DRAWN :327）；deck spec/defaultcard load 解析 (:1646-1650)；保留字表 (:419-422) | 无。"wording 是 B" 与 spec "naming the valid forms" 的措辞层级一致（§0.3 规则自洽） |
| A14 | VERIFIED | "Names resolve case-insensitively, underscores as spaces, articles dropped, longest match first (up to 4 tokens) … suggestions (prefix match, then edit distance ≤ 2)" (:1862-1865)；Game.visible() (:1882-1887)；"Undo = drop the trailing EVENT rows and the last action row, then re-fold" (:1897-1898)；suggestions "light up a non-default non-REFUSE recipe" (:1871-1874) | 无 |
| A15 | VERIFIED | "one verb-template line per clause … PROGRESS ticks indented under the template with their offsets, NARRATE lines unindented, and a REFUSE spoken as its own line" (:521-526)；示例 "(+30m)" (:531)；"Verb templates render per clause" (:534)；"{roll} is rewritten AT EXECUTION" (:1624-1625) | 无 |
| A16 | VERIFIED | "the real counter never moved … The refused attempt's journal prose may still show its `{roll}` number, and a re-attempt throws the SAME dice (same counters) — deterministic, documented, accepted" (:1589-1594) | 见重点复核 1 |
| A17 | VERIFIED | "Bare literals lex as ONE token in outcome args" (:1721)；duration 产生式 (:411-413) | 见重点复核 2 |

**合计：17 VERIFIED / 0 WEAK / 0 REFUTED。**

## 逐项短节（spec 明文 + 代码现状）

- **A1**：公式逐字在 :1570；seed 默认 0 两处（:1572、§10 :1739-1740 及 :1747 注释）；计数器语义 :1577（恰好 m 个）、:1596-1597（count=0 合法、计数器不动）、ROLLED 落盘/折叠 ：1578-1586。代码：`src/core/dice.rs:14-28`（标准 splitmix64 常数 0x9E3779B97F4A7C15 / 0xBF58476D1CE4E5B9 / 0x94D049BB133111EB，移位 30/27/31），`roll_die`（:38-42）u128 承接 `sides * r` 高位后 `>> 64`。测试：`tests/dice.rs:3-4,270,284`（python3 独立 27 行向量）、:304/:316（9007199254740991 = 2^53−1）。
- **A2**：Fisher–Yates over positions、键 `splitmix64(seed XOR hash(deckId) XOR i)`、FNV-1a-64、不消耗 rng counter，全在 :1651-1662；DRAWN 游标断言 ：327 与 ：1675-1678；load 解析与 empty-spec 错误 ：1646-1650。代码：`src/content/content.rs:886-893`（FNV-1a-64 offset 0xCBF29CE484222325、prime 0x100000001B3）、:895-921 `permute_positions`（降序 i，`splitmix64(seed^hash^i) % (i+1)`）、serve 函数 play/fold 共用。
- **A3**：:401（规范输出 paren-less 集）、:398/:326（DRAW paren-less）、:429-431（AT 前置）、:381/:283-284（actors 排序）、:414（bare-name 规范形）；round-trip 法：:442、:1486-1487（"engine-written rows only"）、:1583-1585（ROLLED 必须可再解析）、:1727-1728。现状：`src/core/format.rs`/`parse.rs` 往返 + `tests/dice.rs` "ROLLED round-trips and fold replay"、JSONL 字节同一性门。
- **A4**：:1719-1730（§9.45 全文，含渲染优先序与 `100m→1h40m`、`1441m→1d1m`、`61m`）、:411-413（文法 + 1halfhour=30min）。代码：`src/core/clock.rs:129-160` `format_duration`（d→h→halfhour→复合分解），测试 ：222-226（`90→"3halfhour"`、`100→"1h40m"`、`30→"1halfhour"`）。
- **A5**：JSONL :992-993；§7 列集 ：962-979；v0.5 注记 `startMin`/windows table :180-181；`beginId` :834-835；物化 ruling 18 :1126-1129、§9.15、REPLACE 恒写 attrs= :1225-1227。代码：`src/engine/engine.rs:57-90` `ActionRecord` serde 名逐一对应（tGame :66、advancedMin :69、startMin :71、window :74、recipes :87、sentence :89）；`src/engine/store.rs` JSONL 读写。
- **A6**：:1187-1200（SET 绝对值落盘）、:1409-1419 + :921-924 + :323（IF 只记取分支、log 无 IF）、:1475-1490 + :109-110（编译期展开、log 无调用）。代码：`src/core/format.rs:192`、`src/engine/engine.rs` outcomes 物化路径。
- **A7**：§6.6 六步 ：758-802；§6.5 :745-755；§9.19 :1130-1133；§9.20 :1134-1135；草稿再解析 ：766-768。代码：`src/engine/engine.rs`（validate_slots → 选配方 → scratch draft 应用 → 时钟 → 事件；`Prepared.bindings` 注释明言 Phase 3 对 SCRATCH draft 再解析）。
- **A8**：§6.7 :804-873（BEGIN/COMPLETE/WAIT、窗口表即锁、beginId 锚定、推进序 `(end minute, begin row, clause)`、completions 先于 slot 事件、pass 原子不级联）；§9.28 :1163-1170（死区）。代码：`src/engine/engine.rs:344,2123,2179-2186`（`end → begin_id → clause` 排序）、`src/core/types.rs:135`（`begin_id`）。
- **A9**：:791-796（authored 先于 expiries、fold 信任 EVENT 行、不级联）、:1171-1174（once 投影）、:376-378（expire:<NAME>）、:1206-1214（authored 完全取代 destroy、声明序）。代码：`src/engine/engine.rs:2543`（`format!("expire:{ent_name}")`）。
- **A10**：:1111、:423-428、:1347-1362（BIRTH 序池、最旧副本寻址、引擎销毁按 #id）。代码：`src/engine/fold.rs`/`store.rs` 的 byName 有序池。
- **A11**：ruling 35 :1250-1289 六个子 bullet 逐字覆盖计划的五个子句；endings: 表 :1834-1839。代码：`src/session/game.rs:441`（ended → suggestions 空）等。
- **A12**：ruling 40 :1475-1561 逐字覆盖全部七个子句（展开、canonical equals expansion :1484-1485、深度 8 :1536-1538、环命名错误 ：1532-1534、arity :1538-1539、遮蔽 ：1540-1545、未声明 {token} :1525-1531）。代码：`src/core/fn_expand.rs:82-83`（depth 9 = load error）。
- **A13**：六个存在性检查各有明文（见汇总表行）；措辞层级（"naming the valid forms"）支持 §0.3 的 A/B 切分。代码：`src/content/script.rs`、`src/core/fn_expand.rs`、`src/core/predicate.rs` 各 guard；`assert_err_msg` 正则钉措辞（tests/）。
- **A14**：§11 :1841-1903 逐字覆盖四个子句。代码：`src/session/lenient.rs`（norm 小写/下划线/冠词 ：401-418、MAX_NAME_WORDS=4 :428、prefix→levenshtein≤2 :469-487）、`src/session/game.rs:172`（visible）、:374（undo）、:441（suggestions，ended→空）。
- **A15**：§5.3 :509-539 + 示例 ：528-532；`{roll}` 执行期重写 ruling 42 :1624-1626（并 ：1626-1629：PROGRESS/REFUSE 的 {roll} 是 load error——故"再现骰值的 journal prose"只能是 NARRATE）。代码：`src/session/render.rs:112`（`"  (+{off}) …"` 缩进 tick）。
- **A16**：见下"重点复核 1"。
- **A17**：见下"重点复核 2"。

## 重点复核（三个此前的重分类）

**1. Q7→A16（§9.41 refused-rolls bullet，:1589-1594）——站得住。**
spec 原文逐字："a REFUSE after a ROLL discards the draft — the real counter never
moved … The refused attempt's journal prose may still show its `{roll}` number, and
a re-attempt throws the SAME dice (same counters) — deterministic, documented,
accepted." 三个子句（NARRATE 可携带骰值 / 计数器未动 / 重试掷同骰）全部明文；
且 ruling 42 :1626-1629 规定 {roll} 只能活在 NARRATE（PROGRESS/REFUSE 的 {roll}
是 load error），"journal prose" 即 NARRATE 无歧义。README Q7 行（README.md:168）
描述与之一致。重分类成立。

**2. Q9→A17（§5.1:411-413 + §9.45:1721）——站得住。**
:1721 逐字："Bare literals lex as ONE token in outcome args"；§5.1 duration 产生式
（:411-413）把 `1halfhour` 定为单一 duration 记号并钉 1halfhour=30min。Q9 的可观察面
（单记号词法）是明文，`DUR_RE` 交替序只是其实现载体（`src/core/clock.rs:103-107`
注释自认 load-bearing）。重分类成立。

**3. A1/A2 的 RNG/FNV 常数——站得住。**
spec 以算法名钉死常数集："splitmix64"（:1570，并 ：1573 要求端到端精确 64 位）与
"FNV-1a-64 hash of the deck id"（:1655）。splitmix64/FNV-1a-64 是有标准常数的命名
算法，spec 即以此方式明文；Rust 实现用的正是标准常数（`src/core/dice.rs:14-16`、
`src/content/content.rs:889-892`），且 python 独立向量 + 2^53−1 种子（spec :1575-1576
自己要求该钉法；tests/dice.rs:304）把两端钉死。唯一表述差：spec 的 "BigInt
END-TO-END" 是 TS 语境的精确性要求，Rust 以 u64/u128 定宽算术满足同一要求——行为
等价，非偏差。**A/B 划分无需重审。**

## 保留事项（不改变结论，建议计划下一版吸收）

1. **A2 的节号**：spec 自身交叉引用把牌堆裁定写作 §9.42（:326、:398、:1825），但
   印刷编号是 43（42 号是 `roll` binding）。计划跟随 spec 引用惯例，不算错，但
   值得加一行注避免后续读者困惑。
2. **A3 漏列 REPLACE**：规范 paren-less 集是 SPAWN/NARRATE/REPLACE/ENDING（:401）
   加 DRAW（:398/:326）；计划的列举少 REPLACE。
3. **A5 的 `window` 拼写**：spec 明文钉 `startMin`（:181）与 `beginId`（:834）及
   "windows table" 概念（:858）；JSON 字段名 `window` 是引擎命名。§7 的 SQL 形状
   仍是 v0.2 列集，未含 v0.5 新字段——字段集的 v0.5 部分靠 ：180-181 的注记明文。
4. **A9 的 "(declaration order)"**：全 spec 仅 :1211 一处（§9.32，expiry 触发器
   匹配序）；同一 halfhour 槽内多个时间触发器的相互顺序无明文，该子行为实为
   B（仅测试钉住）。A9 行的其余全部明文，故整体维持 VERIFIED。

## 驳回/降级清单

**空。** 17 项全部 VERIFIED；无 REFUTED、无降级为 WEAK 的整行。三个重点复核
（Q7→A16、Q9→A17、A1/A2 常数）全部成立，A/B 划分的地基不动。上述 4 条保留事项
均为注记级，不影响任何 A/B/C 判定。
