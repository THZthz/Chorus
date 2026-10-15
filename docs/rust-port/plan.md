# chorus → Rust 移植计划（plan v2）

依据：`docs/rust-port/analysis/01–05` 与 `docs/rust-port/verification/01–04` 共 9 份报告；v2 吸收三份评审（plan-review-1-semantics / 2-feasibility / 3-process）的 8 条 must-fix 与采纳的 should-fix（见文末修订记录）。**以验证报告为准**；分析报告中被 PARTIAL/标红/驳回的建议一律不进入本计划。所有 file:line 均指 worktree `/Users/amias/Projects/chorus/.worktrees/rust-port`。本计划只读产出，未改动任何源码，未运行构建/测试。

---

## 1. 目标与不变量

### 1.1 "行为逐位一致"的定义

本项目对"一致"有现成、可机检的定义（spec §7 + plans/v0.4 六条 determinism laws，plans/v0.5 §5）：

1. **相同初始态 + 相同输入序列 ⇒ 逐字节相同的 JSONL 日志**。日志是唯一事实源（State 从不序列化，ENG-08/ENG-20）。
2. **fold 等价**：`fold(log)` 重建的 State（含 rng 计数、decks 游标、fired、windows、ended）与 play 得到的 State 在 `snap()` 意义下相等。
3. **RNG 向量**：`splitmix64` / `rollDie` / `permutePositions` 对语言无关常量向量表逐值相等（test/dice.test.ts:85–89 的 3 个原始值 + :94–113 的 27 行 (seed,counter,sides)→die 表；test/decks.test.ts:141–144 的 4 个置换向量，全部 python3 独立预计算）。
4. **错误消息逐字**：test/ 全仓 **192 处 `toThrow` 断言**（13 个文件；验证报告 CORE-16/TEST-05 勘误：不是分析报告的 221）把错误消息文本与罗列顺序钉成 API；另有无尽数量的 `refusal()/toMatch` 消息断言。Rust 消息必须逐字符相同（含空格与标点）。
5. **输出顺序**：日志行序、canonical 句子的 actors/objects/with 序（UTF-16 码元序）、JSON 对象键序（插入序 + JS 整数样键前置规则）、错误消息内罗列序、**suggestions() 列表序**（play.ts:238，被 script-play.test.ts:421 钉死 `s[0]`，见 §3.21），全部是可观察行为。
6. **canonical 文法契约**：`format(parse(x)) === x`（§5.2 worked examples 逐例）；"canonical equals expansion: a call site IS its expanded body, byte for byte"（parse.ts:654–655）。
7. **reach 输出**：reach-cli stdout（verdict/chain/paths/notes 文案）逐字节对照；同一 (content, State) 两次分析输出逐字节一致；`--all` 名单排序 = UTF-16 码元序（REACH-05，名字集 = prototypes ∪ 活初始实体 ∪ deck.order，**不含 defaultcard**，CLI-02 修正）。

### 1.2 怪癖冻结原则（quirk freeze）

分析发现的所有 bug / 不一致 / "怪癖当特性"，**一律原样移植、只记录不修复**。任何一条的"顺手修复"都是行为变更，等于破坏 §1.1。冻结清单（执行者在对应 Rust 文件写 `// QUIRK(<finding-id>):` 注释）：

| # | 怪癖 | 出处 | 冻结要求 |
|---|------|------|---------|
| Q1 | recipe id 基数漂移：编译 0 基 `.recipe0`（被 script-play.test.ts:175 钉死）、lint 镜像 1 基 `.recipe1` | content.ts:229 / script.ts:485（ENG-15） | 两条路径保持各自基数。可写成一条铸造函数的两个参数化变体，**输出不变**；统一基数另立议题 |
| Q2 | 谓词字符串 tokenizer 静默吞非法字符（`aspect:x >= @3` 等价 `aspect:x>=3`） | predicate.ts:172–178（CORE-08(3)） | 保留 exec 循环"跳过不报错"语义，注释禁止修复 |
| Q3 | `arg()` 里 param 作 pair 键不可达（575 行提前 return），`SET(myparam = 3)` 报 `expected ')', got '='`；而 spawnArgs（533–537）支持 param 键——SPAWN/SET 不对称 | parse.ts:575/587/533–537（CORE-12） | Rust `arg()` 不查 param 键；死分支不移植；严禁给 SET 补 param 键支持 |
| Q4 | 槽位 role 不校验，typo 槽静默失效 | script.ts:391（正则 171）/ content.ts:221–227 / engine.ts:85–87（ENG-16） | 照常编译未知 role，编译期拒绝另立议题 |
| Q5 | 重复声明处理不一致（entities/endings 报错；prototypes/templates 静默覆盖；verbs 重复头静默重置）+ 实体 id `s_${lower(name)}` 与区分大小写查重并存 → `ANA`/`ana` 大小写碰撞静默覆盖 | script.ts:136–163/431、types.ts:159（ENG-17） | 现状保留，统一校验另立议题 |
| Q6 | format∘parse 往返失败的 BEGIN 行**静默不开窗口**（`catch { clauses = [] }`） | engine.ts:344–356、fold 侧 1027（ENG-06） | 静默语义原样保留；禁止改为报错或内存直传 clauses（直传在退化输入上与现状分叉） |
| Q7 | 被拒 REFUSE 的 NARRATE 带预演骰点数字但计数器未动 | engine.ts:437–447 注释（ENG-11） | 原样复刻 |
| Q8 | 正则 `\s` 差异：JS `\s` 含 `\uFEFF` 不含 `\u{85}`，Rust `\s`（White_Space）恰反。适用面=全部移植正则（见 §3.8 与附录 A） | lexer.ts:26、query.ts:18、script.ts 25+ 处、lenient.ts 4 处等（CORE-08(1)、PR2-03） | 统一用显式 JS 字符类（js.rs 单点常量 `JS_WS_CLASS`） |
| Q9 | DUR_RE 交替序 `halfhour|h|m|d` 承重（`h` 在前会把 `1halfhour` 切成 `1h`+`alfhour`） | lexer.ts:15（CORE-08(2)、CORE-14） | 保序；regex crate 与手写扫描均为 leftmost-first，保交替序即逐位等价 |
| Q10 | 引号字符串 `\X` 转义取**字面**下一字符（`"\n"` 的值是字母 n）；尾反斜杠把字符串 `"undefined"` 拼进值后仍抛 unterminated（中间值不可观察） | lexer.ts:30–33（CORE-09） | 逐字面转义；越界分支 `push_str("undefined")` 后照常走 unterminated 抛错（行为等价裁定，不再自问自答） |
| Q11 | bare `inspect` 静默显示第一张可见卡（`startsWith('')` 恒真） | play.ts:133–136（UI-06，无测试锚定） | 若保留 inspect 视图则原样保留该行为 |
| Q12 | ROLES 在 outcome-arg 位置是"软保留"（asRole 先于裸引用拦截），但不在 RESERVED_WORDS；名为 OBJECT 的实体在结果参数位置无法裸引用、子句位置可以 | parse.ts:576–581 / 227–233、dict.ts:11–18（CORE-15(3)） | 拦截顺序承重，原样复刻 |
| Q13 | aspect 单层点 + 后写整值覆盖：`{a:1,'a.b':2}` 子映射静默覆盖标量；`'a.b.c'` 落成 `{a:{'b.c':v}}` 且查询对称可取 | query.ts:102–114、types.ts:194–219（CORE-18） | `enum AspectVal` 如实复刻，不做真嵌套树 |
| Q14 | `sameRef` 对 param ref 抛错而非返回 false；relation 键内嵌 `'\|'` 靠"引擎铸造 #hex id"约定兜底碰撞 | ref.ts:39/51–53、types.ts:225–230（CORE-19） | 原样复刻 + 注释 |
| Q15 | `--demand` 为末位 argv 时静默消失；lint-cli 目录缺失未捕获 exit 1；未知 meta 键报 "bad clock …" 归因错位 | reach-cli.ts:20、lint-cli.ts:16、script.ts:116（CLI-01/05、ENG-19） | CLI 错误路径**按现状冻结**，改进一律另立议题 |
| Q16 | `-0` delta 规范输出 `+0`；`SET(x,-0)` 可经词法产生 | format.ts:84（CORE-07） | i64 侧显式复刻符号分支 |
| Q17 | reach `siteReason` 的 `site.verb && !initNames.size` 分支**可达**（零实体脚本合法），非死代码 | reach.ts:1964（REACH-08 修正） | 必须保留 |

**明确禁止的"分析建议"（验证已驳回/降级，不得实施）**：
- 禁止把 `argSpelling` 的 str/pair 分支并入 `formatArg`（两侧引号拼写故意不同，合并破坏逐字节展开；只可共享 delta/int/dur/bool 拼写层）——CORE-13 标红。
- 禁止 reach 不动点 worklist/事件驱动化（fixpoint 对 `<=`/`<`/`=`/`!=` count 门**非单调**，proven 集合日程依赖；朴素 round-robin 是唯一已知逐位一致日程）——REACH-10 标红。
- 禁止"load 期校验拒绝类整数键名"（改变 TS 已接受输入）；改为复刻 JS `OrdinaryOwnPropertyKeys` 键序——REACH-04。
- 禁止统一 slot role 校验 / 重复声明报错 / id 大小写检查 / recipe 基数（Q1/Q4/Q5）。
- 禁止收紧 ending flavour 为枚举（content.ts:212 开放词表，ENG-13）。
- `sameRef` 不受排序语义影响（验证 CORE-05 修正），不做 cmp_utf16 替换。
- 死代码不移植：`crossedSlots`（clock.ts:78–84）、`attrGet`（types.ts:221–223）、`getEntity`（types.ts:182–184）、`sameRef`（ref.ts:51–53，src 与 test 均零调用）、`render.card()`、`Log.journal()`（UI-05）；engine.ts:1058 仅首个析取支死，`(toMin === now && !hasDue)` 分支必须保留（ENG-18 修正）；`emptyClause` **要移植**（explorer-lib/reach-fuzz-lib 依赖）。
- test 侧 `test.fails` 幽灵注释（TEST-09）不要为其留设计。

### 1.3 范围裁定

- **全量移植**：src/core、src/content、src/engine、src/reach、src/ui 的 render/lenient/play（Game）、tools/ 4 个 CLI。
- **不移植、以最小替代品取代**：src/ui/tui.tsx 的全部 ink/react 组件、runPlain、quoteLine、三个终端怪癖（UI-09）、examples/play.ts 的 ScriptPicker 与平行 title 正则（UI-04，REPL 改为取 `meta.title` + CLI 参数传脚本路径——这是新代码，不属行为冻结面）。**Game 不可丢弃**：7 个测试文件（decks/dice/endings/nightjar/nightjar-explorer/script-play/stackable）直接锚定 handleLine/undo/wait/journal/story/look/cards/inspect/suggestions/visible（验证 04 UI-01 修正计数）。game-over 双路径双文案都保留（UI-02：endings.test.ts:378 钉 UI 版全文案，:186/:219 钉引擎版短文案）。
- **TUI 简化授权不适用于 Game/lenient/render**：它们是语义层（验证 04"必须保留表"逐项核实通过）。

---

## 2. Rust 工程结构

### 2.1 crate 布局：单 crate 多 module + 多 `[[bin]]`（不用 workspace）

**决策**：在 `/Users/amias/Projects/chorus/.worktrees/rust-port/rust/` 建单一 crate `chorus`（lib + 5 个 bin target）。理由：
1. TS 侧 types↔sentence、sentence↔predicate 存在 type 级循环（CORE-02，已核实为纯类型环、无值级初始化问题——PR2-10）；单 crate 内模块互相引用零成本，workspace 拆分要么强迫下沉公共类型 crate 要么制造无谓的 API 面。
2. IndexMap / jsjson / cmp_utf16 等基础设施被所有层共享；跨 crate 会重复依赖或上提。
3. 16GB 内存约束下，单 crate 编译图简单，`-j` 直接控制并行 rustc 数量；多 crate 会在依赖图宽处同时拉起多个 rustc。
4. CLI 是 lib 上的薄 main，`[[bin]]` 即可，无需 workspace 的版本协同。

