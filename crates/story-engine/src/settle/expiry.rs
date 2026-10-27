//! Expiry settlement (spec.md §4.6).

use super::Run;
use crate::effects::EffectCtx;
use crate::mint::Fail;
use crate::{Engine, Event};
use std::collections::BTreeMap;
use story_content::runtime::DiagKind;
use story_core::EntityId;

// ---------------------------------------------------------------------------
// Expiry (spec.md §4.6)
// ---------------------------------------------------------------------------

/// One expiry settlement: the timer is one-shot, cleared at settlement; a
/// declared `on expire` book replaces default destruction and addresses the
/// dying card as `subject`. Context-free: `interior`, `slot:`, and `bound`
/// read empty (T9).
pub(super) fn settle_expiry(
    engine: &mut Engine,
    e: EntityId,
    ev: &mut Vec<Event>,
    run: &mut Run<'_>,
) -> Result<(), Fail> {
    let Some(ent) = engine.state.entities.get(&e) else {
        return Ok(());
    };
    let prototype = ent.prototype.clone();
    let book = engine.template(&prototype).on_expire.clone();
    // Only a book's effects can fail; the book-less default destruction is
    // total, so the checkpoint exists only when there is a book to fire.
    let checkpoint = book.is_some().then(|| engine.state.clone());
    if let Some(ent) = engine.state.entities.get_mut(&e) {
        ent.expires_at = None;
    }
    let ev_checkpoint = ev.len();
    if let Some(book) = book {
        let mut fctx = EffectCtx {
            sit: None,
            now: engine.state.now,
            roll: None,
            handles: BTreeMap::from([("subject".to_string(), vec![e])]),
            continued: None,
        };
        match engine.fire_book(&book, &mut fctx, None, ev, run) {
            Ok(()) => {}
            // The budget guard already restored the entry state.
            Err(f) if f.kind == DiagKind::Livelock => return Err(f),
            // Per-unit policy: roll back to the checkpoint. The timer is
            // spent regardless — expiry is one-shot — so the loop progresses.
            Err(_) => {
                engine.state = checkpoint.expect("a book fired, so a checkpoint exists");
                ev.truncate(ev_checkpoint);
                if let Some(ent) = engine.state.entities.get_mut(&e) {
                    ent.expires_at = None;
                }
            }
        }
    } else {
        engine.destroy_entity(e);
    }
    Ok(())
}
