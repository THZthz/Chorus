use std::collections::{BTreeMap, BTreeSet};
use story_content::ir::{AspectDecl, AspectKey, CardDecl};
use story_content::Content;
use story_core::{CardId, Entity, EntityId, MintPolicy, SlotName, State, ZoneId};
use story_present::{interpolate, PresentationCatalog, PresentationContext};

fn sample_fixture() -> (State, Content, PresentationCatalog) {
    let mut state = State::default();
    state.now = 10;
    state.legacy.insert("sanity".to_string(), 42);
    state.legacy.insert("gold".to_string(), 100);

    let mut content = Content::default();

    // Aspects
    content.aspects.insert(
        AspectKey::from("lore"),
        AspectDecl {
            key: AspectKey::from("lore"),
            default: 0,
            meta: Default::default(),
        },
    );

    // Cards
    let tome_decl = CardDecl {
        id: CardId::from("grimoire"),
        kind: Some("book".to_string()),
        mint: MintPolicy::SingleCopy,
        aspects: BTreeMap::from([("lore".to_string(), 5)]),
        lifetime: None,
        boot: None,
        relates: Vec::new(),
        desc: Some("An ancient text".to_string()),
        mint_alias: None,
        on_complete: Vec::new(),
        on_expire: None,
        meta: BTreeMap::from([("title".to_string(), "Ancient Grimoire".to_string())]),
        decl_index: 0,
    };
    content.cards.insert(CardId::from("grimoire"), tome_decl);

    let scroll_decl = CardDecl {
        id: CardId::from("scroll"),
        kind: Some("parchment".to_string()),
        mint: MintPolicy::SingleCopy,
        aspects: BTreeMap::from([("lore".to_string(), 2)]),
        lifetime: None,
        boot: None,
        relates: Vec::new(),
        desc: None,
        mint_alias: None,
        on_complete: Vec::new(),
        on_expire: None,
        meta: Default::default(),
        decl_index: 1,
    };
    content.cards.insert(CardId::from("scroll"), scroll_decl);

    let scholar_decl = CardDecl {
        id: CardId::from("scholar"),
        kind: Some("person".to_string()),
        mint: MintPolicy::SingleCopy,
        aspects: BTreeMap::from([("lore".to_string(), 7)]),
        lifetime: None,
        boot: None,
        relates: Vec::new(),
        desc: None,
        mint_alias: None,
        on_complete: Vec::new(),
        on_expire: None,
        meta: Default::default(),
        decl_index: 2,
    };
    content.cards.insert(CardId::from("scholar"), scholar_decl);

    let patron_decl = CardDecl {
        id: CardId::from("patron"),
        kind: Some("person".to_string()),
        mint: MintPolicy::SingleCopy,
        aspects: BTreeMap::new(),
        lifetime: None,
        boot: None,
        relates: Vec::new(),
        desc: None,
        mint_alias: None,
        on_complete: Vec::new(),
        on_expire: None,
        meta: Default::default(),
        decl_index: 3,
    };
    content.cards.insert(CardId::from("patron"), patron_decl);

    // Entities in state
    let scholar_ent = Entity {
        id: EntityId(1),
        name: "scholar".to_string(),
        prototype: CardId::from("scholar"),
        kind: Some("person".to_string()),
        aspects: BTreeMap::from([("lore".to_string(), 8)]),
        attrs: Default::default(),
        zone: ZoneId(1),
        mint: MintPolicy::SingleCopy,
        expires_at: None,
    };
    state.entities.insert(scholar_ent.id, scholar_ent);

    let patron_ent = Entity {
        id: EntityId(2),
        name: "patron".to_string(),
        prototype: CardId::from("patron"),
        kind: Some("person".to_string()),
        aspects: BTreeMap::new(),
        attrs: Default::default(),
        zone: ZoneId(1),
        mint: MintPolicy::SingleCopy,
        expires_at: None,
    };
    state.entities.insert(patron_ent.id, patron_ent);

    // Relations: scholar has patron -> patron_ent
    let mut scholar_rels = BTreeMap::new();
    let mut patrons_set = BTreeSet::new();
    patrons_set.insert(EntityId(2));
    scholar_rels.insert("patron".to_string(), patrons_set);
    state.relations.insert(EntityId(1), scholar_rels);

    // Presentation Catalog
    let mut catalog = PresentationCatalog::default();
    catalog.cards.insert(
        "scholar".to_string(),
        story_present::catalog::CardPresentation {
            title: Some("Master Scholar".to_string()),
            description: Some("A learned master".to_string()),
        },
    );
    catalog.cards.insert(
        "patron".to_string(),
        story_present::catalog::CardPresentation {
            title: Some("Wealthy Benefactor".to_string()),
            description: None,
        },
    );

    (state, content, catalog)
}

