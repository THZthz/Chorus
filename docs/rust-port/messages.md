# 错误消息对照表（messages.md）

plan §2.5 / TEST-14 / 验证 04 must#4：错误消息文本是契约。三列对照：消息模板 ↔ 首个钉住它的测试 ↔ TS 源行；Rust 构造点随波登记。模板必须逐字符相同（含空格与标点）；TS `${…}` 插值在表中以 `${…}` 原样记。随波增补，W6 收口覆盖全仓 192 条 `toThrow`（验证 01 CORE-16/TEST-05 勘误口径）。

状态图例：**built** = Rust 构造点已存在；**pinned** = 模板已在 Rust 测试侧逐字冻结（构造点在后续波）；**registered** = 仅登记文本与出处（W2/W3 落地时升为 pinned/built）。

| # | 状态 | 消息模板 | 首个钉住的测试 | TS 源行 | Rust 构造点 |
|---|------|---------|---------------|---------|------------|
| 1 | pinned | ``line ${ln + 1}: seed must be an integer, got '${m[2].trim()}'`` | test/dice.test.ts:181（`/seed must be an integer/`） | src/content/script.ts:113 | （W2 content/script.rs；模板冻结于 rust/tests/dice.rs `script_seed_error`） |
| 2 | built | ``deck '${deck.id}' is exhausted — no 'reset' and no 'defaultcard' (§9.42)`` | test/decks.test.ts:410（refusal `/deck 'SINGLES' is exhausted — no 'reset' and no 'defaultcard'/`；原文含 `(§9.42)` 后缀） | src/content/content.ts:476 | rust/src/core/deck_rng.rs `draw_from` |
| 3 | registered | ``ROLL: dice count must be an integer >= 0, got ${count ?? 'nothing'}`` | test/dice.test.ts:271/346（refusal `/ROLL: dice count must be an integer >= 0/`） | src/engine/engine.ts:764 | （W3 engine/engine.rs） |
| 4 | registered | ``ROLL: sides must be an integer >= 2, got ${sides ?? 'nothing'}`` | test/dice.test.ts:315/328（refusal `/sides must be an integer >= 2/`） | src/engine/engine.ts:767 | （W3 engine/engine.rs） |
| 5 | registered | `'roll' is unbound — no ROLL has run in this outcome list before this point` | test/dice.test.ts:423/433（refusal `/'roll' is unbound/`） | src/core/predicate.ts:477 | （W3 core/predicate.rs） |

备注：

- 行 1 的 `line` 前缀是 1 基行号（`ln + 1`，`ln` 为 `text.split('\n')` 的 0 基下标）；dice.test.ts 的 rig 里 `seed:` 行在 0 基下标 4，消息即 `line 5: …`（rust/tests/dice.rs 已按此冻结）。
- 行 3/4/5 为 W0 预登记（文本自 engine.ts:764/767、predicate.ts:477 原文抄录），W3 移植构造点时逐字核对并升为 built。
- 表行增量须与各波移植的 `toThrow` 数对账（§7 规则 13(d)）；W0 波移植的 dice.test.ts:181 一条对应行 1（decks.test.ts:410 是 refusal/toMatch 断言，非 toThrow，行 2 因此单独登记）。

## W1 增补（T1.1–T1.4，core 构造点全量落地；行 6–55）

W1 将 src/core 全部错误构造点移植为 `ChorusError::new`（状态 **built** = rust/src/core/ 下构造点已存在并有 `// ts:` 锚）。首钉测试列给出已核实的 TS 锚；标「T1.5/W3 对账」的行待测试移植时把锚号钉准（文本均已按 TS 原文逐字符抄录，但 W1 未移植测试，不构成 pinned）。

