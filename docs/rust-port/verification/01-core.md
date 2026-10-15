# core/ 分析报告（01-core.md）逐条验证

验证对象：`docs/rust-port/analysis/01-core.md` 的 21 条 CORE-NN finding。
验证方法：只读。逐条打开其引用的 file:line（worktree `/Users/amias/Projects/chorus/.worktrees/rust-port`），通读 `src/core/` 全部 11 个文件 + `src/index.ts`，并按各条声明抽查跨模块证据（`src/engine/engine.ts`、`src/ui/render.ts`、`src/content/content.ts`、`src/engine/store.ts`、`test/*.ts` 的 grep 抽样与行号定位、全 src 的 `node:` 导入扫描、core 内正则全量清点）。未修改任何源码，未运行构建/测试。

**总判定：21 条中 16 条 VERIFIED、5 条 PARTIAL、0 条 REFUTED。** 无一条整体驳回；5 条 PARTIAL 均为"主声明成立、某处证据数字或子声明有偏差/某条移植建议若照字面执行会改变行为"，详见各节。

| ID | 结论 | 一句话说明 |
|----|------|-----------|
| CORE-01 | VERIFIED | parse.ts 三段结构、行号、展开层仅被 content 调用全部属实；拆分纯搬家。 |
| CORE-02 | VERIFIED | 两处 type 级环与 predicate→sentence 值导入（ROLES）逐行属实，当前确无运行时环。 |
| CORE-03 | PARTIAL | "全部 export *、内部助手成公共面"属实，但模块数是 **21** 个（index.ts:2-22），不是 22。 |
| CORE-04 | VERIFIED | splitmix64 BigInt 链、asUintN 二补码、128 位乘、rng/decks 落位注释逐行属实；测试向量存在。 |
| CORE-05 | PARTIAL | 三处 UTF-16 序比较属实、顺序差异真实；但 **sameRef 是相等比较不是排序**，不受序语义影响，该子声明有误。 |
| CORE-06 | VERIFIED | 五个插入序敏感点全部属实（含 engine.ts:1065→1238 过期序进日志序）；functions.test.ts:138 确实钉了罗列序（且恰为字母序，测试碰巧不排斥 BTreeMap——分析者的谨慎是必要的）。 |
| CORE-07 | VERIFIED | float64 整数、≥1e21 指数化、`String(-0)`→`"+0"` 全部属实，JS 语义引用无误。 |
| CORE-08 | PARTIAL | 三个语义微差（\s 集合、DUR_RE 交替序、tokenizer 静默吞字符）全部属实；但 core 内正则是 **23 处**，"13 处"是漏数（结论仍成立：全部无 lookahead/backref）。 |
| CORE-09 | VERIFIED | UTF-16 索引、offset 语义、字面转义、尾反斜杠的不可观察 undefined 拼接全部属实。 |
| CORE-10 | VERIFIED | 三处 Math.floor 与非负构造注释属实；div_euclid/rem_euclid 对非负输入等价。 |
| CORE-11 | VERIFIED | 全 src 树唯一 node: 导入确为 store.ts:3；两处 no-Math.random grep 门禁测试存在。 |
| CORE-12 | VERIFIED | parse.ts:587 死条件、注释与事实不符、SPAWN/SET 对 param 键的接受度不对称，全部逐步推演属实。 |
| CORE-13 | PARTIAL | 六处重复全部属实；但"argSpelling 复用 formatArg、语义不变"**照字面执行会改变行为**：pair/str 两 case 两侧拼写不同（引号），只能收敛 delta/int/dur/bool。 |
| CORE-14 | VERIFIED | 47 双写、时长单位文法双写、谓词助手名单复写全部属实；建议均为等值重构。 |
| CORE-15 | VERIFIED | registerRelation 无校验 vs registerVerb 强校验、isOp 线性 includes、ROLES 软保留不一致、透传包装全部属实。 |
| CORE-16 | PARTIAL | "错误消息被测试钉死"实质完全属实（抽样正则全部命中）；但断言数量是 **192** 处 toThrow，不是 221。 |
| CORE-17 | VERIFIED | 三值逻辑、strict 零绑定才抛、全死绑定静默 false、every/any 空集、path 取首个存活卡全部属实（total 的序敏感仅在非整数 aspect 值下成立，属可接受的小夸张）。 |
| CORE-18 | VERIFIED | 单层点语义、标量/子映射后写覆盖、'a.b.c' 两侧对称恰好成立，全部属实。 |
| CORE-19 | VERIFIED | sameRef 对 param 抛错、'\|' 键碰撞靠约定兜底、relationAlive 双向查键全部属实。 |
| CORE-20 | VERIFIED | 逐字节展开的全部关键规则（单趟插值、精确拼写、7 保留 token、delta 折入、substExpr 三分支、深度上限、环在 load 拒）逐行属实。 |
| CORE-21 | VERIFIED | predViewFromToks 拼串 + span 截取 + `$1` 压缩比较符空格全部属实；"保留形而非重构"的判断正确。 |

