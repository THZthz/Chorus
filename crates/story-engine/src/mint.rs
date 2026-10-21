//! The mint pipeline, destruction, materialization, and the release path
//! (spec.md §4.2, §4.4, §4.5).

use crate::effects::EffectCtx;
use crate::{Engine, Event};
use story_content::ir::VerbDecl;
use story_content::ir::{CardDecl, Into as IntoZone, RecipeDecl};
use story_content::runtime::{DiagKind, RuntimeDiagnostic};
use story_core::{
    Binding, CardId, Entity, EntityId, MintPolicy, SituationId, SituationSource, SlotName, Zone,
    ZoneId, ZoneKind,
};

/// An effect failure: a diagnostic kind plus the reason carried to
/// `ResolutionFailed` (advance) or the returned diagnostic (commit).
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Fail {
    pub kind: DiagKind,
    pub reason: String,
}

impl Fail {
    pub(crate) fn new(kind: DiagKind, reason: impl Into<String>) -> Self {
        Fail {
            kind,
            reason: reason.into(),
        }
    }

    /// The reason text of a `ResolutionFailed` event.
    pub(crate) fn display_reason(&self) -> String {
        format!("{}: {}", self.kind.word(), self.reason)
    }

    /// The diagnostic a commit path returns for this failure.
    pub(crate) fn into_diagnostic(self) -> RuntimeDiagnostic {
        RuntimeDiagnostic::new(self.kind, self.reason)
    }
}

impl Engine {
    /// The only mint pipeline (spec.md §4.5): prototype template ->
    /// uniqueness -> assign zone -> allocate id -> arm expiry at the
    /// resolution's stepped now. A uniqueness skip is a `SpawnSkipped` event
    /// leaving no residue; a hard name collision is the `NameInUse` failure.
    pub(crate) fn mint(
        &mut self,
        card: &CardId,
        into: &IntoZone,
        fctx: &EffectCtx,
        ev: &mut Vec<Event>,
    ) -> Result<Option<EntityId>, Fail> {
        let decl = self.template(card);
        if self.mint_collision(&decl.mint, card) {
            if matches!(decl.mint, MintPolicy::SingleCopy) {
                return Err(Fail::new(
                    DiagKind::NameInUse,
                    format!("a live `{card}` blocks the mint"),
                ));
            }
            // `unique` and `group` mints skip instead of failing; a skipped
            // mint leaves no residue in the state.
            ev.push(Event::SpawnSkipped {
                name: card.to_string(),
            });
            return Ok(None);
        }
        let zone = self.zone_of_into(into, fctx.sit)?;
        let id = self.mint_body(&decl, card, zone, fctx, ev)?;
        Ok(Some(id))
    }

    /// Mint with a resolved zone (boot instances, swap successors).
    pub(crate) fn mint_body(
        &mut self,
        decl: &CardDecl,
        name: &CardId,
        zone: ZoneId,
        fctx: &EffectCtx,
        _ev: &mut Vec<Event>,
    ) -> Result<EntityId, Fail> {
        let id = EntityId(self.state.next_entity);
        self.state.next_entity += 1;
        let expires_at = decl.lifetime.map(|life| fctx.now.saturating_add(life));
        let entity = Entity {
            id,
            name: name.to_string(),
            prototype: name.clone(),
            kind: decl.kind.clone(),
            aspects: decl.aspects.clone(),
            attrs: Default::default(),
            zone,
            mint: decl.mint.clone(),
            expires_at,
        };
        self.state.entities.insert(id, entity);
        Ok(id)
    }

    /// Does a live entity block mints of this policy? A bound card counts as
    /// live (spec.md §5).
    pub(crate) fn mint_blocks(&self, mint: &MintPolicy, ent: &Entity) -> bool {
        match mint {
            MintPolicy::Stackable => false,
            MintPolicy::SingleCopy | MintPolicy::Unique => true,
            MintPolicy::Group(g) => ent.mint == MintPolicy::Group(g.clone()),
        }
    }

