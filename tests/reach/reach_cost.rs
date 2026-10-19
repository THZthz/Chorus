//! The `--cost` face: the cost/count reading of the settled facts. The
//! hand-provable chain fixture pins the estimate EXACTLY (two applications,
//! the duration sum, the witness chain — a falsification-minded equality,
//! not a bound), the trigger-only fixture pins the 0-applications/minutes
//! split, the deck fixtures pin the copies face (multiplicity vs the MANY
//! defaultcard), the replay fixture pins the cursor-consumed deck, and the
//! CLI faces pin the byte shapes, the composition and the ignored
//! combinations. The default face's byte identity is owned by the whole
//! existing suite plus the eight bundled-script text faces.

use std::process::Command;

use indexmap::IndexMap;

use chorus::content::script::compile_script;
use chorus::content::{
    compile_content, CompiledContent, ContentRaw, DeckRaw, PrototypeDef, RecipeDef, SlotDef,
    SlotTriggerDef, TriggerDef, VerbContent,
};
use chorus::core::clock::Clock;
use chorus::core::dict::core_dictionary;
use chorus::core::types::{add_entity, new_state, Entity, State};
use chorus::engine::store::save_log;
use chorus::engine::{execute_step, ExecOptions, Step};
use chorus::reach::cost::{cost_report, render_cost};
use chorus::reach::midgame::replay_state;
use chorus::reach::Analyzer;

// ---------------------------------------------------------------------------
// fixtures
// ---------------------------------------------------------------------------

const NIGHTJAR: &str = "scripts/the-nightjar-flyer.txt";

/// The hand-provable chain: SEED_A is initial; WORK (30m) binds SEED_A and
/// spawns MID_B; DREAM (1h) binds MID_B and spawns TOP_C. TOP_C costs
/// exactly 2 applications and exactly 90 minutes.
const CHAIN_SCRIPT: &str = "\
script: Cost Fixture
player: ANA
goal: exercise the cost face
seed: 3
start: 0:00

entities:
  ROOM is place
  ANA is character at ROOM
  SEED_A is item

prototypes:
  MID_B is item
  TOP_C is item

verbs:
  WORK:
    object: 1 name:SEED_A
    duration: 30m
    recipes:
      work.make -> SPAWN MID_B
  DREAM:
    object: 1 name:MID_B
    duration: 1h
    recipes:
      dream.make -> SPAWN TOP_C
";

fn ana() -> Entity {
    let mut e = Entity {
        id: "s_ana".into(),
        kind: "character".into(),
        name: "ANA".into(),
        aspects: IndexMap::new(),
        attrs: IndexMap::new(),
        born_action: Some(0),
        dead_action: None,
        stackable: false,
    };
    e.attrs.insert("container".into(), chorus::core::types::AttrValue::Str("ROOM".into()));
    e
}

fn state_with(es: Vec<Entity>) -> State {
    let mut s = new_state(0);
    for e in es {
        add_entity(&mut s, e).expect("fixture entities never collide");
    }
    s
}

fn compile(raw: &ContentRaw) -> CompiledContent {
    let dict = core_dictionary();
    compile_content(raw, &dict, &[]).expect("fixture content compiles")
}

fn proto(kind: &str) -> PrototypeDef {
    PrototypeDef { kind: kind.to_string(), ..PrototypeDef::default() }
}

fn stackable_proto(kind: &str) -> PrototypeDef {
    PrototypeDef { kind: kind.to_string(), stackable: true, ..PrototypeDef::default() }
}

fn slot_trigger(id: &str, tick: i64, once: bool, out: &[&str]) -> TriggerDef {
    TriggerDef::Slot(SlotTriggerDef {
        id: id.to_string(),
        slot: tick,
        once: Some(once),
        out: out.iter().map(|s| (*s).to_string()).collect(),
    })
}

fn object_slot(query: &str) -> SlotDef {
    SlotDef { role: "object".into(), min: 1, max: Some(1), query: Some(query.into()), shown: None }
}

