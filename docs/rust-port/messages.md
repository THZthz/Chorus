# 错误消息对照表（messages.md）

plan §2.5 / TEST-14 / 验证 04 must#4：错误消息文本是契约。三列对照：消息模板 ↔ 首个钉住它的测试 ↔ TS 源行；Rust 构造点随波登记。模板必须逐字符相同（含空格与标点）；TS `${…}` 插值在表中以 `${…}` 原样记。随波增补，W6 收口覆盖全仓 192 条 `toThrow`（验证 01 CORE-16/TEST-05 勘误口径）。

状态图例：**built** = Rust 构造点已存在；**pinned** = 模板已在 Rust 测试侧逐字冻结（构造点在后续波）；**registered** = 仅登记文本与出处（W2/W3 落地时升为 pinned/built）。

| # | 状态 | 消息模板 | 首个钉住的测试 | TS 源行 | Rust 构造点 |
|---|------|---------|---------------|---------|------------|
| 1 | built | ``line ${ln + 1}: seed must be an integer, got '${m[2].trim()}'`` | test/dice.test.ts:181（`/seed must be an integer/`） | src/content/script.ts:113 | rust/src/content/script.rs `parse_script`（W2 升 built：构造点就地构造；W0 测试助手 rust/tests/dice.rs `script_seed_error` 仅为模板冻结，未被生产代码引用） |
| 2 | built | ``deck '${deck.id}' is exhausted — no 'reset' and no 'defaultcard' (§9.42)`` | test/decks.test.ts:410（refusal `/deck 'SINGLES' is exhausted — no 'reset' and no 'defaultcard'/`；原文含 `(§9.42)` 后缀） | src/content/content.ts:476 | rust/src/content/content.rs `draw_from`（W2 自 core/deck_rng.rs 并入） |
| 3 | built | ``ROLL: dice count must be an integer >= 0, got ${count ?? 'nothing'}`` | test/dice.test.ts:271/346（refusal `/ROLL: dice count must be an integer >= 0/`） | src/engine/engine.ts:764 | rust/src/engine/engine.rs `apply_roll`（W3 升 built；`Number.isInteger` 支在 i64 域不可达，undefined/range 支逐字保留） |
| 4 | built | ``ROLL: sides must be an integer >= 2, got ${sides ?? 'nothing'}`` | test/dice.test.ts:315/328（refusal `/sides must be an integer >= 2/`） | src/engine/engine.ts:767 | rust/src/engine/engine.rs `apply_roll`（W3） |
| 5 | built | `'roll' is unbound — no ROLL has run in this outcome list before this point` | test/dice.test.ts:423/433（refusal `/'roll' is unbound/`） | src/core/predicate.ts:477 | rust/src/core/predicate.rs `eval_expr`（W1 构造点已在，W3 经 ROLL/IF 路径验证） |
| 6 | built | ``the story has ended — ${stateIn.ended.title}`` | test/endings.test.ts（引擎版 game-over 短文；T3.7 全量移植） | src/engine/engine.ts:155 | rust/src/engine/engine.rs `execute_step`（W3；rust/tests/engine_ordering.rs `first_ending_wins_and_gates_the_story` 逐字断言） |
| 7 | built | ``clock ${formatClock(clockFromAbsMin(startMin))} is before state.now (${now.day}:${now.halfhour})`` | test/ 无直接 toThrow（§9.5 行为） | src/engine/engine.ts:163 | rust/src/engine/engine.rs `execute_step`（W3；engine.rs 冒烟 `rejected_messages_match_the_ts_texts` 逐字断言） |
| 8 | built | ``nothing to do — the clock already reads ${now.day}:${now.halfhour} (commit an action, or name a later minute to wait)`` | test/parallel.test.ts（`/nothing to do/`；T3.7 移植） | src/engine/engine.ts:169 | rust/src/engine/engine.rs `execute_step`（W3；ENG-18 活分支 `toMin === now && !hasDue`） |
| 9 | built | ``${verb} is already processing — its station frees at ${end.day}:${end.halfhour}`` | test/parallel.test.ts（`/SEIZE is already processing/`；T3.7 移植） | src/engine/engine.ts:283 | rust/src/engine/engine.rs `busy_reason`（W3；engine_ordering.rs `stations_and_committed_locks_gate_phase_two` 逐字断言） |
| 10 | built | ``${e.name.replace(/_/g, ' ')} is busy with ${w.verb} — it frees at ${end.day}:${end.halfhour}`` | test/parallel.test.ts（`/MAGICK BOOK is busy with SEIZE/`；T3.7 移植） | src/engine/engine.ts:293 | rust/src/engine/engine.rs `busy_reason`（W3；同上逐字断言） |
| 11 | built | ``wait row ${record.id} starts at ${record.tGame} but the folded clock reads ${state.now} — wait spans must partition time (log drift, or a pre-v0.5 log)`` | test/engine.test.ts（wait 连续性断言；T3.7 移植） | src/engine/engine.ts:959 | rust/src/engine/engine.rs `apply_record`（W3；两处 Rust 测试逐字断言） |
| 12 | built | ``DRAWN: deck '${deck.id}' draw #${cursor} recorded '${nameArg.ref.name}' but the compiled deck serves '${expected.name}' — content drift between play and fold (§9.42)`` | test/decks.test.ts（内容漂移响亮失败；T3.7 移植） | src/engine/engine.ts:864-866 | rust/src/engine/engine.rs `apply_outcome` DRAWN 臂（W3；engine.rs 冒烟逐字断言） |