#[test]
fn test_slot_resolving_bound_card_title() {
    let (state, content, catalog) = sample_fixture();
    let mut ctx = PresentationContext::default();
    ctx.bound_cards.insert(
        SlotName::from("study_material"),
        vec![CardId::from("grimoire")],
    );

    // "Ancient Grimoire" comes from card DSL meta
    let res = interpolate(
        "Studying {study_material}",
        &state,
        &content,
        &catalog,
        &ctx,
    );
    assert_eq!(res, "Studying Ancient Grimoire");

    // Multiple cards bound to slot
    ctx.bound_cards.insert(
        SlotName::from("study_material"),
        vec![CardId::from("grimoire"), CardId::from("scroll")],
    );
    let res = interpolate("Reading {study_material}", &state, &content, &catalog, &ctx);
    assert_eq!(res, "Reading Ancient Grimoire, scroll");
}

#[test]
fn test_legacy_key_resolving() {
    let (state, content, catalog) = sample_fixture();
    let ctx = PresentationContext::default();

    let res = interpolate(
        "Sanity: {legacy.sanity}, Gold: {legacy.gold}, Dread: {legacy.dread}",
        &state,
        &content,
        &catalog,
        &ctx,
    );
    // Known keys resolve to values, absent legacy key resolves to "0"
    assert_eq!(res, "Sanity: 42, Gold: 100, Dread: 0");
}

#[test]
fn test_related_kind_of_entity_resolving() {
    let (state, content, catalog) = sample_fixture();
    let mut ctx = PresentationContext::default();

    // By target prototype name
    let res = interpolate(
        "Supported by: {related:patron of scholar}",
        &state,
        &content,
        &catalog,
        &ctx,
    );
    assert_eq!(res, "Supported by: Wealthy Benefactor");

    // By subject
    ctx.subject = Some(EntityId(1));
    let res_subject = interpolate(
        "Patron is: {related:patron of subject}",
        &state,
        &content,
        &catalog,
        &ctx,
    );
    assert_eq!(res_subject, "Patron is: Wealthy Benefactor");

    // Reverse relation: incoming edge "to target"
    let res_to = interpolate(
        "Beneficiary: {related:patron to patron}",
        &state,
        &content,
        &catalog,
        &ctx,
    );
    assert_eq!(res_to, "Beneficiary: Master Scholar");

    // By slot binding
    ctx.bound_cards
        .insert(SlotName::from("student"), vec![CardId::from("scholar")]);
    let res_slot = interpolate(
        "Student patron: {related:patron of student}",
        &state,
        &content,
        &catalog,
        &ctx,
    );
    assert_eq!(res_slot, "Student patron: Wealthy Benefactor");

    // By first(slot:...) query
    let res_first_slot = interpolate(
        "First slot patron: {related:patron of first(slot:student)}",
        &state,
        &content,
        &catalog,
        &ctx,
    );
    assert_eq!(res_first_slot, "First slot patron: Wealthy Benefactor");

    // Empty relation resolves to empty string
    let res_empty = interpolate(
        "Rival: [{related:rival of scholar}]",
        &state,
        &content,
        &catalog,
        &ctx,
    );
    assert_eq!(res_empty, "Rival: []");
}

