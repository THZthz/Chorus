//! W4 golden replay harness — plan §4-W4 / §5.4 / §6.2 / PR1-01 / PR2-02 /
//! PR2-09 / PR3-10. For every golden in `tests/goldens/manifest.json`:
//!
//!   1. `fold(load_log(golden.jsonl))` over the driver's base state lands the
//!      manifest `snap` string (verifies fold — the snap.rs key-order
//!      disciplines are the frozen surface);
//!   2. replaying the SAME driver from scratch produces JSONL bytes equal to
//!      the golden file (verifies play; the TS saveLog shape — every log
//!      \n-terminated, so a 0-record golden is one newline byte);
//!   3. the sidecar texts (steps / journal / prose / suggestions) compare
//!      byte-for-byte too — rejections never reach the JSONL, so the steps
//!      sidecar is the only cross-language pin for refusal/rejection texts.
//!
//! The sha256 in the manifest is TS-side bookkeeping and is deliberately NOT
//! verified here (PR2-09: no sha2 dependency; the byte comparison is the
//! judgment). tReal is null everywhere (engine.ts:27/317 — deterministic, no
//! wall clock ever participates).
//!
//! Drivers mirror tools/golden-dump.ts family for family:
//!   - nightjar-main / nightjar-dream / suggestions-nightjar (auto-settle
//!     discipline, ts: nightjar.test.ts:18-32)
//!   - walk-20..100 (the sampled walk with record capture)
//!   - s01..s14 (+s06-plain / s13-high) — the adversarial engine traces
//!   - anchor-* — the 19 ordering anchors (PR1-01); these are the
//!     dump-measured sources for rust/tests/engine_ordering.rs' HAND values
//!   - fuzz-001..200 — reach_fuzz_lib::drive_fuzz_seed under the frozen
//!     selector
//!   - demo — examples/demo.ts journal/prose goldens
//!
//! Failures name the golden, the first divergent line, and both sides' raw
//! text — the §6.2 step-1 minimal report set.

mod common;

use common::explorer_lib::{explorer_world, SCRIPT_TEXT, WIN_PATH};
use common::reach_fuzz_lib;
use common::seed::{seed_content, seed_state};
use common::settle::settle_step;
use common::snap::snap;

use chorus::content::content::{
    compile_content, CompiledContent, ContentRaw, DeckRaw, EndingDef, ExpireTriggerDef,
    PrototypeDef, PrototypeDerive, RecipeDef, SlotDef, SlotTriggerDef, TriggerDef, VerbContent,
};
use chorus::core::clock::{parse_clock, Clock};
use chorus::core::dict::{core_dictionary, Dictionary};
use chorus::core::parse::parse_sentence;
use chorus::core::r#ref::{id_ref, name_ref, Ref};
use chorus::core::sentence::Clause;
use chorus::core::types::{add_entity, clone_state, new_state, AspectVal, Entity, State};
use chorus::engine::engine::{execute_step, ActionRecord, ExecOptions, ExecResult, Step};
use chorus::engine::fold::fold;
use chorus::js::Result;
use chorus::session::game::{Game, TurnKind};
use indexmap::IndexMap;
use serde::Deserialize;

// ---------------------------------------------------------------------------
// manifest + golden I/O
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
struct Manifest {
    #[allow(dead_code)]
    tool: String,
    goldens: Vec<GoldenEntry>,
}

#[derive(Deserialize)]
struct GoldenEntry {
    name: String,
    kind: String,
    #[serde(default)]
    #[allow(dead_code)]
    driver: serde_json::Value,
    /// absent for the reach-cli family (those carry stdout text instead)
    #[serde(default)]
    jsonl: Option<String>,
    /// TS-side bookkeeping only — never verified (PR2-09)
    #[serde(default)]
    #[allow(dead_code)]
    sha256: String,
    #[serde(default)]
    #[allow(dead_code)]
    records: usize,
    snap: String,
    #[serde(default)]
    journal: Option<String>,
    #[serde(default)]
    prose: Option<String>,
    #[serde(default)]
    steps: Option<String>,
    #[serde(default)]
    suggestions: Option<String>,
    /// reach-cli family: the frozen CLI stdout text, re-rendered from the
    /// driver and compared byte for byte (§4-W5, CLI-02 must)
    #[serde(default)]
    stdout: Option<String>,
}

fn manifest() -> Manifest {
    serde_json::from_str(include_str!("goldens/manifest.json")).expect("the golden manifest parses")
}

fn goldens_dir() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests").join("goldens")
}

fn read_golden(rel: &str) -> String {
    std::fs::read_to_string(goldens_dir().join(rel))
        .unwrap_or_else(|e| panic!("cannot read golden {rel}: {e}"))
}

/// ts: store.ts:24-27 — `records.map(JSON.stringify).join('\n') + '\n'` —
/// ALWAYS \n-terminated (a 0-record log is one newline byte). The store's
/// save_log diverges for exactly the empty case, so the harness serializes
/// directly through the same serde impl.
fn to_jsonl(records: &[ActionRecord]) -> String {
    // ts: store.ts:24-27 — `map(JSON.stringify).join('\n') + '\n'`; the join
    // shape keeps the empty log exactly one newline byte
    let lines: Vec<String> = records
        .iter()
        .map(|r| serde_json::to_string(r).expect("a record serializes"))
        .collect();
    let mut body = lines.join("\n");
    body.push('\n');
    body
}

/// TS writer discipline: a trailing newline is appended only when missing.
fn with_nl(s: &str) -> String {
    if s.ends_with('\n') {
        s.to_string()
    } else {
        format!("{s}\n")
    }
}

/// §6.2 step 1 — name, first divergent line number, both raw sides.
fn assert_bytes_eq(golden: &str, played: &str) {
    if golden == played {
        return;
    }
    let g: Vec<&str> = golden.split('\n').collect();
    let p: Vec<&str> = played.split('\n').collect();
    for i in 0..g.len().max(p.len()) {
        match (g.get(i), p.get(i)) {
            (Some(a), Some(b)) if a == b => {}
            (a, b) => panic!(
                "golden diverges at line {}\n  golden: {a:?}\n  played: {b:?}",
                i + 1
            ),
        }
    }
    unreachable!("a byte difference always shows up as a line difference");
}

// ---------------------------------------------------------------------------
// driver plumbing
// ---------------------------------------------------------------------------

/// everything one golden's driver produces — the base state for the fold
/// check, the replayed records for the byte check, and the sidecar texts
struct Played {
    base: State,
    dict: Dictionary,
    content: CompiledContent,
    records: Vec<ActionRecord>,
    journal: Option<String>,
    prose: Option<String>,
    steps: Option<String>,
    suggestions: Option<String>,
}

fn driver_for(name: &str) -> Result<Played> {
    match name {
        "nightjar-main" => nightjar_driver(
            &[
                "wait",
                "explore the first cabin",
                "read the solicitor letter",
                "explore the firebox",
                "read the vane ledger",
                "talk with the spiritualist",
                "seraphine read the wax cylinder",
                "seraphine read the telegram",
                "use on the inquest with the embezzlement the stolen voice the vienna warning",
            ],
            true,
        ),
        "nightjar-dream" => nightjar_driver(
            &[
                "wait",
                "explore the firebox",
                "read the wax cylinder",
                "read the vane ledger",
                "dream with the aether seal",
                "dream with the aether seal",
                "dream with the aether seal",
                "dream with the aether seal",
            ],
            true,
        ),
        "suggestions-nightjar" => suggestions_driver(),
        "demo" => demo_driver(),
        _ if name.starts_with("walk-") => walk_driver(
            name["walk-".len()..].parse::<usize>().expect("walk harvest number"),
        ),
        _ if name.starts_with("fuzz-") => fuzz_driver(
            name["fuzz-".len()..].parse::<u32>().expect("fuzz seed number"),
        ),
        _ if name.starts_with("anchor-") => anchor_driver(name),
        _ => adversarial_driver(name),
    }
}

// ---------------------------------------------------------------------------
// nightjar family (auto-settle discipline — ts: nightjar.test.ts:18-32)
// ---------------------------------------------------------------------------

/// the auto-settle adapter: after a committed action with open windows, the
/// settle rides the same turn. Returns the steps-sidecar line.
fn auto_settle(game: &mut Game, line: &str) -> Result<&'static str> {
    let t = game.handle_line(line)?;
    if t.kind == TurnKind::Action && !game.state.windows.is_empty() {
        game.wait()?;
        Ok("action+wait")
    } else {
        Ok("action")
    }
}

fn nightjar_driver(lines: &[&str], with_steps: bool) -> Result<Played> {
    let world = explorer_world();
    let mut game = Game::from_script(SCRIPT_TEXT, core_dictionary())?;
    let mut slog: Vec<&str> = Vec::new();
    for line in lines {
        slog.push(auto_settle(&mut game, line)?);
    }
    Ok(Played {
        base: world.base,
        dict: world.dict,
        content: world.content,
        records: game.log.records.clone(),
        journal: Some(with_nl(game.journal().first().map(String::as_str).unwrap_or(""))),
        prose: None,
        steps: if with_steps { Some(slog.join("\n") + "\n") } else { None },
        suggestions: None,
    })
}

fn suggestions_driver() -> Result<Played> {
    let world = explorer_world();
    let mut g = Game::from_script(SCRIPT_TEXT, core_dictionary())?;
    let mut sugg: Vec<String> = vec!["[start]".to_string()];
    sugg.extend(g.suggestions()?);
    sugg.push(String::new());
    for line in WIN_PATH {
        auto_settle(&mut g, line)?;
        sugg.push(format!("[after {line}]"));
        sugg.extend(g.suggestions()?);
        sugg.push(String::new());
    }
    Ok(Played {
        base: world.base,
        dict: world.dict,
        content: world.content,
        records: g.log.records.clone(),
        journal: None,
        prose: None,
        steps: None,
        suggestions: Some(sugg.join("\n")),
    })
}

// ---------------------------------------------------------------------------
// walk family (ts: explorer-lib.ts:275-306 with record capture — the tool's
// walkFamily; the harvest points are the `executed % 20` states)
// ---------------------------------------------------------------------------

