# W3 波次门评审记录（§7 规则 13，PR3-04）

- 评审对象：`rust-port-w2..HEAD`（22 commit，五棒：W3a store/fold/engine+排序测试；W3b tests/common+session 层；W3c-A engine/parallel/functions 测试；W3c-B decks/dice 剩余+stackable+conditionals；W3c-C endings/nightjar/nightjar-explorer/script-play 剩余；另 3 个 messages.md 对账 commit 与 1 个 lenient 注释 commit）
- 评审人：wave integrator 指派的独立 reviewer（非本波作者）
- 评审日期：2026-10-14
- 证据方式：(a) 项按规则做 **core/engine 级全量逐段对照**（rust/src/engine/{engine,fold,store}.rs 3399+53+275 行 vs src/engine/{engine.ts 1298,fold.ts 17,store.ts 32} 行逐函数核对算法/顺序/错误文本/边界；rust/src/session/{game,lenient,render}.rs vs src/ui/{play,lenient,render}.ts 逐方法；rust/tests/common/{snap,settle,seed,explorer_lib}.rs vs test/{snap,settle,seed,explorer-lib}.ts 逐 helper）；(b)–(g) 逐项 grep/对账；`cargo test -j 4`（537 passed / 0 failed）与 `cargo clippy -j 4 --all-targets`（0 warning）亲自重跑（rust/ 下，无 vitest/tsx 并发）。

## 总评：**PASS-with-notes**

537/537 全绿（W2 的 163 净增 374：engine 74 + engine_ordering 17 + parallel 13 + functions 27 + decks +42 + dice +45 + stackable 21 + conditionals 36 + endings 25 + nightjar 21 + nightjar-explorer 4 + script_play +14 + session_smoke 7 + common_smoke 9 + store 4 等），clippy 零警告，TS 侧零改动，gate 测试未放松。(a) 全量对照未发现任何可观察行为分叉；(g) 五个重点分歧论证全部成立。遗留 should-fix 3 条（两条 messages.md 记账口径、一条 Phase-2 内部错误通道对齐）+ note 4 条，均不阻塞。可打 `rust-port-w3` tag。

---

## (a) 逐段对照（core/engine 级全量）— PASS

**engine.rs ↔ engine.ts（1298 行，逐段）**

