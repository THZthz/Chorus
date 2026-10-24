//! The loaded CS core tree: element/recipe/deck/ending/verb/lever/portal
//! tables with the load-error log. Iteration is deterministic throughout:
//! elements, decks, endings, verbs, and levers are sorted by id; recipes keep
//! content order (sorted file paths, in-file sequence), which is CS's
//! first-match priority order.

use std::collections::BTreeMap;
use std::path::Path;

use crate::json;

/// Files excluded from the tree, with the reason recorded in the load log.
pub const SKIP_ELEM_FILES: &[&str] = &[
    "credits.json",
    "versionnews.json",
    "experimental.json",
    "xperimental.json",
    "priest_elements.json",
    "legacy_dancer_elements.json",
];
pub const SKIP_RECIPE_FILES: &[&str] = &[
    "1_debug.json",
    "__debug.json",
    "_debug.json",
    "z_old+work_arcane.json",
    "z_legacyrecipepreservation.json",
];

#[derive(Clone, Default)]
pub struct CsData {
    /// Every element by id (first definition wins).
    pub elements: BTreeMap<String, serde_json::Value>,
    /// Recipes in content order (first definition wins).
    pub recipes: Vec<(String, serde_json::Value)>,
    pub recipe_index: BTreeMap<String, usize>,
    /// Decks in id order.
    pub decks: BTreeMap<String, serde_json::Value>,
    /// Endings by id; the DLC files fill only ids the core file lacks.
    pub endings: BTreeMap<String, serde_json::Value>,
    /// Verbs by id (the verb table: labels and the verb-level slot).
    pub verbs: BTreeMap<String, serde_json::Value>,
    /// Levers by id.
    pub levers: BTreeMap<String, serde_json::Value>,
    /// Portals by id (portal consequences feed deck draws).
    pub portals: BTreeMap<String, serde_json::Value>,
    /// Load notes: (kind, name, reason).
    pub skipped: Vec<(String, String, String)>,
}

impl CsData {
    pub fn recipe(&self, id: &str) -> Option<&serde_json::Value> {
        self.recipe_index.get(id).map(|&i| &self.recipes[i].1)
    }

    pub fn element(&self, id: &str) -> Option<&serde_json::Value> {
        self.elements.get(id)
    }

    pub fn note_skip(&mut self, kind: &str, name: &str, reason: &str) {
        self.skipped
            .push((kind.to_string(), name.to_string(), reason.to_string()));
    }
}

fn read(path: &Path) -> Result<serde_json::Value, String> {
    let bytes = std::fs::read(path).map_err(|e| e.to_string())?;
    let text = String::from_utf8(bytes).map_err(|e| e.to_string())?;
    json::parse(&text)
}

fn list_of<'a>(v: &'a serde_json::Value, key: &str) -> Vec<&'a serde_json::Value> {
    v.get(key).and_then(|x| x.as_array()).map(|a| a.iter().collect()).unwrap_or_default()
}

