//! The §4.4 deck machinery: the seed-fixed position permutation, the
//! deck-table compiler, and the shared draw rule. The deck TYPES live in
//! `model.rs` beside the rest of the content model.

use indexmap::IndexMap;

use super::compile::upper_snake_re;
use super::model::{CompiledDeck, DeckCard, DeckRaw, DeclaredEntity, PrototypeDef};
use crate::core::dice::splitmix64;
use crate::core::dict::Dictionary;
use crate::error::{ChorusError, Result};

/// Compile the deck table. Each spec entry must name
/// a declared prototype or entity (load error naming the deck and the entry
/// otherwise); repeats are legal and weight the draw. The fixed draw order is
/// derived per deck from (seed, deck id) ALONE — a permutation over POSITIONS
/// of the spec, so the multiset is preserved whatever order the shuffle picks.
pub(crate) fn compile_decks(
    raws: &[DeckRaw],
    seed: i64,
    dict: &Dictionary,
    prototypes: &IndexMap<String, PrototypeDef>,
    entities: &[DeclaredEntity],
) -> Result<IndexMap<String, CompiledDeck>> {
    // `entity_kinds`: later duplicates overwrite
    // the value (keeping the original insertion slot)
    let mut entity_kinds: IndexMap<&str, &str> = IndexMap::new();
    for e in entities {
        entity_kinds.insert(e.name.as_str(), e.kind.as_str());
    }
    // entries resolve prototype-first, then entity
    // lines — the same name in both is unambiguous (the prototype is what
    // SPAWN would mint)
    let resolve = |deck: &str, what: &str, name: &str| -> Result<DeckCard> {
        if let Some(proto) = prototypes.get(name) {
            return Ok(DeckCard { name: name.to_string(), kind: proto.kind.clone() });
        }
        if let Some(kind) = entity_kinds.get(name) {
            return Ok(DeckCard { name: name.to_string(), kind: (*kind).to_string() });
        }
        Err(ChorusError::new(format!(
            "deck {deck}: {what} '{name}' is neither a declared prototype nor an entity"
        )))
    };
    let mut decks = IndexMap::new();
    for d in raws {
        let re = upper_snake_re();
        if !re.is_match(&d.id) {
            return Err(ChorusError::new(format!("deck '{}' must be UPPER_SNAKE", d.id)));
        }
        if dict.reserved.contains(&d.id) {
            return Err(ChorusError::new(format!(
                "'{}' is reserved and cannot be a deck id",
                d.id
            )));
        }
        if decks.contains_key(&d.id) {
            return Err(ChorusError::new(format!("duplicate deck '{}'", d.id)));
        }
        if d.spec.is_empty() {
            return Err(ChorusError::new(format!("deck {}: the spec is empty", d.id)));
        }
        let mut entries = Vec::with_capacity(d.spec.len());
        for name in &d.spec {
            entries.push(resolve(&d.id, "spec entry", name)?);
        }
        let defaultcard = match &d.defaultcard {
            Some(dc) => Some(resolve(&d.id, "defaultcard", dc)?),
            None => None,
        };
        let order = permute_positions(entries.len(), seed, &d.id)
            .into_iter()
            .map(|p| entries[p].clone())
            .collect();
        decks.insert(
            d.id.clone(),
            CompiledDeck { id: d.id.clone(), order, defaultcard, reset: d.reset },
        );
    }
    Ok(decks)
}

/// FNV-1a 64 over the deck id's UTF-8 bytes,
/// folding the id into one full-width 64-bit key part.
#[must_use]
fn hash_deck_id(id: &str) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in id.as_bytes() {
        h ^= u64::from(*b);
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    h
}

/// Fisher–Yates over positions 0..n-1; position i
/// (descending from n-1 to 1) draws
/// `j = splitmix64(asUintN(seed) ^ hash_deck_id(deck_id) ^ asUintN(i)) % (i+1)`
/// and swaps. `seed as u64` covers the asUintN two's-complement treatment of
/// negative seeds (the (7, -3, 'NEG') vector pins it); `i as u64` is a no-op
/// asUintN for the non-negative loop index. Uniform up to the negligible
/// `u64 mod (i+1)` bias; pure in (seed, deck id): identical at play and
/// fold, fixed at load, never stored on State.
#[must_use]
pub fn permute_positions(n: usize, seed: i64, deck_id: &str) -> Vec<usize> {
    let mut pos: Vec<usize> = (0..n).collect();
    let hash = hash_deck_id(deck_id);
    let mut i: i64 = n as i64 - 1;
    while i >= 1 {
        let key = (seed as u64) ^ hash ^ (i as u64);
        let j = (splitmix64(key) % (i as u64 + 1)) as usize;
        pos.swap(i as usize, j);
        i -= 1;
    }
    pos
}

/// Serve the card at `cursor`, shared by play (the
/// DRAW op) and fold (the DRAWN row's assertion) so the two can never drift:
/// below the period it is `order[cursor]`; past it, `reset` wraps (periodic
/// with period len(order)) and otherwise `defaultcard` serves forever; with
/// neither, the draw FAILS — a §9.2 step rejection at play (nothing logged,
/// cursor unmoved) and a loud fold error there. Requires a non-empty order
/// (compile_decks rejects an empty spec).
///
/// A negative cursor is unreachable (cursors start at 0 and only advance on
/// the engine's DRAW path). The debug_assert documents that
/// unreachability; the release-side `Err` is a guard so the unreachable
/// branch can never become an index panic.
pub fn draw_from(deck: &CompiledDeck, cursor: i64) -> Result<DeckCard> {
    if cursor < 0 {
        debug_assert!(false, "negative deck cursor {cursor} is unreachable");
        return Err(ChorusError::new(format!("internal: negative deck cursor {cursor}")));
    }
    if cursor < deck.order.len() as i64 {
        return Ok(deck.order[cursor as usize].clone());
    }
    if deck.reset {
        return Ok(deck.order[(cursor % deck.order.len() as i64) as usize].clone());
    }
    if let Some(dc) = &deck.defaultcard {
        return Ok(dc.clone());
    }
    Err(ChorusError::new(format!(
        "deck '{}' is exhausted — no 'reset' and no 'defaultcard' (§4.4)",
        deck.id
    )))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deck_machinery_matches_the_w0_vectors_and_the_draw_rules() {
        // the frozen deck vectors stay pinned
        assert_eq!(permute_positions(4, 0, "RUMOURS"), vec![0, 2, 1, 3]);
        assert_eq!(permute_positions(4, 42, "RUMOURS"), vec![3, 0, 1, 2]);
        assert_eq!(permute_positions(7, 0, "SEVEN"), vec![0, 6, 3, 1, 4, 5, 2]);
        assert_eq!(permute_positions(7, -3, "NEG"), vec![4, 0, 2, 6, 3, 1, 5]);
        let deck = CompiledDeck {
            id: "SINGLES".into(),
            order: vec![DeckCard { name: "HEARSAY".into(), kind: "idea".into() }],
            defaultcard: None,
            reset: false,
        };
        let err = draw_from(&deck, 1).unwrap_err();
        assert_eq!(err.msg, "deck 'SINGLES' is exhausted — no 'reset' and no 'defaultcard' (§4.4)");
    }
}
