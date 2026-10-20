//! The reach analyzer's §4.2 uniquegroup faces (v0.10.1) — the five exact
//! unique faces lifted to groups:
//!
//! - a member serves from its FIRST order position only; every later
//!   occurrence of any member name is a dead position (the earliest
//!   serve-or-skip always manifests the group first);
//! - a member of a group in the initial state's manifested set (the midgame
//!   truth) never serves — a deck whose positions are all manifested-group
//!   members falls to the defaultcard face;
//! - a member minted IN PLAY by a proven SPAWN/REPLACE marks the GROUP
//!   (`minted_groups` — the engine's mint face records it permanently,
//!   whether or not the mint lands), so every member's deck positions are
//!   dead for the serving draws after it — the once-draw + in-play-mint
//!   composition agrees with play on both faces;
//! - the count-world clamps each GROUP at ONE living copy across all its
//!   members (the distinct-instance list mirrors), so a proof needing two
//!   living members — from one deck's later position, from deck + in-play
//!   mint, or across two members — decides False instead of over-proving;
//! - a repeat member mint of the same list is a SKIP, never a collision
//!   (`UniqueSkipped` — an earlier unconditional op minting any member of
//!   the group lands the one copy first).

use indexmap::{IndexMap, IndexSet};

use chorus::content::{
    compile_content, ContentRaw, DeckRaw, PrototypeDef, RecipeDef, SlotDef, VerbContent,
};
use chorus::core::dict::core_dictionary;
use chorus::core::types::{add_entity, clone_state, new_state, AspectVal, Entity, State};
use chorus::reach::fixpoint::{CardBlocker, SiteOp};
use chorus::reach::{analyze_card, Analyzer, CardOptions, CardVerdict, Existence};

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

fn proto(name: &str, kind: &str, group: Option<&str>) -> (String, PrototypeDef) {
    (
        name.to_string(),
        PrototypeDef {
            kind: kind.to_string(),
            unique_group: group.map(str::to_string),
            ..PrototypeDef::default()
        },
    )
}

fn gated_verb(name: &str, when: &str, out: &[&str]) -> (String, VerbContent) {
    (
        name.to_string(),
        VerbContent {
            slots: vec![SlotDef {
                role: "actors".into(),
                min: 1,
                max: Some(1),
                query: Some("kind:character".into()),
                shown: None,
                greedy: false,
                consuming: false,
            }],
            duration: 0,
            recipes: vec![RecipeDef {
                id: None,
                when: Some(when.to_string()),
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
                greedy: false,
                consuming: false,
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

/// The deck fixture: `QUEST spec A, B, C` — seed 0 is the identity
/// permutation, so the order reads as authored.
fn quest_deck(spec: &[&str]) -> DeckRaw {
    DeckRaw {
        id: "QUEST".into(),
        spec: spec.iter().map(|s| (*s).to_string()).collect(),
        ..Default::default()
    }
}

fn actors_slot() -> SlotDef {
    SlotDef {
        role: "actors".into(),
        min: 1,
        max: Some(1),
        query: Some("kind:character".into()),
        shown: None,
        greedy: false,
        consuming: false,
    }
}

#[test]
fn a_member_never_proves_twice_the_second_members_position_is_dead() {
    // the later-member face: deck [GA, B, GB] — GA's first position proves,
    // but a gate needing a SECOND living member (GB alongside GA) never
    // proves: the first member's serve — or skip — always manifests the
    // group before the cursor reaches GB, and an in-play GB mint skips
    // while GA lives. The one-member control gate proves.
    let raw = ContentRaw {
        prototypes: Some(IndexMap::from([
            proto("GA", "idea", Some("G1")),
            proto("GB", "idea", Some("G1")),
            proto("B", "idea", None),
            proto("TARGET", "item", None),
            proto("OK", "item", None),
        ])),
        decks: Some(vec![quest_deck(&["GA", "B", "GB"])]),
        verbs: IndexMap::<String, VerbContent>::from([
            wait_verb(&["DRAW QUEST"]),
            gated_verb("STUDY", "count(name:GA) >= 1 AND count(name:GB) >= 1", &["SPAWN TARGET"]),
            gated_verb("TALK", "count(name:GA) >= 1", &["SPAWN OK"]),
        ]),
        ..ContentRaw::default()
    };
    let content = compile(&raw);
    let state = ana_state();
    let ga = analyze(&content, &state, "GA");
    assert_eq!(ga.verdict, CardVerdict::Obtainable); // the first position proves
    let ok = analyze(&content, &state, "OK");
    assert_eq!(ok.verdict, CardVerdict::Obtainable); // one member suffices
    let target = analyze(&content, &state, "TARGET");
    assert_ne!(
        target.verdict,
        CardVerdict::Obtainable,
        "two living members are impossible at play — the analyzer must not over-prove them"
    );
}

#[test]
fn the_group_clamp_refuses_a_two_living_members_proof_across_sources() {
    // the clamp face: the ambient WORK spawns GA unconditionally and marks
    // the GROUP; the deck serves GB (its position is dead through the same
    // mark) — and even without the mark the pooled total clamps the GROUP
    // at ONE living copy across members, so the STUDY gate needing both
    // decides False over the clamped world, whatever the two sources' own
    // counts sum to. B (the deck's middle position) still proves.
    let raw = ContentRaw {
        prototypes: Some(IndexMap::from([
            proto("GA", "idea", Some("G1")),
            proto("GB", "idea", Some("G1")),
            proto("B", "idea", None),
            proto("TARGET", "item", None),
        ])),
        decks: Some(vec![quest_deck(&["GA", "B", "GB"])]),
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
                        out: vec!["SPAWN GA".to_string()],
                    }],
                    ambient: true,
                },
            ),
            gated_verb("STUDY", "count(name:GA) >= 1 AND count(name:GB) >= 1", &["SPAWN TARGET"]),
            wait_verb(&["DRAW QUEST"]),
        ]),
        ..ContentRaw::default()
    };
    let content = compile(&raw);
    let state = ana_state();
    let ga = analyze(&content, &state, "GA");
    assert_eq!(ga.verdict, CardVerdict::Obtainable);
    let b = analyze(&content, &state, "B");
    assert_eq!(b.verdict, CardVerdict::Obtainable); // the deck still serves B
    let target = analyze(&content, &state, "TARGET");
    assert_ne!(
        target.verdict,
        CardVerdict::Obtainable,
        "the count clamp composes the ambient mint + deck to ONE living member, never two"
    );
}

