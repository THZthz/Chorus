//! ts: src/ui/* — the session layer (plan §2.2): `lenient.rs`, then
//! `render.rs` + `game.rs` (play.ts's Game). Headless only: the ink/react
//! TUI (tui.tsx) and examples/play.ts's entry logic are NOT ported
//! (plan §1.3 — Game itself is load-bearing for 7 test files, UI-01).

pub mod lenient;
