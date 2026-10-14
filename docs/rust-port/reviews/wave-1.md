# W1 波次门评审记录（§7 规则 13，PR3-04）

- 评审对象：`rust-port-w0..9dfb479`（22 commit：T1.1–T1.4 core 全模块移植 + T1.5 jsjson/测试基建/3 测试文件/fn_expand 直测子集）；**复审追加 `f589599` + `94e5b3c`（两条 must-fix 的修复）**
- 评审人：wave integrator 指派的独立 reviewer（非本波作者）
- 评审日期：2026-10-14（初审）/ 2026-10-14（复审）
- 证据方式：(a) 项按规则做 **core 全量逐段对照**（rust/src/core/ 全部 13 文件 vs src/core/ 全部 11 个 TS 原文，逐函数核对算法/顺序/错误文本/边界，含 parse.ts 853 行与 predicate.ts 678 行逐段）；(b)–(g) 逐项 grep/对账；`cargo test -j 4` 亲自重跑（串行、rust/ 下、无 vitest/tsx 并发）；关键发现均在 crate 外以独立 rustc/cargo 复现（未改动仓库任何文件）

## 总评：**PASS-with-notes（最终结论，含 f589599/94e5b3c 复审）**

初审 127/127 全绿，但发现 2 条 must-fix（判 FAIL）。修复 commits 复审通过（证据见下节）：**129/129 测试全绿**（56 lib + 12 clock + 18 sentence + 25 query_predicate + 9 fn_expand_direct + 4 dice + 3 decks + 2 gate，新增 2 条回归测试），`cargo clippy -j 4 --all-targets` 零警告，TS 侧零改动（`git diff --stat 9dfb479..HEAD -- src/ test/ scripts/` 为空）。两条 must-fix 均已完全消除、无新引入分歧。遗留 should-fix 2 条 + note 4 条（均不阻塞）。可打 `rust-port-w1` tag。

---

## must-fix 复核（2026-10-14，commits f589599 / 94e5b3c）

**must-fix-1（f589599，lexer.rs `scan_name` 截断分叉）— 已消除。**
- 修复实现：起点判定 `matches!(c, 0x41..=0x5A | 0x61..=0x7A | 0x5F)`、续点判定加 `0x30..=0x39`——对 u16 码元直接范围匹配，类集与 TS `NAME_RE=/^[A-Za-z_][A-Za-z0-9_]*/` 逐字符相同；截断路径不复存在（grep 证实 `as u8 as char` 仅存于注释）。
- 对照 TS 原文复核：ASCII 域行为逐位不变（`is_ascii_alphabetic`/`is_ascii_alphanumeric` 在 ASCII 上与显式范围等值），仅删除了 >0x7F 的误收路径——无回归面。
- 新增回归测试 `non_ascii_code_units_never_enter_name_tokens` 五断言逐一对照 TS：`lex("abš")` → `unexpected character 'š' at offset 2`（U+0161 低字节 0x61）、`lex("Łat")`（U+0141 起点，offset 0）、`lex("aŁb")`（续字符，offset 1）、`lex("x9а")`（U+0430 低字节 0x30 续字符，offset 2）、`lex("a #š")` → `bad id ref at offset 2`——消息模板与 offset 均与 TS 逐字符一致。
- reviewer 独立复现（crate 外临时工程）：三个输入全部 Err 且文本/offset 与 TS 一致；修复前同工程曾复现 Ok(Name) 分叉。闭环。

