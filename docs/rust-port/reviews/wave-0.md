# W0 波次门评审记录（§7 规则 13，PR3-04）

- 评审对象：`33d045b..8a35d89`（4 commit：js.rs 脚手架 / core/dice.rs / core/deck_rng.rs / messages.md 首批）
- 评审人：wave integrator 指派的独立 reviewer（非本波作者）
- 评审日期：2026-10-14
- 证据方式：逐算子对照 TS 原文；独立 python3 任意精度重算全部向量；`cargo test -j 4` 亲自重跑（一次、串行、rust/ 下，无 vitest/tsx 并发）

## 总评：**PASS-with-notes**

11 个测试（3 lib + 4 dice + 3 decks + 1 gate）全绿。30 个骰向量 + 4 个置换向量与 TS 测试文件逐值一致，且经独立重算复核。must-fix = 0；should-fix = 1；note = 3。可打 `rust-port-w0` tag。

---

## (a) 逐段对照 — PASS

**splitmix64**（rust/src/core/dice.rs:14-28 ↔ src/core/dice.ts:19-31）
- 常量逐一相等：GOLDEN_GAMMA `0x9e3779b97f4a7c15`、MIX1 `0xbf58476d1ce4e5b9`、MIX2 `0x94d049bb133111eb`（dice.rs:14-16 = dice.ts:20-22）。
- 运算顺序逐算子一致：`z=(x+γ)` → `z^=z>>30`、`*=M1` → `z^=z>>27`、`*=M2` → `z^=z>>31`（dice.rs:24-27 = dice.ts:27-30）。`wrapping_add/wrapping_mul` ≡ BigInt `& MASK64` 链（mod 2^64 数学等价）。单次应用（finalizer，非完整序列）两侧一致。

**rollDie 的 u128 widening**（dice.rs:38-42 ↔ dice.ts:38-42）
- `key = (seed as u64) ^ (counter as u64)` ≡ `BigInt.asUintN(64, BigInt(·))` 双操作数 XOR：i64→u64 `as` 即二补码位型，负 seed 向量行钉死（dice.rs:50-52）。
- `1 + (((sides as u128) * (r as u128)) >> 64) as i64` ≡ `1 + Number((BigInt(sides) * r) >> 64n)`。ENG-05 修正落实：纯 u64 wrapping 会毁掉 `>> 64` 读的高位，u128 中间量正确（dice.rs:34-36 注释声明；product < 2^127 无 u128 溢出）。

**FNV-1a64**（deck_rng.rs:41-48 ↔ content.ts:430-437）
- offset `0xcbf29ce484222325`、prime `0x100000001b3`；逐字节 XOR-后-乘的次序与 TS 一致（TS `h ^= BigInt(b)` 在 `h = (h * prime) & MASK64` 之前）；`id.as_bytes()` ≡ `TextEncoder().encode(id)` 的 UTF-8 字节列。

**mod (i+1) 位置置换**（deck_rng.rs:58-69 ↔ content.ts:453-461）
- 位置 i 自 n−1 降至 1（`while i >= 1` ≡ `for (i = n-1; i >= 1; i--)`）；`key = (seed as u64) ^ hash ^ (i as u64)` ≡ `asUintN(seed) ^ hashDeckId ^ asUintN(i)`（i 非负，asUintN 恒等）；`j = splitmix64(key) % (i+1)`；Fisher–Yates `pos.swap(i, j)` ≡ TS 三步交换。不耗 rng 计数（纯函数）两侧注释一致（law 5）。

**drawFrom 三规则**（deck_rng.rs:82-96 ↔ content.ts:472-478）
- 分支次序一致：`cursor < len → order[cursor]`；`reset → order[cursor % len]`；`defaultcard → dc`；否则报错。错误文本逐字符含 em-dash 与 `(§9.42)` 后缀（deck_rng.rs:93 = content.ts:476，见 (d)）。

**LCG**（js.rs:76-111 ↔ explorer-lib.ts:276-277 / reach-fuzz-lib.ts:36-38 / reach-scale-lib.ts:28-31）
- 状态推进 `s.wrapping_mul(1664525).wrapping_add(1013904223)`（u32）≡ `Math.imul(s,1664525)+1013904223` 后 `>>>0`（signed i32 + 常量再 ToUint32 ≡ mod 2^32 wrapping，位精确）。
- 三个种子入口一一对应：fuzz `new()` XOR 0x9e3779b9（js.rs:83-85）、scale `Lcg::new(0xc5)`、explorer `from_raw_state(0x4e494748)` **无 XOR**（js.rs:88-90 ↔ explorer-lib.ts:276 原文确为 raw 赋值）。
- `next_f64 = u32/2^32`（2 的幂缩放，f64 位精确）≡ `/0x100000000`；`next_int = floor(r()*n)` 留在 f64（js.rs:98-110）。

