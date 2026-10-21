//! Compiled content database and IR types, plus the diagnostics vocabulary
//! shared by every crate (spec.md §3).

pub mod code;
pub mod content;
pub mod diagnostic;
pub mod ir;
pub mod runtime;

pub use content::Content;
pub use diagnostic::{Diagnostic, Severity};
pub use runtime::{DiagKind, RuntimeDiagnostic};
