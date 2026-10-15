# plan v1 评审（视角 2：Rust 技术可行性与内存预算）

- 评审人：feasibility review agent
- 日期：2026-10-14
- 环境：macOS 27.0.1 arm64，16GB（hw.memsize = 17179869184），rustc 1.98.1 (48a229cea)，cargo 1.98.1
- 方法：只读。通读 plan.md 全文；对照源码逐条核查正则清单、JSON 序列化点、迭代序敏感点、所有权/别名形态、测试基建与 CLI 产物路径；确认工具链版本。未构建、未下载依赖、未运行测试。
- 结论：**需修订后批准**。计划的总体技术判断（单 crate、四件依赖、jsjson 整数域、朴素不动点、码元词法器、-j4 纪律）经核查全部成立；但有 3 个 must-fix：状态所有权/别名模型完全缺失、`localeCompare`（suggestions 排序）无任何对策、JS `\s`/`\S` 显式字符类的适用面被严重低列（script.ts/lenient.ts 未覆盖）。

---

## 总评

计划质量高于常见移植计划：行为判据（JSONL 字节、snap、黄金差分）选得准；quirk 冻结清单 Q1–Q17 抽查与源码一致（Q2 predicate.ts:172–178 静默吞字符、Q9 DUR_RE 交替序、Q10 字面转义、Q16 `-0` 均逐行属实）；核心高危点的对策（splitmix64 单次 finalizer + u128 widening、jsjson 整数域、OrdinaryOwnPropertyKeys、码元词法器、朴素 round-robin 不动点）都验证过源码成立。基础设施选型无误：regex crate 为 leftmost-first 语义（验证 01 已核，DUR_RE 保序即等价）、serde_json `preserve_order` 与 indexmap 2 兼容、`include_str!(concat!(env!("CARGO_MANIFEST_DIR"), …))` 是合法 const 表达式、`[profile.test]` 覆盖合法。

缺口集中在两类：(a) **TS 别名/身份语义在 Rust 的落法没有写**——bindings 持活实体引用、reach 的对象身份键 Map、引擎 mid-list dead-stamp 读 through，这些决定整个 engine/reach 的类型骨架，计划只字未提；(b) 若干"JS 字符串语义"细节（localeCompare、spread/entries 的整数样键重排面、`\s`/`\S`/`\b` 的 Unicode 集）只覆盖了抽查过的 2–3 处而未覆盖全量。

---

## 逐条

### PR2-01 [must-fix] 缺一节"状态所有权与别名模型"——engine/predicate 的 Entity[] 绑定语义决定了整个骨架

依据（源码形态）：
- `validateSlots` 产出 `bindings: RoleBindings = Record<string, Entity[]>`，其中是**对 State.entities 内对象的活引用**；Phase 2/3 乃至 `evalPred`（predicate.ts:399–401 `roleCards` 每次 `filter(e => e.deadAction === null)`）都在**经这些引用读取当前 deadAction**。engine.ts:531 `e.deadAction = ctx.recordId`（DESTROY）、:619 `e.attrs[key] = value`（SET）都是**通过 expand() 返回的引用改 State 内实体**。若 Rust 端把 RoleBindings 实现为 `Vec<Entity>`（clone），mid-list DESTROY 后 `IF object.aspect.x` 的"全死绑定静默 false"（plan §3.10 引 predicate.ts:492–498 dead-stamp）就会读到**旧的活副本**——直接行为分叉。
- reach.ts 大量**对象身份键**：`Map<CompiledVerb, SlotProof[]>`（:984 committable）、`Map<RawSite, string[]>`（:985 proven）、`Map<CompiledVerb, Set<string>>`（fillers）、`WeakMap<CompiledContent, …>`（:233 trigIndexCache）。Rust 无对象身份，需要 arena 索引（site: `usize` 进 `Vec<RawSite>`；verb: content.verbs 的插入序下标；`Analyzer<'c>` 生命周期方案已对口）。

裁定：可行，且**不需要 Rc/RefCell/arena 化 State 本身**——Entity 从不被从 map 删除（销毁 = 盖 deadAction 戳），所以正确的模型是：`RoleBindings = IndexMap<Role, Vec<String>>`（只存 id，顺序 = refs 解析序），所有"读 Entity"都经 `state.entities.get(&id)` 现读；`expand()` 拆两步（先不可变解析出 id，再 `get_mut`）；ApplyCtx 持 `&mut State` + 按字段拆借用。这个方案与"插入序=语义"零冲突（IndexMap 克隆保序，cloneState 的逐项深拷贝照 TS 抄）。单 crate 内 types↔sentence↔predicate↔query 的类型级循环（已核实确为纯类型环）对 Rust 模块无碍。
**但计划必须在 §2 加一节把上述模型写死**（bindings=id 句柄、禁 clone Entity 进 bindings、身份键一律索引化），否则不同执行者会各自发明（Rc<RefCell> 方案会引入运行时 panic 面且与 `&mut` 直改路径冲突）。

