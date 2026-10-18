//! The script-layer data shapes: the parsed [`Script`] (meta + raw content +
//! declared entities/relations) and the [`CompiledScript`] it compiles to.

use crate::content::{CompiledContent, ContentRaw, DeclaredEntity};
use crate::core::clock::Clock;
use crate::core::types::{Aspects, State};

/// Script meta: title, player, goal, start clock, dice seed.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ScriptMeta {
    pub title: Option<String>,
    pub player: Option<String>,
    pub goal: Option<String>,
    pub start: Option<Clock>,
    /// §4.4: the dice seed (default 0) — the run's whole dice sequence is a
    /// pure function of (seed, rng counter)
    pub seed: Option<i64>,
}

/// One declared entity line.
#[derive(Debug, Clone)]
pub struct EntitySpec {
    pub name: String,
    pub kind: String,
    /// §4.2: `NAME is KIND stackable …` — this line's card may share its name
    pub stackable: bool,
    pub aspects: Aspects,
    pub container_name: Option<String>,
}

/// One declared relation line.
#[derive(Debug, Clone)]
pub struct RelationSpec {
    pub a: String,
    /// Rust name `type_` (`type` is a keyword)
    pub type_: String,
    pub b: String,
}

/// The parsed script: meta + raw content + declared entities/relations.
#[derive(Debug, Clone)]
pub struct Script {
    pub meta: ScriptMeta,
    pub content: ContentRaw,
    pub entities: Vec<EntitySpec>,
    pub relations: Vec<RelationSpec>,
}

/// The `compile_script` return shape `{ content, state, meta, dict }` —
/// `dict` is the LOAD-EFFECTIVE dictionary (§4.1): the caller's plus
/// the script's `kinds:` declarations, handed back so tools that re-run
/// load-time grammars over the compiled world (the reach analyzer's
/// `--demand` query parse) validate against the vocabulary the script
/// actually loaded under. The engine itself never reads kinds back —
/// runtime stays vocabulary-free.
#[derive(Debug)]
pub struct CompiledScript {
    pub content: CompiledContent,
    pub state: State,
    pub meta: ScriptMeta,
    pub dict: crate::core::dict::Dictionary,
}

/// The compile-side face of the declared entity lines: deck resolution and
/// the §4.2 stackable sets read name/kind/stackable only — one shared map
/// for every consumer that crosses from the script face to compiled content.
pub(super) fn declared_entities(entities: &[EntitySpec]) -> Vec<DeclaredEntity> {
    entities
        .iter()
        .map(|e| DeclaredEntity {
            name: e.name.clone(),
            kind: e.kind.clone(),
            stackable: e.stackable,
        })
        .collect()
}
