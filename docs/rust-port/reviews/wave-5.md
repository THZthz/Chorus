# W5 波次门评审记录（§7 规则 13，PR3-04）

- 评审对象：`rust-port-w4..HEAD`（11 commit：walk_outcomes 共享遍历器 → census+world → fixpoint → paths+render → clippy 整形 → reach 测试 32/32+S1–S14+REACH-03/07+Q17 → aspects_to_json pub(crate) → reach_scale_lib → goldens 32 条 stdout + golden.rs replay → reach-scale bin/test → reach-fuzz bin/test → reach/lint bin + cli 接线测试）
- 评审人：wave integrator 指派的独立 reviewer（非本波作者）
- 评审日期：2026-10-14（初审，判 FAIL）/ 2026-10-15（4fa6e18 复核）
- 证据方式：(a) reach 属分析器核心，按 core 级**全量逐段对照**：rust/src/reach/{census,world,fixpoint,paths,render}.rs（3931 行全文）↔ src/reach/reach.ts（1968 行全文），另全量对照 walk_outcomes ↔ content.ts:303-311、四 bin ↔ tools/{reach-cli,reach-scale-cli,reach-fuzz-cli,lint-cli}.ts、tests/common/reach_scale_lib.rs ↔ test/reach-scale-lib.ts、run_fuzz_seed ↔ reach-fuzz-lib.ts:457-507；(b)–(d)(f)(g) 逐项 grep/diff 对账；(e) 按命令纪律**不跑全量 cargo test**（integrator 已亲跑 714 绿），亲跑 `cargo clippy -j 4 --all-targets`（touch 强制重编译，0 warning）+ `--test reach`（34 绿）+ `--test cli`（10 绿，覆盖 4 bin 真子进程）+ `--test reach_fuzz` 种子 1..10（10 绿）+ `--test golden reach_cli`（replay 绿）+ `--test reach_scale`（7 绿）

## 总评：**PASS-with-notes（最终结论，含 4fa6e18 复核）**

初审全量对照干净，但发现 1 条 must-fix（判 FAIL）：`--demand ''` 的 **JS falsy 语义在三处门上丢失**（CLI usage 门 / analyzeCard 的 demandQ 门 / 报告头）。修复 commit 4fa6e18 复审通过（证据见下节）：**cli 12/12、reach 35/35、golden 9/9 绿，clippy 零警告**（reviewer 亲跑，单测试文件、无 vitest/tsx 并发），TS 侧零改动，无新引入分歧。should-fix 4 条 + note 3 条，均不阻塞。可打 `rust-port-w5` tag。

---

## must-fix 复核（2026-10-15，commit 4fa6e18）

**must-fix-1（空字符串 demand 的 JS falsy 门）— 已消除。** 修复仅触三处门 + 测试（+108/−7）：

- **bin/reach.rs:40-41**：argv 归一 `demand.filter(|d| !d.is_empty())`——`Some("")` 在解析点变 None，`!demand`（reach-cli.ts:30 usage 门）与报告头经同一归一点恢复 falsy 语义；`reach <script> --demand ''` 重新 **exit 2 + 两行 usage**（无卡名形），`--all --demand ''` 渲染**名字报告**（同一归一点覆盖两分支，注释在位）。
- **paths.rs:640-646**：`Some(d) if !d.is_empty() => Some(parse_query(d)?), _ => None` ≡ `opts.demand ? parseQuery(...) : null`（reach.ts:1310）——`parse_query("")` 的空 Query 不再进入 demand 模式。**过滤宽度逐点核对**：仅 `''` 翻转，纯空白串（`" "`）两侧仍 truthy → 两侧同样进 demand 模式（TS `parseQuery(' ')` 同得空 query、matchQuery 空表全真）——无过度/不足过滤。
- **render.rs:152-156**：`Some(d) if !d.is_empty() => matching, _ => card` ≡ `demand ? \`matching '${demand}'\` : name`（reach-cli.ts:56）——空头行恢复卡名。
- **对照 TS 原文再核**：demand 的全部消费点即此三处（chain 行内 `opts.demand` 仅在 demandQ truthy 分支内可达，两侧同为非空串）；rest 循环仍跳过 `--demand ''` 的值 argv（两侧 `i+=2`/`i++`+for++ 同型）；trailing `--demand`（值缺失）路径未动（Q15 pin 保持）。
- **回归测试三层覆盖**：cli.rs `reach_empty_string_demand_is_falsy_usage_exits_2`（exit 2 + usage 两行逐字 + stdout 空）、`reach_empty_string_demand_degrades_to_name_mode`（有卡名时输出 ≡ 无 demand 基线运行，exit 0）；reach.rs `an_empty_string_demand_is_falsy_renders_exactly_like_no_demand`（API 层 `Some("")` 渲染 ≡ `demand: None` 逐字节 + 头行卡名断言）——TS 期望的 usage/名字模式双形与 analyzer 门均有钉。
- **无新分歧**：diff 全过目，仅三处门 + 测试；reviewer 亲跑 `cargo clippy -j 4 --all-targets`（touch 三文件强制重编译）0 warning、`--test cli` 12/12、`--test reach` 35/35、`--test golden` 9/9（32 条 stdout replay 含在内）；`git status --short -- src/ test/ scripts/ examples/` 仍 0 项。闭环。

