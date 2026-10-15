# W6 波次门评审记录（§7 规则 13，PR3-04）

- 评审对象：`rust-port-w5..HEAD`（4 commit：bin/chorus.rs REPL + tests/repl.rs 脚本化验收 + demo-world.txt + tests/demo.rs + W4/W5 should-fix 清账（golden-dump S10 对照块、reach-fuzz 分数旗标登记）+ lib.rs prelude/CORE-03 + gate.rs 定稿 + README 怪癖附录 + messages.md W5/W6 对账）
- 评审人：wave integrator 指派的独立 reviewer（非本波作者）
- 评审日期：2026-10-14
- 证据方式：本波是**新增 UI 薄层 + 收尾**，非 core 复刻——(a) 按"抽样"口径：bin/chorus.rs 的 dispatcher ↔ tui.tsx runPlain（:450–469）+ BUTTONS（:23/:349–366）+ submit（:368–381）+ play.ts Game 的路由语义（handleLine/inspect/wait 全文），tests/repl.rs 胜路行 ↔ test/explorer-lib.ts:235–245 与 test/nightjar.test.ts:38–48 双处 WIN_PATH，tests/demo.rs ↔ examples/demo.ts 全文（idGen 闭包逐算子），demo-world.txt ↔ test/seed.ts 全文；(b)(c)(d)(f)(g) grep/diff 对账 + 亲跑；(e) 按命令纪律不跑全量（integrator 已亲跑 724 绿），亲跑 `cargo clippy -j 4 --all-targets`（touch 五个 W6 文件强制重编译，0 warning）+ `--test repl`（3 绿）+ `--test demo`（2 绿）+ `--test gate`（4 绿）+ `--test golden`（9 绿），另亲跑 `npx tsx tools/golden-dump.ts --out <tmp>`（不与 cargo 并发）重生成 276 条 goldens，`diff -r` 对入库树 **IDENTICAL**，临时目录已删除

## 总评：**PASS-with-notes**

REPL 是计划授权的新代码（plan §1.3 明确 runPlain/ScriptPicker/title 正则以"最小替代品"取代、不属行为冻结面），本波把它压到 210 行且**路由只此一处**；它脚下的语义栈（session/engine）经 git diff 证实**零触碰**。三条关键分歧（裸 wait=按钮、demo-world 三处偏离、demo journal 的 id 映射）逐条验证全部成立且登记充分——其中 demo journal 的"恰好该映射"断言经 golden 的 #-token 全量清点证实是**精确**的（不多不少恰好 10 条改写覆盖全部 id 承载点）。192 条 toThrow 对账经 reviewer 独立 grep 复数完全吻合；276 条 goldens 亲测重生成零差异。W4/W5 的 4 条 should-fix 清账 3 条、1 条如实顺延。should-fix 1 条（README 附录的措辞超宽）+ note 3 条，均不阻塞。可打 `rust-port-w6` tag。

---

## (a) 抽样对照 — PASS

