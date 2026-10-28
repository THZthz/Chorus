//! Presenter facade coordinating between mechanical Engine and presentation layer.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use story_content::ir::{InstanceCap, QueryTerm, RecipeDecl, VerbDecl, VerbMode};
use story_content::Content;
use story_core::{CardId, EntityId, SituationId, SlotName, Snapshot, State, Time, VerbId};
use story_engine::{CommitAction, Engine, Event, RuntimeDiagnostic};

use crate::catalog::PresentationCatalog;
use crate::interpolate::{interpolate, PresentationContext};
use crate::view::{
    BeatKind, BoardView, CardLoreView, CardSummaryView, EndingView, RecipeOpportunityView,
    SituationView, SlotPromptView, SlottedCardView, StagingSlotView, StagingView, StoryBeat,
    StoryOutput, VerbOpportunityView, ZoneView,
};

pub struct Presenter {
    engine: Engine,
    catalog: PresentationCatalog,
    /// Tracking active situation bindings so start & in-flight contexts know what is slotted
    active_bindings: BTreeMap<SituationId, BTreeMap<SlotName, Vec<CardId>>>,
}

impl Presenter {
    pub fn new(engine: Engine, catalog: PresentationCatalog) -> Self {
        let mut p = Self {
            engine,
            catalog,
            active_bindings: BTreeMap::new(),
        };
        p.sync_initial_bindings();
        p
    }

    fn sync_initial_bindings(&mut self) {
        self.active_bindings.clear();
        for (sit, s) in &self.engine.state().situations {
            let mut map: BTreeMap<SlotName, Vec<CardId>> = BTreeMap::new();
            for (slot, b_list) in &s.bindings {
                let cards = b_list
                    .iter()
                    .filter_map(|b| {
                        self.engine
                            .state()
                            .entities
                            .get(&b.entity)
                            .map(|e| e.prototype.clone())
                    })
                    .collect();
                map.insert(slot.clone(), cards);
            }
            self.active_bindings.insert(*sit, map);
        }
    }

    pub fn engine(&self) -> &Engine {
        &self.engine
    }

    pub fn engine_mut(&mut self) -> &mut Engine {
        &mut self.engine
    }

    pub fn state(&self) -> &State {
        self.engine.state()
    }

    pub fn content(&self) -> &Arc<Content> {
        self.engine.content()
    }

    pub fn catalog(&self) -> &PresentationCatalog {
        &self.catalog
    }

    pub fn catalog_mut(&mut self) -> &mut PresentationCatalog {
        &mut self.catalog
    }

    pub fn snapshot(&self) -> Snapshot {
        self.engine.snapshot()
    }

    pub fn restore(&mut self, snap: &Snapshot) -> Result<(), RuntimeDiagnostic> {
        self.engine.restore(snap)?;
        self.sync_initial_bindings();
        Ok(())
    }

    pub fn commit(&mut self, action: CommitAction) -> Result<StoryOutput, RuntimeDiagnostic> {
        let events = self.engine.commit(action)?;
        Ok(self.process_events(events))
    }

    pub fn advance(&mut self, dt: Time) -> Result<StoryOutput, RuntimeDiagnostic> {
        let events = self.engine.advance(dt)?;
        Ok(self.process_events(events))
    }

    pub fn bind(
        &mut self,
        sit: SituationId,
        slot: SlotName,
        entity: EntityId,
    ) -> Result<StoryOutput, RuntimeDiagnostic> {
        let events = self.engine.bind(sit, slot, entity)?;
        Ok(self.process_events(events))
    }

    /// Commit from a partially staged binding set: the matched recipe's
    /// `internal` slots with `min >= 1` are auto-staged deterministically
    /// (see `match_staged`), and the full set runs the ordinary commit. When
    /// nothing matches, the staged bindings alone go to the kernel so the
    /// caller receives its real refusal.
    pub fn commit_staged(
        &mut self,
        verb: &str,
        staged: &[(SlotName, EntityId)],
    ) -> Result<StoryOutput, RuntimeDiagnostic> {
        let bindings = match self.match_staged(verb, staged) {
            Some((_, full)) => full,
            None => staged.to_vec(),
        };
        self.commit(CommitAction {
            verb: VerbId::from(verb),
            bindings,
        })
    }