---

## (a) 逐段对照（reach 全量）— PASS 除 must-fix-1 一处

**census.rs ↔ reach.ts:127-454（全量）**

- **walk_sites ↔ walkSites（:205-227）+ walk_outcomes ↔ forEachAuthoredOutcome（content.ts:303-311）**：共享遍历器签名 `(ctx, state, visit, arm_ctx, else_ctx)` 的三回调恰承载 TS 的 `(path, segs, fallback)` 线程与 flags 记账；`arm_ctx`（新 seg 分配）在**每臂下潜前**调用，与 TS 递归实参求值序逐点一致（arm1 分配→下潜 arm1→arm2 分配→…→ELSE 分配），seg 编号交错序 call-for-call 相同；REFUSE/throwing 标 INNERMOST seg（`frame.segs.last()`）≡ TS 闭包捕获的 `f`；`o.op==='IF'` 与 REFUSE/throwing 的 else-if 链因 op 单值而与顺序 if 等价；DRAW-into 判定 `Arg::Into` ≡ `a.t==='into'`。TS 在 walkSites **入口**为每 seg 建 flags 条目（含空臂），Rust 在走后 `0..=seg_counter` 补齐——seg 恰稠密 0..=segCounter，条目集合同一（注释在位）。空 fallback：TS `[]` truthy 仍分配 seg，Rust `Some(vec![])` 同。死代码 startSlot/cyclic（外层）未移植 ✓。
- **collect_sites ↔ collectSites**：verbs→recipes 声明序、triggers 随后；shadowed = 前缀 when-less 扫描；`shadowRisk = i>0`；per-list 新 flags/opCounter（opCounter 对**全部 op** 计数，非仅站点——与 TS 一致）；kills 记 (seg,pos,name,throws)；SET-site 行 11 字段全量（三处重复类型收敛为 `SetSite`，REACH-02 落实）；aspects pair 仅取 `v: Str`；spawn/replace/draw 三形的字段默认值逐字段对照（draw `runtime_gated:false`、仅 spawn 带 derives）✓；unknown-deck draw 丢弃（:327）✓；addMint 均匀性/kill-pair/dupRisk/listGate/listStack/co-mint cap（0/1 冻结公式含 order[0] 自撞）逐行同构；postPass 的 killedInList（`segs.includes && (seg≠ || pos>)`）、targetKilledInList（`pos ≠`）、SET-refused 注记跨 list 按 owner 过滤 ✓；live/refused 入 arena 的追加序 = TS push 序 ✓。
- **TrigIndex**：`new Map(triggers.map(...))` 的重复 id later-wins 保首位 ≡ IndexMap::insert，注释在位。
- **Q17 分支**：`site.verb.is_some() && init_names.is_empty()`（paths.rs:608，QUIRK 注释在位）✓，并有专测 `q17_zero_entity_scripts_keep_the_site_reason_branch_reachable`（reach_adversarial.rs:911）。

**world.rs ↔ reach.ts:456-978（全量）**

