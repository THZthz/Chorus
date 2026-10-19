//! The reach analyzer's §4.2 unique faces (v0.9.6) — EXACT in three faces,
//! under-claiming in one composition:
//!
//! - a unique serves from its FIRST order position only; every later
//!   occurrence is a dead position (the earliest copy's serve or skip
//!   always manifests the name before the cursor reaches the later
//!   position — a proof needing the same unique twice must NOT
//!   over-prove);
//! - a unique in the initial state's manifested set (the midgame truth)
//!   never serves from any deck — a deck whose uniques are all manifested
//!   falls to the defaultcard face;
//! - the world's pooled total for a unique name clamps at 1 (one living
//!   copy, ever), whatever the sources' own counts — deck + ambient mint
//!   composes to exactly one living copy by the clamp alone.
//!
//! The boundary: in-play mints do NOT mark manifested in the analyzer's
//! world (the manifested set is read once from the initial state; no mint
//! site writes it), so a unique's deck position reads LIVE even where an
//! in-play mint manifests the name before the draw. A bounded draw whose
//! single serve budget is pinned on that phantom-live position then
//! under-claims — a false unobtainable, ledgered in the reach-fuzz campaign
//! header (reach_fuzz.rs) and pinned here as an engine+reach split test.
//! Everything else stays exact in both directions (at play a unique name
//! never has two living copies).

use indexmap::{IndexMap, IndexSet};

use chorus::content::{
    compile_content, ContentRaw, DeckRaw, PrototypeDef, RecipeDef, SlotDef, VerbContent,
};
use chorus::core::clock::parse_clock;
use chorus::core::dict::core_dictionary;
use chorus::core::r#ref::Ref;
use chorus::core::sentence::Arg;
use chorus::core::types::{add_entity, clone_state, new_state, AspectVal, Entity, State};
use chorus::engine::fold::fold;
use chorus::engine::{execute_step, ExecOptions, Step};
use chorus::reach::{analyze_card, CardOptions, CardVerdict};

use crate::common::rig::settle_line;
use crate::common::snap::snap;

/// Compile a raw fixture against a fresh core dictionary.
fn compile(raw: &ContentRaw) -> chorus::content::CompiledContent {
    let dict = core_dictionary();
    compile_content(raw, &dict, &[]).unwrap()
}

/// The fixture player: an ANA character, so the WAIT verbs commit.
fn ana_state() -> State {
    let mut s = new_state(0);
    let a: IndexMap<String, AspectVal> = IndexMap::new();
    add_entity(
        &mut s,
        Entity {
            id: "s_ana".into(),
            kind: "character".into(),
            name: "ANA".into(),
            aspects: a,
            attrs: IndexMap::new(),
            born_action: Some(0),
            dead_action: None,
            stackable: false,
            desc: None,
        },
    )
    .unwrap();
    s
}

fn analyze(
    content: &chorus::content::CompiledContent,
    state: &State,
    target: &str,
) -> chorus::reach::CardReport {
    let dict = core_dictionary();
    analyze_card(content, state, &dict, target, CardOptions::default()).unwrap()
}

fn proto(name: &str, kind: &str, unique: bool) -> (String, PrototypeDef) {
    (name.to_string(), PrototypeDef { kind: kind.to_string(), unique, ..PrototypeDef::default() })
}

fn wait_verb(out: &[&str]) -> (String, VerbContent) {
    (
        "WAIT".to_string(),
        VerbContent {
            slots: vec![SlotDef {
                role: "actors".into(),
                min: 1,
                max: Some(1),
                query: Some("kind:character".into()),
                shown: None,
            }],
            duration: 0,
            recipes: vec![RecipeDef {
                id: None,
                when: None,
                shown: None,
                hint: None,
                max: None,
                aspects: Vec::new(),
                dur: None,
                out: out.iter().map(|s| (*s).to_string()).collect(),
            }],
            ambient: false,
        },
    )
}

/// The deck fixture: `QUEST spec A, B, A` — seed 0 is the identity
/// permutation, so the order reads [A, B, A] and A's second occurrence sits
/// at position 2. `unique_name` substitutes the spec's unique entry.
fn quest_deck(unique_name: &str) -> DeckRaw {
    DeckRaw {
        id: "QUEST".into(),
        spec: vec![unique_name.into(), "B".into(), unique_name.into()],
        ..Default::default()
    }
}

