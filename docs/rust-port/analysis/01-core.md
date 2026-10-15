# core/ 移植前分析报告（只读分析，未改动任何源码）

范围：`src/core/` 全部 11 个文件 + `src/index.ts`（共 ~2437 行）。所有 file:line 均在 worktree `/Users/amias/Projects/chorus/.worktrees/rust-port` 下核实。

## 模块职责概述

- **types.ts** (240 行)：核心数据模型。State（entities/byName/relations Map、rng 计数、decks 游标、windows 锁表）、cloneState 深拷贝、名字池（stackable）、aspect 点路径读写、relation 规范键。
- **clock.ts** (84)：半点(30min)机械时钟、绝对分钟换算、时长字面量 parse/format、crossedSlots。
- **dict.ts** (92)：L1 词典——核心动词、保留词、结果操作词表（含 ROLL/ROLLED/DRAW/DRAWN）、关系类型注册。
- **ref.ts** (53)：引用（name/kind:NAME/#id/param）的构造、规范格式化、排序、相等。
- **sentence.ts** (77)：句子/子句/Outcome/Arg 的 AST 类型（纯类型 + emptyClause）。
- **lexer.ts** (75)：词法器——name/int/dur/str/#idref/punct，引号感知、'->' 与时长合词。
- **parse.ts** (853)：递归下降句法器 + **函数宏子系统**（FnTable、函数体解析、collectFnCalls、expandFnCalls 展开与字符串插值替换）。
- **format.ts** (107)：规范序列化器（Format∘Parse = canonical），actor 排序、预动词 AT、SPAWN/NARRATE/REPLACE/ENDING 特例拼写。
- **query.ts** (114)：§4.5 槽位查询小语言（kind:/aspect:/state:/name: 项，带否定与比较）+ aspectsFromPairs。
- **predicate.ts** (678)：§6.3 条件谓词——字符串与句子 Tok 双前端解析器、求值器（tri-state）、count/total/roll 操作数、load 期 lint（roles/trigger/roll-may-bind）。
- **dice.ts** (42)：引擎唯一随机源——counter-based splitmix64，BigInt 全程精确。

## 模块间依赖

```
dice(无依赖)
clock → types(type)
dict(无依赖)
ref(无依赖; SafeDict 接口刻意避免 ref→dict 环, ref.ts:11)
sentence → types(type), ref(type), predicate(type)
types → sentence(type: Role), ref(type)          ← 与 sentence 互为 type 级环
lexer → clock
query → types
format → sentence(type), ref, clock
parse → lexer, sentence, dict, predicate, ref(type), clock
predicate → types, query, sentence(值依赖: ROLES!), lexer(type)  ← sentence↔predicate 跨向环
index.ts → 上述全部 `export *`（另含 content/engine/ui/reach）
```

当前无**运行时**循环（环的两侧恰都是 `import type`，编译期擦除），但分层已倒挂：模型层 types 依赖 AST 层 sentence 的 `Role`（types.ts:2），AST 层 sentence 依赖谓词层 predicate 的 `Pred/ValExpr`（sentence.ts:4）。

## 对外公共 API 面

`src/index.ts:2-22` 对 11 个 core 模块做 `export *`，即**全部导出符号都是公共 API**：types 21 个符号（State/newState/cloneState/addEntity/releaseName/oldestNamed/getEntity/getAlive/aspectEntry/aspectGet/aspectSet/attrGet/relationKey/relationAlive + 9 个类型）；clock 10 个（absMin/clockFromAbsMin/nextStepClock/formatClock/parseClock/parseDuration/formatDuration/crossedSlots/两常量 + Clock 重导出）；dict 7 个（Dictionary/coreDictionary/OUTCOME_OPS/RESERVED_WORDS/CORE_VERBS/KINDS/OutcomeOp/RelationTypeDef）；ref 8 个；sentence 7 个；lexer 2 个（lex/Tok）；parse 9 个（parseSentence/parseOutcomes/parseFunctionBody/collectFnCalls/expandFnCalls/FnTable/FnTemplate/FN_RESERVED_TOKENS/MAX_FN_DEPTH）；format 1 个（formatSentence）；query 5 个；dice 2 个（splitmix64/rollDie）；predicate 15 个（含 4 个 lint* 与 parsePredicateToks/parseValueExprToks）。Rust 侧需自行收窄为 prelude + 显式 pub。

---

## Findings

