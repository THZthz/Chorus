//! The kernel (spec.md §3): selection law, settlement, slots, mint pipeline,
//! effect registry, trigger books, snapshots. `core <- content <- engine`:
//! the engine never depends on the compiler.

pub mod effects;
pub mod eval;
mod mint;
pub mod select;
mod settle;

use eval::Ctx;
use select::Source;
use settle::Run;
use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;
use story_content::ir::{InstanceCap, RecipeDecl, SlotDecl, VerbDecl, VerbMode};
use story_content::runtime::{DiagKind, RuntimeDiagnostic};
use story_content::Content;
use story_core::{
    Binding, CardId, DeckId, EntityId, RecipeId, Situation, SituationId, SituationSource, SlotName,
    State, Time, VerbId, ZoneId, FORMAT_VERSION,
};

pub use effects::{Effect, EffectCtx, EffectRegistry, RegistryEntry};
pub use story_core::Snapshot;

/// Resolutions allowed in one `advance` (or one `commit`) call; exceeding it
/// rolls the whole call back (kernel-api.md §6).
pub const MAX_RESOLUTIONS_PER_ADVANCE: u64 = 4096;

/// Native recursion depth allowed inside one resolution (inline instant
/// completion chains, signal cycles); exceeding it takes the Livelock
/// rollback path instead of exhausting the stack (kernel-api.md §6).
pub const MAX_RESOLUTION_DEPTH: u32 = 128;

// ---------------------------------------------------------------------------
// The event stream (kernel-api.md §3)
// ---------------------------------------------------------------------------

/// Why an open was skipped (a `start` or an autonomous poll).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum StartSkipReason {
    InstanceCap,
    MaterializationFailed,
}

/// The v1 event set; the host renders (L6).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Event {
    /// Renderable prose.
    Narration { text: String },
    /// A card joined a slot.
    Bound {
        sit: SituationId,
        slot: SlotName,
        entity: EntityId,
    },
    /// An instance finished its completion sequence.
    SituationCompleted { sit: SituationId, recipe: RecipeId },
    /// A situation adopted a `continue via` target instead of completing:
    /// the same instance goes on executing the target recipe with its timer
    /// re-armed (spec.md §4.2).
    Continued { situation: SituationId, recipe: RecipeId },
    /// A start chain ended silently: no eligible recipe, every `when` false,
    /// or the winner's guard refused.
    ChainEnded { verb: VerbId },
    /// An open was skipped: instance cap or materialization shortfall.
    StartSkipped {
        verb: VerbId,
        reason: StartSkipReason,
    },
    /// A mint was skipped by uniqueness policy.
    SpawnSkipped { name: String },
    /// A resolution unit failed and rolled back. On the advance path the
    /// unit's situation aborts through the release path; in a checkpoint the
    /// failing verb's open rolls back alone. Advance traffic, not a
    /// Diagnostic.
    ResolutionFailed { sit: SituationId, reason: String },
}

/// A player's open request: the verb and the explicit slot bindings.
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct CommitAction {
    pub verb: VerbId,
    pub bindings: Vec<(SlotName, EntityId)>,
}

// ---------------------------------------------------------------------------
// The engine facade (kernel-api.md §1)
// ---------------------------------------------------------------------------

pub struct Engine {
    pub(crate) content: Arc<Content>,
    pub(crate) registry: EffectRegistry,
    pub(crate) state: State,
    pub(crate) seed: u64,
    /// Declared-zone addresses, assigned in declaration order at boot.
    pub(crate) zone_ids: BTreeMap<String, ZoneId>,
    /// Declared-zone declaration order (greedy candidate ordering).
    pub(crate) zone_order: BTreeMap<ZoneId, u32>,
    /// Declared-zone nesting (a zone scope includes its subtree).
    pub(crate) zone_parent: BTreeMap<ZoneId, Option<ZoneId>>,
    pub(crate) home: ZoneId,
    pub(crate) turn: i64,
    /// Per-deck deal permutations, seeded at boot; `State` carries cursors.
    pub(crate) deck_orders: BTreeMap<DeckId, Vec<CardId>>,
}