#[test]
fn a_unique_never_proves_twice_from_its_own_deck_the_second_position_is_dead() {
    // the analyzer's world: A obtainable (its first position proves), but a
    // recipe gated on a SECOND living A never proves — at play the second
    // QUEST position skips (the first serve manifested A), so claiming the
    // gate true would over-prove. The control gate (one A) proves.
    let raw = ContentRaw {
        prototypes: Some(IndexMap::from([
            proto("A", "idea", true),
            proto("B", "idea", false),
            proto("TARGET", "item", false),
            proto("OK", "item", false),
        ])),
        decks: Some(vec![quest_deck("A")]),
        verbs: IndexMap::<String, VerbContent>::from([
            wait_verb(&["DRAW QUEST"]),
            (
                "STUDY".to_string(),
                VerbContent {
                    slots: vec![SlotDef {
                        role: "actors".into(),
                        min: 1,
                        max: Some(1),
                        query: Some("kind:character".into()),
                        shown: None,
                    }],
                    duration: 0,
                    recipes: vec![RecipeDef {
                        id: None,
                        when: Some("count(name:A) >= 2".to_string()),
                        shown: None,
                        hint: None,
                        max: None,
                        aspects: Vec::new(),
                        dur: None,
                        out: vec!["SPAWN TARGET".to_string()],
                    }],
                    ambient: false,
                },
            ),
            (
                "TALK".to_string(),
                VerbContent {
                    slots: vec![SlotDef {
                        role: "actors".into(),
                        min: 1,
                        max: Some(1),
                        query: Some("kind:character".into()),
                        shown: None,
                    }],
                    duration: 0,
                    recipes: vec![RecipeDef {
                        id: None,
                        when: Some("count(name:A) >= 1".to_string()),
                        shown: None,
                        hint: None,
                        max: None,
                        aspects: Vec::new(),
                        dur: None,
                        out: vec!["SPAWN OK".to_string()],
                    }],
                    ambient: false,
                },
            ),
        ]),
        ..ContentRaw::default()
    };
    let content = compile(&raw);
    let state = ana_state();
    let a = analyze(&content, &state, "A");
    assert_eq!(a.verdict, CardVerdict::Obtainable); // the first position proves
    let ok = analyze(&content, &state, "OK");
    assert_eq!(ok.verdict, CardVerdict::Obtainable); // one A suffices
    let target = analyze(&content, &state, "TARGET");
    assert_ne!(
        target.verdict,
        CardVerdict::Obtainable,
        "a second living A is impossible at play — the analyzer must not over-prove it"
    );
}

#[test]
fn a_manifested_unique_never_serves_and_the_all_manifested_deck_falls_to_the_defaultcard() {
    // the midgame face: the initial state's manifested set is the truth —
    // QUEST's A position (a one-entry deck here) is dead, the defaultcard
    // face serves, and the manifested A itself is no longer obtainable by
    // draw
    let raw = ContentRaw {
        prototypes: Some(IndexMap::from([proto("A", "idea", true), proto("D", "idea", false)])),
        decks: Some(vec![DeckRaw {
            id: "TOMB".into(),
            spec: vec!["A".into()],
            defaultcard: Some("D".into()),
            ..Default::default()
        }]),
        verbs: IndexMap::<String, VerbContent>::from([wait_verb(&["DRAW TOMB"])]),
        ..ContentRaw::default()
    };
    let content = compile(&raw);
    let mut state = ana_state();
    state.manifested_uniques.insert("A".to_string());
    let d = analyze(&content, &state, "D");
    assert_eq!(d.verdict, CardVerdict::Obtainable); // the defaultcard face
    let a = analyze(&content, &state, "A");
    assert_ne!(a.verdict, CardVerdict::Obtainable, "a manifested unique's deck position is dead");
    // the control: the same content with an EMPTY manifested set proves A
    let fresh = analyze(&content, &ana_state(), "A");
    assert_eq!(fresh.verdict, CardVerdict::Obtainable);
}

#[test]
fn the_count_clamp_keeps_the_in_play_mint_plus_deck_composition_at_one_living_copy() {
    // the clamp face: the ambient SPAWN proves U unconditionally, and the
    // pooled total clamps U at ONE — the STUDY gate needs a SECOND living U
    // and decides False over the clamped world, whatever the two sources'
    // own counts sum to. The deck position plays no part: the analyzer's
    // world never marks the in-play mint manifested, so QUEST's U position
    // reads live there — and the unbounded WAIT draw serves B either way.
    let raw = ContentRaw {
        prototypes: Some(IndexMap::from([
            proto("U", "idea", true),
            proto("B", "idea", false),
            proto("TARGET", "item", false),
        ])),
        decks: Some(vec![quest_deck("U")]),
        verbs: IndexMap::<String, VerbContent>::from([
            (
                "WORK".to_string(),
                VerbContent {
                    slots: Vec::new(),
                    duration: 30,
                    recipes: vec![RecipeDef {
                        id: None,
                        when: None,
                        shown: None,
                        hint: None,
                        max: None,
                        aspects: Vec::new(),
                        dur: None,
                        out: vec!["SPAWN U".to_string()],
                    }],
                    ambient: true,
                },
            ),
            (
                "STUDY".to_string(),
                VerbContent {
                    slots: vec![SlotDef {
                        role: "actors".into(),
                        min: 1,
                        max: Some(1),
                        query: Some("kind:character".into()),
                        shown: None,
                    }],
                    duration: 0,
                    recipes: vec![RecipeDef {
                        id: None,
                        when: Some("count(name:U) >= 2".to_string()),
                        shown: None,
                        hint: None,
                        max: None,
                        aspects: Vec::new(),
                        dur: None,
                        out: vec!["SPAWN TARGET".to_string()],
                    }],
                    ambient: false,
                },
            ),
            wait_verb(&["DRAW QUEST"]),
        ]),
        ..ContentRaw::default()
    };
    let content = compile(&raw);
    let state = ana_state();
    let u = analyze(&content, &state, "U");
    assert_eq!(u.verdict, CardVerdict::Obtainable);
    let b = analyze(&content, &state, "B");
    assert_eq!(b.verdict, CardVerdict::Obtainable); // the deck still serves B
    let target = analyze(&content, &state, "TARGET");
    assert_ne!(
        target.verdict,
        CardVerdict::Obtainable,
        "the count clamp composes the ambient mint + deck to ONE living copy, never two"
    );
}