**must-fix-2（94e5b3c，`collect_fn_calls` HashSet 迭代序）— 已消除。**
- 修复实现：签名 `&mut HashSet<String>` → `&mut IndexSet<String>`，`IndexSet::iter` 即 TS `Set` 的插入序；重复 call 保首位置（`Set.add` 同型，新测试显式钉住）。`fn_expand.rs` 的 HashSet import 移除。
- 对照 TS 原文复核：content.ts:545-561（pass-3）消费序的两个可观察点——`:548` 第一个未声明调用名进错误文本、`:552-561` DFS 决定先发现的环及 `a -> b -> a` 拼写——现均可确定性复现。
- 镜像同步：fn_expand_direct.rs 的 `content_pass3_guards`（W2 蓝本）同步改 IndexSet（calls_of 值与两处迭代），note-4 一并消除；color/memo 的 HashMap 仅键值查询（TS Map 同），序不敏感，保留正确。
- 新增回归测试 `collect_fn_calls_preserves_ts_set_insertion_order`：跨 list 声明序 + IF 臂先于 fallback + 重复名保首位置，断言 `["zulu", "alpha", "mike"]`——与 TS `collectFnCalls` 的填充序逐点对应（臂体先于 fallback，parse.ts:77-82 顺序核实）。
- reviewer 独立复现：4 名单含重复的填充序输出 `["zulu", "alpha", "mike"]`，与 TS Set 语义一致。闭环。

**新引入分歧排查**：两 commit 仅触 lexer.rs（scan_name 判定 + 测试）、fn_expand.rs（签名 + 测试）、fn_expand_direct.rs（镜像类型）；`scan_name` 的另两个调用点（`#` idref 分支、名字分支）经同一修复自动对齐 TS（`#š` 用例钉住）；`params: HashSet`（parse.rs/predicate.rs）成员查询用途不变，不必改；全仓 `cargo clippy --all-targets` 零警告；gate 2 项仍绿。

---

## (a) 逐段对照（core 全量）— PASS（初审 1 条行为分叉，已由 f589599 消除）

**clock.rs ↔ clock.ts**：`absMin` 运算序一致（saturating 为 §3.5 文档化分歧，注释在位）；`clock_from_abs_min`/`next_step_clock` 的 div_euclid/rem_euclid 在非负域 ≡ Math.floor 与 JS `%`，并按 wave-0 should-fix-1 先例补了 `debug_assert!(m >= 0)`（clock.rs:40/54）；`parseClock`/`parseDuration`/`formatDuration` 分支序与三条消息逐字符一致；47 → `HALFHOURS_PER_DAY - 1`（§3.18 等值改写）；DUR_RE 交替序 `halfhour|h|m|d` 保序（Q9 注释）；`crossedSlots` 死代码未移植（正确）。

**types.rs ↔ types.ts**：`AspectVal` 单层点模型（Q13 注释 ×3 处）；`add_entity` 的 pool.every/`?.stackable === true` 语义逐条对应（缺失实体按非 stackable 读）；`aspect_entry/set` 首点分割 + 双向静默覆盖；`relation_key` 走 `cmp_utf16`（§3.6）；`getEntity`/`attrGet` 死代码未移植（正确）；`Entity::clone`/`State::clone` 手写深拷贝并过 JS 键序（PR2-05 的 types.ts:118/:132/:138 克隆重排点全覆盖）。两处 note 见问题分级（子映射 spread 重排、windows.refs 共享 vs 克隆——均今日不可观察）。

**roles.rs / sentence.rs ↔ sentence.ts**：Role 六名与声明序（ROLE_LIST/`asRole` 承重）逐字一致；Arg 13 变体与 tag 串逐一对应；`Sign` 的 apply/as_i64 对 parse.ts:553/725-726；`empty_clause` 按 §1.2 豁免保留。

**ref.rs ↔ ref.ts**：SAFE_NAME 同型；`format_ref` 只转义 `"`（ref.rs:76 注释 + 测试钉死反斜杠裸传）；`ref_sort_key` param 落 name 分支；`sort_refs` 稳定排序 + `cmp_utf16`（非 BMP 陷阱有测试）；`sameRef` 死代码未移植且注明"等值比较不涉 cmp_utf16"（CORE-05 修正口径）。

**dict.rs ↔ dict.ts**：29 个 RESERVED_WORDS 与 `...CORE_VERBS` 尾部逐字一致（测试钉尾）；OUTCOME_OPS 17 项声明序（unknownCallError 承重）；`register_relation` 无校验的不对称冻结（CORE-15(1) 注释）；`assert_nameable` 两条消息逐字符一致。