### PR2-02 [must-fix] `String.prototype.localeCompare`（play.ts:238）没有任何 Rust 对策

依据：src 全仓唯一一处 `localeCompare` 在 `suggestions()` 的候选池排序：
`play.ts:238 .sort((a, b) => rank(a) - rank(b) || a.name.localeCompare(b.name))`。
而 plan W3 自己的验收就包含移植 script-play.test.ts:412（"suggestions are concrete, executable actions"），该测试 **:421 钉死 `s[0]` === `'read THE MANIFEST'`**——suggestions 的输出顺序是可观察 API。`localeCompare` 是 ICU 本地化排序（默认 locale），与 Rust `str::cmp`（码点序）在大小写混排、下划线/标点权重上不同；精确复刻要么引入 ICU collation crate（**违反 §2.3 依赖冻结**），要么近似（`to_lowercase` 后码点序）——近似在本仓 ASCII 大写名字域大概率与现测试兼容，但这是行为决策，不能由执行者默认。

裁定：计划须补一条决策：推荐"近似比较器（`to_ascii_lowercase()` 后 `cmp`）+ 在 W4 golden-dump 里对 nightjar 状态追加快照 `suggestions()` 全列表（TS 侧产出、Rust 逐字节对拍）"，把近似面钉进差分；若未来出现非 ASCII 名字再升 ICU（另立议题）。golden-dump 增量成本≈0。

### PR2-03 [must-fix] JS `\s`/`\S`/`\b` 显式字符类只列了 2 处适用点——script.ts ~40 个正则、lenient.ts 5 处、query.ts:43/48 的内嵌 `\s*` 全部需要同款处理；正则计数低估

依据（全量清点，plan §2.3/§3.8 只认了 lexer.ts:26 与 query.ts:18 两处）：
- **script.ts** 实际约 **40 个正则使用点**（99,106,113,123,171,205,210,224,230,231,240,257,264,269,270,276,290,292,309,311,325,329,331,332,344,351,357,374,384,387,400–409,504,508,530,589,594,597），其中含 `\s`/`\S` 的超过 25 处；plan 写"script.ts ~15 个行段正则"——**低估近三倍**。
- **lenient.ts**：37（`/^\d+:\d+\s*@/`）、50（`split(/[\s,;]+/)`）、59、182、227 共 5 处含 `\s`。
- **query.ts:43/48**（aspect/state term 的锚定匹配）自身内嵌 `\s*(>=|<=|…)\s*`——不在 plan 的"两处"里。
- 语义差同 Q8：JS `\s` 含 `\uFEFF`、不含 `\u{85}`；Rust `\s` 恰反。`/^\d+:\d+\s*@/`（lenient 37）对一个 `\uFEFF` 前缀的 canonical 行为即分叉。`script.ts:205/257/325/329/331/344` 的 `\S+` 是 `\s` 的补类，必须用**同一个显式类的否定**（`[^ …]`），不能漏。`\b`（script.ts:205/224/257/264 等）：JS `\b` 是 ASCII `[A-Za-z0-9_]` 边界；Rust 默认 `\b` 是 Unicode 词边界——非 ASCII 名字后跟 `stackable` 时判定相反；须统一写 `(?-u:\b)`。
- 另：JS `.` 不匹配 `\r`/`\u2028`/`\u2029`，Rust `.` 只不匹配 `\n`。本仓这些 `(.+)` 全部作用在 trim 过的单行文本上（script.ts:99 只替换 `\t`；行来自 `split('\n')` 后 trim），`\r` 面实际不可达——建议以一行注释冻结此论证，防止执行者在别处复用模式时踩中。

裁定：把 §3.8 从"两处"改为一条**通用规则**：所有移植正则中 `\s`→显式 JS 空白类、`\S`→其否定、`\b`→`(?-u:\b)`，适用清单 = core 23 + script ~40 + lenient 5，并把计数修正写进 §2.3。`js.rs` 应提供 `JS_WS_CLASS` 常量单点定义。

### PR2-04 [should-fix] 整数域边界：jsjson "全整数 i64" 结论成立，但 §3.19 "≥1e21 在 i64 域不可达" 的论断是错的，且 parseDuration 有溢出 panic 面

