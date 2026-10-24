//! Boot, the settlement loop, and the completion sequence (kernel-api.md §6,
//! spec.md §4.2, §4.6).

mod advance;
mod boot;
mod checkpoint;
mod completion;
mod expiry;

pub(crate) use advance::advance;
pub(crate) use boot::boot;
pub(crate) use checkpoint::checkpoint;

use story_core::State;

/// The budget guard of one engine call: the entry checkpoint for the only
/// whole-call rollback (Livelock), the resolution counter, and the native
/// recursion depth of inline resolution (instant start chains, signal
/// cycles), capped well below the stack limit. The optional fork hook
/// (story-analyze spec §2.2) rides along so every settlement path — effect
/// lists, books, `start`/`continue` adoption, checkpoints — sees it without
/// duplicating the machinery.
pub(crate) struct Run<'fork> {
    pub(crate) entry: State,
    pub(crate) units: u64,
    pub(crate) depth: u32,
    pub(crate) fork: Option<crate::ForkHook<'fork>>,
}

impl<'fork> Run<'fork> {
    pub(crate) fn new(entry: State) -> Self {
        Run {
            entry,
            units: 0,
            depth: 0,
            fork: None,
        }
    }

    pub(crate) fn with_fork(entry: State, fork: Option<crate::ForkHook<'fork>>) -> Self {
        Run {
            entry,
            units: 0,
            depth: 0,
            fork,
        }
    }
}