**query.rs ↔ query.ts**：`parse_query` 的 trim/split/filter 合法收敛（split_js_ws 同 JS 空白集）；aspect/state 两正则的 `\s` 全部走 `JS_WS_CLASS`（附录 A.1 对应）；比较符交替序 `>=|<=|>|<|!=|=` 保序；`coerce` 三分支；`match_term` 的 `v !== true`（Bool(true) 精确匹配）、数值/泛型 `===` 两层、`Option` 跨变体 false——全部对应；`aspects_from_pairs` 的 Q13 双向覆盖。**numCmp 收敛进 `CmpOp::num_cmp` 有据**（CORE-13(1) 验证报告明文"(1)-(5) 收敛为单函数零风险"）。

**predicate.rs ↔ predicate.ts（678 行逐段）**：双前端结构保留（CORE-21 拼串 + span，注释明令禁止重构）；`pred_view_from_toks` 词邻插空格 + 三条 literal 抛错；词法器 `/g` exec 跳过语义（**Q2 注释在位**，find_iter 同型）；IF 扫描四条错误文本逐字符一致；exists/count 平衡括号 + `inner()` span 截取 + 比较符压空格 `$1` 展开；`empty predicate` 抛点推迟到 `parse()`——与 TS 的构造期抛出调用方可见等价（见 (g)③）；求值面：and/or 短路（b 可能 throw 的语义保住）、rel 的 `.container` 端点替换 + 自对跳过、every/any 空集、exists 的 byName + 活检、cmp 双操作数先求值后 undefined→false、strict 仅零绑定抛（:499-505 死绑静默）、`roll` 前端门 + unbound 抛、lint 四家族与 `lintRollMayBind` 的 may-bind 传递——逐条一致。

**lexer.rs ↔ lexer.ts（五件手写 scanner）**：PUNCT 13 字符逐一核对；`scan_dur` 的 first-greedy-digits + `halfhour` 先试 + 贪婪尾组 ≡ DUR_RE leftmost-first；Q10 字面转义 + 尾反斜杠 `"undefined"` 后照常 unterminated（逐分支对上）；错误 offset 码元域与 JS 一致；lossy U+FFFD 为 §3.9 已登记分歧。初审发现的 `scan_name` 截断分叉已修复（见 must-fix 复核节），五件 scanner 现全部逐字符达标。

**parse.rs ↔ parse.ts:1-640（逐段）**：clock/EVENT/COMPLETE/WAIT 头部分派序与全部守卫；clause 的 actor/AT/verb/TO/objects/WITH/ABOUT/post-AT 序；`starts_ref` 排除集；`ref()` 的 param→reserved→kind 序（含 kind: 后 param 拒绝）；outcome 分派：IF→fns.byName→isOp→op 分派→SPAWN 兜底的优先级逐条对上；REPLACE/ENDING/ROLL/DRAW 自限定形错误；ROLLED/DRAWN/NARRATE 形状钉板（DRAWN 先 str→ref 强转再查形）；**Q3 双测钉死**（`SET(myparam = 3)` → `expected ')', got '='`；`SET(x, k = p)` 的 value-param 不消费 → `expected ')', got 'p'`，spawnArgs 的 param 键活分支对照）；rollArgExpr 深度扫描；arity 消息模板逐字符一致。

**fn_expand.rs ↔ parse.ts:25-84, 646-853（逐段）**：`FnTable.by_name` IndexMap 声明序（CORE-06）；`expand_list` 的 FN/IF/普通三分支与 bind2 逐参 substArg；subst_arg 的 sign 折叠（int/delta→delta，其余 splice-through）、vexpr、整 ref-param、into 三分支（错误文本 `the call supplies a ${t}` 的 tag 串一致）；`subst_string` 单趟不重扫（手工组装 ≡ JS /g replace 语义）+ 捕获组 1/3 判空 + 保留 token 集合序；`undeclared_token_error`/`arg_spelling`（str/pair 裸拼写，CORE-13 标红注释在位）/`subst_expr` 三分支/`unknown_call_error` 全文（ops 序 + `; declared functions:` / `; no functions declared` + IF 提示）——全部逐字符一致。

