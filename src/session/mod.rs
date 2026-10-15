//! The session layer: `lenient.rs`, `render.rs` and `game.rs` (the Game).
//! Headless only: no TUI ships here; the game logic is fully testable
//! without a TTY.

pub mod game;
pub mod lenient;
pub mod render;