- **executeStep 四阶段序**（:144-271 ↔ engine.rs:275-509）：ended 硬门（含 `the story has ended — ${title}` 逐字）→ `startMin ?? absMin(clock ?? clockFromAbsMin(now))`（Option+unwrap_or_else 保 0，ENG-07 注释）→ before-now 拒绝 → 空 clauses 的 `dueNow` + `nothing to do` 活分支 → Phase 1 `resolveDue` 整体 try/catch→rejected → 裸时钟纯推进早退 → Phase 2 逐 clause（verb 缺失/validateSlots/busy/selectRecipe，错误文本逐字含 `${clause.verb}: ` 前缀）→ Phase 3 REFUSE 预演（scratch 克隆、`s${nextId}_${++smint}` idGen、**逐 clause `sctx.roll = None` 复位**、IF 对门透明、throw≠gate（Rust 以返回 `Result<bool>` 的闭包精确复刻 TS try/catch 语义，Err → refused=false + collected 清空）、refused → `firstRecordId` 复用 + advancedMin=0 + state=stateIn）→ Phase 4 仅 PROGRESS 上 BEGIN 行 + registerWindows。id 通道：注入/默认 `e${firstRecordId}_{n}` 单一闭包贯穿 pass 与 fireWindow，Phase 3 独立 s-gen——与 TS 逐点对应。
- **applyOutcome 17 op 逐 op**：IF（arms 顺序求值、taken-branch 拍平、无真臂无 ELSE 静默）、SPAWN（ref→proto/kind 解析→aspects/attrs JS 键序克隆（PR2-05 的 engine.ts:469/934 两点经 `clone_map_in_js_order`/`clone_aspects` 覆盖）→derive（`Object.entries(derive.aspects)` 走 `ordinary_own_property_keys`）→mintIsStackable(**protoName** 非派生名，与 TS 同)→池检查（缺位 pid ≡ `undefined?.stackable === true` → false）→asId/into/container（at 绑定→首 actor container，`if (container)` 用 `js_truthy` 保 ''/0/false falsy）→JSON pair 物化优先 + attrs pair 权威（`delete attrs['container']` 以 `shift_remove`+`had_container` 保其余键槽位）→concrete（nameRef+into+asId+两 pair 键序走 jsjson）、DESTROY（§2.6.3 id 拆分：stamp 后经 name+id 释放）、REPLACE（池检查在循环内、aspects 模板克隆逐 dying、container/zone 继承、attrs pair **恒**物化、SPAWN+DESTROY 对）、SET（keyConcrete 保 authored 形、delta 的 raw-entry 非数值拒绝（aspect 子映射 REFUSE 不 clobber）、负值允许、role→逐卡 concrete、expires→expiresAt 锚 `ctx.now`）、MOVE（INTO role→idOrSelf、dest 严格）、REVEAL、ADVANCE、NARRATE（Q7 rewrite）、PROGRESS、RELATE/UNRELATE（stamp 不擦除）、ENDING（首胜）、REFUSE、ROLL（三 vexpr 严格求值、`got …'nothing'` 三条逐字、`Number.isInteger` i64 域不可达注释、c..c+m−1、ctx.roll 绑定、ROLLED concrete）、ROLLED（fold 侧只落计数）、DRAW（INTO 严格解析、游标 draft 推进、DRAWN+SPAWN 双行）、DRAWN（漂移响亮错误全文逐字）、unknown op。**全部错误消息与 TS 逐字符一致**（抽查 40+ 条含标点/空格/破折号）。
- **resolveDue/fireWindow/fireSlot 顺序语义**（:1047-1273 ↔ engine.rs:2135-2560）：`hasDue` 活分支保留（ENG-18 首析取支死、第二支活的注释逐点在位）；过期快照一次 hoisted（entities 插入序，`Expiring.name` 死字段镜像保形）；due 全序 `(end, beginId, clause)`；`nextW.end <= slotAbs` 完成先行；**slot cursor 不跳窗口跨度**注释；ADVANCE 扩展 target；fireWindow：drift throw 出 pass（ Phase 1 捕获→rejected）、scratch 克隆、死绑 DROP、锚 `w.start`、try/catch 罩 outcome 循环→REFUSE marker 携 message、`refused ? stateIn : scratch`、`windows.delete`（shift_remove ≡ Map.delete 保序）；fireSlot：authored 声明序 → once 门 → expiry 快照序（**id 寻址**内建 destroy）→ authored on-expire **按名首条匹配**（含 §9.38 池不对称裁决注释）；`makeEventCtx` 的 `absSlot * MIN_PER_HALFHOUR`（ENG-19 三魔数的授权等值改写，注释在位）；wait 行收尾 + `state.now = target`。
- **registerWindows 往返+静默 catch**（:344-378 ↔ engine.rs:634-694）：`parse_sentence` Err → `Vec::new()`（**QUIRK(Q6/ENG-06) 注释 + 冒烟测试** `register_windows_roundtrips_…` 双钉）；startMin `?? absMin(parseClock(tGame))`；offset 累加（checked_step 为 §3.3 文档化分歧）；verb drift throw 不被 execute_step 捕获（`?` 直传出——与 TS 一致）。
- **applyRecord**（:947-1028 ↔ engine.rs:1959-2077）：wait 连续性断言全文逐字；complete 先查删窗（**refused 完成也关窗**）、REFUSE 早退、startMin 窗表→tGame 回退分支保留（ENG-12 注释）；行 1000 的 REFUSE 守卫覆盖 event **与 action**（refused BEGIN 不复开窗）；event 先 `fired.add` 后 parseClock（throw 序一致）；ENG-03 的共享 ctx 构造抽取为 `fold_bindings`，分派骨架保留；action → registerWindows。

**fold.rs ↔ fold.ts / store.rs ↔ store.ts**：`fold` 逐条 applyRecord over clone；`fold_up_to` 的 `r.id < upto_exclusive`（ENG-11 承重注释在位，禁 `<=`）。store：`next_record_id`/`append_all`/`truncate`（play.ts:164 注释）；`Log.journal()` 未移植（死代码 UI-05）；saveLog 逐 record `serde_json::to_string` + `\n`；**两条登记分歧**（原子写 tmp+rename、loadLog 行号）均有模块头论证且无 TS 测试钉住（ENG-20 授权口径）。serde 字段序 = TS 字面量序（`tGame`/`tReal`/`startMin` rename、Option 无 skip 序列化为 null——与 TS `trigger: null` 同形），`kind` 保持 String 的 open-world 理由成立。

