# W2 波次门评审记录（§7 规则 13，PR3-04）

- 评审对象：`rust-port-w1..HEAD`（4 commit：c8098d2 content/content.rs + ba52124 content/script.rs + 6d0c25f messages.md 对账 + **897511c must-fix-1 修复**；含 tests/lint.rs、tests/script_play.rs 编译器子集、decks.rs W2 增补、core/deck_rng.rs 并入删除）
- 评审人：wave integrator 指派的独立 reviewer（非本波作者）
- 评审日期：2026-10-14（初审，判 FAIL）/ 2026-10-15（897511c 复核）
- 证据方式：(a) 项按规则做 **content 全量逐段对照**（rust/src/content/{script,content}.rs vs src/content/{script,content}.ts 逐函数核对算法/顺序/错误文本/边界；script.rs 1556 行 / content.rs 1190 行全量，缩进敏感解析器重点段——meta 正则五选一交替、BRACK/indent/whens 归一化、buildFnTable 三遍、deck 置换接线、load 期校验序——逐段走查）；(b)–(g) 逐项 grep/对账；`cargo test -j 4` 与 `cargo clippy -j 4 --all-targets` 亲自重跑（rust/ 下，无 vitest/tsx 并发）

## 总评：**PASS-with-notes（最终结论，含 897511c 复核）**

初审 162/162 全绿、clippy 零警告、TS 侧零改动，但发现 1 条 must-fix（判 FAIL）：`when`/`query` **空字符串的 JS falsy 语义丢失**。修复 commit 897511c 复审通过（证据见下节）：**163/163 测试全绿**（59 lib + 12 clock + 4 decks + 4 dice + 9 fn_expand_direct + 2 gate + 14 lint + 25 query_predicate + 15 script_play + 18 sentence，新增 1 条回归测试），`cargo clippy -j 4 --all-targets` 零警告，TS 侧零改动（`git diff --stat rust-port-w1..HEAD -- src/ test/ scripts/` 为空）。must-fix 完全消除、无新引入分歧。遗留 should-fix 2 条 + note 4 条（均不阻塞）。可打 `rust-port-w2` tag。

---

## must-fix 复核（2026-10-15，commit 897511c）

**must-fix-1（空串 `when`/`query` falsy 语义）— 已消除。**
- 修复实现（仅触 rust/src/content/content.rs，+66/−8）：slot query 点改为 `match s.query.as_deref() { Some(q) if !q.is_empty() => Some(parse_query(q)?), _ => None }`（:466–470）；recipe 侧引入 `let raw_when = r.when.as_deref().filter(|w| !w.is_empty());`（:486），**同一过滤值同时喂 when parse（:487–490）与 id 铸造（:495–498）**——与 TS 的三处 truthy 门一一对应。
- 对照 content.ts 原文复核：`:224` `s.query ? parseQuery(s.query) : undefined`（'' falsy → undefined）；`:229` `r.id ?? (r.when ? …recipe${i} : …default)` 与 `:230` `r.when ? parsePredicate(r.when) : null`（'' falsy → `.default` + null，不 parse）。`is_empty()` 恰是 JS 字符串 truthy 判据：仅 `''` 翻转，纯空白串（`" "`）两侧仍 truthy → 两侧同报 `empty predicate ' '` / `bad query term ' '`——逐点等价，无过度/不足过滤。
- 求值序核对：id 铸造移到 when parse 之后（对齐对象字面量求值序），但 id 计算不可能 throw，重排不可观察；每 recipe 内错误先后与跨 recipe 顺序不变。
- 覆盖面核对：可观察分歧面恰为 `compile_content` 的手构 `ContentRaw` 输入（`compile_script`/`lint_script` 只收文本，`parse_script` 文法产生不了 `''`）——修复点全覆盖。
- 回归测试 `empty_string_when_and_query_are_falsy_like_js`（content.rs 内嵌）：手构 raw（`when: Some("")` + `query: Some("")`）→ 断言 `query.is_none()`、`recipe.when.is_none()`、`id == "wait.default"` 三件套，恰钉 TS 语义的三个可观察面；非空路径由既有测试钉住（study.recipe0、slot query、lint 套件）。
- reviewer 亲跑：`cargo test -j 4` 163 passed（含新测试）、`cargo clippy -j 4 --all-targets` 0 warning、`git diff --stat rust-port-w1..HEAD -- src/ test/ scripts/` 仍为空、commit 触面仅 rust/src/content/content.rs。闭环。
- 遗留（降级为 note-4，非分歧）：script.rs:1273 lint 镜像仍读未过滤的 `r.when.as_deref()`——但 `lint_script` 只收文本、parse 产生不了 `''`，TS 侧同一 truthy 门同样永不见 `''`，两侧均无可达输入，不构成行为分歧；建议后续加同型过滤或一行注释防未来写入面变化。