fn walk_driver(harvest: usize) -> Result<Played> {
    let world = explorer_world();
    // 'NIGH' — the walk pass's raw LCG state, no XOR (ts: explorer-lib.ts:276)
    let mut lcg = chorus::js::Lcg::from_raw_state(0x4e49_4748);
    let mut state = clone_state(&world.base);
    let mut records: Vec<ActionRecord> = Vec::new();
    let mut turn_lengths: Vec<usize> = vec![0];
    let mut id: i64 = 1;
    let mut executed = 0usize;
    let mut i = 0usize;
    while i < 500 {
        i += 1;
        let cands = common::explorer_lib::candidates_for(
            &world.content,
            &world.dict,
            &state,
            &world.player,
            common::explorer_lib::Depth::Sampled,
        );
        let benign: Vec<&common::explorer_lib::Cand> = cands
            .iter()
            .filter(|c| !c.tag.contains("wrong-kind") && c.tag != "clock rewind")
            .collect();
        let pick_from: Vec<&common::explorer_lib::Cand> =
            if lcg.next_f64() < 0.2 || benign.is_empty() {
                cands.iter().collect()
            } else {
                benign
            };
        let pick = ((lcg.next_f64() * pick_from.len() as f64).floor() as usize) % pick_from.len();
        let cand = &pick_from[pick];
        let r = settle_step(&world.content, &world.dict, state, cand.step.clone(), id, None)?;
        state = r.state;
        if r.rejected.is_some() {
            continue;
        }
        turn_lengths.push(records.len());
        let n = r.records.len() as i64;
        records.extend(r.records);
        id += n;
        // §9.35 ending-awareness: undo through the ending step
        if state.ended.is_some() {
            let len = turn_lengths.pop().expect("turn_lengths never empties");
            records.truncate(len);
            state = fold(&records, &clone_state(&world.base), &world.dict, &world.content)?;
            continue;
        }
        executed += 1;
        if executed == harvest {
            return Ok(Played {
                base: world.base,
                dict: world.dict,
                content: world.content,
                records,
                journal: None,
                prose: None,
                steps: None,
                suggestions: None,
            });
        }
    }
    panic!("the walk never reached harvest {harvest}");
}

// ---------------------------------------------------------------------------
// fuzz family (the frozen selector + caller discipline)
// ---------------------------------------------------------------------------

const FUZZ_K: usize = 25;
const FUZZ_ATTEMPT_CAP: usize = 400;

fn fuzz_driver(seed: u32) -> Result<Played> {
    let run = reach_fuzz_lib::drive_fuzz_seed(seed, FUZZ_K, FUZZ_ATTEMPT_CAP)?;
    let sc = reach_fuzz_lib::gen_scenario(seed);
    let dict = core_dictionary();
    let content = compile_content(&sc.raw, &dict, &sc.entity_kinds)?;
    Ok(Played {
        base: run.base,
        dict,
        content,
        records: run.records,
        journal: None,
        prose: None,
        steps: Some(format!("applied {} attempts {}\n", run.applied, run.attempts)),
        suggestions: None,
    })
}

// ---------------------------------------------------------------------------
// shared test-rig helpers (ts: the reach-adversarial / engine_ordering rigs)
// ---------------------------------------------------------------------------

fn s_ent(name: &str, kind: &str) -> Entity {
    s_ent_a(name, kind, IndexMap::new())
}

fn s_ent_a(name: &str, kind: &str, aspects: IndexMap<String, i64>) -> Entity {
    Entity {
        id: format!("s_{}", name.to_lowercase()),
        kind: kind.to_string(),
        name: name.to_string(),
        aspects: aspects.into_iter().map(|(k, v)| (k, AspectVal::Num(v))).collect(),
        attrs: IndexMap::new(),
        born_action: Some(0),
        dead_action: None,
        stackable: false,
    }
}

fn state_with(es: Vec<Entity>) -> State {
    let mut s = new_state(0);
    for e in es {
        add_entity(&mut s, e).expect("scenario entities never collide (unique names)");
    }
    s
}

fn alive_names(s: &State) -> Vec<String> {
    let mut v: Vec<String> = s
        .entities
        .values()
        .filter(|e| e.dead_action.is_none())
        .map(|e| e.name.clone())
        .collect();
    v.sort_by(|a, b| chorus::js::cmp_utf16(a, b));
    v
}

fn a_slot() -> SlotDef {
    SlotDef {
        role: "actors".into(),
        min: 1,
        max: Some(1),
        query: Some("kind:character".into()),
        shown: None,
    }
}

fn o_slot(query: Option<&str>) -> SlotDef {
    SlotDef {
        role: "object".into(),
        min: 0,
        max: Some(1),
        query: query.map(str::to_string),
        shown: None,
    }
}

fn verb_of(name: &str, slots: Vec<SlotDef>, duration: i64, out: &[&str]) -> (String, VerbContent) {
    (
        name.to_string(),
        VerbContent {
            slots,
            duration,
            recipes: vec![RecipeDef {
                id: None,
                when: None,
                shown: None,
                hint: None,
                out: out.iter().map(|s| (*s).to_string()).collect(),
            }],
        },
    )
}

fn verb_recipes(
    name: &str,
    slots: Vec<SlotDef>,
    duration: i64,
    recipes: Vec<RecipeDef>,
) -> (String, VerbContent) {
    (
        name.to_string(),
        VerbContent {
            slots,
            duration,
            recipes,
        },
    )
}

fn recipe(out: &[&str]) -> RecipeDef {
    RecipeDef {
        id: None,
        when: None,
        shown: None,
        hint: None,
        out: out.iter().map(|s| (*s).to_string()).collect(),
    }
}

fn recipe_when(when: &str, out: &[&str]) -> RecipeDef {
    RecipeDef {
        id: None,
        when: Some(when.to_string()),
        shown: None,
        hint: None,
        out: out.iter().map(|s| (*s).to_string()).collect(),
    }
}

fn wait_verb() -> (String, VerbContent) {
    verb_of(
        "WAIT",
        vec![a_slot()],
        30,
        &[],
    )
}

fn verbs_map(pairs: Vec<(String, VerbContent)>) -> IndexMap<String, VerbContent> {
    pairs.into_iter().collect()
}

/// ts: reach-adversarial.test.ts:145-148 — the raw BEGIN (settleStep would
/// complete the windows it opens)
fn mk_begin(content: &CompiledContent, dict: &Dictionary, state: State, src: &str, id: i64) -> ExecResult {
    let s = parse_sentence(src, dict)
        .unwrap_or_else(|e| panic!("mk_begin parse failed for '{src}': {e}"));
    execute_step(
        content,
        dict,
        state,
        Step {
            start_min: None,
            clock: Some(s.clock.unwrap_or(Clock { day: 0, halfhour: 0 })),
            clauses: s.clauses,
            t_real: None,
        },
        ExecOptions {
            first_record_id: id,
            id_gen: None,
        },
    )
    .unwrap()
}

/// ts: reach-adversarial.test.ts:53-71 — the settleStep runner (monotonic ids,
/// nextId += n + 1) with record + sidecar capture
struct Case {
    records: Vec<ActionRecord>,
    slog: Vec<String>,
    next_id: i64,
}

impl Case {
    fn new() -> Case {
        Case {
            records: Vec::new(),
            slog: Vec::new(),
            next_id: 1,
        }
    }

    fn run(&mut self, content: &CompiledContent, dict: &Dictionary, state: State, src: &str) -> ExecResult {
        let s = parse_sentence(src, dict)
            .unwrap_or_else(|e| panic!("run parse failed for '{src}': {e}"));
        let res = settle_step(
            content,
            dict,
            state,
            Step {
                start_min: None,
                clock: Some(s.clock.unwrap_or(Clock { day: 0, halfhour: 0 })),
                clauses: s.clauses,
                t_real: None,
            },
            self.next_id,
            None,
        )
        .unwrap();
        self.next_id += res.records.len() as i64 + 1;
        assert!(
            res.rejected.is_none(),
            "{src}: expected acceptance, got: {:?}",
            res.rejected
        );
        self.slog.push(format!("ok {}", res.records.len()));
        self.records.extend(res.records.iter().cloned());
        res
    }

    fn advance(&mut self, content: &CompiledContent, dict: &Dictionary, state: State, clock: &str) -> ExecResult {
        let res = settle_step(
            content,
            dict,
            state,
            Step {
                start_min: None,
                clock: Some(parse_clock(clock).unwrap()),
                clauses: vec![],
                t_real: None,
            },
            self.next_id,
            None,
        )
        .unwrap();
        self.next_id += res.records.len() as i64 + 1;
        match &res.rejected {
            None => self.slog.push(format!("ok {}", res.records.len())),
            Some(msg) => self.slog.push(format!("rejected {msg}")),
        }
        self.records.extend(res.records.iter().cloned());
        res
    }

    /// a manual settleStep (fixed id, as in the test bodies)
    fn manual(&self, content: &CompiledContent, dict: &Dictionary, state: State, step: Step, id: i64) -> ExecResult {
        settle_step(content, dict, state, step, id, None).unwrap()
    }
}

fn sentence_step(src: &str, dict: &Dictionary) -> Step {
    let s = parse_sentence(src, dict).unwrap();
    Step {
        start_min: None,
        clock: s.clock,
        clauses: s.clauses,
        t_real: None,
    }
}

// ---------------------------------------------------------------------------
// adversarial family — engine traces of reach-adversarial.test.ts S1–S14
// ---------------------------------------------------------------------------

/// the SHARED dictionary — the TS test file compiles every scenario against
/// ONE accumulating coreDictionary(); by the time any scenario replays, all
/// rig verbs ever registered exist. No scenario's sentences contain another
/// scenario's verb lexemes, so the accumulated set is parse-behavior
/// identical for these drivers (the registrations are pure additions).
fn s_dict() -> Dictionary {
    let mut dict = core_dictionary();
    for name in [
        "FORGE",
        "INSPECT",
        "TOUCH",
        "TORCH",
        "PLANT",
        "WATER",
        "TEND",
        "STEW",
        "PROBE",
        "HEAR",
        "RETELL",
        "CONSECRATE",
        "RITUAL",
        "MAKER",
        "SELFMADE",
        "FORGE_LEFT",
        "FORGE_RIGHT",
        "SEANCE",
        "GAMBLE",
        "STUDY",
    ] {
        if !dict.is_verb(name) {
            dict.register_verb(name).unwrap();
        }
    }
    dict
}

fn compile_s(raw: ContentRaw, dict: &Dictionary) -> Result<CompiledContent> {
    compile_content(&raw, dict, &[])
}

/// ts: reach-adversarial.test.ts:77-79 — the programmatic #id clause
fn probe_clause(actors: Vec<Ref>, objects: Vec<Ref>) -> Clause {
    Clause {
        actors,
        at: None,
        verb: "PROBE".to_string(),
        to: None,
        objects,
        with: vec![],
        about: None,
    }
}

