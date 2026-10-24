//! Pass 3: file-level emission — header, predicates, cards, decks,
//! endings, and verbs.

use std::collections::BTreeSet;

use serde_json::Value;

use crate::naming::mangle;

use super::source::{
    escape_sw, num, pairs, s, str_list, truthy, KIND_ORDER, PLAYER_VERBS, START_IDS, TIME_VERB,
};
use super::{Class, Converter, SEED};

impl<'a> Converter<'a> {
    // -- pass 3: emission ---------------------------------------------------

    pub(super) fn emit(&mut self) -> String {
        let mut out: Vec<String> = Vec::new();
        out.push(
            "# cultist-simulator.sw — converted from the Cultist Simulator core content tree"
                .into(),
        );
        out.push("# by cs2sw: a mechanical, table-driven map of elements, recipes, decks,".into());
        out.push("# endings, and levers onto the .sw grammar. Generated file; edit the".into());
        out.push("# converter, not this text.".into());
        out.push(String::new());
        out.push("game \"Cultist Simulator\"".into());
        out.push(format!("  seed {SEED}"));
        out.push("  turn 1m".into());
        out.push("  home table".into());
        out.push(String::new());
        out.push("zones".into());
        out.push("  table".into());
        out.push(String::new());

        out.push("aspects".into());
        let mut seen_keys = BTreeSet::new();
        for id in self.d.elements.keys() {
            let k = self.key(id);
            if seen_keys.insert(k.clone()) {
                out.push(format!("  {k}: 0"));
            }
        }
        for k in &self.registry {
            if self.d.elements.contains_key(k.as_str()) {
                continue;
            }
            let kk = self.key(k);
            if seen_keys.insert(kk.clone()) {
                out.push(format!("  {kk}: 0"));
            }
        }
        out.push(String::new());

        self.emit_predicates(&mut out);
        self.emit_cards(&mut out);
        self.emit_decks(&mut out);
        self.emit_endings(&mut out);
        self.emit_verbs(&mut out);
        out.join("\n") + "\n"
    }

    /// The derived predicates the disjunctive slot filters call (language.md
    /// §2.5); emitted beside the cards, before any use.
    fn emit_predicates(&mut self, out: &mut Vec<String>) {
        if self.preds.is_empty() {
            return;
        }
        out.push("predicates".into());
        for (name, body) in self.preds.clone() {
            out.push(format!("  predicate {name}(p) = {body}"));
        }
        out.push(String::new());
    }