    /// Whether any entity live at this instant blocks a mint of `card` under
    /// its policy.
    pub(crate) fn mint_collision(&self, mint: &MintPolicy, card: &CardId) -> bool {
        self.state.entities.values().any(|ent| {
            let same = &ent.name == card.as_str();
            match mint {
                MintPolicy::Stackable => false,
                MintPolicy::SingleCopy | MintPolicy::Unique => same,
                MintPolicy::Group(g) => ent.mint == MintPolicy::Group(g.clone()),
            }
        })
    }

    /// The destination zone of an `into` clause. Card carriers create their
    /// `Of(entity)` zone on demand; the id assignment stays deterministic.
    pub(crate) fn zone_of_into(
        &mut self,
        into: &IntoZone,
        sit: Option<SituationId>,
    ) -> Result<ZoneId, Fail> {
        match into {
            IntoZone::Interior => match sit.and_then(|s| self.state.situations.get(&s)) {
                Some(s) => Ok(s.interior),
                None => Err(Fail::new(
                    DiagKind::EffectFailure,
                    "`into interior` outside a running situation",
                )),
            },
            IntoZone::Zone(name) => {
                self.zone_ids.get(name).copied().ok_or_else(|| {
                    Fail::new(DiagKind::UnknownZone, format!("unknown zone `{name}`"))
                })
            }
            IntoZone::Carrier(card) => {
                let Some(host) = self.named_entity(card.as_str()) else {
                    return Err(Fail::new(
                        DiagKind::EffectFailure,
                        format!("`into {card}` names no live card"),
                    ));
                };
                Ok(self.carrier_zone(host))
            }
        }
    }

    /// Find or create the carrier zone of one entity.
    pub(crate) fn carrier_zone(&mut self, host: EntityId) -> ZoneId {
        if let Some(z) = self
            .state
            .zones
            .values()
            .find(|z| z.kind == ZoneKind::Of(host))
            .map(|z| z.id)
        {
            return z;
        }
        let id = ZoneId(self.state.next_zone);
        self.state.next_zone += 1;
        self.state.zones.insert(
            id,
            Zone {
                id,
                kind: ZoneKind::Of(host),
                public: true,
                bindable: true,
            },
        );
        id
    }

    /// Remove a live entity: it drops every binding immediately (no
    /// dead-reference re-resolution), and its carrier zone, if any, releases
    /// its occupants to home (spec.md §4.4).
    pub(crate) fn destroy_entity(&mut self, e: EntityId) {
        for sit in self.state.situations.values_mut() {
            for binds in sit.bindings.values_mut() {
                binds.retain(|b| b.entity != e);
            }
        }
        self.state.entities.remove(&e);
        let carrier = self
            .state
            .zones
            .values()
            .find(|z| z.kind == ZoneKind::Of(e))
            .map(|z| z.id);
        if let Some(z) = carrier {
            let occupants = self.zone_entities(z);
            for o in occupants {
                if let Some(ent) = self.state.entities.get_mut(&o) {
                    ent.zone = self.home;
                }
            }
            self.state.zones.remove(&z);
        }
    }

    // -- materialization (spec.md §4.2) --------------------------------------