---

## (a) 逐段对照（content 全量）— PASS 除 must-fix-1 一处

**script.rs ↔ script.ts（620 行，逐段）**

- **parse_script 主循环**：`replace(/\t/g,'  ')` → `replace('\t',"  ")`（全量替换同型）；`line.trim()`/`trimStart()` → `js_trim`/`js_char_is_ws` take-while（JS 空白集，无 astral 字符故 units==chars，注释在位）；空行/`#` 跳过；meta 仅在 `indent==0 && section==none`。
- **meta 五选一（Q15）**：TS `/^(script|player|goal|start|seed):\s*(.+)$/` + if 链 script/player/goal/seed、else→`parseClock`；Rust 同一交替序 + `match` 四臂、`_ =>` 落 `parse_clock`，注释指认准确（"only `start`, the alternation's last word"，比计划原文"未知 meta 键"的措辞更精确——未知键实际走 "expected a section header"，行为两侧一致）。seed 门 `/^-?\d+$/` → `^-?[0-9]+$` + `js_num_parse`（f64 中转，§3.5），错误就地逐字（messages.md 行 1）。
- **节头/动词块缩进**：节头正则与 `t[..len-1]` 切片；verb header `indent==2 && /^[A-Z][A-Z0-9_]*:$/`、`recipes:` `indent==4`、recipe 体 `indent>=6`、slot 行 `indent>=4` 走 `/^([a-z]+):\s*(.+)$/`——分支序与 TS 160–178 逐条一致；重复 verb header 静默重置（Q5 注释）；`verbs[verbName]!` → `get_mut` + unreachable。
- **entities 臂**：先 push 后查重（`entities.len() > declared`），大小写敏感查重 vs `s_<lower>` 铸造（Q5 注释在位）。
- **endings/templates 臂**：`endings[id]` truthy → `contains_key`（对象恒真 ≡ 键存在）；templates `/^([A-Z_]+) says "(.*)"$` 逐字，静默覆盖 Q5 注释。
- **parse_entity_line**：关系分支 `relationTypes.has` → `contains_key`；`stackable\b` 前缀剥离（`(?-u:\b)`）；at/with 循环的 `atIn.index > withM.index` → `grp0_start` 字节偏移比较（同一串内两偏移同序，注释在位），`(?:^|\s+)` 左端语义与 slice 前缀一致；aspect spec `/^\s*(…):(-?\d+)\s*$/` 逐字 + `js_num_parse`；两条错误（unknown kind 带/不带 expected 后缀、bad aspect spec 实体/原型两变体）逐字符一致。
- **parse_prototype_line**：with/deriving 双正则 + index 比较（与 TS 271 同构）；`duplicate 'with'/'deriving' clause`；derive `minus: dm[3] ? Number : 0` → `grp_opt(...).map(js_num_parse).unwrap_or(0)`（捕获组非空恒真，等价）。
- **parse_trigger_line**：expire 形先行；`(\s+once)?` → `grp_opt(...).is_some()`；hour `n*2`；23/47 上界两条消息逐字。
- **parse_function_line**：三分支括号形/无参形/裸形——`(\S.*?)` 惰性量词照抄（regex crate leftmost-first 同 JS 回溯偏好），`split(/\s+/)` → `split_js_ws`（trim 后输入等价，注释在位）。
- **parse_deck_line**：defaultcard/reset 单个 `\s` 的 `(?:^|\s)` 剥离循环、spec split/trim/filter(Boolean)、`the spec is empty`（parse 侧）逐字。
- **parse_ending_line / parse_slot_line / parse_recipe_line**：flavour Option；`max`：`cm[2]` 有值→Some、`endsWith('..')`→None(=Infinity)、否则 min（ENG-18 恒真合取 `cm[1] !== undefined` 弃用有注释）；Q4 role 强转注释在位；`bad count` 文案逐字。recipe：`indexOf('->')` 首现、`^default\b`、id token `[a-z0-9_.]+`、hidden/shown、hint 位置提取 + `i = i`/`void i`（ENG-18 双注释）、`when` 单 `\s`。
- **compile_script**：ENG-01 接线正确（entities 一次性以 `DeclaredEntity{name,kind,stackable}` 进 `compile_content`，编译后突变删除）；`{...script.content, seed: meta.seed}` → 结构体更新语法；container/zone 分支（container 先 resolve 再 `get_mut` 写 attrs，借用拆分等价 TS 顺序）；`newState(start ? absMin : 0)`；relationDef 在 unknown-entity 检查之后（与 TS 449–451 同序）；player 门 `pool[0]` 最老head → `pool.first()`。
- **lint_script（八族告警逐字）**：no-recipes、unreachable、shadowed、count-gate（dead 判定五算符 + `None`=Infinity 时 `k>∞`/`k>=∞` 恒假、`..∞` 文案）、unknown template token（known=ROLE_NAMES+roll，extra=params）、deck 非 stackable（prototype kind 先、entity line kind 后）、aspect 只读未写（**reads/writes 用 IndexSet，TS Set 插入序决定告警出现序——序可观察点类型正确**）、non-lexeme。`norm = replace(/\s+/g,' ').trim()` → split/filter/join(" ")（trim 后输入逐点等价）；`slotOf` 重复 role 保首位后值覆盖（IndexMap insert 同 JS Map.set）；**lint 镜像 recipe id 1 基**（`mint_recipe_id(..., 1)`，Q1 注释）；1 基行号贯穿全部 parse 错误（`ln` 即 TS 的 `ln+1`）。

