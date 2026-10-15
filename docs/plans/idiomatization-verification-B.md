# B 类清单验证报告（rust-idiomatization.md §1.1）

验证日期 2026-10-14。基准：`docs/spec/text-engine-v0.2.md`（规范）、README Q 表、
实际源码（file:line 均为当日核实）、钉住行为的测试。方法：只读 grep / 读源码；
未跑 cargo（全量编译被禁）。

结论速览：**19 项中，6 项完全成立（Q2/Q4/Q6/Q10/Q11/Q15）；3 项成立但修复须按
本报告调整（Q1/Q3/Q5）；4 项修复方向 REFUTED（Q12/Q13/Q14/Q16）；Q8 与 5 项非
怪癖约定建议维持（唯 B-22 可修且与 spec 更一致）**。计划与 README 的两处事实
错误见 §3（Q14 半边已死、Q11 "no test pins it" 注释过时），另有一处基线数字存疑
（§4："711 tests" 静态数不出）。

## 0. 汇总表

| 项 | 现状一致? | 修复方向 vs spec | 影响面实测 | 连锁 |
|----|----------|-----------------|-----------|------|
| Q1 | 一致（content.rs:479/491 base 0；script.rs:1269 base 1） | 成立；**建议翻转 lint 侧而非 compile 侧**（零日志churn） | 计划夸大：仅 script_play.rs:345 + content.rs:1215 两处钉住；bundled 脚本无 id-less 非默认 recipe，digest 不动 | 无 |
| Q2 | 一致（predicate.rs:298 find_iter 吞字符） | **成立，spec 一致**（§6.3/§4.5 文法无 `@`；空格才是 glue） | 1 个单测（predicate.rs:1160）；query.rs 侧本就响亮报错，修复使两前台一致 | 无 |
| Q3 | 一致（parse.rs:1021 死键检查；spawn_args 键/值皆可） | **"teach" 成立且 spec 一致（§9.40 "pair key or value"）；"reject uniformly" REFUTED** | 0 个现存钉住；纯新增测试 | 无 |
| Q4 | 一致（script.rs:1065 role 只 cast 不验） | **成立，spec 一致**（§4.3 六角色清单 + §10 "failing loudly…bad grammar"） | 0 破坏：bundled+测试脚本 slot 键全部合法（actors24/object16/at8/with4/about2/to1）；仅新增错误用例 | 无 |
| Q5 | 一致（script.rs:516/534/564/581；实体/ending 报错，proto/tmpl/verb 静默） | 成立；**须补第 4 半：id mint 小写（script.rs:1154 `s_{name.to_lowercase()}`）致 `ANA`/`ana` 撞 id，仅大小写敏感查重修不掉它，须加 "minted id 已被占用" 装载错误** | 0 破坏：decks.rs:1165、endings.rs:196 两现有报错测试保持；静默覆盖无钉住 | 与 Q4 无耦合；注意 fixtures 自身勿同时踩 Q4 |
| Q6 | 一致（engine.rs:652） | 成立（spec 对违规日志行为沉默） | 计划指错文件：钉住的是 **engine.rs:3263 单测**（非 fold/store 集成测试） | 无 |
| Q7 | — | spec 钉死（A16/§9.41），无"修复"可言 | — | — |
| Q8 | 一致（js.rs:23-34） | 可修但 **成本/收益倒挂**：Rust `\s` 差异仅 U+FEFF/U+0085，bundled 内容为零；6 个 src 文件改类 + 手扫器 | js.rs 单测 3 处（:425/:440/:483）；其余 tokenizer 测试不受影响 | 无（但面宽，回归风险最大） |
| Q9 | — | 实现注记（A17 拥有行为），无修复对象 | — | — |
| Q10 | 一致（lexer.rs:161） | 成立（spec 对转义沉默；bundled 只用 `\"`，新语义下不变） | 计划夸大：仅 1 单测（lexer.rs:309）；dice/engine 字符串测试未钉 | 无 |
| Q11 | 一致，但 **game.rs:305 "no test pins it" 注释过时**（repl.rs:60 钉住） | 成立（spec §11 只说 inspect 是"一张卡的脸"，未钉裸形式） | repl.rs 一处（:60 + 镜像 :102-104）；cli.rs 无 inspect 钉住（计划 "repl/cli" 中 cli 落空） | 无 |
| Q12 | 一致（parse.rs:25/853） | **REFUTED**：§5.1 明文列举保留字表（不含 ACTORS/OBJECT），且 §9.40 "before entity names and before role names" 钉死角色优先于实体正是设计语义 | — | — |
| Q13 | 一致（types.rs:28/365、query.rs:339） | **REFUTED**：§4.2 钉死 aspects 形状为"weighted principle tags, dot sub-keys"（两层）；nested tree 超出 spec 数据模型 | query.rs:410 单测钉住拍平 | — |
| Q14 | **半不一致**：README/计划"same-ref 比较对 param ref 报错"在 Rust 树已不存在（sameRef 死代码未移植，见 docs/rust-port/reviews/wave-1.md:41,79）；只剩 `|` 键半边（types.rs:159/396） | 键编码"修复"理论上可（spec 沉默）但引擎只产 `#hex` id，`\|` 撞键不可达 → 纯 churn | 约 4-6 处快照钉住（query_predicate.rs:147、common_smoke.rs:292、engine.rs:686-701、seed.rs:523） | — |
| Q15 | 一致（script.rs:488 bad clock 误归因；reach.rs:33-39；lint-scripts.rs:54） | 成立（spec 对 CLI 沉默） | cli.rs ≥4 测试（:50 trailing demand、:89 bad demand exit1、:145 missing dir exit1、:37 usage exit2）；"bad clock" 误归因无钉住 | 无 |
| Q16 | 一致（format.rs:184-188） | **REFUTED（方向空洞）**：i64 无 -0，parse.rs:971 已把 `-0` 归一为 Delta{0}；"canonicalize at parse" 是 no-op。真改法则破坏 delta/绝对值文法区分（`0` 是绝对 set、`+0` 是 delta，§9.31）或需 sign 位模型改动 | 0 个钉住（tests/src 均无 `+0` 断言） | — |
| Q17 | 一致（paths.rs:606） | 非缺陷，无修复对象 | — | — |
| B-18 | 一致：**250 处 `ChorusError::new`、191 处 `assert_err_msg` 精确复核** | 措辞属 B（§0.3 规则）；"修复"= 纯 churn 191 个 regex 钉，无方向收益 → 维持 | — | — |
| B-19 | 一致（js.rs:23 cmp_utf16） | §5.1 "canonical = sorted" 未钉 collation → 属 B 可修；但 name 文法 ASCII-only，差异仅经 quoted-string ref 可达 → 修复近乎不可观测 → 维持；不应升 A（A3 只钉"有序"非"何种序"） | — | — |
| B-20 | 一致（js.rs:269 jsjson；§7 示例确不涉整数型键/控制符） | 整数型键经作者文法（name/dottedName 字母开头）**不可达** → "修复"若成立须先证 byte-neutral（实为 C）；escape 集与 JSON.stringify/serde_json 已一致 → 维持 | — | — |
| B-21 | 一致（js.rs:258） | §11 未钉建议顺序 → B；修复即玩家可见建议序 churn → 维持 | — | — |
| B-22 | 一致（js.rs:86-93；§9.41 确有 2^53−1 seed 向量） | **可修且方向更贴 spec**（§10 "an integer"）：f64 中转在 >2^53 处静默改写作者整数；但该步仅被 js.rs:472、clock.rs:201 两单测钉住（dice 向量的 2^53−1 可精确表示，不依赖 f64 中转）——计划"pinned by dice vectors"依据不准。修时须定义溢出尾部（饱和 or 报错，计划归 C 合理） | js.rs:472、clock.rs:201，另需查 seed:>2^53 的测试（无） | 低 |