### CORE-01 [类别: modularity] [严重度: medium]
**问题**：parse.ts 一个文件混杂三个子系统——(a) 句子解析器（`Parser` 类，parse.ts:86-640）、(b) 函数宏模板系统（`FnTemplate/FnTable/parseFunctionBody/collectFnCalls`，parse.ts:25-84）、(c) 编译期宏展开（`expandFnCalls/substArg/substString/argSpelling/substPred/substExpr`，parse.ts:646-853）。(b)(c) 是 content 编译期机制，与句子文法无关。
**证据**：parse.ts 共 853 行；expandFnCalls 一节自带独立注释块（parse.ts:648-675）；`FN_RESERVED_TOKENS`/`MAX_FN_DEPTH` 是展开层常量（parse.ts:682,686）。
**为何重要**：853 行超载；展开层只被 content compile 调用，却通过 `export *` 进入公共面。
**移植建议**：Rust 侧拆为 `parse_sentence` / `parse_outcome` / `fn_expand` 三个模块（或 parser 内 `mod expand`）。纯搬家不改行为。

### CORE-02 [类别: modularity] [严重度: medium]
**问题**：分层倒挂 + 两处跨向依赖环：(1) types.ts（模型层）从 sentence.ts 导入 `Role`（types.ts:2；用于 `OpenWindow.refs: Record<Role, Ref[]>`，types.ts:71），而 sentence.ts 又从 types.ts 导入 `Clock`（sentence.ts:2）——type 级双向环；(2) sentence.ts 从 predicate.ts 导入 `Pred/ValExpr`（sentence.ts:4），predicate.ts 反向从 sentence.ts **以值导入 `ROLES`**（predicate.ts:14，用于 predicate.ts:518,544,550）。ref.ts:11 注释自证分层紧张："minimal dictionary surface the formatter needs (**avoids a circular import**)"。
**为何重要**：目前无运行时环全靠"环的一侧恰为 type-only"这一巧合；日后任何一处改成值导入就会成环。Rust 同 crate 内环可编译，但会把类型布局/可见性纠缠在一起。
**移植建议**：把 `Role/ROLES` 下沉到独立小模块（或 types），让依赖单向：model ← ast ← pred。行为不变。

### CORE-03 [类别: modularity] [严重度: low]
**问题**：index.ts 对 22 个模块全部 `export *`（src/index.ts:2-22），core 的所有内部助手（`FN_RESERVED_TOKENS`、`MAX_FN_DEPTH`、4 个 lint*、`splitmix64`、`refSortKey`）都是公共 API，无封装边界。
**为何重要**：Rust 需要显式决定 `pub` 面；照单全收会把内部结构冻结成兼容负担。
**移植建议**：core 建一个 `pub use` prelude，只列上层真正引用的符号；grep 确认上层用量后再定。

### CORE-04 [类别: portability] [严重度: high]
**问题**：唯一随机源是 counter-based splitmix64，用 BigInt 保证全程 64 位精确（dice.ts:19-42）：`rollDie` 的 key 是 `BigInt.asUintN(64, BigInt(seed)) ^ BigInt.asUintN(64, BigInt(counter))`（负数按二补码），乘 `sides` 必须在 128 位里做（`sides * u64` 溢出 f64 会量化低位），`>> 64n` 即 floor 除 2^64（dice.ts:38-42）。`State.rng` 是共享计数器，一次 m 骰消耗 c..c+m-1（dice.ts:9-13）；fold 靠具体 `ROLLED(<total>, <ctrAfter>)` 行落位，**重放绝不重掷**（types.ts:90-95）。
**证据**：dice.ts:26-31（finalizer 全 BigInt）、dice.ts:38-42、types.ts:90-95、types.ts:96-103（decks 游标同理）。测试向量独立用 python 大整数算过（dice.ts:16-17）。
**为何重要**：这是"逐位一致"要求的心脏；注释里明言"never through float64"。
**移植建议**：Rust 用 `u64` wrapping 运算（`wrapping_add/mul`）+ `u128` 乘法直接一一对应；seed/counter 为 i64 时先 `as u64`（等价 asUintN）。保留 test/dice.test.ts 向量作移植验收。这是**机械但零容错**的一块。