// ------------------------------------------------ the mint marks the group ---

#[test]
fn the_once_draw_plus_in_play_member_mint_composition_agrees_the_draw_skips_the_dead_member() {
    // the group face of minted_manifested: a PROVEN in-play mint of GB
    // marks the GROUP manifested, so GA's deck position is dead for the
    // serving draws after it. Repro shape: deck QUEST spec GA, B; a `once`
    // slot trigger drawing QUEST; a verb minting GB. At play the mint lands
    // first — the group manifests — and the trigger's single draw then
    // skips the dead GA position and serves B. The analyzer proves GB
    // through the mint, marks the group, reads GA's position dead through
    // the same mark, and proves B through the draw's remaining serve
    // budget (with no group the single serve takes position 0 and B would
    // NOT serve — the marked-position arithmetic is what moves the serve).
    let dict = core_dictionary();
    let text = "\
script: Once Draw Agreement
player: ANA
start: 0:0
seed: 0

entities:
  ANA is character at HOUSE
  HOUSE is place

prototypes:
  GA is idea uniquegroup G1
  GB is idea uniquegroup G1
  B is idea stackable

decks:
  QUEST spec GA, B

triggers:
  grant once at hour 1 -> DRAW QUEST

verbs:
  WAIT:
    actors: 1 kind:character
    duration: 0m
    recipes:
      mint -> SPAWN GB
";
    let compiled = chorus::content::script::compile_script(text, &dict).unwrap();
    let (content, state) = (compiled.content, compiled.state);
    // the repro needs GA at position 0 — the dead position must precede B
    let deck = content.decks.get("QUEST").unwrap();
    assert_eq!(deck.order.iter().map(|c| c.name.as_str()).collect::<Vec<_>>(), ["GA", "B"]);

    // the analyzer's face: GB obtainable (the mint), B obtainable (the
    // marked group's position is dead — the draw's serve budget skips it)
    let gb = analyze(&content, &state, "GB");
    assert_eq!(gb.verdict, CardVerdict::Obtainable);
    let b = analyze(&content, &state, "B");
    assert_eq!(b.verdict, CardVerdict::Obtainable);
    assert_eq!(b.existence, chorus::reach::Existence::Proven);

    // the engine's face: mint, then the trigger's single draw serves B —
    // the group manifested at the mint, GA's position skipped at no cost
    let base = clone_state(&state);
    let r1 = crate::common::rig::settle_line(&content, &dict, state, "0:0 @ ANA WAIT", 1).unwrap();
    assert_eq!(r1.rejected, None);
    let mut records = r1.records.clone();
    let cur = r1.state;
    assert_eq!(cur.manifested_groups, IndexSet::from(["G1".to_string()]));
    let id = records.len() as i64 + 1;
    let r2 = chorus::engine::execute_step(
        &content,
        &dict,
        cur,
        chorus::engine::Step {
            start_min: None,
            clock: Some(chorus::core::clock::parse_clock("1:0").unwrap()),
            clauses: vec![],
            t_real: None,
        },
        chorus::engine::ExecOptions { first_record_id: id, id_gen: None },
    )
    .unwrap();
    assert_eq!(r2.rejected, None);
    records.extend(r2.records.clone());
    let drawn: Vec<&str> = r2
        .records
        .iter()
        .flat_map(|rec| rec.outcomes.iter().filter(|o| o.op == "DRAWN"))
        .map(|o| match o.args.get(1) {
            Some(chorus::core::sentence::Arg::Ref {
                r#ref: chorus::core::r#ref::Ref::Name { name, .. },
            }) => name.as_str(),
            _ => "(?)",
        })
        .collect();
    assert_eq!(drawn, ["B"], "the dead member position is consumed within the serving draw");
    assert_eq!(r2.state.decks.get("QUEST"), Some(&2));
    assert_eq!(r2.state.by_name.get("B").map(Vec::len), Some(1));
    // play == fold across the composition
    let replayed = chorus::engine::fold::fold(&records, &base, &dict, &content).unwrap();
    assert_eq!(crate::common::snap::snap(&replayed), crate::common::snap::snap(&r2.state));
}