#[test]
fn test_target_aspect_resolving() {
    let (state, content, catalog) = sample_fixture();
    let mut ctx = PresentationContext::default();

    // Slot aspect
    ctx.bound_cards
        .insert(SlotName::from("tome"), vec![CardId::from("grimoire")]);
    let res_slot = interpolate("Power: {tome.lore}", &state, &content, &catalog, &ctx);
    assert_eq!(res_slot, "Power: 5");

    // Live entity aspect in state (scholar entity has lore = 8)
    let res_ent = interpolate(
        "Live lore: {scholar.lore}",
        &state,
        &content,
        &catalog,
        &ctx,
    );
    assert_eq!(res_ent, "Live lore: 8");

    // Card prototype aspect in content (scroll has lore = 2)
    let res_proto = interpolate(
        "Scroll lore: {scroll.lore}",
        &state,
        &content,
        &catalog,
        &ctx,
    );
    assert_eq!(res_proto, "Scroll lore: 2");

    // Subject aspect
    ctx.subject = Some(EntityId(1));
    let res_sub = interpolate(
        "Subject lore: {subject.lore}",
        &state,
        &content,
        &catalog,
        &ctx,
    );
    assert_eq!(res_sub, "Subject lore: 8");

    // Default 0 for absent aspect on known card
    let res_absent = interpolate("Heat: {grimoire.heat}", &state, &content, &catalog, &ctx);
    assert_eq!(res_absent, "Heat: 0");
}

#[test]
fn test_subject_resolving_cue_subject() {
    let (state, content, catalog) = sample_fixture();

    // From EntityId
    let mut ctx = PresentationContext::default();
    ctx.subject = Some(EntityId(1));
    let res = interpolate("Behold {subject}!", &state, &content, &catalog, &ctx);
    assert_eq!(res, "Behold Master Scholar!");

    // From subject_card CardId
    let mut ctx_card = PresentationContext::default();
    ctx_card.subject_card = Some(CardId::from("grimoire"));
    let res_card = interpolate(
        "Examining {subject}!",
        &state,
        &content,
        &catalog,
        &ctx_card,
    );
    assert_eq!(res_card, "Examining Ancient Grimoire!");
}

#[test]
fn test_graceful_fallback_for_missing_tokens() {
    let (state, content, catalog) = sample_fixture();
    let ctx = PresentationContext::default();

    // Unknown token left verbatim
    let res = interpolate("Unknown token: {unknown}", &state, &content, &catalog, &ctx);
    assert_eq!(res, "Unknown token: {unknown}");

    // Unknown target aspect left verbatim
    let res_dot = interpolate(
        "Value: {nonexistent.aspect}",
        &state,
        &content,
        &catalog,
        &ctx,
    );
    assert_eq!(res_dot, "Value: {nonexistent.aspect}");

    // Unknown relation target left verbatim
    let res_rel = interpolate(
        "Ally: {related:friend of ghost}",
        &state,
        &content,
        &catalog,
        &ctx,
    );
    assert_eq!(res_rel, "Ally: {related:friend of ghost}");

    // Missing subject left verbatim
    let res_sub = interpolate("Who: {subject}", &state, &content, &catalog, &ctx);
    assert_eq!(res_sub, "Who: {subject}");

    // Missing roll left verbatim
    let res_roll = interpolate("Rolled: {roll}", &state, &content, &catalog, &ctx);
    assert_eq!(res_roll, "Rolled: {roll}");
}

#[test]
fn test_custom_vars_and_roll() {
    let (state, content, catalog) = sample_fixture();
    let mut ctx = PresentationContext::default();
    ctx.vars
        .insert("location".to_string(), "Observatory".to_string());
    ctx.roll = Some(6);

    let res = interpolate(
        "At {location}, roll was {roll}",
        &state,
        &content,
        &catalog,
        &ctx,
    );
    assert_eq!(res, "At Observatory, roll was 6");
}