    /// The staging view for a host UI: what would this partial binding set
    /// open? Slots of the matched recipe that are shown under the full
    /// binding set, each with its bind candidates. When nothing matches, the
    /// view is empty.
    pub fn staging_view(&self, verb: &str, staged: &[(SlotName, EntityId)]) -> StagingView {
        let Some((recipe, full)) = self.match_staged(verb, staged) else {
            return StagingView {
                verb: verb.to_string(),
                matched: None,
                recipe_label: None,
                slots: Vec::new(),
            };
        };
        let content = self.engine.content();
        let qid = recipe.id.as_str();
        let mut slots = Vec::new();
        for s in &recipe.slots {
            if !self
                .engine
                .slot_shown_staged(recipe.id.clone(), s.name.clone(), &full)
            {
                continue;
            }
            let prompt = self.format_slot_prompt(qid, s);
            let candidates = self
                .engine
                .slot_candidates(recipe.id.clone(), s.name.clone())
                .into_iter()
                .map(|e| {
                    let title = self
                        .engine
                        .state()
                        .entities
                        .get(&e)
                        .map(|ent| {
                            self.catalog
                                .card_title(ent.prototype.as_str(), content)
                                .to_string()
                        })
                        .unwrap_or_else(|| e.to_string());
                    (e, title)
                })
                .collect();
            slots.push(StagingSlotView {
                name: s.name.clone(),
                label: prompt.label,
                bounds: prompt.bounds,
                flags: prompt.flags,
                criteria: prompt.criteria,
                candidates,
            });
        }
        StagingView {
            verb: verb.to_string(),
            matched: Some(recipe.id.clone()),
            recipe_label: Some(self.catalog.recipe_label(qid, &recipe).to_string()),
            slots,
        }
    }

    /// The shared staged matcher: craftable recipes of the verb in selection
    /// order (priority ascending, then declaration order — what `commit`
    /// selects by), each tried with the staged bindings plus deterministic
    /// auto-staging; the first whose `preview_commit` selects it wins.
    fn match_staged(
        &self,
        verb: &str,
        staged: &[(SlotName, EntityId)],
    ) -> Option<(RecipeDecl, Vec<(SlotName, EntityId)>)> {
        let v = self.content().verbs.get(verb)?;
        let mut recipes: Vec<&RecipeDecl> = v
            .recipes
            .values()
            .filter(|r| r.craftable && !r.chain_only)
            .collect();
        recipes.sort_by_key(|r| (r.priority, r.decl_index));
        for r in recipes {
            let full = self.auto_staged(r, staged);
            let action = CommitAction {
                verb: r.verb.clone(),
                bindings: full.clone(),
            };
            if self.engine.preview_commit(&action).as_ref() == Ok(&r.id) {
                return Some((r.clone(), full));
            }
        }
        None
    }

    /// Deterministic auto-staging for one recipe: each `internal` slot with
    /// `min >= 1` takes `slot_candidates` in order, skipping entities already
    /// used anywhere in the set, up to `min`.
    fn auto_staged(
        &self,
        recipe: &RecipeDecl,
        staged: &[(SlotName, EntityId)],
    ) -> Vec<(SlotName, EntityId)> {
        let mut full: Vec<(SlotName, EntityId)> = staged.to_vec();
        let mut used: BTreeSet<EntityId> = staged.iter().map(|(_, e)| *e).collect();
        for s in &recipe.slots {
            if !s.internal || s.min == 0 {
                continue;
            }
            let mut taken = 0;
            for c in self
                .engine
                .slot_candidates(recipe.id.clone(), s.name.clone())
            {
                if taken >= s.min as usize {
                    break;
                }
                if used.contains(&c) {
                    continue;
                }
                used.insert(c);
                full.push((s.name.clone(), c));
                taken += 1;
            }
        }
        full
    }