fn recipe(id: &str, out: &[&str]) -> RecipeDef {
    RecipeDef {
        id: Some(id.to_string()),
        when: None,
        shown: None,
        hint: None,
        max: None,
        aspects: Vec::new(),
        dur: None,
        out: out.iter().map(|s| (*s).to_string()).collect(),
    }
}

/// One verb with a single object slot and one recipe.
fn one_verb(slot_query: &str, duration: i64, recipe: RecipeDef) -> VerbContent {
    VerbContent { slots: vec![object_slot(slot_query)], duration, recipes: vec![recipe] }
}

fn wait_to(
    content: &CompiledContent,
    from: State,
    day: i64,
    tick: i64,
    first_record_id: i64,
) -> chorus::engine::ExecResult {
    let dict = core_dictionary();
    execute_step(
        content,
        &dict,
        from,
        Step { start_min: None, clock: Some(Clock { day, tick }), clauses: vec![], t_real: None },
        ExecOptions { first_record_id, id_gen: None },
    )
    .expect("the step executes")
}

// ---------------------------------------------------------------------------
// the chain fixture: the estimate equals the hand-computed numbers EXACTLY
// ---------------------------------------------------------------------------

#[test]
fn the_chain_fixture_costs_exactly_two_applications_and_the_duration_sum() {
    let dict = core_dictionary();
    let compiled = compile_script(CHAIN_SCRIPT, &dict).unwrap();
    let (content, state) = (&compiled.content, &compiled.state);
    let analyzer = Analyzer::new(content, state, &dict);

    // TOP_C: exactly 2 applications (SEED_A free, WORK once, DREAM once) and
    // exactly 90 minutes (30m + 1h) — equality, not a bound
    let top = cost_report(&analyzer, "TOP_C");
    let plan = top.plan.as_ref().expect("TOP_C has a proven DAG");
    assert_eq!(plan.apps, 2, "falsification: the estimate must equal the hand count");
    assert_eq!(plan.minutes, 90, "falsification: the clock must equal the duration sum");
    assert_eq!(
        plan.witness,
        "initial entity SEED_A (item) → recipe work.make of WORK mints MID_B → recipe dream.make of DREAM mints TOP_C"
    );

    // the section renders byte-exact
    let mut buf = String::new();
    render_cost(&mut buf, "script.txt", "TOP_C", &top);
    assert_eq!(
        buf,
        "script.txt · card TOP_C — cost\n  \
         applications: 2 (lower-bound planning estimate)\n  \
         witness: initial entity SEED_A (item) → recipe work.make of WORK mints MID_B → recipe dream.make of DREAM mints TOP_C\n  \
         minutes: 90 (planning estimate)\n  \
         copies: 1 — ×1 SPAWN TOP_C by recipe dream.make of DREAM\n  \
         note: lower-bound planning estimate — each needed recipe applies once with its fillers in hand; gate timing is honored only through provenness\n"
    );

    // the middle hop: exactly 1 application, exactly 30 minutes
    let mid = cost_report(&analyzer, "MID_B");
    let plan = mid.plan.as_ref().expect("MID_B has a proven DAG");
    assert_eq!(plan.apps, 1);
    assert_eq!(plan.minutes, 30);
    assert_eq!(plan.witness, "initial entity SEED_A (item) → recipe work.make of WORK mints MID_B");

    // the initial anchor: 0 applications, 0 minutes, the initial copies fact
    let seed = cost_report(&analyzer, "SEED_A");
    let plan = seed.plan.as_ref().expect("SEED_A is initial");
    assert_eq!(plan.apps, 0);
    assert_eq!(plan.minutes, 0);
    assert_eq!(plan.witness, "initial entity SEED_A (item)");
    let copies = seed.copies.as_ref().expect("the initial fact carries copies");
    assert_eq!(copies.text, "1");
    assert_eq!(copies.detail.as_deref(), Some("×1 initial entity SEED_A (item)"));
}

