//! The content layer: `script.rs` is the line-segment script parser +
//! load-time lint; `content.rs` is the compiler (and hosts the §4.4 deck
//! machinery).

// the clippy "inception" shape (`content::content`) is deliberate — the
// module and its core type share a name.
#![allow(clippy::module_inception)]

pub mod content;
pub mod script;
