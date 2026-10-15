//! Cultist-Simulator-scale load generator for the reachability analyzer —
//! ts: test/reach-scale-lib.ts (plan §4-W5 / §5.2.1 / PR3-03). The companion
//! of `reach_fuzz_lib`: that one checks SOUNDNESS against engine truth, this
//! one checks the analyzer's SCALING — compile + per-card census + path
//! export over CS-shaped content. NO test harness here (explorer-lib
//! precedent — scripts and tests import alike; the reach-scale bin mounts
//! this file via `#[path]` for its `--worker` A/B interface).
//!
//! The content is SYNTHETIC, not a conversion (docs/reverse/cultist-simulator.md
//! §6): N prototypes ≈ CS elements, M recipes ≈ CS's 2,084-recipe rulebook
//! modeled the Chorus way (ONE VERB PER SITUATION — same-verb multi-recipe is
//! first-match and self-shadow), 67 decks, season/expiry triggers.
//! Deterministic in the arguments: same (nProtos, nRecipes) ⇒ byte-identical
//! content, so two builds produce independent, comparable worlds.
//!
//! (Shared test lib discipline: the `allow` lives in this one place.)
#![allow(dead_code)]

use std::time::Instant;

use indexmap::{IndexMap, IndexSet};
use serde::Serialize;

use chorus::content::content::{
    compile_content, CompiledContent, ContentRaw, DeckRaw, ExpireTriggerDef, PrototypeDef,
    RecipeDef, SlotDef, SlotTriggerDef, TriggerDef, VerbContent,
};
use chorus::core::dict::{core_dictionary, Dictionary};
use chorus::core::types::{add_entity, new_state, AspectVal, Entity, State};
use chorus::js::Lcg;
use chorus::reach::{Analyzer, CardReport, ReachPath};

/// ts: reach-scale-lib.ts:18-25 — the world plus every card name worth
/// probing and the shape counters the CLI prints.
pub struct ScaleWorld {
    pub content: CompiledContent,
    pub dict: Dictionary,
    pub initial: State,
    /// every card name worth probing: initial entities + prototypes + deck
    /// cards
    pub names: Vec<String>,
    pub stats: ScaleStats,
}

/// ts: reach-scale-lib.ts:24 — `{ protos, recipes, ops, decks, triggers }`.
pub struct ScaleStats {
    pub protos: usize,
    pub recipes: usize,
    pub ops: usize,
    pub decks: usize,
    pub triggers: usize,
}

// ---------- deterministic RNG (explorer-lib's LCG discipline) ----------

/// ts: reach-scale-lib.ts:28-31 — `lcg(0xc5)`; `Lcg::new` XORs 0x9e3779b9
/// exactly like the TS `seed ^ 0x9e3779b9` (plan §3.12).
#[must_use]
fn lcg(seed: u32) -> Lcg {
    Lcg::new(seed)
}

/// ts: reach-scale-lib.ts:35 — `Math.floor(rnd() * n)`
fn int(r: &mut Lcg, n: usize) -> usize {
    r.next_int(n)
}

/// ts: reach-scale-lib.ts:36 — `pick(arr)`. The INDEX DRAW happens even when
/// the array is EMPTY (`int(0)` = `floor(rnd()*0)` still consumes one LCG
/// draw — QUIRK(REACH-14): the CI-sized 400-prototype slice has no hubs
/// (`protoNames.slice(900, 905)` is empty), so the picked value is
/// `undefined` and the surrounding template literal becomes the literal
/// string "SPAWN undefined"; the draw is what keeps the stream aligned.
fn pick_template(r: &mut Lcg, arr: &[String]) -> String {
    let i = int(r, arr.len());
    arr.get(i)
        .cloned()
        .unwrap_or_else(|| "undefined".to_string())
}

fn str_vec(vs: &[&str]) -> Vec<String> {
    vs.iter().map(|s| (*s).to_string()).collect()
}