fn adversarial_driver(name: &str) -> Result<Played> {
    let dict = s_dict();
    match name {
        "s01" => {
            let content = compile_s(
                ContentRaw {
                    prototypes: Some([("KEYCARD".to_string(), PrototypeDef { kind: "item".into(), ..PrototypeDef::default() })].into_iter().collect()),
                    verbs: verbs_map(vec![
                        verb_of("FORGE", vec![a_slot()], 60, &["SPAWN KEYCARD"]),
                        verb_of("INSPECT", vec![a_slot(), o_slot(Some("kind:item"))], 30, &["NARRATE \"inspected.\""]),
                        verb_of("TOUCH", vec![a_slot(), o_slot(Some("kind:item"))], 30, &["NARRATE \"touched.\""]),
                        wait_verb(),
                    ]),
                    ..ContentRaw::default()
                },
                &dict,
            )?;
            let state = state_with(vec![s_ent("ANA", "character"), s_ent("LAMP", "item")]);
            let mut c = Case::new();
            let mut cur = clone_state(&state);
            let b1 = mk_begin(&content, &dict, cur, "0:0 @ ANA FORGE", 1);
            assert!(b1.rejected.is_none(), "S1 b1 accepted");
            c.slog.push(format!("ok {}", b1.records.len()));
            c.records.extend(b1.records.iter().cloned());
            cur = b1.state;
            assert_eq!(cur.windows.len(), 1);
            let b2 = mk_begin(&content, &dict, cur, "0:0 @ ANA INSPECT LAMP", 2);
            assert!(b2.rejected.is_none(), "S1 b2 accepted");
            c.slog.push(format!("ok {}", b2.records.len()));
            c.records.extend(b2.records.iter().cloned());
            cur = b2.state;
            assert_eq!(cur.windows.len(), 2);
            cur = c.run(&content, &dict, cur, "0:1 @ ANA TOUCH LAMP").state;
            assert!(alive_names(&cur).contains(&"KEYCARD".to_string()));
            cur = c.run(&content, &dict, cur, "0:2 @ ANA INSPECT KEYCARD").state;
            assert!(alive_names(&cur).contains(&"KEYCARD".to_string()));
            Ok(Played {
                base: state,
                dict,
                content,
                records: c.records,
                journal: None,
                prose: None,
                steps: Some(c.slog.join("\n") + "\n"),
                suggestions: None,
            })
        }
        "s02" => {
            let content = compile_s(
                ContentRaw {
                    prototypes: Some([("ASH".to_string(), PrototypeDef { kind: "item".into(), ..PrototypeDef::default() })].into_iter().collect()),
                    verbs: verbs_map(vec![
                        verb_of("TORCH", vec![a_slot()], 0, &["SET(PAPER, expires, 30m)"]),
                        wait_verb(),
                    ]),
                    triggers: Some(vec![TriggerDef::Expire(ExpireTriggerDef {
                        id: "paper_to_ash".to_string(),
                        on_expire_of: "PAPER".to_string(),
                        out: vec!["SPAWN ASH".to_string()],
                    })]),
                    ..ContentRaw::default()
                },
                &dict,
            )?;
            let state = state_with(vec![s_ent("ANA", "character"), s_ent("PAPER", "item")]);
            let mut c = Case::new();
            let cur0 = c.run(&content, &dict, clone_state(&state), "0:0 @ ANA TORCH");
            let mut cur = cur0.state;
            assert_eq!(
                cur.entities.get("s_paper").unwrap().attrs.get("expiresAt"),
                Some(&chorus::core::types::AttrValue::Num(30))
            );
            cur = c.advance(&content, &dict, cur, "0:1").state;
            assert!(alive_names(&cur).contains(&"ASH".to_string()));
            assert!(alive_names(&cur).contains(&"PAPER".to_string()));
            let bomb = c.advance(&content, &dict, cur, "0:2");
            let rejected = bomb.rejected.clone().expect("S2 second crossing rejected");
            assert!(rejected.contains("ASH"));
            assert!(alive_names(&bomb.state).contains(&"ASH".to_string()));
            assert!(alive_names(&bomb.state).contains(&"PAPER".to_string()));
            Ok(Played {
                base: state,
                dict,
                content,
                records: c.records,
                journal: None,
                prose: None,
                steps: Some(c.slog.join("\n") + "\n"),
                suggestions: None,
            })
        }
        "s03" => {
            let proto = |n: &str| (n.to_string(), PrototypeDef { kind: "item".into(), ..PrototypeDef::default() });
            let content = compile_s(
                ContentRaw {
                    prototypes: Some([proto("SEED"), proto("SPROUT"), proto("TREE")].into_iter().collect()),
                    verbs: verbs_map(vec![
                        verb_of("PLANT", vec![a_slot()], 0, &["SPAWN SEED"]),
                        verb_of("WATER", vec![a_slot()], 0, &["REPLACE SEED WITH SPROUT"]),
                        verb_of("TEND", vec![a_slot()], 0, &["REPLACE SPROUT WITH TREE"]),
                    ]),
                    ..ContentRaw::default()
                },
                &dict,
            )?;
            let state = state_with(vec![s_ent("ANA", "character")]);
            let mut c = Case::new();
            let cur = c.run(&content, &dict, clone_state(&state), "0:0 @ ANA PLANT").state;
            assert!(alive_names(&cur).contains(&"SEED".to_string()));
            let cur = c.run(&content, &dict, cur, "0:0 @ ANA WATER").state;
            assert!(!alive_names(&cur).contains(&"SEED".to_string()));
            assert!(alive_names(&cur).contains(&"SPROUT".to_string()));
            let cur = c.run(&content, &dict, cur, "0:0 @ ANA TEND").state;
            assert!(!alive_names(&cur).contains(&"SPROUT".to_string()));
            assert!(alive_names(&cur).contains(&"TREE".to_string()));
            Ok(Played {
                base: state,
                dict,
                content,
                records: c.records,
                journal: None,
                prose: None,
                steps: Some(c.slog.join("\n") + "\n"),
                suggestions: None,
            })
        }
        "s04" => {
            let content = compile_s(
                ContentRaw {
                    prototypes: Some(
                        [
                            (
                                "SUSPICION".to_string(),
                                PrototypeDef {
                                    kind: "state".into(),
                                    stackable: true,
                                    ..PrototypeDef::default()
                                },
                            ),
                            ("INSIGHT".to_string(), PrototypeDef { kind: "idea".into(), ..PrototypeDef::default() }),
                        ]
                        .into_iter()
                        .collect(),
                    ),
                    verbs: verbs_map(vec![
                        verb_of("STEW", vec![a_slot()], 0, &["SPAWN SUSPICION"]),
                        verb_recipes(
                            "PROBE",
                            vec![a_slot(), SlotDef { role: "object".into(), min: 2, max: Some(2), query: Some("kind:state".into()), shown: None }],
                            0,
                            vec![recipe(&["SPAWN INSIGHT"])],
                        ),
                    ]),
                    ..ContentRaw::default()
                },
                &dict,
            )?;
            let state = state_with(vec![s_ent("ANA", "character")]);
            let mut c = Case::new();
            let mut cur = c.run(&content, &dict, clone_state(&state), "0:0 @ ANA STEW").state;
            cur = c.run(&content, &dict, cur, "0:0 @ ANA STEW").state;
            let pool: Vec<String> = cur.by_name.get("SUSPICION").unwrap().clone();
            assert_eq!(pool.len(), 2);
            let single = c.manual(
                &content,
                &dict,
                clone_state(&cur),
                Step {
                    start_min: None,
                    clock: Some(parse_clock("0:0").unwrap()),
                    clauses: vec![Clause {
                        actors: vec![name_ref("ANA", None)],
                        at: None,
                        verb: "PROBE".to_string(),
                        to: None,
                        objects: vec![id_ref(pool[0].clone())],
                        with: vec![],
                        about: None,
                    }],
                    t_real: None,
                },
                800,
            );
            let rejected = single.rejected.clone().expect("S4 under-fill rejected");
            assert!(rejected.contains("'object' takes 2..2, got 1"), "{rejected}");
            c.slog.push(format!("rejected {rejected}"));
            let probe = c.manual(
                &content,
                &dict,
                cur,
                Step {
                    start_min: None,
                    clock: Some(parse_clock("0:0").unwrap()),
                    clauses: vec![probe_clause(
                        vec![name_ref("ANA", None)],
                        pool.iter().map(|id| id_ref(id.clone())).collect(),
                    )],
                    t_real: None,
                },
                900,
            );
            assert!(probe.rejected.is_none(), "S4 #id-bound PROBE commits");
            c.records.extend(probe.records.iter().cloned());
            c.slog.push(format!("ok {}", probe.records.len()));
            assert!(alive_names(&probe.state).contains(&"INSIGHT".to_string()));
            Ok(Played {
                base: state,
                dict,
                content,
                records: c.records,
                journal: None,
                prose: None,
                steps: Some(c.slog.join("\n") + "\n"),
                suggestions: None,
            })
        }
        "s05" => {
            let derive_proto = |kind: &str| PrototypeDef {
                kind: kind.into(),
                derive: Some(PrototypeDerive {
                    name_from: Some("object".into()),
                    aspects: None,
                }),
                ..PrototypeDef::default()
            };
            let content = compile_s(
                ContentRaw {
                    prototypes: Some(
                        [
                            ("RUMOUR".to_string(), derive_proto("idea")),
                            ("WHISPER".to_string(), derive_proto("idea")),
                        ]
                        .into_iter()
                        .collect(),
                    ),
                    verbs: verbs_map(vec![
                        verb_of("HEAR", vec![a_slot(), o_slot(Some("kind:item"))], 0, &["SPAWN RUMOUR"]),
                        verb_of("RETELL", vec![a_slot(), o_slot(Some("kind:idea"))], 0, &["SPAWN WHISPER"]),
                    ]),
                    ..ContentRaw::default()
                },
                &dict,
            )?;
            let mut aspects = IndexMap::new();
            aspects.insert("secret".to_string(), 2i64);
            let state = state_with(vec![s_ent("ANA", "character"), s_ent_a("THE_BOOK", "item", aspects)]);
            let mut c = Case::new();
            let cur = c.run(&content, &dict, clone_state(&state), "0:0 @ ANA HEAR THE_BOOK").state;
            assert!(alive_names(&cur).contains(&"RUMOUR_THE_BOOK".to_string()));
            let cur = c.run(&content, &dict, cur, "0:0 @ ANA RETELL RUMOUR_THE_BOOK").state;
            assert!(alive_names(&cur).contains(&"WHISPER_RUMOUR_THE_BOOK".to_string()));
            assert!(alive_names(&cur).contains(&"RUMOUR_THE_BOOK".to_string()));
            Ok(Played {
                base: state,
                dict,
                content,
                records: c.records,
                journal: None,
                prose: None,
                steps: Some(c.slog.join("\n") + "\n"),
                suggestions: None,
            })
        }
        "s06" | "s06-plain" => {
            let stackable = name == "s06";
            let proto = |n: &str| {
                (
                    n.to_string(),
                    PrototypeDef {
                        kind: "item".into(),
                        stackable,
                        ..PrototypeDef::default()
                    },
                )
            };
            let content = compile_s(
                ContentRaw {
                    prototypes: Some(
                        [proto("DAWN"), proto("DUSK"), proto("EMBER"), proto("ECHO")].into_iter().collect(),
                    ),
                    decks: Some(vec![
                        DeckRaw {
                            id: "CYCLE".to_string(),
                            spec: vec!["DAWN".to_string(), "DUSK".to_string()],
                            defaultcard: None,
                            reset: true,
                        },
                        DeckRaw {
                            id: "FALLBACK".to_string(),
                            spec: vec!["EMBER".to_string()],
                            defaultcard: Some("ECHO".to_string()),
                            reset: false,
                        },
                    ]),
                    triggers: Some(vec![
                        TriggerDef::Slot(SlotTriggerDef { id: "draw_c1".into(), slot: 1, once: None, out: vec!["DRAW CYCLE".into()] }),
                        TriggerDef::Slot(SlotTriggerDef { id: "draw_c2".into(), slot: 2, once: None, out: vec!["DRAW CYCLE".into()] }),
                        TriggerDef::Slot(SlotTriggerDef { id: "draw_f1".into(), slot: 3, once: None, out: vec!["DRAW FALLBACK".into()] }),
                        TriggerDef::Slot(SlotTriggerDef { id: "draw_f2".into(), slot: 4, once: None, out: vec!["DRAW FALLBACK".into()] }),
                        TriggerDef::Slot(SlotTriggerDef { id: "draw_c3".into(), slot: 5, once: None, out: vec!["DRAW CYCLE".into()] }),
                    ]),
                    verbs: verbs_map(vec![wait_verb()]),
                    ..ContentRaw::default()
                },
                &dict,
            )?;
            let state = state_with(vec![s_ent("ANA", "character")]);
            let mut c = Case::new();
            let mut cur = clone_state(&state);
            let last = if stackable { 5 } else { 3 };
            for i in 1..=last {
                cur = c.run(&content, &dict, cur, &format!("0:{i} @ ANA WAIT")).state;
            }
            if stackable {
                for n in ["DAWN", "DUSK", "EMBER", "ECHO"] {
                    assert!(alive_names(&cur).contains(&n.to_string()), "{n} alive");
                }
                assert_eq!(cur.by_name.get("DUSK").map(Vec::len).unwrap_or(0), 2);
            } else {
                let mut names = alive_names(&cur);
                names.sort();
                assert_eq!(names, vec!["ANA", "DAWN", "DUSK", "ECHO", "EMBER"]);
                let boom = c.advance(&content, &dict, cur, "0:5");
                let rejected = boom.rejected.clone().expect("S6-plain freeze");
                assert!(rejected.contains("DUSK"));
            }
            Ok(Played {
                base: state,
                dict,
                content,
                records: c.records,
                journal: None,
                prose: None,
                steps: Some(c.slog.join("\n") + "\n"),
                suggestions: None,
            })
        }
        "s07" => {
            let content = compile_s(
                ContentRaw {
                    prototypes: Some(
                        [
                            ("RELIC".to_string(), PrototypeDef { kind: "item".into(), ..PrototypeDef::default() }),
                            (
                                "RITE_PROTO".to_string(),
                                PrototypeDef {
                                    kind: "rite".into(),
                                    stackable: true,
                                    ..PrototypeDef::default()
                                },
                            ),
                        ]
                        .into_iter()
                        .collect(),
                    ),
                    verbs: verbs_map(vec![verb_of("CONSECRATE", vec![a_slot()], 0, &["SPAWN rite:RELIC"])]),
                    ..ContentRaw::default()
                },
                &dict,
            )?;
            let state = state_with(vec![s_ent("ANA", "character")]);
            let mut c = Case::new();
            let mut cur = c.run(&content, &dict, clone_state(&state), "0:0 @ ANA CONSECRATE").state;
            cur = c.run(&content, &dict, cur, "0:0 @ ANA CONSECRATE").state;
            let pool = cur.by_name.get("RELIC").unwrap();
            assert_eq!(pool.len(), 2);
            for id in pool {
                assert_eq!(cur.entities.get(id).unwrap().kind, "rite");
                assert!(cur.entities.get(id).unwrap().stackable);
            }
            Ok(Played {
                base: state,
                dict,
                content,
                records: c.records,
                journal: None,
                prose: None,
                steps: Some(c.slog.join("\n") + "\n"),
                suggestions: None,
            })
        }
        "s08" => {
            let triple = || ContentRaw {
                prototypes: Some(
                    [
                        ("P_TRIPLE".to_string(), PrototypeDef { kind: "item".into(), ..PrototypeDef::default() }),
                        ("NAME_A".to_string(), PrototypeDef { kind: "idea".into(), ..PrototypeDef::default() }),
                        ("NAME_B".to_string(), PrototypeDef { kind: "idea".into(), ..PrototypeDef::default() }),
                        ("NAME_C".to_string(), PrototypeDef { kind: "idea".into(), ..PrototypeDef::default() }),
                    ]
                    .into_iter()
                    .collect(),
                ),
                triggers: Some(
                    [
                        ("ta", "NAME_A"),
                        ("tb", "NAME_B"),
                        ("tc", "NAME_C"),
                    ]
                    .iter()
                    .map(|(id, name)| {
                        TriggerDef::Slot(SlotTriggerDef {
                            id: (*id).to_string(),
                            slot: 5,
                            once: None,
                            out: vec!["SPAWN P_TRIPLE".to_string(), format!("SPAWN {name}")],
                        })
                    })
                    .collect::<Vec<_>>(),
                ),
                verbs: verbs_map(vec![wait_verb()]),
                ..ContentRaw::default()
            };
            let content = compile_s(triple(), &dict)?;
            let state = state_with(vec![s_ent("ANA", "character")]);
            let mut c = Case::new();
            let early = c.advance(&content, &dict, clone_state(&state), "0:4");
            assert_eq!(alive_names(&early.state), vec!["ANA"]);
            let boom = c.advance(&content, &dict, early.state, "0:5");
            let rejected = boom.rejected.clone().expect("S8 slot 5 rejects");
            assert!(rejected.contains("P_TRIPLE"));
            assert_eq!(alive_names(&boom.state), vec!["ANA"]);
            let again = c.advance(&content, &dict, clone_state(&state), "0:6");
            assert!(again.rejected.is_some(), "S8 the freeze is permanent");
            Ok(Played {
                base: state,
                dict,
                content,
                records: c.records,
                journal: None,
                prose: None,
                steps: Some(c.slog.join("\n") + "\n"),
                suggestions: None,
            })
        }
        "s09" => {
            let content = compile_s(
                ContentRaw {
                    prototypes: Some([("GOAL".to_string(), PrototypeDef { kind: "item".into(), ..PrototypeDef::default() })].into_iter().collect()),
                    verbs: verbs_map(vec![verb_recipes(
                        "RITUAL",
                        vec![a_slot()],
                        0,
                        vec![recipe_when("exists(ROPE)", &["SPAWN GOAL"])],
                    )]),
                    triggers: Some(vec![TriggerDef::Slot(SlotTriggerDef {
                        id: "rope_snaps".into(),
                        slot: 2,
                        once: None,
                        out: vec!["DESTROY ROPE".into()],
                    })]),
                    ..ContentRaw::default()
                },
                &dict,
            )?;
            let state = state_with(vec![s_ent("ANA", "character"), s_ent("ROPE", "item")]);
            let mut c = Case::new();
            let cur = c.run(&content, &dict, clone_state(&state), "0:0 @ ANA RITUAL").state;
            assert!(alive_names(&cur).contains(&"GOAL".to_string()));
            let cur = c.advance(&content, &dict, cur, "0:2").state;
            assert!(!alive_names(&cur).contains(&"ROPE".to_string()));
            let after = c.manual(&content, &dict, cur, sentence_step("0:2 @ ANA RITUAL", &dict), 700);
            let rejected = after.rejected.clone().expect("S9 post-kill rejected");
            assert!(rejected.contains("no recipe matched"));
            c.slog.push(format!("rejected {rejected}"));
            Ok(Played {
                base: state,
                dict,
                content,
                records: c.records,
                journal: None,
                prose: None,
                steps: Some(c.slog.join("\n") + "\n"),
                suggestions: None,
            })
        }
        "s10" => {
            let content = compile_s(
                ContentRaw {
                    prototypes: Some(
                        [
                            ("X".to_string(), PrototypeDef { kind: "item".into(), ..PrototypeDef::default() }),
                            ("GOAL".to_string(), PrototypeDef { kind: "idea".into(), ..PrototypeDef::default() }),
                        ]
                        .into_iter()
                        .collect(),
                    ),
                    verbs: verbs_map(vec![
                        verb_of("MAKER", vec![a_slot()], 0, &["SPAWN X"]),
                        verb_of("SELFMADE", vec![a_slot(), SlotDef { role: "with".into(), min: 1, max: Some(1), query: Some("kind:item".into()), shown: None }], 0, &["SPAWN X", "SPAWN GOAL"]),
                    ]),
                    ..ContentRaw::default()
                },
                &dict,
            )?;
            let state = state_with(vec![s_ent("ANA", "character")]);
            let mut c = Case::new();
            let cur = c.run(&content, &dict, clone_state(&state), "0:0 @ ANA MAKER").state;
            assert!(alive_names(&cur).contains(&"X".to_string()));
            let attempt = c.manual(&content, &dict, cur, sentence_step("0:0 @ ANA SELFMADE WITH X", &dict), 600);
            assert!(attempt.rejected.is_none(), "S10 a refusal is not a rejection");
            c.records.extend(attempt.records.iter().cloned());
            c.slog.push(format!("ok {}", attempt.records.len()));
            assert!(attempt
                .records
                .iter()
                .any(|rec| rec.outcomes.iter().any(|o| o.op == "REFUSE")));
            assert!(!alive_names(&attempt.state).contains(&"GOAL".to_string()));
            assert!(alive_names(&attempt.state).contains(&"X".to_string()));
            Ok(Played {
                base: state,
                dict,
                content,
                records: c.records,
                journal: None,
                prose: None,
                steps: Some(c.slog.join("\n") + "\n"),
                suggestions: None,
            })
        }
        "s11" => {
            let content = compile_s(
                ContentRaw {
                    prototypes: Some(
                        [
                            ("LEFT".to_string(), PrototypeDef { kind: "item".into(), ..PrototypeDef::default() }),
                            ("RIGHT".to_string(), PrototypeDef { kind: "item".into(), ..PrototypeDef::default() }),
                        ]
                        .into_iter()
                        .collect(),
                    ),
                    verbs: verbs_map(vec![
                        verb_of("FORGE_LEFT", vec![a_slot()], 30, &["SPAWN LEFT"]),
                        verb_of("FORGE_RIGHT", vec![a_slot()], 60, &["SPAWN RIGHT"]),
                        wait_verb(),
                    ]),
                    ..ContentRaw::default()
                },
                &dict,
            )?;
            let state = state_with(vec![s_ent("ANA", "character")]);
            let mut c = Case::new();
            let begin = mk_begin(&content, &dict, clone_state(&state), "0:0 @ ANA FORGE_LEFT; ANA FORGE_RIGHT", 1);
            assert!(begin.rejected.is_none());
            c.slog.push(format!("ok {}", begin.records.len()));
            c.records.extend(begin.records.iter().cloned());
            let mut windows: Vec<(String, i64, i64)> = begin
                .state
                .windows
                .values()
                .map(|w| (w.verb.clone(), w.start, w.end))
                .collect();
            windows.sort_by_key(|(_, start, _)| *start);
            assert_eq!(
                windows,
                vec![
                    ("FORGE_LEFT".to_string(), 0, 30),
                    ("FORGE_RIGHT".to_string(), 30, 90)
                ]
            );
            // ts: S11's mid advance is a pure clock step (parseClock, no
            // sentence) — the lone-clock advance
            let mid = run_clock(&content, &dict, begin.state, "0:1", 2);
            assert!(mid.rejected.is_none());
            c.slog.push(format!("ok {}", mid.records.len()));
            c.records.extend(mid.records.iter().cloned());
            assert!(alive_names(&mid.state).contains(&"LEFT".to_string()));
            assert!(!alive_names(&mid.state).contains(&"RIGHT".to_string()));
            c.advance(&content, &dict, mid.state, "0:3");
            Ok(Played {
                base: state,
                dict,
                content,
                records: c.records,
                journal: None,
                prose: None,
                steps: Some(c.slog.join("\n") + "\n"),
                suggestions: None,
            })
        }
        "s12" => {
            let content = compile_s(
                ContentRaw {
                    prototypes: Some([("PHANTOM".to_string(), PrototypeDef { kind: "idea".into(), ..PrototypeDef::default() })].into_iter().collect()),
                    verbs: verbs_map(vec![verb_recipes(
                        "SEANCE",
                        vec![a_slot()],
                        0,
                        vec![recipe_when("exists(GHOST)", &["SPAWN PHANTOM"])],
                    )]),
                    ..ContentRaw::default()
                },
                &dict,
            )?;
            let state = state_with(vec![s_ent("ANA", "character")]);
            let c = Case::new();
            let attempt = settle_step(
                &content,
                &dict,
                clone_state(&state),
                sentence_step("0:0 @ ANA SEANCE", &dict),
                500,
                None,
            )
            .unwrap();
            let rejected = attempt.rejected.clone().expect("S12 rejected at the table");
            assert!(rejected.contains("no recipe matched"));
            let mut slog = c.slog;
            slog.push(format!("rejected {rejected}"));
            Ok(Played {
                base: state,
                dict,
                content,
                records: vec![],
                journal: None,
                prose: None,
                steps: Some(slog.join("\n") + "\n"),
                suggestions: None,
            })
        }
        "s13-low" | "s13-high" => {
            let low = (0i64..).find(|&s| chorus::core::dice::roll_die(s, 0, 6) < 4).unwrap();
            let high = (0i64..).find(|&s| chorus::core::dice::roll_die(s, 0, 6) >= 4).unwrap();
            let gamble = |seed: i64| ContentRaw {
                seed: Some(seed),
                prototypes: Some([("PRIZE".to_string(), PrototypeDef { kind: "item".into(), ..PrototypeDef::default() })].into_iter().collect()),
                verbs: verbs_map(vec![verb_recipes(
                    "GAMBLE",
                    vec![a_slot()],
                    0,
                    vec![recipe(&["ROLL(1, 6), IF roll >= 4 -> (SPAWN PRIZE) ELSE -> (REFUSE(\"frozen — the house always wins\"))"])],
                )]),
                ..ContentRaw::default()
            };
            let state = state_with(vec![s_ent("ANA", "character")]);
            if name == "s13-low" {
                let content = compile_s(gamble(low), &dict)?;
                let mut c = Case::new();
                let mut cur = clone_state(&state);
                for i in 0..3 {
                    let attempt = c.manual(&content, &dict, cur, sentence_step("0:0 @ ANA GAMBLE", &dict), 400 + i);
                    assert!(attempt.rejected.is_none(), "S13 low: refused, not rejected");
                    c.records.extend(attempt.records.iter().cloned());
                    assert_eq!(attempt.state.rng, 0, "S13 low: the counter never moved");
                    assert!(!alive_names(&attempt.state).contains(&"PRIZE".to_string()));
                    c.slog.push(format!("ok {}", attempt.records.len()));
                    cur = attempt.state;
                }
                Ok(Played {
                    base: state,
                    dict,
                    content,
                    records: c.records,
                    journal: None,
                    prose: None,
                    steps: Some(c.slog.join("\n") + "\n"),
                    suggestions: None,
                })
            } else {
                let content = compile_s(gamble(high), &dict)?;
                let win = settle_step(
                    &content,
                    &dict,
                    clone_state(&state),
                    sentence_step("0:0 @ ANA GAMBLE", &dict),
                    399,
                    None,
                )
                .unwrap();
                assert!(win.rejected.is_none());
                assert!(alive_names(&win.state).contains(&"PRIZE".to_string()));
                assert_eq!(win.state.rng, 1);
                Ok(Played {
                    base: state,
                    dict,
                    content,
                    records: win.records.clone(),
                    journal: None,
                    prose: None,
                    steps: Some(format!("ok {}\n", win.records.len())),
                    suggestions: None,
                })
            }
        }
        "s14" => {
            // analysis-only scenario: no engine steps, nothing logs
            let content = compile_s(
                ContentRaw {
                    prototypes: Some([("PRIZE".to_string(), PrototypeDef { kind: "idea".into(), ..PrototypeDef::default() })].into_iter().collect()),
                    triggers: Some(vec![
                        TriggerDef::Expire(ExpireTriggerDef {
                            id: "t_arms".into(),
                            on_expire_of: "CRATE".into(),
                            out: vec!["SET(ANA, expires, 30m)".into()],
                        }),
                        TriggerDef::Expire(ExpireTriggerDef {
                            id: "t_grand".into(),
                            on_expire_of: "ANA".into(),
                            out: vec!["SPAWN PRIZE".into()],
                        }),
                    ]),
                    verbs: verbs_map(vec![wait_verb()]),
                    ..ContentRaw::default()
                },
                &dict,
            )?;
            let state = state_with(vec![s_ent("ANA", "character"), s_ent("CRATE", "item")]);
            Ok(Played {
                base: state,
                dict,
                content,
                records: vec![],
                journal: None,
                prose: None,
                steps: Some("no engine steps (analysis-only scenario)\n".to_string()),
                suggestions: None,
            })
        }
        other => panic!("no adversarial driver for '{other}'"),
    }
}