// ---------------------------------------------------------- the boundary ----

#[test]
fn the_once_draw_plus_in_play_mint_composition_splits_engine_serves_b_analyzer_says_unobtainable() {
    // the ledgered gap (reach_fuzz.rs header, §13.3), pinned as the CURRENT
    // honest face on BOTH sides — the point is visibility, not silence.
    // Repro shape: deck QUEST spec U, B; a `once` slot trigger drawing
    // QUEST; a verb minting U. At play the mint lands first — U manifests —
    // and the trigger's single draw then skips the dead U position and
    // serves B. The analyzer's manifested projection is load-time only, so
    // its once-window pins the single serve on U's phantom-live position 0
    // and judges B unobtainable/None (the conservative false unobtainable;
    // with U non-unique the single serve genuinely takes position 0 and the
    // analyzer would be right).
    let dict = core_dictionary();
    let text = "\
script: Once Draw Split
player: ANA
start: 0:0
seed: 0

entities:
  ANA is character at HOUSE
  HOUSE is place

prototypes:
  U is idea unique
  B is idea stackable

decks:
  QUEST spec U, B

triggers:
  grant once at hour 1 -> DRAW QUEST

verbs:
  WAIT:
    actors: 1 kind:character
    duration: 0m
    recipes:
      mint -> SPAWN U
";
    let compiled = chorus::content::script::compile_script(text, &dict).unwrap();
    let (content, state) = (compiled.content, compiled.state);
    // the repro needs U at position 0 — the dead position must precede B
    let deck = content.decks.get("QUEST").unwrap();
    assert_eq!(deck.order.iter().map(|c| c.name.as_str()).collect::<Vec<_>>(), ["U", "B"]);

    // the analyzer's face: U obtainable, B unobtainable/None
    let u = analyze(&content, &state, "U");
    assert_eq!(u.verdict, CardVerdict::Obtainable);
    let b = analyze(&content, &state, "B");
    assert_eq!(b.verdict, CardVerdict::Unobtainable);
    assert_eq!(b.existence, chorus::reach::Existence::None);

    // the engine's face: mint, then the trigger's single draw serves B
    let base = clone_state(&state);
    let r1 = settle_line(&content, &dict, state, "0:0 @ ANA WAIT", 1).unwrap();
    assert_eq!(r1.rejected, None);
    let mut records = r1.records.clone();
    let cur = r1.state;
    assert_eq!(cur.manifested_uniques, IndexSet::from(["U".to_string()]));
    let id = records.len() as i64 + 1;
    let r2 = execute_step(
        &content,
        &dict,
        cur,
        Step {
            start_min: None,
            clock: Some(parse_clock("1:0").unwrap()),
            clauses: vec![],
            t_real: None,
        },
        ExecOptions { first_record_id: id, id_gen: None },
    )
    .unwrap();
    assert_eq!(r2.rejected, None);
    records.extend(r2.records.clone());
    let drawn: Vec<&str> = r2
        .records
        .iter()
        .flat_map(|rec| rec.outcomes.iter().filter(|o| o.op == "DRAWN"))
        .map(|o| match o.args.get(1) {
            Some(Arg::Ref { r#ref: Ref::Name { name, .. } }) => name.as_str(),
            _ => "(?)",
        })
        .collect();
    assert_eq!(drawn, ["B"], "the dead U position is consumed within the serving draw");
    assert_eq!(r2.state.decks.get("QUEST"), Some(&2));
    assert_eq!(r2.state.by_name.get("B").map(Vec::len), Some(1));
    // play == fold across the split composition
    let replayed = fold(&records, &base, &dict, &content).unwrap();
    assert_eq!(snap(&replayed), snap(&r2.state));
}

/// The unique NAMES set rides the compiled content (the §4.2 build).
#[test]
fn the_unique_sets_build_from_the_declared_prototypes_only() {
    let raw = ContentRaw {
        prototypes: Some(IndexMap::from([proto("A", "idea", true), proto("B", "idea", false)])),
        ..ContentRaw::default()
    };
    let content = compile(&raw);
    assert_eq!(content.unique_names, IndexSet::from(["A".to_string()]));
    // name-level: the kind-mate is not unique, and the kind carries nothing
    assert!(content.mint_is_unique("A"));
    assert!(!content.mint_is_unique("B"));
    assert!(!content.mint_is_unique("idea"));
}