代价：单 crate 全量重编译略慢——以 `[profile.dev] debug=0` 与分波开发（每波只增量编自己模块）对冲，可接受。

```
rust/
├── Cargo.toml            # 见 §2.4 profile/依赖；[[bin]] × 5
├── Cargo.lock            # 入库（PR2-06），记录最小版本
├── src/
│   ├── lib.rs            # pub use 收窄的 prelude（CORE-03：显式 pub 面）
│   ├── js.rs             # JS 语义基础设施：cmp_utf16 / jsjson(JSON.stringify) /
│   │                     #   JS_WS_CLASS(js_is_ws) / ordinary_own_property_keys /
│   │                     #   js_num_parse(f64 中转) / LCG(u32) / ChorusError
│   ├── core/             # 与 src/core/* 一一对应（§2.2 映射表）
│   ├── content/          # content.rs + script.rs（含 deck 置换）
│   ├── engine/           # engine.rs + fold.rs + store.rs
│   ├── session/          # game.rs(play.ts) + lenient.rs + render.rs
│   └── reach/            # census.rs / world.rs / fixpoint.rs / paths.rs / render.rs（REACH-01 切缝线）
└── tests/                # 移植的 TS 测试 + 黄金差分（§5）
    ├── common/           # snap.rs / settle.rs / seed.rs / explorer_lib.rs /
    │                     #   reach_fuzz_lib.rs(W4) / reach_scale_lib.rs(W5) / harness.rs
    └── goldens/          # TS 侧生成的 JSONL + manifest + 场景（W4 产物，入库；§7 规则 10 豁免）
```

bin targets：`chorus`（line REPL）、`reach`、`reach-scale`、`reach-fuzz`、`lint-scripts`。scale CLI 用 `std::thread` + channel 顺序分片（不用 rayon；`--worker i k P R` 分片语义与 NameStat JSON 字段保留为跨语言 A/B 接口，CLI-03；NameStat 输出仅依赖 (world, name)，共享一次构建或每线程各建皆可，PR2-09）。

### 2.2 TS→Rust 模块映射表