依据：
- **浮点进 JSONL 吗**——已核实：不进。记录数值面 = id/advancedMin/startMin/rng/int/delta/dur/pair.v，全部来源是词法 `[0-9]+` 字面量、计数、aspect 值与 i64 域运算；tReal 恒 `string|null`（engine.ts:27/35/317 属实）。jsjson 因此**不需要** JS 最短浮点表示算法（Ryū 类）——这是本计划躲过最大高危点的原因，成立。i64 Display 与 JS 整数打印在 |n| < 1e21 内逐字节相同，也有 `Math.floor` 域佐证。
- 但两条通路可产生 >i64 的值：(1) 词法 int 字面量 `SET(x, 99999999999999999999, 1)` → `Number()` = 1e20 > 2^63；JSONL 会写 `"1e+20"`，Rust i64 装不下。(2) `parseDuration`（clock.ts:55）`Number(p[1]) * 60`：超大字面量乘法后 > i64；Rust 若用 i64 乘法在 debug 下 **panic**、release 下回绕——比 TS 更糟。§3.5 只给 seed 的 f64→mod 2^64 助手，§3.19 只说"经 f64 中转"却未声明落点类型装不下时怎么办，"≥1e21 不可达"的论证只对"输出指数化"成立、对"输入字面量"不成立（可达，只是无测试钉住）。
- 顺带一条必须写死的禁令：**禁止 `str::parse::<i64>()` 直解数字字面量**——>2^53 的字面量直解结果与 JS 的 f64 最近偶数舍入不同（如 `"9007199254740993"`：JS→…992，直解→…993）；plan §3.19 有"f64 中转"但未点名禁直解。

裁定：不要求支持超域值（可作文档化分歧），但计划须明确：(a) 数字字面量一律 `f64` 中转 → i64（禁直解）；(b) i64 溢出路径（parseDuration 乘法、rng 累加）显式 `checked_*` + 与 TS 同步的分叉声明或 `saturating` 注释；(c) 把"≥1e21 不可达"改写为"可达但未钉住，接受为文档化分歧"。

### PR2-05 [should-fix] OrdinaryOwnPropertyKeys 助手的适用点清单漏了 clone/spread 的重排点

依据：JS 对象 spread/`Object.entries` 都按 `OrdinaryOwnPropertyKeys` 序产出（整数样键前置）。除 plan 已列的 8 处外，**每个 clone 都是一次重排**：
- types.ts:118（cloneState 的 `{ ...e, aspects: cloneAspects(...), attrs: { ...e.attrs } }`）、:138（cloneAspects `Object.entries`）、types.ts:132（windows spread）、
- engine.ts:469（`{ ...(proto?.attrs ?? {}) }`）、:934（engine 的 cloneAspects）。
整数样键进入 attrs 的通路真实存在：`SET(x, "5", true)`（引号键不走 UPPER_SNAKE 校验，engine.ts:601–618 rawKey 取自 str arg）。若 Rust 端 IndexMap 只保插入序，`SET(x,"5",a)` 后 `SET(x,"container",b)` 再 clone，JS 序变 `["5","container"]`（整数键前置）、Rust 保持 `["container","5"]`——attrs pair 序列化（engine.ts:525/589 的 `JSON.stringify(attrs)`）与 snap 均分叉。plan §2.3 已认 REACH-04"该重排不是理论"，却没把它接到这些 clone 点。
裁定：规则改为"任何 TS spread/`Object.entries`/`Object.keys` 迭代点，若键源可含整数样键，序列化/克隆时统一过 `js::ordinary_own_property_keys`"，并在实现上给 Entity.aspects/attrs 固定 `IndexMap<String, …>` + `insert` 保位语义（JS 覆写旧键保原位，IndexMap 相同）。此外助手本身的整数样键判据须写死：**canonical 数字串且 ToUint32 回读相等（≤ 2^32−2，V8 语义）**，并以 Node 对拍单测钉死（plan 有对拍要求，补判据）。

### PR2-06 [should-fix] 内存预算：结论够用；建议值细化如下

