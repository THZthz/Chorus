//! chorus — a deterministic text-game engine: content compiler, core
//! evaluator, headless session layer, and a static card-reachability
//! analyzer.
//!
//! Module map: `core` (the engine kernel — parsing, state, queries,
//! predicates, the frozen text/JSON classes), `content` (script/card
//! compilation), `engine` + `session`
//! (the step executor and the headless play surface; no UI layer is part of
//! this crate), `reach` (static reachability analysis, plus the fuzz and
//! scale campaign engines the bins and the test suite share), crate-root
//! `error` (the shared error channel) and `lcg` (the deterministic
//! campaign-side RNG — never the engine's randomness), and the five bins
//! (`bin/chorus` is a line REPL; the other four are fixed-purpose CLI
//! tools). The curated entry surface is [`prelude`].
//!
//! Invariants kept crate-wide: error messages are byte-exact
//! (`ChorusError`); iteration order is observable behavior (IndexMap, never
//! std HashMap at order-sensitive points).

pub mod content;
pub mod core;
pub mod engine;
pub mod error;
pub mod lcg;
pub mod reach;
pub mod session;

/// The crate's curated front door. The module tree above stays public (the
/// reach analyzer's seam modules are part of the reviewed surface), so this
/// prelude is a convenience re-export of the entry points a consumer of the
/// engine actually reaches for — not a visibility boundary. Anything not
/// listed here is reached through its module path.
pub mod prelude {
    // content — a script text becomes a world + logic
    pub use crate::content::script::{
        compile_script, lint_script, lint_script_findings, CompiledScript, LintFinding,
        LintSeverity, ScriptMeta,
    };
    pub use crate::content::{compile_content, CompiledContent, ContentRaw};
    // core — the dictionary and the state types
    pub use crate::core::dict::{core_dictionary, Dictionary};
    pub use crate::core::types::{Entity, State};
    // error — the engine's single error channel
    pub use crate::error::{ChorusError, Result};
    // engine — one step, the settled turn, the log, the replay
    pub use crate::engine::fold::{fold, fold_up_to};
    pub use crate::engine::settle::settle_step;
    pub use crate::engine::store::{load_log, save_log, Log};
    pub use crate::engine::{execute_step, ActionRecord, ExecOptions, Step};
    // session — the play surface and its projections
    pub use crate::session::game::{Game, Turn, TurnKind};
    pub use crate::session::lenient::{parse_player_input, LenientCtx, ParsedInput};
    pub use crate::session::render::{
        ending_notice, flatten_aspects, journal, pretty, prose, prose_lines,
    };
    // reach — the static card-reachability analyzer
    pub use crate::reach::{
        analyze_card, render_path, render_report, Analyzer, CardOptions, CardReport,
    };
}