    fn process_events(&mut self, raw_events: Vec<Event>) -> StoryOutput {
        let mut beats = Vec::new();
        let now = self.engine.state().now;
        let content = self.engine.content().clone();

        for ev in &raw_events {
            if let Event::Bound {
                sit, slot, card, ..
            } = ev
            {
                self.active_bindings
                    .entry(*sit)
                    .or_default()
                    .entry(slot.clone())
                    .or_default()
                    .push(card.clone());
            }
        }

        for ev in &raw_events {
            match ev {
                Event::SituationStarted { sit, verb, recipe } => {
                    let ctx = PresentationContext {
                        sit: Some(*sit),
                        recipe: Some(recipe.clone()),
                        verb: Some(verb.clone()),
                        bound_cards: self.active_bindings.get(sit).cloned().unwrap_or_default(),
                        ..Default::default()
                    };
                    if let Some(rec) = content.recipe(recipe.as_str()) {
                        let qid = rec.id.as_str();
                        if rec.duration > 0 {
                            if let Some(start_prose) = self.catalog.recipe_start(qid, rec) {
                                let text = interpolate(
                                    start_prose,
                                    self.engine.state(),
                                    &content,
                                    &self.catalog,
                                    &ctx,
                                );
                                beats.push(StoryBeat {
                                    kind: BeatKind::ActionStart,
                                    text,
                                    clock: Some(now),
                                });
                            }
                        }
                    }
                }
                Event::Bound { .. } => {}
                Event::SituationCompleted {
                    sit,
                    recipe,
                    bindings,
                } => {
                    let ctx = PresentationContext {
                        sit: Some(*sit),
                        recipe: Some(recipe.clone()),
                        bound_cards: bindings.clone(),
                        ..Default::default()
                    };
                    self.active_bindings.remove(sit);

                    if let Some(rec) = content.recipe(recipe.as_str()) {
                        let qid = rec.id.as_str();
                        if let Some(comp_prose) = self.catalog.recipe_completion(qid, rec) {
                            let text = interpolate(
                                comp_prose,
                                self.engine.state(),
                                &content,
                                &self.catalog,
                                &ctx,
                            );
                            beats.push(StoryBeat {
                                kind: BeatKind::ActionComplete,
                                text,
                                clock: Some(now),
                            });
                        }
                    }
                }
                Event::ResolutionFailed { sit, .. } => {
                    self.active_bindings.remove(sit);
                }
                Event::Continued { sit: _, recipe } => {
                    if let Some(rec) = content.recipe(recipe.as_str()) {
                        let qid = rec.id.as_str();
                        let label = self.catalog.recipe_label(qid, rec);
                        beats.push(StoryBeat {
                            kind: BeatKind::ActionContinue,
                            text: format!("Continues via {label}"),
                            clock: Some(now),
                        });
                    }
                }
                Event::NarrativeCue { cue_id } => {
                    let prose = self.catalog.cue_prose(cue_id);
                    let ctx = PresentationContext::default();
                    let text =
                        interpolate(prose, self.engine.state(), &content, &self.catalog, &ctx);
                    beats.push(StoryBeat {
                        kind: BeatKind::NarrativeCue,
                        text,
                        clock: Some(now),
                    });
                }
                Event::RuleFired { rule_id } => {
                    let prose = self.catalog.cue_prose(rule_id);
                    if prose != rule_id {
                        let ctx = PresentationContext::default();
                        let text =
                            interpolate(prose, self.engine.state(), &content, &self.catalog, &ctx);
                        beats.push(StoryBeat {
                            kind: BeatKind::RuleTriggered,
                            text,
                            clock: Some(now),
                        });
                    }
                }
                Event::Ended { ending } => {
                    let end_decl = content.endings.get(ending.as_str());
                    let (title, tier, text_opt) =
                        self.catalog.ending_prose(ending.as_str(), end_decl);
                    let mut epilogue = format!("Ending: \"{title}\" ({tier})");
                    if let Some(t) = text_opt {
                        let ctx = PresentationContext::default();
                        let body =
                            interpolate(t, self.engine.state(), &content, &self.catalog, &ctx);
                        epilogue.push_str(&format!("\n{body}"));
                    }
                    beats.push(StoryBeat {
                        kind: BeatKind::Ending,
                        text: epilogue,
                        clock: Some(now),
                    });
                }
                _ => {}
            }
        }

        StoryOutput { beats, raw_events }
    }