## 1. 三项点名核验（spec 原文对照）

- **Q4 vs §6.1/§4.3/§10 — 修复成立（spec-ALIGNED）**。§4.3："`Roles:` `actors`
  (subject…) · `object` (direct) · `with` (instruments) · `at` (where) ·
  `about` (topic) · `to` (recipient)"——六个角色名被 spec 枚举；§6.1 文法
  `slot := role ":" count [ query ]`；§10："compileScript parses and compiles
  it once, **failing loudly at load time** (unknown kind, unregistered verb or
  relation type, **bad grammar**)"。拼错的 slot 键在六角色词表之外，拒绝即
  spec 对齐。代码侧修复面极小：`SlotDef.role` 是 String（roles.rs 的
  `Role::from_lower` 现成可用），script.rs:1065 一处校验即可。
- **Q5 vs §10（声明章）— 修复成立，但须扩一半**。§10 只钉：ending id
  "UPPER_SNAKE and **unique**"、deck id "UPPER_SNAKE, unreserved, and
  **unique**"、实体名 "unique among the living (§9.14)"（运行时法）；
  prototypes/templates/verbs 的重复声明 spec 沉默 → 统一报错属 B，无冲突。
  functions（content.rs:1038）与 decks（content.rs:856）**已经报错**——修复
  是向既有多数行为看齐。但 `ANA`/`ana` 半边根因是 id mint 小写
  （script.rs:1154 `format!("s_{}", name.to_lowercase())`），两者是不同名字
  （§6.3："names are case exact, §9.14"），大小写敏感查重抓不到；正确修法是
  增加"minted id `s_ana` 已被 'ANA' 使用"类装载错误（id 唯一性与 spec 一致），
  而非改成大小写不敏感查重（那会与 case-exact 名字法冲突）。