**content.rs ↔ content.ts（592 行，逐段）**

- **compile_content 校验序**：fns → stackable → endings → decks → verbs → triggers → prototypes 克隆 → templates → ending/deck refs——与 TS 199–292 的节序逐段一致（错误先后序即此序）。`Object.entries` 五点（204/208/219/274/276）全部走 JS 键序：前四点 `entries_in_js_order`、274 `clone_map_in_js_order`，REACH-04 达标（整数样键不拒绝、前置）。
- **endings**：UPPER_SNAKE / `!e.title || !e.title.trim()` / flavour `^[a-z]+$`（开放词表不收紧，ENG-13 注释）三条消息逐字。
- **compile_decks**：`entityKinds` Map 重复名后写值保位（IndexMap insert 同型，注释在位）；resolve prototype-first；四守卫序 UPPER_SNAKE→reserved→duplicate→empty spec 与 TS 411–414 一致；`permutePositions(entries.length, seed, d.id).map(p => entries[p])` 同型。
- **hashDeckId/permutePositions/drawFrom**：FNV-1a64 常量与 wrapping 乘逐位；`seed as u64` 二补码、`i as u64`、`splitmix64(key) % (i+1)`；W0 四向量在新家双重钉住（content.rs 测试 + decks.rs）。drawFrom 三规则与耗尽消息逐字；负 cursor 分支为已登记分歧（debug_assert + Err 守卫，TS 恒不可达）。
- **verbs 循环**：isVerb → slots（`s.max < s.min` 仅 Some 检查 ≡ TS Infinity 永不小于；shown `?? true`；query truthy 门——初审 must-fix-1，897511c 已修）→ recipes（`??`→`unwrap_or`；`r.when ?` truthy——同上，`raw_when` 过滤同喂 parse 与 id 铸造）→ lint 阶梯（lintPred when + forEachIfCondition + lintDice）。Q1 注释在 compile 0 基铸造点。
- **triggers**：PROGRESS/REFUSE walk → IF 条件双 lint（lintPred+lintTriggerPred）→ ROLL vexpr `lintTriggerValExpr(…, 'ROLL argument')` → lintDice——四段顺序与 TS 250–270 一致；`once ?? false`；expire/slot 两形展开同型。
- **build_fn_table 三遍**：pass1 七守卫序与消息逐字（op 冲突串的两行拼接合并后逐字节核对一致；重复参数"首个额外出现序去重" ≡ TS filter+Set 迭代序）；pass2 `function 'X': ${err.message}` 包装；pass3 `callsOf` 用 **IndexSet**（W1 must-fix-2 的延续，insertion 序 = 可观察输入）、undeclared 检查迭代 calls 序、WHITE/GRAY/BLACK DFS + `stack.indexOf`→`position`（GRAY 必在栈上，unreachable 有据）、depth memo + MAX_FN_DEPTH——与 content.ts:508–592 同构，真路径消息经 script_play.rs 复核逐字（见 (d)）。
- **lint_dice**：ROLLED/DRAWN 两条、PROGRESS/REFUSE 首个 str 参的 `{roll}` 门（`o.op` 插值同型）、ROLL vexpr `lintValExpr`、尾接 `lintRollMayBind`——逐条一致。