// ---------------------------------------------------------------------------
// the per-recipe `dur` face: the minutes walk reads the recipe's OWN window
// ---------------------------------------------------------------------------

/// WORK's verb default is 1h but its single recipe carries `dur 10m` (§6.4) —
/// the hop costs ONE tick, not the verb's six.
const DUR_CHAIN_SCRIPT: &str = "\
script: Cost Dur Fixture
player: ANA
goal: exercise the cost face
seed: 3
start: 0:00

entities:
  ROOM is place
  ANA is character at ROOM
  SEED_A is item

prototypes:
  MID_B is item

verbs:
  WORK:
    object: 1 name:SEED_A
    duration: 1h
    recipes:
      work.make dur 10m -> SPAWN MID_B
";

#[test]
fn the_minutes_walk_costs_the_recipe_dur_not_the_verb_default() {
    let dict = core_dictionary();
    let compiled = compile_script(DUR_CHAIN_SCRIPT, &dict).unwrap();
    let (content, state) = (&compiled.content, &compiled.state);
    let analyzer = Analyzer::new(content, state, &dict);

    let mid = cost_report(&analyzer, "MID_B");
    let plan = mid.plan.as_ref().expect("MID_B has a proven DAG");
    assert_eq!(plan.apps, 1);
    assert_eq!(plan.minutes, 10, "the recipe's own dur — one tick, not the verb's 60");
}

// ---------------------------------------------------------------------------
// trigger-owned provenance: 0 applications, the first crossing in minutes
// ---------------------------------------------------------------------------

#[test]
fn a_slot_trigger_costs_minutes_but_zero_applications() {
    let raw = ContentRaw {
        prototypes: Some(IndexMap::from([("GEM".to_string(), proto("item"))])),
        triggers: Some(vec![slot_trigger("t_gem", 6, true, &["SPAWN GEM"])]),
        ..ContentRaw::default()
    };
    let content = compile(&raw);
    let state = state_with(vec![ana()]);

    // the analyzer's dictionary: the fixture declares no vocabulary, so
    // the core seed IS the load-effective one
    let dict = core_dictionary();
    let analyzer = Analyzer::new(&content, &state, &dict);

    let rep = cost_report(&analyzer, "GEM");
    let plan = rep.plan.as_ref().expect("GEM is proven");
    // the clock fires the trigger: no verb application, but the wait is real
    assert_eq!(plan.apps, 0);
    assert_eq!(plan.minutes, 60, "tick 6 at now 0 is minute 60");
    assert_eq!(plan.witness, "trigger t_gem mints GEM");

    let mut buf = String::new();
    render_cost(&mut buf, "script.txt", "GEM", &rep);
    assert_eq!(
        buf,
        "script.txt · card GEM — cost\n  \
         applications: 0 (lower-bound planning estimate)\n  \
         witness: trigger t_gem mints GEM\n  \
         minutes: 60 (planning estimate)\n  \
         copies: 1 — ×1 SPAWN GEM by trigger t_gem\n  \
         note: lower-bound planning estimate — each needed recipe applies once with its fillers in hand; gate timing is honored only through provenness\n"
    );
}