### CORE-05 [类别: portability] [严重度: high]
**问题**：JS 字符串比较是 **UTF-16 code unit 序**，且它是行为承载的：`sortRefs` 的比较器用 `<`/`>`（ref.ts:48），`relationKey` 用 `a > b` 决定对称边规范键（types.ts:228），`refSortKey` 同（ref.ts:43-45）。Rust `str: Ord` 是字节序（=码点序）。两者对 **非 BMP 字符（emoji 等，UTF-16 里是 D800-DFFF 代理对，排在 U+E000..U+FFFF 之前）** 会给出不同顺序。名字可通过带引号字符串携带任意 Unicode（ref.ts:37 只对 `"` 转义；lexer 字符串无字符集限制，lexer.ts:28-37）。
**证据**：ref.ts:47-49、types.ts:225-230、ref.ts:36-37、lexer.ts:28-37。
**为何重要**：canonical 句子的 actor/object 顺序、relation 键、以及 `sameRef`（ref.ts:51-53，走 formatRef 字符串比较）都可能因一个 emoji 名字而与 JS 输出不同序。
**移植建议**：写一个 `cmp_utf16` 助手（对 `encode_utf16()` 序列比较）替换这三处比较；或文档化"实体名限 ASCII"并加 debug 断言。**不得**直接用 Rust 默认 `Ord` 了事。

### CORE-06 [类别: portability] [严重度: medium]
**问题**：JS 的 Map/Set/Object **插入序**在多处是可观察行为：(1) `byName` 池数组"head = 最老副本"是寻址语义（types.ts:78-80；engine.ts:120 注释重申）；(2) `entities` Map 插入序被 `existsQuery/countQuery` 遍历（predicate.ts:446,484——结果与序无关，安全）也被 engine 的过期扫描遍历（engine.ts:1065，取序敏感）；(3) `windows`/`decks`/`fired` 是 Map/Set；(4) `Object.entries(aspects)` 的键序进入渲染（render.ts:33）与 `aspectsFromPairs`（query.ts:104）；JS 对象还有**整数样键排最前**的规则（如键 `"42"` 会先于插入序输出），Rust HashMap/BTreeMap 均不复现；(5) `unknownCallError` 把 `fns.byName.keys()` 按**声明序**拼进错误消息（parse.ts:845-849）；`undeclaredTokenError` 拼 `FN_RESERVED_TOKENS` 插入序（parse.ts:784）。
**为何重要**：Rust `HashMap` 迭代序随机；任何"第一个匹配""错误消息罗列顺序"都会漂移。
**移植建议**：State 各 Map 与 FnTable.byName 用 `IndexMap`（或 Vec+索引）；错误消息罗列处若换 BTreeMap 需先确认 JS 侧消息测试未锁声明序。`entities` 的遍历处改用带 birth 序字段的选择也无不可，但 IndexMap 最省心。

### CORE-07 [类别: portability] [严重度: medium]
**问题**：整数走 float64：词法器 `Number(num[0])`（lexer.ts:57）、谓词整数 `Number(t)`（predicate.ts:295,323）、clock `Number(m[1])`（clock.ts:37）。超过 2^53 的字面量**静默丢精度**；`formatArg` 的 `String(a.v)` 对 ≥1e21 输出指数形式 `"1e+21"`（format.ts:83），不再可重解析（NUM_RE 拒绝 `e`）。delta 的 `-0`：`String(-0)` 是 `"0"`，`v < 0` 为 false，规范输出 `+0`（format.ts:84）。
**证据**：lexer.ts:56-58、predicate.ts:294-295、predicate.ts:323、format.ts:83-84。
**为何重要**：Rust 用 i64 后，超大字面量的行为（报错 vs 精确保留）与 JS 不同；格式化分支（`String` 的指数/-0 规则）无 i64 对应物。
**移植建议**：i64 解析并在 >2^53 时**复刻 JS 最近偶数舍入**（`as f64 as i64` 链）或显式拒绝（后者是行为变化，需与现有测试核对——当前测试未覆盖超大整数）；格式化用 i64 Display（对现有合法值逐字节一致）。把"负数分钟/负 day 不可达"写成 debug_assert。

