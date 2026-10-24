//! Narrate-template interpolation and the `related:` token reader
//! (language.md §7).

use super::EffectCtx;
use crate::Engine;
use story_content::ir::{AspectTerm, CmpOp, QueryTerm, Scope};
use story_core::EntityId;

impl Engine {
    /// Prose token interpolation (language.md §7): slots, unique-card name
    /// paths, roll bindings, spawn handles, `{legacy.<key>}`, and `subject`.
    pub(crate) fn interpolate(
        &self,
        template: &str,
        fctx: &EffectCtx,
        recipe: Option<&story_core::RecipeId>,
    ) -> String {
        let chars: Vec<char> = template.chars().collect();
        let mut out = String::new();
        let mut i = 0;
        while i < chars.len() {
            if chars[i] == '{' {
                if let Some(close) = chars[i..].iter().position(|c| *c == '}') {
                    let tok: String = chars[i + 1..i + close].iter().collect();
                    out.push_str(&self.render_token(&tok, fctx, recipe));
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
        &self,
        tok: &str,
        fctx: &EffectCtx,
        recipe: Option<&story_core::RecipeId>,
    ) -> String {
        let ctx = fctx.eval_ctx(recipe);
        if let Some(key) = tok.strip_prefix("legacy.") {
            return self.state.legacy.get(key).copied().unwrap_or(0).to_string();
        }
        if let Some(rest) = tok.strip_prefix("related:") {
            let ids = self.related_token_entities(rest, &ctx);
            return self.render_names(&ids);
        }
        if let Some((head, key)) = tok.split_once('.') {
            if head == "subject" {
                return match ctx.subject {
                    Some(s) => self.read(s, key).to_string(),
                    None => "0".to_string(),
                };
            }
            if let Some(pool) = fctx.handles.get(head) {
                return match pool.first() {
                    Some(&oldest) => self.read(oldest, key).to_string(),
                    None => "0".to_string(),
                };
            }
            return match self.named_entity(head) {
                Some(e) => self.read(e, key).to_string(),
                None => "0".to_string(),
            };
        }
        // A slot name renders its bound cards' names; a handle renders its
        // pool; a roll binding renders the last result; `subject` the card.
        let slot_cards = self.slot_bindings(ctx.sit, None, &story_core::SlotName::from(tok));
        if !slot_cards.is_empty() {
            return self.render_names(&slot_cards);
        }
        if let Some(pool) = fctx.handles.get(tok) {
            return self.render_names(pool);
        }
        if let Some(roll) = fctx.roll {
            return roll.to_string();
        }
        if let Some(s) = ctx.subject {
            return self
                .state
                .entities
                .get(&s)
                .map(|e| e.name.clone())
                .unwrap_or_default();
        }
        String::new()
    }

    fn render_names(&self, ids: &[EntityId]) -> String {
        ids.iter()
            .filter_map(|id| self.state.entities.get(id))
            .map(|e| e.name.clone())
            .collect::<Vec<_>>()
            .join(", ")
    }

    // -- related-token reading (language.md §7) -------------------------------
    //
    // `{related:<kind> (of|to) <card-ref>}` rides in the narrate template as
    // text; the checker (story-lang) admits exactly the spellings this
    // reader understands: a unique name, `subject`, or `first(<scope>
    // [<terms>])` over the aspect:/kind:/name:/- term forms.

    fn related_token_entities(&self, rest: &str, ctx: &crate::eval::Ctx) -> Vec<EntityId> {
        let Some((kind, after)) = rest.split_once(char::is_whitespace) else {
            return Vec::new();
        };
        let after = after.trim_start();
        let (incoming, ref_text) = match strip_direction_word(after) {
            Some((incoming, t)) => (incoming, t),
            None => return Vec::new(),
        };
        let Some(x) = self.parse_card_ref_text(ref_text, ctx) else {
            return Vec::new();
        };
        self.related_entities(x, kind, incoming)
    }

    /// The card-ref spelling of a `related:` token, resolved per read.
    fn parse_card_ref_text(&self, text: &str, ctx: &crate::eval::Ctx) -> Option<EntityId> {
        let r = self.parse_card_ref_ir(text)?;
        self.resolve_card_ref(&r, ctx)
    }

    /// The card-ref spelling as IR, so the shared resolution runs per read.
    fn parse_card_ref_ir(&self, text: &str) -> Option<story_content::ir::CardRef> {
        let text = text.trim();
        if text == "subject" {
            return Some(story_content::ir::CardRef::Subject);
        }
        if let Some(inner) = text.strip_prefix("first(") {
            let body = inner.strip_suffix(')')?.trim();
            let (scope_word, term_part) = match body.split_once(char::is_whitespace) {
                Some((w, t)) => (w, t.trim()),
                None => (body, ""),
            };
            let ctx = crate::eval::Ctx::default();
            let scope = self.parse_scope_text(scope_word, &ctx)?;
            let terms = self.parse_terms_text(term_part)?;
            return Some(story_content::ir::CardRef::Query { scope, terms });
        }
        Some(story_content::ir::CardRef::Name(story_core::CardId::from(
            text.to_string(),
        )))
    }

    fn parse_scope_text(&self, scope: &str, ctx: &crate::eval::Ctx) -> Option<Scope> {
        let _ = ctx;
        match scope {
            "world" => return Some(Scope::World),
            "interior" => return Some(Scope::Interior),
            "table" => return Some(Scope::Table),
            "anywhere" => return Some(Scope::Anywhere),
            "bound" => return Some(Scope::Bindings),
            _ => {}
        }
        if let Some(z) = scope.strip_prefix("zone:") {
            return Some(Scope::Zone(z.to_string()));
        }
        if let Some(s) = scope.strip_prefix("slot:") {
            return Some(Scope::Slot(story_core::SlotName::from(s.to_string())));
        }
        if let Some(rest) = scope.strip_prefix("related:") {
            let (kind, after) = rest.split_once(char::is_whitespace)?;
            let after = after.trim_start();
            let (incoming, ref_text) = match strip_direction_word(after) {
                Some((incoming, t)) => (incoming, t),
                None => return None,
            };
            let of = self.parse_card_ref_ir(ref_text)?;
            return Some(Scope::Related {
                kind: kind.to_string(),
                of: Box::new(of),
                incoming,
            });
        }
        None
    }

    fn parse_terms_text(&self, terms: &str) -> Option<Vec<QueryTerm>> {
        let mut out = Vec::new();
        for term in terms.split_whitespace() {
            if let Some(rest) = term.strip_prefix('-') {
                let mut inner = self.parse_terms_text(rest)?;
                out.push(QueryTerm::Not(Box::new(inner.remove(0))));
                continue;
            }
            if let Some(spec) = term.strip_prefix("aspect:") {
                out.push(parse_aspect_term(spec)?);
                continue;
            }
            if let Some(k) = term.strip_prefix("kind:") {
                out.push(QueryTerm::Kind(k.to_string()));
                continue;
            }
            if let Some(n) = term.strip_prefix("name:") {
                out.push(QueryTerm::Name(story_core::CardId::from(n.to_string())));
                continue;
            }
            return None;
        }
        Some(out)
    }
}

/// `of` / `to` after the kind, word-bounded; returns the incoming flag and
/// the card-ref text.
fn strip_direction_word(after: &str) -> Option<(bool, &str)> {
    for (word, incoming) in [("of", false), ("to", true)] {
        if let Some(t) = after.strip_prefix(word) {
            if t.is_empty() || t.starts_with(char::is_whitespace) {
                return Some((incoming, t.trim()));
            }
        }
    }
    None
}

/// `aspect:<k>` with its optional comparison (`aspect:heat>=2`).
fn parse_aspect_term(spec: &str) -> Option<QueryTerm> {
    let idx = spec.find(['<', '>', '=', '!']);
    let Some(i) = idx else {
        return Some(QueryTerm::Aspect(AspectTerm {
            key: spec.to_string(),
            op: None,
            value: 1,
        }));
    };
    let key = spec[..i].to_string();
    let rest = &spec[i..];
    let (op, value_str) = if let Some(v) = rest.strip_prefix(">=") {
        (CmpOp::Ge, v)
    } else if let Some(v) = rest.strip_prefix("<=") {
        (CmpOp::Le, v)
    } else if let Some(v) = rest.strip_prefix("==") {
        (CmpOp::Eq, v)
    } else if let Some(v) = rest.strip_prefix("!=") {
        (CmpOp::Ne, v)
    } else if let Some(v) = rest.strip_prefix('>') {
        (CmpOp::Gt, v)
    } else {
        let v = rest.strip_prefix('<')?;
        (CmpOp::Lt, v)
    };
    let value: i64 = value_str.parse().ok()?;
    Some(QueryTerm::Aspect(AspectTerm {
        key,
        op: Some(op),
        value,
    }))
}