**dispatcher ↔ runPlain + 按钮（bin/chorus.rs:120–164）**：
- 骨架逐点对上 runPlain（tui.tsx:450–469）：banner 两行 `── {title} ──` + `verbs: {verbList.join(' · ')}`（:451–452；verbList 的 `toLowerCase().replace(/_/g,' ')` 在 bin :79 逐字）；EOF 结束会话（:449↔bin :98）；inspect 前缀正则 `/^inspect\b\s*(.*)$/` 在 **trim 后的行**上求值（:457↔bin :121–137）——Rust 的 `strip_prefix("inspect")` + "下一字符非 [A-Za-z0-9_] 即边界"复刻 JS ASCII `\b`，`inspector`/`inspect5` 落空进 handleLine 与 TS 正则失配同判（dispatcher 测试的 `inspectorish note` 行钉住）；inspect 分支不查元词、handleLine 收**未 trim 原行**（Game 内部自 trim，play.ts:57）都与 TS 同形。
- 差异全部是**计划授权的按钮折叠**：runPlain 本身无元词路由（除 inspect 全进 handleLine），TUI 的按钮走 `submit()` 的**数字**分派（:369–371）→ runButton 按 BUTTONS 标签路由（:349–366）。REPL 把七个按钮折成**打字元词**（look/cards/story/journal/undo/wait/quit），plan §4-W6 明文如此（"元命令 undo/wait/story/journal/look/cards/inspect/quit + 其余进 handleLine"），数字分派按计划弃用。**UI-03 路由收敛一处**成立：src/ 下无任何第二路由点（grep 证实），tests/repl.rs 的库重放是测试镜像非第二实现。
- **裸 wait 语义**（分歧登记①，详见 (g)③）：bin :29–33 头注释明示"meta words are the TUI BUTTON semantics, so bare `wait` means game.wait(), NOT the WAIT verb"——与 runPlain 相反、与 TUI 按钮一致；WAIT 动词仍可达（`march wait`/canonical），胜路首行以 canonical 进就是为这条，journal 字节相等把"canonical 步 ≡ lenient wait 步"钉死。
- **UI-04**：标题取 `Game::from_script → meta.title`（bin :61/:75，game.rs:92 的 `?? 'Untitled'` fallback 同 play.ts:51–53）；examples/play.ts:21 的 `^title:` 平行正则未移植 ✓。`« … »` 回显不加打印层——handleLine 返回的 lines 逐字 emit（play.ts:73 在行内 ✓）。非 TTY：`IsTerminal` 只门 `» ` 提示符（bin :88–95），管道 stdout 字节干净（runPlain 的 readline prompt 会混入——bin 头注释登记了这一点，是 PR3-09 可 diff 的前提；新 UI 面，不算分叉）。
- 错误通道：engine/render Err → stderr `Error: {e}` + exit 1（bin :180–187/:199–204）——TS uncaught throw 的 bin 惯例（W5 登记② 同形）；读文件失败/编译失败同形（:54–67）；无参数 exit 2 + usage 两行（新 UI 自有面）。
- **QUIRK(UI-06/Q11)**：裸 `inspect` 传空 rest 串 → `Game::inspect("")` → play.ts:136 `startsWith('')` 恒真 → 第一张可见卡（bin :129 注释 + dispatcher 测试 `inspect` 行 + game.rs:309 三处在位）✓。

**tests/repl.rs 胜路 ↔ TS WIN_PATH 逐行**：WIN_PATH_SESSION 的 8 条动作行（:69–84 的 explore/read/explore/read/talk/seraphine×2/use on）与 explorer-lib.ts:236–244 **及** nightjar.test.ts:40–48 双处 WIN_PATH 逐行逐字一致、序相同；首行 `wait` 以 canonical `0:42 @ MARCH WAIT` 进（nightjar base now=0:42，nextStepClock 不动 → 与 lenient 'wait' 同一步，journal 字节相等实证）。TS fresh() 适配器是"action 后 windows>0 才 auto-settle"，REPL 是每行后显式 `wait`——胜路九行全部成功提交、每步必有窗口，两者产出的记录序列相同，终局 journal 与 goldens/nightjar-main.journal.txt（TS Game handleLine 黄金）**逐字节相等**（亲跑绿）✓。