    pub fn board_view(&self) -> BoardView {
        let state = self.engine.state();
        let content = self.engine.content();
        let clock = state.now;
        let legacy = state.legacy.clone();

        let locked_entities: BTreeSet<EntityId> = state
            .situations
            .values()
            .flat_map(|s| s.bindings.values().flatten().map(|b| b.entity))
            .collect();

        let mut zones = Vec::new();
        for (z_name, &zid) in self.engine.zone_ids() {
            let zone_obj = state.zones.get(&zid);
            let kind = zone_obj
                .map(|z| match z.kind {
                    story_core::ZoneKind::Table => "table".to_string(),
                    story_core::ZoneKind::Hidden => "hidden".to_string(),
                    story_core::ZoneKind::SituationInterior(sit) => format!("interior({sit})"),
                    story_core::ZoneKind::Of(ent) => format!("of({ent})"),
                })
                .unwrap_or_else(|| "zone".to_string());

            let mut card_map: BTreeMap<String, CardSummaryView> = BTreeMap::new();
            for ent in state.entities.values().filter(|e| e.zone == zid) {
                let proto = ent.prototype.as_str();
                let title = self.catalog.card_title(proto, content).to_string();
                let is_locked = locked_entities.contains(&ent.id);
                if let Some(existing) = card_map.get_mut(proto) {
                    existing.count += 1;
                    existing.entity_ids.push(ent.id);
                    if is_locked {
                        existing.locked_ids.push(ent.id);
                    }
                } else {
                    card_map.insert(
                        proto.to_string(),
                        CardSummaryView {
                            entity_id: ent.id,
                            entity_ids: vec![ent.id],
                            locked_ids: if is_locked { vec![ent.id] } else { Vec::new() },
                            prototype: proto.to_string(),
                            title,
                            kind: ent.kind.clone(),
                            count: 1,
                            aspects: ent.aspects.clone(),
                            expires_at: ent.expires_at,
                        },
                    );
                }
            }

            zones.push(ZoneView {
                zone_id: zid,
                name: z_name.clone(),
                kind,
                cards: card_map.into_values().collect(),
            });
        }

        let mut situations = Vec::new();
        for s in state.situations.values() {
            let verb_title = self
                .catalog
                .verb_title(s.verb.as_str(), content)
                .to_string();
            let rec_decl = content.recipe(s.recipe.as_str());
            let qid = rec_decl.map(|r| r.id.as_str()).unwrap_or(s.recipe.as_str());
            let recipe_label = rec_decl
                .map(|r| self.catalog.recipe_label(qid, r).to_string())
                .unwrap_or_else(|| s.recipe.to_string());

            let mut slotted = Vec::new();
            for (slot, b_list) in &s.bindings {
                for b in b_list {
                    if let Some(ent) = state.entities.get(&b.entity) {
                        let c_title = self
                            .catalog
                            .card_title(ent.prototype.as_str(), content)
                            .to_string();
                        slotted.push(SlottedCardView {
                            slot: slot.to_string(),
                            entity_id: ent.id,
                            title: c_title,
                            prototype: ent.prototype.to_string(),
                        });
                    }
                }
            }

            let mut open_slots = Vec::new();
            if let Some(rec) = rec_decl {
                for sl in &rec.slots {
                    if !sl.open {
                        continue;
                    }
                    // Presence gate: internal slots and slots whose `shown
                    // when` does not hold (or that resolve to nothing) are
                    // not prompted; their bound cards still show in `slotted`.
                    if !self
                        .engine
                        .slot_shown(s.id, sl.name.clone())
                        .unwrap_or(false)
                    {
                        continue;
                    }
                    open_slots.push(self.format_slot_prompt(qid, sl));
                }
            }

            let total_dur = rec_decl.map(|r| r.duration).unwrap_or(0);
            let remaining = s.ends_at.saturating_sub(clock);
            let start_text = rec_decl
                .and_then(|r| self.catalog.recipe_start(qid, r))
                .map(|p| {
                    let ctx = PresentationContext {
                        sit: Some(s.id),
                        recipe: Some(s.recipe.clone()),
                        verb: Some(s.verb.clone()),
                        bound_cards: self.active_bindings.get(&s.id).cloned().unwrap_or_default(),
                        ..Default::default()
                    };
                    interpolate(p, state, content, &self.catalog, &ctx)
                });

            situations.push(SituationView {
                id: s.id,
                verb: s.verb.to_string(),
                verb_title,
                recipe: s.recipe.to_string(),
                recipe_label,
                started_at: s.started_at,
                ends_at: s.ends_at,
                remaining_time: remaining,
                total_duration: total_dur,
                start_text,
                slotted,
                open_slots,
            });
        }

        let ended = state.ended.as_ref().map(|eid| {
            let end_decl = content.endings.get(eid.as_str());
            let (title, tier, text) = self.catalog.ending_prose(eid.as_str(), end_decl);
            let text = text.map(|t| {
                let ctx = PresentationContext::default();
                interpolate(t, state, &content, &self.catalog, &ctx)
            });
            EndingView {
                id: eid.to_string(),
                title: title.to_string(),
                tier: tier.to_string(),
                text,
            }
        });

        BoardView {
            clock,
            legacy,
            zones,
            situations,
            ended,
        }
    }