**cmp_utf16**（js.rs:16-18 ↔ ref.ts:47-49 sortRefs 的 `<`/`>`、types.ts:228 relationKey 的 `>`）
- JS 字符串关系比较即 UTF-16 码元字典序；Rust `encode_utf16().cmp(...)` 精确对应。测试钉住 D800 代理对先于 U+E000 的陷阱（js.rs:118-127），`str::cmp` 替换被注释明令禁止（js.rs:11-14，验证 CORE-05）。`JS_WS_CLASS` 与 plan §3.8 规定串逐字符相同（js.rs:24-25），`js_is_ws` 为同集合码元谓词并以 NEL/MVS 负例钉边（js.rs:145-152）。

**meta-seed 模板**：rust/tests/dice.rs:84-90 的 `line {ln+1}: seed must be an integer, got '{…}'` ≡ script.ts:113 原文；rig 行号核实：dice.test.ts:133-149 模板串首行为空，`seed:` 在 `split('\n')` 0 基下标 4 → `line 5`（dice.rs:96-97 断言正确）。

## (b) 迭代序/所有权审计 — PASS

`grep -rn "HashMap\|HashSet\|BTreeMap\|Rc<\|RefCell\|clone()" rust/src rust/tests`：
- `HashMap`：仅 lib.rs:10 文档注释提及纪律，无实例。无 HashSet/BTreeMap/Rc/RefCell/Cell。
- 本波无序可观察状态：置换用 `Vec<usize>`，deck 顺序用 `Vec<DeckCard>` —— 均为有序结构，无映射迭代点，§3.7/§2.6 无违反。IndexMap 本波尚无用例（合理：无 Map 字段落地）。
- `clone()` 3 处（deck_rng.rs:84/87/90）：均为 `DeckCard`（两个 String 的值型结构）出参克隆。TS `drawFrom` 返回编译内容的**引用**（含 `deck.defaultcard` 本体），但编译后 deck 卡片从不被原位修改（W2 编译期铸造，运行期只读），值/引用差异不可观察。非 §2.6 意义上的 Entity 别名面。

## (c) QUIRK 注释在位性 — PASS

本波模块（js.rs、core/dice.rs、core/deck_rng.rs）适用的 §1.2 条目仅 **Q8**（JS `\s` 集合）：`// QUIRK(CORE-08(1)/Q8)` 在 js.rs:20-23，且配测试钉死 `\uFEFF` 含 / `\u{85}` 不含（js.rs:130-153）。其余 Q1–Q17 均属 W1/W2/W3 模块，不在本波范围。附加核对：「明确禁止」中 CORE-05（sameRef 不做 cmp_utf16 替换、不得 `str::cmp`）在 js.rs:13-14 有据；deck 置换 `u64 mod (i+1)` 偏差注释镜像 TS 原文（deck_rng.rs:55-56 ↔ content.ts:448-450）。

## (d) messages.md 对账 — PASS-with-note

- 行 1（pinned）：模板 ``line ${ln + 1}: seed must be an integer, got '${m[2].trim()}'`` 与 script.ts:113 **逐字符相同**（含 `${m[2].trim()}` 内层引号）；Rust 侧构造/冻结点 dice.rs:84-90 渲染结果 `line 5: seed must be an integer, got 'six'` 与 rig 行号算术一致。
- 行 2（built）：deck_rng.rs:92-95 构造点文本 `deck '{id}' is exhausted — no 'reset' and no 'defaultcard' (§9.42)` 与 content.ts:476 逐字符相同（em-dash、§9.42 后缀）；tests/decks.rs:66-69 全文断言。
- 行 3/4/5（registered，预登记）：与 engine.ts:764 `ROLL: dice count must be an integer >= 0, got ${count ?? 'nothing'}`、engine.ts:767 `ROLL: sides must be an integer >= 2, got ${sides ?? 'nothing'}`、predicate.ts:477 `'roll' is unbound — no ROLL has run in this outcome list before this point` 逐字符核对**一致**（含 `?? 'nothing'` 与 em-dash）。
- 对账口径：W0 移植面 `toThrow` = 1 条（dice.test.ts:181），表增 5 行——超出部分由 plan §4-W0「首批 3–5 条消息」授权，且 messages.md:19 备注已自我说明（行 2 是本波唯一构造点、由 refusal 非toThrow 断言钉住；行 3–5 为 W3 预登记）。**note-1**：不构成漂移，但后续波对账时按备注口径执行。

## (e) grep 门禁 — PASS

- `rand::`：rust/src 下零命中（dice.rs:1 文档注释中的 "randomness" 为普通词，非 crate 引用）；tests/gate.rs:27-48 的 Rust 版门禁扫描 src/core + src/engine（engine/ 未落地时安全跳过）通过。
- `unwrap/expect/panic!`：实现面（dice.rs、deck_rng.rs、js.rs 非测试区）**零命中**；唯一 `unwrap()` 在 js.rs:131 的 `#[cfg(test)]` 内——规则 11 明示测试代码允许。tests/ 下 unwrap 均在测试断言（允许）。
- 按 §4-W0 附带门（dice 路径禁浮点）：dice.rs/deck_rng.rs 实现无 f64 运算（`grep -c f64` 零命中），向量表本身即探测器（TEST-12）。

