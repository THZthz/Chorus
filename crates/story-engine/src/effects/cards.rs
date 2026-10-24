//! Card-economy effect helpers: the `swap` atomization and deck draws.

use super::EffectCtx;
use crate::mint::Fail;
use crate::{Engine, Event};
use story_content::ir::Into as IntoZone;

impl Engine {
    /// `swap`: destroy + spawn atomized; the dying cards are excluded from
    /// the uniqueness check and each successor inherits its card's zone
    /// unless an `into` overrides (spec.md §4.5).
    pub(super) fn apply_swap(
        &mut self,
        target: &story_content::ir::Target,
        count: &story_content::ir::Expr,
        to: &story_core::CardId,
        into: Option<&IntoZone>,
        fctx: &mut EffectCtx,
        recipe: Option<story_core::RecipeId>,
        ev: &mut Vec<Event>,
    ) -> Result<(), Fail> {
        let ctx = fctx.eval_ctx(recipe.as_ref());
        let ids = self.resolve_target(target, &ctx, &fctx.handles);
        let n = self.eval_count(count, &ctx)? as usize;
        let dying: Vec<story_core::EntityId> = ids.into_iter().take(n).collect();
        if dying.is_empty() {
            return Ok(());
        }
        let decl = self.template(to);
        // Uniqueness against the entities live at this instant, the dying
        // cards excluded (swapping oneself = retire and re-mint). Name-aware
        // like every mint: SingleCopy/Unique block on a live entity of that
        // name, Group on a live member, each minus the dying set (spec.md
        // §4.5).
        if self.mint_collision_except(&decl.mint, to, &dying) {
            match decl.mint {
                story_core::MintPolicy::SingleCopy => {
                    return Err(Fail::new(
                        story_content::runtime::DiagKind::NameInUse,
                        format!("a live `{to}` blocks the swap"),
                    ));
                }
                _ => {
                    ev.push(Event::SpawnSkipped {
                        name: to.to_string(),
                    });
                    return Ok(());
                }
            }
        }
        let zones: Vec<story_core::ZoneId> = dying
            .iter()
            .map(|d| match into {
                Some(i) => self.zone_of_into(i, fctx.sit),
                None => Ok(self
                    .state
                    .entities
                    .get(d)
                    .map(|e| e.zone)
                    .unwrap_or(self.home)),
            })
            .collect::<Result<_, _>>()?;
        for d in &dying {
            self.destroy_entity(*d);
        }
        for zone in zones {
            self.mint_body(&decl, to, zone, fctx, ev)?;
        }
        Ok(())
    }

    /// Draw one card from a deck: the compile-time permutation plus the state
    /// cursor; exhaustion walks fallback -> wrap -> the failure (spec.md §5).
    pub(super) fn draw_card(&mut self, deck: &story_core::DeckId) -> Result<story_core::CardId, Fail> {
        let Some(decl) = self.content.decks.get(deck.as_str()) else {
            return Err(Fail::new(
                story_content::runtime::DiagKind::UnknownDeck,
                format!("unknown deck `{deck}`"),
            ));
        };
        let cursor = self.state.decks.get(deck).copied().unwrap_or(0);
        let perm = self.deck_orders.get(deck).cloned().unwrap_or_default();
        let card = if let Some(c) = perm.get(cursor as usize) {
            c.clone()
        } else if let Some(f) = &decl.fallback {
            f.clone()
        } else if decl.wrap && !perm.is_empty() {
            perm[(cursor % perm.len() as u64) as usize].clone()
        } else {
            return Err(Fail::new(
                story_content::runtime::DiagKind::DeckExhausted,
                format!("deck `{deck}` is exhausted and declares no relief path"),
            ));
        };
        self.state.decks.insert(deck.clone(), cursor + 1);
        Ok(card)
    }
}