---

## CORE-01 — parse.ts 三合一（modularity / medium）

**原声明要点**：parse.ts (853 行) 混杂句子解析器（Parser, 86-640）、函数宏模板（25-84）、编译期展开（646-853）；展开层只被 content compile 调用却进了公共面。

**核实过程**：通读 parse.ts 全文。`class Parser` 起于 86 行、闭合于 640 行；第 25 行起为函数宏一节（FnTemplate:30、FnTable:39、parseOutcomes:53、parseFunctionBody:64、collectFnCalls:73，止于 84）；646 行为展开层分隔注释，注释块 648-675，`FN_RESERVED_TOKENS`:682、`MAX_FN_DEPTH`:686。grep 全 src：`expandFnCalls/collectFnCalls/MAX_FN_DEPTH/FN_RESERVED_TOKENS` 在 core 外仅 `src/content/content.ts:8,355,485,506,545,587-588` 引用。

**结论**：VERIFIED。行号、结构与"展开层只被 content compile 调用"全部属实。

**修正表述**：无需修正。

**移植建议**：should。拆 `parse_sentence` / `fn_expand` 纯搬家不改行为；注意 ROLL 的 Tok 平衡扫描（`rollOutcome`/`rollArgExpr`）和 IF 条件的 `parsePredicateToks` 属于解析器与谓词层的接缝，拆分时别把接缝类型搬断。

## CORE-02 — 分层倒挂与两处环（modularity / medium）

**原声明要点**：types.ts:2 从 sentence 导入 `Role`（用于 types.ts:71）而 sentence.ts:2 反向导入 `Clock`（type 级双向环）；sentence.ts:4 从 predicate 导入 `Pred/ValExpr`，predicate.ts:14 反向以值导入 `ROLES`（用于 518,544,550）；ref.ts:11 注释自证。

**核实过程**：逐行核对上述 7 处。types.ts:2 `import type { Role }` ✓；types.ts:71 `refs: Record<Role, Ref[]>` ✓；sentence.ts:2 `import type { Clock }` ✓；sentence.ts:4 `import type { Pred, ValExpr }` ✓；predicate.ts:14 `import { ROLES }`（值导入）✓，使用点 518（`ROLE_LIST`）、544、550 ✓；ref.ts:11 注释含 "(avoids a circular import)" ✓。

**结论**：VERIFIED。当前无运行时环的判断正确（两环各有一侧为 type-only，编译期擦除）。

**修正表述**：无需修正。

**移植建议**：should。Rust 同 crate 内前向引用不成环，此条不阻塞，但下沉 `Role/ROLES` 能让模块边界干净；属于零行为风险的结构性重构。

## CORE-03 — export * 无封装边界（modularity / low）

**原声明要点**：index.ts:2-22 对 22 个模块全部 `export *`；`FN_RESERVED_TOKENS`、`MAX_FN_DEPTH`、4 个 lint*、`splitmix64`、`refSortKey` 均成公共 API。

**核实过程**：通读 index.ts——第 2-22 行共 **21** 条 `export *`（11 个 core + 10 个 content/engine/ui/reach）。上述符号均确为 core 侧导出（parse.ts:682,686；predicate.ts:524,543,564,593；dice.ts:26；ref.ts:43）。

**结论**：PARTIAL。实质（无封装边界、内部助手外泄）完全成立；模块计数错误（21，非 22）。另注：分析报告"对外公共 API 面"一节的部分符号计数也有 ±1 出入（types 21→实为 22、dict 7→8、ref 8→9、sentence 7→8、query 5→6），不影响结论方向。

**修正表述**："index.ts:2-22 对 21 个模块全部 `export *`"。

**移植建议**：should。Rust 侧显式 `pub use` prelude 是正确的；grep 上层用量再定 pub 面不会改变行为（导出可见性不是运行时行为）。

## CORE-04 — splitmix64 位级一致（portability / high）

**原声明要点**：dice.ts:19-42 全程 BigInt；rollDie key 为 `asUintN(64, BigInt(seed)) ^ asUintN(64, BigInt(counter))`（负数二补码）；`sides * r` 大数乘、`>> 64n` 即 floor 除 2^64；`State.rng` 共享计数、一次 m 骰消耗 c..c+m-1；fold 靠 `ROLLED(<total>,<ctrAfter>)` 落位、绝不重掷；测试向量独立计算。