依据与环境：16GB、rustc 1.98、单 crate（预计 ~10–15k 行 Rust，最大单模块 engine/reach ~2k 行）、四件依赖（最重的是 serde_derive/syn，dev profile debug=0 下自身构建峰值 ~0.5–1GB）。
- `-j 4`：峰值估算 4 × rustc(~0.5–1.5GB) + 链接器（ld64，debug=0 下 <1GB）≤ ~7GB，**16GB 余量充足**；这是保守值，可保持。
- `[profile.dev] debug = 0`：同时作用于依赖与本地 crate，是最大的一刀，正确。建议改 `debug = "line-tables-only"`：内存增量 ~10–20%，保留 panic 回溯行号——本计划引擎路径禁 unwrap（§7.11），测试失败时的回溯价值高；至少对 `[profile.test]` 单独开。`[profile.test]` 显式 debug=0 与继承 dev 重复，无害，可留。
- `cargo test` 编译侧：tests/ 下每个根文件是独立测试二进制（~10 个），链接共享 lib 的 rlib，-j4 下串行化足够；**测试执行侧**（harness 线程）默认 = 逻辑核数（Apple Silicon 8–14），本套件每测试内存 ≤ 数 MB，最重的 reach-scale 400×400 双世界在进程内也仅几十 MB——无需 RUST_TEST_THREADS 限制，plan 未提但不是缺口，建议写一句"默认即可"封口。
- `CARGO_INCREMENTAL=0`（全量重构建）：增量会翻倍磁盘与部分内存，16GB 下日常迭代其实开得起；plan 的保守开关可以，但建议改为"仅全量重构建/CI 关，日常开"——计划已是这个意思，确认即可。
- 补充建议：(a) `Cargo.lock` 入库 + 记录最小版本（indexmap 2.x、serde 1.0.x + **features=["derive"]**、serde_json 1.0.x + `preserve_order`、regex 1.11.x）——plan §2.3 未写 serde 的 derive feature，也没提 lock 入库；(b) 两条 cargo 命令不并行的禁令保留；(c) manifest 的 sha256 **Rust 侧不要**为了校验引入 sha2（依赖冻结，见 PR2-09）。

### PR2-07 [should-fix] W4 对抗场景 S1–S14 的"转写为脚本文本"有保真风险，建议改为 dump 脚本内复刻场景构造函数

依据：reach-adversarial.test.ts（1017 行）的 S1–S14 是**程序化构造的 ContentRaw**（内联在 test 里，未导出），含 `when`/IF/ROLL/expiry/deck 组合。转写成 `.txt` 再经 parseScript 编译，等价性依赖转写者对 script 文法的把握（缩进、section、meta），多一层会错的映射；plan 的兜底"用 vitest 既有断言校验转写"绕得远。
裁定：golden-dump.ts 直接**复制场景构造代码**（TS 侧、只读、不动 src/test）产出 ContentRaw → compileContent → settleStep，跳过文本层；工作量相同、少一个失真源。S1–S14 场景名/序保留为 manifest 字段即可。

### PR2-08 [note] 单 crate + 5 bin、模块映射、词法器码元化：成立，附两条实现提示

- 单 crate 多 module 对 types↔sentence↔predicate↔query 类型环是正解（核实：环为纯 type-level，无值级循环初始化问题）；workspace 拆分的代价判断正确。5 个 `[[bin]]` 薄 main 在单 crate 下无链接开销问题。
- 词法器在 `Vec<u16>` 上运行（CORE-09）与 regex crate **不兼容**（regex 只吃 &str/字节）：lexer 的 NAME_RE/NUM_RE/DUR_RE/`/\s/` 四件在码元域必须**手写**字符判定，plan §2.3"允许手写"的口子应升格为"此四处必须手写"，其余 19 处 core 正则留在 &str 域用 regex crate（query/predicate/format/parse/clock 的输入都是自有 String，成立）。`String::from_utf16_lossy` 的孤立代理分歧 plan 已文档化，接受。
- DUR_RE 交替序：regex crate 为 leftmost-first（非 POSIX 最长），保序即逐位等价——验证 01 已核，plan Q9 正确，无需额外措施。

### PR2-09 [note] W4/W5 产物路径与成本：可操作

- 路径约定成立：goldens 落 `rust/tests/goldens/`，dump 脚本从仓库根写相对路径，Rust 测试以 `CARGO_MANIFEST_DIR` 定位；nightjar 脚本 `include_str!(CARGO_MANIFEST_DIR/../scripts/…)` 指向 worktree 根 scripts/，成立。建议 dump 脚本带 `--out` 参数（默认 rust/tests/goldens），manifest 存相对路径，JSONL 恒 `\n`。
- **TS 侧生成成本实测估计：不构成风险**。nightjar 主锚（9 步）、sampledWalkStates（explorer-lib.ts:284 硬帽 500 次迭代/10 状态）、S1–S14、fuzz 200 种子 × 25 settleStep（纯引擎步，无 explore()）——合计秒级、单 tsx 进程 <200MB。真正重的两个东西（reach-fuzz CLI 的深 explore、reach-scale 全量 1566×2084 多进程 bench）plan 都**没有**放进黄金生成，边界画对了。
- reach-scale Rust 版用 thread+channel 替代 TS 的多进程：NameStat 输出仅依赖 (world, name)，world 确定性构建——共享一次构建或每线程各建皆可，逐字段一致可达；`--worker i k P R` 与 tmpdir `reach-scale-{i}.json` 语义（TS 侧对照）保留即可。
- manifest 的 sha256 仅为入库完整性元数据；Rust 差分判据是 `cmp` + snap 串（plan 如此），**不要**为验 sha 引入 sha2 crate。