**发现的行为分叉**：初审 1 条（must-fix-1，空串 truthy），已由 897511c 消除（见 must-fix 复核节）；除此以外 (a) 无分叉。

## (b) 迭代序/所有权审计 — PASS

`grep -n "HashMap\|HashSet\|BTreeMap\|Rc<\|RefCell\|Cell<"` 于两文件：**零命中**。全部映射/集合为 IndexMap/IndexSet，或成员查询用途。逐处核对：

- `entries_in_js_order`/`ordinary_own_property_keys`：接在 content 侧全部 5 个 `Object.entries` 点 + script.ts:185 的 `Object.keys` 点（计划 §3.7 口径）；其余直接 IndexMap 迭代的点（lint 的 `Object.entries(verbs)` 两处、aspects/derive 遍历）键文法证明非整数样（`[A-Z][A-Z0-9_]*` / `[A-Za-z_][A-Za-z0-9_.]*`），脚本注释在位。
- **stackable 三集合合一**：TS 侧全部消费点 grep 证实仅 `has()`（script.ts:557、engine.ts:404、reach.ts:1526/1529），src/test/tools 无任何迭代——合一（含实体侧并入、构建序改为实体先）无观察面，ENG-01 注释与事实相符。
- `clone()` 45 处（script 20 + content 25）：逐一核对均为 String/Vec/小结构值克隆；`slot_by_role` 与 `slots` 各存一份 clone（TS 为同一对象别名）——grep 证实 TS 编译后无任何 CompiledContent 原位突变（engine/fold/reach/play 零命中，engine.ts:826/869 是 State.decks 游标），别名 vs 克隆不可观察。无 Entity 克隆入绑定结构（本波无 bindings 面）。
- 引擎纪律：无 `unwrap`/`expect`（grep 零命中）；`unreachable!` 6 处 + js.rs helper 3 处，全部带 TS 依据注释（规则 11）。

## (c) QUIRK 注释在位性 — PASS

| 条目 | 位置 | 判定 |
|---|---|---|
| Q1（recipe id 双基数） | content.rs:472–474（compile 0 基）、:924–936（mint_recipe_id 双变体文档）、script.rs:1270–1273（lint 1 基）+ 测试 `recipe_id_mint_keeps_both_frozen_bases` | ✓ 指认准确 |
| Q4（slot role typo 静默） | content.rs:34–37（SlotRole=String）、:240（slot_by_role 按 raw role）、script.rs:1067–1069（role 强转） | ✓ |
| Q5（大小写碰撞/重复声明不一致） | script.rs:517–519（实体查重 case-sensitive vs id 小写）、:535–537（prototype 静默覆盖）、:565–567（templates）、:582–584（verb header 重置） | ✓ |
| Q15（meta else 归因） | script.rs:486–490 | ✓（措辞比计划原文更精确，见 (a)） |
| Q8（JS `\s` 集） | script.rs 模块头 whitespace discipline + 全部 24 个正则经 `JS_WS_CLASS`/`JS_NON_WS_CLASS`；无一处裸 `\s`/`\S` 漏网（grep 证实） | ✓ |
| ENG-18 死代码/恒真 | `cm[1] !== undefined` 弃用（script.rs:1058–1060）、`i=i`/`void i`（:1113–1119） | ✓ |
| `.` 单行冻结论证 | script.rs:8–11 模块头（计划 §3.8 指定位置） | ✓（论证宽度见 note-1） |

## (d) messages.md 对账 — PASS（附 1 条 carried should-fix）

