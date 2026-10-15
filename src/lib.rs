//! chorus — Rust port of the TS text-game engine (docs/rust-port/plan.md).
//!
//! Module tree mirrors `src/` one-to-one (plan §2.2): `core` (W1), `content`
//! (W2), `engine` + `session` (W3, the headless session layer — the ink/react
//! TUI is deliberately NOT ported, plan §1.3), `reach` (W5) and the five bins
//! (`bin/chorus` is W6's line REPL; the other four are the frozen tools/
//! CLI shapes). The curated entry surface is [`prelude`] (CORE-03).
//!
//! Porting discipline (plan §7): every ported unit carries a
//! `// ts: <file>:<line>` anchor; frozen quirks carry `// QUIRK(<id>):`;
//! error messages are byte-exact (`ChorusError`); iteration order is
//! observable behavior (IndexMap, never std HashMap at order-sensitive
//! points).

pub mod content;
pub mod core;
pub mod engine;
pub mod js;
pub mod reach;
pub mod session;

/// CORE-03 — the crate's curated front door. The module tree above stays
/// public (the ported test files anchor `chorus::<module>` paths one-to-one
/// with the TS imports, and the reach analyzer's seam modules are part of the
/// reviewed surface), so this prelude is a convenience re-export of the
/// entry points a consumer of the ported engine actually reaches for — not a
/// visibility boundary. Anything not listed here is reached through its
/// module path, exactly like its TS counterpart's import.
pub mod prelude {
    // content — a script text becomes a world + logic
    pub use crate::content::content::{compile_content, CompiledContent, ContentRaw};
    pub use crate::content::script::{compile_script, lint_script, CompiledScript, ScriptMeta};
    // core — the dictionary and the JS-semantics error channel
    pub use crate::core::dict::{core_dictionary, Dictionary};
    pub use crate::core::types::{Entity, State};
    pub use crate::js::{ChorusError, Result};
    // engine — one step, the log, the replay
    pub use crate::engine::engine::{execute_step, ActionRecord, ExecOptions, Step};
    pub use crate::engine::fold::{fold, fold_up_to};
    pub use crate::engine::store::{load_log, save_log, Log};
    // session — the play surface (play.ts's Game) and its projections
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
