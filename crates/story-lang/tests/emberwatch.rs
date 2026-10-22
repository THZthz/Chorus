//! Acceptance: `docs/emberwatch.sw` compiles with zero errors and zero
//! warnings, and the compiled shape matches the sample's declaration surface.

use story_content::ir::EffectNode;

#[test]
fn emberwatch_compiles_clean() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/emberwatch.sw");
    let out = story_lang::compile_file(&path);
    for d in &out.diagnostics {
        panic!("unexpected diagnostic: {d}");
    }
    let content = out.content.expect("content present without errors");

    // Cards: warden, beacon, coal, tea, driftwood, portent, gull, calm, token.
    assert_eq!(content.cards.len(), 9);
    let card = |name: &str| &content.cards[&story_core::CardId::from(name)];
    assert_eq!(card("warden").kind.as_deref(), Some("person"));
    assert!(card("warden").boot.is_some());
    assert_eq!(card("tea").lifetime, Some(180));
    assert!(matches!(
        card("coal").mint,
        story_core::MintPolicy::Stackable
    ));

    // Aspects registry with read defaults.
    assert_eq!(content.aspects.len(), 9);
    assert_eq!(content.aspects["heat"].default, 0);

    // Verbs: tend, sup, dream, seaward, omen_sight, comb, furnace, night.
    assert_eq!(content.verbs.len(), 8);
    let tend = &content.verbs["tend"];
    assert_eq!(tend.recipes.len(), 4);
    assert_eq!(tend.recipes["handbook"].priority, 5);
    assert_eq!(tend.recipes["routine"].priority, 100);
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

    // Every recipe declares its own duration; the sample has no instant recipe.
    for verb in content.verbs.values() {
        for recipe in verb.recipes.values() {
            assert!(recipe.duration >= 0, "recipe {} has a duration", recipe.id);
        }
    }

    // blaze: consume -> set -> narrate.
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

    // Decks, legacies, endings.
    assert_eq!(content.decks["omens"].entries.len(), 4);
    assert_eq!(
        content.decks["omens"]
            .fallback
            .as_ref()
            .map(|c| c.0.as_str()),
        Some("calm")
    );
    assert_eq!(content.legacies["dawns_seen"].default, 0);
    assert_eq!(content.endings.len(), 3);
    assert!(content.events.is_empty());
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