| TS | Rust | 备注 |
|---|---|---|
| src/core/types.ts | core/types.rs | `Role/ROLES` 下沉到 core/roles.rs（CORE-02，零行为风险）；`byName` 外层 `HashMap<String, Vec<String>>`（池数组保序即够，ENG-04 修正），entities/relations/windows/decks 用 IndexMap，fired 用 IndexSet |
| src/core/dice.ts | core/dice.rs | W0 最先移植 |
| src/core/clock.ts | core/clock.rs | div_euclid/rem_euclid + debug_assert（CORE-10） |
| src/core/dict.ts | core/dict.rs | isOp 可用 phf/数组，行为相同（CORE-15） |
| src/core/ref.ts | core/ref.rs | cmp_utf16 用于 sortRefs/relationKey/refSortKey |
| src/core/sentence.ts | core/sentence.rs | 含 emptyClause（test lib 需要） |
| src/core/lexer.ts | core/lexer.rs | 在 `Vec<u16>` 码元上扫描（§3.9）；四个 scanner 手写（§3.8/PR2-08） |
| src/core/parse.ts（Parser 类） | core/parse.rs | CORE-01 |
| src/core/parse.ts（FnTemplate/FnTable/expandFnCalls 等 25–84、646–853） | core/fn_expand.rs | 纯搬家；与 content 编译器的接缝类型不断 |
| src/core/format.ts | core/format.rs | argSpelling 留在 fn_expand.rs，str/pair 拼写两套冻结（CORE-13 标红） |
| src/core/query.ts | core/query.rs | |
| src/core/predicate.ts | core/predicate.rs | 保留 predViewFromToks"拼串 + span 截取"形（CORE-21） |
| src/content/content.ts | content/content.rs | hashDeckId/permutePositions/drawFrom |
| src/content/script.ts | content/script.rs | lintScript 通道不抛错 |
| src/engine/engine.ts | engine/engine.rs | 所有权模型见 §2.6 |
| src/engine/fold.ts | engine/fold.rs | foldUpTo 按 `r.id < uptoExclusive` |
| src/engine/store.ts | engine/store.rs | std::fs；原子写（tmp+rename，ENG-20 允许的改进）；loadLog 错误带行号（记录为有意分歧，无测试钉住旧文案） |
| src/ui/play.ts | session/game.rs | Game 整体（UI-01） |
| src/ui/lenient.ts | session/lenient.rs | `MAX_NAME_WORDS=4` 常量化（UI-07） |
| src/ui/render.ts | session/render.rs | 不带 card() |
| src/ui/tui.tsx | （不移植） | bin/chorus.rs 新 REPL 取代 runPlain |
| src/reach/reach.ts | reach/{census,world,fixpoint,paths,render}.rs | 按报告切缝线 L146/456/577/1294；walkSites 与 forEachAuthoredOutcome 共享单一遍历器（REACH-01；seg 语义见 §4-W5） |
| tools/reach-cli.ts 等 4 个 | src/bin/{reach,reach_scale,reach_fuzz,lint_scripts}.rs | 行为冻结（Q15） |
| examples/demo.ts | tests/demo.rs（fold-verify 冒烟）+ demo 黄金输出（W4 生成，W6 REPL 对拍） | 记账用 nextRecordId 简化（UI-13） |
| scripts/the-nightjar-flyer.txt | `include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/../scripts/..."))` | 脚本文本=数据，不预编译（ENG-22） |
| test/settle.ts、snap.ts、seed.ts、explorer-lib.ts | tests/common/*.rs | **不在 src/，最易漏**（TEST-11）；W3 落位 |
| test/reach-fuzz-lib.ts | tests/common/reach_fuzz_lib.rs | **W4 落位**（PR3-03）：genScenario + LCG + 动作选择器，与 tools/golden-dump.ts 内选择器同源 |
| test/reach-scale-lib.ts | tests/common/reach_scale_lib.rs | **W5 落位**（PR3-03）：buildCsScale 等价物 |

命名纪律：Rust 函数名 = TS 名的 snake_case（`formatSentence`→`format_sentence`），每个移植单元头部注释 `// ts: src/core/dice.ts:NN` 指向原文行；后续每段标注来源行号，方便差分时人肉对照。

### 2.3 依赖选型

| 依赖 | 用途 | 约束 |
|---|---|---|
| `indexmap = "2"` | State.entities/relations/windows/decks、fired(IndexSet)、FnTable.byName、CompiledContent 各映射、reach 的 initNames/avail/proven/committable | 禁止 std HashMap 出现在任何迭代序可观察处（CORE-06、ENG-04、REACH-03）；FnTable.byName 必须保序（迭代点 parse.ts:845/849、content.ts:543/571/585，PR2-10） |
| `serde`（features=["derive"]）+ `serde_json`（`preserve_order` feature） | ActionRecord JSONL 序列化/反序列化；测试 deep-equal 的 `Value` | record struct 字段声明序 = TS 对象字面量序；可空字段 `skip_serializing_if = "Option::is_none"`（复刻 JSON.stringify 丢弃 undefined）；数字字段全部整数类型（已核 tReal 为 `string|null`、advancedMin/startMin/id 为整数，engine.ts:27/35/317）。`Arg` 内部 tag `t`、`Ref` 内部 tag `form`、`into` 的 `Ref \| {role}` 用 untagged 枚举、`sign?: 1\|-1` 用 Option（PR2-10 已核可表达） |
| `regex = "1"` | core + content + session 的正则（**词法器四件除外**，见下）+ 测试消息断言 | 全部已验证无 lookahead/lookbehind/后向引用；`$1` 仅出现在替换串（regex crate replace 原生支持）。逐条附 JS 原文注释；字符类转写通用规则见 §3.8 + 附录 A 逐条清单。计数勘误（PR2-03/PR2-11）：core = **23 个使用行 / 25 个字面量**（predicate.ts:155 与 format.ts:98 各一行两字面量）；script.ts ≈ **40 个使用行**（v1 写 "~15" 是低估）；lenient.ts 含 `\s` 的 **4 处**（37/50/182/227；另 59 为 `/^\d+:\d+$/` 无需转写） |
| **明确不用** | `rand`（grep 门禁禁入 core/engine，TEST-12）、`tokio`/任何 async（游戏路径全同步，UI-08）、`chrono`、`rayon`、`uuid`、`sha2`（manifest sha256 仅为入库元数据，Rust 差分判据是 `cmp`+snap 串，不为验 sha 引依赖——PR2-09）、ICU collation（§3.21 另立议题） | REPL 用 `std::io`；TTY 判定 `std::io::IsTerminal` |

**词法器例外（PR2-08，强制）**：regex crate 只吃 `&str`，与 `Vec<u16>` 码元域不兼容——lexer 的 NAME_RE（lexer.ts:13）、NUM_RE（:14）、DUR_RE（:15）、`/\s/`（:26）、`/[0-9]/`（:45）共 **5 件必须在码元域手写**字符判定（附录 A 标 H）；其余 core 正则的输入都是自有 String，留在 `&str` 域用 regex crate。

正则逐条转写清单见**附录 A**。

### 2.4 内存约束（写入 Cargo.toml 与执行纪律，所有波次适用）

```toml
[profile.dev]
debug = 0                 # 不生成调试信息：编译内存/磁盘大降
opt-level = 0
[profile.test]            # 继承 dev；单独开行号表，测试失败回溯可定位（PR2-06）
debug = "line-tables-only"
opt-level = 0
[profile.release]         # 仅 reach-scale 基准用
opt-level = 2
debug = 0
lto = false
codegen-units = 16
```

- 构建与测试一律 `-j 4`（或 `CARGO_BUILD_JOBS=4`）；全量重构建时 `CARGO_INCREMENTAL=0`（保守），日常迭代可开。峰值估算（PR2-06）：4 × rustc(0.5–1.5GB) + 链接器 <1GB ≤ ~7GB，16GB 余量充足。
- 测试**执行**侧线程数用 cargo 默认（=逻辑核数；本套件每测试内存 ≤ 数 MB，最重的 reach-scale 400×400 双世界进程内仅几十 MB）——无需 RUST_TEST_THREADS 限制，封口于此（PR2-06）。
- **禁止同时跑 TS 测试（vitest/tsx）与 cargo 测试**；两条 cargo 命令也不并行（多 subagent 场景见 §7 规则 14：cargo 全局串行）。
- 黄金日志比对用 shell `cmp`/`diff`，不在测试进程里整文件读入两份以上副本。
- release 基准（bench:reach 对齐）单独跑，跑前 `cargo clean` 或用独立 `CARGO_TARGET_DIR`。
- `Cargo.lock` 入库；最小版本记录：indexmap 2.x、serde 1.0.x（derive）、serde_json 1.0.x（preserve_order）、regex 1.11.x（PR2-06）。

### 2.5 错误处理策略

- `pub struct ChorusError { msg: String }`，`impl Display` 原样返回 msg；`pub type Result<T> = std::result::Result<T, ChorusError>`。**不做分层错误枚举**——枚举变体 + Display 的双维护面正是消息漂移的温床。
- 每个 TS `throw new Error(...)` 对应一个 Rust 构造点，模板串逐字符照抄（含标点/空格），就地 `ChorusError::new(format!(...))`，行尾注释 `// ts: engine.ts:492`。编译器保证没有遗漏分支（TS throw → Rust `?` 传播）。
- 消息内嵌罗列的（`unknownCallError` 拼 FnTable 声明序 parse.ts:845–849、`undeclaredTokenError` 拼 FN_RESERVED_TOKENS 序 parse.ts:784、三条名冲突消息 engine.ts:492/568、types.ts:153）**由调用点各自提供文本**，不得统一成一条（ENG-09 补充）。注意 functions.test.ts:138 现钉的罗列序"one, zero"恰为字母序是巧合——仍用 IndexMap 声明序（验证 01 附加发现）。
- reach 的两处 `throw new Error('internal: …')`（reach.ts:505/521）保留为 `Err`（TS 里可被捕获），不用 panic。
- 真"不可达"用 `debug_assert!`/`unreachable!`（如负分钟、idGen 回退），release 下不产生运行时行为。
- 测试侧 192 条 `toThrow(/re/)` → `assert_err_msg(result, r"re")`：拿 `ChorusError.msg` 跑 `regex::Regex::is_match`，模式文本照抄 JS 正则（已核无比需特性；若个别模式需转义调整，在 harness.rs 集中登记）。另建 `docs/rust-port/messages.md`：消息文本 ↔ 首个钉住它的测试 ↔ TS 源行 三列对照表（TEST-14），随波次增补。

### 2.6 状态所有权与别名模型（PR2-01，engine/reach 的类型骨架）

TS 的对象别名/身份语义在 Rust 的落法**写死如下**，禁止执行者各自发明：

1. **Entity 存于 `State.entities: IndexMap<String, Entity>`，从不被删除**（销毁 = 盖 `dead_action` 戳，engine.ts:531）。所有"读 Entity"都经 `state.entities.get(&id)` / `get_mut(&id)` 现读现写。
2. **槽位绑定是 id 句柄，不是克隆**：`RoleBindings = IndexMap<Role, Vec<String>>`（只存 entity id，顺序 = refs 解析序）。**禁止把 `Entity` clone 进 bindings**——`roleCards`（predicate.ts:399–401）与 `evalPred` 必须在 mid-list DESTROY 后经 `entities.get(&id)` 读到**当前** `dead_action`（dead-stamp"全死绑定静默 false"，predicate.ts:492–498）；克隆副本会读到旧活状态，直接行为分叉。
3. **expand() 拆两步**：先以 `&State` 不可变解析出目标 id（resolveRef 等），再 `state.entities.get_mut(&id)` 逐个写（engine.ts:531 DESTROY、:619 SET attrs）。避免同时持有 `&` 与 `&mut`；ApplyCtx 持 `&mut State`，`now`/`rng` 等标量先读出为局部量（按字段拆借用）。
4. **不需要 Rc/Arc/RefCell/Cell**：Entity 从不搬移、从不删除，`&mut State` 直改路径即可表达全部语义；内部可变容器会引入运行时 panic 面并与借用拆分冲突，**禁入引擎/reach 数据面**。
5. **reach 的对象身份键一律索引化**：`Map<RawSite,…>` → site 存入 `Vec<RawSite>`、以 `usize` 为键；`Map<CompiledVerb,…>` → content.verbs 的**插入序下标**为键；`WeakMap<CompiledContent,…>`（trigIndexCache/worldCache）→ `Analyzer<'c>` 持 `&'c CompiledContent` + 惰性 WorldFacts（§4-W5）。禁止模拟 WeakMap。
6. **IndexMap 克隆保序**：`clone_state`（types.ts:115–134）逐项深拷贝照 TS 抄，池数组/windows.locked 的 law 6 注释约束保留；`RoleBindings` 深拷贝 = id Vec 克隆，天然保序。
7. 单 crate 内 types↔sentence↔predicate↔query 的类型级循环（CORE-02）不干预：不加 dyn/trait-object 解环，模块互相 `use` 即可。

---

## 3. 语义对齐规范（逐位一致实现要点）

每条给出处与 Rust 落法。这是执行者的**逐条验收清单**，实现顺序即清单序。

1. **splitmix64 finalizer（单次应用，非完整序列）** — dice.ts:19–31。常量 GOLDEN_GAMMA=0x9e3779b97f4a7c15、MIX1=0xbf58476d1ce4e5b9、MIX2=0x94d049bb133111eb；`z=(x+γ)`、`z^=z>>30`、`z*=M1`、`z^=z>>27`、`z*=M2`、`z^=z>>31`，全部 `u64` wrapping（wrapping_add/wrapping_mul）。原始向量：splitmix64(0)=16294208416658607535、(1)=10451216379200822465、(42)=13679457532755275413。
2. **rollDie 的 u128 widening** — dice.ts:38–42。`key = (seed as u64) ^ (counter as u64)`（i64→u64 的 `as` 恰是 asUintN 二补码，覆盖负 seed）；`1 + (((sides as u128) * (r as u128)) >> 64) as i64`。**验证 ENG-05 修正：不能用纯 u64 wrapping，需要 128 位中间量**。27 行向量表（含 2^53−1、负 seed、2^40 计数器）为验收门。
3. **State.rng 计数语义** — types.ts:90–103、engine.ts:772–791。一掷 m 骰消耗 c..c+m−1（play 在 draft 上推进）；fold 的 ROLLED 行只做 `state.rng = ctr_after`，**绝不重掷**；rolled 行 `total` 从 bonus 起累加。rng/游标的**累加路径**（W3）用 `checked_add` 并显式声明分叉方向（TS 在 f64 整数区到 2^53 才失真；i64 到 2^63——实际不可达，声明即可，PR2-04）。
4. **FNV-1a64 + deck 置换** — content.ts:430–437、453–461。hashDeckId：offset 0xcbf29ce484222325、prime 0x100000001b3，对 UTF-8 字节（`.as_bytes()`）；permutePositions：位置 i 自 n−1 降序，`j = splitmix64(asUintN(seed ⊕ hash ⊕ i)) % (i+1)`，Fisher–Yates 交换。置换是 (seed, deckId) 纯函数，不耗 rng 计数。向量（decks.test.ts:141–144）：`(4,0,'RUMOURS')=[0,2,1,3]`、`(4,42,'RUMOURS')=[3,0,1,2]`、`(7,0,'SEVEN')=[0,6,3,1,4,5,2]`、`(7,-3,'NEG')=[4,0,2,6,3,1,5]`。seed 默认 0（content.ts:293）。
5. **seed 与一切数字字面量的解析：f64 中转，禁直解** — script.ts:113–114（`^-?\d+$` 后 `Number()`）与 lexer.ts:56–58 / predicate.ts:294,323 / clock.ts:37 同规则：**一律 `js::js_num_parse`（f64 解析 → i64 落点），禁止 `str::parse::<i64>()` 直解**——`"9007199254740993"` 直解得 …993，JS f64 最近偶数舍入得 …992（PR2-04）。>2^53 的字面量保持 JS f64 取整结果（f64 中转天然如此）。**i64 落点装不下时**（如 `SET(x, 99999999999999999999, 1)` → 1e20 > 2^63；`parseDuration`（clock.ts:55）`Number(p[1]) * 60` 乘法可超域）：用 `checked_*`，溢出按**文档化分歧**处理（TS 侧为 f64 有损值、无测试钉住；不做任意精度）——计划明示接受，不静默回绕、不 debug panic（PR2-04）。
6. **UTF-16 码元排序** — ref.ts:47–49（sortRefs）、types.ts:225–230（relationKey）、reach-cli.ts:73（--all）。`js::cmp_utf16(a,b)`：对 `encode_utf16()` 序列做字典序。受影响面仅此三处（sameRef 是相等比较，不在此列——验证 CORE-05 修正）。**不得**用 Rust `str::cmp`（字节序）静默替代。
7. **插入序纪律（全 port 通用）** — CORE-06、ENG-04、REACH-03：
   - State：entities/relations/windows/decks=IndexMap，fired=IndexSet，byName=HashMap<String,Vec>（池数组保序、head=最老，types.ts:78–80，外层映射全仓无未排序迭代点——snap 与 script-play:430 均先 sort，PR2-10）。
   - CompiledContent：verbs/prototypes/templates/endings/slotByRole 保序（编译报错顺序 + `.recipe${i}` 下标 + UI 遍历序依赖它）。
   - reach：initNames（State.entities 序、同名 stackable **后写胜**，中局 State 必现）、avail（push 序）、proven/committable（**证明序**，先到先证明）。
   - JSON 对象键序：任何 TS `Object.entries`/`JSON.stringify`/`Object.keys`/**对象 spread 克隆**迭代点都要 JS 序。JS 序 = 整数样键按数值升序前置，其余字符串键按插入序（`OrdinaryOwnPropertyKeys`）。落法：js.rs 提供 `ordinary_own_property_keys(&IndexMap)` 助手，适用点=编译期迭代（content.ts:204/208/219/274/276）+ 序列化/渲染（engine.ts:511–525/585–589、render.ts:33、query.ts:104）+ **克隆/重排点**（PR2-05）：types.ts:118（cloneState 实体 spread）、:132（windows spread）、:138（cloneAspects entries）、engine.ts:469（proto attrs spread）、:934（engine cloneAspects）。整数样键进 attrs 的通路真实存在：`SET(x, "5", true)`（引号键绕过 UPPER_SNAKE 校验，engine.ts:601–618）——若 IndexMap 只保插入序，clone 后 `["5","container"]` 会变 `["container","5"]`，attrs pair 与 snap 均分叉。**实现要点**：Entity.aspects/attrs 固定 `IndexMap<String,…>`；IndexMap 的 `insert` 对已存在键保原位（与 JS 覆写旧键保位一致）；序列化/克隆前过助手。**整数样键判据写死**：canonical 数字串（无符号、无前导零以外的格式照 V8）且 `ToUint32` 回读相等（≤ 2^32−2），js.rs 单点实现 + Node 对拍单测钉死（PR2-05）。
   - JSON.stringify 复刻：js.rs 的 `jsjson`（对象/数组/字符串/整数/布尔，键序如上；字符串转义同 JS：`"` `\` 与控制符，`\b\t\n\f\r` 短转义其余 `\uXXXX`，非 ASCII 原样）。SPAWN/REPLACE 的 aspects/attrs pair 以**不透明 String** 存取（ENG-08），jsjson 的输出与 Node 对拍单测钉死。整数域已核实：记录数值面 = id/advancedMin/startMin/rng/int/delta/dur/pair.v，全为整数（tReal 恒 `string|null`），jsjson **不需要**浮点最短表示算法（PR2-04）。
8. **正则字符类通用规则（PR2-03，适用全部移植正则）**：
   - `\s` → 显式 JS 空白类（js.rs 单点常量 `JS_WS_CLASS`）：`[\t\n\v\f\r \u{a0}\u{1680}\u{2000}-\u{200a}\u{2028}\u{2029}\u{202f}\u{205f}\u{3000}\u{feff}]`；`\S` → 同类的**否定** `[^ …]`，不得漏。
   - `\b` → `(?-u:\b)`（JS `\b` 是 ASCII `[A-Za-z0-9_]` 边界；Rust 默认 `\b` 是 Unicode 词边界，非 ASCII 名字后跟 `stackable` 时判定相反）。
   - 适用清单 = core 23 使用行 + script.ts ~40 使用行 + lenient.ts 4 处 + query.ts:43/48 内嵌 `\s*`——**逐条见附录 A**，移植时按表核对，不得凭记忆。
   - `.` 语义差（JS `.` 不匹配 `\r`/`\u2028`/`\u2029`，Rust 只不匹配 `\n`）：本仓全部 `(.+)` 作用在 `split('\n')` + trim 后的单行文本上（script.ts:99 只替换 `\t`），`\r` 面实际不可达——**在 script.rs 顶部一行注释冻结此论证**，禁止在别处复用这些模式于多行输入（PR2-03）。
   - 三处高危语义冻结（Q2/Q8/Q9）：谓词 tokenizer 静默吞字符、JS `\s` 集合、DUR_RE 交替序。
9. **词法器在码元上运行** — lexer.ts:25–66（CORE-09）。`Lexer` 持 `Vec<u16>`（`s.encode_utf16().collect()`）与码元游标；`src[i]`、错误 offset 天然与 JS 一致；NAME_RE/NUM_RE/DUR_RE/空白/数字五件**手写**判定（§2.3）；`\X` 取字面下一码元；尾反斜杠分支 `j+1` 越界时 `out.push_str("undefined")` 后照常抛 unterminated（TS 同型：中间值不可观察，行为等价）。Tok 文本出界转 `String::from_utf16_lossy` 并注释：孤立代理项名字不可表示，golden 集不覆盖（文档化分歧）。
10. **JS 三值/truthy 语义** — predicate.ts:452–454（cmp 任一 undefined → false）、:499–505（strict 仅在**角色绑定数为 0** 时抛；绑定存在但全死 → 静默 false，:492–498 dead-stamp，读取路径见 §2.6.2）、:428–435（`every([])` 空真、`any([])` 空假，Rust `iter().all/any` 同语义）、query.ts:81（`state:!x` 即 `v !== true`——字符串 "false"、0 都算未封印）、query.ts:84（`===` 类型不等即 false：`enum AttrValue` 的 PartialEq 跨变体自然为 false）、engine.ts:159（`??` 保 0：`step.start_min.unwrap_or_else(|| abs_min(...))` 用 Option 非 0 哨兵）。`Option<i64>` + `and_then` 风格，**"缺失"的两种纪律（when: 读 false vs strict 抛）逐点对应不统一**（ENG-07 must）。
11. **哨兵值模型（reach）** — REACH-06/13：`MANY = 1e9` 用 i64 精确常量（参与 `min(count,32)` 权重帽 L645、`cap !== MANY` 导出门 L1231、`cap > idx` L1929–1934）；真 Infinity（freezeAt/freezeDist/firstCross 比较）用 `Option<i64>`/`i64::MAX` 并保留比较方向（L888/971/909–911/1178）；`'\u0000derived'` 哨兵键用 `enum MintKey::Bare/Blinded`，但保持三性质：占 avail/byName 一个键、槽查询可中、`exists(NAME)`/`count(name:)` 名字门不中（L1336/1341/1878–1888/1902–1907）；firstCross 的 `d===0 → 1440` 分支勿漏（L684–687）；firstFreeze 实为 Set（L878/885，REACH-13）；slotSupport 的 cap=4 与 `min(count===MANY?cap:count, cap)` 逐字保留（L1015–1023）。
12. **测试侧 LCG 与状态签名** — LCG：`(s ^ 0x9e3779b9) as u32`，`s = s.wrapping_mul(1664525).wrapping_add(1013904223)`（u32 wrapping 与 Math.imul+>>>0 位精确，REACH-14）。三个种子入口保留：fuzz XOR 0x9e3779b9、scale `lcg(0xc5)`、explorer `'NIGH'`=0x4e494748（TEST-04）。LCG 的 `r()/0x100000000` 与 `floor(r()*n)` 按 f64 复刻。**sig() 字段清单属冻结面**（TEST-07/验证 04 must#9）：now、**rng**（`g: s.rng`，fuzz-lib:253，漏了会静默丢子树）、ended、entities(name,kind,aspects,attrs→container)、relations、fired、decks、windows(verb,start,end,recipe,refs,locked)；**排除** beginId/clause（"exploration-order artifacts"，:268–271）。
13. **format∘parse 往返（law 2 承重墙）** — engine.ts:344–356、1027。完整移植 format+parse 并先建逐字节 round-trip 测试（canonical() 助手，TEST-02）；`catch { clauses = [] }` 静默保留（Q6）。**选方案 (a) 忠实移植，不做内存直传**（验证 ENG-06 must）。
14. **顺序/原子性矩阵（ENG-10/11/12）**：每条落一个 Rust 排序测试；**W3 新排序测试的期望值一律从 W4 golden-dump 的锚点场景产物导出**（记录序抄 dump 实测，不手推——PR1-01），golden 锚点场景清单见 §4-W4-A：
    - advance pass：due 窗口按 `(end, beginId, clause)` 全序（engine.ts:1073）；completion ≤ 边界先发、slot 事件在后（1076、注释 1081–1086）；slot cursor 不跳窗口跨度（同一边界只打一次）。
    - 过期快照一次、无级联（1062–1067）；本 pass 造的 expires 下个 advance 才解析；过期序 = entities 插入序（1065→1238）。
    - fireSlot：authored triggers 声明序（1228）→ expiries 快照序（1238）；authored `on expire of` 按名首条匹配（1252）。
    - per-clause roll 绑定：每子句 `sctx.roll = None` 复位（233–235）；list 内 last-wins（775）。
    - 首 ENDING 生效后续 no-op（735）；REFUSE 丢弃整个 draft + **id 回收**：拒绝行复用 firstRecordId、下轮同 id 重放（257–262、1070）——禁止改单调全局 id（foldUpTo 的 `<` 过滤依赖它，fold.ts:15–17）。
    - 只有 wait 行移动 now；wait 跨度必须分区时间（956–963 断言保留）；complete 锚定 `w.start`，tGame 回退分支保留（975–978）；窗口完成失败只拒该窗、窗口仍关（1154–1168）。
    - adv addTarget 扩展落到 wait 跨度（1085–1086、1099–1100）；engine.ts:1058 的 `toMin === now && !hasDue` 活分支保留。
15. **日志 serde** — store.ts:24–27（saveLog 逐 record `JSON.stringify` 按行拼接）。每种 record kind 一个 serde struct，字段序 = TS 字面量序（makeBegin/Complete/Wait/EventRecord 四个构造点，PR2-10 已核与接口序一致）；`Option` 字段 skip（undefined 丢弃语义）；ActionRecord 全 plain（Ref/Arg/Outcome 无 Map/Set，ENG-20 已核），serde 直映射。验收：`fold(loadLog(saveLog(log)))` 与 TS 侧同日志逐字节相等。
16. **宏展开逐字节** — parse.ts:654–655, 670–838（CORE-20）。`{param}` 插值单趟不重扫（regex `replace_all` + 闭包与 JS `/g` replace 同为单趟不重叠）、只匹配精确拼写（`{ x }` 报 undeclared）；FN_RESERVED_TOKENS 七 token（actors/object/with/at/about/to/roll）原样穿过；delta 符号折入绑定（722–728）；substExpr int/delta/dur→int 三分支（825–838）；argSpelling 的 str/pair **不加引号**（与 formatArg 故意不同，CORE-13 标红）；MAX_FN_DEPTH=8、环在 load 期拒（content.ts:545–588）。
17. **谓词双前端保留拼串形** — predicate.ts:141–161（predViewFromToks 词邻插空格 + span）、269–273/339–340（span substring 还原内层原文）、273/340（比较符压空格 replace，`$1` 用 regex crate 捕获组展开）。保留"拼串 + span"形，封装为可测函数；**不重构为 Tok 直解**（错误文本会漂移，CORE-21）。
18. **clock** — clock.ts:16–19/70–72/80–81。`div_euclid/rem_euclid`（非负输入与 Math.floor 一致）+ `debug_assert!(m >= 0)`；上界 47 写成 `HALFHOURS_PER_DAY - 1`（等值重构，clock.ts:35、parse.ts:122、HALFHOURS_PER_DAY=48 clock.ts:11）；三处裸 `* 30` 魔数（engine.ts:1239/1279/1287）可换 MIN_PER_HALFHOUR 常量（等值）；crossedSlots 不移植（死代码）。parseDuration 的乘法溢出按 §3.5 分叉声明。
19. **数词与整数域** — i64 域结论成立（记录数值面全整数，PR2-04 已核）；formatArg 用 i64 Display（对 \|n\| < 1e21 与 JS 整数打印逐字节相同）；`-0`→`+0` 分支显式保留（Q16）。**改口（PR2-04）**：≥1e21 的**输入字面量可达**（词法接受任意位数数字），只是无测试钉住——按 §3.5 的 f64 中转 + checked 溢出文档化分歧处理，不写"不可达"。
20. **Game/wait/lenient 承重语义** — play.ts:184–192（wait 的 ceil-to-halfhour 量化 + now+60，UI-10）；lenient.ts:37–48（canonical 逃生舱：`/^\d+:\d+\s*@/` 或含 `->` 整行走严格 parse，错误消息变原始 parser message，UI-11；`\s` 按 §3.8 转写）；lenient.ts:59–66（lone-clock 纯推进 `clauses: []`）；lenient.ts:127–147（with→to/actors 路由）；§9.38 oldest-copy 寻址在 engine/fold，Game 只记账 turnLengths（play.ts:44/69/161–167，undo 截断点无法从 log 后缀恢复——注释明示，必须保留记账字段）。
21. **suggestions() 排序（localeCompare，PR2-02）** — play.ts:238 `.sort((a,b) => rank(a)-rank(b) \|\| a.name.localeCompare(b.name))`。`localeCompare` 是 ICU 本地化排序，与 Rust 码点序不同。**裁定**：Rust 用近似比较器 `cmp_ascii_ci`（双方 `to_ascii_lowercase()` 后 `str::cmp`）——本仓名字域为大写 ASCII 实体名与小写英文词混排，近似序预期与 ICU 序在现有内容上一致；**近似面必须钉进差分**：W4 golden-dump 对 nightjar 胜路若干状态快照 `suggestions()` 全列表（TS 产出），Rust 逐字节对拍（script-play.test.ts:421 的 `s[0]==='read THE MANIFEST'` 断言随测试移植双保险）。**禁止**现在引入 ICU collation crate（§2.3 依赖冻结）；未来出现非 ASCII 名字再升 ICU（另立议题）。

---

## 4. 分波执行计划

> 每波 = 一组可独立 `cargo test -j 4` 全绿的交付。**"全绿"= 迄今已移植到 Rust 侧的全部测试绿**（未移植的 TS 测试不在 Rust 侧存在，不算红；P0 全集 §5.3 是终局门而非每波门——PR3-06）。每波全绿 + §7 规则 13 的 review 门通过才进下一波。所有波遵守 §2.4 内存纪律。波内子任务切分见 §4.7。

### W0 — 脚手架 + RNG 向量（不依赖其余代码）

- **范围**：`rust/` crate 初始化（Cargo.toml、profile、Cargo.lock、空 lib.rs）；js.rs 骨架（cmp_utf16、LCG、ChorusError、JS_WS_CLASS 常量）；core/dice.rs（splitmix64/rollDie）+ deck 置换纯函数（hashDeckId/permutePositions/drawFrom，暂放 core/deck_rng.rs，W2 并入 content）；移植 dice.test.ts 与 decks.test.ts 的**纯向量部分**（3+27 骰向量、4 置换向量、die 值域/d2 全覆盖、`seed must be an integer` 消息）；grep 门禁雏形（禁 `rand::` 进 core/engine，禁 dice 路径浮点——向量表本身就是探测器，TEST-12）。
- **前置**：无。
- **产出**：可编译 crate + 向量测试全绿；`docs/rust-port/messages.md` 建立（首批 3–5 条消息）。
- **验收**：dice 30 个常量 + decks 4 个置换逐值相等；负 seed/2^53−1/2^40 路径覆盖。（CORE-04、ENG-05、TEST-05/06/12、验证 04 must#2/#3）
- **内存**：首次 `cargo build -j 4`；四件依赖编译通过即锁定依赖集。

### W1 — core（types/roles/dict/ref/sentence/lexer/parse/fn_expand/format/query/predicate/dice 接线/clock）

- **范围**：src/core 11 个文件全量移植（含 emptyClause；不含 crossedSlots/attrGet/getEntity/sameRef 死代码）；Role/ROLES 下沉 core/roles.rs；parse 拆 parse.rs + fn_expand.rs；predViewFromToks 拼串形；jsjson 完成（aspects/attrs 序列化 + ordinary_own_property_keys + Node 对拍单测）；cmp_utf16 接入 sortRefs/relationKey/refSortKey；词法器五件手写 scanner。
- **波内移植序（PR3-05，范围列表无顺序含义，按此序）**：roles → clock → types（State/IndexMap 纪律落型）→ ref → dict → query → predicate（类型先行）→ sentence → lexer → parse/fn_expand → format → dice 接线。
- **测试移植（3 个文件，PR3-01）**：sentence.test（18）、clock.test（12）、query-predicate.test（25）+ canonical() 助手（tests/common/harness.rs）。vitest→cargo 断言基建（deep-equal via serde_json Value、assert_err_msg）在本波定型。**functions.test 移至 W3**（其 import content/engine/store/settle，W1 无此依赖——PR3-01）。
- **宏展开验收改直测子集（PR3-01）**：不经 compileScript——手工构造 FnTable，直接调 `collect_fn_calls` / `expand_fn_calls` / `format_sentence` 断言展开产物逐字节（含"call site === 手工展开孪生"的可直测子集、`{ x }` 报错、保留 token 穿透、delta `+n` 拼写、深度 8/环错误文本）。经 compileScript→executeStep 的全链路展开验证在 W3（functions.test 移植时）补齐。
- **前置**：W0。
- **产出**：core 全模块 + 3 个测试文件移植版全绿；messages.md 增补本波对应行。
- **验收**：
  - `format(parse(x)) === x` worked examples 全过。
  - 三值边界四测：every([])/any([])/全死绑定/strict 零绑定（CORE-17）。
  - aspect 覆盖冲突、`'a.b.c'` 对称（CORE-18）。
  - arg() 不查 param 键（`SET(myparam = 3)` 报 `expected ')', got '='`）+ spawnArgs 支持（Q3）。
  - **本波所移 3 个测试文件内的 toThrow 逐条过** + messages.md 增补（PR3-06：core 模块的错误消息大多被 decks/conditionals/functions 的断言钉住，而这三文件在 W3，故验收按"所移文件内"口径）。
  - UTF-16 序：一条非 BMP 名字的 sortRefs/relationKey 序测试（合成比较测试，不必进 golden）。（CORE-05、ENG-21）
- **注意**：fn_expand 的 str/pair 拼写两套冻结（CORE-13 标红）；23 使用行正则全部移植勿按"13 处"清单照抄（验证 01 附加发现；clock.ts:52 matchAll 别漏）；词法器五件手写（PR2-08）。

### W2 — content（script.rs + content.rs，脚本文本=数据）

- **范围**：parseScript（缩进敏感逐行解析器；script.ts ~40 处正则按附录 A 转写）+ compileContent（verbs/triggers/prototypes/templates/endings/decks 全量编译与 load 期校验）+ lintScript；entities 一次性传入 compileContent 消除编译后突变（ENG-01，结构改进、行为保持）；stackable 三集合合一构建；OrdinaryOwnPropertyKeys 键序助手接入 4 处 Object.entries 迭代点；recipe id 铸造函数（两个基数变体，Q1）；the-nightjar-flyer.txt 经 include_str! 嵌入 + `compile_script` 冒烟。测试移植：script-play.test 的**编译器子集**（不含 Game/lenient 部分）、lint.test（14）。
- **前置**：W1（fn_expand 接缝类型、dict、sentence/roles、js.rs 的 jsjson+ordinary_own_property_keys 均已就位——PR3-05 核对成立）。
- **产出**：compile_script(the-nightjar-flyer) 得到与 TS 逐字段一致的 CompiledContent；lint 告警文本逐字。
- **验收**：
  - `study.recipe0` 0 基（script-play.test.ts:175 移植）；lint 1 基文案保留（Q1，无测试钉住但冻结）。
  - load 期错误文本逐字：`'TO' is a reserved word`（lint.test.ts:36–39）及 lint.test 全部 14 test。
  - deck 置换经 permutePositions 与 W0 向量同源；drawFrom 三规则（reset 环绕/defaultcard 永续/`deck 'D1' is exhausted` 抛错，content.ts:472–477）。
  - 大小写碰撞/重复声明怪癖保持（Q5）；slot role typo 静默失效（Q4）。
  - meta 解析保持 else 落 parseClock 的 "bad clock" 文案（Q15；不做 meta 枚举化）。
- **finding**：ENG-01/05/13/14/15/16/17/19/22、REACH-04、CORE-14。

### W3 — engine + 会话层（store/fold/engine + lenient/render/Game 无头部分）+ tests/common 基建

- **范围**：
  - store.rs（Log/JSONL serde；原子写；`truncate(len)` 供 undo；initialState 标测试夹具）；fold.rs；engine.rs（executeStep 四阶段、applyOutcome 17 op、applyRecord、resolveDue/fireWindow/fireSlot、registerWindows 往返 + 静默 catch）。所有权模型按 §2.6。
  - **测试基建随本波**（TEST-11 must，最易漏）：tests/common/{snap.rs（按 snap.ts 实况：**5 处** `a<b?-1:1` 排序 + fired `.sort()` + windows 排序浅拷贝，PR2-11 勘误）、settle.rs（**唯一 run 助手**：parallel 的裸时钟分支 + settleStep 系的 idGen 通道 + 可注入字典——验证 04 TEST-01 修正的蓝本，不用 parallel 单版）、seed.rs、explorer_lib.rs（LCG 'NIGH'、WIN_PATH、checkStep、sampledWalkStates、emptyClause 消费点）}。
  - 会话层：lenient.rs（4 处 `\s` 转写）、render.rs、game.rs（Game 全部方法：handleLine/undo/wait/visible/suggestions/story/journal/look/cards/inspect——7 个测试文件锚定，一个不丢；suggestions 排序按 §3.21）。
  - 测试移植（PR3-01/PR3-02 修订后清单）：dice.test 剩余（ROLL 语义/绑定/ROLLED 往返/undo 计数器重构/JSONL 字节恒等/grep gate）、decks.test 剩余（游标 fold/耗尽/livelock/drift 响亮失败）、**engine.test（74）**、**parallel.test（13**，验证 04 勘误）、**functions.test（27，自 W1 移入——PR3-01；补齐 W1 直测子集未覆盖的经 compileScript 全链路展开验证）**、stackable（21）、conditionals（36）、endings（25）、**nightjar.test（21——PR3-02 补入）**、**script-play.test 剩余（lenient input 两个 describe + game loop describe，:246–421——PR3-02 补入）**、nightjar-explorer（4）。
- **前置**：W2（需要 CompiledContent 与 seed.ts fixture 的可编译性）。
- **产出**：P0/P1 主体测试全绿；`fold(loadLog(saveLog(log))) === played` 单实现内自洽（law 1）。
- **验收**：
  - §3.14 顺序/原子性矩阵逐条对应的排序测试全过（parallel.test 为模板）；**排序测试的期望值从 W4 golden 锚点场景导出**（§4-W4-A；W4 未跑前可先用手推值占位，W4 后逐条替换为 dump 实测——PR1-01）。
  - JSONL 字节恒等：同 seed 双跑字节相同（dice.test.ts:917–933 移植版）。
  - 两条 game-over 文案（UI 版 notice 全文 + 引擎版 rejected 短文）各自过测（UI-02）。
  - `suggestions()`/`visible()`/`inspect()` 断言（endings:356、script-play:384/401–407/**421**、stackable:175–176）。
  - applyRecord 的 complete/event 分支只共享 ctx 构造，分派骨架保留（ENG-03 修正）。
- **finding**：ENG-02/03/04/06/07/08/09/10/11/12/13/14/18、UI-01/02/05/06/07/10/11/12、TEST-01/07/10/11/13/15。

### W4 — 黄金日志差分（TS 生成 → Rust 重放比对）

- **范围**：建立跨语言黄金库并让 Rust 全部通过。
- **TS 侧生成**（只**新增**一个只读导出脚本 `tools/golden-dump.ts`，不改 src/ 与 test/ 任何文件；`npx tsx` 单独跑，**不与 cargo 并发**；带 `--out` 参数默认 `rust/tests/goldens`，manifest 存相对路径，JSONL 恒 `\n`——PR2-09）：
  1. **nightjar 主锚**：`Game.fromScript(the-nightjar-flyer.txt, coreDictionary())` + WIN_PATH（explorer-lib.ts:235–245）输入行 → 逐条 `handleLine` → JSONL 全量 + 最终 snap 字符串 + journal 文本。seed 1899。
  2. **确定性 walk**：`sampledWalkStates()`（explorer-lib.ts:275–306）→ 每步 JSONL + snap。
  3. **对抗场景 S1–S14**：**在 dump 脚本内直接复制场景构造代码**（程序化 ContentRaw，不转写脚本文本——PR2-07/PR1-02）→ compileContent → canonical 步 driver（canonical 句可携带 #id ref，覆盖 S 场景里 `probeClause` 的 #id 绑定）→ JSONL。**转写校验在工具内完成**：dump 前对每场景复算原 S1–S14 的 expect 断言（断言逻辑只读移植自 reach-adversarial.test.ts，验证 03 勘误：原断言是 verdict/引擎状态断言，非骰序），全部通过才写 goldens/（PR3-08——不向 test/ 加临时文件，goldens/ 属 §7 规则 10 的产物豁免）。
  4. **引擎排序锚点场景（PR1-01，manifest 场景名显式可辨识）**——每个至少一条 golden，driver 用 ContentRaw + canonical 步（settleStep 等价）精确构造：
     - `anchor-due-tie`：同 `end` 的多个 due 窗口，按 `(beginId, clause)` 并列打破序关闭（engine.ts:1073；现有 S1 只有不同时长窗口，无并列）。
     - `anchor-expiry-insertion-order`：同一 halfhour 多个实体到期，event 记录序 = entities 插入序（engine.ts:1065→1238）。
     - `anchor-on-expire-first-match`：同 slot 多条 authored `on expire of` 声明序首条匹配 + 内建 destroy 共存（engine.ts:1252–1265）。
     - `anchor-refuse-id-recycle`：REFUSE 丢弃 advance 记录 + id 回收重放（engine.ts:257–262/1070；fuzz 虽随机覆盖，此处显式点名）。
  5. **suggestions 快照（PR2-02）**：对 nightjar 胜路若干状态快照 `suggestions()` 全列表（钉 §3.21 近似比较器）。
  6. **fuzz 种子批**：`genScenario(seed)`（reach-fuzz-lib.ts:78–223，导出纯函数）对 seed 1..200，各跑 K=25 个 settleStep（**固定 LCG 动作选择器**，其源码入库并经 manifest 引用，属事实冻结面——评审 1 附四(a)；与 tests/common/reach_fuzz_lib.rs 内选择器**同源**）→ JSONL。
  7. **fold-prefix 模式（PR3-10）**：dump 工具支持对任意 golden 日志的任意前缀做 fold + snap dump（供 §6.2 二分第 2 步使用；仍是只读工具，同一豁免）。
  8. manifest.json：每条 golden 记 { name, script|scenario|anchor, driver(输入行/canonical 步/种子), sha256(jsonl), snap }。
- **Rust 侧**：
  - **移植 tests/common/reach_fuzz_lib.rs（PR3-03，本波落位）**：genScenario + LCG + 同源动作选择器——否则"200 种子重放"无执行前提。
  - `fold(loadLog(golden.jsonl))` → snap 与 manifest 内 TS snap 相等（验 fold；snap.rs 键序规则见 W3）。
  - 同 driver 从头重放 → 产出 JSONL 与 golden **逐字节** `cmp` 相等（验 play；tReal 恒 `null|TS 传入值`，确定性无时钟参与，engine.ts:27/317 已核；**不为验 sha256 引入 sha2**，PR2-09）。
  - 失败即触发 §6.2 二分协议。
- **前置**：W3（Game/lenient/settle/snap 齐备）。decks 的 DRAWN 断言（decks.test 移植版）须已绿——它是内容漂移唯一探测器（ENG-14 must）。
- **产出**：goldens/ 入库（JSONL + manifest + 动作选择器源码）；golden 差分套件全绿；W3 排序测试期望值替换为 dump 实测。
- **验收**：nightjar 9 步胜路的骰序 `[[5,2],[8,4],[14,7]]`、牌序 `['SCANDAL','RUMOUR']`、PORTENT×2/VOID×1（nightjar.test:153/163/347–348）全部在 Rust 重放中逐值复现；200 个 fuzz 种子 ×25 步零字节差；**四个 anchor 场景逐字节差为零**（PR1-01：没有它们，"fuzz 零字节差"不能宣称排序语义逐位一致）。
- **finding**：TEST-04/06/07、ENG-05/10/11/14、CLI-04、REACH-14、dice.test:917–933 判据。

### W5 — reach + 4 个 CLI

- **范围**：reach.rs 按五模块切缝（census/world/fixpoint/paths/render）；**朴素 round-robin 不动点**（verbs 按 content.verbs 序 → sites 按 live 序，逐轮至无变化；**禁止 worklist/事件驱动**，REACH-10 标红）；WorldFacts 显式 `Analyzer<'c>` 持 (content, initial)（worldCache/trigIndexCache 替代；身份键索引化见 §2.6.5）；matchMemo **先不做**（matchQuery 便宜，REACH-07 must 的保守解；后续做则键含完整描述符含 aspects）；siteProven 的 17 参收敛为 `&Ctx`（REACH-02）；**共享遍历器（REACH-01 + 验证 03 修正细节，PR1-04）**：walkSites 对每个 IF 臂分配新 seg、REFUSE/throwing 记在**所在 seg** 上——这是行为的一部分；共享遍历器的回调须同时供给 content 遍历与 seg/flag 记账两种消费形态，不得把 seg 分配丢在遍历器之外；哨兵模型与 cap 公式按 §3.11；siteReason 的 `!initNames.size` 分支保留（Q17）；死代码（外层 startSlot/cyclic、analyzeCard 未用解构、siteChain 的 content 参）不移植。**移植 tests/common/reach_scale_lib.rs（PR3-03）**：buildCsScale 等价物。
  CLI 四件：reach（stdout 逐字节 golden 冻结，含 renderPath 4 空格缩进/`path i/n:`/`verdict: X (existence: y)`；`--all` 名单与 UTF-16 排序；Q15 的 --demand/退出码现状冻结）、reach-fuzz（flags 200/7/400/1、FAIL→stderr/进度→stdout、soft 行也在 stderr、退出码 1 iff failure、坏种子空格列表）、reach-scale（线程+channel；`--worker i k P R` 分片 `idx % k===i` 与 NameStat 字段保留）、lint-scripts（恒 0 语义 + 现状缺失输入 exit 1 冻结）。
- **前置**：W3（content/engine API + tests/common/settle.rs）；W2 的 CompiledContent；W4 的 reach_fuzz_lib.rs 与 golden 基建（fuzz/scale 差分部分排在 W4 验收后——PR3-03）。
- **产出**：reach 全模块 + 4 bin；测试：reach.test（32）、reach-adversarial 的分析器侧断言（引擎侧确认已在 W3/W4 覆盖）、reach-fuzz.test（100 种子）、reach-scale.test（CI 缩小版 400×400）。
- **验收**：
  - TS 侧 golden-dump 追加 reach-cli `--all` 与逐卡 stdout 快照（nightjar 脚本 + genScenario 抽样世界），Rust 逐字节相等（CLI-02 must）。
  - 同 (content, State) 两次分析输出逐字节一致（REACH-03）。
  - reach-scale 同参世界 TS/Rust 各建一次：**verdict 分布一致 + NameStat 非计时字段（name/verdict/existence/roots/nodes/depth）逐字段一致；`ms` 仅作量级参考**（PR1-03：跨语言对计时字段逐字段一致不可满足）。
- **finding**：REACH-01–08、11、12、14、15、CLI-01–05。

### W6 — 简化 UI（line REPL）+ demo + 收尾

- **范围**：
  - `bin/chorus.rs`：最小 line REPL——读脚本文件（CLI 参数）→ `Game::from_script`（标题取 `meta.title`，UI-04）→ 循环"读一行 → 单一 dispatcher（`inspect` 前缀 + 元命令 undo/wait/story/journal/look/cards/inspect/quit + 其余进 handleLine）→ 打印 proseLines"。以 runPlain（tui.tsx:450–469）为骨架原型（UI-03：路由收敛到一处）；`« … »` 回显格式保留（play.ts:73）；非 TTY 也可运行（`IsTerminal` 仅作提示）。
  - demo 移植：tests/demo.rs（fold-verify 冒烟，UI-13）。
  - 收尾：pub use prelude 定稿（CORE-03）；`docs/rust-port/messages.md` 覆盖全部 192 条；grep 门禁定稿；README 附录"已冻结怪癖清单"（引用 §1.2 表）。
- **前置**：W4/W5。
- **产出**：可玩 REPL；全测试套件绿。
- **验收（脚本化，PR3-09）**：管道喂入固定输入行序列（demo 6 个 canonical 步 + nightjar 胜路输入行），stdout 与 **examples/demo.ts 的 journal/prose 黄金输出**（W4 一并生成，属 golden 产物）`diff` 为空；`cargo test -j 4` 全绿；tui.tsx 的可丢弃清单确认零残留。
- **finding**：UI-03/04/05/06/09/13/14/15、CORE-03、TEST-12。

### 4.7 波内子任务切分与认领（PR3-11；"编辑可并行、构建串行"前提见 §7 规则 14）

| 波 | 子任务（T 编号 = 认领粒度） |
|---|---|
| W0 | T0.1 crate 脚手架 + Cargo.toml profile + 门禁雏形；T0.2 js.rs 骨架 + messages.md 建立；T0.3 dice.rs + deck_rng.rs + 全部向量测试（波次门：T0.3 全绿） |
| W1 | T1.1 roles+clock+types（按 §4-W1 序）；T1.2 ref+dict+query；T1.3 predicate+sentence；T1.4 lexer+parse+fn_expand+format；T1.5 js.rs 完成（jsjson/OwnPropKeys 对拍）+ 测试基建 + 3 个测试文件。T1.1→T1.2→T1.3→T1.4 强串行；T1.5 部分可并行 |
| W2 | T2.1 parseScript 行段解析器 + meta/seed + lintScript；T2.2 compileContent（fn_expand 接线、recipe 铸造、decks、stackable 三集合一、OwnPropKeys 接入）；T2.3 lint.test + script-play 编译器子集 + nightjar 冒烟。T2.1/T2.2 可两人先后并行，T2.3 收口 |
| W3 | T3.1 store.rs serde + Log；T3.2 fold.rs；T3.3 engine.rs 主干；T3.4 resolveDue/fireWindow/fireSlot 顺序机制 + §3.14 排序测试；T3.5 tests/common（snap/settle/seed/explorer_lib）；T3.6 lenient+render+game；T3.7 测试移植三包：A=engine+parallel+functions，B=decks 剩余+dice 剩余+stackable+conditionals，C=endings+nightjar+nightjar-explorer+script-play 剩余。T3.3→T3.4 串行；T3.5 完成后 T3.7A/B 与 T3.6 可交错 |
| W4 | T4.1 golden-dump.ts + goldens 生成 + manifest（纯 TS 侧，与 cargo 天然错开）；T4.2 Rust 重放 harness + reach_fuzz_lib.rs；T4.3 失败二分循环（§6.2）。T4.1/T4.2 可并行开发 |
| W5 | T5.1 census+world（Analyzer/WorldFacts）；T5.2 fixpoint（朴素 round-robin，禁优化）；T5.3 paths+render；T5.4 四个 bin + reach_scale_lib.rs + 四个测试移植。T5.1→T5.2→T5.3 强依赖链 |
| W6 | T6.1 bin/chorus.rs + 脚本化 REPL 冒烟；T6.2 demo.rs + prelude/messages.md/门禁收口 + README 怪癖附录 |

---

## 5. 测试与验收策略

### 5.1 总原则

**日志字节是最终裁判**（§1.1）。全部验收是它的投影：向量表 → 单实现 fold/serde 自洽 → 跨语言黄金差分 → P1 全量 → fuzz 骨架长期跑。**"每波全绿"指迄今已移植的测试全绿**（PR3-06）；每波另须过 §7 规则 13 的 review 门。

### 5.2 vitest → cargo test 映射（harness.rs 一次定型，W0/W1 落地）

| vitest | Rust |
|---|---|
| `describe/test` | `mod` + `#[test]`，**保留 TS 测试名**（snake_case）作追溯锚 |
| `expect(a).toEqual(b)` | `assert_json_eq(&Value, &Value)`（serde_json Value 深比较；测试数据一律整数字面量，避免 Number(1) vs 1.0 假阳性） |
| `expect(s).toBe(str)` / `toMatch(/re/)` | `assert_eq!` / `regex::Regex::is_match`（模式文本照抄） |
| `expect(f).toThrow(/re/)` | `assert_err_msg(res, r"re")`：取 `ChorusError.msg` 跑 regex；模式照抄 JS 正则，无比需特性（TEST-05 已核） |
| 模块级 `const dict = coreDictionary()` + registerVerb | 每测试函数新建 Dictionary（与 TS 每文件隔离等价，TEST-10——**这个选择影响 reserved-word 冲突类测试行为，必须每测新建**） |
| `snap(fold(records, base)) === snap(played)` | snap.rs 逐字段复刻（按 snap.ts 实况：**5 处** `a<b?-1:1` 排序 + fired 字典序 + windows 排序浅拷贝，PR2-11） |
| `run()` settle 适配器（8 处漂移副本） | 唯一 `run` 助手：parallel 裸时钟分支 + idGen 通道 + 可注入字典（验证 04 TEST-01 修正蓝本） |

### 5.2.1 测试文件 → 波次 → Rust 文件完整映射（PR3-02，25 个文件逐一落位）

| TS 文件 | 波次 | Rust 落点 |
|---|---|---|
| dice.test.ts | W0（向量/门禁）+ W3（其余） | tests/dice.rs |
| decks.test.ts | W0（置换向量）+ W3（其余） | tests/decks.rs |
| sentence.test.ts | W1 | tests/sentence.rs |
| clock.test.ts | W1 | tests/clock.rs |
| query-predicate.test.ts | W1 | tests/query_predicate.rs |
| lint.test.ts | W2 | tests/lint.rs |
| script-play.test.ts | W2（编译器子集）+ W3（lenient/game loop） | tests/script_play.rs |
| engine.test.ts | W3 | tests/engine.rs |
| parallel.test.ts | W3 | tests/parallel.rs |
| functions.test.ts | W3（自 W1 移入，PR3-01） | tests/functions.rs |
| stackable.test.ts | W3 | tests/stackable.rs |
| conditionals.test.ts | W3 | tests/conditionals.rs |
| endings.test.ts | W3 | tests/endings.rs |
| nightjar.test.ts | W3（PR3-02 补入） | tests/nightjar.rs |
| nightjar-explorer.test.ts | W3 | tests/nightjar_explorer.rs |
| snap.ts / settle.ts / seed.ts / explorer-lib.ts | W3（lib） | tests/common/{snap,settle,seed,explorer_lib}.rs |
| reach-fuzz-lib.ts | W4（lib，PR3-03） | tests/common/reach_fuzz_lib.rs |
| reach.test.ts | W5 | tests/reach.rs |
| reach-adversarial.test.ts | W5（分析器侧；引擎侧确认 W3/W4） | tests/reach_adversarial.rs |
| reach-fuzz.test.ts | W5 | tests/reach_fuzz.rs |
| reach-scale-lib.ts / reach-scale.test.ts | W5 | tests/common/reach_scale_lib.rs、tests/reach_scale.rs |
| examples/demo.ts | W6（+W4 生成其黄金输出） | tests/demo.rs |

### 5.3 P0 最低集（不通过不得宣称行为一致）

dice、engine、parallel（13）、sentence、clock、decks、nightjar + 基建 snap/settle/seed（TEST-11：不在 src/，最易漏）。P1：stackable、conditionals、functions、endings、query-predicate、script-play、lint、nightjar-explorer。P2：reach.test、reach-adversarial（引擎侧确认部分在 W3/W4 提前消化）、reach-fuzz（骨架长期跑）、reach-scale（CI 缩小版）。

### 5.4 差分与门禁

- **向量先行**（W0）：3+27 骰向量、4 置换向量，语言无关照抄。
- **黄金差分**（W4）：§4-W4 清单（nightjar seed 1899、确定性 walk、S1–S14、fuzz 200 种子、**四个排序锚点 anchor 场景**、suggestions 快照）。判据两级：JSONL 逐字节（`cmp`）、snap 串相等（fold）。
- **fuzz 骨架长期跑**：genScenario+LCG 逐位移植后，TS/Rust 同种子流逐 record 比对可随时重放；异常报 seed 号（runFuzzSeed 协议平移）。
- **grep 门禁**（TEST-12）：全测试扫描 core/engine 源码禁 `rand::`/`rand::random`；dice 路径禁浮点由向量表天然探测（一个 f64 参与即红）。
- **消息对照表**：docs/rust-port/messages.md 随波增补，W6 收口覆盖 192 条。

---

## 6. 风险与回退

### 6.1 最难移植点 Top5 与缓解

| # | 风险 | 依据 | 缓解 |
|---|---|---|---|
| 1 | **JSONL 逐字节（serde 字段序 + undefined 丢弃 + JS 键序/整数样键 + jsjson 转义）** | ENG-04/08/20、REACH-04、PR2-05 | jsjson 与 Node 对拍单测前置（W1）；record struct 字段序逐一对照构造点；OwnPropKeys 助手覆盖 clone/spread 点；W3 首个 fold 自洽测试即暴露；golden 差分兜底 |
| 2 | **引擎顺序/原子性矩阵**（交错序、快照、id 回收、wait 分区——多数无现成测试） | ENG-10/11/12、PR1-01 | 每条锚点落成独立排序测试（W3，期望值从 golden 锚点导出）；W4 四个 anchor 场景提供跨语言证据；adversarial 黄金覆盖边角 |
| 3 | **错误消息 192 条逐字 + 顺序敏感罗列** | CORE-16/TEST-05/14 | ChorusError 单通道 + 消息表随波对照；罗列序用 IndexMap 声明序不取巧 |
| 4 | **reach 不动点 proven 序与日程依赖**（chain/paths byte-stable 全系于此；反单调 count 门） | REACH-03/10 标红 | 朴素 round-robin 唯一日程；禁优化准入判据（验证 03"性能优化准入判据"全文采纳：先朴素对齐，之后任何优化在同一差分套件重跑，一字节差即回退）；matchMemo 先不做 |
| 5 | **format∘parse 往返 + 词法器码元怪癖**（互逆是引擎承重墙；UTF-16 索引/字面转义/手写 scanner） | ENG-06、CORE-09、PR2-08、Q6/Q10 | W1 canonical() 往返测试先行；lexer 在 Vec<u16> 上运行 + 五件手写 scanner；`catch { clauses = [] }` 冻结 + 一条专测 |
| 6 | **别名/身份语义错译（bindings 克隆、身份键选错）** | PR2-01 | §2.6 模型写死 + §7 规则 13 review 门 (b) 迭代序/所有权审计 |

### 6.2 差分失败时的二分定位协议

1. **逐字节 diff** 首个分歧 JSONL 行：定位 record id / kind / 字段。
2. **回退一步重放**：截取分歧行之前的日志，TS/Rust 各 `fold` 得 State，`snap` 对拍 → 分歧在 fold 还是 play。TS 侧能力由 golden-dump.ts 的 **fold-prefix 模式**（§4-W4 第 7 条）提供。
3. **按模块插桩 dump**：
   - fold 分歧 → 对该行 applyRecord 前后各 dump snap 差量（哪个字段漂移：rng/decks/windows/entities 直指模块）。
   - play 分歧 → executeStep 四阶段各 dump（advance 记录列表 / 选谱结果 / REFUSE 预演骰点 / BEGIN 后 windows）；`ROLLED` 行的 ctrAfter 直接验 RNG 链。
   - 编译分歧 → TS golden-dump 增加 CompiledContent JSON 导出（纯数据可序列化），与 Rust `compile_script` 结果做 Value 深比较（先比 verbs/slots/recipes 结构，再比 deck 排列数组）。
   - 文法分歧 → canonical() 表逐例比对 + 对该输入分别 dump parse AST / format 输出。
   - **插桩形态（PR3-10）**：Rust 侧插桩走临时分支或 env 门控（如 `CHORUS_DEBUG_DUMP=1`），修复后删除、不入库。
4. **最小化输入**：分歧输入行二分裁剪（对 walk/fuzz 场景按步二分），缩到 ≤3 步可复现后按模块归属开修复任务（**只修移植 bug，不改行为语义**）。
5. **失败报告必备字段（PR3-10）**：seed（或 golden 名）、步号、首个分歧 record 的 id/kind、两侧该行原文——跨 agent 协作排查的最小信息集。
6. 回退规则：某波差分不过 → 该波不合并下一波；热修以最小 diff 进行，禁止顺带重构。

---

## 7. 执行者须知（后续执行 subagents 硬性规则）

1. **不改行为**。任何让现有 TS 测试语义（除 §1.3 范围裁定删除的 TUI 项外）变化的 diff 都是错误。错误消息文本、日志字节、输出顺序是 API。
2. **怪癖冻结**：§1.2 表 Q1–Q17 逐条在对应 Rust 代码处写 `// QUIRK(<finding-id>): <一句说明>`；发现报告未列的可疑行为 → 停下记录，不修复。
3. **来源注释**：每个移植函数/结构头部 `// ts: <file>:<line>`；大段逻辑逐段标行。**移植时对照验证报告该条的"修正表述"**（如 REACH-01 的 seg 语义、ENG-18 的活分支）——分析报告原文可能与验证修正有出入，以验证为准。
4. **每完成一个子模块即跑对应测试**（§4 各波"验收"），不攒批。测试移植与实现同 commit。
5. **内存纪律**（§2.4）：一律 `-j 4`；`debug=0`（test 行号表除外）；**禁止 vitest/tsx 与 cargo 并发**；golden 生成与比对分两步跑；release 仅 bench 用。
6. **依赖冻结**：只许 §2.3 四件（indexmap/serde/serde_json/regex）。禁止引入 rand/async/chrono/rayon/sha2/ICU collation；新依赖需在计划层签字。`Cargo.lock` 入库，升级依赖视为新依赖。
7. **插入序与所有权纪律**：任何新 Map/Set 字段默认 IndexMap/IndexSet；在迭代序可观察处出现 std HashMap 视为 bug。JSON 序列化只走 jsjson 或定序 serde struct。**遵守 §2.6**：bindings 存 id 不克隆 Entity；身份键索引化；禁 Rc/Arc/RefCell/Cell 入引擎/reach 数据面；数字字面量禁 `parse::<i64>()` 直解（§3.5）。
8. **不做未列的重构**：死代码不搬（清单见 §1.2"明确禁止"节与 ENG-18 修正）；不动点不优化；matchMemo 不做；缓存重构仅限 REACH-07 的 Analyzer 形态。性能问题记录不处理（对齐前）。
9. **commit 粒度**：一个子模块（如 core/lexer.rs）+ 其测试 = 一个 commit，message 格式 `port(core/lexer): ts src/core/lexer.ts, refs CORE-08/09`；golden 入库单独 commit（附 manifest sha）；每波收口一个 tag（`rust-port-w0` …）。
10. **TS 侧零修改**：src/、test/、scripts/ 一律不动；唯一例外是新增只读工具 tools/golden-dump.ts（W4）与 goldens/ 产物（含 scenarios/锚点场景、动作选择器源码、demo 黄金输出）。
11. **panic 纪律**：引擎路径禁 `unwrap`/`expect`（测试代码允许）；不可达用 `debug_assert!`/`unreachable!`，并注释 TS 依据行。
12. **遇歧义即停**：本计划与 9 份报告未覆盖的行为问题（尤其报错文本的空格/标点）→ 打开源文件对照原文，仍不明则记录后按 TS 原文逐字符照抄，不自行发明。
13. **波次收口 review 门（PR3-04，强制）**：每波 tag（规则 9）前由**非本波作者的 reviewer** 执行并产出一段 review 记录（随 tag 入库），检查清单：
    - (a) 按 `// ts:` 行锚对本波新移植文件与 TS 原文逐段对照（core/engine 全量，其他抽样）；
    - (b) 迭代序/所有权审计：`grep -n "HashMap\|HashSet\|BTreeMap\|Rc<\|RefCell\|clone()" 本波文件`，逐一核对是否处于序可观察点或违反 §2.6（对照 §3.7）；
    - (c) §1.2 QUIRK 注释在本波范围模块内逐条在位；
    - (d) docs/rust-port/messages.md 增量行数 = 本波移植 toThrow 数；
    - (e) grep 门禁（`rand::`、引擎路径 `unwrap`/`expect`）通过；
    - (f) `git diff --stat` 证实 src/、test/、scripts/ 零改动（goldens/ 与 tools/golden-dump.ts 除外）；
    - (g) "有意分歧"登记表（原子写、loadLog 行号、lossy 代理项、§3.5 溢出分歧、§3.21 近似比较器）逐条有据可查。
    本门是不可省略的防线：lint 1 基文案（Q1）、bare inspect（Q11）、罗列序字母序巧合（验证 01 附加发现）等行为**无测试钉住**，"测试全绿"对它们零约束力。
14. **多 subagent 协作纪律（PR3-07）**：波内允许多 agent 按 §4.7 的 T 编号认领，认领表注明文件边界、互不重叠；commit 由认领者提交各自文件；**cargo 构建/测试全局串行**，由 wave integrator 一人执行并宣布波次门（含规则 13 的 review 调度）；两个 agent 不得同时持有 cargo。

---

## 附录 A：正则逐条转写清单（PR2-03）

通用规则（§3.8）：`\s`→`JS_WS_CLASS`（显式 JS 空白类）、`\S`→其否定、`\b`→`(?-u:\b)`；regex crate 默认 leftmost-first，交替序照抄即等价。标记：**H**=手写码元扫描（regex 不吃 Vec<u16>）；**R**=regex crate on &str；**S**=纯字符串操作（不用 regex）。

### A.1 core（23 使用行 / 25 字面量）

| 位置 | 原模式要点 | 处理 |
|---|---|---|
| clock.ts:32 | `/^(\d+):(\d+)$/` | R |
| clock.ts:52 | matchAll `(\d+)(halfhour|h|m|d)` | R（captures_iter） |
| dict.ts:72 | `/^[A-Z][A-Z0-9_]*$/` | R |
| lexer.ts:13 | NAME_RE | **H** |
| lexer.ts:14 | NUM_RE | **H** |
| lexer.ts:15 | DUR_RE（交替序 halfhour\|h\|m\|d 承重，Q9） | **H**（保交替序） |
| lexer.ts:26 | `/\s/` 空白跳过 | **H**（js_is_ws，Q8） |
| lexer.ts:45 | `/[0-9]/` | **H** |
| query.ts:18 | split(/\s+/) | R（JS_WS 类切分） |
| query.ts:37 | `/[<>=!]/` | R |
| query.ts:43 | aspect term，内嵌 `\s*(>=\|…)\s*` | R（`\s` 显式类） |
| query.ts:48 | state term，内嵌 `\s*` | R（同上） |
| query.ts:61 | `/^-?\d+$/` | R |
| predicate.ts:155 | `/[A-Za-z0-9_]$/` + `/^[A-Za-z0-9_]/`（一词两字面量） | R |
| predicate.ts:172 | tokenizer 字符集 /g exec 循环（**Q2 静默吞字符**） | R + 手写 exec 循环保留跳过语义 |
| predicate.ts:273 | 比较符压缩 replace `$1`，内嵌 `\s*` | R（replace + 捕获组展开） |
| predicate.ts:294 | `/^\d+$/` | R |
| predicate.ts:323 | `/^\d+$/` | R |
| predicate.ts:340 | 同 273 | R |
| ref.ts:16 | SAFE_NAME | R |
| ref.ts:37 | `replace(/"/g, '\\"')` | S |
| format.ts:98 | quote 两 replace（一行两字面量） | S |
| parse.ts:770 | substString `/\{(\s*)(…)(\s*)\}/g` | R（replace_all + 闭包；`\s` 显式类） |

### A.2 script.ts（≈40 使用行；全部 R，输入是 trim 后单行 &str）

| 行 | 模式要点 | 转写 |
|---|---|---|
| 99 | `/\t/g` → 两空格 | S |
| 106 | meta `^(script\|player\|goal\|start\|seed):\s*(.+)$` | `\s` |
| 113 | seed `/^-?\d+$/` | — |
| 123 | section 头 /^(entities\|…\|decks):$/ | — |
| 171 | slot `^([a-z]+):\s*(.+)$` | `\s` |
| 205 | prototype `^(\S+)\s+is\s+([A-Za-z_.]+)\b\s*(.*)$` | `\S`+`\s`+`\b` |
| 210 | `of\s+(\S+)$` | `\s`+`\S` |
| 224 | `^stackable\b\s*` | `\b`+`\s` |
| 230 | `(?:^\|\s+)(?:at\|in)\s+(\S+)\s*$` | `\s`+`\S` |
| 231 | `(?:^\|\s+)with\s+(.+)$` | `\s` |
| 240 | aspect spec `^\s*…\s*$` | `\s` |
| 257 | prototype `^(\S+)\s+is\s+([a-z]+)\b\s*(.*)$` | `\S`+`\s`+`\b` |
| 264 | `^stackable\b\s*` | `\b`+`\s` |
| 269 | `(?:^\|\s+)with\s+(.+)$` | `\s` |
| 270 | `(?:^\|\s+)deriving\s+(.+)$` | `\s` |
| 276 | spec 同 240 | `\s` |
| 290 | `^\s*name\s+from\s+object\s*$` | `\s` |
| 292 | derive `^\s*…\s+from\s+object\.…(?:\s*-\s*(\d+))?` | `\s` |
| 309 | expiry `^(\S+)\s+on\s+expire\s+of\s+(\S+)\s*->\s*(.+)$` | `\S`+`\s` |
| 311 | trigger `^(\S+)(\s+once)?\s+at\s+(hour\|halfhour)\s+(\d+)\s*->\s*(.+)$` | `\S`+`\s` |
| 325 | fn `^(\S+)\s*\(([^)]*)\)\s*->\s*(.+)$` | `\S`+`\s`（补类 `[^)]` 直接照抄） |
| 329 | fn `^(\S+)\s*->\s*(.+)$` | `\S`+`\s` |
| 331 | fn bare `^(\S+)\s+(\S.*?)\s*->\s*(.+)$`（惰性 `\S.*?`） | `\S`+`\s`（regex crate 支持惰性量词） |
| 332 | split(/\s+/) | JS_WS |
| 344 | deck `^(\S+)\s+spec\b\s*(.*)$` | `\S`+`\s`+`\b` |
| 351 | `(?:^\|\s)defaultcard\s+(\S+)\s*$` | `\s`+`\S` |
| 357 | `(?:^\|\s)reset\s*$` | `\s` |
| 374 | ending `^([A-Z][A-Z0-9_]*):\s*"(.+)"(?:\s+\(([a-z]+)\))?$` | `\s` |
| 384 | recipe `^(\S+)(?:\s+(.*))?$` | `\S`+`\s` |
| 387 | count spec `/^(\d+)(?:\.\.(\d+)?)?$/` | — |
| 400 | `^default\b` | `\b` |
| 401 | split(/\s+/) | JS_WS |
| 407 | `hint\s+"([^"]*)"` | `\s` |
| 409 | `(?:^\|\s)when\s+(.+)$` | `\s` |
| 504 | when 归一 replace(/\s+/g,' ') | JS_WS |
| 508 | matchAll count gate `…\)\s*(>=\|…)\s*(\d+)` | `\s` |
| 530 | template `/\{([a-z_][a-z0-9_]*)\}/g` | — |
| 589 | readSetAspect `/aspect:([a-z_][a-z0-9_.]*)/g` | — |
| 594 | matchAll `\baspect\.([a-z_][a-z0-9_.]*)` | `\b` |
| 597 | matchAll `SET\(\s*[^,()]+,\s*aspect:…` | `\s`（补类 `[^,()]` 照抄） |

### A.3 lenient.ts（4 处含 `\s`；R）

| 行 | 模式要点 | 转写 |
|---|---|---|
| 37 | `/^\d+:\d+\s*@/`（canonical 逃生舱，UI-11） | `\s` |
| 50 | split(/[\s,;]+/) | JS_WS 扩展类 `[JS_WS,;]` |
| 182 | split(/\s+/) | JS_WS |
| 227 | split(/[\s_]/) | JS_WS 扩展类 `[JS_WS_]` |
| （59） | `/^\d+:\d+$/` lone-clock——**无 \s**（评审 2 所列 5 处勘误为 4 处 + 此行） | — |

---

## 修订记录（v1 → v2）

三份评审（plan-review-1/2/3）的 must-fix 全部落实；should-fix 采纳情况逐条注明。

| 编号 | 级别 | 落实位置 |
|---|---|---|
| PR1-01 | must | §4-W4 第 4 条（四个 anchor 场景：anchor-due-tie / anchor-expiry-insertion-order / anchor-on-expire-first-match / anchor-refuse-id-recycle）+ §3.14（W3 排序测试期望值一律从 golden 锚点导出）+ W4 验收（anchor 零字节差为独立判据）+ §6.1 风险 2 |
| PR2-01 | must | 新增 §2.6（状态所有权与别名模型：bindings=id 句柄、禁克隆 Entity、身份键索引化、禁 Rc/RefCell、expand 拆两步、IndexMap 克隆保序）+ §7 规则 7/13(b) |
| PR2-02 | must | 新增 §3.21（localeCompare 近似比较器 cmp_ascii_ci + W4 suggestions 快照对拍 + 禁 ICU 另立议题）+ §1.1.5（suggestions 序列为可观察行为）+ §4-W4 第 5 条 + W3 验收（script-play:421） |
| PR2-03 | must | §3.8 重写为通用规则（`\s`/`\S`/`\b` 三类转写 + `.` 语义冻结论证）+ §2.3 计数勘误（core 23 行/25 字面量、script ≈40、lenient 4）+ 新增附录 A 逐条转写清单（A.1 core / A.2 script.ts 逐行 / A.3 lenient）+ §2.2 lexer 行 |
| PR3-01 | must | §4-W1（测试清单改为 sentence/clock/query-predicate 3 文件；宏展开验收改直测子集：手工 FnTable 直调 collect/expand/format，不经 compileScript）+ §4-W3（functions.test 27 个移入，补全链路展开验证）+ §5.2.1 映射表 |
| PR3-02 | must | §4-W3（补 nightjar.test 21 个 + script-play 剩余 lenient/game loop）+ 新增 §5.2.1（25 个测试文件 → 波次 → Rust 文件完整映射表） |
| PR3-03 | must | §2.2（reach-fuzz-lib → W4、reach-scale-lib → W5 落位行）+ §4-W4（Rust 侧移植 reach_fuzz_lib.rs，与 dump 选择器同源）+ §4-W5（移植 reach_scale_lib.rs；前置补 tests/common settle/reach_fuzz_lib） |
| PR3-04 | must | 新增 §7 规则 13（波次收口 review 门：非作者 reviewer + 检查清单 (a)–(g) + "无测试钉住行为零约束力"理由） |
| PR1-02 | should（采纳） | §4-W4 第 3 条：S1–S14 转写判据改为 dump 工具内复算原 S1–S14 expect 断言（verdict/状态断言，非骰序——按验证 03 勘误）；#id 绑定场景走 canonical 步 driver |
| PR1-03 | should（采纳） | §4-W5 验收：verdict 分布 + NameStat 非计时字段一致，ms 仅量级参考 |
| PR1-04 | should（采纳） | §4-W5：共享遍历器补验证 03 REACH-01 修正细节（IF 臂新 seg、REFUSE/throwing 记所在 seg、回调双消费形态）+ §7 规则 3（对照验证"修正表述"） |
| PR1-05 / PR2-11 | note（采纳） | §2.5 重复 bullet 删除、乱码修正；§3.9 自问自答改单句裁定；§5.2 snap 改"5 处排序 + fired sort"；§2.3 注明 23 行/25 字面量 |
| PR1-06 | note（采纳） | §2.2 映射表 TS 列 store.rs → store.ts |
| PR1-08 | note（采纳） | §3.12 补 sig() 字段清单为冻结面（含 rng、排除 beginId/clause） |
| PR2-04 | should（采纳） | §3.5（f64 中转、禁 parse::<i64> 直解、checked_* 溢出文档化分歧）+ §3.19（"≥1e21 不可达"改口为"可达但未钉住"）+ §3.3（rng 累加 checked）+ §3.18（parseDuration 溢出） |
| PR2-05 | should（采纳） | §3.7（OwnPropKeys 补 clone/spread 重排点 types.ts:118/132/138、engine.ts:469/934；`SET(x,"5",…)` 通路；ToUint32 判据 + Node 对拍） |
| PR2-06 | should（采纳） | §2.4（profile.test debug="line-tables-only"、测试线程默认封口、Cargo.lock 入库 + 最小版本、serde derive feature） |
| PR2-07 | should（采纳） | §4-W4 第 3 条：S1–S14 改为 dump 内复制场景构造代码，不做文本转写（与 PR1-02/PR3-08 合并落实） |
| PR2-08 | note（采纳） | §2.3/§3.8/附录 A：词法器五件手写 scanner 升格为强制（regex 不吃 Vec<u16>）；DUR_RE leftmost-first 保序结论收录 Q9 |
| PR2-09 | note（采纳） | §4-W4（--out 参数、相对路径、恒 `\n`、sha2 禁入）+ §2.3 禁用清单补 sha2 |
| PR3-05 | should（采纳） | §4-W1 波内移植序（roles→clock→types→ref→dict→query→predicate→sentence→lexer→parse/fn_expand→format→dice 接线） |
| PR3-06 | should（采纳） | §4 开头 + §5.1（"全绿"=迄今已移植测试）；§4-W1 验收（"core 域 toThrow"改为"本波所移 3 文件内"口径） |
| PR3-07 | should（采纳） | 新增 §7 规则 14（模块认领表、cargo 全局串行、integrator 宣布波次门）+ §4.7 |
| PR3-08 | should（采纳） | §4-W4 第 3 条：转写校验并入 golden-dump.ts；goldens/ 属规则 10 豁免（§7 规则 10 已注明） |
| PR3-09 | note（采纳） | §4-W6 验收脚本化（管道输入 + 与 demo 黄金输出 diff；黄金在 W4 生成） |
| PR3-10 | note（采纳） | §4-W4 第 7 条（fold-prefix 模式）+ §6.2 第 3/5 条（插桩 env 门控不入库、失败报告必备字段） |
| PR3-11 | note（采纳） | 新增 §4.7（各波 T 编号子任务切分，"编辑可并行、构建串行"前提） |

未采纳项：无（三份评审的 should-fix/note 全部采纳；PR2-03 中 lenient.ts:59 经源码核实无 `\s`，附录 A.3 按实况勘误为 4 处并注明）。