#[test]
fn a_member_of_an_initially_manifested_group_never_serves_the_deck_falls_to_the_defaultcard() {
    // the midgame face: the initial state's manifested GROUPS are the truth
    // — TOMB's GA position (a one-member deck here) is dead, the defaultcard
    // face serves, and the member itself is no longer obtainable by draw
    let raw = ContentRaw {
        prototypes: Some(IndexMap::from([
            proto("GA", "idea", Some("G1")),
            proto("GB", "idea", Some("G1")),
            proto("D", "idea", None),
        ])),
        decks: Some(vec![DeckRaw {
            id: "TOMB".into(),
            spec: vec!["GA".into()],
            defaultcard: Some("D".into()),
            ..Default::default()
        }]),
        verbs: IndexMap::<String, VerbContent>::from([wait_verb(&["DRAW TOMB"])]),
        ..ContentRaw::default()
    };
    let content = compile(&raw);
    let mut state = ana_state();
    state.manifested_groups.insert("G1".to_string());
    let d = analyze(&content, &state, "D");
    assert_eq!(d.verdict, CardVerdict::Obtainable); // the defaultcard face
    let ga = analyze(&content, &state, "GA");
    assert_ne!(ga.verdict, CardVerdict::Obtainable, "a manifested member's deck position is dead");
    // the control: the same content with an EMPTY group set proves GA
    let fresh = analyze(&content, &ana_state(), "GA");
    assert_eq!(fresh.verdict, CardVerdict::Obtainable);
}

// ----------------------------------------------- the skip-not-collision face -

#[test]
fn a_repeat_member_mint_of_one_list_skips_and_never_collides() {
    // the census face: one list minting GA then GB — the earlier op lands
    // the one copy first (GA), so GB's own site never mints
    // (`UniqueSkipped` — the skip replaces the §9.2 name-in-use rejection
    // for members exactly as for uniques). TARGET, gated on the sibling's
    // existence and sourced only through that second site, never proves;
    // the GA-gated control proves.
    let raw = ContentRaw {
        prototypes: Some(IndexMap::from([
            proto("GA", "idea", Some("G1")),
            proto("GB", "idea", Some("G1")),
            proto("TARGET", "item", None),
            proto("OK", "item", None),
        ])),
        verbs: IndexMap::<String, VerbContent>::from([
            wait_verb(&["SPAWN GA", "SPAWN GB"]),
            gated_verb("STUDY", "exists(GB)", &["SPAWN TARGET"]),
            gated_verb("TALK", "exists(GA)", &["SPAWN OK"]),
        ]),
        ..ContentRaw::default()
    };
    let content = compile(&raw);
    let state = ana_state();
    let ga = analyze(&content, &state, "GA");
    assert_eq!(ga.verdict, CardVerdict::Obtainable); // the earlier op proves
    let ok = analyze(&content, &state, "OK");
    assert_eq!(ok.verdict, CardVerdict::Obtainable);
    let target = analyze(&content, &state, "TARGET");
    assert_ne!(
        target.verdict,
        CardVerdict::Obtainable,
        "the sibling site's mint is a skip, not a collision — its product never exists"
    );
}

