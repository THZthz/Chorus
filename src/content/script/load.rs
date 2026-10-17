//! The load entry point: [`compile_script`] parses a script and turns it
//! into runnable game content + the initial State.

use indexmap::{IndexMap, IndexSet};

use super::parse::parse_script;
use super::types::{declared_entities, CompiledScript, Script};
use crate::content::{compile_content, ContentRaw, VerbContent};
use crate::core::clock::abs_min;
use crate::core::dict::Dictionary;
use crate::core::jsonw::canonical_key_order;
use crate::core::types::{add_entity, new_state, relation_key, AttrValue, Attrs, Entity, Relation};
use crate::error::{ChorusError, Result};

/// The load-effective dictionary: the caller's dictionary plus the script's
/// `kinds:` declarations (§4.1, v0.7.5) and its `verbs:` declarations
/// (§4.3, v0.7.6), declaration order preserved. Vocabulary is load-time
/// state only — the engine and the log never read the dictionary's word
/// lists — so the augmentation rides a per-load CLONE and the caller's
/// dictionary is left untouched: loading the same text twice against one
/// dictionary stays idempotent, and `Game::from_script` keeps owning its
/// dictionary verbatim. Verbs register IF-UNKNOWN: a lexeme already present
/// (a core verb being configured, or one an earlier load of this same text
/// inserted) is skipped — `register_verb` is not idempotent, and the skip
/// is what makes recompiling one text against one dictionary — or against
/// its own effective output — succeed twice. An unknown lexeme runs the
/// full nameable grammar (§4.3): UPPER_SNAKE, never a reserved word (§5.1)
/// — `EVENT:` or `TO:` as a header fails loud here, naming the lexeme.
pub(super) fn dict_with_vocabulary(
    dict: &Dictionary,
    kinds: &IndexSet<String>,
    verbs: &IndexMap<String, VerbContent>,
) -> Result<Dictionary> {
    let mut eff = dict.clone();
    for k in kinds {
        // every declaration was validated by parse_script against the same
        // base dictionary — this cannot fail; if it ever does, the load
        // fails loud right here
        eff.register_kind(k)?;
    }
    for name in verbs.keys() {
        // the header grammar (verb_header_re) already enforced the shape at
        // parse; the reserved/verb-collision arms of the nameable grammar
        // are re-checked HERE, where the registration actually happens — an
        // intra-file duplicate header never reaches this loop (the parse-side
        // duplicate rule fired first)
        if !eff.is_verb(name) {
            eff.register_verb(name)?;
        }
    }
    Ok(eff)
}

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
    // §4.1/§4.3: the compile half sees the same effective dictionary the
    // parse half validated against — the script's kinds and verb lexemes
    // re-registered onto a fresh clone, so every slot/recipe-body query parse
    // below accepts them and `compile_verb`'s registration gate passes for a
    // script-declared lexeme
    let eff = dict_with_vocabulary(dict, &raw.kinds, &raw.verbs)?;
    // v0.7.6, order-independent: a declared verb lexeme may not share its
    // name with a declared entity or prototype. Registration grows
    // `reserved` (§5.1), so an overlap would flip `format_ref` to quoting
    // the name and the bare-ref parser to refusing it — and without this
    // check, whether the load errored at all would depend on which section
    // the file happened to list first (an entity line parses against the
    // base reserved set, so `entities:` before `verbs:` would slip a name
    // past `check_name` that the reverse order rejects). One walk over the
    // whole load kills the order dependence both directions.
    for name in script.entities.iter().map(|e| &e.name) {
        if raw.verbs.contains_key(name) {
            return Err(ChorusError::new(format!(
                "'{name}' is declared both as an entity and as a verb — a verb lexeme is a reserved word (§5.1)"
            )));
        }
    }
    // prototype names are `\S+` (integer-like keys legal), so the walk goes
    // through `canonical_key_order`, same as parse_script's entity/prototype
    // double-declaration check
    if let Some(protos) = &raw.prototypes {
        for name in canonical_key_order(protos) {
            if raw.verbs.contains_key(&name) {
                return Err(ChorusError::new(format!(
                    "'{name}' is declared both as a prototype and as a verb — a verb lexeme is a reserved word (§5.1)"
                )));
            }
        }
    }
    let content = compile_content(&raw, &eff, &declared)?;
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
    Ok(CompiledScript { content, state, meta: script.meta, dict: eff })
}