// ---------------------------------------------------------------------------
// anchor family (PR1-01) — the 19 ordering anchors; rigs mirror
// rust/tests/engine_ordering.rs exactly
// ---------------------------------------------------------------------------

fn ordering_seed() -> State {
    let mut s = new_state(0);
    let card = |id: &str, kind: &str, name: &str, attrs: IndexMap<String, chorus::core::types::AttrValue>| Entity {
        id: id.to_string(),
        kind: kind.to_string(),
        name: name.to_string(),
        aspects: IndexMap::new(),
        attrs,
        born_action: None,
        dead_action: None,
        stackable: false,
    };
    let base = |container: &str| {
        let mut a = IndexMap::new();
        a.insert("container".to_string(), chorus::core::types::AttrValue::Str(container.to_string()));
        a
    };
    add_entity(&mut s, card("house", "place", "HOUSE", IndexMap::new())).unwrap();
    add_entity(&mut s, card("ana", "character", "ANA", base("house"))).unwrap();
    add_entity(&mut s, card("bertha", "character", "BERTHA", base("house"))).unwrap();
    add_entity(&mut s, card("book", "text", "MAGICK_BOOK", base("house"))).unwrap();
    s
}

fn expiring_item(s: &mut State, id: &str, name: &str, at: i64) {
    let mut attrs = IndexMap::new();
    attrs.insert("container".to_string(), chorus::core::types::AttrValue::Str("house".to_string()));
    attrs.insert("expiresAt".to_string(), chorus::core::types::AttrValue::Num(at));
    add_entity(
        s,
        Entity {
            id: id.to_string(),
            kind: "item".to_string(),
            name: name.to_string(),
            aspects: IndexMap::new(),
            attrs,
            born_action: None,
            dead_action: None,
            stackable: false,
        },
    )
    .unwrap();
}