impl Engine {
    /// Boot from compiled content: validate the seed, fold legacy over the
    /// declared defaults, arm boot instances at t=0, and run one autonomous
    /// checkpoint. Diagnostics split into warnings (play may proceed) and
    /// errors (the host must not proceed).
    pub fn boot(
        content: Arc<Content>,
        registry: EffectRegistry,
        seed: u64,
        legacy_in: Option<BTreeMap<String, i64>>,
    ) -> (Self, Vec<RuntimeDiagnostic>) {
        settle::boot(content, registry, seed, legacy_in)
    }

    // -- recognition face ---------------------------------------------------

    pub fn state(&self) -> &State {
        &self.state
    }

    /// The content database behind this engine.
    pub fn content(&self) -> &Arc<Content> {
        &self.content
    }

    /// The minimum advance unit in minutes.
    pub fn turn(&self) -> Time {
        self.turn
    }

    /// Run the commit selection pipeline — eligibility, minimum-fill,
    /// when-scan, guard — without mutating state or spending time.
    pub fn preview_commit(&self, action: &CommitAction) -> Result<RecipeId, RuntimeDiagnostic> {
        if self.state.ended.is_some() {
            return Err(DiagKind::StoryEnded.refused("the story has ended"));
        }
        let verb = self.verb(&action.verb)?;
        if verb.mode != VerbMode::Committable {
            return Err(DiagKind::VerbAutonomous.in_context(
                verb.id.to_string(),
                "only committable verbs accept player commits",
            ));
        }
        if self.verb_full(verb) {
            return Err(DiagKind::InstanceCapReached
                .in_context(verb.id.to_string(), "the verb's instance cap is full"));
        }
        self.validate_commit_bindings(&verb, &action.bindings)?;
        match self.select(Source::Commit, &verb, None, Some(&action.bindings), None) {
            select::Selection::Matched(rec) => Ok(rec.id.clone()),
            select::Selection::GuardRefused(prose) => {
                Err(DiagKind::GuardRefused.in_context(action.verb.to_string(), prose))
            }
            select::Selection::Nothing => Err(DiagKind::NoRecipe.in_context(
                action.verb.to_string(),
                "no craftable recipe of this verb accepts these bindings right now",
            )),
        }
    }

    /// Bind eligibility for one open slot of one live situation.
    pub fn can_bind(
        &self,
        sit: SituationId,
        slot: SlotName,
        e: EntityId,
    ) -> Result<(), RuntimeDiagnostic> {
        let situation = self.situation(sit)?;
        let rec = self.recipe_of(&situation.recipe);
        let Some(sd) = rec.slots.iter().find(|s| s.name == slot) else {
            return Err(
                DiagKind::UnresolvableRef.refused(format!("situation {sit} has no slot `{slot}`"))
            );
        };
        let mut d = self.check_bindable(sit, &sd, e);
        if let Err(dg) = d.as_mut() {
            dg.context = format!("situation {sit}");
        }
        d.map(|_| ())
    }

    /// Greedy-ordered bind candidates for one recipe slot: alive, unbound, in
    /// bindable zones, filter-matching (kernel-api.md §8).
    pub fn slot_candidates(&self, recipe: RecipeId, slot: SlotName) -> Vec<EntityId> {
        let Some(rec) = self.content.recipe(recipe.as_str()) else {
            return Vec::new();
        };
        let Some(sd) = rec.slots.iter().find(|s| s.name == slot) else {
            return Vec::new();
        };
        let ctx = Ctx::default();
        self.greedy_pool()
            .into_iter()
            .filter(|e| self.matches_slot(e, sd, &ctx))
            .collect()
    }

    // -- actions ------------------------------------------------------------

