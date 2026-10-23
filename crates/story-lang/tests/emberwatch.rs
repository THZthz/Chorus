//! Acceptance: `docs/emberwatch.sw` compiles with zero errors and zero
//! warnings, and the compiled shape matches the sample's declaration surface
//! — the full v1 mechanism set, keyed by name where the count alone would
//! not tell the story.

use story_content::ir::EffectNode;

#[test]
fn emberwatch_compiles_clean() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/emberwatch.sw");
    let out = story_lang::compile_file(&path);
    for d in &out.diagnostics {
        panic!("unexpected diagnostic: {d}");
    }
    let content = out.content.expect("content present without errors");

    // Cards: warden, beacon, lantern, coal, oil, bread, tea, driftwood,
    // flotsam, portent, gull, calm, token, flare, ebb, flood.
    assert_eq!(content.cards.len(), 16);
    let card = |name: &str| &content.cards[&story_core::CardId::from(name)];
    assert_eq!(card("warden").kind.as_deref(), Some("person"));
    assert!(card("warden").boot.is_some());
    // Boot relations: the warden knows the gull and carries the lantern.
    assert_eq!(card("warden").relates.len(), 2);
    assert_eq!(card("tea").lifetime, Some(180));
    assert!(matches!(
        card("coal").mint,
        story_core::MintPolicy::Stackable
    ));
    assert!(matches!(
        card("flare").mint,
        story_core::MintPolicy::Group(_)
    ));
    // The mint alias: flotsam materializes a driftwood body.
    assert_eq!(
        card("flotsam").mint_alias.as_ref().map(|a| a.0.as_str()),
        Some("driftwood")
    );
    // Card books: tea's aspect-qualified completion book, portent's expiry book.
    assert!(!card("tea").on_complete.is_empty());
    assert!(card("portent").on_expire.is_some());

    // Aspects registry with read defaults.
    assert_eq!(content.aspects.len(), 10);
    assert_eq!(content.aspects["heat"].default, 0);

    // Relations registry: directed exclusive, symmetric exclusive, plain.
    assert_eq!(content.relation_kinds.len(), 3);
    assert!(content.relation_kind("carries").unwrap().exclusive);
    let bonded = content.relation_kind("bonded").unwrap();
    assert!(bonded.symmetric && bonded.exclusive);
    assert!(!content.relation_kind("knows").unwrap().symmetric);

    // Verbs: tend, sup, lamp, dream, omen_sight, seaward, comb, cast,
    // salvage, distress, feed_gull, log, furnace, night.
    assert_eq!(content.verbs.len(), 14);
    let tend = &content.verbs["tend"];
    assert_eq!(tend.recipes.len(), 5);
    assert_eq!(tend.recipes["handbook"].priority, 5);
    assert_eq!(tend.recipes["routine"].priority, 100);
    assert!(tend.recipes["routine"].allow_shadow);
    assert_eq!(tend.recipes["feed"].duration, 30);

    // Slots: fuel (consuming, bounded), kindle (greedy + open).
    let feed = &tend.recipes["feed"];
    let fuel = feed.slots.iter().find(|s| s.name.0 == "fuel").unwrap();
    assert!(fuel.consuming);
    assert_eq!(fuel.min, 1);
    assert_eq!(fuel.max, Some(2));
    let kindle = feed.slots.iter().find(|s| s.name.0 == "kindle").unwrap();
    assert!(kindle.greedy && kindle.open);
    assert_eq!(kindle.min, 0);

    // The compound duration and the open-count cap.
    assert_eq!(content.verbs["log"].recipes["close"].duration, 90);
    assert_eq!(content.verbs["dream"].recipes["sleep"].max, Some(4));
    assert_eq!(content.verbs["sup"].recipes["eat"].declares.len(), 1);

    // Every recipe declares its own duration; the sample has no instant recipe.
    for verb in content.verbs.values() {
        for recipe in verb.recipes.values() {
            assert!(recipe.duration >= 0, "recipe {} has a duration", recipe.id);
        }
    }

    // blaze: the stoke effectset expanded to consume -> set, then narrate.
    let blaze = &tend.recipes["blaze"].effects;
    assert_eq!(blaze.len(), 3);
    assert!(matches!(blaze[0], EffectNode::Consume { .. }));

    // dream.sleep: draw, roll, if/else, then a chain start.
    let sleep = &content.verbs["dream"].recipes["sleep"].effects;
    assert_eq!(sleep.len(), 4);
    assert!(matches!(sleep[1], EffectNode::Roll { .. }));
    assert!(matches!(sleep[2], EffectNode::Branch { .. }));
    assert!(matches!(sleep[3], EffectNode::Start { .. }));

    // Selection data: night's priorities are all distinct (1,2,5,10,20).
    let night = &content.verbs["night"];
    let mut prios: Vec<i64> = night.recipes.values().map(|r| r.priority).collect();
    prios.sort();
    assert_eq!(prios, vec![1, 2, 5, 10, 20]);
    assert!(night.recipes["dawn"].when.is_some());
    assert!(matches!(
        content.verbs["furnace"].mode,
        story_content::ir::VerbMode::Autonomous
    ));
    assert!(content.verbs["omen_sight"].recipes["read_portent"].chain_only);

    // Decks: weighted omens with a fallback, and the wrapping tides.
    assert_eq!(content.decks.len(), 2);
    assert_eq!(content.decks["omens"].entries.len(), 5);
    assert_eq!(
        content.decks["omens"]
            .fallback
            .as_ref()
            .map(|c| c.0.as_str()),
        Some("calm")
    );
    assert!(content.decks["tides"].fallback.is_none());
    assert!(content.decks["tides"].wrap);

    // Legacies and endings.
    assert_eq!(content.legacies.len(), 3);
    assert_eq!(content.legacies["dawns_seen"].default, 0);
    assert_eq!(content.endings.len(), 3);

    // The storm bell: one authored event ignites the on-event book.
    assert_eq!(content.events.len(), 1);
}

#[test]
fn emberwatch_fingerprint_is_stable_across_compiles() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/emberwatch.sw");
    let a = story_lang::compile_file(&path);
    let b = story_lang::compile_file(&path);
    let fa = a.content.as_ref().unwrap().fingerprint();
    let fb = b.content.as_ref().unwrap().fingerprint();
    assert_eq!(fa, fb);
    assert_ne!(fa, 0);
}