fn rig(
    verbs: Vec<(String, VerbContent)>,
    extra: impl FnOnce(&mut ContentRaw),
) -> (Dictionary, CompiledContent) {
    let mut dict = core_dictionary();
    let mut map = IndexMap::new();
    for (name, v) in verbs {
        if !dict.is_verb(&name) {
            dict.register_verb(&name).unwrap();
        }
        map.insert(name, v);
    }
    let mut raw = ContentRaw {
        verbs: map,
        ..ContentRaw::default()
    };
    extra(&mut raw);
    let content = compile_content(&raw, &dict, &[]).unwrap();
    (dict, content)
}

fn run_step(content: &CompiledContent, dict: &Dictionary, state: State, src: &str, first: i64) -> ExecResult {
    let s = parse_sentence(src, dict).unwrap();
    execute_step(
        content,
        dict,
        state,
        Step {
            start_min: None,
            clock: Some(s.clock.unwrap_or(Clock { day: 0, halfhour: 0 })),
            clauses: s.clauses,
            t_real: None,
        },
        ExecOptions {
            first_record_id: first,
            id_gen: None,
        },
    )
    .unwrap()
}

fn run_clock(content: &CompiledContent, dict: &Dictionary, state: State, clock: &str, first: i64) -> ExecResult {
    execute_step(
        content,
        dict,
        state,
        Step {
            start_min: None,
            clock: Some(parse_clock(clock).unwrap()),
            clauses: vec![],
            t_real: None,
        },
        ExecOptions {
            first_record_id: first,
            id_gen: None,
        },
    )
    .unwrap()
}

