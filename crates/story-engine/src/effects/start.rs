//! The `start` effect: recipe selection and inline instant chains.

use super::EffectCtx;
use crate::mint::Fail;
use crate::select::{Selection, Source};
use crate::settle::Run;
use crate::{Engine, Event, StartSkipReason};

impl Engine {
    /// `start <verb> [via <recipe>] [count <n>]`: fan-out is best-effort —
    /// one branch's failure never rolls back its siblings or the parent
    /// resolution (spec.md §4.2).
    pub(crate) fn apply_start(
        &mut self,
        verb: &story_core::VerbId,
        via: Option<&story_core::RecipeId>,
        n: i64,
        fctx: &mut EffectCtx,
        ev: &mut Vec<Event>,
        run: &mut Run<'_>,
    ) -> Result<(), Fail> {
        let Some(verb_decl) = self.content.verbs.get(verb.as_str()).cloned() else {
            return Err(Fail::new(
                story_content::runtime::DiagKind::UnknownVerb,
                format!("unknown verb `{verb}`"),
            ));
        };
        for _ in 0..n {
            match self.select(Source::Start, &verb_decl, via, None, fctx.sit) {
                Selection::Nothing | Selection::GuardRefused(_) => {
                    // The chain ends silently: no eligible recipe, every
                    // `when` false, or the winner's guard refused.
                    ev.push(Event::ChainEnded { verb: verb.clone() });
                    return Ok(());
                }
                Selection::Matched(rec) => {
                    if self.verb_full(&verb_decl) {
                        ev.push(Event::StartSkipped {
                            verb: verb.clone(),
                            reason: StartSkipReason::InstanceCap,
                        });
                        return Ok(());
                    }
                    match self.materialize(
                        &verb_decl,
                        &rec,
                        story_core::SituationSource::Start {
                            parent: fctx.sit.unwrap_or_default(),
                        },
                        &[],
                        ev,
                    ) {
                        Err(()) => {
                            ev.push(Event::StartSkipped {
                                verb: verb.clone(),
                                reason: StartSkipReason::MaterializationFailed,
                            });
                            return Ok(());
                        }
                        Ok(sit) => {
                            if rec.duration == 0 {
                                // A zero-duration recipe resolves inline
                                // within the current resolution unit, each
                                // inline completion counting against the
                                // per-advance budget; a failure propagates to
                                // the enclosing unit (spec.md §4.2).
                                self.charge(run)?;
                                self.completion_steps(sit, ev, run)?;
                            }
                        }
                    }
                }
            }
        }
        Ok(())
    }
}