### CORE-08 [类别: portability] [严重度: medium]
**问题**：core 内 13 处正则（clock.ts:32；dict.ts:72；lexer.ts:13-15；query.ts:43,48,61；predicate.ts:155,172,294,323；ref.ts:16）**均无 lookahead/lookbehind/后向引用**——Rust `regex` crate 全部可表达。但有三处语义微差要盯：(1) JS `\s` 含 `\uFEFF`、不含 `\u{85}`；Rust regex 的 Unicode `\s`（White_Space）恰相反——影响 lexer 空白跳过（lexer.ts:26）与 query 分词（query.ts:18）；(2) DUR_RE 的交替序 `halfhour|h|m|d`（lexer.ts:15）是承重的——若写成 `h|halfhour`，`1halfhour` 会切成 `1h`+`alfhour`（Rust regex 也是 leftmost-first，保持原序即可）；(3) **谓词字符串 tokenizer 对未匹配字符静默跳过**：`re.exec` 循环遇到不在字符集里的字符（如 `@`）直接略过不报错（predicate.ts:172-178）——`when:` 行里的垃圾字符被吞。
**为何重要**：(3) 是"bug 当特性"，移植时最容易"顺手修掉"从而改变接受/拒绝行为。
**移植建议**：Rust 端逐条复刻：`\s` 用显式 JS 集合 `[\t\n\v\f\r \u{a0}\u{1680}\u{2000}-\u{200a}\u{2028}\u{2029}\u{202f}\u{205f}\u{3000}\u{feff}]`；tokenizer 保留"跳过不报错"语义并加注释禁止"修复"。

### CORE-09 [类别: portability] [严重度: low]
**问题**：词法器按 UTF-16 code unit 索引：`src[i]`、`src[j]`（lexer.ts:25-66），错误消息里的 offset 也是 code unit（lexer.ts:40,66）。非 BMP 字符在 JS 里 `src[i]` 取到孤立代理项；字符串转义 `\X` 取**字面**下一字符（`\n` 即字母 n，lexer.ts:30-32）。极端输入 `"abc\`（引号内尾反斜杠）会执行 `out += src[j+1]` 把 `undefined` 拼成 `"abcundefined"`（lexer.ts:31）——随后仍抛 unterminated，故不可观察，但 Rust 移植时需显式处理越界分支。
**为何重要**：错误消息中的字节/字符 offset 会与 JS 不一致；转义语义必须原样保留（quote() 只转义 `\` 和 `"`，format.ts:97-99，两侧对称才可 round-trip）。
**移植建议**：Rust 用字节索引复刻错误 offset（ASCII 场景等价）；转义保持"取字面下一字节"。

### CORE-10 [类别: portability] [严重度: low]
**问题**：`Math.floor` 出现在除法/取模处：clockFromAbsMin（clock.ts:16-19）、formatDuration 分解（clock.ts:70-72）、crossedSlots（clock.ts:80-81）。JS `%` 与 Rust `%` 同为截断余号，但 `/` 不同：JS 有 `Math.floor` 包裹时为向下取整，Rust 整除是向零截断。当前所有输入按构造非负（clock.ts:36 注释自证 day>=0），负分钟一旦出现，JS 给 `{day:-1,halfhour:-1}` 型结果而 Rust 完全不同。
**为何重要**：不变量靠约定不靠类型；Rust 侧若有人"简化"掉 floor 就悄悄分叉。
**移植建议**：`clockFromAbsMin`/`crossedSlots` 用 `div_euclid`/`rem_euclid`（非负输入下与 Math.floor 一致），并加 `debug_assert!(m >= 0)`。

### CORE-11 [类别: portability] [严重度: low]
**问题**：Node 专属 API 在 core/ 内为**零**；全 src 树仅 engine/store.ts:3 用 `node:fs`（存档读写，属 IO 边缘）。随机性纪律由 grep 门禁测试强制：test/dice.test.ts:935-940 与 test/decks.test.ts:831-836 扫描 src 禁止出现 `Math.random`。
**为何重要**：这是极好的移植基线——core 已是纯函数式、无时钟无环境依赖；Rust 侧保持"无 rand crate、无系统时间入引擎"的纪律即可，且建议移植后保留同款 grep 门禁。
**移植建议**：在 CI/测试里保留 no-Math.random 门禁（对 Rust 即 no `rand::` in engine/core crate）。