/// The expiry face: the trigger's firing is arranged (costed 0) but its
/// required card recurs — and the minutes walk may route differently from
/// the applications walk.
#[test]
fn an_expiry_trigger_is_arranged_and_its_required_card_recurs() {
    let raw = ContentRaw {
        prototypes: Some(IndexMap::from([
            ("SEED_X".to_string(), proto("item")),
            ("YIELD_Y".to_string(), proto("item")),
        ])),
        triggers: Some(vec![TriggerDef::Expire(chorus::content::ExpireTriggerDef {
            id: "t_exp".to_string(),
            on_expire_of: "SEED_X".to_string(),
            out: vec!["SPAWN YIELD_Y".to_string()],
        })]),
        ..ContentRaw::default()
    };
    let content = compile(&raw);
    let mut seed = Entity {
        id: "s_seed_x".into(),
        kind: "item".into(),
        name: "SEED_X".into(),
        aspects: IndexMap::new(),
        attrs: IndexMap::new(),
        born_action: Some(0),
        dead_action: None,
        stackable: false,
    };
    seed.attrs.insert("container".into(), chorus::core::types::AttrValue::Str("ROOM".into()));
    seed.attrs.insert("expires".into(), chorus::core::types::AttrValue::Str("1h".into()));
    let state = state_with(vec![ana(), seed]);
    let dict = core_dictionary();
    let analyzer = Analyzer::new(&content, &state, &dict);

    let rep = cost_report(&analyzer, "YIELD_Y");
    let plan = rep.plan.as_ref().expect("YIELD_Y is proven through the expiry trigger");
    assert_eq!(plan.apps, 0, "the expiry is arranged, not applied");
    assert_eq!(plan.minutes, 0, "arranged — no clock weight on the trigger hop");
    assert_eq!(plan.witness, "initial entity SEED_X (item) → trigger t_exp mints YIELD_Y");
    assert!(
        rep.notes.iter().any(|n| n == "trigger t_exp's firing is arranged (its required card's expiry) — costed 0; the required card still recurs"),
        "{:?}",
        rep.notes
    );
}

// ---------------------------------------------------------------------------
// the copies face: deck multiplicity vs the MANY defaultcard
// ---------------------------------------------------------------------------

#[test]
fn copies_read_the_avail_counts_deck_multiplicity_versus_defaultcard_many() {
    let raw = ContentRaw {
        prototypes: Some(IndexMap::from([
            ("LEAF".to_string(), stackable_proto("item")),
            ("OTHER".to_string(), stackable_proto("item")),
            ("PEBBLE".to_string(), stackable_proto("item")),
        ])),
        decks: Some(vec![
            DeckRaw {
                id: "POOL".to_string(),
                spec: vec!["LEAF".to_string(), "LEAF".to_string()],
                defaultcard: None,
                reset: false,
            },
            DeckRaw {
                id: "HAND".to_string(),
                spec: vec!["OTHER".to_string()],
                defaultcard: Some("PEBBLE".to_string()),
                reset: false,
            },
        ]),
        verbs: IndexMap::from([
            // two verbs, one draw list each: a repeatable draw sharing its
            // list with a co-mint freezes the cursor at 1 (the census's
            // own rule) and would never register the second order position
            ("ACQUIRE".to_string(), one_verb("kind:item", 0, recipe("deal", &["DRAW POOL"]))),
            ("DREAM".to_string(), one_verb("kind:item", 0, recipe("deal_hand", &["DRAW HAND"]))),
        ]),
        ..ContentRaw::default()
    };
    let content = compile(&raw);
    let mut seed = Entity {
        id: "s_seed".into(),
        kind: "item".into(),
        name: "SEED".into(),
        aspects: IndexMap::new(),
        attrs: IndexMap::new(),
        born_action: Some(0),
        dead_action: None,
        stackable: false,
    };
    seed.attrs.insert("container".into(), chorus::core::types::AttrValue::Str("ROOM".into()));
    let state = state_with(vec![ana(), seed]);
    let dict = core_dictionary();
    let analyzer = Analyzer::new(&content, &state, &dict);

    // the recipe-owned draws cost one application each and their verb's
    // duration (0 here) in minutes
    let leaf = cost_report(&analyzer, "LEAF");
    let plan = leaf.plan.as_ref().expect("LEAF is proven through the deal recipe");
    assert_eq!(plan.apps, 1);
    assert_eq!(plan.minutes, 0);

    // the deck order carries LEAF twice within the served window: 2 copies.
    // The permutation is seed-fixed at load — the FIRST occurrence's
    // position is read off the compiled deck, never assumed.
    let pool = content.decks.get("POOL").unwrap();
    let first_leaf = pool.order.iter().position(|c| c.name == "LEAF").unwrap();
    let copies = leaf.copies.as_ref().expect("the draw registered copies");
    assert_eq!(copies.text, "2");
    assert_eq!(
        copies.detail.as_deref(),
        Some(&*format!("×2 LEAF ← deck POOL order[{first_leaf}]"))
    );

    // the defaultcard is the MANY sentinel: unbounded, no detail rows
    let pebble = cost_report(&analyzer, "PEBBLE");
    let copies = pebble.copies.as_ref().expect("the defaultcard registered copies");
    assert_eq!(copies.text, "unbounded");
    assert_eq!(copies.detail, None);

    // a single order position: one copy
    let other = cost_report(&analyzer, "OTHER");
    let copies = other.copies.as_ref().expect("OTHER is dealt");
    assert_eq!(copies.text, "1");
    assert_eq!(copies.detail.as_deref(), Some("×1 OTHER ← deck HAND order[0]"));
}

