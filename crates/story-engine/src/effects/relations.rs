//! The `relate` and `unrelate` effects over the logical relation edges.

use super::EffectCtx;
use crate::{Engine, Event};

impl Engine {
    /// `relate <from> to <to> as <kind>`: subjects × objects in resolved
    /// order; self-pairs skipped silently; an exclusive kind steals first
    /// (one `Unrelated` per victim, map order, before the `Related`); one
    /// `Related` per written logical edge (authored orientation).
    pub(super) fn apply_relate(
        &mut self,
        from: &story_content::ir::Target,
        to: &story_content::ir::Target,
        kind: &str,
        fctx: &EffectCtx,
        recipe: Option<story_core::RecipeId>,
        ev: &mut Vec<Event>,
    ) {
        let ctx = fctx.eval_ctx(recipe.as_ref());
        let subjects = self.resolve_target(from, &ctx, &fctx.handles);
        let objects = self.resolve_target(to, &ctx, &fctx.handles);
        let (symmetric, exclusive) = self.relation_flags(kind);
        for a in subjects {
            for b in objects.clone() {
                if a == b {
                    continue;
                }
                if exclusive {
                    let victims: Vec<story_core::RemovedEdge> = if symmetric {
                        story_core::edges_touching(&self.state.relations, kind, a, b)
                    } else {
                        story_core::edges_into(&self.state.relations, kind, b)
                    }
                    .into_iter()
                    .filter(|(f, _, t)| !(*f == a && *t == b) && !(symmetric && *f == b && *t == a))
                    .collect();
                    for (f, k, t) in victims {
                        if story_core::unrelate_edge(&mut self.state.relations, f, &k, t) {
                            ev.push(Event::Unrelated {
                                from: f,
                                kind: k,
                                to: t,
                            });
                        }
                    }
                }
                if story_core::relate_edge(&mut self.state.relations, a, kind, b, symmetric) {
                    ev.push(Event::Related {
                        from: a,
                        kind: kind.to_string(),
                        to: b,
                    });
                }
            }
        }
    }

    /// `unrelate <from> from <to> [as <kind>]`: an omitted kind severs every
    /// logical edge between each pair, either direction, each once (stored
    /// orientation in the events).
    pub(super) fn apply_unrelate(
        &mut self,
        from: &story_content::ir::Target,
        to: &story_content::ir::Target,
        kind: Option<&str>,
        fctx: &EffectCtx,
        recipe: Option<story_core::RecipeId>,
        ev: &mut Vec<Event>,
    ) {
        let ctx = fctx.eval_ctx(recipe.as_ref());
        let subjects = self.resolve_target(from, &ctx, &fctx.handles);
        let objects = self.resolve_target(to, &ctx, &fctx.handles);
        for a in subjects {
            for b in objects.clone() {
                if a == b {
                    continue;
                }
                match kind {
                    Some(k) => {
                        if story_core::unrelate_edge(&mut self.state.relations, a, k, b) {
                            ev.push(Event::Unrelated {
                                from: a,
                                kind: k.to_string(),
                                to: b,
                            });
                        }
                    }
                    None => {
                        for (f, k, t) in story_core::sever_pair(&mut self.state.relations, a, b) {
                            ev.push(Event::Unrelated {
                                from: f,
                                kind: k,
                                to: t,
                            });
                        }
                    }
                }
            }
        }
    }

    /// The declared flags of a relation kind; the compiler guarantees every
    /// authored kind is declared, so the fallback is defensive.
    fn relation_flags(&self, kind: &str) -> (bool, bool) {
        self.content
            .relation_kind(kind)
            .map(|k| (k.symmetric, k.exclusive))
            .unwrap_or((false, false))
    }
}