**tests/demo.rs ↔ examples/demo.ts**：六步逐字一致（:21–28 ↔ demo.ts:16–23）；`compile_content(seed_content(), …)`/`seed_state()`/parseSentence/executeStep({start_min:None, clock, clauses, t_real:None}, {first_record_id, id_gen}) ↔ demo.ts:29–38 逐参同形（Step 无 startMin、tReal undefined 两空缺都对上）；拒步 panic ↔ `throw new Error(r.rejected)`；journal/prose 断言 ↔ demo.ts:41–47（prose 的 `'· ' + prose(rec)` 逐记录形）；fold 复核 ↔ :49–53（entities.size 相等 + 时钟 0:40）；JSONL 字节 = demo.jsonl golden（law 1 @ demo 尺度）。**idGen 复刻（重点）**：demo.ts:33 `() => \`x${id}_${++n}\`` 的 `id` 是**调用时**读的活变量——但 `id += r.records.length` 只在 executeStep 返回后执行，故步内 id 恒定，demo.rs 的 `id_now = id` 步前快照 ≡ 活读；`n` 跨步共享（TS 闭包变量 ↔ Rust `mut n` 捕获）逐算子等价；`AS #x3_1` 指纹专测在位。demo 家族黄金本身是 W4 从 demo.ts 逐字复制的 dump 产出、W4 已审——Rust 若与 demo.ts 记账有任何一处差，journal/prose/JSONL 三重字节断言即红。✓

## (b) 迭代序/所有权审计 — PASS（薄层，预期干净，实测干净）

- `grep -n "HashMap\|HashSet\|BTreeMap\|Rc<\|RefCell\|Cell<"` 于 bin/chorus.rs、tests/repl.rs、tests/demo.rs、src/lib.rs：唯一命中是 lib.rs:12 的**文档注释**（"IndexMap, never std HashMap"——说禁令本身）。零代码命中。
- `clone()`：仅 tests/demo.rs:160 的 String clone（测试面）。REPL/游戏数据面无克隆结构引入（REPL 持 `&mut Game` 调方法，无状态复制）。
- unwrap/expect：bin/chorus.rs **零处**（错误全部走 Result/ExitCode；gate 的 src/ 扫描也不含 bin，但 bin 自律达标）。tests 侧 unwrap/expect 属豁免面。

## (c) README 怪癖附录 — PASS（附 should-fix-1：措辞超宽）

- Q1–Q17 **逐条齐**（README.md 附录 17 行），每行怪癖表述与 plan §1.2 冻结表逐条对读无失真：Q1 双基数、Q2 静默吞字符、Q3 arg/SPAWN 不对称、Q4 role 不校验、Q5 重复声明不一致+大小写碰撞、Q6 静默不开窗、Q7 预演骰点、Q8 `\s` 差异+JS_WS_CLASS 单点、Q9 DUR_RE 交替序、Q10 字面转义+undefined、"明确禁止"节（worklist 化/整数键名拒收/统一基数/收紧校验/合并拼写）五条全列 ✓。
- **指认准确**（抽样到点）：Q11 → game.rs:309 + bin/chorus.rs:129 ✓；Q13 → query.rs:337/411 ✓；Q9/Q10 → lexer.rs:99/162 ✓；Q2 → predicate.rs:296 ✓；Q3/Q12 → parse.rs:1020/:26 ✓；Q4/Q5/Q15/Q1 → script.rs:1067/:517,535,565,582/:489/:1270 + content.rs:477 ✓；Q6/Q7 → engine.rs:660/:809 ✓；Q17 → paths.rs:606–610 ✓；Q8 → js.rs ✓；Q14 后半（`'|'` 键碰撞）→ types.rs:397 `QUIRK(CORE-19/Q14)` ✓。
- **should-fix-1（措辞）**：附录导语称 "every row below carries a `// QUIRK(<finding-id>):` comment at its ported site"——两句不符：① Q14 前半（sameRef 对 param ref 抛错）**无 Rust 站点可挂**——sameRef 整体是 TS 死代码、按 §1.2"明确禁止"清单**有意不移植**，登记形是 ref.rs 尾部的 NOTE 注释（非 QUIRK 形；ref.rs 内唯一的 QUIRK 是 :145 测试注释、对应 ref.ts:37 引号转义）；② Q16 在 format.rs:184 是行内 `(Q16: …)` 注释、非 `// QUIRK(` 形。冻结本身完好（行为在位/不移植有据），但按附录导语去 audit Q14 会找不到声称的注释形——改一句导语（"QUIRK comment or, for deliberate non-ports, a NOTE"）即清。

## (d) messages.md 对账 — PASS（192 亲数复核 + 锚抽查）

