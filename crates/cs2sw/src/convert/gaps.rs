//! The gap report and accounting types: classified gap rows, conversion
//! stats, and the conversion result.

use std::collections::BTreeMap;

// ---------------------------------------------------------------------------
// gaps, stats, result
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Class {
    /// The language needs a new seam to carry this construct.
    ExtensionSeam,
    /// The construct is outside the converter's scope by design.
    NonGoal,
    /// A converter-side approximation of CS semantics.
    ConverterIdiom,
}

impl Class {
    pub fn label(&self) -> &'static str {
        match self {
            Class::ExtensionSeam => "extension-seam candidate",
            Class::NonGoal => "permanent non-goal",
            Class::ConverterIdiom => "converter idiom",
        }
    }
}

#[derive(Clone)]
pub struct GapRow {
    pub construct: String,
    pub classification: Class,
    pub note: String,
    /// Every affected id, uncapped (the authoritative count).
    pub ids: Vec<String>,
}

#[derive(Default)]
pub struct Gaps {
    pub rows: Vec<GapRow>,
    index: BTreeMap<String, usize>,
}

impl Gaps {
    pub fn row(&mut self, construct: &str, class: Class, note: &str) -> &mut GapRow {
        let i = match self.index.get(construct) {
            Some(&i) => i,
            None => {
                let i = self.rows.len();
                self.rows.push(GapRow {
                    construct: construct.to_string(),
                    classification: class,
                    note: note.to_string(),
                    ids: Vec::new(),
                });
                self.index.insert(construct.to_string(), i);
                i
            }
        };
        &mut self.rows[i]
    }

    pub fn hit(&mut self, construct: &str, class: Class, note: &str, id: &str) {
        let i = match self.index.get(construct) {
            Some(&i) => i,
            None => {
                self.row(construct, class, note);
                self.index[construct]
            }
        };
        self.rows[i].ids.push(id.to_string());
    }

    pub fn count(&self, construct: &str) -> usize {
        self.index
            .get(construct)
            .map(|&i| self.rows[i].ids.len())
            .unwrap_or(0)
    }
}

#[derive(Default, Clone)]
pub struct Stats {
    pub elements_in: usize,
    pub aspect_elements: usize,
    pub cards_out: usize,
    pub recipes_in: usize,
    pub recipes_out: usize,
    pub verbs_out: usize,
    pub decks_out: usize,
    pub endings_out: usize,
    pub aspect_keys: usize,
    pub output_lines: usize,
    /// Emitted slot lines across all recipes (source + verb + synthesized).
    pub slots_out: usize,
    /// Dead census: emitted recipes on uncommittable, non-ambient verbs with
    /// no incoming `start` or `continue`, and the verbs whose every recipe is
    /// dead.
    pub dead_recipes: usize,
    pub dead_recipes_referenced: usize,
    pub dead_verbs: usize,
}

pub struct Conversion {
    pub sw: String,
    pub stats: Stats,
    pub gaps: Gaps,
    /// Files that failed to load or were excluded, (kind, name, reason).
    pub skipped: Vec<(String, String, String)>,
    /// Naming collisions, (CS id, assigned name).
    pub collisions: Vec<(String, String)>,
    /// The dead census detail: (verb id, dead recipe count) per verb whose
    /// every recipe is unreachable, in verb order.
    pub dead_verb_detail: Vec<(String, usize)>,
}