**session 层 ↔ play/lenient/render.ts**：

- **Game 全部方法一个不丢**（UI-01 的 7 文件锚定面）：from_script/title/handle_line/undo(+turn_lengths 记账)/wait/visible/look/cards/inspect/story/journal/processing/suggestions——逐方法对照：handleLine 的 trim→ended→parse→error+`did you mean`→executeStep→rejected→turnLengths push→prose+`« … »`（空行分隔）序完整；undo 截断+重 fold；**wait ceil 量化**（`(due_at+29)/30*30` 对非负域精确，UI-10 注释）；visible 的 named 集合（ref/into 的 name 形）、`Option` 相等 ≡ TS `===` 的 undefined==undefined 真值、placed 容器三段全对应；inspect 的 Q11 bare-inspect 冻结 + zone 的 JS truthiness（`is_truthy` + `to_js_string`）；suggestions 的 busy 集合、rank、lights（REFUSE 墙）、phrase 的 role 顺序与 `'to'→'with'` 前缀共用、pools Map=IndexMap 插入序、base/alt 绑定（cap 的 `min(max,None→2)`）、round-robin 64 帽、dedupe+可解析过滤+limit——逐行对应。**§3.21 比较器**：池排序尾 `cmp_ascii_ci`（js.rs 单点、禁 ICU 注释、W4 快照义务写明）。
- **lenient.rs**：4 处 `\s` 全走 `JS_WS_CLASS`（Q8 模块头注释 + 附录 A.3 逐条行号）；passthrough `^\d+:\d+{JS_WS}@`/`->` 逃生舱（原始 parser message 透传，UI-11）；lone-clock 纯推进；verb 单/双 token 搜索序；**unknown 收集序**（actors→objects→to→with→at/about 收尾）逐点保持；`I don't know`/`no verb found`/`bad clock` 文案逐字；with→to/actors 路由（fits 的 byName 池头、`includes` 身份等价→结构相等的论证成立：fits 是值决定的，两removed 集合重合，注释在位）；MAX_NAME_WORDS=4 常量化（UI-07）；suggest/suggestVerb/levenshtein 逐行（`split(/[\s_]/)` 的 UPPER_SNAKE 论证成立）。
- **render.rs**：journal/endingBanner/endingNotice（**UI-02 UI 版全文**）/pretty/flattenAspects（Q13 单层展开 ≡ TS 递归）/proseLines（wait 行、模板行、PROGRESS 缩进、REFUSE 直出、ENDING 尾行 `def?.title ?? id ?? 'the end'` 三层）/prose/interpolate×2（replace /g ≡ str::replace，次序同字面量）/refName/refNames/joinAnd；`card()` 未移植（死代码）。

**tests/common ↔ test/ 四件**：

- **snap.rs**：root 键序 `ents,rels,byName,now,rng,decks,fired,ended,windows` = TS 字面量序；**5 处排序**（ents/rels/byName/decks/windows）全部 `cmp_utf16`（≡ `a<b?-1:1` 的 UTF-16 码元序——PR2-11 勘误逐处核对）；fired 默认串序；windows 排序 + `{...w}` 浅拷贝的"拷贝在 stringify 后不可观察、结构体/refs IndexMap 镜像插入序"论证成立；entity/relation JSON 字段序 = TS 声明序；aspects/attrs 走 ordinary_own_property_keys。
- **settle.rs**：唯一 run 助手 = parallel 裸时钟分支 + settleStep 系 idGen 通道（两次 execute_step 共享同一 `&mut Option<&mut dyn FnMut>`  reborrow）+ 可注入字典（TEST-01 修正蓝本）；`Math.max(...ends)` 逐点；**§9.19 整轮回滚**（见 (g)②）；act/done/refusal 三探针（refusal 的 `s ? s.v : ''` → `unwrap_or_default`）。
- **seed.rs**：seed_content 的 12 动词/slots/durations/recipes/whens/templates、dusk_falls(slot 32)、6 原型（含 CLUE derive minus 2）、seed_state 全实体/aspect/relation——逐字段对照无差。
- **explorer_lib.rs**：ExplorerWorld 每调用构建（见 (g)④）；candidates_for 的 sampled/full 两空间逐分支（subsets 的 max:None=Infinity、full 边界 `>max && max>0` 的 Option 映射、min-fill、wrong-kind/dupe/shared/product≤3000/to×about 的枚举序、multi-clause/`then wait`/`wait then`/成对、clock rewind）；WIN_PATH 9 行逐字；win_path_states 的 id 记账与 `win path broke at` 错误；sampled_walk_states 的 `Lcg::from_raw_state(0x4e494748)`（**无 XOR**，js.rs 位精确向量钉住）、0.2 采样门、`floor(r()*n)%n`、ending-undo（turn_lengths 种子 [0]）、`executed % 20` 收割。