    pub fn verbs_view(&self, filter_runnable: bool) -> Vec<VerbOpportunityView> {
        let content = self.engine.content();
        let mut views = Vec::new();

        for verb in content.verbs.values() {
            let v_title = self
                .catalog
                .verb_title(verb.id.as_str(), content)
                .to_string();
            let v_desc = self
                .catalog
                .verb_desc(verb.id.as_str(), content)
                .map(String::from);
            let mode = match verb.mode {
                VerbMode::Committable => "committable",
                VerbMode::Autonomous => "autonomous",
                VerbMode::Uncommittable => "uncommittable",
            }
            .to_string();
            let instances = match verb.instances {
                InstanceCap::Any => "any".to_string(),
                InstanceCap::AtMost(n) => n.to_string(),
            };

            let mut recipes = Vec::new();
            for rname in &verb.recipe_order {
                if let Some(r) = verb.recipes.get(rname) {
                    let qid = format!("{}.{}", verb.id, r.name);
                    let r_label = self.catalog.recipe_label(&qid, r).to_string();
                    let runnable = self.is_recipe_runnable(verb, r);

                    if filter_runnable && !runnable {
                        continue;
                    }

                    let slots = r
                        .slots
                        .iter()
                        .map(|sl| self.format_slot_prompt(&qid, sl))
                        .collect();
                    let guards = r.guards.iter().map(|g| g.prose.clone()).collect();

                    recipes.push(RecipeOpportunityView {
                        id: r.id.to_string(),
                        name: r.name.clone(),
                        label: r_label,
                        priority: r.priority,
                        duration: r.duration,
                        craftable: r.craftable,
                        chain_only: r.chain_only,
                        runnable,
                        slots,
                        guards,
                    });
                }
            }

            if !recipes.is_empty() || !filter_runnable {
                views.push(VerbOpportunityView {
                    id: verb.id.to_string(),
                    title: v_title,
                    mode,
                    instances,
                    description: v_desc,
                    recipes,
                });
            }
        }

        views
    }

    pub fn inspect_card(&self, query: &str) -> Option<CardLoreView> {
        let state = self.engine.state();
        let content = self.engine.content();

        // Support `#<id>`, `name#<id>`, or `name`
        let entity_opt = if let Some(id_str) = query
            .strip_prefix('#')
            .or_else(|| query.split_once('#').map(|(_, id)| id))
        {
            id_str
                .parse::<u64>()
                .ok()
                .and_then(|id| state.entities.get(&story_core::EntityId(id)))
        } else {
            // Try matching entity by prototype or name
            state
                .entities
                .values()
                .find(|e| e.prototype.as_str() == query || e.name == query)
        };

        let proto_hint = query.split_once('#').map(|(p, _)| p).unwrap_or(query);
        let proto = entity_opt
            .map(|e| e.prototype.as_str())
            .unwrap_or(proto_hint);
        let card_decl = content.cards.get(proto)?;

        let title = self.catalog.card_title(proto, content).to_string();
        let description = self.catalog.card_desc(proto, content).map(String::from);
        let aspects = entity_opt.map(|e| e.aspects.clone()).unwrap_or_else(|| {
            card_decl
                .aspects
                .iter()
                .map(|(k, v)| (k.to_string(), *v))
                .collect()
        });

        let mut relations = Vec::new();
        if let Some(ent) = entity_opt {
            if let Some(kinds) = state.relations.get(&ent.id) {
                for (kind, targets) in kinds {
                    for target_id in targets {
                        if let Some(tgt_ent) = state.entities.get(target_id) {
                            let t_title = self
                                .catalog
                                .card_title(tgt_ent.prototype.as_str(), content)
                                .to_string();
                            relations.push((kind.clone(), t_title, Some(*target_id)));
                        }
                    }
                }
            }
        }

        Some(CardLoreView {
            entity_id: entity_opt.map(|e| e.id),
            prototype: proto.to_string(),
            title,
            kind: card_decl.kind.clone(),
            description,
            aspects,
            expires_at: entity_opt.and_then(|e| e.expires_at),
            lifetime: card_decl.lifetime,
            relations,
        })
    }