fn anchor_driver(name: &str) -> Result<Played> {
    let build = |dict: Dictionary,
                 content: CompiledContent,
                 base: State,
                 records: Vec<ActionRecord>,
                 steps: Vec<String>| {
        Ok(Played {
            base,
            dict,
            content,
            records,
            journal: None,
            prose: None,
            steps: Some(steps.join("\n") + "\n"),
            suggestions: None,
        })
    };
    match name {
        "anchor-complete-before-slot-event" => {
            let (dict, content) = rig(
                vec![verb_of("PACE_A", vec![a_slot()], 60, &["NARRATE \"a done.\""])],
                |raw| {
                    raw.triggers = Some(vec![TriggerDef::Slot(SlotTriggerDef {
                        id: "the_chime".into(),
                        slot: 2,
                        once: None,
                        out: vec!["NARRATE \"the halfhour chime sings.\"".into()],
                    })]);
                },
            );
            let base = ordering_seed();
            let mut steps = Vec::new();
            let mut records = Vec::new();
            let r1 = run_step(&content, &dict, clone_state(&base), "0:0 @ ANA PACE_A", 1);
            steps.push(format!("ok {}", r1.records.len()));
            records.extend(r1.records.iter().cloned());
            let adv = run_clock(&content, &dict, r1.state, "0:2", r1.records.len() as i64 + 1);
            steps.push(format!("ok {}", adv.records.len()));
            records.extend(adv.records.iter().cloned());
            build(dict, content, base, records, steps)
        }
        "anchor-advance-chronological" => {
            let (dict, content) = rig(
                vec![
                    verb_of("SLOW", vec![a_slot()], 90, &["NARRATE \"slow.\""]),
                    verb_of("FAST", vec![a_slot()], 30, &["NARRATE \"fast.\""]),
                ],
                |raw| {
                    raw.triggers = Some(vec![
                        TriggerDef::Slot(SlotTriggerDef { id: "halfhour_mark".into(), slot: 1, once: None, out: vec!["NARRATE \"mark.\"".into()] }),
                        TriggerDef::Slot(SlotTriggerDef { id: "ninety_mark".into(), slot: 3, once: None, out: vec!["NARRATE \"ninety.\"".into()] }),
                    ]);
                },
            );
            let base = ordering_seed();
            let mut steps = Vec::new();
            let mut records = Vec::new();
            let r1 = run_step(&content, &dict, clone_state(&base), "0:0 @ ANA SLOW", 1);
            steps.push(format!("ok {}", r1.records.len()));
            records.extend(r1.records.iter().cloned());
            let r2 = run_step(&content, &dict, r1.state, "0:0 @ BERTHA FAST", r1.records.len() as i64 + 1);
            steps.push(format!("ok {}", r2.records.len()));
            records.extend(r2.records.iter().cloned());
            let adv = run_clock(&content, &dict, r2.state, "0:3", (r1.records.len() + r2.records.len()) as i64 + 1);
            steps.push(format!("ok {}", adv.records.len()));
            records.extend(adv.records.iter().cloned());
            build(dict, content, base, records, steps)
        }
        "anchor-due-tie" => {
            let (dict, content) = rig(
                vec![
                    verb_of("PACE_A", vec![a_slot()], 30, &["NARRATE \"a.\""]),
                    verb_of("PACE_B", vec![a_slot()], 30, &["NARRATE \"b.\""]),
                ],
                |_| {},
            );
            let base = ordering_seed();
            let mut steps = Vec::new();
            let mut records = Vec::new();
            let r1 = run_step(&content, &dict, clone_state(&base), "0:0 @ ANA PACE_A", 1);
            steps.push(format!("ok {}", r1.records.len()));
            records.extend(r1.records.iter().cloned());
            let r2 = run_step(&content, &dict, r1.state, "0:0 @ ANA PACE_B", 2);
            steps.push(format!("ok {}", r2.records.len()));
            records.extend(r2.records.iter().cloned());
            let adv = run_clock(&content, &dict, r2.state, "0:1", 3);
            steps.push(format!("ok {}", adv.records.len()));
            records.extend(adv.records.iter().cloned());
            build(dict, content, base, records, steps)
        }
        "anchor-due-tie-clause" => {
            let (dict, content) = rig(
                vec![
                    verb_of("PACE_A", vec![a_slot()], 30, &["NARRATE \"a.\""]),
                    verb_of("PACE_Z", vec![a_slot()], 0, &["NARRATE \"z.\""]),
                ],
                |_| {},
            );
            let base = ordering_seed();
            let mut steps = Vec::new();
            let mut records = Vec::new();
            let s = parse_sentence("0:0 @ ANA PACE_A; ANA PACE_Z", &dict).unwrap();
            assert_eq!(s.clauses.len(), 2);
            let r = execute_step(
                &content,
                &dict,
                clone_state(&base),
                Step {
                    start_min: None,
                    clock: Some(Clock { day: 0, halfhour: 0 }),
                    clauses: s.clauses,
                    t_real: None,
                },
                ExecOptions { first_record_id: 1, id_gen: None },
            )
            .unwrap();
            assert!(r.rejected.is_none());
            steps.push(format!("ok {}", r.records.len()));
            records.extend(r.records.iter().cloned());
            let adv = run_clock(&content, &dict, r.state, "0:1", r.records.len() as i64 + 1);
            steps.push(format!("ok {}", adv.records.len()));
            records.extend(adv.records.iter().cloned());
            build(dict, content, base, records, steps)
        }
        "anchor-expiry-insertion-order" => {
            let (dict, content) = rig(vec![], |_| {});
            let mut state = ordering_seed();
            expiring_item(&mut state, "z_card", "ZOMBIE_CARD", 30); // insertion order, NOT name order
            expiring_item(&mut state, "a_card", "APPLE_CARD", 30);
            let base = clone_state(&state);
            let adv = run_clock(&content, &dict, state, "0:1", 1);
            let steps = vec![format!("ok {}", adv.records.len())];
            build(dict, content, base, adv.records.clone(), steps)
        }
        "anchor-on-expire-first-match" => {
            let (dict, content) = rig(vec![], |raw| {
                raw.triggers = Some(vec![
                    TriggerDef::Expire(ExpireTriggerDef {
                        id: "decay_first".into(),
                        on_expire_of: "INFLUENCE_MOTH_G".into(),
                        out: vec!["NARRATE \"first wins.\"".into()],
                    }),
                    TriggerDef::Expire(ExpireTriggerDef {
                        id: "decay_second".into(),
                        on_expire_of: "INFLUENCE_MOTH_G".into(),
                        out: vec!["NARRATE \"never fires.\"".into()],
                    }),
                ]);
                raw.prototypes = Some(
                    [("PARANOIA".to_string(), PrototypeDef { kind: "state".into(), ..PrototypeDef::default() })]
                        .into_iter()
                        .collect(),
                );
            });
            let mut state = ordering_seed();
            expiring_item(&mut state, "moth_g", "INFLUENCE_MOTH_G", 30);
            expiring_item(&mut state, "dust", "DUST", 30);
            let base = clone_state(&state);
            let adv = run_clock(&content, &dict, state, "0:1", 1);
            let steps = vec![format!("ok {}", adv.records.len())];
            build(dict, content, base, adv.records.clone(), steps)
        }
        "anchor-expiry-hoist-no-cascade" => {
            let (dict, content) = rig(
                vec![verb_of("INCUBATE", vec![a_slot(), o_slot(Some("kind:item"))], 30, &["SET(OBJECT, expires, 1h)"])],
                |_| {},
            );
            let mut state = ordering_seed();
            // 2^53-1: "no expiry of its own yet", representable identically in TS and Rust
            expiring_item(&mut state, "egg", "EGG", 9_007_199_254_740_991);
            let base = clone_state(&state);
            let mut steps = Vec::new();
            let mut records = Vec::new();
            let r = run_step(&content, &dict, state, "0:0 @ ANA INCUBATE EGG", 1);
            steps.push(format!("ok {}", r.records.len()));
            records.extend(r.records.iter().cloned());
            let adv1 = run_clock(&content, &dict, r.state, "0:1", r.records.len() as i64 + 1);
            steps.push(format!("ok {}", adv1.records.len()));
            records.extend(adv1.records.iter().cloned());
            let adv2 = run_clock(&content, &dict, adv1.state, "0:2", adv1.records.len() as i64 + 1 + 10);
            steps.push(format!("ok {}", adv2.records.len()));
            records.extend(adv2.records.iter().cloned());
            build(dict, content, base, records, steps)
        }
        "anchor-refuse-id-recycle" => {
            let (dict, content) = rig(
                vec![
                    verb_of("SEEDV", vec![a_slot()], 30, &["NARRATE \"planted.\""]),
                    verb_of("GATE", vec![a_slot()], 30, &["REFUSE(\"not without a lantern.\")"]),
                ],
                |_| {},
            );
            let base = ordering_seed();
            let mut steps = Vec::new();
            let mut records = Vec::new();
            let r1 = run_step(&content, &dict, clone_state(&base), "0:0 @ ANA SEEDV", 1);
            steps.push(format!("ok {}", r1.records.len()));
            records.extend(r1.records.iter().cloned());
            let r2 = run_step(&content, &dict, clone_state(&r1.state), "0:1 @ ANA GATE", 2);
            steps.push(format!("ok {}", r2.records.len()));
            records.extend(r2.records.iter().cloned());
            let r3 = run_step(&content, &dict, clone_state(&r1.state), "0:1 @ ANA GATE", 2);
            steps.push(format!("ok {}", r3.records.len()));
            records.extend(r3.records.iter().cloned());
            let adv = run_clock(&content, &dict, r1.state, "0:1", 2);
            steps.push(format!("ok {}", adv.records.len()));
            records.extend(adv.records.iter().cloned());
            build(dict, content, base, records, steps)
        }
        "anchor-wait-partition" => {
            let (dict, content) = rig(vec![verb_of("PACE", vec![a_slot()], 30, &["NARRATE \"d.\""])], |_| {});
            let base = ordering_seed();
            let mut steps = Vec::new();
            let mut records = Vec::new();
            let r = run_step(&content, &dict, clone_state(&base), "0:0 @ ANA PACE", 1);
            steps.push(format!("ok {}", r.records.len()));
            records.extend(r.records.iter().cloned());
            let adv = run_clock(&content, &dict, r.state, "0:1", 2);
            steps.push(format!("ok {}", adv.records.len()));
            records.extend(adv.records.iter().cloned());
            build(dict, content, base, records, steps)
        }
        "anchor-window-completion-failure" => {
            let (dict, content) = rig(vec![verb_of("BAD", vec![a_slot()], 30, &["SPAWN NOSUCH_PROTOTYPE"])], |_| {});
            let base = ordering_seed();
            let mut steps = Vec::new();
            let mut records = Vec::new();
            let r = run_step(&content, &dict, clone_state(&base), "0:0 @ ANA BAD", 1);
            steps.push(format!("ok {}", r.records.len()));
            records.extend(r.records.iter().cloned());
            let adv = run_clock(&content, &dict, r.state, "0:1", 2);
            steps.push(format!("ok {}", adv.records.len()));
            records.extend(adv.records.iter().cloned());
            let adv2 = run_clock(&content, &dict, adv.state, "0:2", 10);
            steps.push(format!("ok {}", adv2.records.len()));
            records.extend(adv2.records.iter().cloned());
            build(dict, content, base, records, steps)
        }
        "anchor-first-ending" => {
            let (dict, content) = rig(
                vec![verb_of("FINISH", vec![a_slot()], 30, &["ENDING QUIET_DUSK", "ENDING OTHER_END"])],
                |raw| {
                    raw.endings = Some(
                        [
                            (
                                "QUIET_DUSK".to_string(),
                                EndingDef {
                                    title: "A quiet dusk.".to_string(),
                                    flavour: Some("calm".to_string()),
                                },
                            ),
                            (
                                "OTHER_END".to_string(),
                                EndingDef {
                                    title: "Another end.".to_string(),
                                    flavour: None,
                                },
                            ),
                        ]
                        .into_iter()
                        .collect(),
                    );
                },
            );
            let base = ordering_seed();
            let mut steps = Vec::new();
            let mut records = Vec::new();
            let r = run_step(&content, &dict, clone_state(&base), "0:0 @ ANA FINISH", 1);
            steps.push(format!("ok {}", r.records.len()));
            records.extend(r.records.iter().cloned());
            let adv = run_clock(&content, &dict, r.state, "0:1", 2);
            steps.push(format!("ok {}", adv.records.len()));
            records.extend(adv.records.iter().cloned());
            let gated = run_step(&content, &dict, adv.state, "0:2 @ ANA FINISH", 10);
            assert!(
                gated.rejected.as_deref() == Some("the story has ended — A quiet dusk."),
                "anchor-ending: the story-over gate"
            );
            steps.push(format!("rejected {}", gated.rejected.unwrap()));
            build(dict, content, base, records, steps)
        }
        "anchor-roll-last-wins" => {
            let (dict, content) = rig(
                vec![verb_of("CAST", vec![a_slot()], 0, &["ROLL(1, 6)", "ROLL(1, 6)", "NARRATE \"r={roll}.\""])],
                |_| {},
            );
            let base = ordering_seed();
            let mut steps = Vec::new();
            let mut records = Vec::new();
            let r = run_step(&content, &dict, clone_state(&base), "0:0 @ ANA CAST", 1);
            steps.push(format!("ok {}", r.records.len()));
            records.extend(r.records.iter().cloned());
            let adv = run_clock(&content, &dict, r.state, "0:0", 2);
            steps.push(format!("ok {}", adv.records.len()));
            records.extend(adv.records.iter().cloned());
            build(dict, content, base, records, steps)
        }
        "anchor-roll-per-clause" => {
            let cast = VerbContent {
                slots: vec![a_slot()],
                duration: 0,
                recipes: vec![recipe(&["ROLL(1, 6)", "IF roll >= 0 -> (NARRATE \"bound.\")"])],
            };
            let bare = VerbContent {
                slots: vec![a_slot()],
                duration: 0,
                recipes: vec![recipe(&["ROLL(1, 6)", "IF roll >= 0 -> (NARRATE \"bare {roll}.\")"])],
            };
            let (dict, content) = rig(
                vec![("CAST".to_string(), cast), ("BARE".to_string(), bare)],
                |_| {},
            );
            let base = ordering_seed();
            let mut steps = Vec::new();
            let mut records = Vec::new();
            let s = parse_sentence("0:0 @ ANA CAST; ANA BARE", &dict).unwrap();
            let r = execute_step(
                &content,
                &dict,
                clone_state(&base),
                Step {
                    start_min: None,
                    clock: Some(Clock { day: 0, halfhour: 0 }),
                    clauses: s.clauses,
                    t_real: None,
                },
                ExecOptions { first_record_id: 1, id_gen: None },
            )
            .unwrap();
            assert!(r.rejected.is_none());
            steps.push(format!("ok {}", r.records.len()));
            records.extend(r.records.iter().cloned());
            let adv = run_clock(&content, &dict, r.state, "0:0", 2);
            steps.push(format!("ok {}", adv.records.len()));
            records.extend(adv.records.iter().cloned());
            build(dict, content, base, records, steps)
        }
        "anchor-advance-extension" => {
            let (dict, content) = rig(
                vec![verb_of("HATCH", vec![a_slot()], 30, &["ADVANCE(1h)", "NARRATE \"warm.\""])],
                |_| {},
            );
            let base = ordering_seed();
            let mut steps = Vec::new();
            let mut records = Vec::new();
            let r = run_step(&content, &dict, clone_state(&base), "0:0 @ ANA HATCH", 1);
            steps.push(format!("ok {}", r.records.len()));
            records.extend(r.records.iter().cloned());
            let adv = run_clock(&content, &dict, r.state, "0:1", 2);
            steps.push(format!("ok {}", adv.records.len()));
            records.extend(adv.records.iter().cloned());
            build(dict, content, base, records, steps)
        }
        "anchor-flash-zero-duration" => {
            let (dict, content) = rig(vec![verb_of("FLASH", vec![a_slot()], 0, &["NARRATE \"zap.\""])], |_| {});
            let base = ordering_seed();
            let mut steps = Vec::new();
            let mut records = Vec::new();
            let r = run_step(&content, &dict, clone_state(&base), "0:0 @ ANA FLASH", 1);
            steps.push(format!("ok {}", r.records.len()));
            records.extend(r.records.iter().cloned());
            let adv = run_clock(&content, &dict, r.state, "0:0", 2);
            steps.push(format!("ok {}", adv.records.len()));
            records.extend(adv.records.iter().cloned());
            build(dict, content, base, records, steps)
        }
        "anchor-station-locks" => {
            let seize = VerbContent {
                slots: vec![a_slot(), o_slot(None)],
                duration: 60,
                recipes: vec![recipe(&["NARRATE \"seized.\""])],
            };
            let touch = VerbContent {
                slots: vec![a_slot(), o_slot(None)],
                duration: 30,
                recipes: vec![recipe(&["NARRATE \"touched.\""])],
            };
            let (dict, content) = rig(
                vec![("SEIZE".to_string(), seize), ("TOUCH".to_string(), touch)],
                |_| {},
            );
            let base = ordering_seed();
            let mut steps = Vec::new();
            let mut records = Vec::new();
            let r1 = run_step(&content, &dict, clone_state(&base), "0:0 @ ANA SEIZE MAGICK_BOOK", 1);
            steps.push(format!("ok {}", r1.records.len()));
            records.extend(r1.records.iter().cloned());
            let r2 = run_step(&content, &dict, clone_state(&r1.state), "0:0 @ BERTHA SEIZE", 3);
            steps.push(format!("rejected {}", r2.rejected.clone().unwrap()));
            let r3 = run_step(&content, &dict, clone_state(&r1.state), "0:0 @ ANA TOUCH MAGICK_BOOK", 3);
            steps.push(format!("rejected {}", r3.rejected.clone().unwrap()));
            let r4 = run_step(&content, &dict, clone_state(&r1.state), "0:0 @ BERTHA TOUCH", 3);
            steps.push(format!("ok {}", r4.records.len()));
            // r5 continues from r4's state so the golden log is ONE continuous
            // play — a log assembled from two diverged branches is not
            // fold-able to its snap (law 1). The en-route lock release is
            // unchanged: the advance resolves 3:0 and 1:0, freeing MAGICK
            // BOOK, then TOUCH begins.
            let r5 = run_step(&content, &dict, r4.state, "1:0 @ ANA TOUCH MAGICK_BOOK", 3);
            steps.push(format!("ok {}", r5.records.len()));
            records.extend(r4.records.iter().cloned());
            records.extend(r5.records.iter().cloned());
            build(dict, content, base, records, steps)
        }
        "anchor-overlapping-windows" => {
            let (dict, content) = rig(
                vec![
                    verb_of("PACE_A", vec![a_slot()], 60, &["NARRATE \"a.\""]),
                    verb_of("PACE_B", vec![a_slot()], 30, &["NARRATE \"b.\""]),
                ],
                |_| {},
            );
            let base = ordering_seed();
            let mut steps = Vec::new();
            let mut records = Vec::new();
            let r1 = run_step(&content, &dict, clone_state(&base), "0:0 @ ANA PACE_A", 1);
            steps.push(format!("ok {}", r1.records.len()));
            records.extend(r1.records.iter().cloned());
            let r2 = run_step(&content, &dict, r1.state, "0:0 @ ANA PACE_B", r1.records.len() as i64 + 1);
            steps.push(format!("ok {}", r2.records.len()));
            records.extend(r2.records.iter().cloned());
            build(dict, content, base, records, steps)
        }
        "anchor-expiry-locked-drop" => {
            let (dict, content) = rig(
                vec![verb_of("SEIZE", vec![a_slot(), o_slot(None)], 60, &["SET(OBJECT, aspect:touched, 1), DESTROY OBJECT"])],
                |_| {},
            );
            let mut state = ordering_seed();
            state
                .entities
                .get_mut("book")
                .unwrap()
                .attrs
                .insert("expiresAt".to_string(), chorus::core::types::AttrValue::Num(30));
            let base = clone_state(&state);
            let mut steps = Vec::new();
            let mut records = Vec::new();
            let r1 = run_step(&content, &dict, state, "0:0 @ ANA SEIZE MAGICK_BOOK", 1);
            steps.push(format!("ok {}", r1.records.len()));
            records.extend(r1.records.iter().cloned());
            let adv = run_clock(&content, &dict, r1.state, "1:0", 2);
            steps.push(format!("ok {}", adv.records.len()));
            records.extend(adv.records.iter().cloned());
            build(dict, content, base, records, steps)
        }
        "anchor-slot-cadence-once" => {
            let (dict, content) = rig(vec![], |raw| {
                raw.triggers = Some(vec![
                    TriggerDef::Slot(SlotTriggerDef { id: "mark".into(), slot: 1, once: None, out: vec!["NARRATE \"mark.\"".into()] }),
                    TriggerDef::Slot(SlotTriggerDef { id: "mark".into(), slot: 2, once: None, out: vec!["NARRATE \"mark again.\"".into()] }),
                    TriggerDef::Slot(SlotTriggerDef { id: "once_dusk".into(), slot: 1, once: Some(true), out: vec!["NARRATE \"dusk.\"".into()] }),
                ]);
            });
            let base = ordering_seed();
            let mut steps = Vec::new();
            let mut records = Vec::new();
            let adv = run_clock(&content, &dict, clone_state(&base), "0:2", 1);
            steps.push(format!("ok {}", adv.records.len()));
            records.extend(adv.records.iter().cloned());
            let adv2 = run_clock(&content, &dict, adv.state, "1:4", 10);
            steps.push(format!("ok {}", adv2.records.len()));
            records.extend(adv2.records.iter().cloned());
            build(dict, content, base, records, steps)
        }
        other => panic!("no anchor driver for '{other}'"),
    }
}

