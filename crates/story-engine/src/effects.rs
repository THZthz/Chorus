//! The effect registry and the built-in interpreter (kernel-api.md §4,
//! spec.md §4.5). The authored vocabulary is closed; host Rust extends the
//! kernel by registering `Host` closures under op names.

mod cards;
mod narrate;
mod relations;
mod run;
mod start;

use crate::eval::Ctx;
use crate::Event;
use std::collections::BTreeMap;
use std::sync::Arc;
use story_content::ir::EffectNode;
use story_core::{EntityId, State};

/// The public effect seam: a host-registered operation applies onto the state
/// and returns events. Evaluation totality applies to host ops as well.
pub trait Effect: Send + Sync {
    fn apply(&self, state: &mut State, ctx: &mut EffectCtx) -> Vec<Event>;
}

impl<F> Effect for F
where
    F: Fn(&mut State, &mut EffectCtx) -> Vec<Event> + Send + Sync,
{
    fn apply(&self, state: &mut State, ctx: &mut EffectCtx) -> Vec<Event> {
        self(state, ctx)
    }
}

/// The per-resolution-unit effect context (kernel-api.md §4). `subject` for
/// card-completion and expiry books rides in `handles` under that name, which
/// is how the compiled IR addresses it.
#[derive(Default)]
pub struct EffectCtx {
    /// `None` in context-free books (expire, event) and commit selection.
    pub sit: Option<story_core::SituationId>,
    /// The stepped now of the running resolution.
    pub now: story_core::Time,
    /// Last-wins roll binding.
    pub roll: Option<i64>,
    /// Spawn-handle pools, mint-ordered; cleared when their effect list ends.
    pub handles: BTreeMap<String, Vec<EntityId>>,
    /// The `continue via` target this list executed, if any: recorded on the
    /// taken path (a `continue` inside an untaken branch never fires) and
    /// read by the completion's adoption transition. Last-wins, like the
    /// roll binding.
    pub continued: Option<story_core::RecipeId>,
}

impl EffectCtx {
    pub(crate) fn eval_ctx(&self, recipe: Option<&story_core::RecipeId>) -> Ctx {
        Ctx {
            sit: self.sit,
            sel_bindings: None,
            subject: self.handles.get("subject").and_then(|v| v.first()).copied(),
            roll: self.roll,
            recipe: recipe.cloned(),
        }
    }
}

/// One registry entry: precompiled nodes or a host closure, addressed by op
/// name through `EffectNode::Op`. The host closure rides an `Arc`, so a
/// clone shares it (story-analyze spec §2.2).
#[derive(Clone)]
pub enum RegistryEntry {
    Compiled(Vec<EffectNode>),
    Host(Arc<dyn Effect>),
}

/// The op-name registry; `boot` verifies every referenced op is registered.
#[derive(Clone, Default)]
pub struct EffectRegistry {
    pub(crate) ops: BTreeMap<String, RegistryEntry>,
}

impl EffectRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn register(&mut self, name: impl Into<String>, entry: RegistryEntry) {
        self.ops.insert(name.into(), entry);
    }

    pub fn register_host(
        &mut self,
        name: impl Into<String>,
        f: impl Fn(&mut State, &mut EffectCtx) -> Vec<Event> + Send + Sync + 'static,
    ) {
        self.register(name, RegistryEntry::Host(Arc::new(f)));
    }

    pub(crate) fn contains_op(&self, name: &str) -> bool {
        self.ops.contains_key(name)
    }
}
