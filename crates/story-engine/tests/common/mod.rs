//! Shared helpers for the story-engine acceptance tests: fixture compilation,
//! boot, and small state readers. Mutating access goes through the engine API.
//! Each test crate compiles this module whole; it uses a subset.

#![allow(dead_code)]

use std::sync::Arc;

use story_content::Content;
use story_core::{EntityId, RecipeId, SituationId, SlotName, VerbId};
use story_engine::{CommitAction, EffectRegistry, Engine, Event};

/// Compile a fixture; any error diagnostic fails the test.
pub fn compile(src: &str) -> Arc<Content> {
    let out = story_lang::compile_source("fixture", src);
    let errors: Vec<String> = out
        .diagnostics
        .iter()
        .filter(|d| d.is_error())
        .map(|d| d.to_string())
        .collect();
    assert!(
        errors.is_empty(),
        "fixture failed to compile:\n{}",
        errors.join("\n")
    );
    Arc::new(out.content.expect("no errors means content"))
}

/// Boot with the host seed; any error diagnostic fails the test.
pub fn boot(content: Arc<Content>, seed: u64) -> Engine {
    let (engine, diags) = Engine::boot(content, EffectRegistry::new(), seed, None);
    let errors: Vec<String> = diags
        .iter()
        .filter(|d| d.is_error())
        .map(|d| d.to_string())
        .collect();
    assert!(errors.is_empty(), "boot failed:\n{}", errors.join("\n"));
    engine
}

/// The oldest live entity of a card name.
pub fn named(engine: &Engine, name: &str) -> EntityId {
    engine
        .state()
        .entities
        .values()
        .find(|e| e.name == name)
        .map(|e| e.id)
        .unwrap_or_else(|| panic!("no live entity named `{name}`"))
}

/// Commit with explicit entity ids (no name lookup).
pub fn commit(
    engine: &mut Engine,
    verb: &str,
    bindings: &[(&str, EntityId)],
) -> Result<Vec<Event>, story_content::RuntimeDiagnostic> {
    let bindings: Vec<(SlotName, EntityId)> = bindings
        .iter()
        .map(|(slot, e)| (SlotName::from(*slot), *e))
        .collect();
    engine.commit(CommitAction {
        verb: VerbId::from(verb),
        bindings,
    })
}

/// The zone kind of an entity's location, as a readable word.
pub fn zone_kind_of(engine: &Engine, e: EntityId) -> &'static str {
    let z = engine.state().entities[&e].zone;
    match &engine.state().zones[&z].kind {
        story_core::ZoneKind::Table => "table",
        story_core::ZoneKind::Hidden => "hidden",
        story_core::ZoneKind::SituationInterior(_) => "interior",
        story_core::ZoneKind::Of(_) => "carrier",
    }
}

pub fn commit_named(
    engine: &mut Engine,
    verb: &str,
    bindings: &[(&str, &str)],
) -> Result<Vec<Event>, story_content::RuntimeDiagnostic> {
    let bindings: Vec<(SlotName, EntityId)> = bindings
        .iter()
        .map(|(slot, card)| (SlotName::from(*slot), named(engine, card)))
        .collect();
    engine.commit(CommitAction {
        verb: VerbId::from(verb),
        bindings,
    })
}

/// A card's aspect read through the state (absent keys read the declared
/// default).
pub fn aspect(engine: &Engine, entity: EntityId, key: &str) -> i64 {
    engine
        .state()
        .entities
        .get(&entity)
        .and_then(|e| e.aspects.get(key).copied())
        .unwrap_or(0)
}

/// How many live entities carry this card name.
pub fn count_named(engine: &Engine, name: &str) -> usize {
    engine
        .state()
        .entities
        .values()
        .filter(|e| e.name == name)
        .count()
}

pub fn narration(events: &[Event]) -> Vec<String> {
    events
        .iter()
        .filter_map(|e| match e {
            Event::Narration { text } => Some(text.clone()),
            _ => None,
        })
        .collect()
}

pub fn ended(engine: &Engine) -> Option<String> {
    engine.state().ended.as_ref().map(|e| e.to_string())
}

/// Serialized state bytes — the determinism and drift witness.
pub fn legacy_out(engine: &Engine) -> &std::collections::BTreeMap<String, i64> {
    engine.legacy_out()
}

pub fn state_bytes(engine: &Engine) -> Vec<u8> {
    serde_json::to_vec(engine.state()).expect("state serializes")
}

/// Live situation ids.
pub fn situations(engine: &Engine) -> Vec<SituationId> {
    engine.state().situations.keys().copied().collect()
}

pub fn recipe_runs(engine: &Engine, recipe: &str) -> u32 {
    engine
        .state()
        .recipe_runs
        .get(&RecipeId::from(recipe))
        .copied()
        .unwrap_or(0)
}