## (f) TS 侧零改动 — PASS

- `git diff --stat 42ee5ba..HEAD -- src/ test/ scripts/` → **空**（exit 0，无输出）。
- `git status` 未跟踪项全部位于 docs/rust-port/（plan.md、plan-review-*.md、analysis/、verification/——计划与报告文档，允许）；`rust/target` 由 rust/.gitignore（`/target`）忽略。无源码侧未跟踪文件。
- `rust-port-w0` tag 尚未打（正确：规则 9 要求 review 门后收口）。

## (g) 有意分歧逐条核对 — PASS

| # | 分歧声明 | 核对结论 |
|---|---------|---------|
| 1 | u64 wrapping ≡ TS BigInt `& MASK64` 链 | **行为无关**。mod 2^64 数学等价；3 个 raw 向量独立重算一致 |
| 2 | i64→u64 `as` ≡ `BigInt.asUintN(64, BigInt(·))` | **行为无关**。二补码位型相同；负 seed 行（−1×2、−42）与 2^53−1 行重算一致 |
| 3 | draw_from 负 cursor 不可达（deck_rng.rs:79-81） | **声明与 TS 原文一致**。content.ts:473 首分支 `cursor < order.length` 对负 cursor 为**真**，TS 返回 `undefined`（`!` 断言掩盖）——Rust 注释原文「TS would hand back `undefined` from the first branch」准确。不可达性成立：cursor 起于 0、仅在 DRAW 成功路径前移（W2/W3 接线时须复核）。若不可达假设破裂，Rust 表现为 `cursor as usize` 回绕后索引 panic（响亮），TS 为静默 undefined——**should-fix-1**：按规则 11 补 `debug_assert!(cursor >= 0, …)` 附 TS 依据行，release 行为不变 |
| 4 | LCG u32 ≡ `Math.imul` + `>>>0` | **行为无关**。9 个常量（fuzz 4 态、NIGH 2 个 f64、scale 3 个 f64 + next_int(10)=4）独立重算逐位一致 |
| 5 | `next_int` 返回 usize | **行为无关**。n < 2^52 时 f64 路径与 TS 完全一致（本仓 n 为候选数/长度，远小） |
| 6 | 行 3/4/5 消息 pre-registered | **文本逐字一致**（engine.ts:764/767、predicate.ts:477，见 (d)）；登记不产生行为，W3 构造点落地时升 built 并再核对 |

plan §7 规则 13(g) 标准登记项（原子写、loadLog 行号、lossy 代理项、§3.5 溢出、§3.21 比较器）对应 store/reach/predicate——本波均未移植，无适用项。

## 向量独立复核（reviewer 亲测，非抄测试输出）

python3 任意精度独立重算 **30 值**（超过抽查 10 个的门槛），全部与 TS 测试文件及 Rust 断言一致：
- splitmix64 raw 3 值（0/1/42 → 16294208416658607535 / 10451216379200822465 / 13679457532755275413）；
- 骰表 14/27 行含**全部负 seed**（(−1,0,6)=6、(−1,1,6)=6、(−42,7,20)=13）与**两行 2^53−1**（counter 3→5、counter 0→1）及 2^40 计数器两行（→1、→7）；
- 置换 4 向量（含 (7,−3,'NEG')=[4,0,2,6,3,1,5]）；
- LCG 9 常量（见 (g)#4）。
`cargo test -j 4` 亲自重跑：**11/11 全绿**（3 lib + 4 dice + 3 decks + 1 gate，0 failed）。

## 问题分级

**must-fix：0 条。**

**should-fix：1 条**
1. deck_rng.rs:79-84——负 cursor「不可达」仅文档注释声明，按 §7 规则 11 应以 `debug_assert!` + TS 依据行（content.ts:473）显式化（当前若假设破裂会以无上下文的 index-out-of-bounds panic 失败）。release 行为不变，W2 接线 DRAW 路径前落即可（也可本波顺手加）。

**note：3 条**
1. messages.md 表行增量（5）≠ 本波 toThrow 数（1）：plan §4-W0「首批 3–5 条」授权 + messages.md:19 已自我对账，后续波沿用该口径。
2. roll_die 未声明 sides 定义域：TS 对负 sides 会得到 ≤0 的小值，Rust `sides as u128` 会得到位型垃圾值——引擎在 engine.ts:767 先拒 `sides < 2` 故不可达，且 W0 无调用方；建议 W1/W3 接线时在 dice.rs 加一行 sides ≥ 2 域注释（或 debug_assert）。
3. `draw_from` 的 `DeckCard` 克隆 vs TS 引用返回：编译后 deck 卡只读，不可观察（(b) 已证）；W2 把 DeckSpec 并入 content.rs 时保持该只读约定即可。

**结论：PASS-with-notes——可打 `rust-port-w0` tag；should-fix-1 由 W2 认领者（或本波作者顺手）处理，不阻塞收口。**