- 静态谓词演算：exprStatic/predStatic 在 census.rs（collectSites 消费），evalStatic/pathOpen/writablePaths 在 world.rs——词表与 TS 逐一对应（and/or/not/cmp 六算符/exists/existsQuery）；两处 `internal:` throw 保留为 Err（注释 :505/:521）。
- **initNames/initCount**：State.entities 序、后写胜（IndexMap insert 保位）+ 注释指认中局 State 必现（REACH-03 修正表述）✓；push 序 = initNames 序。
- **risky block（:712-953）逐子规则**：byList=live∪refused 序；mintSetOf 仅收 collisionNames；risky 初值行；trigBySlot 声明序 + 同槽 (i,j) 对 + 整 pass 连坐（`risky.get(o)==Some(&true)` ≡ `===true` 的 undefined 区分）；trigFlat `sort_by_key` 稳定序 ≡ ES 稳定 sort（且等 c 跳过，REACH-05 双保险）；otherMints/gated-branch（`gated.includes` ≡ Vec contains、跨 list 排除式）；listBlocked；throwFrozen（selfKilled/crossKilled/deterministic 的 `fc<myFirst || (fc== && decl<)`）；**firstFreeze Set 化**（REACH-13：Vec+min，注释在位）与 `freezeAt` min-or-Infinity；boundarySlot find 首条；clean-boundary 豁免（recurring-on-boundary + 无 first-firing-throws）；risky/throwFrozen/listBlocked 三轮 demote + riskyOwners；triggerMintNames hoist——逐行对照无分叉。块内死代码（第二处 startSlot/cyclic）未移植 ✓。
- **flatCache 逐字复刻**：push 更新缓存内容但**不更 ver**、availVer++ 使下次重建——TS 的"O(1) warmth"实为死更新，port 注释明示 "reproduced, not repaired"，`cards_of` 重建读 avail 全量，值序一致 ✓。
- freezeDist 单位、refused note 文案逐字 ✓。`first_cross` 的 `rem_euclid(1440)` ≡ 双 mod + `d===0→1440` 分支在位（REACH-14）✓。`INFINITY=i64::MAX` 比较方向论证成立（距离域 1..=1440）。

**fixpoint.rs ↔ reach.ts:980-1078 + 1575-1911（全量）**

- **不动点（REACH-10 标红）**：`while changed` 每轮 verbs 按 content.verbs 序（`0..len` 索引序）→ sites 按 live 序（clone Vec 保序），`committable.contains_key`/`proven.contains_key` 跳过、proven-once-never-removed——**无 worklist/事件驱动**，模块头与循环点双注释禁令在位 ✓。
- **slotSupport**：slotByRole（非 slots）+ min>0 过滤（seed 262 注释）；空 proof = `Some(vec![])`（JS 空数组 truthy，注释在位）；cap=4 与 `min(count===MANY?cap:count,cap)` 逐字（§3.11）；cand 展开序 = avail push 序；assign 首适配递归（used/chosen 语义一致）。
- **siteProven 17 参 → &WorldFacts**（REACH-02）✓。卫列序逐条对照：fallback/killedInList/dupRisk/selfGated/throwOps−selfSafe/shadowRisk/pathOpen/gatedNames∪destroyedBy/gate-timing（deadAtMyFiring 的 recipe-false/无主-true/expiry-false/同槽 decl 序/跨槽 crossing 序）/filler-doomed/trigger-vs-recipe（duration<firstCross 剪枝）/frozen-completion（duration>0）/recipe（when 静态真 + committable + slotProofLines 去重前 3）/slot 触发行文案/expiry（desc、selfArm、ownerArmable/expirableCard 递归 + visiting 破环 + memo 仅顶层 + witness 八连卫：refused/shadowed/dirtyOwners/selfArm/pathNonStatic/frozen×(duration>0‖ownerSlot crossing≥freezeDist)/committable/whenPred/target≡name‖role-slot-fits/ownerArmable）——全谓词逐字对上；`committable.has(v)` 的身份键 → `verb_idx_of` 索引键等价 ✓；expiry note 的 push 位置（证明成功前）与 TS 一致。
- **addProduct/recordMintSource**：served 切片 cap-aware（150846）、counts→find/position 同指（注释论证 find+indexOf 一致）、infinite=reset‖defaultcard、盲名三处（draw 卡/默认卡/derive-mint）与 `\u0000derived` 键、how 文案逐字节（含 `← deck D… order[i]` 与 kind-override 后缀）✓。
- **selfGated**：gates∪listGateMints 相交、once-owner 的 listStackMints 分支、mints-driven 的 `site.verb.slots`（**裸 slots 含被遮蔽副本**——与 TS 同一读取面）✓。