// ---------------------------------------------------------------------------
// demo (ts: examples/demo.ts, verbatim)
// ---------------------------------------------------------------------------

fn demo_driver() -> Result<Played> {
    let dict = core_dictionary();
    let content = compile_content(&seed_content(), &dict, &[])?;
    let steps = [
        "0:28 @ ANA AT HOUSE READ THE_FORGETTING", // 14:00
        "0:30 @ ANA, BERTHA READ FAIRY_TALES",     // 15:00 — ends 16:00 → dusk_falls fires
        "0:32 @ ANA, BERTHA AT DINING_HALL TALK ABOUT CLUE_SECRET_HIDEOUT",
        "0:34 @ BERTHA READ MAGICK_BOOK", // refused — nobody who knows is present
        "0:34 @ ANA TRAVEL_TO THE_STUDY; ANA AT THE_STUDY READ MAGICK_BOOK",
        "0:40 @ ANA WAIT",
    ];
    let mut records: Vec<ActionRecord> = Vec::new();
    let mut state = seed_state();
    let mut id: i64 = 1;
    let mut n = 0usize;
    for src in steps {
        let s = parse_sentence(src, &dict)?;
        // ts: `idGen: () => `x${id}_${++n}`` — id is read at CALL time and
        // does not change within one step; n is the closure's OWN counter,
        // shared across every step (the TS closure is one object)
        let id_now = id;
        let mut id_gen = || {
            n += 1;
            format!("x{id_now}_{n}")
        };
        let r = execute_step(
            &content,
            &dict,
            state,
            Step {
                start_min: None,
                clock: Some(s.clock.expect("demo sentences carry clocks")),
                clauses: s.clauses,
                t_real: None,
            },
            ExecOptions {
                first_record_id: id,
                id_gen: Some(&mut id_gen),
            },
        )?;
        if let Some(rejected) = r.rejected {
            panic!("demo step rejected: {rejected}");
        }
        state = r.state;
        id += r.records.len() as i64;
        records.extend(r.records);
    }
    let mut prose_out = String::new();
    for rec in &records {
        prose_out.push_str("· ");
        prose_out.push_str(&chorus::session::render::prose(rec, &content, &dict)?);
        prose_out.push('\n');
    }
    let journal_text = with_nl(&chorus::session::render::journal(&records));
    Ok(Played {
        base: seed_state(),
        dict,
        content,
        records,
        journal: Some(journal_text),
        prose: Some(prose_out),
        steps: None,
        suggestions: None,
    })
}