**format.rs ↔ format.ts**：四种 kind 行、clause 序、SPAWN/NARRATE/REPLACE/ENDING 特形与 `?` 兜底、format_arg 13 分支（**Q16 的 `-0`→`+0` 符号分支显式保留**，注释在位）、quote 先 `\\` 后 `"`（appendix A.1 S 项）、formatTriggerId 逐段 bare/quoted——一致。

## (b) 迭代序/所有权审计 — PASS（初审 1 条，已由 94e5b3c 消除）

`grep -rn "HashMap\|HashSet\|BTreeMap\|Rc<\|RefCell\|Cell<"`（rust/src，实现面）：

- `State.by_name: HashMap<String, Vec<String>>`（types.rs:154）：§3.7 豁免项，**注释在位**（types.rs:5-9 模块头 + :150-153 字段注释，"外层无未排序迭代点、池数组即序"）。
- `params: HashSet<String>`（parse.rs:72 / predicate.rs:147/204/271）：仅 `contains` 成员查询（TS `Set.has` 同），序不可观察 — 合格。
- ~~`collect_fn_calls` 的 `HashSet`~~ → **已改 `IndexSet`**（94e5b3c，见 must-fix 复核节）：TS `Set` 插入序是 content.ts pass-3 的可观察输入，现确定性保持。
- `clone()` 65 处（含测试）：逐一核对均为 String/Vec/Arg/Pred/Tok 值克隆；**无 Entity 克隆进 bindings**（role_cards/eval 经 id 读回 `entities.get`，predicate.rs:671-684 + 测试文件注释，§2.6.2 达标）；无 Rc/Arc/RefCell/Cell；无 BTreeMap。

## (c) QUIRK 注释在位性 — PASS

W1 范围适用条目逐条核到（指认准确、无错挂）：

| 条目 | 位置 | 判定 |
|---|---|---|
| Q2（tokenizer 吞字符） | predicate.rs:288-292（构造点）+ :1151-1161（测试钉 `@` 消失） | ✓ |
| Q3（arg/spawn param 键不对称） | parse.rs:1020-1027（name_arg）+ :873-876/:905-907（spawnArgs 活分支对照）+ 双测试 | ✓ |
| Q8（JS `\s` 集） | js.rs:24-29（单点常量）+ js_is_ws；query/predicate/lexer/subst_string 全部走它 | ✓ |
| Q9（DUR_RE 交替序） | lexer.rs:94-97 + clock.rs:105-107 双处 | ✓ |
| Q10（字面转义/尾反斜杠） | lexer.rs:158-171，含 `undefined` 拼接后照常抛 | ✓ |
| Q12（ROLES 软保留/拦截序） | parse.rs:25-30（as_role）+ :851-852（role_or_ref_arg） | ✓ |
| Q13（aspect 单层点） | types.rs:29-32/:327-328/:353-356 + query.rs:331-335 | ✓ |
| Q14（`\|` 键 + sameRef 抛错） | types.rs:156-158 + :385-386；sameRef 死代码未移植注明 | ✓ |
| Q16（-0→+0） | format.rs:180-185（format_arg 符号分支） | ✓（fn_expand arg_spelling 同型分支未挂 tag，note-3） |
| CORE-13 双拼写冻结 | fn_expand.rs:318-321 + format.rs:172-174 双侧注释；未合并，只共享 delta/int/dur/bool | ✓ |
| 附加：死代码清单 | crossedSlots/attrGet/getEntity/sameRef 未移植且各有一行 NOTE；emptyClause 保留 | ✓ |

## (d) messages.md 对账 — PASS（附 note-1）

