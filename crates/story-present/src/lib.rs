//! `story-present`: Thin presentation and narrative layer over the mechanical Chorus² engine.
//!
//! Enforces Chorus² Kernel Law L6 (Mechanics vs Presentation): the engine runs pure
//! mechanics, while this layer translates domain lifecycle events, queries state, and
//! renders human-readable narrative output, view models, and story beats.

pub mod catalog;
pub mod interpolate;
pub mod presenter;
pub mod view;

pub use catalog::PresentationCatalog;
pub use interpolate::{interpolate, PresentationContext};
pub use presenter::Presenter;
pub use view::{
    BeatKind, BoardView, CardLoreView, CardSummaryView, EndingView, RecipeOpportunityView,
    SituationView, SlotPromptView, SlottedCardView, StoryBeat, StoryOutput, VerbOpportunityView,
    ZoneView,
};