### CORE-12 [类别: dead-code] [严重度: medium]
**问题**：parse.ts:587 `if (this.params?.has(t.v)) pName = t.v` **不可达**——同名判断在 parse.ts:575 已提前 `return { t: 'param', ... }`，能走到 583 行的 `t.v` 必不是参数。其上的注释（parse.ts:584-585 "a parameter as the pair's key OR value replaces the whole pair"）与事实不符：通用 `arg()` 里 **param 作 pair 键**的路径不存在，实际行为是 `SET(myparam = 3)`（param 键、括号内通用 arg 位置）解析报错；而 `spawnArgs` 里的同型逻辑（parse.ts:533-537，该处可达）支持 param 键。
**证据**：parse.ts:543-592（575 提前 return；587 死条件）、对照 parse.ts:528-538。
**为何重要**：这是行为怪癖：同一语法位置在 SPAWN 与 SET 里对 param 键的接受度不同。移植时最容易"顺手修复"造成行为分叉。
**移植建议**：Rust 端**原样复刻**（arg() 不查 param 键），删除死分支仅限编译器警告层面；在本报告或代码注释里记录"非 bug"。

### CORE-13 [类别: duplication] [严重度: medium]
**问题**：多处逐字/近逐字重复：(1) `numCmp` 完全相同地存在两份——query.ts:91-100 与 predicate.ts:615-624；(2) `AS #id` 参数构造两份——parse.ts:522-527 与 565-570；(3) `INTO` 目标解析四处同型——parse.ts:311, 376, 521, 564；(4) pair 的 param 键/值探测两份——parse.ts:533-537 与 584-590；(5) "peek→param?→role?→ref" 三连两份——`roleOrRefArg`（parse.ts:502-516）与 `refTarget`（parse.ts:614-626）；(6) delta/pair 的规范拼写逻辑两份——format.ts:84,91 与 parse.ts:799,804（argSpelling 重写了 formatArg 的子集）。
**为何重要**：重复 = 移植时两处独立翻译、日后两处漂移；argSpelling 与 formatArg 的 `+` 符号规则必须永远一致（canonical 展开"byte for byte"，parse.ts:653-654）。
**移植建议**：Rust 各收敛为一个函数（`num_cmp`、`parse_as_id`、`parse_into_target`、`arg_spelling` 复用 `format_arg` 的纯拼写层）。语义不变。

### CORE-14 [类别: clarity] [严重度: medium]
**问题**：魔法数字与文法双写：(1) 半点上界 `47` 硬编码两处——clock.ts:35 与 parse.ts:122（已有 `HALFHOURS_PER_DAY`，clock.ts:11，却没派生）；(2) 时长单位文法双写：DUR_RE（lexer.ts:15）重新编码了 clock.ts:41-46 `UNIT_MINUTES` 的单位表，加单位要改两文件（lexer.ts:71-75 的 `durToMinutes` 只是 parseDuration 的透传包装，纯属间接层）；(3) 谓词助手名单 `'exists','count','total','rel','every','any'`（parse.ts:844）复写了 predicate.ts 解析器的关键字集合（predicate.ts:223-225,256,324,346）。
**为何重要**：双写文法是行为分叉的温床；47 若有人改成 `HALFHOURS_PER_DAY-1` 之外的值会静默放开非法 clock。
**移植建议**：Rust 中单位表单一来源（const 表 + 从表生成 lexer 分支）；47 写成 `HALFHOURS_PER_DAY - 1`；助手名单集中一处常量。

### CORE-15 [类别: clarity] [严重度: low]
**问题**：(1) `Dictionary.registerRelation` 不做任何名字校验，而 `registerVerb` 强制 UPPER_SNAKE + 查重（dict.ts:52-54 vs 71-76）——关系类型可以注册任意字符串；(2) `isOp` 对**可变导出数组** `OUTCOME_OPS` 做线性 `includes`（dict.ts:29-35,60-62）；(3) 角色名（OBJECT/ACTORS/…，sentence.ts:8）在 outcome-arg 位置被 `asRole` 优先拦截（parse.ts:576-581），却**不在** `RESERVED_WORDS`（dict.ts:11-18）里——名为 OBJECT 的实体在结果参数里无法裸引用（子句位置却可以，parse.ts:227-233 的 startsRef 也不拦 ROLES），是"软保留"的不一致；(4) `durToMinutes` 透传包装（lexer.ts:71-75）。
**为何重要**：都是移植时需要"决策是否原样保留"的点；软保留词最阴——现有解析行为已经依赖拦截顺序。
**移植建议**：Rust 端先原样复刻（isOp 用 phf/Set 无妨，行为相同）；把"ROLES 是 arg 位置软保留"写进移植规格。OUTCOME_OPS 若改 `const` 断言/冻结不影响行为。

