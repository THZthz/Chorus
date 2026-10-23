//! The canonical state key (story-analyze spec §5.3) and the symmetry
//! classes it orders by (§5.2): inductive carrier-tree signatures,
//! contiguity partitioning, relation re-indexing under the canonical entity
//! numbering, watermark stripping, and the include list.

use std::collections::BTreeMap;

use story_analyze::canonical::CanonicalStateKey;
use story_analyze::symmetry::symmetry_classes;
use story_content::ir::{
    CardDecl, RecipeDecl, RelationKindDecl, SlotDecl, VerbDecl, VerbMode, InstanceCap,
};
use story_content::Content;
use story_core::{
    Binding, CardId, DeckId, EndingId, Entity, EntityId, MintPolicy, RecipeId, Situation,
    SituationId, SituationSource, SlotName, State, TriggerRuleId, VerbId, Zone, ZoneId, ZoneKind,
};

// ---------------------------------------------------------------------------
// Builders
// ---------------------------------------------------------------------------

fn card_decl(name: &str, charge: i64) -> CardDecl {
    let mut aspects = BTreeMap::new();
    aspects.insert("charge".to_string(), charge);
    CardDecl {
        id: CardId::from(name),
        kind: Some("gadget".to_string()),
        mint: MintPolicy::Stackable,
        aspects,
        lifetime: None,
        boot: None,
        relates: Vec::new(),
        desc: None,
        mint_alias: None,
        on_complete: Vec::new(),
        on_expire: None,
        meta: BTreeMap::new(),
        decl_index: 0,
    }
}

/// A content with the relation kinds and one candidacy surface: a slot whose
/// filter reads `aspect:charge >= 1`.
fn content(beta_charge: i64) -> Content {
    let mut c = Content::default();
    for (name, symmetric) in [("knows", true), ("owns", false)] {
        c.relation_kinds.insert(
            name.to_string(),
            RelationKindDecl {
                name: name.to_string(),
                symmetric,
                exclusive: false,
                meta: BTreeMap::new(),
                decl_index: 0,
            },
        );
    }
    c.cards.insert(CardId::from("alpha"), card_decl("alpha", 2));
    c.cards.insert(CardId::from("zeta"), card_decl("zeta", 1));
    c.cards.insert(CardId::from("beta"), card_decl("beta", beta_charge));
    c.cards.insert(CardId::from("gem"), card_decl("gem", 0));
    c.cards.insert(CardId::from("box"), card_decl("box", 0));

    let mut aspects = BTreeMap::new();
    aspects.insert("charge".to_string(), 1);
    let slot = SlotDecl {
        name: SlotName::from("any"),
        min: 0,
        max: Some(1),
        greedy: false,
        consuming: false,
        open: false,
        take: false,
        query: vec![story_content::ir::QueryTerm::Aspect(
            story_content::ir::AspectTerm {
                key: "charge".into(),
                op: None,
                value: 1,
            },
        )],
        decl_index: 0,
    };
    let mut recipes = BTreeMap::new();
    recipes.insert(
        "act".to_string(),
        RecipeDecl {
            verb: VerbId::from("use"),
            name: "act".into(),
            id: RecipeId::from("use.act"),
            label: None,
            priority: 100,
            craftable: true,
            chain_only: false,
            max: None,
            duration: 0,
            declares: Vec::new(),
            when: None,
            guards: Vec::new(),
            slots: vec![slot],
            effects: Vec::new(),
            on_complete: None,
            allow_shadow: false,
            meta: BTreeMap::new(),
            decl_index: 0,
        },
    );
    c.verbs.insert(
        "use".to_string(),
        VerbDecl {
            id: VerbId::from("use"),
            mode: VerbMode::Committable,
            instances: InstanceCap::Any,
            title: None,
            releases: None,
            recipes,
            recipe_order: vec!["act".into()],
            meta: BTreeMap::new(),
            decl_index: 0,
        },
    );
    c
}

