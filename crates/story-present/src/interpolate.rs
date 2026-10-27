//! Token interpolation for prose templates.

use crate::catalog::PresentationCatalog;
use std::collections::BTreeMap;
use story_content::Content;
use story_core::{CardId, EntityId, RecipeId, SituationId, SlotName, State, VerbId};

#[derive(Clone, Debug, Default)]
pub struct PresentationContext {
    pub sit: Option<SituationId>,
    pub recipe: Option<RecipeId>,
    pub verb: Option<VerbId>,
    pub bound_cards: BTreeMap<SlotName, Vec<CardId>>,
    pub subject: Option<EntityId>,
    pub subject_card: Option<CardId>,
    pub roll: Option<i64>,
    pub vars: BTreeMap<String, String>,
}

pub fn interpolate(
    template: &str,
    state: &State,
    content: &Content,
    catalog: &PresentationCatalog,
    ctx: &PresentationContext,
) -> String {
    let chars: Vec<char> = template.chars().collect();
    let mut out = String::new();
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == '{' {
            if let Some(close) = chars[i..].iter().position(|c| *c == '}') {
                let tok: String = chars[i + 1..i + close].iter().collect();
                if let Some(rendered) = render_token(&tok, state, content, catalog, ctx) {
                    out.push_str(&rendered);
                } else {
                    out.push('{');
                    out.push_str(&tok);
                    out.push('}');
                }
                i += close + 1;
                continue;
            }
        }
        out.push(chars[i]);
        i += 1;
    }
    out
}

fn render_token(
    tok: &str,
    state: &State,
    content: &Content,
    catalog: &PresentationCatalog,
    ctx: &PresentationContext,
) -> Option<String> {
    // Custom presentation variables
    if let Some(v) = ctx.vars.get(tok) {
        return Some(v.clone());
    }

    // Legacy variable: {legacy.<key>}
    if let Some(key) = tok.strip_prefix("legacy.") {
        return Some(state.legacy.get(key).copied().unwrap_or(0).to_string());
    }

    // Related token: {related:kind of target} or {related:kind to target}
    if let Some(rest) = tok.strip_prefix("related:") {
        let (kind, target, incoming) = if let Some((k, t)) = rest.split_once(" of ") {
            (k.trim(), t.trim(), false)
        } else if let Some((k, t)) = rest.split_once(" to ") {
            (k.trim(), t.trim(), true)
        } else {
            return None;
        };

        let target_ent = resolve_target_entity(target, state, content, ctx);
        if let Some(tid) = target_ent {
            let related_ids: Vec<EntityId> = if !incoming {
                state
                    .relations
                    .get(&tid)
                    .and_then(|kinds| kinds.get(kind))
                    .map(|set| set.iter().copied().collect())
                    .unwrap_or_default()
            } else {
                state
                    .relations
                    .iter()
                    .filter_map(|(&subj, kinds)| {
                        if kinds.get(kind).map(|s| s.contains(&tid)).unwrap_or(false) {
                            Some(subj)
                        } else {
                            None
                        }
                    })
                    .collect()
            };
            return Some(render_entity_names(&related_ids, state, content, catalog));
        }
        return None;
    }

    // Dot navigation: {target.aspect}
    if let Some((head, key)) = tok.split_once('.') {
        if head == "subject" {
            if let Some(s) = ctx.subject {
                return Some(
                    state
                        .entities
                        .get(&s)
                        .and_then(|e| e.aspects.get(key).copied())
                        .unwrap_or(0)
                        .to_string(),
                );
            }
            return Some("0".to_string());
        }

        // Try slot name first
        let slot_name = SlotName::from(head);
        if let Some(cards) = ctx.bound_cards.get(&slot_name) {
            if let Some(first_card) = cards.first() {
                if let Some(card_decl) = content.cards.get(first_card) {
                    return Some(card_decl.aspects.get(key).copied().unwrap_or(0).to_string());
                }
            }
        }

        // Try named entity in state
        if let Some(ent) = state
            .entities
            .values()
            .find(|e| e.prototype.as_str() == head || e.name == head)
        {
            return Some(ent.aspects.get(key).copied().unwrap_or(0).to_string());
        }

        // Try card decl in content
        if let Some(card_decl) = content.cards.get(head) {
            return Some(card_decl.aspects.get(key).copied().unwrap_or(0).to_string());
        }

        return None;
    }

    // Slot binding: {slot_name}
    let slot_name = SlotName::from(tok);
    if let Some(cards) = ctx.bound_cards.get(&slot_name) {
        if !cards.is_empty() {
            let names: Vec<&str> = cards
                .iter()
                .map(|c| catalog.card_title(c.as_str(), content))
                .collect();
            return Some(names.join(", "));
        } else {
            return Some(String::new());
        }
    }

    // Roll binding
    if tok == "roll" {
        if let Some(r) = ctx.roll {
            return Some(r.to_string());
        }
        return None;
    }

    // Subject title
    if tok == "subject" {
        if let Some(ref sc) = ctx.subject_card {
            return Some(catalog.card_title(sc.as_str(), content).to_string());
        }
        if let Some(s) = ctx.subject {
            if let Some(ent) = state.entities.get(&s) {
                return Some(
                    catalog
                        .card_title(ent.prototype.as_str(), content)
                        .to_string(),
                );
            }
        }
        return None;
    }

    // Fallback: if it names a card in content
    if content.cards.contains_key(tok) {
        return Some(catalog.card_title(tok, content).to_string());
    }

    None
}

fn resolve_target_entity(
    target: &str,
    state: &State,
    _content: &Content,
    ctx: &PresentationContext,
) -> Option<EntityId> {
    if target == "subject" {
        return ctx.subject;
    }
    let slot_name = SlotName::from(target);
    if let Some(cards) = ctx.bound_cards.get(&slot_name) {
        if let Some(c) = cards.first() {
            if let Some(e) = state.entities.values().find(|e| e.prototype == *c) {
                return Some(e.id);
            }
        }
    }
    if let Some(inner) = target
        .strip_prefix("first(")
        .and_then(|s| s.strip_suffix(')'))
    {
        let inner = inner.trim();
        if let Some(s) = inner.strip_prefix("slot:") {
            let sn = SlotName::from(s.trim());
            if let Some(cards) = ctx.bound_cards.get(&sn) {
                if let Some(c) = cards.first() {
                    if let Some(e) = state.entities.values().find(|e| e.prototype == *c) {
                        return Some(e.id);
                    }
                }
            }
        }
        for term in inner.split_whitespace() {
            if let Some(n) = term.strip_prefix("name:") {
                if let Some(e) = state
                    .entities
                    .values()
                    .find(|e| e.name == n || e.prototype.as_str() == n)
                {
                    return Some(e.id);
                }
            }
        }
    }
    state
        .entities
        .values()
        .find(|e| e.name == target || e.prototype.as_str() == target)
        .map(|e| e.id)
}

fn render_entity_names(
    ids: &[EntityId],
    state: &State,
    content: &Content,
    catalog: &PresentationCatalog,
) -> String {
    let titles: Vec<&str> = ids
        .iter()
        .filter_map(|id| state.entities.get(id))
        .map(|e| catalog.card_title(e.prototype.as_str(), content))
        .collect();
    titles.join(", ")
}
