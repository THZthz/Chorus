//! The load entry point: [`compile_script`] parses a script and turns it
//! into runnable game content + the initial State.

use super::parse::parse_script;
use super::types::{declared_entities, CompiledScript, Script};
use crate::content::{compile_content, ContentRaw};
use crate::core::clock::abs_min;
use crate::core::dict::Dictionary;
use crate::core::types::{add_entity, new_state, relation_key, AttrValue, Attrs, Entity, Relation};
use crate::error::{ChorusError, Result};

/// `compile_script(text, dict)` → `{ content, state, meta }`.
pub fn compile_script(text: &str, dict: &Dictionary) -> Result<CompiledScript> {
    compile_parsed(parse_script(text, dict)?, dict)
}

/// The compile half of [`compile_script`], shared with the lint — which
/// needs both the parsed face and a compiling script, so one parse serves
/// both.
pub(super) fn compile_parsed(script: Script, dict: &Dictionary) -> Result<CompiledScript> {
    // §4.4: the meta `seed:` rides into the compiled content — every die is
    // a pure function of (seed, counter), so the sequence is fixed at load.
    // The deck permutations (§4.4) are keyed on the same seed. Declared
    // entities ride along so deck spec entries may name plain entity lines
    // too. The entities go in ONCE (with
    // their stackability flags) — compile_content builds the §4.2 sets
    // from them.
    let declared = declared_entities(&script.entities);
    let raw = ContentRaw { seed: script.meta.seed, ..script.content };
    let content = compile_content(&raw, dict, &declared)?;
    let now = match script.meta.start {
        Some(start) => abs_min(start),
        None => 0,
    };
    let mut state = new_state(now);

    let id_of = |name: &str| format!("s_{}", name.to_lowercase());
    for e in &script.entities {
        add_entity(
            &mut state,
            Entity {
                id: id_of(&e.name),
                kind: e.kind.clone(),
                name: e.name.clone(),
                aspects: e.aspects.clone(),
                attrs: Attrs::new(),
                born_action: Some(0),
                dead_action: None,
                stackable: e.stackable,
            },
        )?;
    }
    for e in &script.entities {
        if let Some(container_name) = &e.container_name {
            // resolve the container FIRST (shared borrow ends), so the attrs
            // write below can take the exclusive borrow of state.entities
            let container_id = state.entities.get(&id_of(container_name)).map(|c| c.id.clone());
            // the entity was added in the loop above
            let Some(self_e) = state.entities.get_mut(&id_of(&e.name)) else {
                unreachable!("entity '{}' was added above", e.name);
            };
            if let Some(container_id) = container_id {
                self_e.attrs.insert("container".to_string(), AttrValue::Str(container_id));
            } else if e.kind == "place" {
                // a zone: a grouping with no card (§10)
                self_e.attrs.insert("zone".to_string(), AttrValue::Str(container_name.clone()));
            } else {
                return Err(ChorusError::new(format!(
                    "entity {}: unknown container '{container_name}'",
                    e.name
                )));
            }
        }
    }
    for rel in &script.relations {
        let a = state.entities.get(&id_of(&rel.a));
        let b = state.entities.get(&id_of(&rel.b));
        let (Some(a), Some(b)) = (a, b) else {
            return Err(ChorusError::new(format!(
                "relation {} is {} of {}: unknown entity",
                rel.a, rel.type_, rel.b
            )));
        };
        let def = dict.relation_def(&rel.type_)?;
        let key = relation_key(&a.id, &b.id, &rel.type_, def.symmetric);
        state.relations.insert(
            key,
            Relation {
                a: a.id.clone(),
                b: b.id.clone(),
                type_: rel.type_.clone(),
                value: 1,
                born_action: 0,
                dead_action: None,
            },
        );
    }
    // §11: the front door defaults actors to `player:` — resolve it against
    // the declared world here, at load, instead of at the first typed line
    if let Some(player) = &script.meta.player {
        let p = state
            .by_name
            .get(player)
            .and_then(|pool| pool.first())
            .and_then(|id| state.entities.get(id));
        let Some(p) = p else {
            return Err(ChorusError::new(format!("player '{player}' is not a declared entity")));
        };
        if p.kind != "character" {
            return Err(ChorusError::new(format!(
                "player '{player}' must be a character (got {})",
                p.kind
            )));
        }
    }
    Ok(CompiledScript { content, state, meta: script.meta })
}