/// ts: reach-scale-lib.ts:33-143 — `buildCsScale`, draw-for-draw identical.
pub fn build_cs_scale(n_protos: usize, n_recipes: usize) -> ScaleWorld {
    let mut rnd = lcg(0xc5);

    /// ts: reach-scale-lib.ts:38
    const KINDS: [&str; 8] = [
        "item", "text", "idea", "resource", "rite", "state", "place", "character",
    ];
    /// ts: reach-scale-lib.ts:39
    const ASPECTS: [&str; 12] = [
        "lantern", "forge", "edge", "winter", "knock", "heart", "grail", "moth", "lore",
        "ingredient", "tool", "follower",
    ];
    // ts: reach-scale-lib.ts:40 — `R1`..`R20`
    let resources: Vec<String> = (1..=20).map(|i| format!("R{i}")).collect();
    let kinds_non_char: Vec<&str> = KINDS.iter().copied().filter(|k| *k != "character").collect();

    // ts: reach-scale-lib.ts:42-51 — the prototypes. `kind` for i < 30 is
    // 'character' with NO draw (the ternary's false arm never evaluates).
    let mut protos: IndexMap<String, PrototypeDef> = IndexMap::new();
    let mut proto_names: Vec<String> = Vec::with_capacity(n_protos);
    for i in 0..n_protos {
        let id = format!("E{i:04}");
        proto_names.push(id.clone());
        let kind = if i < 30 {
            "character".to_string()
        } else {
            // ts: `pick(KINDS.filter(k => k !== 'character'))`
            kinds_non_char[int(&mut rnd, kinds_non_char.len())].to_string()
        };
        // ts: `for (let a = 0, n = 1 + int(4); a < n; a++) aspects[pick(ASPECTS)] = 1 + int(15)`
        // — n drawn once; a repeated key draw overwrites in place (object
        // assignment keeps the first-insertion position — IndexMap alike).
        let mut aspects: IndexMap<String, AspectVal> = IndexMap::new();
        let n_aspects = 1 + int(&mut rnd, 4);
        for _ in 0..n_aspects {
            let key = ASPECTS[int(&mut rnd, ASPECTS.len())].to_string();
            let val = 1 + int(&mut rnd, 15) as i64;
            aspects.insert(key, AspectVal::Num(val));
        }
        // ts: `...(rnd() < 0.24 ? {} : { stackable: true })` — CS: 373/1566
        // unique; the ROLL always happens (ternary condition)
        let stackable = rnd.next_f64() >= 0.24;
        protos.insert(
            id,
            PrototypeDef {
                kind,
                stackable,
                aspects: Some(aspects),
                attrs: None,
                derive: None,
            },
        );
    }

    // CS's "big verb + requirements selection" is modeled one verb per
    // situation: same-verb multi-recipe is first-match in Chorus, so recipe
    // #2+ self-shadows (ts: reach-scale-lib.ts:53-58)
    /// ts: reach-scale-lib.ts:55-58 — `[label, query]` pairs; the label is
    /// discarded by the destructuring pick.
    const VERB_SLOTS: [&str; 6] = [
        "aspect:lore>=1",
        "aspect:heart>=1",
        "aspect:lantern>=1",
        "aspect:moth>=1",
        "aspect:winter>=1",
        "aspect:knock>=1",
    ];
    // ts: reach-scale-lib.ts:59 — `protoNames.slice(900, 905)`; EMPTY for the
    // CI-sized 400-prototype slice (then `pick` templates "undefined").
    let hubs: Vec<String> = proto_names.iter().skip(900).take(5).cloned().collect();
    let n_decks = 67usize;

    // The scenario's mutable build context — the TS closure (`op`) captures
    // `rnd` and the local tables; Rust threads them explicitly.
    struct Ctx<'a> {
        rnd: &'a mut Lcg,
        proto_names: &'a [String],
        resources: &'a [String],
        hubs: &'a [String],
        n_decks: usize,
    }
    impl Ctx<'_> {
        /// ts: reach-scale-lib.ts:61-69 — the outcome-op generator; every
        /// branch guard's draw happens EXACTLY where the TS evaluates it.
        fn op(&mut self, allow_refuse: bool) -> String {
            let roll = self.rnd.next_f64();
            if roll < 0.6 {
                return format!("SPAWN {}", pick_template(self.rnd, self.proto_names));
            }
            if roll < 0.7 {
                return format!("DESTROY {}", pick_template(self.rnd, self.resources));
            }
            if roll < 0.8 {
                return format!("DRAW D{}", 1 + int(self.rnd, self.n_decks));
            }
            if roll < 0.9 {
                let r = pick_template(self.rnd, self.resources);
                let a = ASPECTS[int(self.rnd, ASPECTS.len())];
                let v = 1 + int(self.rnd, 10) as i64;
                return format!("SET({r}, aspect:{a}, {v})");
            }
            // ts: `roll < 0.95 && allowRefuse` — no extra draw either way
            if roll < 0.95 && allow_refuse {
                return "REFUSE(\"no\")".to_string();
            }
            "NARRATE \"beat\"".to_string()
        }
    }

    // ts: reach-scale-lib.ts:70-72 — WAIT comes FIRST in the verbs object
    // (content.verbs insertion order is observable; its recipe carries the
    // authored id 'time.needs').
    let mut verbs_raw: IndexMap<String, VerbContent> = IndexMap::new();
    verbs_raw.insert(
        "WAIT".to_string(),
        VerbContent {
            slots: vec![SlotDef {
                role: "actors".to_string(),
                min: 1,
                max: Some(1),
                query: Some("kind:character".to_string()),
                shown: None,
            }],
            duration: 30,
            recipes: vec![RecipeDef {
                id: Some("time.needs".to_string()),
                when: None,
                shown: None,
                hint: None,
                out: vec![],
            }],
        },
    );

    let mut ctx = Ctx {
        rnd: &mut rnd,
        proto_names: &proto_names,
        resources: &resources,
        hubs: &hubs,
        n_decks,
    };
    let mut ops = 0usize;
    for i in 0..n_recipes {
        // ts: `const [, query] = pick(VERB_SLOTS)` — the label is dropped
        let query = VERB_SLOTS[int(ctx.rnd, VERB_SLOTS.len())];
        let name = format!("V{i}");
        let mut out: Vec<String>;
        let mut when: Option<String> = None;
        if i % 4 == 0 {
            // clean chain: deterministic CS-style producers (hubs minted
            // widely) (ts: reach-scale-lib.ts:79-82)
            let a = format!("SPAWN {}", pick_template(ctx.rnd, ctx.hubs));
            let b = format!("SPAWN {}", pick_template(ctx.rnd, ctx.proto_names));
            out = if ctx.rnd.next_f64() < 0.5 { vec![a, b] } else { vec![b, a] };
        } else {
            // ts: reach-scale-lib.ts:83-88 — the ROLL/IF-exists/when pushes;
            // each guard draw happens before its template's draws
            let n_out = 1 + int(ctx.rnd, 4);
            out = Vec::with_capacity(n_out + 2);
            for _ in 0..n_out {
                out.push(ctx.op(true));
            }
            if ctx.rnd.next_f64() < 0.12 {
                let bound = 2 + int(ctx.rnd, 4) as i64;
                let p = pick_template(ctx.rnd, ctx.proto_names);
                out.push(format!(
                    "ROLL(1, 6), IF roll >= {bound} -> (SPAWN {p}) ELSE -> (NARRATE \"low\")"
                ));
            }
            if ctx.rnd.next_f64() < 0.1 {
                let g = pick_template(ctx.rnd, ctx.proto_names);
                let p = pick_template(ctx.rnd, ctx.proto_names);
                out.push(format!(
                    "IF exists({g}) -> (SPAWN {p}) ELSE -> (NARRATE \"nested\")"
                ));
            }
            // ts: reach-scale-lib.ts:86-88 — the three `when` templates are
            // built EAGERLY (all their draws happen), then the pick draws
            if ctx.rnd.next_f64() < 0.25 {
                let t1 = format!("count(name:{}) >= 1", pick_template(ctx.rnd, ctx.proto_names));
                let t2 = format!("exists({})", pick_template(ctx.rnd, ctx.proto_names));
                let a = ASPECTS[int(ctx.rnd, ASPECTS.len())];
                let t3 = format!("actors.aspect.{a} >= {}", 1 + int(ctx.rnd, 4) as i64);
                let choices: [&str; 3] = [&t1, &t2, &t3];
                when = Some(choices[int(ctx.rnd, choices.len())].to_string());
            }
        }
        ops += out.len();
        // ts: reach-scale-lib.ts:91-99 — slots then duration then the recipe;
        // the optional `with` slot's ROLL always happens (spread ternary)
        let mut slots = vec![
            SlotDef {
                role: "actors".to_string(),
                min: 1,
                max: Some(1),
                query: Some("kind:character".to_string()),
                shown: None,
            },
            SlotDef {
                role: "object".to_string(),
                min: 0,
                max: Some(1),
                query: Some(query.to_string()),
                shown: None,
            },
        ];
        if ctx.rnd.next_f64() < 0.5 {
            let a = ASPECTS[int(ctx.rnd, ASPECTS.len())];
            slots.push(SlotDef {
                role: "with".to_string(),
                min: 0,
                max: Some(1),
                query: Some(format!("aspect:{a}>=2")),
                shown: None,
            });
        }
        // ts: `duration: pick([0, 30, 60, 60, 60])`
        let durations: [&str; 5] = ["0", "30", "60", "60", "60"];
        let duration: i64 = durations[int(ctx.rnd, durations.len())].parse().expect("duration literal");
        verbs_raw.insert(
            name.clone(),
            VerbContent {
                slots,
                duration,
                recipes: vec![RecipeDef {
                    id: Some(format!("{name}.r0")),
                    when,
                    shown: None,
                    hint: None,
                    out,
                }],
            },
        );
    }
    // (ctx's &mut rnd borrow ends at its last use above — NLL, no explicit
    // drop needed)

    // ts: reach-scale-lib.ts:101-106 — the 67 decks; spec length 5+int(26);
    // the defaultcard pick only draws when its roll passes (ternary)
    let mut decks: Vec<DeckRaw> = Vec::with_capacity(n_decks);
    for i in 0..n_decks {
        let n_spec = 5 + int(&mut rnd, 26);
        let mut spec: Vec<String> = Vec::with_capacity(n_spec);
        for _ in 0..n_spec {
            spec.push(proto_names[int(&mut rnd, proto_names.len())].clone());
        }
        let defaultcard = if rnd.next_f64() < 0.45 {
            Some(proto_names[int(&mut rnd, proto_names.len())].clone())
        } else {
            None
        };
        let reset = rnd.next_f64() < 0.8;
        decks.push(DeckRaw {
            id: format!("D{}", i + 1),
            spec,
            defaultcard,
            reset,
        });
    }

    // ts: reach-scale-lib.ts:107-113 — 8 season slot triggers + the expiry
    // pair, concatenated in that order
    let mut triggers: Vec<TriggerDef> = Vec::with_capacity(10);
    for i in 0..8 {
        let p = proto_names[int(&mut rnd, proto_names.len())].clone();
        let r = resources[int(&mut rnd, resources.len())].clone();
        let a = ASPECTS[int(&mut rnd, ASPECTS.len())];
        let v = 1 + int(&mut rnd, 5) as i64;
        triggers.push(TriggerDef::Slot(SlotTriggerDef {
            id: format!("season{i}"),
            slot: (i * 6) as i64,
            once: Some(false),
            out: vec![format!("SPAWN {p}"), format!("SET({r}, aspect:{a}, {v})")],
        }));
    }
    triggers.push(TriggerDef::Slot(SlotTriggerDef {
        id: "t_expiry".to_string(),
        slot: 40,
        once: Some(true),
        out: vec![format!("SPAWN {}", proto_names[int(&mut rnd, proto_names.len())].clone())],
    }));
    triggers.push(TriggerDef::Expire(ExpireTriggerDef {
        id: "t_big".to_string(),
        on_expire_of: "R1".to_string(),
        out: vec![
            format!("SPAWN {}", proto_names[int(&mut rnd, proto_names.len())].clone()),
            format!("DESTROY {}", resources[int(&mut rnd, resources.len())].clone()),
        ],
    }));

    // ts: reach-scale-lib.ts:135-137 — register the verb names, then compile.
    // Registration draws NOTHING, so hoisting it above the raw construction
    // (which consumes verbs_raw) leaves the LCG draw order untouched.
    let mut dict = core_dictionary();
    for name in verbs_raw.keys() {
        if !dict.is_verb(name) {
            dict.register_verb(name).expect("verb registration cannot fail here");
        }
    }
    let raw = ContentRaw {
        seed: Some(424242),
        verbs: verbs_raw,
        prototypes: Some(protos),
        decks: Some(decks.clone()),
        triggers: Some(triggers.clone()),
        ..ContentRaw::default()
    };

    // ts: reach-scale-lib.ts:122-133 — the initial world: ANA + the 20
    // resources (ALL 12 aspects each, drawn in ASPECTS order) + two followers
    fn ent(name: &str, kind: &str, aspects: IndexMap<String, AspectVal>) -> Entity {
        Entity {
            id: format!("s_{}", name.to_lowercase()),
            kind: kind.to_string(),
            name: name.to_string(),
            aspects,
            attrs: IndexMap::new(),
            born_action: Some(0),
            dead_action: None,
            stackable: false,
        }
    }
    let mut initial = new_state(0);
    add_entity(&mut initial, ent("ANA", "character", IndexMap::new()))
        .expect("initial entities never collide");
    for r in &resources {
        let mut aspects: IndexMap<String, AspectVal> = IndexMap::new();
        for a in ASPECTS {
            let v = 1 + int(&mut rnd, 3) as i64;
            aspects.insert(a.to_string(), AspectVal::Num(v));
        }
        add_entity(&mut initial, ent(r, "item", aspects)).expect("initial entities never collide");
    }
    let mut follower = IndexMap::new();
    follower.insert("follower".to_string(), AspectVal::Num(1));
    add_entity(&mut initial, ent("BERTHA", "character", follower.clone()))
        .expect("initial entities never collide");
    add_entity(&mut initial, ent("CARL", "character", follower))
        .expect("initial entities never collide");

    // ts: reach-scale-lib.ts:137 — compileContent(raw, dict) — NO entityKinds:
    // the declared list is empty
    let content = compile_content(&raw, &dict, &[]).expect("the CS-scale world compiles");

    // ts: reach-scale-lib.ts:138 — insertion-order dedup (a JS Set)
    let mut seen: IndexSet<String> = IndexSet::new();
    for n in &proto_names {
        seen.insert(n.clone());
    }
    for r in &resources {
        seen.insert(r.clone());
    }
    for n in ["ANA", "BERTHA", "CARL"] {
        seen.insert(n.to_string());
    }
    for d in &decks {
        for s in &d.spec {
            seen.insert(s.clone());
        }
    }
    ScaleWorld {
        content,
        dict,
        initial,
        names: seen.into_iter().collect(),
        stats: ScaleStats {
            protos: n_protos,
            recipes: n_recipes,
            ops,
            decks: n_decks,
            triggers: triggers.len(),
        },
    }
}

