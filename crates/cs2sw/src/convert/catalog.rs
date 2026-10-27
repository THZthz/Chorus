//! Narrative presentation catalog extraction from Cultist Simulator core data.

use std::collections::BTreeMap;

use story_present::catalog::{
    CardPresentation, EndingPresentation, PresentationCatalog, RecipePresentation,
    SlotPresentation, VerbPresentation,
};

use super::source::s;
use super::Converter;
use crate::naming::mangle;

impl<'a> Converter<'a> {
    pub(super) fn extract_catalog(&self) -> PresentationCatalog {
        let mut catalog = PresentationCatalog::default();

        // 1. Elements (Cards): For each element that becomes a card, map its CS
        // `label` -> `title` and `description` -> `description`, stored in `catalog.cards[card_id]`.
        for id in &self.card_ids {
            let card_id = self.card_name(id).unwrap_or_else(|| id.clone());
            let e = self.d.element(id);
            let title = e.and_then(|x| s(x, "label")).map(String::from);
            let description = e.and_then(|x| s(x, "description")).map(String::from);
            let cp = CardPresentation { title, description };
            catalog.cards.insert(card_id.clone(), cp.clone());
            if card_id != *id {
                catalog.cards.insert(id.clone(), cp);
            }
        }

        // 2. Verbs: For each verb, map `label` -> `title` and `description` -> `description`,
        // stored in `catalog.verbs[verb_id]`.
        for (vid, v) in &self.d.verbs {
            let title = s(v, "label").map(String::from);
            let description = s(v, "description").map(String::from);
            let mut slots = BTreeMap::new();
            if let Some(sl) = v.get("slot") {
                let sl_label = s(sl, "label").map(String::from);
                let sl_desc = s(sl, "description").map(String::from);
                if sl_label.is_some() || sl_desc.is_some() {
                    let sp = SlotPresentation {
                        label: sl_label,
                        description: sl_desc,
                    };
                    if let Some(raw_id) = s(sl, "id") {
                        slots.insert(raw_id.to_string(), sp.clone());
                        let mangled = mangle(raw_id);
                        if mangled != raw_id {
                            slots.insert(mangled, sp.clone());
                        }
                        let prefixed = mangle(&format!("{vid}_{raw_id}"));
                        slots.insert(prefixed, sp);
                    }
                }
            }
            let vp = VerbPresentation {
                title,
                description,
                slots,
            };
            catalog.verbs.insert(vid.clone(), vp.clone());
            if let Some(vname) = self.names.verb.get(vid) {
                if vname != vid {
                    catalog.verbs.insert(vname.clone(), vp);
                }
            }
        }

        // 3. Recipes: For each recipe, map:
        //    - `label` -> `recipe_presentation.label`
        //    - `startdescription` -> `recipe_presentation.start`
        //    - `description` -> `recipe_presentation.completion`
        //    - Recipe slots: for each slot, map `label` -> `slot_presentation.label`
        //      and `description` -> `slot_presentation.description` in `recipe_presentation.slots[slot_name]`.
        //    Store in `catalog.recipes[recipe_id]` (and recipe name).
        for (rid, r) in &self.d.recipes {
            let label = s(r, "label").map(String::from);
            let start = s(r, "startdescription").map(String::from);
            let completion = s(r, "description").map(String::from);

            let mut slots = BTreeMap::new();
            if let Some(source_slots) = r.get("slots").and_then(|x| x.as_array()) {
                for (i, sl) in source_slots.iter().enumerate() {
                    let sl_label = s(sl, "label").map(String::from);
                    let sl_desc = s(sl, "description").map(String::from);
                    if sl_label.is_some() || sl_desc.is_some() {
                        let sp = SlotPresentation {
                            label: sl_label,
                            description: sl_desc,
                        };
                        if let Some(raw_id) = s(sl, "id") {
                            slots.insert(raw_id.to_string(), sp.clone());
                            let mangled = mangle(raw_id);
                            if mangled != raw_id {
                                slots.insert(mangled, sp.clone());
                            }
                        }
                        if let Some(plan) = self.plans.get(rid) {
                            if let Some(ps) = plan.slots.get(i) {
                                slots.insert(ps.name.clone(), sp);
                            }
                        }
                    }
                }
            }

            let rp = RecipePresentation {
                label,
                start,
                completion,
                slots,
            };

            catalog.recipes.insert(rid.clone(), rp.clone());

            let mut local_names: Vec<String> = self
                .names
                .recipe
                .iter()
                .filter(|((_, r), _)| r == rid)
                .map(|(_, l)| l.clone())
                .collect();
            if local_names.is_empty() {
                local_names.push(mangle(rid));
            }
            for name in local_names {
                catalog.recipes.insert(name, rp.clone());
            }
            for ((v, r), local) in &self.names.recipe {
                if r == rid {
                    catalog.recipes.insert(format!("{v}.{local}"), rp.clone());
                }
            }
        }

        // 4. Endings: For each ending, map `label`/`title` -> `title` and `description` -> `text`
        // in `catalog.endings[ending_id]`.
        for (eid, e) in &self.d.endings {
            let title = s(e, "label").or_else(|| s(e, "title")).map(String::from);
            let text = s(e, "description").map(String::from);
            let tier = s(e, "flavour").map(|f| mangle(f));
            let ep = EndingPresentation { title, tier, text };

            catalog.endings.insert(eid.clone(), ep.clone());
            if let Some(name) = self.names.ending.get(eid) {
                if name != eid {
                    catalog.endings.insert(name.clone(), ep.clone());
                }
            }
        }

        for ((end, flavour), var_name) in &self.names.ending_variant {
            if let Some(base) = self.d.endings.get(end) {
                let title = s(base, "label")
                    .or_else(|| s(base, "title"))
                    .map(String::from);
                let text = s(base, "description").map(String::from);
                let tier = Some(mangle(flavour));
                catalog
                    .endings
                    .insert(var_name.clone(), EndingPresentation { title, tier, text });
            }
        }

        catalog
    }
}
