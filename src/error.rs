//! The engine's single error channel.

use std::fmt;

/// One error type for the whole engine: a plain message, deliberately NOT
/// an enum of site-grouped variants. Every error message lives at exactly
/// one construction site — `ChorusError::new(format!(...))` with its exact
/// text — so each wording is maintained in one place, and the type can
/// never grow variant/Display pairs that drift apart. The message texts
/// themselves are the real maintenance surface (the test suite pins each
/// one character-for-character); a variant taxonomy would add a second
/// surface — match arms plus Display strings — over the same bytes, not a
/// smaller one.
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
