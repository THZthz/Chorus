//! The full-conversion test, the playability smoke, and the F-2/F-3 play
//! probes, gated on the CS core root: `CS2SW_CS_CORE` (pinned on this
//! machine in `.cargo/config.toml`). They skip cleanly when the tree is
//! absent.

use std::path::PathBuf;
use std::sync::Arc;

use story_core::{Entity, EntityId, MintPolicy, SlotName, VerbId};
use story_engine::{CommitAction, EffectRegistry, Engine, Event, Snapshot};

fn cs_core() -> Option<PathBuf> {
    let root = cs2sw::cs_core_dir()?;
    if root.join("elements").is_dir() && root.join("recipes").is_dir() {
        Some(root)
    } else {
        eprintln!("skipping: CS core tree not found at {}", root.display());
        None
    }
}

fn compile(root: &PathBuf, name: &str) -> Arc<story_content::Content> {
    let (conversion, diags) = cs2sw::run(root);
    assert!(
        diags.iter().all(|d| !d.is_error()),
        "the converted content must compile: {diags:?}"
    );
    let out = story_lang::compile_source(name, &Arc::new(conversion.sw));
    assert!(!out.diagnostics.iter().any(|d| d.is_error()));
    Arc::new(out.content.expect("compiled"))
}

fn find(engine: &Engine, name: &str) -> Option<EntityId> {
    engine
        .state()
        .entities
        .values()
        .find(|x| x.name == name)
        .map(|x| x.id)
}

#[test]
fn full_conversion_compiles_clean() {
    let Some(root) = cs_core() else { return };
    let (conversion, diags) = cs2sw::run(&root);
    let errors: Vec<_> = diags.iter().filter(|d| d.is_error()).collect();
    assert!(
        errors.is_empty(),
        "the full conversion must compile with zero errors; first: {:#?}",
        errors.iter().take(20).collect::<Vec<_>>()
    );
    let warnings: Vec<_> = diags.iter().filter(|d| !d.is_error()).collect();
    assert!(
        warnings.is_empty(),
        "no unexpected warnings; got: {:#?}",
        warnings.iter().take(10).collect::<Vec<_>>()
    );
    // The mechanical-regularity contract: same tree, same bytes.
    let data = cs2sw::load_tree(&root);
    let again = cs2sw::convert(&data);
    assert_eq!(conversion.sw, again.sw, "the conversion is deterministic");

    let s = &conversion.stats;
    assert!(s.cards_out > 1000, "the full card population converts");
    assert!(s.recipes_out > 1800, "the full recipe population converts");
    assert!(s.verbs_out > 60, "player, ambient, shadow, and popup verbs");
    // The F-1 census: same-context linked continuations are reachable
    // (inlined or started); the residual dead set is small and every dead
    // verb is individually reported.
    assert!(
        s.dead_recipes < 100,
        "the linked cascade routes CS continuations; {} dead recipes remain",
        s.dead_recipes
    );
    assert_eq!(s.dead_verbs, 8, "the dead verbs stay individually reported");
    println!(
        "full conversion: {} cards, {} recipes, {} verbs, {} decks, {} endings; {} gap rows; dead census: {} recipes / {} verbs",
        s.cards_out,
        s.recipes_out,
        s.verbs_out,
        s.decks_out,
        s.endings_out,
        conversion.gaps.rows.len(),
        s.dead_recipes,
        s.dead_verbs
    );
}

#[test]
fn playability_smoke_zero_engine_errors() {
    let Some(root) = cs_core() else { return };
    let content = compile(&root, "cultist-simulator.smoke.sw");
    let (mut engine, boot) =
        Engine::boot(content.clone(), EffectRegistry::new(), cs2sw::SEED, None);
    let boot_errors: Vec<_> = boot.iter().filter(|d| d.is_error()).collect();
    assert!(
        boot_errors.is_empty(),
        "boot must report no error diagnostics: {boot_errors:#?}"
    );

    // Commit the common verbs with plausible offerings.
    let offers = ["health", "reason", "passion", "funds", "introjob"];
    let mut committed = Vec::new();
    for verb in ["work", "dream", "study"] {
        for offer in offers.iter().map(|o| Some(*o)).chain(std::iter::once(None)) {
            let bindings = match offer {
                Some(name) => match find(&engine, name) {
                    Some(id) => vec![(SlotName::from("offer"), id)],
                    None => continue,
                },
                None => vec![],
            };
            let action = CommitAction {
                verb: VerbId::from(verb),
                bindings,
            };
            match engine.commit(action) {
                Ok(_) => {
                    committed.push((verb, offer));
                    break;
                }
                // A refusal (the card already bound, nothing eligible) moves
                // down the offer ladder; only diagnostics with error severity
                // count against the zero-error bar, and commit refusals never
                // mutate state.
                Err(_) => continue,
            }
        }
    }
    assert!(
        committed.len() >= 3,
        "work, dream, and study all commit; got {committed:?}"
    );

    // F-3: the funds card greedy-bound into `needs`' expenses slot at boot is
    // destroyed by the same-context linked continuation (`discardseason`'s
    // funds wipe) running inside the parent situation at its completion.
    let needs_sit = *engine.state().situations.keys().next().expect("needs opens at boot");
    let bound_funds = engine.state().situations[&needs_sit].bindings[&SlotName::from("expenses")]
        .iter()
        .map(|b| b.entity)
        .collect::<Vec<_>>();
    assert!(
        !bound_funds.is_empty(),
        "the expenses slot greedy-pulls a funds card at boot"
    );

    // Advance through two full season cycles (the ambient `needs` window is
    // 60m): every advance returns Ok and no resolution fails.
    let mut turns = 0;
    let mut resolution_failures = Vec::new();
    let mut journal = Vec::new();
    let mut wipe_checked = false;
    while turns < 140 {
        match engine.advance(engine.turn()) {
            Ok(events) => {
                for e in &events {
                    if let Event::ResolutionFailed { sit, reason } = e {
                        resolution_failures.push(format!("sit {sit}: {reason}"));
                    }
                    if let Event::SituationCompleted { sit, recipe } = e {
                        journal.push(format!("[sit {sit}] {recipe} completed"));
                    }
                }
            }
            Err(d) => panic!("advance failed at turn {turns}: {d}"),
        }
        turns += 1;
        if !wipe_checked && engine.state().now >= 60 {
            wipe_checked = true;
            let wiped: Vec<_> = bound_funds
                .iter()
                .filter(|id| engine.state().entities.contains_key(id))
                .collect();
            assert!(
                wiped.is_empty(),
                "the funds wipe consumed the expenses-bound cards: {wiped:?}"
            );
            assert!(
                journal.iter().any(|l| l.contains("time.needs completed")),
                "needs completed within the first season: {journal:?}"
            );
            println!(
                "smoke: the expenses-bound funds card(s) {bound_funds:?} were destroyed at the \
                 first needs completion (clock {}m)",
                engine.state().now
            );
        }
        if engine.state().ended.is_some() {
            break;
        }
    }
    assert!(wipe_checked, "the first season completed");
    assert!(
        turns >= 20,
        "at least 20 turns advance; got {turns}"
    );
    assert!(
        resolution_failures.is_empty(),
        "zero resolution failures: {resolution_failures:?}"
    );
    println!(
        "smoke: committed {} verbs, advanced {} turns to clock {}m, {} live cards",
        committed.len(),
        turns,
        engine.state().now,
        engine.state().entities.len()
    );
}

