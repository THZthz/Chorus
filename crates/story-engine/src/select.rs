//! The selection law (spec.md §4.3): the winner of any recipe scan is
//! `min_by (priority, decl_index)` over the candidates whose `when` holds.
//! Candidacy: player commits fill slot minimums explicitly; starts and
//! autonomous polls count greedy candidates in the current state.

use crate::eval::Ctx;
use crate::Engine;
use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;
use story_content::ir::{RecipeDecl, VerbDecl};
use story_content::Content;
use story_core::{EntityId, RecipeId, SituationId, SlotName, VerbId};

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
    Matched(Arc<RecipeDecl>),
    /// No eligible recipe, or every `when` false.
    Nothing,
    /// The winner's guard refused; the scan stops here.
    GuardRefused(String),
}

#[derive(Clone, Debug)]
pub(crate) struct RecipeMatchInfo {
    pub recipe: Arc<RecipeDecl>,
    pub required_slots: BTreeSet<SlotName>,
    pub declared_slots: BTreeSet<SlotName>,
}

#[derive(Clone, Debug, Default)]
pub(crate) struct VerbRecipes {
    pub craftable: Vec<RecipeMatchInfo>,
    pub poll: Vec<RecipeMatchInfo>,
    pub all: Vec<RecipeMatchInfo>,
}

pub(crate) fn build_verb_recipes(content: &Content) -> BTreeMap<VerbId, VerbRecipes> {
    let mut out = BTreeMap::new();
    for verb in content.verbs.values() {
        let mut all = Vec::with_capacity(verb.recipe_order.len());
        for name in &verb.recipe_order {
            if let Some(r) = content.recipe(&format!("{}.{}", verb.id, name)) {
                let required_slots = r
                    .slots
                    .iter()
                    .filter(|s| s.min >= 1)
                    .map(|s| s.name.clone())
                    .collect();
                let declared_slots = r.slots.iter().map(|s| s.name.clone()).collect();
                all.push(RecipeMatchInfo {
                    recipe: Arc::new(r.clone()),
                    required_slots,
                    declared_slots,
                });
            }
        }
        all.sort_by_key(|info| (info.recipe.priority, info.recipe.decl_index));
        let poll = all
            .iter()
            .filter(|info| !info.recipe.chain_only)
            .cloned()
            .collect();
        let craftable = all
            .iter()
            .filter(|info| info.recipe.craftable && !info.recipe.chain_only)
            .cloned()
            .collect();
        out.insert(
            verb.id.clone(),
            VerbRecipes {
                craftable,
                poll,
                all,
            },
        );
    }
    out
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
        let mut query_cache: BTreeMap<(usize, EntityId), bool> = BTreeMap::new();
        if let Some(via) = via {
            let Some(rec) = self.content.recipe(via.as_str()) else {
                return Selection::Nothing;
            };
            return self.adjudicate(rec, source, bindings, &ctx, &mut query_cache, None);
        }

        let Some(vr) = self.verb_recipes.get(&verb.id) else {
            return Selection::Nothing;
        };

        let candidates = match source {
            Source::Commit => &vr.craftable,
            Source::Poll => &vr.poll,
            Source::Start => &vr.all,
        };

        let greedy_pool = match source {
            Source::Start | Source::Poll => Some(self.greedy_pool()),
            Source::Commit => None,
        };

        let bound_slots: Option<BTreeSet<&SlotName>> = if source == Source::Commit {
            bindings.map(|binds| binds.iter().map(|(s, _)| s).collect())
        } else {
            None
        };

        for info in candidates {
            if let Some(bound) = &bound_slots {
                if !bound.iter().all(|s| info.declared_slots.contains(*s)) {
                    continue;
                }
                if !info.required_slots.iter().all(|s| bound.contains(s)) {
                    continue;
                }
            }
            match self.adjudicate(
                &info.recipe,
                source,
                bindings,
                &ctx,
                &mut query_cache,
                greedy_pool.as_deref(),
            ) {
                Selection::Nothing => continue,
                Selection::GuardRefused(p) => return Selection::GuardRefused(p),
                Selection::Matched(_) => return Selection::Matched(info.recipe.clone()),
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
        query_cache: &mut BTreeMap<(usize, EntityId), bool>,
        greedy_pool: Option<&[EntityId]>,
    ) -> Selection {
        if !self.eligible(rec, source) || !self.max_left(rec) {
            return Selection::Nothing;
        }
        if !self.candidacy(rec, source, bindings, ctx, query_cache, greedy_pool) {
            return Selection::Nothing;
        }
        if let Some(w) = &rec.when {
            if !self.eval_cond_ctx(w, ctx) {
                return Selection::Nothing;
            }
        }
        // Guards gate player-facing commits and autonomous polls. A `start`
        // opens the situation regardless: the situation's own slots and
        // interior are necessarily empty at selection time, so any guard
        // over them would hold vacuously and silently kill the chain — the
        // content holds the open instead (an open slot for a mid-run bind,
        // a self-`continue` to re-arm the timer).
        if source != Source::Start {
            for g in &rec.guards {
                if self.eval_cond_ctx(&g.when, ctx) {
                    return Selection::GuardRefused(g.prose.clone());
                }
            }
        }
        Selection::Matched(Arc::new(rec.clone()))
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
        query_cache: &mut BTreeMap<(usize, EntityId), bool>,
        greedy_pool: Option<&[EntityId]>,
    ) -> bool {
        match source {
            Source::Commit => {
                let Some(binds) = bindings else { return false };
                for (s, e) in binds {
                    let Some(sd) = rec.slots.iter().find(|sd| &sd.name == s) else {
                        return false;
                    };
                    let key = (sd.query.as_ptr() as usize, *e);
                    let matched = match query_cache.get(&key) {
                        Some(&m) => m,
                        None => {
                            let m = self.matches_slot(e, sd, ctx);
                            query_cache.insert(key, m);
                            m
                        }
                    };
                    if !matched {
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
                let local_pool;
                let pool = match greedy_pool {
                    Some(p) => p,
                    None => {
                        local_pool = self.greedy_pool();
                        &local_pool
                    }
                };
                rec.slots.iter().all(|sd| {
                    if sd.min == 0 {
                        return true;
                    }
                    let mut count = 0;
                    for e in pool {
                        let key = (sd.query.as_ptr() as usize, *e);
                        let matched = match query_cache.get(&key) {
                            Some(&m) => m,
                            None => {
                                let m = self.matches_slot(e, sd, ctx);
                                query_cache.insert(key, m);
                                m
                            }
                        };
                        if matched {
                            count += 1;
                            if count >= sd.min as usize {
                                return true;
                            }
                        }
                    }
                    false
                })
            }
        }
    }

    /// Greedy candidate order (spec.md §4.4): unbound entities in bindable
    /// zones, by zone class (Table first), then public zones in declaration
    /// order, then mint order (oldest first).
    pub fn greedy_pool(&self) -> Vec<EntityId> {
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