    fn kind_of(&self, id: &str, e: &Value) -> &'static str {
        let own = pairs(e, "aspects");
        for (marker, kind) in KIND_ORDER {
            if *marker == id || own.iter().any(|(k, _)| k == *marker) {
                return kind;
            }
        }
        "state"
    }

    fn uniqueness(&self, e: &Value) -> String {
        if let Some(g) = s(e, "uniquenessgroup") {
            let gname = self
                .names
                .group
                .get(g)
                .cloned()
                .unwrap_or_else(|| mangle(g));
            format!("group {gname}")
        } else if truthy(e, "unique") {
            "unique".to_string()
        } else {
            "stackable".to_string()
        }
    }

    /// A card to mint or swap to: a resolvable, non-aspect element.
    pub(super) fn target_card(&self, id: &str) -> Option<String> {
        if self.is_aspect(id) {
            return None;
        }
        self.card_name(id)
    }

    pub(super) fn roll_bind(&mut self) -> String {
        self.roll_counter += 1;
        format!("roll{}", self.roll_counter)
    }

    fn emit_cards(&mut self, out: &mut Vec<String>) {
        out.push("cards".into());
        for id in self.card_ids.clone() {
            let Some(e) = self.d.element(&id).cloned() else {
                continue;
            };
            let Some(name) = self.card_name(&id) else {
                continue;
            };
            let mut lines: Vec<String> = Vec::new();
            let kind = self.kind_of(&id, &e);
            let uniq = self.uniqueness(&e);
            lines.push(format!("  card {name}"));
            lines.push(format!("    kind {kind}  {uniq}"));
            let mut pair_list: Vec<String> = vec![format!("{} 1", self.key(&id))];
            for (k, v) in self.aspects_of(&id) {
                pair_list.push(format!("{} {v}", self.key(&k)));
            }
            if let Some(g) = s(&e, "uniquenessgroup") {
                if self.read_groups.contains(g) {
                    pair_list.push(format!("{} 1", self.key(g)));
                }
            }
            lines.push(format!("    aspects {}", pair_list.join(", ")));
            if let Some(life) = num(&e, "lifetime") {
                if life > 0 {
                    lines.push(format!("    lifetime {life}m"));
                }
            }
            if START_IDS.contains(&id.as_str()) {
                lines.push("    start in table".into());
            }
            if let Some(desc) = s(&e, "description") {
                let d = escape_sw(desc);
                if !d.is_empty() {
                    lines.push(format!("    desc \"{d}\""));
                }
            }

            // A card's non-consuming element slots ride as `0..1 open` merged
            // slots on every recipe whose requirements or slot filters admit
            // the card (plan_recipe). Where no recipe admits it — CS opens the
            // sphere on the resting card, which the map has no host for — the
            // storage slot binds nowhere and is named here.
            let unmerged_storage_slot = e
                .get("slots")
                .and_then(|x| x.as_array())
                .map(|a| {
                    a.iter()
                        .filter(|sl| !truthy(sl, "consumes"))
                        .any(|sl| {
                            let raw = s(sl, "id").unwrap_or("slot").to_string();
                            !self
                                .merged_element_slots
                                .contains(&(id.clone(), raw))
                        })
                })
                .unwrap_or(false);
            if unmerged_storage_slot {
                self.gaps.hit(
                    "non-consuming element slot",
                    Class::ConverterIdiom,
                    "no recipe admits the element, so its storage slot has no host: CS's table-anchored spheres stay out of the map (in-situation slots merge into admitting recipes' slot blocks)",
                    &id,
                );
            }

            // Completion books, merged per qualifier (one `on complete`
            // block per aspect key): element xtriggers, element induces, and
            // consuming element slots.
            let mut books: Vec<(Option<String>, Vec<(String, Vec<String>)>)> = Vec::new();
            let mut rule_ids: BTreeSet<String> = BTreeSet::new();
            let push_rule = |books: &mut Vec<(Option<String>, Vec<(String, Vec<String>)>)>,
                             rule_ids: &mut BTreeSet<String>,
                             qual: Option<String>,
                             rule: String,
                             body: Vec<String>| {
                let mut rule = rule;
                let mut i = 2;
                while !rule_ids.insert(rule.clone()) {
                    rule = format!("{rule}_{i}");
                    i += 1;
                }
                match books.iter_mut().find(|(q, _)| *q == qual) {
                    Some((_, rules)) => rules.push((rule, body)),
                    None => books.push((qual, vec![(rule, body)])),
                }
            };

            // Element xtriggers -> aspect-qualified completion books; the
            // catalyst aspect must be declared by the completing recipe.
            let xtrs: Vec<(String, Value)> = e
                .get("xtriggers")
                .and_then(|x| x.as_object())
                .map(|o| o.iter().map(|(k, v)| (k.clone(), v.clone())).collect())
                .unwrap_or_default();
            for (cat, tgt) in xtrs {
                let Some(tgt_id) = tgt.as_str().map(String::from) else {
                    self.gaps.hit(
                        "xtrigger with a non-element target",
                        Class::NonGoal,
                        "the target is not an element id; no book is emitted",
                        &format!("{id}:{cat}"),
                    );
                    continue;
                };
                let Some(tname) = self.target_card(&tgt_id) else {
                    self.gaps.hit(
                        "xtrigger target is an aspect or unknown element",
                        Class::NonGoal,
                        "the replacement has no card; no book is emitted",
                        &format!("{id}:{cat} -> {tgt_id}"),
                    );
                    continue;
                };
                let rule = format!("xtr_{}_{}", name, self.key(&cat));
                let body = vec![format!("swap subject.{} to {tname}", self.key(&id))];
                push_rule(&mut books, &mut rule_ids, Some(self.key(&cat)), rule, body);
            }

            // decayTo -> an expiry book swapping to the decay target.
            if let Some(dt) = s(&e, "decayTo").map(String::from) {
                match self.target_card(&dt) {
                    Some(tname) => {
                        let rule = format!("decay_{name}");
                        lines.push("    on expire".into());
                        lines.push(format!("      rule {rule}"));
                        lines.push("        effects".into());
                        lines.push(format!(
                            "          swap subject.{} to {tname}",
                            self.key(&id)
                        ));
                    }
                    None => {
                        self.gaps.hit(
                            "decayTo target is an aspect or unknown element",
                            Class::NonGoal,
                            "the decay replacement has no card; no book is emitted",
                            &format!("{id} -> {dt}"),
                        );
                    }
                }
            }

            // Element `induces` -> an aspect-qualified completion book that
            // starts the induced recipe.
            let induces = e
                .get("induces")
                .and_then(|x| x.as_array())
                .cloned()
                .unwrap_or_default();
            for ind in &induces {
                let Some(tid) = s(ind, "id").map(String::from) else {
                    continue;
                };
                let Some((verb, local)) = self.recipe_target(&tid) else {
                    continue;
                };
                let chance = num(ind, "chance").unwrap_or(100);
                let rule = format!("ind_{name}_{}", mangle(&tid));
                let mut body: Vec<String> = Vec::new();
                if chance < 100 {
                    let bind = self.roll_bind();
                    body.push(format!("roll 1d100 as {bind}"));
                    body.push(format!("if {bind} <= {chance}"));
                    body.push(format!("  start {verb} via {local}"));
                } else {
                    body.push(format!("start {verb} via {local}"));
                }
                self.note_start(&tid);
                push_rule(&mut books, &mut rule_ids, Some(self.key(&id)), rule, body);
            }

            // Consuming element slots: the merge sites are settled in the
            // plan pass; anything unmerged is named here. The old
            // unqualified consume-book approximation is gone — it fired
            // after the completion's swap books and ate cards a consuming
            // slot would have burned earlier (the rite eating its own
            // freshly summoned spirit).
            let slots: Vec<(String, Vec<String>)> = e
                .get("slots")
                .and_then(|x| x.as_array())
                .map(|a| {
                    a.iter()
                        .filter(|sl| truthy(sl, "consumes"))
                        .map(|sl| {
                            let mut keys: Vec<String> = Vec::new();
                            for dk in ["required", "essential"] {
                                for (k, _) in pairs(sl, dk) {
                                    keys.push(k);
                                }
                            }
                            keys.sort();
                            let raw = s(sl, "id").unwrap_or("slot").to_string();
                            (raw, keys)
                        })
                        .collect()
                })
                .unwrap_or_default();
            for (raw, keys) in slots {
                if self
                    .merged_element_slots
                    .contains(&(id.clone(), raw.clone()))
                {
                    continue;
                }
                if keys.is_empty() {
                    self.gaps.hit(
                        "consuming element slot without a filter",
                        Class::NonGoal,
                        "nothing identifies the burned card; no merge site can carry it",
                        &id,
                    );
                    continue;
                }
                self.gaps.hit(
                    "consuming element slot with no host recipe",
                    Class::ConverterIdiom,
                    "no emitted recipe's requirements or slot filters admit this element, so the consuming slot has no slot block to merge into and its burn is dropped",
                    &format!("{id}:{raw}"),
                );
            }

            for (qual, rules) in books {
                match &qual {
                    Some(q) => lines.push(format!("    on complete [{q}]")),
                    None => lines.push("    on complete".into()),
                }
                for (rule, body) in rules {
                    lines.push(format!("      rule {rule}"));
                    lines.push("        effects".into());
                    for l in body {
                        lines.push(format!("          {l}"));
                    }
                }
            }

            out.extend(lines);
            out.push(String::new());
            self.stats.cards_out += 1;
        }
    }

    fn emit_decks(&mut self, out: &mut Vec<String>) {
        let has_regular = !self.d.decks.is_empty();
        let internal_ids: Vec<String> = self
            .d
            .recipes
            .iter()
            .filter(|(_, r)| r.get("internaldeck").is_some())
            .map(|(id, _)| id.clone())
            .collect();
        if !has_regular && internal_ids.is_empty() {
            return;
        }
        out.push("decks".into());
        for (id, dk) in self.d.decks.clone() {
            let Some(dname) = self.names.deck.get(&id).cloned() else {
                continue;
            };
            let Some(mut lines) = self.deck_lines(&dname, &dk, &id) else {
                self.gaps.hit(
                    "deck with an empty spec",
                    Class::NonGoal,
                    "nothing resolvable to deal; no deck is emitted",
                    &id,
                );
                continue;
            };
            self.gap_relief(&dk, &id);
            let entries_line = lines.remove(1);
            out.push(lines.remove(0));
            out.push(entries_line);
            out.extend(lines);
            self.stats.decks_out += 1;
        }
        for rid in internal_ids {
            let Some(r) = self.d.recipe(&rid).cloned() else {
                continue;
            };
            let idk = r.get("internaldeck").cloned().unwrap_or_default();
            let raw = format!("deck_{rid}");
            let dname = self.names.reserve_id(&raw);
            self.internal_deck.insert(rid.clone(), dname.clone());
            let Some(mut lines) = self.deck_lines(&dname, &idk, &rid) else {
                self.gaps.hit(
                    "internaldeck with an empty spec",
                    Class::NonGoal,
                    "nothing resolvable to deal; no deck is emitted",
                    &rid,
                );
                self.internal_deck.remove(&rid);
                continue;
            };
            self.gap_relief(&idk, &rid);
            let entries_line = lines.remove(1);
            out.push(lines.remove(0));
            out.push(entries_line);
            out.extend(lines);
            self.stats.decks_out += 1;
        }
        out.push(String::new());
    }

    /// The lines for one deck; `None` drops the deck (empty spec).
    fn deck_lines(&mut self, dname: &str, dk: &Value, id: &str) -> Option<Vec<String>> {
        let mut entries: Vec<String> = Vec::new();
        for sp in str_list(dk, "spec") {
            if self.is_aspect(&sp) {
                self.gaps.hit(
                    "deck spec entry that is an aspect",
                    Class::NonGoal,
                    "aspects have no cards; the entry is dropped",
                    &format!("{id}:{sp}"),
                );
                continue;
            }
            match self.card_name(&sp) {
                Some(n) => entries.push(n),
                None => {
                    self.gaps.hit(
                        "deck spec entry with no card",
                        Class::NonGoal,
                        "the element is unknown; the entry is dropped",
                        &format!("{id}:{sp}"),
                    );
                }
            }
        }
        if entries.is_empty() {
            return None;
        }
        let mut lines = vec![
            format!("  deck {dname}"),
            format!("    entries {}", entries.join(", ")),
        ];
        let mut relieved = false;
        if let Some(dc) = s(dk, "defaultcard").and_then(|dc| self.target_card(dc)) {
            lines.push(format!("    fallback {dc}"));
            relieved = true;
        }
        if truthy(dk, "resetonexhaustion") {
            lines.push("    wrap".into());
            relieved = true;
        }
        if !relieved {
            // Every deck needs a relief path: the deck's own first entry
            // serves when CS declares none.
            let first = entries[0].clone();
            lines.push(format!("    fallback {first}"));
        }
        Some(lines)
    }

    fn gap_relief(&mut self, dk: &Value, id: &str) {
        let relieved = s(dk, "defaultcard")
            .map(|dc| self.target_card(dc).is_some())
            .unwrap_or(false)
            || truthy(dk, "resetonexhaustion");
        if !relieved {
            self.gaps.hit(
                "deck without a declared relief path",
                Class::ConverterIdiom,
                "the deck's first entry is emitted as the fallback so exhaustion never fails",
                id,
            );
        }
    }

    fn emit_endings(&mut self, out: &mut Vec<String>) {
        out.push("endings".into());
        for (id, e) in self.d.endings.clone() {
            let Some(name) = self.names.ending.get(&id).cloned() else {
                continue;
            };
            let title = escape_sw(s(&e, "label").unwrap_or(&id));
            let tier = mangle(s(&e, "flavour").unwrap_or("Melancholy"));
            out.push(format!("  ending {name} \"{title}\" {tier}"));
            if let Some(text) = s(&e, "description") {
                let t = escape_sw(text);
                if !t.is_empty() {
                    out.push(format!("    text \"{t}\""));
                }
            }
            self.stats.endings_out += 1;
        }
        for ((end, flavour), name) in self.names.ending_variant.clone() {
            let Some(base) = self.d.endings.get(&end).cloned() else {
                continue;
            };
            let title = escape_sw(s(&base, "label").unwrap_or(&end));
            let tier = mangle(&flavour);
            out.push(format!("  ending {name} \"{title}\" {tier}"));
            self.stats.endings_out += 1;
        }
        out.push(String::new());
    }

    fn verb_title(&self, action: &str) -> String {
        let label = self
            .d
            .verbs
            .get(action)
            .and_then(|v| s(v, "label"))
            .unwrap_or(action);
        escape_sw(label)
    }

    fn emit_verbs(&mut self, out: &mut Vec<String>) {
        out.push("verbs".into());
        for (verb, recipes) in self.by_verb() {
            let vname = self
                .names
                .verb
                .get(&verb)
                .cloned()
                .unwrap_or_else(|| verb.clone());
            let (mode, instances) = if PLAYER_VERBS.contains(&verb.as_str()) {
                ("committable", "1")
            } else if verb == TIME_VERB {
                ("autonomous", "1")
            } else {
                ("uncommittable", "any")
            };
            let action = verb.strip_suffix("_chains").unwrap_or(&verb);
            let title = self.verb_title(action);
            out.push(format!("  verb {vname} {mode} instances {instances}"));
            out.push(format!("    title \"{title}\""));
            out.push(String::new());

            for (i, rid) in recipes.iter().enumerate() {
                let Some(local) = self
                    .names
                    .recipe
                    .get(&(verb.clone(), rid.to_string()))
                    .cloned()
                else {
                    continue;
                };
                let Some(r) = self.d.recipe(rid).cloned() else {
                    continue;
                };
                self.emit_recipe(out, &verb, rid, &local, i, &r);
                self.stats.recipes_out += 1;
            }
            self.stats.verbs_out += 1;
            out.push(String::new());
        }
    }
}