**核实过程**：通读 dice.ts（42 行）：MASK64/GAMMA/MIX 常量 19-22、finalizer 26-31 全 BigInt、rollDie 38-42 与声明逐字一致；注释 8-13 明言共享计数与 c..c+m-1，16-17 明言 python3 独立向量。types.ts:90-95（rng 注释：play 消耗于 draft、fold 从 ROLLED 行落位、replay never re-derives）✓、96-103（decks/DRAWN 同理）✓。test/dice.test.ts:83-128 存在 pinned 向量与连续性断言。seed 为负时 `BigInt(-5)` → `asUintN(64,…)` = 二补码 = Rust `i64 as u64` ✓；`sides < 2^53`、`r < 2^64`，乘积 < 2^117，u128 可容纳 ✓。

**结论**：VERIFIED。u64 wrapping + u128 乘的建议与 JS 语义一一对应，等价性成立。

**修正表述**：无需修正。

**移植建议**：must。逐位一致的心脏；以 test/dice.test.ts 向量（含 asUintN 负数路径）作移植验收门。

## CORE-05 — UTF-16 比较序（portability / high）

**原声明要点**：`sortRefs` 比较器（ref.ts:48）、`relationKey` 的 `a > b`（types.ts:228）按 JS UTF-16 code unit 序；Rust `str: Ord` 是码点序；非 BMP 字符（代理对 D800-DFFF）排在 U+E000..U+FFFF 之前，两侧序不同；名字可经带引号字符串携带任意 Unicode（ref.ts:37 只转义 `"`；lexer.ts:28-37 无字符集限制）；sameRef 亦可能受影响。

**核实过程**：ref.ts:48 比较器用 `<`/`>` ✓；types.ts:225-230 ✓；ref.ts:43-45 refSortKey 构造键 ✓；ref.ts:36-37 只替换 `"` ✓；lexer.ts:28-37 字符串扫描无限制 ✓。UTF-16 vs 码点序的结论独立推演复核：首差异落在 D800-DFFF（非 BMP 高代理）与 E000-FFFF 之间时两序相反，属实。

**结论**：PARTIAL。三处比较点与顺序差异全部属实、风险真实；**但 sameRef 子声明错误**——sameRef（ref.ts:51-53）是 `formatRef(a) === formatRef(b)` 相等比较，与排序语义无关：良构字符串下两侧相等结果恒一致。sameRef 唯一的极端差异来自 JS 可持有孤立代理项而 Rust String（UTF-8）根本无法表示，那是表示层问题不是"序"问题。

**修正表述**："受影响的是两处**排序/规范化键**（sortRefs 的 actor/object 顺序、relationKey 的对称边键方向）；sameRef 是相等比较，不受影响（孤立代理项这类无法在 Rust String 中表示的输入除外）"。

**移植建议**：must。写 `cmp_utf16`（对 `encode_utf16()` 序列比较）替换 sortRefs/relationKey 两处（refSortKey 是键构造，比较发生在前两处）；**不得**直接用 Rust `Ord`。"文档化限 ASCII"是行为变化（JS 接受非 ASCII 名），只能作为 debug 断言补充，不能替代。

## CORE-06 — 插入序可观察（portability / medium）

**原声明要点**：五处插入序承载行为：(1) byName 池 head=最老（types.ts:78-80、engine.ts:120）；(2) entities Map 遍历——existsQuery/countQuery 结果与序无关（predicate.ts:446,484），engine 过期扫描取序敏感（engine.ts:1065）；(3) windows/decks/fired 是 Map/Set；(4) `Object.entries(aspects)` 进渲染（render.ts:33）与 aspectsFromPairs（query.ts:104），JS 整数样键前置规则两侧不复现；(5) unknownCallError 按声明序拼函数表（parse.ts:845-849）、undeclaredTokenError 拼 FN_RESERVED_TOKENS 插入序（parse.ts:784）。

**核实过程**：types.ts:78-80 注释 ✓、engine.ts:120 注释 "HEAD of the byName pool (insertion order, never id sort)" ✓；predicate.ts:446/484 遍历且布尔/计数与序无关 ✓；engine.ts:1065 `const expiring = [...state.entities.values()]...` 且 1238 `for (const exp of expiring)` 按该序产出记录——同一 slot 多个过期实体的日志行序即 entities 插入序，取序敏感属实 ✓；parse.ts:845 `[...fns.byName.keys()]`、849 join ✓、784 ✓；render.ts:33 `Object.entries(a)` ✓、query.ts:104 ✓。整数样键前置是 JS 规范行为 ✓。另抽查 functions.test.ts:112-114/138：函数表按 `one, zero` 声明、消息断言钉 "declared functions: one, zero"——**恰为字母序**，改 BTreeMap 该测试不会红，分析者"换 BTreeMap 前先确认"的谨慎必要且方向正确。

**结论**：VERIFIED。

**修正表述**：无需修正（可补充：现有钉罗列序的测试恰好不区分插入序/字母序，但这是巧合不是保证）。

**移植建议**：must。State 的 entities/byName/relations/decks/windows（Map）与 fired（Set）、FnTable.byName 用 IndexMap（或 Vec+索引）；过期扫描等"产出日志行序"的遍历必须保序；错误消息罗列处若改有序容器须重新对照 192 条消息断言。