/// F-2: the requirement card routes into the offer slot (the slot filter),
/// and at the completing recipe's completion the bound card's aspect-qualified
/// book walks and swaps it. The probe uses `getcommissionbechet`, whose
/// requirement key `bechetready` is the catalyst of Mme Bechet's
/// `commissiongiven` book. The pure core reaches patron cards only through
/// the lore-gated patron chain, so the probe hand-prepares the save (L2: the
/// snapshot is the save) instead of minting through engine surgery.
#[test]
fn card_book_fires_at_a_completion_that_binds_the_requirement_card() {
    let Some(root) = cs_core() else { return };
    let content = compile(&root, "cultist-simulator.book.sw");
    let (mut engine, boot) =
        Engine::boot(content.clone(), EffectRegistry::new(), cs2sw::SEED, None);
    assert!(boot.iter().all(|d| !d.is_error()), "{boot:?}");

    // The offer slot of getcommissionbechet carries the filter
    // `name:bechetready`; committing a card that does not satisfy it is
    // refused before opening.
    let reason = find(&engine, "reason").expect("reason at boot");
    let refusal = engine.commit(CommitAction {
        verb: VerbId::from("talk"),
        bindings: vec![(SlotName::from("offer"), reason)],
    });
    assert!(
        refusal.is_err(),
        "the requirement-routed offer slot refuses a non-matching card"
    );

    // Hand-prepare the save: one `bechetready` card on the table.
    let snap: Snapshot = engine.snapshot();
    let mut state: story_core::State = serde_json::from_slice(&snap.state).expect("state bytes");
    let table = state
        .zones
        .values()
        .find(|z| matches!(z.kind, story_core::ZoneKind::Table))
        .map(|z| z.id)
        .expect("the declared table zone");
    let id = EntityId(state.next_entity);
    state.next_entity += 1;
    state.entities.insert(
        id,
        Entity {
            id,
            name: "bechetready".into(),
            prototype: "bechetready".into(),
            kind: Some("character".into()),
            aspects: [
                ("bechetready".to_string(), 1),
                ("mortal".to_string(), 1),
                ("patron".to_string(), 1),
            ]
            .into_iter()
            .collect(),
            attrs: Default::default(),
            zone: table,
            mint: MintPolicy::SingleCopy,
            expires_at: None,
        },
    );
    let prepared = Snapshot {
        format: snap.format,
        content_fingerprint: snap.content_fingerprint,
        state: serde_json::to_vec(&state).expect("serialize"),
    };
    engine.restore(&prepared).expect("restore");

    // Commit talk binding the requirement card: the filter accepts it and the
    // recipe declares its catalyst (`commissiongiven`).
    let bechetready = find(&engine, "bechetready").expect("the prepared card");
    let events = engine
        .commit(CommitAction {
            verb: VerbId::from("talk"),
            bindings: vec![(SlotName::from("offer"), bechetready)],
        })
        .expect("the routed commit opens getcommissionbechet");
    for e in &events {
        if let Event::Bound { sit, slot, entity } = e {
            println!("journal: [sit {sit}] {slot} <- entity {entity}");
        }
    }

    // Advance past the 10m warmup: at the completion the bound card's
    // `on complete [commissiongiven]` book swaps Mme Bechet to her
    // commission-given face.
    engine.advance(engine.turn() * 11).expect("advance");
    let journal_swap = format!(
        "bechetready #{} destroyed, bechet minted",
        bechetready.0
    );
    assert!(
        !engine.state().entities.contains_key(&bechetready),
        "the book swapped the bound card away ({journal_swap})"
    );
    let bechet = find(&engine, "bechet").expect("the book's swap target is in play");
    println!(
        "journal: the commissiongiven book fired at getcommissionbechet's completion: \
         card bechetready (entity {bechetready}) swapped to bechet (entity {bechet})"
    );
    assert_ne!(bechet, bechetready);
}