### PR2-10 [note] 其余"听起来对"点的核查结论（无需改计划）

- **RNG**：splitmix64 常量/移位（dice.ts:19–31 属实）、rollDie u128 widening（BigInt 路径核实，ENG-05 修正必要）、27 行向量表（dice.test.ts:94–113 逐行核对）与 4 置换向量（decks.test.ts:141–144）全部属实；FNV-1a over `.as_bytes()` 与 TextEncoder 等价；`% (i+1)` 在 u64 与 BigInt 同余。
- **错误消息**：ChorusError 单通道 + 调用点各自拼装的策略与 192 条 toThrow 的形态匹配；regex crate 断言 `is_match` 对抽查的 JS 模式（/bad clock/、/expected '\'/、/->/ 等）全部可表达。
- **fold/id 回收**：`foldUpTo` 的 `r.id < uptoExclusive` 与拒绝行复用 firstRecordId（engine.ts:257–262/1070）核实，禁止改单调 id 的禁令正确。
- **不动点**：朴素 round-robin 唯一日程 + REACH-10 禁优化，与 reach.ts 的 proven 插入序语义（`mintSources` push 序、committable 先到先证明）一致。
- **serde 结构**：ActionRecord 四个构造点（makeBegin/Complete/Wait/EventRecord）字段字面量序与接口序一致 → 单个 serde struct 声明序可覆盖；`Option + skip_serializing_if` 复刻 undefined 丢弃；`Arg` 内部 tag `t`、`Ref` 内部 tag `form`、`into.ref` 的 `Ref | {role}` 用 untagged 枚举可解；`sign?: 1|-1` 用 Option。全部可表达，无阻点。
- **迭代序**：State.byName 外层无未排序迭代点（snap 与 script-play:430 均先 sort）→ HashMap 外层成立；FnTable.byName 的迭代点（parse.ts:845/849、content.ts:543/571/585）是声明序承重 → 必须用 IndexMap（plan §2.2 已列）。

### PR2-11 [note] 编辑性问题（合并修复）

- §2.5 第一条 bullet **整段重复两次**，且首份含错字"温др"。
- §3.9 尾反斜杠句含未删净的自问自答（`"undefined" 四字符? 不——`）。
- §5.2/TEST-13 说 snap"三处 `a<b?-1:1`"，实际 snap.ts 有**五处**（ents/rels/byName/decks/windows）+ fired sort——snap.rs 实现按文件实况，不按"三处"。
- core 正则"23 处"实为 23 个使用行 / 24 个字面量（format.ts:98 一行两个 replace）——按行清单执行即可，建议注明。

---

## 判定汇总

| 编号 | 级别 | 主题 |
|---|---|---|
| PR2-01 | must-fix | 状态所有权/别名模型一节缺失（bindings=id 句柄、身份键索引化） |
| PR2-02 | must-fix | localeCompare 排序无对策（script-play.test.ts:421 钉死 s[0]） |
| PR2-03 | must-fix | `\s`/`\S`/`\b` 规则适用面（script ~40 + lenient 5 + query 内嵌），正则计数低估 |
| PR2-04 | should-fix | i64 域声明、溢出 checked、禁直解 i64、"≥1e21 不可达"论断改口 |
| PR2-05 | should-fix | 键序助手补 clone/spread 重排点 + ToUint32 判据 |
| PR2-06 | should-fix | 内存建议值细化（line-tables-only、lock 入库、serde derive feature、默认测试线程封口） |
| PR2-07 | should-fix | S1–S14 黄金改为 dump 内复刻构造，不做文本转写 |
| PR2-08 | note | 单 crate/码元词法器成立；lexer 四 scanner 手写升格为强制 |
| PR2-09 | note | W4/W5 路径与 TS 成本可操作；sha2 禁入 |
| PR2-10 | note | RNG/消息/序/fold/serde/迭代序核查全过 |
| PR2-11 | note | 编辑性问题四处 |

must-fix 共 3 条。修订（补一节所有权模型、localeCompare 决策、字符类通用规则 + 计数修正）后即可批准；三者均为计划文本增补，不动摇既有架构决策。