## CORE-07 — float64 整数（portability / medium）

**原声明要点**：`Number(num[0])`（lexer.ts:57）、`Number(t)`（predicate.ts:295,323）、`Number(m[1])`（clock.ts:37）——超 2^53 静默丢精度；`String(a.v)` 对 ≥1e21 输出 `"1e+21"` 不可重解析（NUM_RE 拒 `e`）；delta 的 `-0`：`String(-0)`="0"、`v<0` 为 false，规范输出 `+0`（format.ts:84）。

**核实过程**：五处行号逐一属实（lexer.ts:56-58；predicate.ts:294-295、323；clock.ts:37；format.ts:83-84）。JS 语义独立复核：`Number("9999999999999999999999")`=1e22 有损 ✓；`String(1e21)`==="1e+21"（≥1e21 走指数）✓；NUM_RE=/^[0-9]+/ ✓；`-0 < 0` 为 false、`String(-0)` 为 "0" → formatArg 输出 `+0` ✓（-0 可经 `SET(x, -0)` 词法产生：'-' punct + int 0）。

**结论**：VERIFIED。

**修正表述**：无需修正。

**移植建议**：should。i64 解析后超大字面量的两种去向中，"显式拒绝"是行为变化（须与测试核对——当前确无超大整数测试），"复刻 JS 最近偶数舍入"才是零分叉选项；格式化用 i64 Display 对现有合法值逐字节一致；负分钟/负 day 的 debug_assert 合理。

## CORE-08 — 正则面（portability / medium）

**原声明要点**：core 内 13 处正则均无 lookahead/lookbehind/后向引用，Rust regex 可表达；三处语义微差：(1) JS `\s` 含 \uFEFF 不含 \u{85}，Rust White_Space 恰反（影响 lexer.ts:26、query.ts:18）；(2) DUR_RE 交替序 `halfhour|h|m|d` 承重（lexer.ts:15）；(3) 谓词字符串 tokenizer 对未匹配字符静默跳过（predicate.ts:172-178）。

**核实过程**：全量清点 core 正则字面量（grep .exec/.test/.match/.replace/.split/matchAll）：实际 **23 处**——分析者列的 13 处全部属实，另漏计 clock.ts:52（parseDuration 的 matchAll）、lexer.ts:26（/\s/）、lexer.ts:45（/[0-9]/）、query.ts:18（split(/\s+/)）、query.ts:37（/[<>=!]/）、predicate.ts:273、340（replace 比较符压缩）、ref.ts:37、format.ts:98（quote 两个 replace）、parse.ts:770（substString）。漏计的 10 处逐一目验：同样无 lookahead/backref（predicate.ts:273/340 的 `$1` 是**替换侧**捕获展开，非模式内反向引用；regex crate 的 replace 原生支持 `$1`）。(1) JS \s 集合含 \uFEFF、不含 U+0085；Rust White_Space 含 U+0085、不含 U+FEFF——属实，影响 lexer 空白跳过与 query 分词 ✓。(2) 交替序推演：若写 `h|halfhour`，"1halfhour" 匹配 "1h"，且 NUM_RE 守卫（lexer.ts:51）恰好放行，会切成 dur(1h)+name(alfhour)——承重属实；regex crate 同为 leftmost-first，保序即可 ✓。(3) predicate.ts:172-178：`/g` exec 循环遇 `@` 等字符直接推进 lastIndex，静默吞掉——"垃圾字符消失"属实（如 `aspect:x >= @3` 等价 `aspect:x>=3`）。

**结论**：PARTIAL。三个语义微差全部属实、是最容易"顺手修掉"的真风险；正则计数漏数近半，但"零 lookahead/backref、Rust 全部可表达"的结论在 23 处上仍然成立。

**修正表述**："core 内 23 处正则均无 lookahead/lookbehind/后向引用（$1 仅出现在替换串）"。

**移植建议**：must（(3) 的"保留吞字符"语义 + \s 显式集合逐字复刻；(2) 保交替序）。漏计的正则也要一并移植——尤其 clock.ts:52 的 matchAll 与 parse.ts:770，别按"13 处"清单照抄。

## CORE-09 — 词法器 UTF-16 索引（portability / low）

**原声明要点**：`src[i]`/`src[j]` 按 code unit；错误 offset 是 code unit（lexer.ts:40,66）；`\X` 取字面下一字符（lexer.ts:30-32）；`"abc\` 尾反斜杠拼入 `undefined` 成 "abcundefined" 但随后抛 unterminated 不可观察（lexer.ts:31）。

**核实过程**：通读 lexer.ts。code unit 索引 ✓；offset 消息 40/66 ✓；转义 `out += src[j+1]; j += 2`（31 行）——`"\n"` 的值是字母 n ✓；尾反斜杠路径推演：`out += undefined` → "abcundefined"、`j += 2` 越界、33 行抛 unterminated，错误发生在消息可见之前 ✓。format.ts:97-99 quote 只转义 `\` 与 `"`，两侧对称 ✓。

