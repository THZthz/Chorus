//! The midgame face (`--from-state`): rebuild the analysis state from a
//! JSONL log by the engine's OWN public replay path —
//! `store::load_log` + `fold::fold` (spec §7: state = fold(log)) — and
//! hand the replayed [`State`] to the [`Analyzer`](super::world::Analyzer)
//! as its initial world. No engine or session change: this module only
//! composes shipped entry points, exactly like the campaign modules
//! (`fuzz` / `scale`) it sits beside — the reach analysis modules proper
//! stay engine-free.
//!
//! What the replayed state carries into the analysis for free (the
//! analyzer reads `entities` filtered to the living and `now` only): the
//! midgame entities (with their death filters) and the midgame clock —
//! every `first_cross` re-anchors to the replayed minute automatically.
//! What the face models on top (world.rs / fixpoint.rs / paths.rs):
//! already-fired once triggers (they never fire again — the `AlreadyFired`
//! rung), the deck cursors (draws serve from the cursor forward —
//! `deck_serves_cap` / `add_product`), and the manifested unique set (§4.2:
//! its deck positions never serve again — the same two predicates). What
//! stays deliberately unmodeled
//! is named in the fidelity note every replayed report carries: open
//! windows (the lock table), the executed-recipe map, the rng counter.

use std::path::Path;

use crate::content::CompiledContent;
use crate::core::dict::Dictionary;
use crate::core::types::State;
use crate::engine::fold::fold;
use crate::engine::store::load_log;
use crate::error::Result;

/// Load a JSONL log (§7 wire shape) and fold it onto `base` — the script's
/// own initial state. Errors carry the file name and 1-based line number
/// (`load_log`'s contract); the fold re-derives every projection (wait
/// spans partition time — a drifting log is a loud error).
pub fn replay_state(
    log_path: &Path,
    base: &State,
    dict: &Dictionary,
    content: &CompiledContent,
) -> Result<State> {
    let records = load_log(log_path)?;
    fold(&records, base, dict, content)
}