// ---------------------------------------------------------------------------
// --from-state: the costs anchor to the replayed world (the cursor-consumed
// deck shrinks the draw fact; the drawn copies ride as initial facts)
// ---------------------------------------------------------------------------

#[test]
fn the_replayed_world_anchors_costs_and_the_cursor_consumes_deck_copies() {
    let dict = core_dictionary();
    let raw = ContentRaw {
        seed: Some(3),
        prototypes: Some(IndexMap::from([
            ("A_CARD".to_string(), stackable_proto("item")),
            ("B_CARD".to_string(), stackable_proto("item")),
        ])),
        decks: Some(vec![DeckRaw {
            id: "BOX".to_string(),
            spec: vec!["A_CARD".to_string(), "B_CARD".to_string(), "B_CARD".to_string()],
            defaultcard: None,
            reset: false,
        }]),
        triggers: Some(vec![slot_trigger("t_draw", 3, false, &["DRAW BOX"])]),
        ..ContentRaw::default()
    };
    let content = compile(&raw);
    let initial = state_with(vec![ana()]);
    let order = content.decks.get("BOX").unwrap().order.clone();
    let bs_in = |from: usize| order[from..].iter().filter(|c| c.name == "B_CARD").count();

    // the script face: the whole permutation serves — both B copies
    let script_face = Analyzer::new(&content, &initial, &dict);
    let script_cost = cost_report(&script_face, "B_CARD");
    assert_eq!(script_cost.copies.as_ref().expect("copies").text, "2");

    // play two wait steps a day apart: the recurring draw fires twice, the
    // cursor consumes order[0..2), and the drawn copies ride the replay
    let r1 = wait_to(&content, new_state(0), 0, 3, 1);
    assert_eq!(r1.state.decks.get("BOX"), Some(&1));
    let r2 = wait_to(&content, r1.state, 1, 3, r1.records.len() as i64 + 1);
    assert_eq!(r2.state.decks.get("BOX"), Some(&2));
    let mut records = r1.records;
    records.extend(r2.records);
    let path = crate::common::rig::tmp_log_path("reach-cost-replay");
    save_log(&path, &records).unwrap();
    let mid = replay_state(&path, &initial, &dict, &content).unwrap();
    assert_eq!(mid.now, 1470, "day 1, tick 3");

    // the replayed face: the deck row shrinks to the remaining window
    // (order[2..3)), the drawn copies are initial facts, and the clock
    // re-anchors (t_draw's next crossing is a full day out: 1440)
    let analyzer = Analyzer::new(&content, &mid, &dict).replayed();
    let cost = cost_report(&analyzer, "B_CARD");
    let hand = mid.entities.values().filter(|e| e.name == "B_CARD").count();
    let window = bs_in(2);
    assert_eq!(
        cost.copies.as_ref().expect("copies").text,
        (hand + window).to_string(),
        "the copies total is the replayed world's own count"
    );
    let detail = cost.copies.as_ref().unwrap().detail.clone().unwrap_or_default();
    if hand > 0 {
        assert!(detail.contains(&format!("×{hand} initial entity B_CARD (item)")), "{detail}");
    }
    if window > 0 {
        assert!(detail.contains(&format!("×{window} B_CARD ← deck BOX order[")), "{detail}");
    }
    assert!(window < 2, "the cursor must have consumed at least one B position");
    let plan = cost.plan.as_ref().expect("the recurring draw still proves");
    assert_eq!(plan.apps, 0);
    // the clock re-anchors to the replayed clock: t_draw's next crossing is
    // a full day out (the d == 0 → 1440 branch) — unless a drawn copy is in
    // hand, in which case the initial anchor's 0 wins
    assert_eq!(plan.minutes, if hand > 0 { 0 } else { 1440 });
    // the consumed head card: gone from the deck window it was drawn from —
    // what remains is its replayed copy (or copies, when the head name was
    // drawn twice into hand) plus any tail occurrence
    let head = &order[0].name;
    let head_cost = cost_report(&analyzer, head);
    let hand_head = mid.entities.values().filter(|e| e.name == *head).count();
    let tail_head = usize::from(order[2].name == *head);
    assert_eq!(
        head_cost.copies.as_ref().expect("copies").text,
        (hand_head + tail_head).to_string()
    );
}