**结论**：VERIFIED。

**修正表述**：无需修正。

**移植建议**：should。字节索引复刻 offset 在 ASCII 前缀下等价（非 ASCII 前缀会有差——错误消息文本本就属 CORE-16 的"逐字复刻"范畴，建议一并按 UTF-16 offset 或统一约定处理）；转义保持"取字面下一字节"。

## CORE-10 — Math.floor 与整除方向（portability / low）

**原声明要点**：Math.floor 在 clockFromAbsMin（16-19）、formatDuration（70-72）、crossedSlots（80-81）；JS `%` 与 Rust `%` 同为截断余号；非负由构造保证（clock.ts:36 注释自证）；负输入下两侧完全不同。

**核实过程**：三处行号与 `Math.floor` 用法 ✓；clock.ts:36 "day needs no check: `\d+` cannot lex a sign" ✓；JS/JS `%` 语义与 Rust i64 `%` 一致（余号随被除数）✓；`div_euclid/rem_euclid` 对非负输入与 Math.floor 除法等价 ✓。

**结论**：VERIFIED。

**修正表述**：无需修正。

**移植建议**：should。div_euclid/rem_euclid + `debug_assert!(m >= 0)`；不要用裸 `/`（向零截断）"简化"。

## CORE-11 — 零 Node API 与随机纪律（portability / low）

**原声明要点**：core 内零 Node API；全 src 树仅 store.ts:3 用 node:fs；test/dice.test.ts:935-940 与 test/decks.test.ts:831-836 有 no-Math.random grep 门禁。

**核实过程**：`grep -rn "node:" src` 全树仅 store.ts:3（readFileSync/writeFileSync）✓；core 11 文件的 import 面核对无环境依赖 ✓；dice.test.ts 门禁测试实际位于 934-941（引用 935-940 基本准确）、decks.test.ts 门禁位于 830-837（引用 831-836 基本准确），二者均扫描 src 全树禁 `Math.random` ✓。

**结论**：VERIFIED。

**修正表述**：无需修正。

**移植建议**：should。移植后保留同款门禁（Rust 侧禁 `rand::` 进入 core/engine crate）。

## CORE-12 — arg() 里不可达的 param 键分支（dead-code / medium）

**原声明要点**：parse.ts:587 `if (this.params?.has(t.v))` 不可达（575 行已提前 return param）；584 行注释与事实不符；`SET(myparam = 3)` 解析报错；spawnArgs（533-537）同型逻辑可达、支持 param 键——SPAWN 与 SET 对 param 键接受度不对称。

**核实过程**：逐行推演 arg()：563 行进入 name 分支 → 575 行 `if (this.params?.has(t.v)) { return { t:'param', … } }` 先于 583 行的 `atPunct('=',1)` 分支，故到达 587 时 `params.has(t.v)` 恒 false，死条件成立 ✓。`SET(myparam = 3)`（函数体内）实跑路径推演：575 行返回 `{t:'param'}` 且只消费 `myparam`，回到 outcome 循环后 `=` 非 `,` 退出、`expectPunct(')')` 抛 "expected ')', got '='"——报错属实（报错点是括号闭合处，非 arg() 内部）✓。spawnArgs 528-538：pair 循环无前置 param 检查，535 行 `if (this.params?.has(k.v))` 可达 ✓。不对称属实。

**结论**：VERIFIED。这是"不得顺手修复"清单里最典型的一条。

**修正表述**：无需修正（可精确化：SET 路径的报错文本是 `expected ')', got '='`，不是 arg() 内的专门错误）。

**移植建议**：must。Rust 端 arg() **不查 param 键**，与 spawnArgs 的差异原样保留并冻结注释；死分支可不移植（不可达，删除零行为影响），但严禁"顺手补齐" SET 的 param 键支持。

## CORE-13 — 六处重复（duplication / medium）

**原声明要点**：(1) numCmp 两份（query.ts:91-100 / predicate.ts:615-624）；(2) AS #id 两份（parse.ts:522-527 / 565-570）；(3) INTO 目标四处（parse.ts:311,376,521,564）；(4) pair 的 param 探测两份（533-537 / 584-590）；(5) peek→param?→role?→ref 两份（roleOrRefArg 502-516 / refTarget 614-626）；(6) delta/pair 规范拼写两份（format.ts:84,91 / parse.ts:799,804）。