// ---------------------------------------------------------------------------
// reach-cli family (§4-W5, CLI-02 must) — re-render the CLI's stdout from the
// driver and compare byte for byte. Drivers (from tools/golden-dump.ts):
//   { type: 'reach-cli', mode: 'all'|'card'|'probes', script?|seed?, card?,
//     demand?, usable? } and { type: 'reach-scale-census', protos, recipes }.
// ---------------------------------------------------------------------------

/// ts: tools/reach-cli.ts:69-76 — the `--all` name set: prototypes ∪ LIVE
/// initial entities ∪ deck.order (NOT defaultcard — CLI-02 correction),
/// UTF-16 code-unit sorted (REACH-05).
fn reach_cli_all_names(
    content: &CompiledContent,
    state: &State,
) -> Vec<String> {
    let mut names: indexmap::IndexSet<String> = indexmap::IndexSet::new();
    names.extend(content.prototypes.keys().cloned());
    for e in state.entities.values() {
        if e.dead_action.is_none() {
            names.insert(e.name.clone());
        }
    }
    for d in content.decks.values() {
        for c in &d.order {
            names.insert(c.name.clone());
        }
    }
    let mut sorted: Vec<String> = names.into_iter().collect();
    sorted.sort_by(|a, b| chorus::js::cmp_utf16(a, b));
    sorted
}

fn reach_cli_stdout(driver: &serde_json::Value, name: &str) -> Result<String> {
    let dtype = driver["type"].as_str().unwrap_or("");
    if dtype == "reach-scale-census" {
        // the PR1-03 A/B reference: the CI-sized 400×400 world's NameStat
        // NON-timing fields, one TSV row per name (ms is timing — it does
        // not ship)
        let protos = driver["protos"].as_u64().expect("census protos") as usize;
        let recipes = driver["recipes"].as_u64().expect("census recipes") as usize;
        let world = common::reach_scale_lib::build_cs_scale(protos, recipes);
        let stats = common::reach_scale_lib::census(&world, &world.names);
        let lines: Vec<String> = stats
            .iter()
            .map(|st| {
                [
                    st.name.clone(),
                    st.verdict.clone(),
                    st.existence.clone(),
                    st.roots.to_string(),
                    st.nodes.to_string(),
                    st.depth.to_string(),
                ]
                .join("\t")
            })
            .collect();
        return Ok(lines.join("\n") + "\n");
    }
    let mode = driver["mode"].as_str().expect("reach-cli mode");
    let usable = driver["usable"].as_bool().unwrap_or(false);
    let demand = driver["demand"].as_str().map(str::to_string);
    let mut out = String::new();
    match mode {
        // the whole `--all` sweep over the bundled script (the CLI compiles
        // it fresh per run, ts: tools/reach-cli.ts:35-36); the report closure
        // captures the demand, so --all --demand renders the demand report
        // per name (the frozen quirk — the dump's all-demand snapshot)
        "all" => {
            let dict = core_dictionary();
            let compiled = chorus::content::script::compile_script(SCRIPT_TEXT, &dict)?;
            let analyzer = chorus::reach::Analyzer::new(&compiled.content, &compiled.state);
            for card in reach_cli_all_names(&compiled.content, &compiled.state) {
                let rep = analyzer.analyze_card(
                    &card,
                    chorus::reach::CardOptions { usable, demand: demand.clone() },
                )?;
                chorus::reach::render_report(
                    &mut out,
                    driver["script"].as_str().expect("script label"),
                    &card,
                    demand.as_deref(),
                    usable,
                    &rep,
                );
            }
        }
        // one report (a named card, or a lone --demand where the target is
        // ignored — the TS card argv is absent then; '' is the stand-in)
        "card" => {
            let dict = core_dictionary();
            let compiled = chorus::content::script::compile_script(SCRIPT_TEXT, &dict)?;
            let card = driver["card"].as_str().expect("card");
            let rep = chorus::reach::analyze_card(
                &compiled.content,
                &compiled.state,
                card,
                chorus::reach::CardOptions {
                    usable,
                    demand: demand.clone(),
                },
            )?;
            chorus::reach::render_report(
                &mut out,
                driver["script"].as_str().expect("script label"),
                card,
                demand.as_deref(),
                usable,
                &rep,
            );
        }
        // a genScenario world: every probe through the CLI shapes
        // (file label 'scenario', matching the dump; a demand probe's header
        // carries ITS query — ts: reportInto(…, '', usable, p.q))
        "probes" => {
            let seed = driver["seed"].as_u64().expect("probe seed") as u32;
            let sc = reach_fuzz_lib::gen_scenario(seed);
            let dict = core_dictionary();
            let content = compile_content(&sc.raw, &dict, &sc.entity_kinds)?;
            // one Analyzer across the probes — the TS dump-side analyzeCard
            // calls share the module-level worldCache for this (content,
            // initial) pair
            let analyzer = chorus::reach::Analyzer::new(&content, &sc.initial);
            for p in &sc.probes {
                let (target, opts, probe_demand) = match p.mode {
                    reach_fuzz_lib::ProbeMode::Name => (
                        p.q.as_str(),
                        chorus::reach::CardOptions { usable, demand: None },
                        None,
                    ),
                    reach_fuzz_lib::ProbeMode::Demand => (
                        "",
                        chorus::reach::CardOptions { usable, demand: Some(p.q.clone()) },
                        Some(p.q.as_str()),
                    ),
                };
                let rep = analyzer.analyze_card(target, opts)?;
                chorus::reach::render_report(&mut out, "scenario", target, probe_demand, usable, &rep);
            }
        }
        other => panic!("golden {name}: unknown reach-cli mode {other}"),
    }
    Ok(out)
}

// ---------------------------------------------------------------------------
// the harness — one test per family, every manifest golden of the kind
// ---------------------------------------------------------------------------

fn check_family(kind: &str) {
    let m = manifest();
    let mut checked = 0usize;
    for g in &m.goldens {
        if g.kind != kind {
            continue;
        }
        checked += 1;
        let played = driver_for(&g.name).unwrap_or_else(|e| panic!("driver {} failed: {e}", g.name));
        // record-count bookkeeping (cheap consistency of the driver mirror)
        assert_eq!(
            played.records.len(),
            g.records,
            "golden {}: record count diverges (driver {} vs manifest {})",
            g.name,
            played.records.len(),
            g.records
        );
        // 2: same driver replayed from scratch → byte-identical JSONL
        let golden_bytes = read_golden(g.jsonl.as_deref().expect("record goldens carry a jsonl"));
        assert_bytes_eq(&golden_bytes, &to_jsonl(&played.records));
        // 1: fold(load_log(golden)) over the base lands the manifest snap
        let loaded = chorus::engine::store::load_log(&goldens_dir().join(g.jsonl.as_ref().expect("record goldens carry a jsonl")))
            .unwrap_or_else(|e| panic!("golden {}: load_log failed: {e}", g.name));
        let folded = fold(&loaded, &played.base, &played.dict, &played.content)
            .unwrap_or_else(|e| panic!("golden {}: fold failed: {e}", g.name));
        assert_eq!(snap(&folded), g.snap, "golden {}: fold snap diverges", g.name);
        // 3: sidecars
        if let Some(steps) = &g.steps {
            assert_bytes_eq(&read_golden(steps), played.steps.as_deref().unwrap_or_default());
        }
        if let Some(journal) = &g.journal {
            assert_bytes_eq(&read_golden(journal), played.journal.as_deref().unwrap_or_default());
        }
        if let Some(prose) = &g.prose {
            assert_bytes_eq(&read_golden(prose), played.prose.as_deref().unwrap_or_default());
        }
        if let Some(sugg) = &g.suggestions {
            assert_bytes_eq(&read_golden(sugg), played.suggestions.as_deref().unwrap_or_default());
        }
    }
    assert!(checked > 0, "no goldens of kind '{kind}' in the manifest");
}

#[test]
fn nightjar_goldens_replay_byte_identical() {
    check_family("script");
}

#[test]
fn walk_goldens_replay_byte_identical() {
    check_family("walk");
}

#[test]
fn adversarial_goldens_replay_byte_identical() {
    check_family("scenario");
}

#[test]
fn anchor_goldens_replay_byte_identical() {
    check_family("anchor");
}

#[test]
fn fuzz_goldens_replay_byte_identical() {
    check_family("fuzz");
}

#[test]
fn demo_goldens_replay_byte_identical() {
    check_family("demo");
}

/// §4-W5 CLI-02 must — the reach-cli stdout freeze: every snapshot is
/// re-rendered from its driver and compared byte for byte.
#[test]
fn reach_cli_goldens_replay_byte_identical() {
    let m = manifest();
    let mut checked = 0usize;
    for g in &m.goldens {
        if g.kind != "reach-cli" {
            continue;
        }
        checked += 1;
        let golden = read_golden(g.stdout.as_ref().unwrap_or_else(|| panic!("golden {}: reach-cli goldens carry stdout", g.name)));
        let played = reach_cli_stdout(&g.driver, &g.name)
            .unwrap_or_else(|e| panic!("reach-cli driver {} failed: {e}", g.name));
        assert_bytes_eq(&golden, &played);
    }
    assert!(checked > 0, "no reach-cli goldens in the manifest");
}

/// every manifest golden has a driver — no orphan entries (record goldens
/// replay through driver_for; the reach-cli goldens' drivers are exercised by
/// reach_cli_goldens_replay_byte_identical, so here they only get the shape
/// check — re-rendering the census golden twice would double its minutes)
#[test]
fn every_golden_has_a_driver() {
    for g in manifest().goldens {
        if g.stdout.is_some() {
            let t = g.driver["type"].as_str().unwrap_or("");
            assert!(
                t == "reach-cli" || t == "reach-scale-census",
                "golden {}: unknown stdout driver type {t}",
                g.name
            );
            continue;
        }
        assert!(
            driver_for(&g.name).is_ok(),
            "golden {} has no driver",
            g.name
        );
    }
}

/// the §6.2 harness probe: first-divergent-line reporting stays sharp
#[test]
fn assert_bytes_eq_reports_the_first_divergent_line() {
    let a = "line1\nline2\nline3\n";
    let b = "line1\nlineX\nline3\n";
    let err = std::panic::catch_unwind(|| assert_bytes_eq(a, b))
        .expect_err("bytes differ")
        .downcast_ref::<String>()
        .cloned()
        .unwrap_or_default();
    assert!(err.contains("line 2"), "{err}");
    assert!(err.contains("lineX"), "{err}");
}