fn ent(id: u64, proto: &str, zone: u64, charge: i64) -> Entity {
    let mut aspects = BTreeMap::new();
    aspects.insert("charge".to_string(), charge);
    Entity {
        id: EntityId(id),
        name: proto.to_string(),
        prototype: CardId::from(proto),
        kind: Some("gadget".to_string()),
        aspects,
        attrs: BTreeMap::new(),
        zone: ZoneId(zone),
        mint: MintPolicy::Stackable,
        expires_at: None,
    }
}

fn table(id: u64) -> Zone {
    Zone {
        id: ZoneId(id),
        kind: ZoneKind::Table,
        public: true,
        bindable: true,
    }
}

fn interior(id: u64, sit: u64) -> Zone {
    Zone {
        id: ZoneId(id),
        kind: ZoneKind::SituationInterior(SituationId(sit)),
        public: false,
        bindable: false,
    }
}

fn carrier(id: u64, host: u64) -> Zone {
    Zone {
        id: ZoneId(id),
        kind: ZoneKind::Of(EntityId(host)),
        public: true,
        bindable: true,
    }
}

fn sit(id: u64, interior_zone: u64, bound: u64) -> Situation {
    let mut bindings = BTreeMap::new();
    bindings.insert(
        SlotName::from("any"),
        vec![Binding {
            entity: EntityId(bound),
            bound_from: ZoneId(1),
            seq: 0,
        }],
    );
    Situation {
        id: SituationId(id),
        verb: VerbId::from("use"),
        source: SituationSource::Commit,
        recipe: RecipeId::from("use.act"),
        interior: ZoneId(interior_zone),
        bindings,
        started_at: 0,
        ends_at: 5,
    }
}

fn base_state() -> State {
    let mut s = State::default();
    s.zones.insert(ZoneId(1), table(1));
    s.entities.insert(EntityId(1), ent(1, "alpha", 1, 2));
    s.entities.insert(EntityId(2), ent(2, "zeta", 1, 1));
    s
}

// ---------------------------------------------------------------------------
// Symmetry classes (§5.2)
// ---------------------------------------------------------------------------

#[test]
fn identical_batch_cards_form_one_class() {
    let mut s = base_state();
    s.entities.insert(EntityId(2), ent(2, "alpha", 1, 2));
    s.entities.insert(EntityId(3), ent(3, "alpha", 1, 2));
    let classes = symmetry_classes(&s, &content(1));
    assert_eq!(classes, vec![vec![EntityId(1), EntityId(2), EntityId(3)]]);
}

#[test]
fn interleaved_foreign_card_sharing_candidacy_splits_the_class() {
    // beta carries charge, so it shares the slot-filter candidacy surface
    // with the alphas: the interleaved card breaks the batch apart.
    let mut s = base_state();
    s.entities.insert(EntityId(2), ent(2, "beta", 1, 1));
    s.entities.insert(EntityId(3), ent(3, "alpha", 1, 2));
    let classes = symmetry_classes(&s, &content(1));
    assert_eq!(
        classes,
        vec![
            vec![EntityId(1)],
            vec![EntityId(3)],
            vec![EntityId(2)],
        ]
    );
}

#[test]
fn interleaved_foreign_card_without_candidacy_keeps_the_batch() {
    // beta reads charge 0 and no effect can write the key on it, so it does
    // not share candidacy with the alphas and cannot break the tie.
    let mut s = base_state();
    s.entities.insert(EntityId(2), ent(2, "beta", 1, 0));
    s.entities.insert(EntityId(3), ent(3, "alpha", 1, 2));
    let classes = symmetry_classes(&s, &content(0));
    assert_eq!(classes, vec![vec![EntityId(1), EntityId(3)], vec![EntityId(2)]]);
}

