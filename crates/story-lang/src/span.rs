//! Source positions. Every diagnostic carries `file:line:col` (language.md,
//! design goals); columns are 1-based character offsets.

use std::sync::Arc;
use story_content::Diagnostic;

#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Span {
    pub file: Arc<str>,
    pub line: u32,
    pub col: u32,
}

impl Span {
    pub fn new(file: Arc<str>, line: u32, col: u32) -> Self {
        Span { file, line, col }
    }

    pub fn error(&self, number: u32, message: impl Into<String>) -> Diagnostic {
        Diagnostic::error(number, &self.file, self.line, self.col, message)
    }

    pub fn warning(&self, number: u32, message: impl Into<String>) -> Diagnostic {
        Diagnostic::warning(number, &self.file, self.line, self.col, message)
    }
}