## (b) 迭代序/所有权审计 — PASS

`grep -n "HashMap\|HashSet\|BTreeMap\|Rc<\|RefCell\|Cell<"` 于 engine/session/common：

- **HashSet 7 处全部成员查询**（`contains/insert`，无迭代）：validate_slots `placed`、make_begin_record `seen`（≈ TS `new Set` 同型）；game.rs 的 `named`/`busy_verbs`/`busy_cards`/dedupe `seen`（TS Set 同）。零 HashMap/BTreeMap/Rc/RefCell/Cell。
- **§2.6 逐条**：bindings 全程存 entity id（validate_slots/fold_bindings/Phase-3 rebound/suggestions）；`expand` 返回 ids、各 caller `entities.get/get_mut` 现读现写（DESTROY 的 stamp-then-release 以 `(name,id)` 拆双借，行为同 TS 别名——`release_name_entry` 重构为值恒等）；IndexMap 用于 windows/relations/pools/byName 消费点，`shift_remove` 精确对齐 TS `Map.delete` 的保序语义；IndexMap.insert 对已存在键保位（Object.assign 复刻点有注释）。
- `clone()` 126 处（engine.rs）：逐一抽核均为 String/Vec/Ref/Outcome/OpenWindow 值克隆；`due.first().map(clone)` ≡ TS 传引用（w 从不被原位突变，仅 map 删除）；`clone_state` 边界与 TS cloneState 调用点一一对应。无 Entity 克隆进任何绑定结构。
- 引擎纪律：实现面零 `unwrap`/`expect`（`#[cfg(test)]` 冒烟除外，gate 测试仍绿）；`unreachable!`/`debug_assert!` 各处均带 TS 依据行（规则 11）：`checked_step`（PR2-04 饱和方向）、`next_slot_after` 负 cursor、suggestions 的活 id 前置论证、`Expiring.at` 快照过滤、windows 非空等。

## (c) QUIRK 在位性 — PASS

| 条目 | 位置 | 判定 |
|---|---|---|
| Q6（往返失败静默不开窗） | engine.rs:642（构造点）+ :3270（冒烟测试） | ✓ 指认准确 |
| Q7（被拒 REFUSE 的 NARRATE 带数字） | engine.rs:793（rewrite_roll_prose 文档）+ T3.7A 备注的 id 序列间接钉 | ✓ |
| Q8（JS `\s` 集） | lenient.rs:8（A.3 四处全走 JS_WS_CLASS；grep 无裸 `\s` 漏网） | ✓ |
| Q11/UI-06（bare inspect 首卡） | game.rs:311 | ✓ |
| UI-02（双文案分立） | game.rs 模块头 + render.rs:35（UI 版 `…(flavour).`）与 engine.rs:286（引擎版短文）两条构造点分离；session_smoke `game_over_speaks_both_texts_and_undo_reopens` + endings.rs 双向直锚 | ✓ 未合并 |
| UI-10（wait 量化） | game.rs:420（ceil 手写除法对非负域精确 + 注释） | ✓ |
| ENG-18（死支/活支） | engine.rs:2148（首析取支死、`toMin==now && !hasDue` 活）、:2302（idGen 回退不移植） | ✓ |
| UI-05/Q1（carried） | Log.journal()/render.card() 未移植；Q1 不涉本波 | ✓ |

## (d) messages.md 对账 — PASS（附 should-fix 1/2）