    /// Open a new instance of a verb. All-or-nothing: a refusal or an effect
    /// failure leaves the state bit-identical and spends zero time (T3).
    pub fn commit(&mut self, action: CommitAction) -> Result<Vec<Event>, RuntimeDiagnostic> {
        if self.state.ended.is_some() {
            return Err(DiagKind::StoryEnded.refused("the story has ended"));
        }
        let verb = self.verb(&action.verb)?.clone();
        if verb.mode != VerbMode::Committable {
            return Err(DiagKind::VerbAutonomous.in_context(
                verb.id.to_string(),
                "only committable verbs accept player commits",
            ));
        }
        if self.verb_full(&verb) {
            return Err(DiagKind::InstanceCapReached
                .in_context(verb.id.to_string(), "the verb's instance cap is full"));
        }
        self.validate_commit_bindings(&verb, &action.bindings)?;
        let rec = match self.select(Source::Commit, &verb, None, Some(&action.bindings), None) {
            select::Selection::Matched(rec) => rec,
            select::Selection::GuardRefused(prose) => {
                return Err(DiagKind::GuardRefused.in_context(verb.id.to_string(), prose));
            }
            select::Selection::Nothing => {
                return Err(DiagKind::NoRecipe.in_context(
                    verb.id.to_string(),
                    "no craftable recipe of this verb accepts these bindings right now",
                ));
            }
        };
        let mut run = Run::new(self.state.clone());
        let mut ev = Vec::new();
        // Candidacy checked every slot minimum against the submitted
        // bindings, so a shortfall here is unreachable; it returns a
        // diagnostic all the same — never a panic.
        let sit = match self.materialize(
            &verb,
            &rec,
            SituationSource::Commit,
            &action.bindings,
            &mut ev,
        ) {
            Ok(sit) => sit,
            Err(()) => {
                self.state = run.entry.clone();
                return Err(DiagKind::SlotUnderMin.in_context(
                    verb.id.to_string(),
                    format!("recipe `{}` could not fill its slot minimums", rec.id),
                ));
            }
        };
        if rec.duration == 0 {
            // The instant recipe completes inline inside this action
            // (spec.md §4.2): the open and completion events return together,
            // and the whole open-complete cycle sits inside the commit's
            // atomic action.
            let charged = self
                .charge(&mut run)
                .and_then(|_| self.completion_steps(sit, &mut ev, &mut run));
            if let Err(f) = charged {
                // The refusal returns to the caller: zero state change, zero
                // time (the entry checkpoint restores bit-for-bit, dice
                // included).
                self.state = run.entry.clone();
                return Err(f.into_diagnostic());
            }
        }
        Ok(ev)
    }

    /// Add a card to an open slot of a live situation: no re-selection, no
    /// clock movement (spec.md §4.4).
    pub fn bind(
        &mut self,
        sit: SituationId,
        slot: SlotName,
        e: EntityId,
    ) -> Result<Vec<Event>, RuntimeDiagnostic> {
        if self.state.ended.is_some() {
            return Err(DiagKind::StoryEnded.refused("the story has ended"));
        }
        {
            let situation = self.situation(sit)?;
            let rec = self.recipe_of(&situation.recipe);
            let Some(sd) = rec.slots.iter().find(|s| s.name == slot) else {
                return Err(DiagKind::UnresolvableRef
                    .refused(format!("situation {sit} has no slot `{slot}`")));
            };
            self.check_bindable(sit, &sd, e)?;
        }
        let seq = bind_count(&self.state, sit);
        let bound_from = self.state.entities[&e].zone;
        let interior = self.state.situations[&sit].interior;
        let entity = self.state.entities.get_mut(&e).expect("checked alive");
        entity.zone = interior;
        let situation = self.state.situations.get_mut(&sit).expect("checked live");
        situation
            .bindings
            .entry(slot.clone())
            .or_default()
            .push(Binding {
                entity: e,
                bound_from,
                seq,
            });
        Ok(vec![Event::Bound {
            sit,
            slot,
            entity: e,
        }])
    }

    /// Settle everything due within `dt` (a positive multiple of the
    /// content's `turn`): completions, expiries, and autonomous checkpoints
    /// under the total-order key `(moment, phase, id)` (kernel-api.md §6).
    pub fn advance(&mut self, dt: Time) -> Result<Vec<Event>, RuntimeDiagnostic> {
        if self.state.ended.is_some() {
            return Err(DiagKind::StoryEnded.refused("the story has ended"));
        }
        if dt <= 0 || dt % self.turn != 0 {
            return Err(DiagKind::BadDuration.refused(format!(
                "advance duration {dt} must be a positive multiple of turn {}",
                self.turn
            )));
        }
        settle::advance(self, dt)
    }

    // -- persistence face ---------------------------------------------------

    pub fn legacy_out(&self) -> &BTreeMap<String, i64> {
        &self.state.legacy
    }

    /// The snapshot is the save (L2): full state bytes plus the envelope.
    pub fn snapshot(&self) -> Snapshot {
        Snapshot {
            format: FORMAT_VERSION,
            content_fingerprint: self.content.fingerprint(),
            state: serde_json::to_vec(&self.state).unwrap_or_default(),
        }
    }