    fn format_slot_prompt(&self, qid: &str, slot: &story_content::ir::SlotDecl) -> SlotPromptView {
        let s_name = slot.name.as_str();
        let label = self.catalog.slot_label(qid, s_name).to_string();
        let description = self.catalog.slot_desc(qid, s_name).map(String::from);
        let bounds = match slot.max {
            Some(max) => format!("{}..{}", slot.min, max),
            None => format!("{}..", slot.min),
        };
        let mut flags = Vec::new();
        if slot.consuming {
            flags.push("consuming");
        }
        if slot.open {
            flags.push("open");
        }
        if slot.greedy {
            flags.push("greedy");
        }
        if slot.internal {
            flags.push("internal");
        }
        let flags_str = flags.join(" ");

        let criteria = if slot.query.is_empty() {
            "any".to_string()
        } else {
            slot.query
                .iter()
                .map(format_query_term)
                .collect::<Vec<_>>()
                .join(" ")
        };

        SlotPromptView {
            name: s_name.to_string(),
            label,
            bounds,
            flags: flags_str,
            description,
            criteria,
        }
    }

    fn is_verb_full(&self, verb: &VerbDecl) -> bool {
        match verb.instances {
            InstanceCap::Any => false,
            InstanceCap::AtMost(n) => {
                self.engine
                    .state()
                    .situations
                    .values()
                    .filter(|s| s.verb == verb.id)
                    .count()
                    >= n as usize
            }
        }
    }

    fn can_recipe_commit(&self, verb: &VerbDecl, recipe: &RecipeDecl) -> bool {
        if self.engine.state().ended.is_some() {
            return false;
        }
        if self.is_verb_full(verb) {
            return false;
        }
        if let Some(m) = recipe.max {
            if self
                .engine
                .state()
                .recipe_runs
                .get(&recipe.id)
                .copied()
                .unwrap_or(0)
                >= m
            {
                return false;
            }
        }
        if recipe.chain_only || !recipe.craftable {
            return false;
        }

        if recipe.slots.is_empty() {
            let action = CommitAction {
                verb: verb.id.clone(),
                bindings: Vec::new(),
            };
            return self.engine.preview_commit(&action).as_ref() == Ok(&recipe.id);
        }

        let mut req_slots: Vec<(&SlotName, usize, Vec<EntityId>)> = Vec::new();
        let mut opt_slots: Vec<(&SlotName, Vec<EntityId>)> = Vec::new();
        let mut total_req_min = 0;
        let mut all_req_cands = BTreeSet::new();

        for s in &recipe.slots {
            let cands = self
                .engine
                .slot_candidates(recipe.id.clone(), s.name.clone());
            if s.min > 0 {
                let min = s.min as usize;
                if cands.len() < min {
                    return false;
                }
                total_req_min += min;
                all_req_cands.extend(cands.iter().copied());
                req_slots.push((&s.name, min, cands));
            } else if !cands.is_empty() {
                opt_slots.push((&s.name, cands));
            }
        }

        if all_req_cands.len() < total_req_min {
            return false;
        }

        let mut used = BTreeSet::new();
        let mut current_bindings = Vec::new();

        self.search_required_bindings(
            &verb.id,
            &recipe.id,
            &req_slots,
            0,
            &opt_slots,
            &mut used,
            &mut current_bindings,
        )
    }