**paths.rs + render.rs ↔ reach.ts:1080-1505 + 1913-1968（全量）**

- universe：flatten(cap32) → prototypes → decks(order+defaultcard) → live spawn 的 override 合并（`{...proto, ...JSON.parse}` 的键序 ≡ IndexMap insert 保位，注释在位）；JSON.parse 失败 → opaqueSpawnAspects（**语义缝隙见 should-fix-1**）。
- deadVerbs：slotByRole + min>0 + `!canEverMatch`；**canEverMemo 去除**——can_ever_match 是终态字段的纯函数，值等论证成立（⑤）。
- deckCap：proven 序遍历、frozen 下 recipe-unbounded/trigger `firstCross<freezeDist?1:0`、max 归并 ✓。
- provenSourceFor/provenSources/initialPath/pathFor：首命中 chain、derive+objectAlways 过滤、servedByCap 查询期过滤（REACH-11 修正表述）、deck 导出 `position ⊕ defaultcard` + `cap!==MANY` 门、needed 去重、seen 链式拷贝、MAX_PATH_DEPTH=12 ✓。
- analyzeCard：demand 分支（init 扫描→proven 描述符扫描含盲名/servedByCap→pump/derive/witness notes→candidate 抬升）、name 分支（proven 扫描→candidateSite 复合谓词含 frozen-cap 例外（81401）→derivedOnly/anyDerived→derivedPattern）、usability（descriptor 三级 fallback、裸 slots、文案两变体）、verdict 四态、candidates 平列——逐行对照，除 must-fix-1 的空串门外无分叉。
- **siteChain/siteReason/servedByCap/deckServes/slotProofLines**：文案逐字节（`· ` 前缀、`finite draw capacity: N once-trigger draw(s) — only order[0..N-1]`、reset 前置于 defaultcard（74985）、`<non-static rel/every/any>` 三 tag）；siteChain 的死参 content 未移植（REACH-08）✓。
- **render_path/render_report ↔ reach-cli.ts:38-67**：4 空格缩进步、`path i/n:`、`verdict: X  (existence: y)` 双空格、六行段序、尾空行——与 golden-dump.ts 的逐字副本互证，replay 逐字节通过。

**__must-fix-1（唯一行为分叉）：空字符串 demand 的 JS falsy 门丢失。** TS 三处以 truthy 消费 demand：reach-cli.ts:30 `!demand`（usage 门）、reach.ts:1310 `opts.demand ? parseQuery(...) : null`、reach-cli.ts:56 `demand ? \`matching '${demand}'\` : name`。Rust 侧均为 `Option` 的 Some 检查：bin:60 `demand.is_some()`、paths.rs:640 `Some(d) => Some(parse_query(d)?)`（`parse_query("")` 返回**空 Query 不报错**，进入 demand 模式）、render.rs:151 `Some(d) => matching`。可观察后果：`reach <script> --demand ''` 在 TS 下 **exit 2 + usage 两行**，Rust 下**正常渲染 demand 报告**（空 query 匹配一切，首个实体 proven）；带卡名时 TS 走**名字模式**（头行打卡名），Rust 走 demand 模式（头行 `matching ''`）。`--all --demand ''` 同理（TS 打全部名字报告，Rust 打 demand 报告）。修复面三处（bin 门 + analyze_card 的 `.filter(|d| !d.is_empty())` + render_report 头），或单一 helper 贯通；补一条 `--demand ''` 回归测试。

**walk_outcomes 接线复查**：content.rs 的改造是把 `for_each_authored_outcome` 变为 `walk_outcomes(())` 的退化调用——守卫消费点（lint_dice/check_ending_refs/for_each_if_condition）遍历序逐字节不变；engine.rs 仅 `aspects_to_json` 可见性放宽（单源 JSON.stringify(aspects) 格式，PR2-05 键序）。两 diff 全过目，行为中性。

## (b) 迭代序/所有权审计 — PASS