| # | 状态 | 消息模板 | 首个钉住的测试 | TS 源行 | Rust 构造点 |
|---|------|---------|---------------|---------|------------|
| 6 | pinned | ``bad clock '${s}' (expected "day:halfhour")`` | test/clock.test.ts:26-32（`/bad clock/`）→ rust/tests/clock.rs `day_cannot_be_negative_d_cannot_lex_a_sign`、`rejects_halfhour_out_of_range_and_malformed_input` | src/core/clock.ts:33 | rust/src/core/clock.rs `parse_clock` |
| 7 | pinned | ``bad halfhour '${s}' (0..47)`` | test/clock.test.ts:29（`/bad halfhour/`）→ rust/tests/clock.rs `rejects_halfhour_out_of_range_and_malformed_input` | src/core/clock.ts:35 | rust/src/core/clock.rs `parse_clock` |
| 8 | pinned | ``bad duration '${s}' (expected e.g. 1halfhour, 2h, 90m, 1d, 1h30m)`` | test/clock.test.ts:50-53（`/bad duration/`）→ rust/tests/clock.rs `rejects_unknown_units_bare_ints_and_stray_suffixes` | src/core/clock.ts:54 | rust/src/core/clock.rs `parse_duration` |
| 9 | built | ``negative duration ${minutes}`` | T1.5/W3 对账（无已知直锚） | src/core/clock.ts:63 | rust/src/core/clock.rs `format_duration` |
| 10 | built | ``duplicate name '${e.name}' among alive entities`` | test/stackable.test.ts:240-248（`/duplicate name 'X'/`） | src/core/types.ts:153 | rust/src/core/types.rs `add_entity` |
| 11 | built | ``unknown relation type '${type}' (register it in the dictionary)`` | T1.5/W3 对账 | src/core/dict.ts:66 | rust/src/core/dict.rs `relation_def` |
| 12 | built | ``${what} '${lexeme}' must be UPPER_SNAKE`` | 变体钉于 test/decks.test.ts:631（deck 版）；dict 版 T1.5 对账 | src/core/dict.ts:73 | rust/src/core/dict.rs `assert_nameable` |
| 13 | built | ``${what} '${lexeme}' collides with a verb`` | T1.5/W3 对账 | src/core/dict.ts:75 | rust/src/core/dict.rs `assert_nameable` |
| 14 | built | ``a function parameter is authored form only — calls expand at load (§9.40)`` | T1.5/W3 对账（formatRef(param) 抛点） | src/core/ref.ts:39 | rust/src/core/ref.rs `format_ref`、rust/src/core/format.rs `format_arg`（param 分支同文） |
| 15 | built | ``bad query term '${raw}' (expected kind:/aspect:/state:/name:)`` | T1.5/W3 对账 | src/core/query.ts:26/54 | rust/src/core/query.rs `parse_term` |
| 16 | built | ``bad name term '${raw}' (expected name:<NAME>)`` | T1.5/W3 对账 | src/core/query.ts:33 | rust/src/core/query.rs `parse_term` |
| 17 | pinned | ``bad name term '${raw}' (name:X is an exact-case match — put the comparison outside: count(name:X) >= 1)`` | test/query-predicate.test.ts:231-233（`/bad name term/` ×3）→ rust/tests/query_predicate.rs `a_comparison_inside_a_name_term_is_a_load_error_it_would_silently_bake_the_name` | src/core/query.ts:38 | rust/src/core/query.rs `parse_term` |
| 18 | built | ``bad aspect term '${raw}'`` / ``bad state term '${raw}'`` | T1.5/W3 对账 | src/core/query.ts:44/49 | rust/src/core/query.rs `parse_term` |
| 19 | built | ``empty predicate '${src}'`` | T1.5/W3 对账 | src/core/predicate.ts:178 | rust/src/core/predicate.rs `PredParser::parse` |
| 20 | built | ``unbalanced ')' in IF condition`` | T1.5/W3 对账（conditionals 域） | src/core/predicate.ts:107 | rust/src/core/predicate.rs `parse_predicate_toks` |
| 21 | built | ``an IF condition needs '-> ( outcomes )'``（两处：逗号/分号与无箭头） | T1.5/W3 对账 | src/core/predicate.ts:110/116 | rust/src/core/predicate.rs `parse_predicate_toks` |
| 22 | built | ``an IF needs '-> ( outcomes )' before 'ELSE'`` | T1.5/W3 对账 | src/core/predicate.ts:113 | rust/src/core/predicate.rs `parse_predicate_toks` |
| 23 | built | ``an IF needs a condition before '->'`` | T1.5/W3 对账 | src/core/predicate.ts:118 | rust/src/core/predicate.rs `parse_predicate_toks` |
| 24 | built | ``an IF condition cannot contain a string literal`` / ``…a duration literal`` / ``…a #id literal`` | T1.5/W3 对账 | src/core/predicate.ts:151-153 | rust/src/core/predicate.rs `pred_view_from_toks` |
| 25 | built | ``expected comparison op, got '${opTok}'`` | T1.5/W3 对账 | src/core/predicate.ts:292/302 | rust/src/core/predicate.rs `scalar_cond`/`cmp_from_expr` |
| 26 | built | ``expected integer in scalar condition, got '${v}'`` | T1.5/W3 对账 | src/core/predicate.ts:294 | rust/src/core/predicate.rs `scalar_cond` |
| 27 | built | ``'roll' is not available in 'when:' predicates — it binds inside an outcome list, after a ROLL (§9.41)`` | test/dice.test.ts:473（`/'roll' is not available in 'when:' predicates/`） | src/core/predicate.ts:319 | rust/src/core/predicate.rs `val_expr` |
| 28 | built | ``'roll' is unbound — no ROLL has run in this outcome list before this point`` | test/dice.test.ts:423/433（行 5 升级：构造点已落地） | src/core/predicate.ts:477 | rust/src/core/predicate.rs `eval_expr` |
| 29 | pinned | ``exists() needs a name or a slot query`` / ``count() needs a role or a slot query`` | test/query-predicate.test.ts:174（`/needs a name or a slot query/`）→ rust/tests/query_predicate.rs `composability_and_parse_errors`（exists 变体；count 变体仍待 W3 对账） | src/core/predicate.ts:267/338 | rust/src/core/predicate.rs `exists_pred`/`val_expr` |
| 30 | pinned | ``exists() takes one name or a kind:/aspect:/state:/name: query, got '${inner.trim()}'`` | test/query-predicate.test.ts:175（`/takes one name/`）→ rust/tests/query_predicate.rs `composability_and_parse_errors` | src/core/predicate.ts:275 | rust/src/core/predicate.rs `exists_pred` |
| 31 | pinned | ``total() pools a bound role's aspects — total(<query>, …) is not offered; use count(<query>) to count a pool by name (e.g. count(name:X))`` | test/query-predicate.test.ts:236（`/total\(\) pools a bound role/`）→ rust/tests/query_predicate.rs `a_comparison_inside_a_name_term_is_a_load_error_it_would_silently_bake_the_name` | src/core/predicate.ts:351-354 | rust/src/core/predicate.rs `val_expr` |
| 32 | built | ``bad path '${t}'`` / ``expected aspect|attr in path '${t}'`` / ``expected aspect|attr after '${t}.', got '${field}'`` | T1.5/W3 对账 | src/core/predicate.ts:368/371/378 | rust/src/core/predicate.rs `val_expr` |
| 33 | built | ``${where}: count() argument '${x.role}' is neither a role (${ROLE_LIST}) nor a query (name:x, kind:k, aspect:a>=1, state:s, -term) — did you mean count(name:${x.role})?`` | test/conditionals.test.ts:432（count(DREAD) typo lint 域） | src/core/predicate.ts:545-548 | rust/src/core/predicate.rs `lint_val_expr` |
| 34 | built | ``${where}: total() first argument '${x.role}' is not a declared role (${ROLE_LIST}) — total pools a role's aspects; use count(name:x) to count a pool by name`` | T1.5/W3 对账 | src/core/predicate.ts:550-554 | rust/src/core/predicate.rs `lint_val_expr` |
| 35 | built | ``${where}: IF operand rel(${rtype}, ${role}) reads a step role — …``（全文见源） | test/conditionals.test.ts:441（trigger IF 域） | src/core/predicate.ts:579-582 | rust/src/core/predicate.rs `lint_trigger_pred` |
| 36 | built | ``${where}: IF operand ${t}(${role}, …) reads a step role — … it would always be ${t==='every'?'true':'false'}; …`` | test/conditionals.test.ts:441 域 | src/core/predicate.ts:585-589 | rust/src/core/predicate.rs `lint_trigger_pred` |
| 37 | built | ``${where}: ${what} count(${role}) reads a step role — … it would always be 0; use count(<query>) (e.g. count(name:${role}))`` | test/conditionals.test.ts:441 域 | src/core/predicate.ts:596-599 | rust/src/core/predicate.rs `lint_trigger_val_expr_what` |
| 38 | built | ``${where}: ${what} total(${role}, aspect:${path}) reads a step role — … it would always be 0`` | T1.5/W3 对账 | src/core/predicate.ts:600-604 | 同上 |
| 39 | built | ``${where}: ${what} ${role}.${field}.${key} reads a step role — … it would always be false; …`` | T1.5/W3 对账 | src/core/predicate.ts:605-609 | 同上 |
| 40 | built | ``IF condition: operand '${role}.${field}.${key}' cannot resolve — role '${role}' has no bound card`` | test/conditionals.test.ts:372（`/IF condition: operand 'object\.aspect\.demand' cannot resolve/`） | src/core/predicate.ts:500-503 | rust/src/core/predicate.rs `eval_expr` |
| 41 | built | ``internal: unresolved function parameter '${name}' was evaluated`` | （tripwire，无测试可达） | src/core/predicate.ts:512 | rust/src/core/predicate.rs `eval_expr` |
| 42 | built | ``${where}: 'roll' is read but no ROLL can have run before it in this outcome list — author a ROLL earlier in the list (the dice binding is per list/clause, §9.41)`` | test/dice.test.ts:427 域（may-bind lint） | src/core/predicate.ts:663-666 | rust/src/core/predicate.rs `walk_roll_may_bind` |
| 43 | built | ``unterminated string literal`` | T1.5/W3 对账 | src/core/lexer.ts:33 | rust/src/core/lexer.rs `lex` |
| 44 | built | ``bad id ref at offset ${i} (expected #id)`` | T1.5/W3 对账 | src/core/lexer.ts:40 | rust/src/core/lexer.rs `lex` |
| 45 | built | ``unexpected character '${c}' at offset ${i}`` | T1.5/W3 对账 | src/core/lexer.ts:66 | rust/src/core/lexer.rs `lex` |
| 46 | built | ``unexpected end of sentence`` | T1.5/W3 对账 | src/core/parse.ts:98 | rust/src/core/parse.rs `Parser::next` |
| 47 | built | ``expected '${v}', got '${show(t)}'`` | test/conditionals.test.ts:144（`/expected '\('/`） | src/core/parse.ts:104 | rust/src/core/parse.rs `expect_punct`、predicate.rs `expect`（同文 ``expected '${w}', got '${t}'``） |
| 48 | built | ``bad clock`` / ``bad halfhour (0..47)``（Tok 前端，parse.ts 侧） | T1.5/W3 对账 | src/core/parse.ts:119/122 | rust/src/core/parse.rs `sentence` |
| 49 | built | ``expected event trigger name`` / ``expected a name after ':' in an event trigger id`` | T1.5/W3 对账 | src/core/parse.ts:134/141 | rust/src/core/parse.rs `sentence` |
| 50 | built | ``expected a verb name after COMPLETE`` / ``expected a duration after WAIT`` | T1.5/W3 对账 | src/core/parse.ts:159/172 | rust/src/core/parse.rs `sentence` |
| 51 | built | ``unexpected trailing token '${show(t)}'`` | T1.5/W3 对账 | src/core/parse.ts:187 | rust/src/core/parse.rs `expect_end` |
| 52 | pinned | ``expected a registered verb, got '${show(verbTok)}'`` | test/sentence.test.ts:110/114（`/registered verb/`）→ rust/tests/sentence.rs `reserved_words_cannot_be_names`、`unregistered_verbs_are_rejected_at_the_actors_verb_boundary` | src/core/parse.ts:199 | rust/src/core/parse.rs `clause` |
| 53 | built | ``AT given twice in one clause`` | T1.5/W3 对账 | src/core/parse.ts:221 | rust/src/core/parse.rs `clause` |
| 54 | pinned | ``'${v}' is reserved and cannot be a name (quote it)`` | test/sentence.test.ts:108/118/131（`/reserved/`）→ rust/tests/sentence.rs `reserved_words_cannot_be_names`、`names_colliding_with_verbs_must_be_quoted`、`study_and_mansus_register_and_are_reserved_like_every_verb_lexeme`（/reserved/ 抽样锚原为 decks.test.ts:368 等） | src/core/parse.ts:246/252 | rust/src/core/parse.rs `ref_` |
| 55 | built | ``a parameter cannot follow 'kind:' — bind the whole ref, not the name part`` | T1.5/W3 对账 | src/core/parse.ts:251 | rust/src/core/parse.rs `ref_` |
| 56 | built | ``REPLACE takes the paren-less form: REPLACE ref WITH prototype`` / ``ENDING takes the paren-less form: ENDING ENDING_ID`` | T1.5/W3 对账 | src/core/parse.ts:290/294 | rust/src/core/parse.rs `op_outcome` |
| 57 | built | ``ROLL takes the paren form: ROLL(<count-expr>, <sides-expr>[, <bonus-expr>])``（含 roll_arg_expr 的 unexpected end 变体） | T1.5/W3 对账 | src/core/parse.ts:301/489 | rust/src/core/parse.rs `op_outcome`/`roll_arg_expr` |
| 58 | built | ``DRAW takes the paren-less form: DRAW <DECK> [INTO target]`` / ``DRAW needs a deck id: DRAW <DECK> [INTO target]`` | T1.5/W3 对账 | src/core/parse.ts:308/309 | rust/src/core/parse.rs `op_outcome` |
| 59 | built | ``ROLLED takes exactly two integers: ROLLED(<total>, <ctrAfter>)`` | T1.5/W3 对账（dice.test 域） | src/core/parse.ts:327 | rust/src/core/parse.rs `op_outcome` |
| 60 | built | ``DRAWN takes exactly two name refs: DRAWN(<DECK>, <name>)`` | T1.5/W3 对账（decks.test 域） | src/core/parse.ts:341 | rust/src/core/parse.rs `op_outcome` |
| 61 | built | ``NARRATE takes exactly one string`` / ``NARRATE expects a string, got '${show(s)}'`` | T1.5/W3 对账 | src/core/parse.ts:349/357 | rust/src/core/parse.rs `op_outcome` |
| 62 | built | ``REPLACE expects WITH`` / ``MOVE expects INTO`` / ``${op} requires parentheses`` | T1.5/W3 对账 | src/core/parse.ts:368/374/379 | rust/src/core/parse.rs `op_outcome` |
| 63 | built | ``expected key=value pair, got '${show(k)}'`` / ``expected #id after AS, got '${show(id)}'`` | T1.5/W3 对账 | src/core/parse.ts:530/525/568 | rust/src/core/parse.rs `spawn_args`/`name_arg` |
| 64 | pinned | ``deltas are integers only (no duration deltas)`` | test/sentence.test.ts:100-103（`/integers only/`）→ rust/tests/sentence.rs `deltas_are_integers_only_a_duration_after_a_sign_is_a_parse_error` | src/core/parse.ts:555 | rust/src/core/parse.rs `arg` |
| 65 | pinned | ``expected an integer after '${sign}', got '${show(n)}'`` / ``expected a value, got '${show(t)}'`` / ``unexpected token in outcome args: '${show(t)}'`` | test/sentence.test.ts:102（`/expected an integer/`）→ rust/tests/sentence.rs `deltas_are_integers_only_a_duration_after_a_sign_is_a_parse_error`（仅首变体；后两变体仍 T1.5/W3 对账） | src/core/parse.ts:556/638/611 | rust/src/core/parse.rs `arg`/`value_token` |
| 66 | built | ``function '${name}' expects ${n} argument(s)${list}, got ${got}`` | test/functions.test.ts:131-137 域 | src/core/parse.ts:402-406 | rust/src/core/parse.rs `fn_call` |
| 67 | built | unknownCallError 全文（``unknown '${name}(...)' in outcome position — the outcome vocabulary is: ${OUTCOME_OPS}…``） | test/functions.test.ts:138（`declared functions: one, zero`；罗列序=声明序） | src/core/parse.ts:843-853 | rust/src/core/fn_expand.rs `unknown_call_error` |
| 68 | built | undeclaredTokenError 全文（``function '${fn}': '${token}' in a body string names neither a parameter of '${fn}' nor a reserved token ({…}) — …``） | test/functions.test.ts:227（`/'\{nope\}' in a body string names neither a parameter of 'bad' nor a reserved token/`） | src/core/parse.ts:781-788 | rust/src/core/fn_expand.rs `undeclared_token_error` |
| 69 | built | ``function '${fn}': parameter '${param}' cannot interpolate into a string (bound to a ${t})`` | T1.5/W3 对账 | src/core/parse.ts:806 | rust/src/core/fn_expand.rs `arg_spelling` |
| 70 | built | ``function '${fn}': parameter '${name}' stands in an INTO position — the call supplies a ${t}, not a ref or role`` | T1.5/W3 对账 | src/core/parse.ts:750-753 | rust/src/core/fn_expand.rs `subst_arg` |
| 71 | built | ``function '${fn}': parameter '${name}' stands in an IF condition — the call supplies a ${t}, not a number`` | T1.5/W3 对账 | src/core/parse.ts:832-835 | rust/src/core/fn_expand.rs `subst_expr` |
| 72 | built | internal 家族（``internal: call to undeclared function '${name}' survived parsing`` 等 5 处） | （tripwire/不可达，无测试可达） | src/core/parse.ts:391/694/721/742/747/795/828 | rust/src/core/parse.rs、rust/src/core/fn_expand.rs |
| 73 | built | ``an IF outcome is authored form only — the log records the taken branch (§9.39)`` 及 FN/ROLL/DRAW/vexpr/param 五姊妹 | T1.5/W3 对账 | src/core/format.ts:41-44/92/93 | rust/src/core/format.rs `format_outcome`/`format_arg` |
| 74 | pinned | ``functions recurse: ${cycle.join(' -> ')} — calls expand at load time, so a recursion cycle never terminates; rewrite without the cycle`` | test/functions.test.ts:291-301（`/functions recurse: a -> b -> a/`、`/functions recurse: self -> self/`；全量移植在 W3）→ 模板先行冻结于 rust/tests/fn_expand_direct.rs `mutual_recursion_names_the_cycle_self_recursion_names_itself` | src/content/content.ts:559-562 | （W2 content.rs 构造点；按 dice.rs `script_seed_error` 同例先行钉模板） |
| 75 | pinned | ``function '${name}' sits ${d} calls deep — the nesting cap is ${MAX_FN_DEPTH}`` | test/functions.test.ts:303-310（`/function 'f1' sits 9 calls deep — the nesting cap is 8/`；全量移植在 W3）→ 模板先行冻结于 rust/tests/fn_expand_direct.rs `a_depth_8_chain_expands_transitively_depth_9_is_a_load_error` | src/content/content.ts:586 | （W2 content.rs 构造点；同上先行钉模板） |
| 76 | pinned | ``function '${tpl.name}': call to undeclared function '${c}'`` | （无 TS toThrow 直锚——parse 前端已拒未知调用，此为 content.ts pass-3 防御 tripwire；模板冻结于 rust/tests/fn_expand_direct.rs `an_undeclared_call_in_a_body_is_named_by_its_declaring_function`，手造 FN 节点可达） | src/content/content.ts:548 | （W2 content.rs 构造点） |

备注（W1）：

- 行 27-42、43-65 的文本在 rust 侧已有 `#[cfg(test)]` 冒烟断言冻结（fn_expand.rs / parse.rs / predicate.rs / clock.rs / query.rs 测试模块），但正式 pinned 口径以 T1.5 移植的 TS 测试文件为准。
- 行 67 的罗列序是 CORE-06 承重点：`unknownCallError` 用 `FnTable.by_name`（IndexMap）声明序 + `OUTCOME_OPS` 常量序，rust 冒烟测试已钉全文。
- internal 家族（行 72）在 TS 侧同样不可达（`!` 断言/防御分支），rust 侧以 `unreachable!` 或同文 `ChorusError` 落地，均为规则 11 注释锚定。

## T1.5 对账（W1 收口；随 rust/tests/{sentence,clock,query_predicate}.rs 移植升级）

- **升 pinned（11 行）**：6、7、8（clock 三消息，锚 rust/tests/clock.rs）；52、54、64、65（parse 域，锚 rust/tests/sentence.rs；行 65 仅 `/expected an integer/` 首变体）；17、29、30、31（query/predicate 域，锚 rust/tests/query_predicate.rs；行 29 仅 exists 变体）。
- **仍待对账（行 9、11、12(dict 版)、13、14、15、16、18-28、32-51、53、55-63、66-73 及 65/29 的次要变体）**：本波 3 个测试文件无可达断言，锚点在 W3 移植的 decks/conditionals/dice/engine/functions 各文件（行 33-42 的 lint 家族在 conditionals.test.ts，行 66-71 在 functions.test.ts，均在 W3）。
- **新增 3 行（74-76，W2 content.rs 构造点）**：fn_expand 直测子集（PR3-01）在 T1.5 先行把宏展开的环/深度/未声明调用三条消息模板逐字钉进 rust/tests/fn_expand_direct.rs——同 dice.rs 行 1 的"构造点未到、模板先冻结"先例；行 74/75 的 TS 锚（functions.test.ts:291-310）随 W3 全量移植复核。
- 对账口径（§7 规则 13(d)）：本波移植的 toThrow 计数 = sentence 9 + clock 9 + query-predicate 6 = 24 条 toThrow 断言（与 TS 原文逐条对应）；其中 10 行升级 pinned，其余断言命中的模板已在表行内登记（多断言共钉一行，如行 54 的三处 /reserved/ 抽样）。