- **reviewer 独立复数**：`grep -o toThrow test/*.test.ts` 合计 **192**、13 个非零文件，逐文件 clock 9 / conditionals 29 / decks 30 / dice 34 / endings 13 / engine 8 / functions 25 / lint 10 / nightjar-explorer 2 / query-predicate 6 / script-play 7 / sentence 9 / stackable 10——与 W6 对账节**逐格相等**（nightjar/parallel/reach 系 6 文件为 0，192−16 `.not.toThrow` = 176 正向，五波和 24+16+32+91+13 = 176 ✓）。
- **pinned 锚抽查**（要求 3，实查 6，全部真实存在）：`bad_ending_lines_fail_loudly`（endings.rs:214）、`reserved_words_cannot_be_names`（lint.rs:75）、`arity_mismatch_is_a_load_error_too_few_too_many…`（functions.rs:399）、`duplicate_entity_declarations_are_a_load_error…`（lint.rs:95）、`steps_cannot_rewind_the_clock`（engine.rs:809）、`malformed_progress_arity…`（engine.rs:1098）✓。
- W5 增补节 = wave-5 review should-fix-4 要求的三点（四文件 toThrow=0 → 零行；cli.rs 钉 exit 码/usage；demand parse 错沿用 W1 pinned 行）✓。W6 自身 0 新增行（demo.rs 非 vitest、repl.rs 非移植）——13(d) 记 0=0 ✓。

## (e) 亲测 — PASS

- `cargo clippy -j 4 --all-targets`（touch 五 W6 文件强制重编译）：**0 warning**。
- `--test repl` **3/3**（含两条真子进程 REPL 会话 + dispatcher 全分支库重放）、`--test demo` **2/2**、`--test gate` **4/4**（新门 math-random/tui-residue 均绿）、`--test golden` **9/9**（276 条 replay）。
- **黄金重生成**（cargo 结束后单独跑）：`npx tsx tools/golden-dump.ts --out <tmp>` → `goldens: 276 ({script:3,walk:5,scenario:16,anchor:19,fuzz:200,demo:1,reach-cli:32}), jsonl bytes total: 5498805`；`diff -r` 对入库树 **IDENTICAL**，临时目录已删除——S10 补块零语料影响的 commit 声明独立复证。

## (f) TS 侧零改动 + golden-dump S10 补块 — PASS

- `git diff --stat rust-port-w5..HEAD -- src/ test/ scripts/ examples/` → **空** ✓；`git status --short` 于 TS 侧（含 tools/）无未交残file。全波 Rust 触面 = rust/{Cargo.toml(+1 bin), src/{lib.rs, bin/{chorus,reach-fuzz}}, tests/{repl,demo,gate}.rs + demo-world.txt} + README + messages + tools/golden-dump.ts（唯一 TS 触面，规则 10 豁免）。
- **S10 补块对照**（W4 should-fix-1 清账）：新增 :1046–1073 的 optional 对照块与 reach-adversarial.test.ts:783–809 原文**逐行对读一致**——同一 optional 内容 rig（X/GOAL 原型、MAKER/SELFMADE 双动词、SELFMADE 的 min:0 with 槽）、两条断言（`analyzeCard(optional,…,'GOAL').verdict === 'obtainable'` + `fwOpt.everNames.has('GOAL')`）以 `check()` 形复现，`compileS`/`sDict`/`state` 用 dump 侧既有夹具（与已审的 S10 主块同形）；头注释指认 W4 should-fix-1 与"不产引擎记录、语料零变化"。check 门在 emit('s10',…) **之前**全过 ✓。亲测重生成 IDENTICAL（见 (e)）——"check-only、零字节影响"为真。
- reach-fuzz.rs 分数旗标登记（W5 should-fix-2 清账）：`steps as usize`/`states as usize` 处补 REGISTERED DIVERGENCE 注释，`--steps 2.5`/`--states 2.5` 的 TS 浮点循环界多放行一轮的两侧行为写明，整数/默认/NaN 精界面圈清 ✓——与 wave-5 review should-fix-2 的描述逐点相符。