- **toThrow 计数实测**（grep TS 原文，net = toThrow − not.toThrow）：engine 8 / parallel 0 / **functions 24**（25−1）；decks 28 / dice 28 / stackable 9 / conditionals 26 = **91** ✓；endings 13 / nightjar 0 / nightjar-explorer 0（2 原始 toThrow 均 not.toThrow）/ script-play 剩余 0（7 条已计入 W2 编译器子集）= **13** ✓。
- **T3.7A 对账写作"8+0+22=30"**：functions 实为 24 net——差额 2 条是 ：531/:535（`a function call/parameter is authored form only`），命中 W1 已登记的行 73/14 模板，而这两行至今标 built/"T3.7 对账"未升 pinned（Rust 锚已在 rust/tests/functions.rs:1307/:1328 逐字落位）。口径本身可辩护（"本波升 pinned 的断言数"），但与规则 13(d)"对账数 = toThrow 数"的字面口径差 2，且行 14/73 的 W3 对账义务实际已完成而未销账 → **should-fix-1**。
- **行 142 的 TS 源行写 `engine.ts:1785`**——文件仅 1298 行，实为 **:819**；模板列 `${deck_id}` 应为 TS 的 `${deckId}`（正文文本本身逐字正确）→ **should-fix-2**。
- **抽查 8 条 pinned 行逐字对照 TS 构造点**：行 6 `the story has ended — ${stateIn.ended.title}`（:155）、行 129/130/131（validateSlots 三条，:89/:95/:102）、行 133 `is not a content verb`（:202）、行 137 `SET: cannot add to non-numeric`（:633）、行 12 DRAWN 漂移全文（:864-866 双行拼接后逐字）、行 10 `duplicate name '${e.name}' among alive entities`（types.ts:153）、行 101 `endings expect  ID: "Title" (flavour)  — …`（script.ts:376，双空格两处）、行 110 三条（content.ts:209-212）——**全部逐字符一致**。
- 五包锚号钉准抽核：行 8/9/10（parallel 直锚）、行 78/101/110/117（endings 直锚与 TS 行号一一对应）、行 125/126/127（T3.7C 直锚落位）均核实。
- **carried should-fix（W1→W2→本波第三次登记）**：messages.md:115 "**11** 行" vs :118 "**10** 行" 自相矛盾仍未修订 → 并入 should-fix-1 一并处理。

## (e) gate + 亲测 — PASS

- `cargo test -j 4`（reviewer 亲跑）：**537 passed / 0 failed**，与声明一致；分文件计数 engine 74 / engine_ordering 17 / parallel 13 / functions 27 / decks 46 / dice 49 / stackable 21 / conditionals 36 / endings 25 / nightjar 21 / nightjar-explorer 4 / script_play 29 / session_smoke 7 / common_smoke 9（+ store 4 + engine 冒烟 9 等 lib 内嵌），与 commit 口径逐一对上。
- `cargo clippy -j 4 --all-targets`（亲跑）：**0 warning / 0 error**。
- gate：`tests/gate.rs` 在 W3 范围 **零改动**（git diff 为空）；两项门（rand 禁入 core/engine、unwrap/expect 排除规则 `self.expect(` 唯一豁免）亲跑通过；W3c 另增 dice/decks 侧 `grep_gate_no_rand_anywhere_in_src`（扫 rust/src 全树）与 TEST-12 的 TS gate 等价。

## (f) TS 侧零改动 — PASS