#[test]
fn the_serving_predicate_is_the_one_home_for_the_group_faces() {
    // the serving predicate's disjuncts side by side: deck [GA, B, GB] —
    // with a proven in-play member mint (marking the group) GA's position
    // dies too, while the untouched B keeps its own; without the mint GA's
    // position proves and GB's NEVER does (the later member occurrence is
    // dead in every execution — the earliest serve-or-skip manifests the
    // group first). The SAME predicate serves the card, graph and demand
    // faces (export reads deck_serves_cap), so a dead position's graph
    // edge never emits either.
    let proto_rows = || {
        IndexMap::from([
            proto("GA", "idea", Some("G1")),
            proto("GB", "idea", Some("G1")),
            proto("B", "idea", None),
        ])
    };
    // with the mint: GB proves through STUDY, and the mark kills BOTH
    // member positions — GA is no longer obtainable by draw (its proof
    // was its own position; the minted group's positions are dead), while
    // B (its position preceding... behind one live serve) needs the
    // second draw the capped WAIT cannot fund — pinned via GA's loss, the
    // sharpest observable of the mark
    let raw = ContentRaw {
        prototypes: Some(proto_rows()),
        decks: Some(vec![quest_deck(&["GA", "B", "GB"])]),
        verbs: IndexMap::<String, VerbContent>::from([
            wait_verb(&["DRAW QUEST"]),
            (
                "STUDY".to_string(),
                VerbContent {
                    slots: vec![actors_slot()],
                    duration: 0,
                    recipes: vec![RecipeDef {
                        id: None,
                        when: None,
                        shown: None,
                        hint: None,
                        max: None,
                        aspects: Vec::new(),
                        dur: None,
                        out: vec!["SPAWN GB".to_string()],
                    }],
                    ambient: false,
                },
            ),
        ]),
        ..ContentRaw::default()
    };
    let content = compile(&raw);
    let state = ana_state();
    let gb = analyze(&content, &state, "GB");
    assert_eq!(gb.verdict, CardVerdict::Obtainable); // the mint proves and marks the group
    let ga = analyze(&content, &state, "GA");
    assert_ne!(
        ga.verdict,
        CardVerdict::Obtainable,
        "the in-play member mint marks the group — its own deck position dies with it"
    );
    // without the mint: GB proves through its FIRST position only — the
    // second GB position is a later member occurrence, dead in every
    // execution (the first serve-or-skip always manifests the group
    // first), so a proof needing a SECOND living GB never over-proves
    let raw2 = ContentRaw {
        prototypes: Some(IndexMap::from([
            proto("GB", "idea", Some("G1")),
            proto("B", "idea", None),
            proto("TARGET", "item", None),
        ])),
        decks: Some(vec![quest_deck(&["GB", "B", "GB"])]),
        verbs: IndexMap::<String, VerbContent>::from([
            wait_verb(&["DRAW QUEST"]),
            gated_verb("TALK", "count(name:GB) >= 2", &["SPAWN TARGET"]),
        ]),
        ..ContentRaw::default()
    };
    let content2 = compile(&raw2);
    let state2 = ana_state();
    let gb2 = analyze(&content2, &state2, "GB");
    assert_eq!(gb2.verdict, CardVerdict::Obtainable);
    let target2 = analyze(&content2, &state2, "TARGET");
    assert_ne!(
        target2.verdict,
        CardVerdict::Obtainable,
        "the later member occurrence never serves in any execution"
    );
}

// ------------------------------------------------- the group-block face -----