## (g) 分歧登记四条逐条核 — PASS（4/4 成立）

**① demo journal 的 10-token id 映射断言（"多一处即 fail"）— 成立，且实测精确。**
repl.rs:138–156 的 map（4 条 x→e mint + 4 条裸 id→`s_` id + 2 条转义容器值）对 golden 的 id 承载点做了**全量清点**：demo.journal.txt 的 `#`-token 恰为 `#ana`×1、`#house`×2、`#study`×1、`#dining`×1、`#x3_1`/`#x6_2`/`#x6_3`/`#x11_5` 各 1，加 attrs 转义 `"house"`×2、`"dining"`×1——**每一条都在映射内，映射的每一条都有真实承载点**，无空转规则、无漏网 token。断言形是 `assert_eq!(journal_section, map_demo_ids(golden))`：golden 是固定串、map 是固定变换，REPL 侧任何超出该映射的 id 拼写/多余/缺失 token 都使等式崩。映射的三条来源各自核实：`s_${lower(name)}`（script.ts 编译铸造）、engine 默认 `e{firstRecordId}_{++mint}` 且 `mint` 每步归零（engine.ts:178–179，`let mint = 0` 在 executeStep 内）——e3_1/e6_1/e6_2/e11_1 与各步 firstRecordId 对账吻合。物理逐字节不可达的论证（TS 例子注入 idGen 而 Game 路径没有）成立，补偿面（demo.rs 用 verbatim idGen 真逐字节对 golden）到位——两个面合起来把 demo 语义钉了两遍。残余note 见 note-2（replace_all 的子串作用域）。

**② demo-world.txt 偏离①的 script.ts:186 守卫论证 — 成立。**
守卫原文在位：`if (entityNames.has(name)) throw new Error(`'${name}' is declared both as an entity and a prototype — spawns of it could never mint`)`（script.ts:186）——而 seed.ts 的 CLUE_SECRET_HIDEOUT **既是实体又是原型**（程序化 ContentRaw 直进 compileContent，绕过 parseScript 的 load 守卫），文本转写**不可能**两全，弃原型是必然选择而非偷懒。弃原型无行为差：六步的 SPAWN 面是 CLUE/BONDING_MEMORY/PLAN_INFILTRATION/DREAD（CLUE_SECRET_HIDEOUT 原型零 spawn），实体侧 TALK ABOUT 只按名解析实体。偏离②③（`s_` id、默认 idGen）的可见面只在 canonical 句/attrs（prose 无 id）——与 golden 对读吻合，repl.rs 头注 + demo-world.txt 头注双登记 ✓。

**③ 裸 wait 语义分叉只在 bin 层 — 成立（git diff 实证）。**
`git diff --stat rust-port-w5..HEAD -- rust/src/session/` → **空**：game.rs（wait() 的 ceil-to-halfhour 量化、handleLine、inspect、suggestions 全部）零触碰，session 层仍是 play.ts 的逐字移植、被 W3/W4 的 7 文件锚 + 黄金差分钉死。分叉是 bin dispatcher 的**新 UI 路由选择**（按钮语义），plan §4-W6 明文授权，bin 头注释"recorded, not accidental"登记 + WAIT 动词可达路径（canonical/march wait）在位，胜路 journal 字节相等证明语义栈未被迁就。✓