- **toThrow 计数**：`grep -o toThrow` 实测 sentence 9 + clock 9 + query-predicate 6 = **24**，与 T1.5 对账口径一致；pinned 升级 11 行（6/7/8、52/54/64/65、17/29/30/31）与 24 条断言的"多断言共钉一行"关系成立。
- **抽查 5 条 pinned 行 vs TS 原文逐字**：行 6 `bad clock '${s}' (expected "day:halfhour")`（clock.ts:33）、行 8 `bad duration …(expected e.g. 1halfhour, 2h, 90m, 1d, 1h30m)`（clock.ts:54）、行 52 `expected a registered verb, got '${show(verbTok)}'`（parse.ts:199）、行 64 `deltas are integers only (no duration deltas)`（parse.ts:555）、行 17 `bad name term …(name:X is an exact-case match — put the comparison outside: count(name:X) >= 1)`（query.ts:38）——全部逐字符一致（含引号/破折号/空格）。行 74/75/76 三条 pre-frozen 模板对照 content.ts:559-562/:586/:548 亦逐字符一致（见 (g)④）。
- **note-1（= should-fix-1，仍开放）**：messages.md:110 仍写"其中 **10** 行升级 pinned"，与 :107 已修正的 11 行自相矛盾（commit 9dfb479 只改了 :107 一处）。一行 doc 修订，W2 顺手处理即可。

## (e) grep 门禁 + 亲测 — PASS

- `cargo test -j 4`（reviewer 复审时亲自重跑）：**129 passed / 0 failed**（56 lib + 12 clock + 3 decks + 4 dice + 9 fn_expand_direct + 2 gate + 25 query_predicate + 18 sentence，含 2 条新增回归测试），零 warning。
- `cargo clippy -j 4 --all-targets`：**0 warning / 0 error**。
- `rand::`：src/ 零命中（gate.rs 扫 core+engine）。
- unwrap/expect 门（gate.rs:207-287 文本扫描）：注释/字符串/原始字符串清甲 + `#[cfg(test)]` mod 整体剪除 + **`self.expect(` 排除**——核对合理：唯一被排除的是 `PredParser::expect`（predicate.rs:330，返回 Result 的文法方法，不 panic），且要求接收者恰为 `self`（`x.expect(`/`self.f.expect(` 照样触雷）；限制三条已在注释自我声明。实现面手工复核：仅 `unreachable!`/`debug_assert!`（js.rs:124、fn_expand.rs:98/114/280、clock.rs:119、query.rs:67、parse.rs:701、dict.rs:196、lexer.rs:204，全部带 TS 依据行，规则 11 达规）。

## (f) TS 侧零改动 — PASS

`git diff --stat rust-port-w0..HEAD -- src/ test/ scripts/` → **空**（含两个修复 commit 后复核）；`git status --short -- src/ test/ scripts/` → 0 项；未跟踪文件全部在 docs/rust-port/（计划与报告文档，允许）。两修复 commit 触碰面 = rust/src/core/lexer.rs、rust/src/core/fn_expand.rs、rust/tests/fn_expand_direct.rs（+ 本评审文档），无越界。

## (g) 有意分歧逐条核对