备注：

- 行 1 的 `line` 前缀是 1 基行号（`ln + 1`，`ln` 为 `text.split('\n')` 的 0 基下标）；dice.test.ts 的 rig 里 `seed:` 行在 0 基下标 4，消息即 `line 5: …`（rust/tests/dice.rs 已按此冻结）。
- 行 3/4/5 为 W0 预登记（文本自 engine.ts:764/767、predicate.ts:477 原文抄录），W3 移植构造点时逐字核对并升为 built。
- 表行增量须与各波移植的 `toThrow` 数对账（§7 规则 13(d)）；W0 波移植的 dice.test.ts:181 一条对应行 1（decks.test.ts:410 是 refusal/toMatch 断言，非 toThrow，行 2 因此单独登记）。
- W3（T3.1–T3.4）：engine/store/fold 构造点全部落地，行 3–5 升 built；行 6–12 为本波新增构造点中已被 Rust 侧测试逐字断言者（先行钉住，T3.7 移植 TS 测试后自然对账）。engine.ts 其余 throw 文案（validateSlots 五条、SPAWN/REPLACE/SET/MOVE/RELATE/ENDING/DRAW 各臂、`begin row … drift`、fireWindow 漂移、unknown outcome op、store 的行号错误）构造点均已按原文落地（`// ts:` 锚逐条在位），随 T3.7 的 9 个测试文件移植逐条升 pinned/登记。

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
| 74 | built | ``functions recurse: ${cycle.join(' -> ')} — calls expand at load time, so a recursion cycle never terminates; rewrite without the cycle`` | test/functions.test.ts:291-301（`/functions recurse: a -> b -> a/`、`/functions recurse: self -> self/`；全量移植在 W3）→ 模板先行冻结于 rust/tests/fn_expand_direct.rs `mutual_recursion_names_the_cycle_self_recursion_names_itself`；W2 经真路径复核（rust/tests/script_play.rs `function_guards_fire_through_the_real_compile_path_with_the_frozen_texts`，a→b→a 逐字相等） | src/content/content.ts:559-562 | rust/src/content/content.rs `build_fn_table` pass 3 |
| 75 | built | ``function '${name}' sits ${d} calls deep — the nesting cap is ${MAX_FN_DEPTH}`` | test/functions.test.ts:303-310（`/function 'f1' sits 9 calls deep — the nesting cap is 8/`；全量移植在 W3）→ 模板先行冻结于 rust/tests/fn_expand_direct.rs `a_depth_8_chain_expands_transitively_depth_9_is_a_load_error`；W2 经真路径复核（同上，f1→…→f9 逐字相等） | src/content/content.ts:586 | rust/src/content/content.rs `build_fn_table` pass 3 |
| 76 | built | ``function '${tpl.name}': call to undeclared function '${c}'`` | （无 TS toThrow 直锚——parse 前端已拒未知调用，此为 content.ts pass-3 防御 tripwire；模板冻结于 rust/tests/fn_expand_direct.rs `an_undeclared_call_in_a_body_is_named_by_its_declaring_function`，手造 FN 节点可达） | src/content/content.ts:548 | rust/src/content/content.rs `build_fn_table` pass 3 |

