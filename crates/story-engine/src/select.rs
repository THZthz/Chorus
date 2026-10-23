//! The selection law (spec.md §4.3): the winner of any recipe scan is
//! `min_by (priority, decl_index)` over the candidates whose `when` holds.
//! Candidacy: player commits fill slot minimums explicitly; starts and
//! autonomous polls count greedy candidates in the current state.

use crate::eval::Ctx;
use crate::Engine;
use story_content::ir::{RecipeDecl, VerbDecl};
use story_core::{EntityId, RecipeId, SituationId, SlotName};

/// The source of an instance open; it fixes the eligibility row.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Source {
    /// Player commit: craftable, not chain-only.
    Commit,
    /// A `start` effect: every eligibility row qualifies.
    Start,
    /// An autonomous heartbeat: chain-only recipes never qualify.
    Poll,
}

/// The outcome of one recipe scan.
pub(crate) enum Selection {
    Matched(RecipeDecl),
    /// No eligible recipe, or every `when` false.
    Nothing,
    /// The winner's guard refused; the scan stops here.
    GuardRefused(String),
}

impl Engine {
    /// One selection scan over one verb. `via` pins the scan to one recipe
    /// (a named hand-off is still checked against `when` and the caps).
    pub(crate) fn select(
        &self,
        source: Source,
        verb: &VerbDecl,
        via: Option<&RecipeId>,
        bindings: Option<&[(SlotName, EntityId)]>,
        parent_sit: Option<SituationId>,
    ) -> Selection {
        let ctx = Ctx {
            sit: parent_sit,
            sel_bindings: bindings.map(|b| b.to_vec()),
            subject: None,
            roll: None,
            recipe: None,
        };
        let mut candidates: Vec<&RecipeDecl> = verb
            .recipe_order
            .iter()
            .filter_map(|name| self.content.recipe(&format!("{}.{}", verb.id, name)))
            .collect();
        candidates.sort_by_key(|r| (r.priority, r.decl_index));
        if let Some(via) = via {
            let Some(rec) = candidates.into_iter().find(|r| &r.id == via) else {
                return Selection::Nothing;
            };
            return self.adjudicate(rec, source, bindings, &ctx);
        }
        for rec in candidates {
            match self.adjudicate(rec, source, bindings, &ctx) {
                Selection::Nothing => continue,
                other => return other,
            }
        }
        Selection::Nothing
    }

    /// Eligibility, candidacy, `when`, and guards for one recipe under one
    /// source: the first eligible recipe whose `when` holds wins, and its
    /// guards are checked in declaration order before anything locks.
    fn adjudicate(
        &self,
        rec: &RecipeDecl,
        source: Source,
        bindings: Option<&[(SlotName, EntityId)]>,
        ctx: &Ctx,
    ) -> Selection {
        if !self.eligible(rec, source) || !self.max_left(rec) {
            return Selection::Nothing;
        }
        if !self.candidacy(rec, source, bindings, ctx) {
            return Selection::Nothing;
        }
        if let Some(w) = &rec.when {
            if !self.eval_cond_ctx(w, ctx) {
                return Selection::Nothing;
            }
        }
        for g in &rec.guards {
            if self.eval_cond_ctx(&g.when, ctx) {
                return Selection::GuardRefused(g.prose.clone());
            }
        }
        Selection::Matched(rec.clone())
    }

    /// Eligibility table: craftable/chain-only x PlayerCommit/Start/Poll
    /// (language.md §2.6).
    fn eligible(&self, rec: &RecipeDecl, source: Source) -> bool {
        match source {
            Source::Commit => rec.craftable && !rec.chain_only,
            Source::Start => true,
            Source::Poll => !rec.chain_only,
        }
    }

    /// The recipe's lifetime open-count cap: spent at materialization —
    /// adoption included, which materializes the new recipe in place.
    pub(crate) fn max_left(&self, rec: &RecipeDecl) -> bool {
        match rec.max {
            Some(m) => self.state.recipe_runs.get(&rec.id).copied().unwrap_or(0) < m,
            None => true,
        }
    }

    /// Candidacy. Player commits: explicit bindings must satisfy every slot
    /// minimum, match every filter, and respect every maximum; slots the
    /// recipe does not declare disqualify it. Engine sources: every slot
    /// minimum is satisfiable by greedy candidates counted in the current
    /// state — counted per slot, independently (the shared-pool competition
    /// is materialization's business, where a shortfall is `StartSkipped`).
    fn candidacy(
        &self,
        rec: &RecipeDecl,
        source: Source,
        bindings: Option<&[(SlotName, EntityId)]>,
        ctx: &Ctx,
    ) -> bool {
        match source {
            Source::Commit => {
                let Some(binds) = bindings else { return false };
                for (s, e) in binds {
                    if !rec.slots.iter().any(|sd| &sd.name == s) {
                        return false;
                    }
                    let sd = rec.slots.iter().find(|sd| &sd.name == s).unwrap();
                    if !self.matches_slot(&e, sd, ctx) {
                        return false;
                    }
                }
                for sd in &rec.slots {
                    let n = binds.iter().filter(|(s, _)| s == &sd.name).count() as u32;
                    if n < sd.min {
                        return false;
                    }
                    if let Some(max) = sd.max {
                        if n > max {
                            return false;
                        }
                    }
                }
                true
            }
            Source::Start | Source::Poll => {
                let pool = self.greedy_pool();
                rec.slots.iter().all(|sd| {
                    sd.min == 0
                        || pool
                            .iter()
                            .filter(|e| self.matches_slot(e, sd, ctx))
                            .count()
                            >= sd.min as usize
                })
            }
        }
    }

    /// Greedy candidate order (spec.md §4.4): unbound entities in bindable
    /// zones, by zone class (Table first), then public zones in declaration
    /// order, then mint order (oldest first).
    pub(crate) fn greedy_pool(&self) -> Vec<EntityId> {
        let mut pool: Vec<(u8, u32, EntityId)> = self
            .state
            .entities
            .values()
            .filter_map(|ent| {
                let z = self.state.zones.get(&ent.zone)?;
                if !z.public || !z.bindable || self.is_locked(ent.id) {
                    return None;
                }
                let class = match z.kind {
                    story_core::ZoneKind::Table => 0u8,
                    _ => 1u8,
                };
                let order = self.zone_order.get(&ent.zone).copied().unwrap_or(u32::MAX);
                Some((class, order, ent.id))
            })
            .collect();
        pool.sort_by_key(|(class, order, id)| (*class, *order, *id));
        pool.into_iter().map(|(_, _, e)| e).collect()
    }
}