// ---------- census (single-process; the CLI shards across cores) ----------

/// ts: reach-scale-lib.ts:147-155 — the per-name stat row. FIELD ORDER IS THE
/// CROSS-LANGUAGE A/B INTERFACE (CLI-03 must): the worker-mode JSON and the
/// TS `JSON.stringify(stats)` must list these keys in this order.
#[derive(Debug, Clone, Serialize)]
pub struct NameStat {
    pub name: String,
    pub ms: f64,
    pub verdict: String,
    pub existence: String,
    pub roots: usize,
    pub nodes: usize,
    pub depth: usize,
}

/// ts: reach-scale-lib.ts:158-159
fn depth_of(p: &ReachPath, d: usize) -> usize {
    let mut best = d;
    for r in &p.requires {
        for sub in &r.paths {
            best = best.max(depth_of(sub, d + 1));
        }
    }
    best
}

/// ts: reach-scale-lib.ts:160-161
fn nodes_of(p: &ReachPath) -> usize {
    1 + p
        .requires
        .iter()
        .map(|r| r.paths.iter().map(nodes_of).sum::<usize>())
        .sum::<usize>()
}

/// ts: reach-scale-lib.ts:157-173 — one `analyze_card` per name; `ms` is
/// wall-clock and only a magnitude reference (PR1-03: the cross-language
/// comparison covers the NON-timing fields only).
///
/// Holds ONE [`Analyzer`] across all names: the TS module-level
/// `worldCache` WeakMap (reach.ts:615) shares WorldFacts across the
/// per-name `analyzeCard` calls — first call cold, the rest warm — and
/// that cache sharing is pinned observable behavior. The one-shot
/// `analyze_card` convenience would rebuild the Analyzer per name and
/// pay full price every time.
pub fn census(world: &ScaleWorld, names: &[String]) -> Vec<NameStat> {
    let analyzer = Analyzer::new(&world.content, &world.initial);
    names
        .iter()
        .map(|name| {
            let t = Instant::now();
            let rep = analyzer
                .analyze_card(name, Default::default())
                .expect("census probes analyze cleanly");
            let ms = t.elapsed().as_secs_f64() * 1000.0;
            let CardReport { paths, .. } = &rep;
            NameStat {
                name: name.clone(),
                ms,
                verdict: rep.verdict.as_str().to_string(),
                existence: rep.existence.as_str().to_string(),
                roots: paths.len(),
                nodes: paths.iter().map(nodes_of).sum(),
                depth: paths.iter().map(|p| depth_of(p, 0)).max().unwrap_or(0),
            }
        })
        .collect()
}