// ---------------------------------------------------------------------------
// the bounded-time faces compose: the --by window bounds the DAG
// ---------------------------------------------------------------------------

#[test]
fn the_by_window_bounds_the_dag_and_a_note_says_so() {
    let raw = ContentRaw {
        prototypes: Some(IndexMap::from([("GEM".to_string(), proto("item"))])),
        triggers: Some(vec![slot_trigger("t_gem", 6, true, &["SPAWN GEM"])]),
        ..ContentRaw::default()
    };
    let content = compile(&raw);
    let state = state_with(vec![ana()]);

    // the analyzer's dictionary: the fixture declares no vocabulary, so
    // the core seed IS the load-effective one
    let dict = core_dictionary();

    // M before the crossing: the site fell out of the window — no DAG
    let before = Analyzer::new(&content, &state, &dict).by(30);
    let rep = cost_report(&before, "GEM");
    assert!(rep.plan.is_none());
    let mut buf = String::new();
    render_cost(&mut buf, "script.txt", "GEM", &rep);
    assert_eq!(
        buf,
        "script.txt · card GEM — cost\n  \
         (no proven provenance DAG for 'GEM' — the candidate sources above are excluded from costing)\n"
    );

    // a wide M: the site participates, the window note rides
    let wide = Analyzer::new(&content, &state, &dict).by(2000);
    let rep = cost_report(&wide, "GEM");
    let plan = rep.plan.as_ref().expect("inside the window the site proves");
    assert_eq!(plan.apps, 0);
    assert_eq!(plan.minutes, 60);
    assert!(
        rep.notes.iter().any(|n| n
            == "the by 2000 window bounds the DAG — a source proven only beyond it is excluded"),
        "{:?}",
        rep.notes
    );
}

// ---------------------------------------------------------------------------
// the CLI faces
// ---------------------------------------------------------------------------

struct Cli {
    out: String,
    err: String,
    code: Option<i32>,
}

fn run(args: &[&str]) -> Cli {
    let out = Command::new(env!("CARGO_BIN_EXE_reach"))
        .args(args)
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .output()
        .expect("the bin spawns");
    Cli {
        out: String::from_utf8_lossy(&out.stdout).to_string(),
        err: String::from_utf8_lossy(&out.stderr).to_string(),
        code: out.status.code(),
    }
}