**核实过程**：六处逐一对照。(1)(2)(4)(5) 逐字/同型属实。(3) 四处均为 `this.refTarget()` 调用点，同型属实。(6) **delta 两侧逐字相同**；**pair 不同**：format.ts:91 是 `${a.k}=${typeof a.v === 'string' ? quote(a.v) : String(a.v)}`（字符串加引号），parse.ts:804（argSpelling）是 `${a.k}=${String(a.v)}`（裸值）；str case 同理——formatArg 走 `quote(a.v)`（format.ts:85），argSpelling 返回 `a.v` 裸文（parse.ts:800）。CORE-20 注释"canonical equals expansion: byte for byte"（parse.ts:654-655）属实。

**结论**：PARTIAL。重复本身全部属实；**但移植建议"argSpelling 复用 formatArg 的纯拼写层……语义不变"照字面执行会改变行为**：若统一为带引号的 formatArg 拼写，函数体字符串插值的产物会多出引号，破坏逐字节展开等价。两侧只在 delta/int/dur/bool 拼写上真正相同。

**修正表述**："argSpelling 与 formatArg 在 delta/int/dur/bool 上拼写一致、在 str/pair 上**故意不同**（插值不加引号）；收敛时只能共享前者，str/pair 必须保持两套"。

**移植建议**：should（(1)-(5) 收敛为单函数零风险）；(6) 处标红——**禁止**把 argSpelling 的 str/pair 分支并进 formatArg。给"两套拼写必须各自冻结"写进移植规格与测试。

## CORE-14 — 魔法数字与文法双写（clarity / medium）

**原声明要点**：(1) 上界 47 硬编码两处（clock.ts:35、parse.ts:122），已有 HALFHOURS_PER_DAY=48（clock.ts:11）未派生；(2) DUR_RE（lexer.ts:15）重新编码 UNIT_MINUTES（clock.ts:41-46）的单位表，durToMinutes（lexer.ts:71-75）是透传包装；(3) 谓词助手名单（parse.ts:844）复写 predicate.ts 关键字集合（223-225,256,324,346）。

**核实过程**：clock.ts:35 `halfhour > 47`、parse.ts:122 `halfhourTok.v > 47` ✓、clock.ts:11 `HALFHOURS_PER_DAY = 48` ✓；DUR_RE 交替序与 UNIT_MINUTES 键集（halfhour/h/m/d）一致 ✓、durToMinutes 仅 `return parseDuration(s)` ✓；parse.ts:844 名单与 predicate.ts 的 every/any（223）、exists（224）、rel（225）、existsPred（256）、count（324）、total（346）对照 ✓。

**结论**：VERIFIED。

**修正表述**：无需修正。

**移植建议**：should。47 写成 `HALFHOURS_PER_DAY - 1` 是等值重构；单位表单一来源时**必须生成与现 DUR_RE 相同的交替序**（halfhour 在 h 前，见 CORE-08(2)），否则引入新分叉。

## CORE-15 — Dictionary 不一致面（clarity / low）

**原声明要点**：(1) registerRelation 无校验 vs registerVerb 强制 UPPER_SNAKE+查重（dict.ts:52-54 vs 71-76）；(2) isOp 对可变数组线性 includes（dict.ts:29-35,60-62）；(3) 角色名在 outcome-arg 位置被 asRole 拦截（parse.ts:576-581）却不在 RESERVED_WORDS（dict.ts:11-18），子句位置不拦（parse.ts:227-233）——软保留不一致；(4) durToMinutes 透传。

**核实过程**：dict.ts:52-54 registerRelation 直接 set、无 assertNameable ✓；46-50 registerVerb 走 assertNameable（71-76：UPPER_SNAKE 正则 + verbs 查重）✓；OUTCOME_OPS 可变数组 29-35、isOp includes 60-62 ✓；RESERVED_WORDS 11-18 全大写、无小写角色名 ✓；parse.ts:576-581 asRole 先于裸引用 ✓、startsRef 227-233 只拦 verb/CLAUSE_KEYWORDS/EVENT、不拦角色名 ✓——名为 `object` 的实体在结果参数位置无法裸引用、子句位置可以，属实。

**结论**：VERIFIED。

**修正表述**：无需修正。

**移植建议**：should。全部原样复刻；isOp 改 phf/Set 行为相同；"ROLES 是 arg 位置软保留、拦截顺序承重"写进移植规格。

## CORE-16 — 错误消息即 API（semantics / high）

**原声明要点**：test/ 有 221 处 toThrow，大量正则匹配消息片段（/bad clock/、/reserved/、/expected '\('/、/->/、/bad duration/）；core 错误消息为内联英文模板（clock.ts:33,35,54,63；parse.ts:104,187,199,246,251；predicate.ts:107,110,113,116,118 等）。