- **Q2 vs 谓词文法（§6.3/§4.5）— 修复成立（spec-ALIGNED）**。§6.3 比较符仅
  `>= · <= · > · < · = · !=`，`@` 不在文法；spec 对空格的唯一豁免是
  "**Spaces around a comparison inside a query are glue**"——即修复必须
  （也只需）对空白保持静默跳过、对其余文法外字符报错。§4.5 查询解析器
  （query.rs:187-236）对坏 term 本就响亮报错（"bad aspect term"），Q2 修复
  使 predicate.rs 与之对齐，方向自洽。

## 2. 影响面抽查（≥5 项，grep 实测）

| 项 | 计划声称 | 实测 |
|----|---------|------|
| Q1 | "breaks byte-identity of every recorded log and the engine/lint id assertions (engine.rs, lint.rs)" | **夸大且文件指错**：`grep -rn '\.recipe[0-9]' tests/ src/` 仅 script_play.rs:345（集成）+ content.rs:1215（单测）；nightjar/demo 所有 recipe 带显式 id 或 `default`，日志字节与 golden digest 不动；lint 警告测试全用显式 id（lint.rs:156 read.late） |
| Q2 | "lint/query_predicate tests may need new error cases" | 现存钉住仅 predicate.rs:1160 `tokenizer_swallows_illegal_characters_quirk` 1 处；新增用例说法成立 |
| Q4 | "no bundled content has typo'd slots; lint.rs gains cases" | 成立：scripts+tests 全部 slot 键 ∈ 六角色（actors 24 / object 16 / at 8 / with 4 / about 2 / to 1）；零现存破坏，仅新增 |
| Q5 | "script.rs tests + a fixture sweep" | 现存报错测试 2 处保持（decks.rs:1165、endings.rs:196）；静默覆盖零钉住；新增错误用例即可 |
| Q6 | "fold/store tests" | **指错**：钉住是 engine.rs:3263-3292 单测 `register_windows_roundtrips_the_sentence_and_silently_skips_degenerate_rows`；tests/ 无集成钉住 |
| Q11 | "repl/cli tests pin the current card dump" | **cli 半边落空**：cli.rs 无 inspect；repl.rs:60/:102-104 一处 |
| Q15 | "cli.rs/lint.rs pin exit codes and usage text" | 成立：cli.rs:37（usage exit2）、:50（trailing demand）、:89（bad demand exit1）、:145（missing dir exit1）；lint.rs 不涉（其测试全走干净脚本） |
| Q16 | "format.rs tests" | **落空**：tests/ 与 format.rs 单测均无 `+0` 断言，现无任何钉住 |
| Q3 | "functions.rs / sentence.rs tests" | **夸大**：无任何测试钉 `expected ')'` 或 SET 参数键拒绝；零现存破坏 |
| Q10 | "dice/engine string tests" | **夸大**：仅 lexer.rs:309 单测；bundled 内容只用 `\"`（新语义下逐字节不变） |
| Q8 | "rippling into every tokenizer test" | **夸大**：U+FEFF/U+0085 仅 js.rs 单测钉（:425/:440/:483）；6 个 src 文件携带类常量，其余 tokenizer 测试不涉 |

