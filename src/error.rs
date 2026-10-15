//! The engine's single error channel.

use std::fmt;

/// One error type for the whole engine: every error message lives at exactly
/// one construction site `ChorusError::new(format!(...))` with its exact
/// text. Deliberately a plain message, NOT an enum
/// of variants — the variants + Display double maintenance surface is where
/// messages drift.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChorusError {
    pub msg: String,
}

impl ChorusError {
    pub fn new(msg: impl Into<String>) -> Self {
        ChorusError { msg: msg.into() }
    }
}

impl fmt::Display for ChorusError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.msg)
    }
}

impl std::error::Error for ChorusError {}

pub type Result<T> = std::result::Result<T, ChorusError>;