    /// Restore a snapshot: format then fingerprint are checked; a mismatch
    /// discards the snapshot (the host recompiles and reboots).
    pub fn restore(&mut self, snap: &Snapshot) -> Result<(), RuntimeDiagnostic> {
        if snap.format != FORMAT_VERSION {
            return Err(DiagKind::FormatVersion.refused(format!(
                "snapshot format {} does not match {FORMAT_VERSION}",
                snap.format
            )));
        }
        let fp = self.content.fingerprint();
        if snap.content_fingerprint != fp {
            return Err(DiagKind::ContentMismatch.refused(format!(
                "snapshot fingerprint {:016x} does not match content {fp:016x}",
                snap.content_fingerprint
            )));
        }
        match serde_json::from_slice::<State>(&snap.state) {
            Ok(s) => {
                self.state = s;
                Ok(())
            }
            Err(e) => {
                Err(DiagKind::Deserialize.refused(format!("snapshot does not deserialize: {e}")))
            }
        }
    }

    // -- shared internals ---------------------------------------------------

    pub(crate) fn verb(&self, id: &VerbId) -> Result<&VerbDecl, RuntimeDiagnostic> {
        self.content
            .verbs
            .get(id.as_str())
            .ok_or_else(|| DiagKind::UnknownVerb.refused(format!("unknown verb `{id}`")))
    }

    pub(crate) fn recipe_of(&self, id: &RecipeId) -> RecipeDecl {
        self.content
            .recipe(id.as_str())
            .cloned()
            .unwrap_or_else(|| kernel_invariant("UnknownRecipe", format!("unknown recipe `{id}`")))
    }

    pub(crate) fn situation(&self, sit: SituationId) -> Result<&Situation, RuntimeDiagnostic> {
        self.state
            .situations
            .get(&sit)
            .ok_or_else(|| DiagKind::UnresolvableRef.refused(format!("no live situation `{sit}`")))
    }

    /// The mint template of a card: the aliased card's body when the
    /// declaration carries `mint <card>` (spec.md §5).
    pub(crate) fn template(&self, card: &CardId) -> story_content::ir::CardDecl {
        let mut cur = card.clone();
        let mut seen = BTreeSet::new();
        loop {
            let Some(decl) = self.content.card(&cur) else {
                kernel_invariant("UnknownEntity", format!("unknown card `{cur}`"))
            };
            match &decl.mint_alias {
                Some(next) if seen.insert(cur.clone()) => cur = next.clone(),
                _ => return decl.clone(),
            }
        }
    }

    /// Is the entity bound by any live instance? Locks are derived by walking
    /// situation bindings; locking is total (spec.md §4.1).
    pub(crate) fn is_locked(&self, e: EntityId) -> bool {
        self.state
            .situations
            .values()
            .any(|s| s.bindings.values().any(|b| b.iter().any(|b| b.entity == e)))
    }

    pub(crate) fn verb_full(&self, verb: &VerbDecl) -> bool {
        match verb.instances {
            InstanceCap::Any => false,
            InstanceCap::AtMost(n) => {
                self.state
                    .situations
                    .values()
                    .filter(|s| s.verb == verb.id)
                    .count()
                    >= n as usize
            }
        }
    }

    /// Entity-level validation of every submitted binding: alive, unlocked,
    /// in a bindable zone, and not duplicated within one slot.
    pub(crate) fn validate_commit_bindings(
        &self,
        verb: &VerbDecl,
        bindings: &[(SlotName, EntityId)],
    ) -> Result<(), RuntimeDiagnostic> {
        let mut seen = BTreeSet::new();
        for (slot, e) in bindings {
            let Some(ent) = self.state.entities.get(e) else {
                return Err(DiagKind::UnknownEntity.refused(format!("unknown entity `{e}`")));
            };
            if self.is_locked(*e) {
                return Err(DiagKind::EntityLocked
                    .in_context(ent.name.clone(), "the card is bound by a live instance"));
            }
            match self.state.zones.get(&ent.zone) {
                Some(z) if z.bindable => {}
                _ => {
                    return Err(DiagKind::ZoneNotBindable
                        .in_context(ent.name.clone(), "the card's zone is not bindable"));
                }
            }
            if !seen.insert((slot.clone(), *e)) {
                return Err(DiagKind::SlotOverMax.in_context(
                    verb.id.to_string(),
                    format!("entity `{e}` is submitted twice for slot `{slot}`"),
                ));
            }
        }
        Ok(())
    }

