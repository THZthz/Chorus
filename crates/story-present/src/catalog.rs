//! Presentation catalog definition and two-tier resolution cascade.

use std::collections::BTreeMap;
use std::path::Path;

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct PresentationCatalog {
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub cards: BTreeMap<String, CardPresentation>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub verbs: BTreeMap<String, VerbPresentation>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub recipes: BTreeMap<String, RecipePresentation>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub cues: BTreeMap<String, String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub endings: BTreeMap<String, EndingPresentation>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct CardPresentation {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct VerbPresentation {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub slots: BTreeMap<String, SlotPresentation>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct RecipePresentation {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub start: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub completion: Option<String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub slots: BTreeMap<String, SlotPresentation>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct SlotPresentation {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct EndingPresentation {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tier: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
}

impl PresentationCatalog {
    pub fn from_json(json: &str) -> Result<Self, serde_json::Error> {
        serde_json::from_str(json)
    }

    pub fn to_json_pretty(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string_pretty(self)
    }

    pub fn from_file(path: &Path) -> Result<Self, std::io::Error> {
        let text = std::fs::read_to_string(path)?;
        serde_json::from_str(&text)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))
    }

    pub fn save_to_file(&self, path: &Path) -> Result<(), std::io::Error> {
        let text = self
            .to_json_pretty()
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
        std::fs::write(path, text)
    }

    pub fn card_title<'a>(
        &'a self,
        card_id: &'a str,
        content: &'a story_content::Content,
    ) -> &'a str {
        if let Some(cp) = self.cards.get(card_id) {
            if let Some(ref title) = cp.title {
                return title;
            }
        }
        if let Some(card) = content.cards.get(card_id) {
            if let Some(title) = card.meta.get("title") {
                return title;
            }
        }
        card_id
    }

    pub fn card_desc<'a>(
        &'a self,
        card_id: &'a str,
        content: &'a story_content::Content,
    ) -> Option<&'a str> {
        if let Some(cp) = self.cards.get(card_id) {
            if let Some(ref desc) = cp.description {
                return Some(desc);
            }
        }
        content.cards.get(card_id).and_then(|c| c.desc.as_deref())
    }

    pub fn verb_title<'a>(
        &'a self,
        verb_id: &'a str,
        content: &'a story_content::Content,
    ) -> &'a str {
        if let Some(vp) = self.verbs.get(verb_id) {
            if let Some(ref title) = vp.title {
                return title;
            }
        }
        if let Some(verb) = content.verbs.get(verb_id) {
            if let Some(ref title) = verb.title {
                return title;
            }
        }
        verb_id
    }

    pub fn verb_desc<'a>(
        &'a self,
        verb_id: &str,
        content: &'a story_content::Content,
    ) -> Option<&'a str> {
        if let Some(vp) = self.verbs.get(verb_id) {
            if let Some(ref desc) = vp.description {
                return Some(desc);
            }
        }
        content
            .verbs
            .get(verb_id)
            .and_then(|v| v.meta.get("description").map(|s| s.as_str()))
    }

    pub fn recipe_label<'a>(
        &'a self,
        qualified_id: &str,
        recipe: &'a story_content::ir::RecipeDecl,
    ) -> &'a str {
        let rp = self
            .recipes
            .get(qualified_id)
            .or_else(|| self.recipes.get(&recipe.name));
        if let Some(rp) = rp {
            if let Some(ref label) = rp.label {
                return label;
            }
        }
        recipe.label.as_deref().unwrap_or(&recipe.name)
    }

    pub fn recipe_start<'a>(
        &'a self,
        qualified_id: &str,
        recipe: &'a story_content::ir::RecipeDecl,
    ) -> Option<&'a str> {
        let rp = self
            .recipes
            .get(qualified_id)
            .or_else(|| self.recipes.get(&recipe.name));
        if let Some(rp) = rp {
            if let Some(ref start) = rp.start {
                return Some(start);
            }
        }
        recipe.meta.get("start").map(|s| s.as_str())
    }

    pub fn recipe_completion<'a>(
        &'a self,
        qualified_id: &str,
        recipe: &'a story_content::ir::RecipeDecl,
    ) -> Option<&'a str> {
        let rp = self
            .recipes
            .get(qualified_id)
            .or_else(|| self.recipes.get(&recipe.name));
        if let Some(rp) = rp {
            if let Some(ref comp) = rp.completion {
                return Some(comp);
            }
        }
        recipe
            .meta
            .get("completion")
            .or_else(|| recipe.meta.get("description"))
            .map(|s| s.as_str())
    }

    pub fn slot_label<'a>(&'a self, qualified_id: &str, slot_name: &'a str) -> &'a str {
        let rp = self.recipes.get(qualified_id).or_else(|| {
            qualified_id
                .split_once('.')
                .and_then(|(_, r)| self.recipes.get(r))
        });
        if let Some(rp) = rp {
            if let Some(sp) = rp.slots.get(slot_name) {
                if let Some(ref label) = sp.label {
                    return label;
                }
            }
        }
        let verb = qualified_id
            .split_once('.')
            .map(|(v, _)| v)
            .unwrap_or(qualified_id);
        if let Some(vp) = self.verbs.get(verb) {
            if let Some(sp) = vp.slots.get(slot_name) {
                if let Some(ref label) = sp.label {
                    return label;
                }
            }
        }
        slot_name
    }

    pub fn slot_desc<'a>(&'a self, qualified_id: &str, slot_name: &str) -> Option<&'a str> {
        let rp = self.recipes.get(qualified_id).or_else(|| {
            qualified_id
                .split_once('.')
                .and_then(|(_, r)| self.recipes.get(r))
        });
        if let Some(rp) = rp {
            if let Some(sp) = rp.slots.get(slot_name) {
                if let Some(ref desc) = sp.description {
                    return Some(desc);
                }
            }
        }
        let verb = qualified_id
            .split_once('.')
            .map(|(v, _)| v)
            .unwrap_or(qualified_id);
        if let Some(vp) = self.verbs.get(verb) {
            if let Some(sp) = vp.slots.get(slot_name) {
                if let Some(ref desc) = sp.description {
                    return Some(desc);
                }
            }
        }
        None
    }

    pub fn cue_prose<'a>(&'a self, cue_id: &'a str) -> &'a str {
        self.cues.get(cue_id).map(|s| s.as_str()).unwrap_or(cue_id)
    }

    pub fn ending_prose<'a>(
        &'a self,
        ending_id: &'a str,
        ending: Option<&'a story_content::ir::EndingDecl>,
    ) -> (&'a str, &'a str, Option<&'a str>) {
        let ep = self.endings.get(ending_id);
        let default_title = ending.map(|e| e.title.as_str()).unwrap_or(ending_id);
        let default_tier = ending.map(|e| e.tier.as_str()).unwrap_or("somber");
        let default_text = ending.and_then(|e| e.text.as_deref());

        let title = ep.and_then(|e| e.title.as_deref()).unwrap_or(default_title);
        let tier = ep.and_then(|e| e.tier.as_deref()).unwrap_or(default_tier);
        let text = ep.and_then(|e| e.text.as_deref()).or(default_text);
        (title, tier, text)
    }
}
