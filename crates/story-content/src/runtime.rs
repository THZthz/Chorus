//! The runtime diagnostic (kernel-api.md §7): content-driven failure is
//! always a value, never a panic. Distinct from the compile `Diagnostic`:
//! runtime failures carry a kind and a message, not a source span.

use crate::Severity;
use std::fmt;

/// The runtime diagnostic kinds, in the four families of kernel-api.md §7.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DiagKind {
    // Lookup.
    UnknownVerb,
    UnknownEntity,
    UnknownRecipe,
    UnknownZone,
    UnknownDeck,
    UnknownEnding,
    UnresolvableRef,
    UndeclaredEvent,
    // Eligibility (commit and bind refusals).
    VerbAutonomous,
    StoryEnded,
    InstanceCapReached,
    EntityDead,
    EntityLocked,
    ZoneNotBindable,
    SlotFilterFail,
    SlotUnderMin,
    SlotOverMax,
    NoRecipe,
    GuardRefused,
    // Effects.
    NameInUse,
    DeckExhausted,
    UnregisteredOp,
    BadLifetime,
    BadDuration,
    /// A generic effect failure (shortage, invalid operands, misaddressed
    /// target) on the runtime path.
    EffectFailure,
    // Advance / snapshot / boot.
    Livelock,
    FormatVersion,
    ContentMismatch,
    Deserialize,
    SeedMismatch,
}

impl DiagKind {
    /// The family this kind belongs to (kernel-api.md §7).
    pub fn family(&self) -> &'static str {
        match self {
            DiagKind::UnknownVerb
            | DiagKind::UnknownEntity
            | DiagKind::UnknownRecipe
            | DiagKind::UnknownZone
            | DiagKind::UnknownDeck
            | DiagKind::UnknownEnding
            | DiagKind::UnresolvableRef
            | DiagKind::UndeclaredEvent => "lookup",
            DiagKind::VerbAutonomous
            | DiagKind::StoryEnded
            | DiagKind::InstanceCapReached
            | DiagKind::EntityDead
            | DiagKind::EntityLocked
            | DiagKind::ZoneNotBindable
            | DiagKind::SlotFilterFail
            | DiagKind::SlotUnderMin
            | DiagKind::SlotOverMax
            | DiagKind::NoRecipe
            | DiagKind::GuardRefused => "eligibility",
            DiagKind::NameInUse
            | DiagKind::DeckExhausted
            | DiagKind::UnregisteredOp
            | DiagKind::BadLifetime
            | DiagKind::BadDuration
            | DiagKind::EffectFailure => "effects",
            DiagKind::Livelock
            | DiagKind::FormatVersion
            | DiagKind::ContentMismatch
            | DiagKind::Deserialize
            | DiagKind::SeedMismatch => "advance/snapshot/boot",
        }
    }

    pub fn word(&self) -> &'static str {
        match self {
            DiagKind::UnknownVerb => "UnknownVerb",
            DiagKind::UnknownEntity => "UnknownEntity",
            DiagKind::UnknownRecipe => "UnknownRecipe",
            DiagKind::UnknownZone => "UnknownZone",
            DiagKind::UnknownDeck => "UnknownDeck",
            DiagKind::UnknownEnding => "UnknownEnding",
            DiagKind::UnresolvableRef => "UnresolvableRef",
            DiagKind::UndeclaredEvent => "UndeclaredEvent",
            DiagKind::VerbAutonomous => "VerbAutonomous",
            DiagKind::StoryEnded => "StoryEnded",
            DiagKind::InstanceCapReached => "InstanceCapReached",
            DiagKind::EntityDead => "EntityDead",
            DiagKind::EntityLocked => "EntityLocked",
            DiagKind::ZoneNotBindable => "ZoneNotBindable",
            DiagKind::SlotFilterFail => "SlotFilterFail",
            DiagKind::SlotUnderMin => "SlotUnderMin",
            DiagKind::SlotOverMax => "SlotOverMax",
            DiagKind::NoRecipe => "NoRecipe",
            DiagKind::GuardRefused => "GuardRefused",
            DiagKind::NameInUse => "NameInUse",
            DiagKind::DeckExhausted => "DeckExhausted",
            DiagKind::UnregisteredOp => "UnregisteredOp",
            DiagKind::BadLifetime => "BadLifetime",
            DiagKind::BadDuration => "BadDuration",
            DiagKind::EffectFailure => "EffectFailure",
            DiagKind::Livelock => "Livelock",
            DiagKind::FormatVersion => "FormatVersion",
            DiagKind::ContentMismatch => "ContentMismatch",
            DiagKind::Deserialize => "Deserialize",
            DiagKind::SeedMismatch => "SeedMismatch",
        }
    }
}

/// A runtime diagnostic value: the kind, the message, the context it arose
/// in, and a severity — boot warnings may proceed, errors must not
/// (kernel-api.md §1, §7).
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct RuntimeDiagnostic {
    pub severity: Severity,
    pub kind: DiagKind,
    pub message: String,
    pub context: String,
}

impl RuntimeDiagnostic {
    pub fn new(kind: DiagKind, message: impl Into<String>) -> Self {
        RuntimeDiagnostic {
            severity: Severity::Error,
            kind,
            message: message.into(),
            context: String::new(),
        }
    }

    pub fn warning(kind: DiagKind, message: impl Into<String>) -> Self {
        RuntimeDiagnostic {
            severity: Severity::Warning,
            kind,
            message: message.into(),
            context: String::new(),
        }
    }

    pub fn in_context(
        kind: DiagKind,
        context: impl Into<String>,
        message: impl Into<String>,
    ) -> Self {
        RuntimeDiagnostic {
            severity: Severity::Error,
            kind,
            message: message.into(),
            context: context.into(),
        }
    }

    pub fn is_error(&self) -> bool {
        matches!(self.severity, Severity::Error)
    }
}

impl fmt::Display for RuntimeDiagnostic {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{} {} [{}]: {}",
            self.severity.word(),
            self.kind.word(),
            self.kind.family(),
            self.message
        )?;
        if !self.context.is_empty() {
            write!(f, " ({})", self.context)?;
        }
        Ok(())
    }
}
