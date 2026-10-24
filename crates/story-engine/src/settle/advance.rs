//! The due queue and the settlement loop over it (kernel-api.md §6).

use super::checkpoint;
use super::expiry::settle_expiry;
use super::Run;
use crate::mint::Fail;
use crate::{DiagExt, Engine, Event};
use std::collections::BTreeSet;
use story_content::runtime::{DiagKind, RuntimeDiagnostic};
use story_core::{EntityId, SituationId, Time};

pub(crate) fn advance(
    engine: &mut Engine,
    dt: Time,
    fork: Option<crate::ForkHook<'_>>,
) -> Result<Vec<Event>, RuntimeDiagnostic> {
    let Some(target) = engine.state.now.checked_add(dt) else {
        return Err(DiagKind::BadDuration.refused(format!(
            "advance to now {} + {dt} overflows the clock",
            engine.state.now
        )));
    };
    let mut run = Run::with_fork(engine.state.clone(), fork);
    let mut ev: Vec<Event> = Vec::new();
    // Each distinct settled moment polls once, after its 0/1 phases finish;
    // the end of the advance polls once (deduplicated against it).
    let mut polled: BTreeSet<Time> = BTreeSet::new();
    loop {
        // Drain the due queue in (moment, phase, id) order; when the next
        // item lies past the moment just settled, that moment's 0/1 phases
        // are finished and it polls — then the queue is re-taken, because
        // the checkpoint may open instances at moments in between.
        let mut last: Option<Time> = None;
        while let Some(item) = next_due(engine, target) {
            let m = item.moment();
            if let Some(pm) = last {
                if m > pm && !polled.contains(&pm) {
                    polled.insert(pm);
                    step_now(engine, pm);
                    checkpoint(engine, &mut ev, &mut run)?;
                    if engine.state.ended.is_some() {
                        return Ok(ev);
                    }
                    continue;
                }
            }
            last = Some(m);
            step_now(engine, m);
            match item {
                Due::Completion(sit, _) => {
                    engine.charge(&mut run).map_err(Fail::into_diagnostic)?;
                    engine
                        .settle_completion(sit, &mut ev, &mut run)
                        .map_err(Fail::into_diagnostic)?;
                }
                Due::Expiry(e, _) => {
                    engine.charge(&mut run).map_err(Fail::into_diagnostic)?;
                    settle_expiry(engine, e, &mut ev, &mut run).map_err(Fail::into_diagnostic)?;
                }
            }
            if engine.state.ended.is_some() {
                // The ending closed the run at this instant: the remaining
                // queue is discarded and the clock freezes here (T7).
                return Ok(ev);
            }
        }
        // Drained: the last settled moment polls now (it may open instances
        // that are due again before `target`), so re-scan.
        if let Some(pm) = last {
            if !polled.contains(&pm) {
                polled.insert(pm);
                step_now(engine, pm);
                checkpoint(engine, &mut ev, &mut run)?;
                if engine.state.ended.is_some() {
                    return Ok(ev);
                }
                continue;
            }
        }
        // The end-of-advance checkpoint runs once the drain is complete; a
        // freed past-due card or an instant pulse can re-arm the queue, so
        // re-scan afterwards too.
        if !polled.contains(&target) {
            step_now(engine, target);
            polled.insert(target);
            checkpoint(engine, &mut ev, &mut run)?;
            if engine.state.ended.is_some() {
                return Ok(ev);
            }
            continue;
        }
        break;
    }
    step_now(engine, target);
    Ok(ev)
}

fn step_now(engine: &mut Engine, m: Time) {
    engine.state.now = engine.state.now.max(m);
}

/// One due item of the derived queue: a situation completion (phase 0) or an
/// unbound entity's expiry (phase 1); autonomous checkpoints (phase 2) run
/// after the 0/1 phases drain at each settled moment.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Due {
    Completion(SituationId, Time),
    Expiry(EntityId, Time),
}

impl Due {
    fn moment(&self) -> Time {
        match *self {
            Due::Completion(_, m) | Due::Expiry(_, m) => m,
        }
    }

    fn phase(&self) -> u8 {
        match *self {
            Due::Completion(..) => 0,
            Due::Expiry(..) => 1,
        }
    }

    fn id(&self) -> u64 {
        match *self {
            Due::Completion(s, _) => s.0,
            Due::Expiry(e, _) => e.0,
        }
    }
}

/// The least due item at or before `target` under the total-order key
/// `(moment, phase, id)`. Bound (locked) cards are not due: their expiry is
/// deferred while bound.
fn next_due(engine: &Engine, target: Time) -> Option<Due> {
    let completion = engine
        .state
        .situations
        .values()
        .filter(|s| s.ends_at <= target)
        .map(|s| Due::Completion(s.id, s.ends_at))
        .min_by_key(|d| (d.moment(), d.phase(), d.id()));
    let expiry = engine
        .state
        .entities
        .values()
        .filter_map(|e| {
            let t = e.expires_at?;
            if t > target || engine.is_locked(e.id) {
                None
            } else {
                Some(Due::Expiry(e.id, t))
            }
        })
        .min_by_key(|d| (d.moment(), d.phase(), d.id()));
    match (completion, expiry) {
        (Some(c), Some(e)) => {
            if (c.moment(), c.phase(), c.id()) <= (e.moment(), e.phase(), e.id()) {
                Some(c)
            } else {
                Some(e)
            }
        }
        (Some(c), None) => Some(c),
        (None, Some(e)) => Some(e),
        (None, None) => None,
    }
}
