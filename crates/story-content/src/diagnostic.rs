//! The diagnostic value every crate shares (spec.md §4.8): content-driven
//! failure is always a value, never a panic.

use std::fmt;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Severity {
    Error,
    Warning,
}

impl Severity {
    pub fn word(&self) -> &'static str {
        match self {
            Severity::Error => "error",
            Severity::Warning => "warning",
        }
    }
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Diagnostic {
    pub severity: Severity,
    /// Stable number from `story_content::code`.
    pub number: u32,
    pub file: String,
    pub line: u32,
    pub col: u32,
    pub message: String,
    pub notes: Vec<String>,
}

impl Diagnostic {
    pub fn error(number: u32, file: &str, line: u32, col: u32, message: impl Into<String>) -> Self {
        Diagnostic {
            severity: Severity::Error,
            number,
            file: file.to_string(),
            line,
            col,
            message: message.into(),
            notes: Vec::new(),
        }
    }

    pub fn warning(
        number: u32,
        file: &str,
        line: u32,
        col: u32,
        message: impl Into<String>,
    ) -> Self {
        Diagnostic {
            severity: Severity::Warning,
            number,
            file: file.to_string(),
            line,
            col,
            message: message.into(),
            notes: Vec::new(),
        }
    }

    pub fn with_note(mut self, note: impl Into<String>) -> Self {
        self.notes.push(note.into());
        self
    }

    pub fn is_error(&self) -> bool {
        matches!(self.severity, Severity::Error)
    }
}

impl fmt::Display for Diagnostic {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let letter = if self.is_error() { 'E' } else { 'W' };
        write!(
            f,
            "{}:{}:{}: {} {letter}{}: {}",
            self.file,
            self.line,
            self.col,
            self.severity.word(),
            self.number,
            self.message
        )?;
        for note in &self.notes {
            write!(f, "\n  note: {note}")?;
        }
        Ok(())
    }
}