/// Load every JSON file of the core tree. A file that fails to parse is
/// recorded and skipped; conversion proceeds over the rest.
pub fn load_tree(root: &Path) -> CsData {
    let mut d = CsData::default();

    let files_in = |dir: &str| -> Vec<std::path::PathBuf> {
        let mut out: Vec<std::path::PathBuf> = std::fs::read_dir(root.join(dir))
            .into_iter()
            .flatten()
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.extension().map(|x| x == "json").unwrap_or(false))
            .collect();
        out.sort();
        out
    };

    for p in files_in("elements") {
        let base = p.file_name().unwrap().to_string_lossy().to_string();
        if base.starts_with("DLC_") {
            d.note_skip("element-file", &base, "DLC-prefixed file");
            continue;
        }
        if SKIP_ELEM_FILES.contains(&base.as_str()) {
            d.note_skip("element-file", &base, "junk or unprefixed-DLC leak file");
            continue;
        }
        match read(&p) {
            Ok(v) => {
                for e in list_of(&v, "elements") {
                    let Some(id) = e.get("id").and_then(|x| x.as_str()) else {
                        continue;
                    };
                    if d.elements.contains_key(id) {
                        d.note_skip("element", id, "duplicate id");
                        continue;
                    }
                    d.elements.insert(id.to_string(), e.clone());
                }
            }
            Err(e) => d.note_skip("element-file", &base, &format!("unparseable: {e}")),
        }
    }

    for p in files_in("recipes") {
        let base = p.file_name().unwrap().to_string_lossy().to_string();
        if base.starts_with("DLC_") {
            d.note_skip("recipe-file", &base, "DLC-prefixed file");
            continue;
        }
        if SKIP_RECIPE_FILES.contains(&base.as_str()) {
            d.note_skip("recipe-file", &base, "debug or superseded file");
            continue;
        }
        match read(&p) {
            Ok(v) => {
                for r in list_of(&v, "recipes") {
                    ingest_recipe(&mut d, r);
                }
            }
            Err(e) => d.note_skip("recipe-file", &base, &format!("unparseable: {e}")),
        }
    }

    for p in files_in("decks") {
        let base = p.file_name().unwrap().to_string_lossy().to_string();
        if base.starts_with("DLC_") {
            d.note_skip("deck-file", &base, "DLC-prefixed file");
            continue;
        }
        match read(&p) {
            Ok(v) => {
                for dk in list_of(&v, "decks") {
                    let Some(id) = dk.get("id").and_then(|x| x.as_str()) else {
                        continue;
                    };
                    if id.starts_with("priest.") || id.starts_with("ghoul.") || id.starts_with("exile")
                    {
                        d.note_skip("deck", id, "unprefixed-DLC leak deck");
                        continue;
                    }
                    d.decks.entry(id.to_string()).or_insert_with(|| dk.clone());
                }
            }
            Err(e) => d.note_skip("deck-file", &base, &format!("unparseable: {e}")),
        }
    }

    match read(&root.join("endings").join("endings.json")) {
        Ok(v) => {
            for e in list_of(&v, "endings") {
                if let Some(id) = e.get("id").and_then(|x| x.as_str()) {
                    d.endings.insert(id.to_string(), e.clone());
                }
            }
        }
        Err(e) => d.note_skip("ending-file", "endings.json", &format!("unparseable: {e}")),
    }
    for p in files_in("endings") {
        let base = p.file_name().unwrap().to_string_lossy().to_string();
        if !base.starts_with("DLC_") || base == "endings.json" {
            continue;
        }
        if let Ok(v) = read(&p) {
            for e in list_of(&v, "endings") {
                if let Some(id) = e.get("id").and_then(|x| x.as_str()) {
                    d.endings.entry(id.to_string()).or_insert_with(|| e.clone());
                }
            }
        }
    }

    for (dir, key, table) in [
        ("verbs", "verbs", "verbs"),
        ("levers", "levers", "levers"),
        ("portals", "portals", "portals"),
    ] {
        for p in files_in(dir) {
            let base = p.file_name().unwrap().to_string_lossy().to_string();
            if base.starts_with("DLC_") {
                continue;
            }
            match read(&p) {
                Ok(v) => {
                    for x in list_of(&v, key) {
                        if let Some(id) = x.get("id").and_then(|i| i.as_str()) {
                            match table {
                                "verbs" => {
                                    d.verbs.insert(id.to_string(), x.clone());
                                }
                                "levers" => {
                                    d.levers.insert(id.to_string(), x.clone());
                                }
                                _ => {
                                    d.portals.insert(id.to_string(), x.clone());
                                }
                            }
                        }
                    }
                }
                Err(e) => d.note_skip("file", &base, &format!("unparseable: {e}")),
            }
        }
    }

    d
}

/// One recipe object: register it (first definition wins; `hintonly` never
/// executes), then extract the recipe definitions nested inside its
/// `linked`/`alt`/`inductions` entries.
fn ingest_recipe(d: &mut CsData, r: &serde_json::Value) {
    if let Some(id) = r.get("id").and_then(|x| x.as_str()) {
        if r.get("hintonly").map(|x| x.as_bool().unwrap_or(false)).unwrap_or(false) {
            d.note_skip("recipe", id, "hintonly: UI hint, never executes");
        } else if d.recipe_index.contains_key(id) {
            d.note_skip("recipe", id, "duplicate id");
        } else {
            d.recipe_index.insert(id.to_string(), d.recipes.len());
            d.recipes.push((id.to_string(), r.clone()));
        }
    }
    ingest_nested(d, r);
}

/// A chain entry is a full recipe definition when it carries any field
/// beyond the link modifiers (`id`, `chance`, `challenges`, `additional`)
/// and the linked-slot spec `expulsion`; CS merges such entries into the
/// same recipe table, where other chains reference them by id. Their own
/// chains recurse through the same rule.
fn ingest_nested(d: &mut CsData, r: &serde_json::Value) {
    for k in ["linked", "alt", "inductions"] {
        for e in list_of(r, k) {
            let is_definition = e.as_object().is_some_and(|o| {
                o.keys().any(|kk| {
                    !matches!(kk.as_str(), "id" | "chance" | "challenges" | "additional" | "expulsion")
                })
            });
            if is_definition {
                ingest_recipe(d, e);
            }
        }
    }
}