**④ gate.rs 的 defang 是否漏真实违例 — 推演：不漏（可达范围内）。**
blank_comments_and_strings 是单遍状态机（行注释/块注释/raw string/普通串转义/char-vs-lifetime），逐构造推演：(1) 违例 token 只出现在注释/字符串内 = 按定义不可执行代码，非违例；(2) `"https://…"` 类串内 `//`——状态机先入串态，串内 `//` 不触发行注释，串后真违例照扫 ✓；(3) 块注释嵌套近似（非嵌套）只会**过扫**（首个 `*/` 提前结束注释，其后违例照扫）——漏扫方向不存在；(4) lifetime/char 启发式（闭引号 2 字符内）错判方向同样是过扫/过blank，合法 Rust 中无把真代码blank掉的构造（`'a>` 形一律按 lifetime 保留）；(5) raw string `r#"…"#` 处理在位（regex 字面量全走此形）。能构造的真实缺口只有**对抗式**一条：`use rand as r; r::random()`——别名击穿 `rand::` 字面扫描。但 gate 头注释明写"the gate only ever promised to catch accidents"、语法树升级被规则 6 依赖冻结排除且登记——威胁模型声明与实际能力一致，判登记充分、不算漏。tui 零残差形式化：RESIDUE 六 token（react/jsx/tsx/useState/useEffect/ink!）在 blank 后扫，文档注释与 reach CLI 冻结 usage 串（`tsx tools/reach-cli.ts`）不误报——"清单零残留"的形式化可信（检查本身登记为文本扫描）。

**prelude（CORE-03）实际内容**：lib.rs 新增 `pub mod prelude` 收 8 组再导出（content 编译/脚本、dict/State/Entity、js 错误通道、engine 执行/fold/store、session Game/lenient/render、reach 分析器面）——与 CORE-03"显式前门"相符；**模块树保持全 pub**（偏离 plan §2.1 "pub use 收窄"的字面），lib.rs 注释登记了理由（移植测试的 `chorus::<module>` 路径与 TS import 一一对应、reach 缝隙模块属受审面）——登记在位、理由成立，记 note-3。

## 附带核对（前波遗留清账）

- W4 should-fix-1（S10 对照块）：**已清**（见 (f)，亲测语料零变化）。
- W5 should-fix-2（fuzz 分数旗标登记）：**已清**（bin/reach-fuzz.rs:113–122 注释在位）。
- W5 should-fix-4（messages.md W5 节）：**已清**（W5 增补节三点齐全）。
- W5 should-fix-1（aspects= pair 非 Q13 JSON 角）：**如实顺延**——messages.md 明写"不在 W6 清单内，遗留至下一工具/分析器触面"；本波零分析器触面，顺延合规。台账保持开放。

## 问题分级

**must-fix：0 条。**

**should-fix：1 条（不阻塞收口）**

1. **README 附录导语对"登记形"的超宽声明**：Q14 前半（sameRef param-throw）是**有意不移植**的 TS 死代码，其登记是 ref.rs 尾部 NOTE 注释而非"ported site 上的 `// QUIRK(<finding-id>)` 注释"；Q16 在 format.rs:184 是行内 `(Q16: …)` 形。行为冻结本身 17/17 完好（(c) 逐点核实），仅导语与两行实际登记形不符——改一句导语即可清账。

**note：3 条**

1. messages.md W6 节称"一次性对账脚本核对后即弃"——reviewer 以独立 grep 复数得同一 192/13 文件/逐格分布，结论可复现，无入档脚本不构成缺口（记录备查）。
2. map_demo_ids 的 replace_all 作用域是**整串子串**（如 `#ana` 会改写假想中的 `#anax`）：在本固定 golden 上经 token 清点证实每条规则恰有真实承载点、无过配，但该精确性依赖语料不变（golden 是冻结产物，成立）；若未来 demo 语料再生且引入含这些子串的新 token，映射需随之复核。
3. prelude 的"收窄"字面偏离（模块树保持全 pub + 加性 prelude）已由 lib.rs 注释登记并给出测试锚理由；与 plan §2.1 的措辞差属计划字面 vs 实现取舍，行为面零影响，记录备查。

**结论：PASS-with-notes——可打 `rust-port-w6` tag。** 三条关键分歧的论证与登记全部经原文对照/推演/亲测证实；should-fix-1 建议随任意后续文档触面顺手清（一行导语）；W5 should-fix-1 继续挂在台账上等分析器触面。