- **toThrow 计数**：grep 实测 lint.test.ts 9（模板未注册 1 + player 2 + reserved 4 + 重复实体 1 + 双声明 1）+ script-play 编译器子集 7（坏 expiry 3 + 坏脚本 4）= **16**，与 W2 对账节一致；升 pinned 的 11 行（77/79/80/82/88/89/97/108/109/113/120）与断言落点一一核实。
- **抽查 6 条 pinned 行 vs TS 原文逐字**：行 77 `line ${ln+1}: expected a section header, got '${t}'`（script.ts:129）、行 80 `'{name}' is declared both as an entity and a prototype — spawns of it could never mint`（:186）、行 88 `line ${ln}: ${what} '${name}' is a reserved word — rename it (§5.1)`（:200）、行 97 `triggers expect  ID [once] …  or  ID on expire of …`（:312，双空格两处）、行 109 `player '${meta.player}' must be a character (got ${p.kind})`（:463）、行 120 `deck ${deck}: ${what} '${name}' is neither a declared prototype nor an entity`（content.ts:407）——全部逐字符一致（含双空格/破折号/引号）。
- **行 74–76 真路径复核证据成立**：`function_guards_fire_through_the_real_compile_path_with_the_frozen_texts`（script_play.rs:604–621）经 `compile_script`→`build_fn_table` 触发，`functions recurse: a -> b -> a — …` 与 `function 'f1' sits 9 calls deep — the nesting cap is 8` 与 content.ts:559–562/:586 及 fn_expand_direct 先行冻结模板三方逐字相等。
- **carried should-fix**：W1 评审指出的 messages.md T1.5 对账节自相矛盾（:110 "其中 **10** 行升级 pinned" vs :107 的 **11** 行）**仍未修订**——一行 doc 修正，建议随 must-fix 同 commit 处理。

## (e) gate + 亲测 — PASS（897511c 复审后复核）

- `cargo test -j 4`（reviewer 两轮亲跑）：初审 **162 passed / 0 failed**；897511c 后 **163 passed / 0 failed**（59 lib + 12 clock + 4 decks + 4 dice + 9 fn_expand_direct + 2 gate + **14 lint** + 25 query_predicate + **15 script_play** + 18 sentence，含 1 条新增回归测试），零 warning。与 W1 的 129 相比净增 34（lint 14 + script_play 15 + deck_rng 并入 + deck 向量 + 修复回归 1）。
- `cargo clippy -j 4 --all-targets`（touch 强制重跑，两轮）：**0 warning / 0 error**。
- rand 门（gate.rs 扫 core+engine）与 unwrap/expect 文本门仍绿；content 两文件手工复核仅 `unreachable!`/`debug_assert!`（均带依据行，规则 11）。

## (f) TS 侧零改动 — PASS