`git diff --stat rust-port-w2..HEAD -- src/ test/ scripts/` → **空**；`git status --short -- src/ test/ scripts/` → 0 项。W3 触面 = rust/{src/engine,src/session,src/core/*,src/js.rs,src/lib.rs,tests/*} + docs/rust-port/messages.md，无越界。core/* 触面逐一复核为行为中性：predicate/query/ref/roles/sentence/types 均为 **serde derive 补全**（注释声明且核实：Pred/ValExpr/Query/IfArm/arms 等 authored-only 类型从不上 JSONL 线；Ref 的 `tag="form"`+kind-skip、Role 小写串、AttrValue untagged、Sign ±1、Outcome 的 op/args 前置+extras skip 全部对齐 TS 线形——engine.rs 冒烟的字节串测试承重，见 (g)①）；types.rs 的 `release_name_entry` 为纯重构；js.rs 新增 `cmp_ascii_ci`（§3.21 授权）；lib.rs 增 module 声明。

## (g) 有意分歧逐条核对

| # | 分歧声明 | 核对结论 |
|---|---------|---------|
| ① | serde 字段序"手写期望串测试"真实承重 | **成立**。engine.rs 冒烟 `records_serialize_in_the_ts_field_order` 把一条真实 BEGIN 行的完整 JSON 串（含 tGame/tReal rename、null 字段、actors 内部 tag、recipes、sentence）逐字节断言并 parse-back 无损；dice.rs `two_fresh_same_seed_runs…` 以 save_log 落盘字节读回比对（dice.test.ts:917-933 判据）；store.rs `save_then_load_resaves_byte_identical` 三方字节恒等。三条互证，字段序漂移即红 |
| ② | settle 回滚腿 clone 前置的行为恒等 | **成立**。TS executeStep 从不原位突变入参（draft 内部克隆、fireWindow scratch、refused 返 stateIn），settle_step 的前置 clone 仅为在 Rust 值语义下交还"未被触碰的原状态"——每个分支值恒等（refused/ rejected/Err 三路核对）。sampled_walk 的 `state = r.state` 提前到拒绝检查之前，依赖同一恒等（拒绝时 r.state 即回滚克隆），注释在位 |
| ③ | suggestions `cmp_ascii_ci` 近似面在 W4 前的钉板 | **按计划成立（note-1）**。现有钉：script-play:421 `s[0]==='read THE MANIFEST'`（session_smoke + script_play 双处，跨比较器敏感）+ endings 空 suggestions + 大量序不敏感锚；全列表序的钉按 §3.21 归 W4 golden 快照（义务在 js.rs 注释与计划双处登记）。W4 前若比较器漂移，仅 s[0] 一点可探测——这是计划明文接受的近似面，非缺口 |
| ④ | explorer_lib 单例→按调用构建的值恒等 | **成立**。compile_script 为纯函数（同文本+同 dict → 同 content/state/meta）；grep 证实 TS 消费方（nightjar-explorer.test 等）从不在共享 dict 上 registerVerb、从不原位改 content/base（decks/stackable 的 registerVerb 在各自模块 dict 上，Rust 以每测新建对齐 TEST-10）。每调用 ExplorerWorld 与 TS 模块单例值恒等；显式传 dict 的签名差异已注释 |
| ⑤ | nightjar `base_of` 重编译代位 | **成立**。TS `game['base']` 为构造期存入、此后从不被突变（handleLine/undo 均克隆），`flyer_base` 重跑纯 `compile_script` 得到值相同的状态——注释在位，等价成立 |
| ⑥ | store 原子写 + loadLog 行号 | **声明成立**（ENG-20 授权、无 TS 测试钉旧文案；模块头论证 + `load_log_reports_the_offending_line` 钉新文案） |
| ⑦ | `checked_step` 饱和（rng/游标/offset/span/ADVANCE） | §3.3/PR2-04 已登记，debug_assert 在位，无 >2^53 可达输入（W1 已核口径沿用） |
| ⑧ | Step Clone/Debug、`clock?: Clock|null → Option`、JSON pair 的 `bad aspects/attrs pair` 内部错误文本 | 前两者为建模选择，注释声明且不可观察；后者是 TS `JSON.parse` SyntaxError（引擎自产 pair 恒为合法 JSON，不可达）的 Rust 侧替代文本，属 internal 族——建议随 W6 收口在 messages.md internal 行补记（note-4） |

## 测试移植保真抽查 — PASS

1. **tests/engine.rs**（3026 行，74 test）：rig（setup/runWith 的 `x${firstRecordId}_${++n}` idGen、step_of 的 `?? {day:0,halfhour:0}`）与 TS :24-36 逐行对应。逐断言抽查：`cardinality_violations…`/`slot_discipline…`×2（输入句、正则、toHaveLength(0) 三件套）；`spawn_cannot_mint_a_duplicate_name…`（双窗拒绝 + byName len 1；TS 的 `expect(r2.state).not.toBe(r.state)` 引用不等断言在值语义下正确省略）；`records_carry_sorted_actors…`（['ANA','BERTHA'] 序 + sentence 前缀）。
2. **tests/nightjar.rs**（21 test）：P0 锚点逐值落位——`[[5,2],[8,4],[14,7]]`（:153）、`['SCANDAL','RUMOUR']`（:163）、PORTENT×2/VOID×1（:347-348）+ SUSPICION 池/rng=7/border_stop/the_furnace_takes_it/inquest.win，注释显式标注 W4 复用；`full_win_path…` 全断言对应。
3. **tests/script_play.rs**（29 test）：`suggestions_are_concrete_executable_actions`（可解析性循环 + s[0] 逐字）与 `the_log_replays…`（byName 键集比较**显式用 cmp_utf16 复刻 JS `.sort()`**——比裸 Rust 排序更精确）；auto_settle_line ≡ TS autoSettle 适配器；`game['base']` 走 (g)⑤ 代位。
4. **tests/nightjar_explorer.rs**（4 test）：checkStateShape/checkRecords/checkStep 与 TS :53-137 逐断言对应（`expect(r.state).toBe(pre)` 以 snap 相等代位并注释；sweep 覆盖地板全过）；`not.toThrow` 两处转 `prose_lines` Ok 断言。
5. **engine_ordering.rs 的 17 个排序测试**：`#[test]` 计 17；模块头声明 PR1-01 义务；**27 处 `// HAND:` 标记分布覆盖全部 17 测**，四个 anchor 场景（anchor-due-tie :313 / anchor-expiry-insertion-order :381 / anchor-on-expire-first-match :405 / anchor-refuse-id-recycle :520）显式点名——W4 替换义务未丢。每测均带 fold(snap) 回放等式。

## 问题分级

**must-fix：0 条。**

（初审中排查过的一个疑点，定性为 should-fix-3 而非 must-fix：execute_step **Phase 2** 把 `validate_slots` 的 Err 全量折叠为 `rejected("<verb>: …")`（engine.rs:380-383），而 TS 中 validateSlots 内 `resolveRef` 的 param-ref throw（engine.ts:118）会**未捕获地穿出** executeStep——busyReason（`?` 直传）与 selectRecipe（`?` 直传）都按异常通道映射，唯独此处不一致，且 engine.rs:207-209 自己的注释承诺"the Rust `Err` does too (never a rejection)"。**可达性为零**：parse 在 `kind:` 后拒绝 param、lenient/record 路径只产 name/id ref，全仓（两侧）无任何测试或调用点构造 param-ref clause——故不构成可观察行为分歧，但与文件自述模型相悖，建议改 `?` 直传或补一行通道分歧注释。）

**should-fix：3 条（均不阻塞收口）**

1. messages.md T3.7A 对账口径：functions 记 22 条 vs 文件实为 24 条 net toThrow（:531/:535 命中行 73/14 模板）；行 14/73 的"W3 对账"义务实际已由 rust/tests/functions.rs:1307/:1328 完成而未升 pinned。连同三波 carried 的 :115(11 行)/:118(10 行) 自相矛盾，一次 doc commit 清账。
2. messages.md 行 142 的 TS 源行 `engine.ts:1785` 不存在（文件 1298 行），实为 **:819**；模板列 `${deck_id}` 应为 `${deckId}`。
3. 上文 Phase-2 内部错误通道对齐（`validate_slots` 的 param-ref Err 应与 busy_reason/select_recipe 一致直传，或注释声明通道分歧）。

**note：4 条**

1. （g)③：cmp_ascii_ci 的近似序面在 W4 前仅 s[0] 一点可探测——按 §3.21 这是计划接受的过渡态；W4 golden 的 suggestions 全列表快照落地后即闭合。
2. engine.rs 冒烟 `all_seventeen_ops_smoke` 用 `saturating_sub` 注释 PR2-04，与实现一致；derive minus 的 f64→i64 饱和方向已登记，无需动作。
3. `Expiring.name` 死字段与 TS 同构保留（`#[allow(dead_code)]`）——镜像保形的正确选择，W4 无需清理。
4. `aspects_from_json`/`attrs_from_json` 的 `bad aspects/attrs pair: …` 为无 TS 对应的新内部文本（TS 是 JSON.parse SyntaxError、不可达）；建议随 W6 internal 族收口登记。

**结论：PASS-with-notes——可打 `rust-port-w3` tag。** should-fix 1/2 建议随 W4 首个 doc commit 清账，should-fix-3 可由任一 engine 触面作者顺手处理；全部不阻塞。