备注（W1）：

- 行 27-42、43-65 的文本在 rust 侧已有 `#[cfg(test)]` 冒烟断言冻结（fn_expand.rs / parse.rs / predicate.rs / clock.rs / query.rs 测试模块），但正式 pinned 口径以 T1.5 移植的 TS 测试文件为准。
- 行 67 的罗列序是 CORE-06 承重点：`unknownCallError` 用 `FnTable.by_name`（IndexMap）声明序 + `OUTCOME_OPS` 常量序，rust 冒烟测试已钉全文。
- internal 家族（行 72）在 TS 侧同样不可达（`!` 断言/防御分支），rust 侧以 `unreachable!` 或同文 `ChorusError` 落地，均为规则 11 注释锚定。

## T1.5 对账（W1 收口；随 rust/tests/{sentence,clock,query_predicate}.rs 移植升级）

- **升 pinned（11 行）**：6、7、8（clock 三消息，锚 rust/tests/clock.rs）；52、54、64、65（parse 域，锚 rust/tests/sentence.rs；行 65 仅 `/expected an integer/` 首变体）；17、29、30、31（query/predicate 域，锚 rust/tests/query_predicate.rs；行 29 仅 exists 变体）。
- **仍待对账（行 9、11、12(dict 版)、13、14、15、16、18-28、32-51、53、55-63、66-73 及 65/29 的次要变体）**：本波 3 个测试文件无可达断言，锚点在 W3 移植的 decks/conditionals/dice/engine/functions 各文件（行 33-42 的 lint 家族在 conditionals.test.ts，行 66-71 在 functions.test.ts，均在 W3）。
- **新增 3 行（74-76，W2 content.rs 构造点）**：fn_expand 直测子集（PR3-01）在 T1.5 先行把宏展开的环/深度/未声明调用三条消息模板逐字钉进 rust/tests/fn_expand_direct.rs——同 dice.rs 行 1 的"构造点未到、模板先冻结"先例；行 74/75 的 TS 锚（functions.test.ts:291-310）随 W3 全量移植复核。
- 对账口径（§7 规则 13(d)）：本波移植的 toThrow 计数 = sentence 9 + clock 9 + query-predicate 6 = 24 条 toThrow 断言（与 TS 原文逐条对应）；其中 10 行升级 pinned，其余断言命中的模板已在表行内登记（多断言共钉一行，如行 54 的三处 /reserved/ 抽样）。

## W2 增补（T2.1–T2.3，content/script.rs + content/content.rs 构造点全量落地；行 77–111）

W2 落地 `parse_script`/`compile_script`/`lint_script` 与 `compile_content`/`build_fn_table` 的全部 TS throw 点（状态 **built**，构造点带 `// ts:` 锚）。本波移植的测试（lint.test 14 test + script-play 编译器子集）含 **16 条 toThrow 断言**（lint.test 9：模板未注册 1 + player 2 + reserved 4 + 重复实体 1 + 双声明 1；script-play 编译器子集 7：坏 expiry 触发器 3 + 坏脚本 4），对应行 77/79/80/82/88/89/97/97b/108/109/110/116 的首钉标注；其余行为 W3 对账（锚在 dice/decks/engine/functions/stackable/conditionals 的 W3 移植）。lint 八族告警文本（从不 throw）登记于行 110b。