### CORE-16 [类别: semantics] [严重度: high]
**问题**：**错误消息文本是被测试钉死的行为**。test/ 目录有 221 处 `toThrow(...)` 断言，大量以正则匹配消息片段（如 `/bad clock/`、`/reserved/`、`/expected '\('/`、`/->/`、`/bad duration/`）。而 core 的错误消息全是内联英文模板串（如 clock.ts:33,35,54,63；parse.ts:104,187,199,246,251；predicate.ts:107,110,113,116,118 等）。
**证据**：`grep -c toThrow test/*.ts` = 221；抽样见上文正则。
**为何重要**：Rust 重写消息时任何一个措辞漂移都会打破测试；且 `unknownCallError`（parse.ts:843-853）、`undeclaredTokenError`（parse.ts:781-788）等消息还内嵌**顺序敏感**的罗列（见 CORE-06）。
**移植建议**：把"错误消息 = API"写进移植规格；建立消息常量表并逐条对照测试正则；Rust 测试直接移植这 221 条断言。

### CORE-17 [类别: semantics] [严重度: medium]
**问题**：谓词求值是**三值逻辑**：`evalExpr` 返回 `number | undefined`（predicate.ts:469-514），`cmp` 把任一 undefined 操作数读作 false（predicate.ts:451-455）；`strict` 只在角色绑定卡数为 0 时抛（predicate.ts:499-505），绑定存在但全死则**静默 false**（predicate.ts:492-498 注释明言 dead-stamp 规则）；`every([])` 空真、`any([])` 空假（predicate.ts:428-435，JS `Array.every/some` 语义）；`total` 求和按**角色绑定数组顺序**逐卡累加（predicate.ts:490），`path` 取**第一个**存活绑定卡（predicate.ts:493）——绑定数组的顺序本身是承重语义（来自 slot 解析序）。
**证据**：predicate.ts:399-401（roleCards 死卡过滤）、428-435、451-455、489-507。
**为何重要**：Rust 的 Option<i64> 映射自然，但"undefined→false"与"strict 例外"的每个分支都要逐条对照；`iter().all()/any()` 的空集行为恰好同 JS，但值得用测试钉住。
**移植建议**：`Option<i64>` + 显式 `and_then` 风格复刻；为 `every([])`、`any([])`、全死绑定三种边界各写一条移植测试。

### CORE-18 [类别: semantics] [严重度: low]
**问题**：aspect 一层嵌套 + 点叶子键是**脆弱约定**：`aspectsFromPairs`（query.ts:102-114）与 `aspectSet`（types.ts:209-219）遇 `{a:1,'a.b':2}` 冲突时**静默用子映射覆盖标量**（`typeof out[head] === 'object' ? … : (out[head] = {})`，query.ts:109；types.ts:217 同型）；而 `aspectEntry/aspectGet` 只支持一层点（types.ts:194-206），键 `'a.b.c'` 落成 `{a:{'b.c':v}}`、查询 `a.b.c` 恰好也能取到——两侧靠"都只切第一个点"对称才成立。
**为何重要**：Rust 用 `HashMap<String, AspectVal>` 复刻时要保留"标量被静默覆盖"的次序敏感性（后写覆盖先写），以及单层点语义；改成真嵌套树就是行为变化。
**移植建议**：类型上如实复刻 `enum AspectVal { Num, Sub(BTreeMap/IndexMap) }`；把"单层点 + 覆盖语义"写进移植规格；冲突覆盖建议加一条行为测试钉住。

### CORE-19 [类别: clarity] [严重度: low]
**问题**：(1) `sameRef` 对 param 形 ref 会抛错（ref.ts:51-53 → formatRef case 'param' throw，ref.ts:39）——比较操作抛异常而非返回 false，契约外即 panic；(2) relation 键内嵌 `'|'` 分隔（types.ts:225-230），id 含 `|` 会碰撞，当前靠"引擎铸造 #hex id"的隐式约定兜底；(3) `relationAlive` 故意双向查键（types.ts:232-240），与 relationKey 的规范化形成双保险——Rust 侧保留两处还是收敛为一处需记录。
**为何重要**：都是隐式不变量（param ref 不进比较、id 字符集受限），Rust 类型系统可以接管但会改变签名。
**移植建议**：Rust 用 `newtype EntityId`（构造受限）消除 (2)；(1)(3) 原样复刻并注释。