`git diff --stat rust-port-w1..HEAD -- src/ test/ scripts/` → **空**；`git status --short -- src/ test/ scripts/` → 0 项。W2 全部触面 = rust/{src/content,src/js.rs,src/lib.rs,src/core/mod.rs,src/core/deck_rng.rs(删),tests/*} + docs/rust-port/messages.md，无越界；deck_rng 向量在新家双重钉住（content.rs 内嵌测试 + decks.rs）。

## (g) 有意分歧逐条核对

| # | 分歧声明 | 核对结论 |
|---|---------|---------|
| ① | ENG-01 集合合一后 Set 插入序无观察面 | **成立**。TS 全部消费点仅 `has()`（script.ts:557 / engine.ts:404 / reach.ts:1526,1529），grep 全仓（src/test/tools）无对这些 Set 的任何迭代；合一不改变任何 `has()` 结果 |
| ② | Infinity→Option\<i64\> 比较点语义等价 | **成立**。TS 涉及 slot max 的全部比较点枚举：content.ts:223 `s.max < s.min`（Infinity 恒假 ⇒ 仅 Some 检查等价）；script.ts:512–516 lint dead 五算符（`=/>/>=` 对 Infinity 恒假 ⇒ None→false 等价）；script.ts:518 `=== Infinity` range（None→`..∞`）；engine.ts:93–94（W3：`provided.length > slot.max` Infinity 恒假 + range 文案——`is_some_and` 映射保方向）。reach.ts 的 Infinity（freezeAt/freezeDist）是 REACH-06/13 哨兵，非本面 |
| ③ | meta else 归因论证与原文交替序一致 | **成立**。交替序 `(script|player|goal|start|seed)` 与 if 链的差集恰为 `start`；m[1] 捕获文本与交替序无关，Rust `match` 四臂 + `_` 与 TS if 链逐分支对应；Rust 注释对机制的表述（"only start, the alternation's last word"）准确且比计划原文的"未知 meta 键"更精确（未知键实际报 section header，两侧一致） |
| ④ | drawFrom 负 cursor 守卫 | **声明成立**。TS 首分支 `order[-1]`→undefined 恒不可观察（游标从 0 起且只在 DRAW 路径推进）；Rust debug_assert + `internal:` Err 守卫，规则 11 达规 |
| ⑤ | 其余登记项（§3.5 饱和、lossy 代理项、单行 `.` 冻结） | §3.5/lossy 为 W1 已登记、本波沿用（seed/count 字面量）；`.` 冻结为计划 §3.8 明文授权、注释在计划指定位置——见 note-1 |

## 抽查 4 个测试（逐断言比对）

1. **lint.test.ts reserved words（:36–43）** ↔ `reserved_words_cannot_be_names`：4 条 toThrow 正则逐字 + `not.toThrow`（X-Y）→ `is_ok()`；每测新建 Dictionary（TEST-10，计划 §5.2 授权口径）。
2. **lint.test.ts identical-when shadow（:84–96）** ↔ `an_identical_when_shadows_the_later_recipe`：脚本文本字节形状（前导换行/双空格 when）保留，`.some(includes)` 双片段断言同型。
3. **script-play.test.ts:175 `study.recipe0`** ↔ script_play.rs:351：0 基断言 + Q1 注释在位；同测试的 READ id 全表、`max: Infinity`→`None`、derive/hint/模板 12 张全表逐断言对应（toMatchObject 子集在 Rust 以等价精确断言覆盖并注明）。
4. **decks exhausted 消息** ↔ decks.rs 两条路径（手构 deck + 编译 deck）：整句含 `(§9.42)` 后缀逐字相等；TS 侧锚（decks.test.ts:410 refusal toMatch）为引擎路径，构造点文本本波先行钉住（messages.md 行 2 口径一致）。

## 问题分级（最终，含 897511c 复核）

**must-fix：0 条（初审 1 条，897511c 已消除并复核，见 must-fix 复核节）。**

初审 must-fix-1 记录（已消除）：`when`/`query` 空串 falsy 语义丢失——TS `content.ts:224/229/230` 以 truthy 门消费，`''` → 无 query / default recipe（`.default`）/ 不 parse；Rust `Option` match 曾把 `Some("")` 送进 parse 报 load error。触达面仅手构 `ContentRaw`（parse_script 文法产生不了空串，初审时现有测试零覆盖），但 TS `RecipeDef` 文档（content.ts:25）与 Rust 字段文档（content.rs:56）均承诺该语义。

**should-fix：2 条（均不阻塞收口）**

1. messages.md:110 "其中 10 行升级 pinned" 与 :107 "11 行" 自相矛盾（W1 should-fix-1 未随 W2 处理）；一行 doc 修订。
2. `draw_from` 对手构零长度 `order` + `reset` 会 `cursor % 0` panic（TS 返回 undefined，均不可观察——compile_decks 拒空 spec、全部构造点非空，函数文档已声明前置条件）；加 `debug_assert!(!order.is_empty())` 更稳。

**note：4 条**

1. script.rs 模块头的 `.` 冻结论证（"single-line ⇒ 分歧不可达"）比事实略宽：行**中部**的 `\r`/U+2028/U+2029 会让 TS `(.+)` 整体失配而 Rust 匹配（split('\n') 不切这些字符，trim 只去边缘）。CRLF 文件与现有内容不可达，仅对抗性输入可及；建议把注释收窄为"边缘空白不可达 + 禁止用于多行/含行内分隔符输入"。
2. 附录 A.2 未列 script.ts:155/160/403/614 四个正则（templates/verb-header/id-lexeme/name-lexeme）——本波已全部正确转写（`^[A-Z_]+ says "(.*)"$`、`^[A-Z][A-Z0-9_]*:$`、`^[a-z0-9_.]+$`、`^[A-Za-z_][A-Za-z0-9_]*$` 逐一核对），仅清单完整性缺口。
3. W1 note-3（fn_expand.rs `arg_spelling` delta 分支未挂 Q16 tag）仍未处理——非 W2 触面，顺带记录。
4. script.rs:1273 lint 镜像读未过滤的 `r.when.as_deref()`：两侧均无可达 `''` 输入（lint 只收文本、parse 产生不了空串），非行为分歧；防未来写入面变化建议补同型 `filter` 或一行注释。

**结论：PASS-with-notes——可打 `rust-port-w2` tag。** should-fix 1 建议随 W3 顺手处理，其余不阻塞。