| # | 状态 | 消息模板 | 首个钉住的测试 | TS 源行 | Rust 构造点 |
|---|------|---------|---------------|---------|------------|
| 77 | pinned | ``line ${ln + 1}: expected a section header, got '${t}'`` | test/script-play.test.ts:242（`/section header/`）→ rust/tests/script_play.rs `bad_scripts_fail_loudly` | src/content/script.ts:129 | rust/src/content/script.rs `parse_script` |
| 78 | built | ``line ${ln + 1}: duplicate ending id '${id}'`` | T2.3/W3 对账 | src/content/script.ts:152 | rust/src/content/script.rs `parse_script`（endings 臂） |
| 79 | pinned | ``line ${ln + 1}: duplicate entity declaration '${name}'`` | test/lint.test.ts:46（`/duplicate entity declaration 'HOUSE'/`）→ rust/tests/lint.rs `duplicate_entity_declarations_are_a_load_error_not_last_write_wins` | src/content/script.ts:138 | rust/src/content/script.rs `parse_script`（entities 臂；QUIRK(Q5) 大小写敏感查重） |
| 80 | pinned | ``'${name}' is declared both as an entity and a prototype — spawns of it could never mint`` | test/lint.test.ts:50（`/'SPARK' is declared both as an entity and a prototype/`）→ rust/tests/lint.rs `a_name_declared_both_as_entity_and_prototype_can_never_spawn_load_error` | src/content/script.ts:186 | rust/src/content/script.rs `parse_script`（收尾查重，JS 键序遍历） |
| 81 | built | ``line ${ln + 1}: templates expect  VERB says "..."``（expect 后双空格） | T2.3/W3 对账 | src/content/script.ts:156 | rust/src/content/script.rs `parse_script`（templates 臂） |
| 82 | pinned | ``line ${ln + 1}: verb '${verbName}' is not registered in the dictionary`` | test/script-play.test.ts:241（`/not registered/`）→ rust/tests/script_play.rs `bad_scripts_fail_loudly` | src/content/script.ts:162 | rust/src/content/script.rs `parse_script`（verb 头） |
| 83 | built | ``line ${ln + 1}: expected a verb header inside 'verbs:'`` | T2.3/W3 对账 | src/content/script.ts:167 | rust/src/content/script.rs `parse_script`（verbs 臂） |
| 84 | built | ``line ${ln + 1}: expected 'role: count query', 'duration: X', or 'recipes:'`` | T2.3/W3 对账 | src/content/script.ts:172 | rust/src/content/script.rs `parse_script`（slot 行正则失败） |
| 85 | built | ``line ${ln + 1}: unexpected line in verb ${verbName}: '${t}'`` | T2.3/W3 对账 | src/content/script.ts:178 | rust/src/content/script.rs `parse_script`（verbs 臂 else） |
| 86 | built | ``line ${ln}: entities expect  NAME is KIND [with ...] [at PLACE]``（expect 后双空格） | T2.3/W3 对账 | src/content/script.ts:206 | rust/src/content/script.rs `parse_entity_line` |
| 87 | built | ``line ${ln}: relations expect  A is TYPE of B``（expect 后双空格） | T2.3/W3 对账 | src/content/script.ts:211 | rust/src/content/script.rs `parse_entity_line` |
| 88 | pinned | ``line ${ln}: ${what} '${name}' is a reserved word — rename it (§5.1)``（what ∈ entity name / relation endpoint / prototype name） | test/lint.test.ts:37-40（`/'TO' is a reserved word/` ×3、`/'IF' is a reserved word/`）→ rust/tests/lint.rs `reserved_words_cannot_be_names` | src/content/script.ts:200 | rust/src/content/script.rs `check_name` |
| 89 | pinned | ``line ${ln}: unknown kind '${second}' (expected one of ${KINDS.join(', ')})``（实体变体）与 ``line ${ln}: unknown kind '${kind}'``（prototype 变体，无 expected 后缀） | test/script-play.test.ts:239-240（`/unknown kind/` ×2，实体变体；'pals' 非注册 relation 走同文）→ rust/tests/script_play.rs `bad_scripts_fail_loudly` | src/content/script.ts:219/261 | rust/src/content/script.rs `parse_entity_line`/`parse_prototype_line` |
| 90 | built | ``line ${ln}: could not parse entity line '${t}'`` | T2.3/W3 对账 | src/content/script.ts:250 | rust/src/content/script.rs `parse_entity_line` |
| 91 | built | ``line ${ln}: prototypes expect  NAME is KIND [stackable] [deriving ...] [with ...]`` | T2.3/W3 对账 | src/content/script.ts:258 | rust/src/content/script.rs `parse_prototype_line` |
| 92 | built | ``line ${ln}: duplicate 'with' clause`` ／ ``line ${ln}: duplicate 'deriving' clause`` | T2.3/W3 对账 | src/content/script.ts:272/285 | rust/src/content/script.rs `parse_prototype_line` |
| 93 | built | ``line ${ln}: bad aspect spec '${spec.trim()}' (expected aspect:level)``（实体变体）／``line ${ln}: bad aspect spec '${spec.trim()}'``（prototype 变体） | T2.3/W3 对账 | src/content/script.ts:241/277 | rust/src/content/script.rs `parse_entity_line`/`parse_prototype_line` |
| 94 | built | ``line ${ln}: bad derive '${item.trim()}' (expected 'name from object' or 'field from object.field-2')`` | T2.3/W3 对账 | src/content/script.ts:293 | rust/src/content/script.rs `parse_prototype_line` |
| 95 | built | ``line ${ln}: could not parse prototype line '${t}'`` | T2.3/W3 对账 | src/content/script.ts:301 | rust/src/content/script.rs `parse_prototype_line` |
| 97 | pinned | ``line ${ln}: triggers expect  ID [once] at (hour|halfhour) N -> OUTCOMES  or  ID on expire of NAME -> OUTCOMES``（OUTCOMES 与 or 间双空格） | test/script-play.test.ts:233-235（`/triggers expect/` ×3）→ rust/tests/script_play.rs `bad_expiry_trigger_grammar_fails_loudly` | src/content/script.ts:312 | rust/src/content/script.rs `parse_trigger_line` |
| 97b | built | ``line ${ln}: trigger hour must be 0..23`` ／ ``line ${ln}: trigger halfhour must be 0..47`` | T2.3/W3 对账 | src/content/script.ts:314-315 | rust/src/content/script.rs `parse_trigger_line` |
| 98 | built | ``line ${ln}: functions expect  NAME(param, …) -> OUTCOMES  or  NAME param … -> OUTCOMES`` | T2.3/W3 对账 | src/content/script.ts:333 | rust/src/content/script.rs `parse_function_line` |
| 99 | built | ``line ${ln}: decks expect  ID spec NAME, … [defaultcard NAME] [reset]`` | T2.3/W3 对账 | src/content/script.ts:345 | rust/src/content/script.rs `parse_deck_line` |
| 100 | built | ``line ${ln}: deck ${id}: the spec is empty``（parse 侧） | T2.3/W3 对账 | src/content/script.ts:366 | rust/src/content/script.rs `parse_deck_line` |
| 101 | built | ``line ${ln}: endings expect  ID: "Title" (flavour)  — flavour optional, default melancholy``（flavour) 与 — 间双空格） | T2.3/W3 对账 | src/content/script.ts:376 | rust/src/content/script.rs `parse_ending_line` |
| 102 | built | ``line ${ln}: bad slot '${role}: ${rest}'`` ／ ``line ${ln}: bad count '${countSpec}' (expected 1, 1..2, 2.., 0..1)`` | T2.3/W3 对账 | src/content/script.ts:385/388 | rust/src/content/script.rs `parse_slot_line` |
| 103 | built | ``line ${ln}: recipes expect  [id] [hidden|shown] [hint "..."] [when COND] -> OUTCOMES`` | T2.3/W3 对账 | src/content/script.ts:396 | rust/src/content/script.rs `parse_recipe_line` |
| 104 | built | ``entity ${e.name}: unknown container '${e.containerName}'`` | T2.3/W3 对账 | src/content/script.ts:445 | rust/src/content/script.rs `compile_script` |
| 105 | built | ``relation ${rel.a} is ${rel.type} of ${rel.b}: unknown entity`` | T2.3/W3 对账 | src/content/script.ts:450 | rust/src/content/script.rs `compile_script` |
| 108 | pinned | ``player '${meta.player}' is not a declared entity`` | test/lint.test.ts:31（`/player 'GHOST' is not a declared entity/`）→ rust/tests/lint.rs `player_must_name_a_declared_character` | src/content/script.ts:462 | rust/src/content/script.rs `compile_script` |
| 109 | pinned | ``player '${meta.player}' must be a character (got ${p.kind})`` | test/lint.test.ts:33（`/player 'HOUSE' must be a character/`）→ rust/tests/lint.rs `player_must_name_a_declared_character` | src/content/script.ts:463 | rust/src/content/script.rs `compile_script` |
| 110 | built | ``ending '${id}' must be UPPER_SNAKE`` ／ ``ending ${id}: needs a title`` ／ ``ending ${id}: flavour must be a lowercase word, got '${flavour}'`` | T2.3/W3 对账（endings.test W3） | src/content/content.ts:209-212 | rust/src/content/content.rs `compile_content` |
| 110b | built | lint 八族告警（从不 throw，逐字）：``verb ${name}: declares slots but no recipes — every action rejects at selection (§6.6)``／``recipe ${id} (${name}): unreachable — ${catchAll} already matches everything (first match wins, §6.4)``／``recipe ${id} (${name}): shadowed by ${prior} — identical when, it can never fire (§6.4)``／``recipe ${id} (${name}): count(${role}) ${op} ${k} can never hold — the slot takes ${range} (§6.1)``／``${where}: unknown template token {${tok}} — roles are {actors} {object} {with} {at} {about} {to}, plus {roll} (§5.3)``／``deck ${id}: entry ${entry} is not stackable — served again while a copy lives, the spawn collides (§10 livelock rule)``／``aspect '${r}' is queried but never declared on any entity or prototype — check the spelling``／``entity '${e.name}': not a name lexeme — every ref to it must be quoted (§5.1, §9.42)`` | test/lint.test.ts 全部 9 个 lint test（`.some(includes)` 抽样钉）→ rust/tests/lint.rs；`the_bundled_showcase_lints_clean`（空列表）为反向整体验收 | src/content/script.ts:470-619 | rust/src/content/script.rs `lint_script`（注：lint 侧 recipe id 为 1 基镜像，QUIRK(Q1/ENG-15)） |
| 111 | built | ``verb '${name}' is not registered in the dictionary``（compile 侧，raw.verbs 直构路径） | T2.3/W3 对账 | src/content/content.ts:220 | rust/src/content/content.rs `compile_content` |
| 112 | built | ``verb ${name}: slot ${role} has max < min`` | T2.3/W3 对账 | src/content/content.ts:223 | rust/src/content/content.rs `compile_content` |
| 113 | pinned | ``template '${name}' is not a registered verb — templates configure verbs, same as the verbs: section`` | test/lint.test.ts:26（`/WRONGVERB.*not a registered verb/`）→ rust/tests/lint.rs `a_template_for_an_unregistered_verb_is_rejected` | src/content/content.ts:277 | rust/src/content/content.rs `compile_content` |
| 114 | built | ``trigger ${id}: PROGRESS belongs to verb recipes, not events`` ／ ``trigger ${id}: REFUSE belongs to verb recipes — events have no step to refuse`` | T2.3/W3 对账（endings/conditionals W3；PROGRESS 版另见 test/engine.test.ts:1002） | src/content/content.ts:252/255 | rust/src/content/content.rs `compile_content`（触发器守卫） |
| 115 | built | ``${where}: ROLLED is engine-written — author ROLL and the log records the concrete result (§9.41)`` ／ ``${where}: DRAWN is engine-written — author DRAW and the log records the concrete DRAWN+SPAWN rows (§9.42)`` | T2.3/W3 对账（dice/decks W3） | src/content/content.ts:340/343 | rust/src/content/content.rs `lint_dice` |
| 116 | built | ``${where}: {roll} is only available in NARRATE strings — ${op} prose is load-fixed (§9.41)`` | T2.3/W3 对账（dice W3） | src/content/content.ts:348 | rust/src/content/content.rs `lint_dice` |
| 117 | built | ``${where}: ENDING '${id ?? '?'}' is not in the endings table`` | T2.3/W3 对账（endings W3） | src/content/content.ts:365 | rust/src/content/content.rs `check_ending_refs` |
| 118 | built | ``${where}: DRAW '${id ?? '?'}' is not a declared deck (decks:)`` | T2.3/W3 对账（decks W3，decks.test.ts:619-621 域） | src/content/content.ts:378 | rust/src/content/content.rs `check_deck_refs` |
| 119 | built | ``deck '${id}' must be UPPER_SNAKE`` ／ ``'${id}' is reserved and cannot be a deck id`` ／ ``duplicate deck '${id}'`` ／ ``deck ${id}: the spec is empty``（compile 侧） | T2.3/W3 对账（decks W3；UPPER_SNAKE 变体钉于 test/decks.test.ts:631 域） | src/content/content.ts:411-414 | rust/src/content/content.rs `compile_decks` |
| 120 | pinned | ``deck ${deck}: ${what} '${name}' is neither a declared prototype nor an entity``（what ∈ spec entry / defaultcard） | W2 冒烟：rust/tests/script_play.rs `direct_compile_content_without_entities_rejects_entity_deck_entries`（整句相等断言，content.ts:194 的"无 entities 只对 prototypes 校验"行为）；TS 全量锚 test/decks.test.ts:620（`/deck RUMOURS: defaultcard 'NOSUCH' is neither…/`）在 W3 | src/content/content.ts:407 | rust/src/content/content.rs `compile_decks` `resolve` |
| 121 | built | ``function name '${def.name}' must be an identifier (letters, digits, _)`` ／ ``function '${def.name}' collides with an outcome op — functions and ops share one caller vocabulary; rename the function`` ／ ``function '${def.name}' cannot be named a conditional keyword — IF/ELSE dispatch before functions in outcome position`` ／ ``duplicate function '${def.name}'`` ／ ``function '${def.name}': parameter '${p}' must be an identifier`` ／ ``function '${def.name}': duplicate parameter(s) ${dups.join(', ')}`` ／ ``function '${def.name}': ${err.message}``（pass-2 包装） | T2.3/W3 对账（functions.test 全量在 W3） | src/content/content.ts:513-538 | rust/src/content/content.rs `build_fn_table` pass 1/2 |

W2 对账备注：

- 行 77/79/80/82/88/89/97/108/109/113/120 升 **pinned**（本波 16 条 toThrow 的落点；多断言共钉一行如行 88 的三处 /reserved/ 抽样、行 89 的两处 /unknown kind/）。
- 行 1（seed）升 **built**：构造点在 `parse_script` 就地逐字构造，W0 的测试助手未进生产代码（计划 §2.5）。
- 行 74/75/76 升 **built** 且经真路径复核：`compile_script`→`build_fn_table` 触发，`a -> b -> a` 环与 `f1..f9` 深度 9 的全文在 rust/tests/script_play.rs 与先行冻结模板逐字相等。
- 其余 built 行的 TS 首钉锚位于 W3 移植的测试文件（dice/decks/engine/functions/stackable/conditionals/endings），W3 移植时升 pinned。
- 附注（行 102）：`parse_slot_line` 的 `max` 承接 `2..`→Infinity（None 建模），`bad count` 文案与 TS 逐字一致。