#[test]
fn the_group_block_demotes_a_landing_an_engine_timed_sibling_blocks_and_the_player_outruns() {
    // §4.2 v0.10.1's GroupMemberBlocked face: the ambient WORK
    // unconditionally mints the sibling P1 — the sweep's own landing, at
    // the sweep boundary plus the window, unoutrunnable — so the player
    // STUDY's SPAWN P2 SKIPS while P1 lives (the group living-check), and
    // the analyzer demotes the site: TARGET, gated on P2's existence, never
    // proves. Half two pins the outrun arm: the same P2 site with the
    // sibling minted only by a LATE slot trigger stays proven — the player
    // commits at will, before the sibling's first crossing.
    let protos = || {
        IndexMap::from([
            proto("P1", "idea", Some("G1")),
            proto("P2", "item", Some("G1")),
            proto("TARGET", "item", None),
        ])
    };
    let ambient_sibling = ContentRaw {
        prototypes: Some(protos()),
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
                        out: vec!["SPAWN P1".to_string()],
                    }],
                    ambient: true,
                },
            ),
            wait_verb(&["SPAWN P2"]),
            gated_verb("TALK", "exists(P2)", &["SPAWN TARGET"]),
        ]),
        ..ContentRaw::default()
    };
    let content = compile(&ambient_sibling);
    let state = ana_state();
    let p1 = analyze(&content, &state, "P1");
    assert_eq!(p1.verdict, CardVerdict::Obtainable); // the ambient sibling proves
    let target = analyze(&content, &state, "TARGET");
    assert_ne!(
        target.verdict,
        CardVerdict::Obtainable,
        "P2's landing skips while the engine-timed sibling lives — the site must demote"
    );
    // half two: the sibling rides a LATE slot trigger (hour 23) — the
    // player's instant commit outruns its first crossing, so the same P2
    // site proves and TARGET with it
    let late_trigger = ContentRaw {
        prototypes: Some(protos()),
        triggers: Some(vec![chorus::content::TriggerDef::Slot(chorus::content::SlotTriggerDef {
            id: "late".to_string(),
            slot: 138,
            once: Some(false),
            out: vec!["SPAWN P1".to_string()],
        })]),
        verbs: IndexMap::<String, VerbContent>::from([
            wait_verb(&["SPAWN P2"]),
            gated_verb("TALK", "exists(P2)", &["SPAWN TARGET"]),
        ]),
        ..ContentRaw::default()
    };
    let content2 = compile(&late_trigger);
    let state2 = ana_state();
    let p2 = analyze(&content2, &state2, "P2");
    assert_eq!(p2.verdict, CardVerdict::Obtainable);
    let target2 = analyze(&content2, &state2, "TARGET");
    assert_eq!(
        target2.verdict,
        CardVerdict::Obtainable,
        "the player outruns the late sibling's first crossing — no demotion"
    );
}

#[test]
fn an_initial_living_member_blocks_the_sibling_members_mint_site() {
    // the midgame face of the group block: the analysis state is a REPLAYED
    // world whose initial entities carry a living GA (the log minted it —
    // the fold rebuilt G1's manifested set from that mint row) and no verb
    // destroys GA, so the group is living from minute zero and MINT2's
    // SPAWN GB hits the §4.2 group living-check on EVERY future
    // application — the engine skips it forever, and GB stays unproven at
    // the GroupMemberBlocked rung. The GA-gated control proves: the initial
    // member feeds gates normally, only the sibling MINT is blocked.
    let dict = core_dictionary();
    let text = "\
script: Initial Member Block
player: ANA
start: 0:0
seed: 0

entities:
  ANA is character

prototypes:
  GA is idea uniquegroup G1
  GB is idea uniquegroup G1
  TARGET is item
  OK is item

verbs:
  MINT2:
    actors: 1 kind:character
    duration: 0m
    recipes:
      mint2 -> SPAWN GB
  TALK:
    actors: 1 kind:character
    duration: 0m
    recipes:
      talk when exists(GA) -> SPAWN OK
";
    let compiled = chorus::content::script::compile_script(text, &dict).unwrap();
    let (content, base) = (compiled.content, compiled.state);
    // the replayed midgame world: GA alive beside ANA, G1 manifested
    let mut state = clone_state(&base);
    add_entity(
        &mut state,
        Entity {
            id: "s_ga".into(),
            kind: "idea".into(),
            name: "GA".into(),
            aspects: IndexMap::new(),
            attrs: IndexMap::new(),
            born_action: Some(0),
            dead_action: None,
            stackable: false,
            desc: None,
        },
    )
    .unwrap();
    state.manifested_groups.insert("G1".to_string());
    let ok = analyze(&content, &state, "OK");
    assert_eq!(ok.verdict, CardVerdict::Obtainable); // the initial member feeds gates
    let gb = analyze(&content, &state, "GB");
    assert_eq!(gb.verdict, CardVerdict::Unproven);
    assert_eq!(gb.existence, Existence::Candidate);
    // the --why table: MINT2's mint site demotes at the GroupMemberBlocked
    // rung — the initial sibling is unorderable (minute zero)
    let analyzer = Analyzer::new(&content, &state, &dict);
    let rows = analyzer.card_blockers("GB");
    assert_eq!(rows.len(), 1, "exactly MINT2's SPAWN GB site serves GB");
    assert_eq!(rows[0].owner, "recipe mint2 of MINT2");
    assert_eq!(rows[0].op, SiteOp::Spawn);
    assert_eq!(rows[0].name, "GB");
    assert_eq!(rows[0].blocker, CardBlocker::GroupMemberBlocked);
}