- `grep -n "HashMap\|HashSet\|BTreeMap\|Rc<\|RefCell\|Cell<"` 于 rust/src/reach/*.rs + 四 bin + reach_scale_lib.rs：**零命中**。全部映射/集合为 IndexMap/IndexSet 或 Vec（arena/有序表）。
- **§2.6.5 身份键索引化抽查 3 处**：① `proven: IndexMap<usize, Vec<String>>`——arena 下标键、插入序=证明序，消费点（provenSourceFor/provenSources/analyzeCard 扫描）全靠该序定 chain 首条与 paths 序，类型正确；② `mint_sources: IndexMap<String, Vec<usize>>`——值 Vec 按证明序追加（与 TS push 序一致），消费全为 `all/any`（序不敏感），双保险；③ `risky: IndexMap<String, bool>`——byList 插入序，demote 循环按同序迭代 ≡ TS `for (const [owner, isRisky] of risky)`。committable/fillers 以 verb 下标为键（`verb_idx_of` 单点换算），dead_verbs/deck_cap/destroyed_by 均为成员判断或可交换归并。
- **clone() 106 处**（census 26/fixpoint 26/paths 23/world 31）：逐一核对均为 String/Pred/Entity/RawSite/Vec 值克隆；`site_proven` 入口 clone RawSite 为只读快照（risky 块的 dupRisk 变更发生在不动点之前，经 arena 下标直达）——无 Entity 克隆进任何键控绑定结构（§2.6.2 无 reach 面）。`used` IndexSet 的 `swap_remove`（assign 回溯）仅作成员判断、从不迭代，序不可观察（见 (g)④）。
- unwrap/expect：reach src 零裸用；`unreachable!`/`debug_assert!` 全部带 TS 依据行（规则 11）；reach-scale worker 的两处显式 `panic!` 带 unwrap_or_else 注释（bin 非引擎路径，gate 不扫 bin）。

## (c) QUIRK/冻结注释在位性 — PASS

| 条目 | 位置 | 判定 |
|---|---|---|
| Q17（siteReason `!initNames.size` 可达） | paths.rs:606-610（QUIRK 注释）+ reach_adversarial.rs `q17_zero_entity_scripts…`（zero-entity 脚本 → `verb committability unknown`） | ✓ |
| Q15/CLI-01（--demand 末位消失/usage exit 2/坏 query exit 1） | bin/reach.rs:10-19,33-34,54-56,73-75,98-102 注释 + cli.rs 真子进程测试 | ✓ |
| CLI-05（lint CWD readdir/缺失输入 exit 1 与头注释矛盾冻结） | bin/lint-scripts.rs 头注释 + exit 路径 | ✓ |
| Q13（aspects 单层模型） | census.rs:68-85（parse_aspects_json 模型声明）——但论证缝隙见 should-fix-1 | ✓（带缺口） |
| REACH-10 标红禁令 | mod.rs + fixpoint.rs 模块头 + run_fixpoint 循环点三处注释 | ✓ |
| REACH-13（firstFreeze 实为 Set） | world.rs:726-730 | ✓ |
| REACH-14（firstCross d===0→1440） | world.rs:226-237 | ✓ |
| 哨兵三件（MANY=1e9 i64 / Infinity=i64::MAX / '\u0000derived' 原样字符串保三性质+对抗名角） | census.rs:18-32、world.rs:25-29 | ✓（DERIVED_NAME 弃枚举的论证在位） |
| REACH-08 死代码 | startSlot/cyclic 外层、analyzeCard 未用解构、siteChain content 参——均未移植且 paths.rs:527 注释指认 | ✓ |

## (d) messages.md 对账 — PASS（附 should-fix）

- W5 移植的 4 个 TS 测试文件（reach.test 32 / reach-adversarial / reach-fuzz / reach-scale）`toThrow` 计数 = **0**（grep 实测四文件全 0）——增量行数 0 = 0，规则 13(d) 形式满足。reach 侧仅有的错误构造点是 `internal:` 两条（不可达、非测试钉）与 demand 的 parse_query 错（W1 行已 pinned，CLI exit 1 路径由 cli.rs 钉 exit 码）。
- **should-fix-4**：messages.md 无 W5 节（git log 证实本波零触面）。建议补一节三行对账（"W5：四测试文件 toThrow=0，无新增行；cli.rs 钉 exit 码与 usage 文案"），维持逐波有节的记账惯例。

## (e) gate 与亲测 — PASS

- **未跑全量 cargo test**（integrator 已跑 714 绿，命令纪律）。reviewer 亲跑：`cargo clippy -j 4 --all-targets`（touch src/reach/mod.rs + bin/reach.rs 强制重编译）**0 warning**；`--test reach` **34/34**（= TS 32 + REACH-03 双跑逐字节 + REACH-07 共享 Analyzer ≡ 逐卡 fresh Analyzer）；`--test cli` **10/10**（四 bin 真子进程：usage 两行 exit 2、trailing --demand 静默消失、坏 demand exit 1、--all 名单与 UTF-16 序、lint-scripts exit 0/1）；`--test reach_fuzz` seed_1..10 **10/10**（91 其余过滤）；`--test golden reach_cli` **1/1**（32 条 stdout 逐字节 replay）；`--test reach_scale` **7/7**（含 400×400 census TS 行对行 A/B）。
- tests/gate.rs 本波零改动（diff 空）——grep 门未放松；reach 测试的种子数守卫（`the_fuzz_suite_covers_100_seeds`）防种子清单漂移。

## (f) TS 侧零改动 — PASS（附对"只增不改"的精确口径）

- `git diff --stat rust-port-w4..HEAD -- src/ test/ scripts/ examples/` → **空** ✓。
- tools/golden-dump.ts（+197）：**只增**——family 7（reachCliFamily：nightjar 6 快照 + genScenario seeds 1..20 ±usable 1..5 = 25 + reach-scale-census 400×400 TSV）、`renderPathInto/reportInto` 逐字副本（console.log→line sink）、GoldenSpec 的 kind 联合放宽 + stdout 字段、manifest 写出分支。既有行的**两处非纯增加**均为扩展的必要伴随且对既有面值恒等：`kind` 类型联合加 `'reach-cli'`（类型面）、`totalBytes` reduce 加 `?? 0`（stdout 条目无 jsonlBytes；对 W4 全部条目结果不变）——判定满足"扩展只增不改"的实质（既有 244 条产物字节零变化）。
- Rust 触面 = rust/{src/reach(5), src/bin(4), src/content/content.rs(遍历器重构), src/engine/engine.rs(可见性), src/lib.rs} + tests/{reach*,cli,golden,common/*} + tests/goldens/**（32 stdout + manifest）——无越界。

## (g) 有意分歧逐条核对（W5a 6 + W5b 5）与黄金三元组

**W5a（分析器库）**

| # | 分歧声明 | 核对结论 |
|---|---------|---------|
| 1 | Analyzer<'c> 单例惰性 WorldFacts 替代 worldCache/trigIndexCache 两层 WeakMap（REACH-07） | **成立**。`OnceLock::get_or_init` ≡ worldFor 的"每 (content,initial) 一次"；trigIndex 内联进 WorldFacts（每世界建一次 ≡ WeakMap 每 content 一次）；callers 不变异契约由 `&'c` 借用结构性保证（比 TS 注释更强）。reach.rs bin / reach_scale_lib.census / run_fuzz_seed / golden.rs 四个消费点均持 ONE Analyzer 跨卡——TS worldCache 共享语义的钉住面（REACH-07）逐点对上 |
| 2 | matchMemo 不移植（计划 §4-W5 保守解）+ canEverMemo 去除 | **成立（⑤）**。matchQuery 纯函数，memo 只省时；can_ever_match 是终态字段（universe/set_writes/derive_paths/opaque）上的纯函数，去 memo 值等；`a_shared_analyzer_matches_fresh_per_call_analyzers_reach07` 反向钉住共享≠改变值 |
| 3 | DERIVED_NAME 保留字面 `"\u{0000}derived"` 而非计划 §3.11 提议的枚举 | **成立**。三性质（占 avail/byName 键、槽查询经 kind/aspect 可中、exists/count 名字门不中）恰是 JS 字符串同一性；原样字符串连"作者真用哨兵名"的对抗角都与 TS 同判。偏离计划字面但保行为，论证在 census.rs:24-32 |
| 4 | 身份键索引化（arena usize / verbs 下标） | **成立**（见 (b) 抽查）。proven/mint_sources/risky 三处序语义逐一验证 |
| 5 | site_aspects 只收 Q13 模型成员（文档化分歧） | **大体成立，论证有缝**（见 should-fix-1）：对象内非 i64 值丢弃的"miss==absent"论证对**浮点值**不成立（`aspect:x<=3` 对 TS 的 2.5 读真、对 Rust 的缺失读假），且**顶层非对象合法 JSON**（数组/数字/字符串）在 TS 是"解析成功→展开合并、opaque 不置位"，Rust 是"Err→opaque=true+无 override"——`aspects='3'` 即可让有界阈值从 TS 的 none 翻成 Rust 的 candidate |
| 6 | evalStatic/pathOpen 的 `internal:` throw → `debug_assert!` + 保守 None | **成立**。throw 点恰在 predStatic 守卫之后（两侧词表同一），不可达性成立；TS 的"可捕获"在不可达面上无观察差，规则 11 达规 |

**W5b（CLI/库）**

| # | 分歧声明 | 核对结论 |
|---|---------|---------|
| ① | 线程+channel 替代 spawn-per-core（C 类拓扑分歧，CLI-03） | **成立**。A/B 接口原样：`--worker i k P R` 分片 `idx % k === i`（f64 语义含 NaN 论证在位）、每 worker 全量重建世界、NameStat 七字段**声明序**=JSON 键序（serde derive）、tmpfile 名含 `${i}` 的 NaN 形、shard 序=flatten 序。**`on N workers` 默认值**：TS `cpus().length` 是机器相关值（本就非跨语言稳定可观察量），plan §4-W5 冻结面= worker 语义+NameStat 字段，模块头明示 min(avail,4) 是 §2.4 内存纪律——判**不算行为分叉**，为已登记 C 类分歧。计时行（build+compile/census wall/summary 计时段）本就在冻结面外 |
| ② | stderr 只保 message 行（JS 堆栈不可复现） | **成立**。reach/lint-scripts/reach-fuzz/reach-scale 四 bin 的错误路径均为 `Error: {e}` + 原 exit 码（1），首行文本与 TS uncaught 的首行同形；模块头三处登记 |
| ③ | fuzz bin 分数 steps/states 截断 |**论证不足（should-fix-2）**。`steps as usize`/`states as usize` 向下取整，而 TS 的浮点循环界实际**放行多一轮**（`steps < 2.5` 跑 s=0,1,2 三层；`queue.length >= 2.5` 容 3 个状态）——`--steps 2.5` 两侧深挖深度差一。整数/默认/NaN（js_number 逐字复刻 Number，含 '' → 0、hex、±Infinity）全等；仅分数旗标这一病态入口分叉，且**未见文档登记**（bin 内只登记了 seed 的 f64 处理）。补一行注释即可 |
| ④ | IndexSet swap_remove 的序不可观察 | **成立**。`used` 仅 contains/insert/swap_remove（assign 回溯），全程不迭代——TS `Set.delete` 亦无序概念；chosen/cand/proofs 的序不受影响 |
| ⑤ | canEverMemo 去除 | **成立**（见 W5a-2） |

**其余登记项**：reach bin `report(card!)` 的 undefined 目标以 `""` 代位（demand 门下不可达，名字域无空名）；`--all --demand` 渲染 demand 报告 per name（闭包捕获形状=行为，golden 钉）；reach-scale `as_num` 的 plain-integer 口径（`Number('')=0` vs NaN、hex——文档声明，病理 argv，note-2）；lint-scripts 的 readdir 序（两侧同为 OS readdir 未排序，同机同序）。

**黄金三元组抽查（3 样本）**：manifest 276 条中 reach-cli 族恰 32（25 probes + 6 nightjar + 1 census，与 git stat 的 32 个 stdout 文件数一致性相符）。样本① `reach-cli-nightjar-all`（driver {mode:all, script} ↔ golden.rs reach_cli_stdout "all" 臂：SCRIPT_TEXT 现编译 + reach_cli_all_names（prototypes∪活实体∪deck.order、cmp_utf16 排序）+ render_report 循环 ↔ 553 行 stdout 文件）；样本② `reach-cli-fuzz-007`（{mode:probes, seed:7, usable:false} ↔ "probes" 臂：gen_scenario + ONE Analyzer + 逐 probe name/demand 双形 + 'scenario' 标签 ↔ 63 行文件）；样本③ `reach-scale-census`（{protos:400, recipes:400} ↔ census 臂：build_cs_scale + census + 六列 TSV（ms 不入金）↔ 423 行文件）。三元组一致，且 `--test golden reach_cli` 亲跑逐字节通过——该测试本身就是 32 条的跨语言差分（Rust 重渲染 ≡ TS dump）。

**fuzz 种子清单**：TS reach-fuzz.test.ts `for seed 1..=100`（SEEDS=100，默认界 8×500）↔ reach_fuzz.rs `fuzz_seed!(seed_1..seed_100)` 逐种子宏 + SEEDS 守卫 + `run_fuzz_seed(seed, 8, 500)`——清单与界逐一相等 ✓。测试计数对账：reach 34=32+2（REACH-03/07）、adversarial 16=14（S1–S14）+Q17+compile 守卫、fuzz 101=100+计数守卫、scale 7=7、cli 10（T5.4 接线）。

## 问题分级（最终，含 4fa6e18 复核）

**must-fix：0 条（初审 1 条，4fa6e18 已消除并复核，见 must-fix 复核节）。**

初审 must-fix-1 记录（已消除）：空字符串 demand 的 JS falsy 门丢失——TS 以 truthy 消费 demand 的三处（reach-cli.ts:30 usage 门 / reach.ts:1310 demandQ 门 / reach-cli.ts:56 报告头）在 Rust 均为 `Option::is_some` 型检查，`--demand ''` 在 TS 为 exit 2 + usage（无卡名时）或名字模式报告（有卡名时），Rust 为空-query demand 模式报告（`parse_query("")` 返回空表不报错）或 `matching ''` 头行。

**should-fix：4 条（均不阻塞收口）**

1. **aspects= pair 的非 Q13 JSON 角**（census.rs:73-102,476-488）：(a) 顶层非对象合法 JSON（`aspects='3'` / 数组 / 字符串）在 TS 是解析成功——数字/布尓名 spread 为空键集、opaque **不**置位；Rust 判 Err → opaque=true + 无 override，有界阈值可从 none 翻 candidate。(b) 对象内**浮点**值：TS 参与阈值比较（2.5<=3 真），Rust 丢弃（恒 miss）——现有"非数读 miss"的论证只覆盖字符串/bool。对齐（展开语义 + f64 载荷）或把分歧注释收窄到真实覆盖面，并说明不可达论证（现有内容与 fuzz 语料均只产对象整数 JSON）。
2. **fuzz bin 分数旗标截断未登记**（bin/reach-fuzz.rs:115）：`steps as usize`/`states as usize` 向下取整 vs TS 浮点循环界放行多一（`--steps 2.5` 深挖 3 层 vs 2 层）。补一行分歧注释（或照 TS 保留 f64 界）。
3. **W4 should-fix-1 未随本次工具触面清账**：golden-dump.ts 的 S10 复算门仍缺原测试的 optional 对照块（reach-adversarial.test.ts:789-809 两条分析器断言）。本波正是"下次工具触面"，再次顺延——下波补两行 check 或登记豁免。
4. **messages.md 补 W5 对账节**：三行（四测试文件 toThrow=0 → 零新增行；cli.rs 钉 exit 码/usage 文案；demand parse 错沿用 W1 pinned 行）。

**note：3 条**

1. reach-scale `--cores 0`：TS `Promise.all([])` 正常打印空汇总，Rust `rx.recv()` 立即 Err → exit 1（"Error: a worker failed"）。病理旗标，TS 侧行为亦属巧合；登记即可。
2. reach-scale `as_num` 与 reach-fuzz `js_number` 的残余 Number 缝：`Number('')=0` vs NaN、hex/`--5` 双符号（reach-fuzz 的 is_decimal 允许体内第二个符号，`--steps --5` 得 −5 而 TS 得 NaN）。plain-integer 口径已文档声明，病理 argv。
3. reach_scale_lib 的 `pick_template` 对空表返回 `"undefined"` 字符串（模板字面量化）——REACH-14 抽签对齐的必要怪癖，注释已在位（CI 400 片无 hubs 时两侧同样产出 `SPAWN undefined`）；记录备查。

**结论：PASS-with-notes——可打 `rust-port-w5` tag。** 初审 must-fix-1 已由 4fa6e18 消除并复核（三处 truthy 门恢复 + 三层回归钉）；should-fix 1/2 建议随下一波顺手处理，其余不阻塞。