/// ts: reach-scale-lib.ts:175-184 — the CLI's one-line summary.
pub fn summarize(stats: &[NameStat]) -> String {
    let mut times: Vec<f64> = stats.iter().map(|st| st.ms).collect();
    times.sort_by(|a, b| a.partial_cmp(b).expect("ms values are finite"));
    let q = |p: f64| -> f64 { times[std::cmp::min(times.len() - 1, (times.len() as f64 * p).floor() as usize)] };
    // ts: `const verdicts: Record<string, number> = {}` — insertion order =
    // first occurrence in stats order (JSON.stringify prints it)
    let mut verdicts: IndexMap<&str, i64> = IndexMap::new();
    for st in stats {
        *verdicts.entry(st.verdict.as_str()).or_insert(0) += 1;
    }
    let verdicts_json = {
        let parts: Vec<String> = verdicts
            .iter()
            .map(|(k, v)| format!("\"{k}\":{v}"))
            .collect();
        format!("{{{}}}", parts.join(","))
    };
    let total: f64 = times.iter().sum();
    let roots: usize = stats.iter().map(|st| st.roots).sum();
    let max_roots: usize = stats.iter().map(|st| st.roots).max().unwrap_or(0);
    let depth: usize = stats.iter().map(|st| st.depth).max().unwrap_or(0);
    format!(
        "{} names: total {:.1} s | avg {:.1} ms | p50 {:.1} | p95 {:.1} | max {:.1} ms | {} | {} roots ({:.2}/name, max {max_roots}), deepest tree {depth} hops",
        stats.len(),
        total / 1000.0,
        total / stats.len() as f64,
        q(0.5),
        q(0.95),
        q(1.0),
        verdicts_json,
        roots,
        roots as f64 / stats.len() as f64,
    )
}