    /// Total-ordered bind eligibility for one slot of one live situation:
    /// alive -> not locked -> zone bindable -> open slot -> filter match ->
    /// not full (spec.md §4.4). Locks come first because they are total (T8)
    /// and a bound card sits in a non-bindable interior, so the zone check
    /// alone would never reach `EntityLocked`. The slot-full count reads the
    /// addressed situation's bindings only: situations sharing a slot name do
    /// not fill each other.
    pub(crate) fn check_bindable(
        &self,
        sit: SituationId,
        sd: &SlotDecl,
        e: EntityId,
    ) -> Result<(), RuntimeDiagnostic> {
        let Some(ent) = self.state.entities.get(&e) else {
            return Err(DiagKind::UnknownEntity.refused(format!("unknown entity `{e}`")));
        };
        let name = ent.name.clone();
        if self.is_locked(e) {
            // Locks are total, whatever asks (T8).
            return Err(
                DiagKind::EntityLocked.in_context(name, "the card is bound by a live instance")
            );
        }
        match self.state.zones.get(&ent.zone) {
            Some(z) if z.bindable => {}
            _ => {
                return Err(
                    DiagKind::ZoneNotBindable.in_context(name, "the card's zone is not bindable")
                );
            }
        }
        if !sd.open {
            // Mid-run intervention addresses open slots only (spec.md §4.4).
            return Err(DiagKind::ZoneNotBindable.in_context(name, "the slot is not open"));
        }
        let ctx = Ctx::default();
        if !self.matches_slot(&e, sd, &ctx) {
            return Err(DiagKind::SlotFilterFail
                .in_context(name, "the card does not match the slot filter"));
        }
        if let Some(max) = sd.max {
            let bound = self
                .state
                .situations
                .get(&sit)
                .and_then(|s| s.bindings.get(&sd.name))
                .map(|b| b.len())
                .unwrap_or(0);
            if bound as u32 >= max {
                return Err(DiagKind::SlotOverMax.in_context(name, "the slot is full"));
            }
        }
        Ok(())
    }

    /// Charge one resolution unit against the per-advance budget; exceeding
    /// the budget restores the entry state and fails the whole call (T5).
    pub(crate) fn charge(&mut self, run: &mut Run) -> Result<(), crate::mint::Fail> {
        run.units += 1;
        if run.units > MAX_RESOLUTIONS_PER_ADVANCE {
            self.state = run.entry.clone();
            return Err(crate::mint::Fail::new(
                DiagKind::Livelock,
                format!("more than {MAX_RESOLUTIONS_PER_ADVANCE} resolutions in one call"),
            ));
        }
        Ok(())
    }
}

/// Total binding count of one situation (bind-order sequencing).
pub(crate) fn bind_count(state: &State, sit: SituationId) -> u64 {
    state
        .situations
        .get(&sit)
        .map(|s| s.bindings.values().map(|v| v.len() as u64).sum())
        .unwrap_or(0)
}

/// For references the compiler has already validated; content-driven failure
/// on the validated paths would be a kernel invariant violation.
pub(crate) fn kernel_invariant(kind: &str, message: String) -> ! {
    panic!("kernel invariant violated ({kind}): {message}")
}

/// Constructor helpers on the runtime diagnostic kind, usable crate-wide.
pub(crate) trait DiagExt {
    fn refused(self, message: impl Into<String>) -> RuntimeDiagnostic;
    fn in_context(
        self,
        context: impl Into<String>,
        message: impl Into<String>,
    ) -> RuntimeDiagnostic;
    fn warning(self, message: impl Into<String>) -> RuntimeDiagnostic;
}

impl DiagExt for DiagKind {
    fn refused(self, message: impl Into<String>) -> RuntimeDiagnostic {
        RuntimeDiagnostic::new(self, message)
    }

    fn in_context(
        self,
        context: impl Into<String>,
        message: impl Into<String>,
    ) -> RuntimeDiagnostic {
        RuntimeDiagnostic::in_context(self, context, message)
    }

    fn warning(self, message: impl Into<String>) -> RuntimeDiagnostic {
        RuntimeDiagnostic::warning(self, message)
    }
}