### CORE-20 [类别: semantics] [严重度: medium]
**问题**：函数宏展开必须**逐字节可复现**："Canonical equals expansion: a call site IS its expanded body, byte for byte"（parse.ts:653-654）。关键规则：`{param}` 字符串插值是**单趟不重扫**、只匹配**精确拼写**（`{ x }` 带空格直接报 undeclared 错，parse.ts:769-777）；`FN_RESERVED_TOKENS` 七个保留 token 原样穿过（parse.ts:682,772）；delta 符号折入绑定（parse.ts:722-728）；vexpr 内参数替换走 substExpr 的 int/delta/dur→int 三分支（parse.ts:825-838）；`argSpelling` 定义每种 arg 的插值拼写（ref 带 kind、delta 带符号、dur 走 formatDuration，parse.ts:791-808）。调用图环与深度（MAX_FN_DEPTH=8，parse.ts:686）在更早的 load 阶段拒。
**为何重要**：展开产物直接进日志与规范比较，任何拼写差（如 `+3` vs `3`）都会让"canonical equals expansion"破产。
**移植建议**：Rust 端 substString 的正则 `/\{(\s*)([A-Za-z_][A-Za-z0-9_]*)(\s*)\}/g` 可用 regex crate 表达；替换单趟语义用 `replace_all` + 闭包即可（regex crate 无重扫问题）；为"插值后不再展开"写一条移植测试。

### CORE-21 [类别: modularity] [严重度: low]
**问题**：`parsePredicateToks` 前端要把句子 Tok **重序列化回字符串**再走字符串解析器：`predViewFromToks` 拼接 token 文本、按词邻插入空格、记录 span（predicate.ts:141-161），`exists()/count()` 再用 span 做 `src.substring(spans[first].start, spans[close].start)` **还原内层查询原文**交给 `parseQuery`（predicate.ts:269-273,339-340），还要先 `replace(/\s*(>=|<=|!=|=|>|<)\s*/g,'$1')` 压掉比较符两侧空格（predicate.ts:273,340，注意 `$1` 是**反向引用式替换**，Rust regex 需用捕获组展开 `$1` 等价实现）。
**为何重要**：这是"one grammar, two front-ends"的实现技巧，链条长且每环都可能引入空格差（空格差会被 §4.5 的 split(/\s+/) 吸收，但**错误消息里的原文**会带这些拼接痕迹，见 CORE-16）；Rust 若直接从 Tok 解析查询会更干净但错误文本会变。
**移植建议**：移植时保留"拼串 + span 截取"的形（错误文本一致），还是重构为 Tok 直解（错误文本变）——**建议保留**，并在 Rust 侧把该字符串构造封装成可测函数。

---

## 移植风险清单（最难移植的 5 个点）

1. **骰子位级一致（CORE-04）**：splitmix64 BigInt 链 + asUintN 二补码 XOR + 128 位乘、`State.rng`/`decks` 游标靠 `ROLLED(<total>,<ctrAfter>)`/`DRAWN` 行落位。Rust 用 u64 wrapping + u128 乘机械直译，但**零容错**——以 test/dice.test.ts 的独立向量为验收门。
2. **字符串序与错误文本即行为（CORE-05 + CORE-16 + CORE-21）**：UTF-16 code unit 比较序（sortRefs/relationKey）≠ Rust 字节序；221 条 `toThrow` 断言钉死错误消息；predViewFromToks 拼串还原内层查询的原文痕迹。三者都要求"连消息带顺序逐字复刻"。
3. **插入序语义（CORE-06）**：byName 池 head=最老、windows/decks/fired、FnTable 声明序进错误消息、Object 键序（含整数样键前置）——Rust 必须 IndexMap 化，否则顺序类行为随机漂移。
4. **三值求值与承重的数组序（CORE-17）**：`undefined→false`、strict 的"零绑定才抛"、every/any 空集、total/path 依赖角色绑定数组顺序——全是静默分支，差一处就是语义分叉。
5. **文法双写与行为怪癖的"不得修复"清单（CORE-08 + CORE-12 + CORE-14）**：静默吞字符的谓词 tokenizer、`\s` 集合差异、DUR_RE 交替序、arg() 里不可达的 param 键分支（与 spawnArgs 行为不一致）、硬编码 47——这些"bug 当特性"必须原样迁移并冻结注释，防止 Rust 侧顺手修正。

**基线利好**：core/ 无任何 Node API、无 Math.random（有 grep 门禁，CORE-11）、正则零 lookahead/backreference（CORE-08）、dice 已 BigInt 化规避浮点——移植面比典型 TS 项目干净得多。
