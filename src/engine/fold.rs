//! Fold (spec §7: state = fold(log)). Rebuilds the projections (entities,
//! relations, provenance, clock) by replaying concrete outcomes in order.

use crate::content::content::CompiledContent;
use crate::core::dict::Dictionary;
use crate::core::types::{clone_state, State};
use crate::engine::engine::{apply_record, ActionRecord};
use crate::error::Result;

/// Replay every record in order onto `base`.
pub fn fold(
    records: &[ActionRecord],
    base: &State,
    dict: &Dictionary,
    content: &CompiledContent,
) -> Result<State> {
    fold_iter(records.iter(), base, dict, content)
}

/// Undo = re-fold without the tail (spec §7). The `<` filter is
/// load-bearing — a REFUSED step's record REUSES its id next round
/// (engine.rs execute_step Phase 3), so `<=` here would replay the new
/// attempt when undoing past the refusal. Never widen.
pub fn fold_up_to(
    records: &[ActionRecord],
    base: &State,
    dict: &Dictionary,
    content: &CompiledContent,
    upto_exclusive: i64,
) -> Result<State> {
    fold_iter(
        records.iter().filter(|r| r.id < upto_exclusive),
        base,
        dict,
        content,
    )
}

/// The shared replay loop; `fold_up_to` feeds it the filtered iterator
/// without materializing a Vec.
fn fold_iter<'a>(
    records: impl Iterator<Item = &'a ActionRecord>,
    base: &State,
    dict: &Dictionary,
    content: &CompiledContent,
) -> Result<State> {
    let mut state = clone_state(base);
    for r in records {
        apply_record(r, &mut state, dict, content)?;
    }
    Ok(state)
}