    /// One instance path for all three sources: mint the interior zone ->
    /// plan and apply the binds -> lock (derived) -> bump the recipe's run
    /// counter -> enter the due queue at `now + duration`. Engine sources
    /// auto-fill slot minimums from the shared greedy pool; a shortfall
    /// returns `Err(())` with nothing applied (`StartSkipped` upstream, no
    /// rollback of the parent resolution).
    pub(crate) fn materialize(
        &mut self,
        verb: &VerbDecl,
        rec: &RecipeDecl,
        source: SituationSource,
        explicit: &[(SlotName, EntityId)],
        ev: &mut Vec<Event>,
    ) -> Result<SituationId, ()> {
        let now = self.state.now;
        let sit_id = SituationId(self.state.next_situation);
        self.state.next_situation += 1;
        let zid = ZoneId(self.state.next_zone);
        self.state.next_zone += 1;
        self.state.zones.insert(
            zid,
            Zone {
                id: zid,
                kind: ZoneKind::SituationInterior(sit_id),
                public: false,
                bindable: false,
            },
        );

        // Plan every bind before applying anything: explicit first, then the
        // pulls per slot in declaration order, all drawing from one shared
        // candidate pool. Engine sources fill every slot minimum and carry
        // greedy slots to max; a minimum shortfall skips the open (nothing
        // applied). Player commits top greedy slots up toward max, best
        // effort — greedy never establishes candidacy and never satisfies a
        // minimum (spec.md §4.3, §4.4).
        let is_commit = matches!(source, SituationSource::Commit);
        let mut planned: Vec<(SlotName, EntityId)> = explicit.to_vec();
        let pool = self.greedy_pool();
        for sd in &rec.slots {
            let mut have = planned.iter().filter(|(s, _)| s == &sd.name).count() as u32;
            let want = if is_commit {
                if sd.greedy {
                    sd.max.unwrap_or(have)
                } else {
                    have
                }
            } else if sd.greedy {
                sd.max.unwrap_or(sd.min)
            } else {
                sd.min
            };
            while have < want {
                let pick = pool.iter().copied().find(|c| {
                    !planned.iter().any(|(_, e)| e == c) && {
                        let ctx = crate::eval::Ctx::default();
                        self.matches_slot(c, sd, &ctx)
                    }
                });
                match pick {
                    Some(c) => {
                        planned.push((sd.name.clone(), c));
                        have += 1;
                    }
                    None => {
                        if !is_commit && have < sd.min {
                            // Nothing applied; the open is skipped.
                            self.state.next_situation -= 1;
                            self.state.next_zone -= 1;
                            self.state.zones.remove(&zid);
                            return Err(());
                        }
                        break;
                    }
                }
            }
        }

        let mut bindings = std::collections::BTreeMap::new();
        for (seq, (slot, e)) in planned.iter().enumerate() {
            let bound_from = self.state.entities.get(e).map(|ent| ent.zone).ok_or(())?;
            let entity = self.state.entities.get_mut(e).ok_or(())?;
            entity.zone = zid;
            bindings
                .entry(slot.clone())
                .or_insert_with(Vec::new)
                .push(Binding {
                    entity: *e,
                    bound_from,
                    seq: seq as u64,
                });
            ev.push(Event::Bound {
                sit: sit_id,
                slot: slot.clone(),
                entity: *e,
            });
        }

        self.state
            .recipe_runs
            .entry(rec.id.clone())
            .and_modify(|n| *n += 1)
            .or_insert(1);
        let situation = story_core::Situation {
            id: sit_id,
            verb: verb.id.clone(),
            source,
            recipe: rec.id.clone(),
            interior: zid,
            bindings,
            started_at: now,
            ends_at: now.saturating_add(rec.duration),
        };
        self.state.situations.insert(sit_id, situation);
        Ok(sit_id)
    }

    // -- release (completion steps 4-5, spec.md §4.2) -------------------------

    /// Release every binding — `take` slots return cards to the zone each was
    /// bound from, the rest unlock in place — move interior leftovers to the
    /// verb's `releases` zone (default home), destroy the interior, and
    /// remove the instance.
    pub(crate) fn release_and_remove(&mut self, sit_id: SituationId) {
        let Some(sit) = self.state.situations.remove(&sit_id) else {
            return;
        };
        let rec = self.recipe_of(&sit.recipe);
        let interior = sit.interior;
        for sd in &rec.slots {
            if let Some(binds) = sit.bindings.get(&sd.name) {
                for b in binds {
                    if sd.take {
                        if let Some(ent) = self.state.entities.get_mut(&b.entity) {
                            if ent.zone == interior {
                                ent.zone = b.bound_from;
                            }
                        }
                    }
                }
            }
        }
        // Interior leftovers: everything still inside (non-take released
        // cards, spawned survivors) goes to the verb's releases zone, which
        // overrides home.
        let verb_id = rec.verb.clone();
        let target_zone = self
            .content
            .verbs
            .get(verb_id.as_str())
            .and_then(|v| v.releases.as_ref())
            .and_then(|name| self.zone_ids.get(name).copied())
            .unwrap_or(self.home);
        let leftovers = self.zone_entities(interior);
        for e in leftovers {
            if let Some(ent) = self.state.entities.get_mut(&e) {
                ent.zone = target_zone;
            }
        }
        self.state.zones.remove(&interior);
    }
}