#[test]
fn carrier_contents_enter_the_signature_inductively() {
    // Two boxes on the table, one gem in each: the boxes are interchangeable
    // (their contents multisets agree) and so are the gems.
    let mut s = State::default();
    s.zones.insert(ZoneId(1), table(1));
    s.zones.insert(ZoneId(2), carrier(2, 2));
    s.zones.insert(ZoneId(3), carrier(3, 3));
    s.entities.insert(EntityId(2), ent(2, "box", 1, 0));
    s.entities.insert(EntityId(3), ent(3, "box", 1, 0));
    s.entities.insert(EntityId(4), ent(4, "gem", 2, 0));
    s.entities.insert(EntityId(5), ent(5, "gem", 3, 0));
    let classes = symmetry_classes(&s, &content(0));
    let names: Vec<Vec<&str>> = classes
        .iter()
        .map(|c| {
            c.iter()
                .map(|e| s.entities[e].name.as_str())
                .collect::<Vec<_>>()
        })
        .collect();
    assert_eq!(names, vec![vec!["box", "box"], vec!["gem", "gem"]]);

    // A box that carries nothing is a different entity from its loaded twin,
    // even with identical fields.
    let mut s = State::default();
    s.zones.insert(ZoneId(1), table(1));
    s.zones.insert(ZoneId(2), carrier(2, 2));
    s.entities.insert(EntityId(2), ent(2, "box", 1, 0));
    s.entities.insert(EntityId(3), ent(3, "box", 1, 0));
    s.entities.insert(EntityId(4), ent(4, "gem", 2, 0));
    let classes = symmetry_classes(&s, &content(0));
    let box_classes: Vec<Vec<u64>> = classes
        .iter()
        .filter(|c| s.entities[&c[0]].name == "box")
        .map(|c| c.iter().map(|e| e.0).collect())
        .collect();
    assert_eq!(
        box_classes.iter().map(|c| c.len()).collect::<Vec<_>>(),
        vec![1, 1],
        "the loaded box and the empty twin split apart"
    );
}

// ---------------------------------------------------------------------------
// The canonical key (§5.3)
// ---------------------------------------------------------------------------

#[test]
fn permuting_situations_over_interchangeable_cards_collides() {
    // Two symmetric situations; which interchangeable card sits in which one
    // is the same logical state.
    let build = |swap: bool| {
        let mut s = State::default();
        s.zones.insert(ZoneId(1), table(1));
        s.zones.insert(ZoneId(2), interior(2, 1));
        s.zones.insert(ZoneId(3), interior(3, 2));
        let (first, second) = if swap { (2u64, 1u64) } else { (1, 2) };
        s.entities.insert(EntityId(1), ent(1, "alpha", 1, 2));
        s.entities.insert(EntityId(2), ent(2, "alpha", 1, 2));
        if swap {
            s.entities.get_mut(&EntityId(1)).unwrap().zone = ZoneId(3);
            s.entities.get_mut(&EntityId(2)).unwrap().zone = ZoneId(2);
        } else {
            s.entities.get_mut(&EntityId(1)).unwrap().zone = ZoneId(2);
            s.entities.get_mut(&EntityId(2)).unwrap().zone = ZoneId(3);
        }
        s.situations
            .insert(SituationId(1), sit(1, 2, first));
        s.situations
            .insert(SituationId(2), sit(2, 3, second));
        s
    };
    assert_eq!(
        CanonicalStateKey::of(&build(false), &content(0)),
        CanonicalStateKey::of(&build(true), &content(0))
    );
}

#[test]
fn allocator_watermarks_never_reach_the_key() {
    let mut s = base_state();
    let key = CanonicalStateKey::of(&s, &content(0));
    s.next_entity = 999;
    s.next_situation = 888;
    s.next_zone = 777;
    assert_eq!(key, CanonicalStateKey::of(&s, &content(0)));
}

#[test]
fn the_source_tag_is_dropped() {
    let mut s = base_state();
    s.situations.insert(SituationId(1), sit(1, 2, 2));
    let key = CanonicalStateKey::of(&s, &content(0));
    s.situations.get_mut(&SituationId(1)).unwrap().source = SituationSource::Autonomous;
    assert_eq!(key, CanonicalStateKey::of(&s, &content(0)));
    s.situations.get_mut(&SituationId(1)).unwrap().source =
        SituationSource::Start { parent: SituationId(1) };
    assert_eq!(key, CanonicalStateKey::of(&s, &content(0)));
}