/// Write `text` to a per-process temp file, return its path.
fn temp_file(tag: &str, name: &str, text: &str) -> String {
    let dir = std::env::temp_dir().join(format!("chorus-test-{tag}-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let p = dir.join(name);
    std::fs::write(&p, text).unwrap();
    p.to_string_lossy().to_string()
}

#[test]
fn the_cli_appends_one_cost_section_per_card_and_the_default_face_stays_frozen() {
    let script = temp_file("reach-cost-cli", "chain.txt", CHAIN_SCRIPT);

    // --all --cost: one cost section per walked card (the cost header also
    // contains " · card ", so the reports are counted by their verdict lines)
    let c = run(&[&script, "all", "--cost"]);
    assert_eq!(c.code, Some(0), "{}{}", c.out, c.err);
    let cards = c.out.matches("\n  verdict: ").count();
    assert_eq!(cards, 5, "prototypes ∪ live initials: MID_B TOP_C SEED_A ANA ROOM");
    assert_eq!(c.out.matches(" — cost\n").count(), cards, "{}", c.out);
    assert!(c.out.contains("applications: 2 (lower-bound planning estimate)"), "{}", c.out);

    // without the flag: byte-identical frozen face, no cost section
    let plain = run(&[&script, "all"]);
    assert_eq!(plain.code, Some(0));
    assert!(!plain.out.contains("— cost"), "{}", plain.out);

    // a single card: the report precedes the section byte-exactly
    let c = run(&[&script, "card", "TOP_C", "--cost"]);
    assert_eq!(c.code, Some(0), "{}{}", c.out, c.err);
    let plain = run(&[&script, "card", "TOP_C"]);
    assert!(c.out.starts_with(plain.out.as_str()), "the frozen face must be a byte prefix");

    // a script with NO command word is the argv error: the command list, exit 2
    let c = run(&[&script]);
    assert_eq!(c.code, Some(2), "{}{}", c.out, c.err);
    assert!(c.err.contains("reach needs a command"), "{}", c.err);
    assert!(c.err.contains("usage: reach <script.txt> <COMMAND>"), "{}", c.err);
}

#[test]
fn the_cli_composes_with_usable_why_and_ignores_demand_and_json() {
    // --usable composes: the semantics bracket and the section coexist
    let c = run(&[NIGHTJAR, "card", "THE_TENDED_LORD", "--usable", "--cost"]);
    assert_eq!(c.code, Some(0), "{}{}", c.out, c.err);
    assert!(c.out.contains("[usable semantics]"), "{}", c.out);
    assert!(c.out.contains(" · card THE_TENDED_LORD — cost\n"), "{}", c.out);

    // --why composes: the report, then the why section, then the cost section
    let c = run(&[NIGHTJAR, "card", "THE_TENDED_LORD", "--why", "--cost"]);
    assert_eq!(c.code, Some(0), "{}{}", c.out, c.err);
    let report_end = c.out.find("\n\n").expect("the report's blank line");
    let why = c.out.find(" — why\n").expect("the why section");
    let cost = c.out.find(" — cost\n").expect("the cost section");
    assert!(report_end < why && why < cost, "{}", c.out);

    // --demand: documented ignore — the demand match is not costed
    let c = run(&[NIGHTJAR, "demand", "aspect:secret>=3", "--cost"]);
    assert_eq!(c.code, Some(0), "{}{}", c.out, c.err);
    assert!(!c.out.contains("— cost"), "{}", c.out);

    // --json: kept out for now — the envelope carries no cost data
    let c = run(&[NIGHTJAR, "card", "THE_TENDED_LORD", "--json", "--cost"]);
    assert_eq!(c.code, Some(0), "{}{}", c.out, c.err);
    assert!(c.out.trim_start().starts_with('{'), "{}", c.out);
    assert!(!c.out.contains("\"cost\""), "{}", c.out);
}

#[test]
fn the_non_proven_target_renders_the_exclusion_face() {
    // the candidate card: no proven DAG, the candidates sit above
    let c = run(&[NIGHTJAR, "card", "THE_WAX_CYLINDER", "--cost"]);
    assert_eq!(c.code, Some(0), "{}{}", c.out, c.err);
    assert!(
        c.out.contains(
            "scripts/the-nightjar-flyer.txt · card THE_WAX_CYLINDER — cost\n  \
             (no proven provenance DAG for 'THE_WAX_CYLINDER' — the candidate sources above are excluded from costing)\n"
        ),
        "{}",
        c.out
    );
}
