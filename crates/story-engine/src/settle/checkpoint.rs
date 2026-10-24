//! Autonomous checkpoints (spec.md §4.6).

use super::Run;
use crate::mint::Fail;
use crate::select::{Selection, Source};
use crate::{Engine, Event, StartSkipReason};
use story_content::ir::VerbMode;
use story_content::runtime::{DiagKind, RuntimeDiagnostic};
use story_core::{SituationSource, VerbId};

// ---------------------------------------------------------------------------
// Autonomous checkpoints (spec.md §4.6)
// ---------------------------------------------------------------------------

/// One autonomous checkpoint: in verb declaration order, each autonomous verb
/// not at its instance cap runs the selection law over empty bindings; a hit
/// materializes at most one instance; a miss is silent. An all-instant verb
/// pulses exactly once: one selection, one inline open-complete cycle.
pub(crate) fn checkpoint(
    engine: &mut Engine,
    ev: &mut Vec<Event>,
    run: &mut Run<'_>,
) -> Result<(), RuntimeDiagnostic> {
    engine.charge(run).map_err(Fail::into_diagnostic)?;
    let verbs: Vec<VerbId> = {
        let mut vs: Vec<&story_content::ir::VerbDecl> = engine.content.verbs.values().collect();
        vs.sort_by_key(|v| v.decl_index);
        vs.into_iter()
            .filter(|v| v.mode == VerbMode::Autonomous)
            .map(|v| v.id.clone())
            .collect()
    };
    for verb in verbs {
        let Some(decl) = engine.content.verbs.get(verb.as_str()).cloned() else {
            continue;
        };
        if engine.verb_full(&decl) {
            continue;
        }
        match engine.select(Source::Poll, &decl, None, None, None) {
            Selection::Matched(rec) => {
                // Per-verb checkpoint: a failure rolls this verb's open back
                // only — earlier verbs' opens survive with their events.
                let cp = engine.state.clone();
                let sit = engine.materialize(&decl, &rec, SituationSource::Autonomous, &[], ev);
                match sit {
                    Err(()) => ev.push(Event::StartSkipped {
                        verb: verb.clone(),
                        reason: StartSkipReason::MaterializationFailed,
                    }),
                    Ok(sit) => {
                        if rec.duration == 0 {
                            match engine.completion_steps(sit, ev, run) {
                                Ok(()) => {}
                                // The Livelock breach already restored the
                                // entry state; the whole call unwinds (T5).
                                Err(f) if f.kind == DiagKind::Livelock => {
                                    return Err(f.into_diagnostic());
                                }
                                Err(f) => {
                                    // The event names the situation while it
                                    // is still live, before its rollback.
                                    ev.push(Event::ResolutionFailed {
                                        sit,
                                        reason: f.display_reason(),
                                    });
                                    engine.state = cp;
                                }
                            }
                        }
                    }
                }
            }
            // A miss is silent; a guard refusing at a poll is a silent miss.
            Selection::Nothing | Selection::GuardRefused(_) => {}
        }
        if engine.state.ended.is_some() {
            return Ok(());
        }
    }
    Ok(())
}