**核实过程**：`grep -c toThrow test/*.ts` 合计 **192** 行、`grep -o toThrow` 同为 **192** 处（每行一处；`toThrowError` 为 0，src 内为 0）——**"221"是错的**。抽样断言全部命中：/bad clock/ clock.test.ts:26-32、/bad duration/ clock.test.ts:50-53、/reserved/ decks.test.ts:368 与 dice.test.ts:284 与 conditionals.test.ts:139、/expected '\('/ conditionals.test.ts:144、/->/ conditionals.test.ts:146-147；另 functions.test.ts:131-141 钉 unknownCallError 全文片段、138 行钉 "declared functions: one, zero"。core 内联消息行号逐一属实（clock.ts:33,35,54,63；parse.ts:104,187,199,246,251；predicate.ts:107-118）。

**结论**：PARTIAL。实质（消息被测试钉死、内联模板、顺序敏感罗列）完全成立且 high 成立；计数证据错误（192，非 221；该错误还出现在风险清单第 2 条）。

**修正表述**："test/ 有 192 处 toThrow 断言"；风险清单同步改。

**移植建议**：must。"错误消息 = API"写进移植规格、建消息常量表逐条对照、移植这 192 条断言。

## CORE-17 — 三值求值与承重数组序（semantics / medium）

**原声明要点**：evalExpr 返回 `number | undefined`（469-514）；cmp 把 undefined 操作数读 false（451-455）；strict 只在绑定数为 0 时抛（499-505）、绑定全死静默 false（492-498）；every([]) 空真、any([]) 空假（428-435）；total 按绑定数组序累加（490）、path 取第一个存活卡（493）；roleCards 过滤死卡（399-401）。

**核实过程**：全部行号属实：cmp 452-454 `if (l === undefined || r === undefined) return false` ✓；path 493 `bound.find(c => c.deadAction === null)` ✓、499 `if (ctx.strict && bound.length === 0) throw` ✓、495-498 dead-stamp 注释 ✓；every/any 430/434 用 `Array.every/some`（空集语义同 Rust `iter().all/any`）✓；total 490 reduce 沿 roleCards 序 ✓；roleCards 399-401 ✓。

**结论**：VERIFIED。一处小夸张：total 的**和**对整数值与顺序无关（aspect 值通常为整数），顺序真正承重的是 path 的"第一个存活卡"；不影响结论。

**修正表述**：可精确化为"path 无条件序敏感；total 仅在非整数值/溢出下序敏感"。

**移植建议**：must（语义复刻本身；`Option<i64>` + and_then 风格自然）。为 every([])、any([])、全死绑定、strict 零绑定四条边界各写移植测试（should）。

## CORE-18 — aspect 单层点与覆盖语义（semantics / low）

**原声明要点**：aspectsFromPairs（query.ts:102-114）与 aspectSet（types.ts:209-219）遇 `{a:1,'a.b':2}` 静默用子映射覆盖标量（109/217 同型）；aspectEntry/aspectGet 只切第一个点（types.ts:194-206），'a.b.c' 落成 `{a:{'b.c':v}}` 且查询对称可取。

**核实过程**：query.ts:109 `typeof out[head] === 'object' ? out[head] : (out[head] = {})` ✓、types.ts:217 同型 ✓——后写覆盖先写（子映射覆盖标量、标量也能整个覆盖子映射）✓；types.ts:194-206 首个点切分、`sub?.[rest]` ✓——'a.b.c' 双向对称推演成立 ✓。

**结论**：VERIFIED。

**修正表述**：无需修正（可补充：严格说是"后写整值覆盖"，两个方向都静默）。

**移植建议**：should。`enum AspectVal { Num, Sub(…) }` 如实复刻；"单层点 + 整值后写覆盖"写进规格；补一条冲突覆盖行为测试。

## CORE-19 — 隐式不变量（clarity / low）

**原声明要点**：(1) sameRef 对 param ref 抛错（ref.ts:51-53 → 39）；(2) relation 键内嵌 '|'，id 含 '|' 会碰撞，靠引擎铸造 #hex id 兜底（types.ts:225-230）；(3) relationAlive 双向查键（types.ts:232-240）。

**核实过程**：ref.ts:39 param case throw、sameRef 51-53 直接 formatRef 比较（无 try）✓；types.ts:229 `` `${a}|${b}|${type}` ``、228 对称 swap ✓——`a="x|y",b="z"` 与 `a="x",b="y|z"` 同型碰撞成立 ✓；232-240 双向数组查键 ✓。

**结论**：VERIFIED。

**修正表述**：无需修正。

**移植建议**：should。(1)(3) 原样复刻 + 注释（param 不进比较、双向查键是双保险）；(2) newtype EntityId 属 Rust 侧签名设计、只要铸造仍限于引擎即零行为变化——可与 CORE-03 的 pub 面决策一起定，ignore 亦可。

## CORE-20 — 逐字节宏展开（semantics / medium）