#[test]
fn relations_reindex_when_the_symmetry_order_flips() {
    // A symmetric row is stored under the mint-order subject; when the
    // canonical entity numbering flips the endpoints' order, the row
    // re-canonicalizes under the new smaller endpoint and both states
    // collide.
    let build = |flip: bool| {
        let mut s = State::default();
        s.zones.insert(ZoneId(1), table(1));
        if flip {
            s.entities.insert(EntityId(1), ent(1, "zeta", 1, 1));
            s.entities.insert(EntityId(2), ent(2, "alpha", 1, 2));
        } else {
            s.entities.insert(EntityId(1), ent(1, "alpha", 1, 2));
            s.entities.insert(EntityId(2), ent(2, "zeta", 1, 1));
        }
        // Both raw rows are stored under the canonical subject 1.
        s.relations
            .entry(EntityId(1))
            .or_default()
            .entry("knows".to_string())
            .or_default()
            .insert(EntityId(2));
        s
    };
    assert_eq!(
        CanonicalStateKey::of(&build(false), &content(0)),
        CanonicalStateKey::of(&build(true), &content(0))
    );
}

#[test]
fn directed_relations_keep_their_orientation() {
    let build = |reverse: bool| {
        let mut s = base_state();
        let (from, to) = if reverse {
            (EntityId(2), EntityId(1))
        } else {
            (EntityId(1), EntityId(2))
        };
        s.relations
            .entry(from)
            .or_default()
            .entry("owns".to_string())
            .or_default()
            .insert(to);
        s
    };
    assert_ne!(
        CanonicalStateKey::of(&build(false), &content(0)),
        CanonicalStateKey::of(&build(true), &content(0))
    );
}

/// One state mutation under a field label.
type Mutation<'a> = (&'a str, Box<dyn Fn(&mut State)>);

#[test]
fn include_list_fields_all_reach_the_key() {
    let mutations: Vec<Mutation<'_>> = vec![
        ("now", Box::new(|s: &mut State| s.now = 9)),
        ("rng_counter", Box::new(|s: &mut State| s.rng_counter = 9)),
        (
            "decks",
            Box::new(|s: &mut State| {
                s.decks.insert(DeckId::from("pile"), 3);
            }),
        ),
        (
            "recipe_runs",
            Box::new(|s: &mut State| {
                s.recipe_runs.insert(RecipeId::from("use.act"), 1);
            }),
        ),
        (
            "fired_once",
            Box::new(|s: &mut State| {
                s.fired_once.insert(TriggerRuleId::from("use.act.r"));
            }),
        ),
        (
            "legacy",
            Box::new(|s: &mut State| {
                s.legacy.insert("dawns".to_string(), 4);
            }),
        ),
        (
            "ended",
            Box::new(|s: &mut State| s.ended = Some(EndingId::from("lit"))),
        ),
        (
            "binding_seq",
            Box::new(|s: &mut State| {
                s.situations.insert(SituationId(1), sit(1, 2, 2));
                s.situations
                    .get_mut(&SituationId(1))
                    .unwrap()
                    .bindings
                    .get_mut(&SlotName::from("any"))
                    .unwrap()[0]
                    .seq = 7;
            }),
        ),
    ];
    for (name, mutate) in &mutations {
        let mut s = base_state();
        let key = CanonicalStateKey::of(&s, &content(0));
        mutate(&mut s);
        assert_ne!(
            key,
            CanonicalStateKey::of(&s, &content(0)),
            "{name} must reach the key"
        );
    }
}

#[test]
fn zone_policy_bits_reach_the_key() {
    let mut s = base_state();
    s.zones.insert(ZoneId(5), table(5));
    s.entities.insert(EntityId(3), ent(3, "alpha", 5, 2));
    let key = CanonicalStateKey::of(&s, &content(0));
    s.zones.get_mut(&ZoneId(5)).unwrap().bindable = false;
    assert_ne!(key, CanonicalStateKey::of(&s, &content(0)));
}