另：B-18 的 250/191 两数**精确复核成立**；计划"79 unit tests"精确成立。

## 3. 计划/README 事实修正（应随 P3 或更早落文档）

1. **README Q14 前半已死**："same-ref comparison errors on a param ref" 是 TS
   `sameRef` 的行为，Rust 移植时按死代码未移植（docs/rust-port/reviews/
   wave-1.md:41 "sameRef 死代码未移植"；:79 "Q14（`|` 键 + sameRef 抛错）… ✓"；
   现树 `grep -rn sameRef src/ tests/` 为空）。README 行与计划 §1.1 Q14 行
   均应改为仅述 `|` 键半边；该半边属不可达输入，建议"维持 + 修文档"。
2. **game.rs:305 注释过时**："no test pins it; frozen as-is" —— repl.rs:60
   与 :102-104 恰恰钉住裸 `inspect`。Q11 修复提交应同步改注释。
3. **Q1 文件指针漂移**：计划称 id 断言在 "engine.rs, lint.rs"，实测在
   script_play.rs:345 与 content.rs:1215。
4. **基线数字存疑**：全仓静态 `grep '#\[test\]'` = 615（tests/ 536 + src/ 79），
   与计划"711 (79 unit + 632 integration)"差 96；unit 数精确吻合，integration
   数对不上（无 doctest：src 无代码围栏）。P0 落地时应以一次真实 `cargo test -j 4`
   重新定基，验收门槛数字随之修正。

## 4. 连锁分析与建议 P3 修复顺序

Q2/Q4/Q5/Q3/Q1/Q11/Q15/Q10/Q6 相互之间**无编译或语义依赖**（解析位点互不相交：
predicate.rs 词法 / script.rs slot 行 / script.rs 声明族 / parse.rs arg() /
script.rs+content.rs id mint / chorus.rs+game.rs / bin/* / lexer.rs /
engine.rs register_windows），可任意排列、一怪癖一提交。唯二注意点：
(a) Q5 的测试 fixture 若同时写拼错 slot 会同时触发 Q4 的错误——fixture 各犯
一错即可；(b) Q1 选"翻 lint 侧"则与 golden digest 零交集，选"翻 compile 侧"
才产生日志 churn——建议前者。

建议分批（同批内无连锁，批间按风险递增）：

- **批 1（编译期拒绝，零日志/digest 变动，零现存测试破坏）**：
  Q4 → Q5（含 minted-id 冲突检查）→ Q2。每项 = 修复 + 新增错误用例 +
  README 状态列。
- **批 2（接受面放宽 / CLI / 外观）**：Q3（teach arg() 键形式）→
  Q1（lint 镜像翻 0-based，仅 README+注释+2 处断言核对）→ Q11（usage 卡表，
  repl.rs 一处）→ Q15（cli.rs 4 处 + 新错误文案）。
- **批 3（语义微变，各改恰好 1 个单测）**：Q10（真转义语义；lexer.rs:309 重写，
  bundled `\"` 逐字节不变）→ Q6（响亮 fold 错误；engine.rs:3263 翻为 expect Err）。
- **驳回/维持，不进 P3**：Q12（§5.1 保留字表 + §9.40 遮蔽序 = spec 行为）、
  Q13（§4.2 数据模型）、Q14（半边已死+不可达）、Q16（方向空洞）、
  Q8（可修但成本/收益倒挂）；B-18/B-19/B-20/B-21 维持，B-22 可选并入批 2
  （直 i64 解析 + 定义溢出尾部）。

每批验收沿用计划 §2 门槛：711（应修正为实测基线，见 §3.4）绿、clippy 零警告、
golden digest 逐字节不变（批 1/2/3 均不应动 digest；唯 Q11/Q15 会动 CLI stdout/stderr
金样本，须在提交信息里逐行枚举）。