| # | 分歧声明 | 核对结论 |
|---|---------|---------|
| 1 | §3.5 饱和语义（abs_min/parse_duration/total/js_num_parse 溢出饱和 vs TS f64 有损） | **无测试钉住，声明成立**。grep 全 test/：>2^53 字面量仅 dice.test.ts:86-88 的 splitmix64 BigInt 向量与 :101/:110 的 2^53−1 seed（W0 u64 域，已处理）；SET/duration/query/predicate 数值通路无任何 >i64 断言。各饱和点均有 `§3.5` 注释，无回绕、无 panic |
| 2 | predicate span 字节偏移 vs UTF-16 偏移等价 | **声明成立且理由充分**。Tok 前端全部 token 文本 ASCII（NAME_RE/NUM/PUNCT 皆 ASCII 类；Str 进谓词即抛），故字节 offset ≡ 码元 offset；即便放宽到多字节，两侧各自以自身表示累计 span、边界都是整 token 边缘，`inner()` 截取内容仍相同（predicate.rs:126-128 注释成立）。抽查含多字节字符的谓词走查通过 |
| 3 | empty-token 抛出点推迟（构造期→parse 期） | **调用方可见边界一致**。字符串前端唯一入口 `parse_predicate` 构造+parse 相邻，抛出时机不可区分；Tok 前端空 slice 由 `parse_predicate_toks` 的 `slice.is_empty()` 先报 `an IF needs a condition before '->'`（TS 同序），`parse_value_expr_toks` 空输入两侧同为 `unexpected end of predicate`（Rust 的空集合检查只在 `parse()`，不在 `parse_value_expr`——位置放对了） |
| 4 | 深度 8/环消息 pass-3 镜像钉板 | **逐字一致**。`content_pass3_guards`（fn_expand_direct.rs:350-422，94e5b3c 后为 IndexSet 版）对照 content.ts:543-588 同构（WHITE/GRAY/BLACK + stack + depth memo），产出消息与 content.ts:559-562（`functions recurse: ${cycle.join(' -> ')} — calls expand at load time, so a recursion cycle never terminates; rewrite without the cycle`）、:586（`function 'f1' sits 9 calls deep — the nesting cap is 8`）、:548 逐字符相同，锚 functions.test.ts:291-301/:303-310 原文核实。镜像迭代序随 must-fix-2 修复后与 TS 确定性一致 |
| 5 | 其余登记项（lossy 代理项、负分钟 debug_assert、NaN→0、numCmp 收敛、fresh-dict-per-test、verbs 排序走 cmp_utf16） | lossy=plan §3.9 授权；debug_assert 与 wave-0 should-fix-1 同型达标；NaN 仅不可达输入；numCmp 收敛=验证 01 CORE-13 修正表述明文授权"(1)-(5) 收敛零风险"；TEST-10/断言基建符合 §5.2 |

## 问题分级（最终）

**must-fix：0 条（初审 2 条，f589599/94e5b3c 已消除并复核）。**

**should-fix：2 条（均不阻塞收口）**

1. messages.md:110 "其中 10 行升级 pinned" 与 :107 的 11 行矛盾（9dfb479 只改了一处）；一行修订。
2. js.rs `clone_map_in_js_order` 用于 `Aspects` 时对 `AspectVal::Sub` 的内层 IndexMap 只做 `v.clone()`（插入序），而 TS `cloneAspects` 的子映射 `{ ...v }` spread 会把整数样键前置（types.rs:231-236 注释只覆盖了顶层）。今日不可达（aspect 子键全部来自 NAME/标识符文法，永远不可能是整数样键），但 W3 引擎写入接线前应递归处理或把"子键不可能是整数样键"的论证写成注释，防止未来写入面变化时静默漂移。

**note：3 条**

1. messages.md 行 74/76 的 TS 锚口径（"无 toThrow 直锚/全量在 W3"）与实际（functions.test.ts:291-310 有直锚）在 T1.5 对账节已自我说明，W3 移植时把锚号钉准即可。
2. types.rs `State::clone` 对 `windows[].refs` 做深克隆，TS `cloneState` 的 `{ ...w }` 只浅共享 refs 对象。TS 从不在注册后原位改写 refs（完成/回滚都读），不可观察；W3 若出现原位 mutation 需回头对表。
3. fn_expand.rs `arg_spelling` 的 delta 分支与 format_arg 同型（`v < 0 ? v : +v`）但未挂 Q16 tag——分支本身正确，补一行注释更稳。

（初审 note-4"测试镜像自身用 HashSet"已随 94e5b3c 消除，注销。）

**结论：PASS-with-notes——可打 `rust-port-w1` tag。** should-fix 1/2 由 W2 认领者（或本波作者顺手）处理，不阻塞。