    fn search_required_bindings(
        &self,
        verb_id: &story_core::VerbId,
        recipe_id: &story_core::RecipeId,
        req_slots: &[(&SlotName, usize, Vec<EntityId>)],
        slot_idx: usize,
        opt_slots: &[(&SlotName, Vec<EntityId>)],
        used: &mut BTreeSet<EntityId>,
        current_bindings: &mut Vec<(SlotName, EntityId)>,
    ) -> bool {
        if slot_idx == req_slots.len() {
            let action = CommitAction {
                verb: verb_id.clone(),
                bindings: current_bindings.clone(),
            };
            if self.engine.preview_commit(&action).as_ref() == Ok(recipe_id) {
                return true;
            }

            for (opt_name, cands) in opt_slots {
                for &cand in cands {
                    if used.contains(&cand) {
                        continue;
                    }
                    current_bindings.push(((*opt_name).clone(), cand));
                    let opt_action = CommitAction {
                        verb: verb_id.clone(),
                        bindings: current_bindings.clone(),
                    };
                    let matches = self.engine.preview_commit(&opt_action).as_ref() == Ok(recipe_id);
                    current_bindings.pop();
                    if matches {
                        return true;
                    }
                }
            }
            return false;
        }

        let (slot_name, min, cands) = &req_slots[slot_idx];
        if *min == 1 {
            for &cand in cands {
                if used.contains(&cand) {
                    continue;
                }
                used.insert(cand);
                current_bindings.push(((*slot_name).clone(), cand));
                if self.search_required_bindings(
                    verb_id,
                    recipe_id,
                    req_slots,
                    slot_idx + 1,
                    opt_slots,
                    used,
                    current_bindings,
                ) {
                    return true;
                }
                current_bindings.pop();
                used.remove(&cand);
            }
        } else {
            let avail: Vec<EntityId> = cands
                .iter()
                .copied()
                .filter(|e| !used.contains(e))
                .collect();
            if avail.len() < *min {
                return false;
            }
            for &e in &avail[..*min] {
                used.insert(e);
                current_bindings.push(((*slot_name).clone(), e));
            }
            let found = self.search_required_bindings(
                verb_id,
                recipe_id,
                req_slots,
                slot_idx + 1,
                opt_slots,
                used,
                current_bindings,
            );
            for &e in &avail[..*min] {
                used.remove(&e);
                current_bindings.pop();
            }
            if found {
                return true;
            }
        }

        false
    }

    fn is_recipe_runnable(&self, verb: &VerbDecl, recipe: &RecipeDecl) -> bool {
        match verb.mode {
            VerbMode::Uncommittable => false,
            VerbMode::Autonomous => {
                if self.engine.state().ended.is_some() {
                    return false;
                }
                if self.is_verb_full(verb) {
                    return false;
                }
                if recipe.chain_only {
                    return false;
                }
                if let Some(m) = recipe.max {
                    if self
                        .engine
                        .state()
                        .recipe_runs
                        .get(&recipe.id)
                        .copied()
                        .unwrap_or(0)
                        >= m
                    {
                        return false;
                    }
                }
                if let Some(w) = &recipe.when {
                    if !self.engine.eval_cond(w) {
                        return false;
                    }
                }
                for g in &recipe.guards {
                    if self.engine.eval_cond(&g.when) {
                        return false;
                    }
                }
                for s in &recipe.slots {
                    if s.min > 0 {
                        let cands = self
                            .engine
                            .slot_candidates(recipe.id.clone(), s.name.clone());
                        if cands.len() < s.min as usize {
                            return false;
                        }
                    }
                }
                true
            }
            VerbMode::Committable => self.can_recipe_commit(verb, recipe),
        }
    }
}

fn format_query_term(term: &QueryTerm) -> String {
    match term {
        QueryTerm::Aspect(a) => match a.op {
            Some(op) => format!("aspect:{}{}{}", a.key, format_cmp(op), a.value),
            None => format!("aspect:{}", a.key),
        },
        QueryTerm::Name(id) => format!("name:{id}"),
        QueryTerm::Kind(k) => format!("kind:{k}"),
        QueryTerm::Not(inner) => format!("not({})", format_query_term(inner)),
        QueryTerm::Pred(_) => "predicate".to_string(),
    }
}

fn format_cmp(op: story_content::ir::CmpOp) -> &'static str {
    match op {
        story_content::ir::CmpOp::Ge => ">=",
        story_content::ir::CmpOp::Le => "<=",
        story_content::ir::CmpOp::Gt => ">",
        story_content::ir::CmpOp::Lt => "<",
        story_content::ir::CmpOp::Eq => "==",
        story_content::ir::CmpOp::Ne => "!=",
    }
}