**原声明要点**："canonical equals expansion: byte for byte"（parse.ts:653-654）；`{param}` 插值单趟不重扫、只匹配精确拼写、`{ x }` 报 undeclared（769-777）；FN_RESERVED_TOKENS 七 token 原样穿过（682,772）；delta 符号折入（722-728）；substExpr int/delta/dur→int 三分支（825-838）；argSpelling 定义各 arg 拼写（791-808）；环与深度（MAX_FN_DEPTH=8，686）在更早 load 拒。

**核实过程**：逐行核对：引文实际在 654-655（一行偏差，无碍）；substString 770-777（正则、pre/post 非空即抛、保留 token 返回原文、未声明抛）✓；FN_RESERVED_TOKENS = actors/object/with/at/about/to/roll 恰 7 个 ✓；substArg 的 sign 折入 722-728（int/delta 相乘、其余穿透）✓；substExpr 825-838 三分支 ✓；argSpelling 791-808 ✓；MAX_FN_DEPTH=8 ✓；环/深度在 content.ts:545-588（collectFnCalls + 深度检查）load 期拒绝 ✓。substString 正则无 lookahead/backref，regex crate `replace_all` + 闭包等价（JS /g replace 与 replace_all 均为单趟不重叠、不重扫）✓。

**结论**：VERIFIED。

**修正表述**：无需修正（引文行号 653-654 → 654-655）。

**移植建议**：must。逐字节展开是 canonical 等价的心脏；除"插值后不再展开"测试外，为 `{ x }` 报错、保留 token 穿透、delta `+n` 拼写各补一条移植测试。

## CORE-21 — "一文法两前端"的拼串技巧（modularity / low）

**原声明要点**：predViewFromToks 把句子 Tok 重序列化为字符串并记 span（predicate.ts:141-161）；exists()/count() 用 span substring 还原内层查询原文（269-273, 339-340）；先 `replace(/\s*(>=|<=|!=|=|>|<)\s*/g,'$1')` 压空格（273,340，`$1` 为替换侧反向引用）；空格差被 §4.5 `split(/\s+/)` 吸收但错误文本带拼接痕迹。

**核实过程**：predicate.ts:141-161（词邻插空格的 `[A-Za-z0-9_]` 双侧判定、span 记录）✓；exists inner() 269、countQuery inner 339、replace 273/340 ✓；parseQuery query.ts:18 `trim().split(/\s+/)` ✓。`$1` 在 regex crate 的 replace 中原生支持（Captures 展开），结论可达 ✓。

**结论**：VERIFIED。

**修正表述**：无需修正。

**移植建议**：should。保留"拼串 + span 截取"形（错误文本一致），封装成可测函数；**不建议**重构为 Tok 直解（错误文本会漂移，连带 CORE-16 的断言）。

---

## 计划必须吸收（验证为真且 high）

1. **CORE-04（dice 位级一致）**：u64 wrapping + u128 乘 + `i64 as u64` 等价 asUintN；以 test/dice.test.ts 向量为验收门。
2. **CORE-16（错误消息 = API）**：**192** 条（原报告误作 221）toThrow 断言钉死消息文本与罗列顺序；建消息常量表并整体移植断言。

**建议按 must 对待的 medium 级（行为承载，非"顺手修复"可及）**：CORE-05（cmp_utf16 替换 sortRefs/relationKey 两处，不得用 Rust 默认 Ord）、CORE-06（IndexMap 化 State 各容器与 FnTable.byName；过期扫描序即日志序）、CORE-08（谓词 tokenizer 保留吞字符语义 + JS \s 显式集合 + DUR_RE 交替序）、CORE-12（arg() 不查 param 键，SPAWN/SET 不对称冻结）、CORE-17（三值逻辑与 path 首存活卡语义）、CORE-20（逐字节展开拼写）。

## 应驳回

- **无整条驳回**（0 条 REFUTED）。须修正/降级的子声明：
  - CORE-05 的 sameRef 子声明（相等比较不受排序语义影响）——从受影响面中删除。
  - CORE-13 的"argSpelling 复用 formatArg、语义不变"——str/pair 两 case 两侧拼写不同（引号），照字面合并会破坏逐字节展开；只可合并 delta/int/dur/bool。
  - 计数修正：CORE-16 的 221→**192**；CORE-03 的 22→**21** 个模块（分析报告概述节的若干符号计数亦有 ±1 出入，不影响结论）；CORE-08 的 13→**23** 处正则（结论"零 lookahead/backref"在全集上仍成立，但移植清单须按 23 处走）。

## 附：验证中的额外发现（分析报告未提，供移植计划参考）

- test/functions.test.ts:138 钉死的函数罗列序 "one, zero" 恰为字母序——现有测试不排斥 BTreeMap 化，切换后测试不会红但行为仍会漂移；CORE-06 的 IndexMap 建议不能因"测试绿"而放松。
- core 内正则全集（23 处）中漏计的 clock.ts:52（parseDuration matchAll）同样必须移植，别按 CORE-08 的 13 处清单照抄。
