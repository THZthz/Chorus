//! Deck permutation machinery (spec §9.42) — the pure (seed, deck id)
//! shuffle, the id hash, and the shared draw rule. W0 hosts these in
//! `core/deck_rng.rs`; W2 folds them into `content/` once CompiledDeck is
//! compiled there (plan §2.2 / §4-W0).
//!
//! The permutation is a Fisher–Yates shuffle over POSITIONS 0..n-1 where
//! position i draws from `splitmix64(seed XOR hash(deckId) XOR i)` —
//! dice.rs's mixer with the id hash standing in for the dice counter. Deck
//! streams and the dice stream never interact (different key spaces, NO rng
//! counter consumed; law 5). Pure in (seed, deck id): identical at play and
//! fold, fixed at load, never stored on State.

use crate::core::dice::splitmix64;
use crate::js::{ChorusError, Result};

/// ts: src/content/content.ts:104-108 — one resolvable deck entry: the card
/// NAME and the kind its mint carries.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeckCard {
    pub name: String,
    pub kind: String,
}

/// ts: src/content/content.ts:157-163 (`CompiledDeck`). W0 carries only the
/// shape `draw_from` reads; the full compiled struct (built by compileDecks,
/// content.ts:397-424) lands with W2's content.rs.
#[derive(Debug, Clone)]
pub struct DeckSpec {
    pub id: String,
    /// the draw order: spec positions shuffled, entries resolved to (name, kind)
    pub order: Vec<DeckCard>,
    /// served forever once the order is exhausted and `reset` is absent
    pub defaultcard: Option<DeckCard>,
    pub reset: bool,
}

/// ts: src/content/content.ts:430-437 — FNV-1a 64 over the deck id's UTF-8
/// bytes (`.as_bytes()` is the same byte sequence TextEncoder produces),
/// folding the id into one full-width 64-bit key part.
#[must_use]
pub fn hash_deck_id(id: &str) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in id.as_bytes() {
        h ^= u64::from(*b);
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    h
}

/// ts: src/content/content.ts:453-461 — Fisher–Yates over positions 0..n-1;
/// position i (descending from n-1 to 1) draws
/// `j = splitmix64(asUintN(seed) ^ hashDeckId(deckId) ^ asUintN(i)) % (i+1)`
/// and swaps. `seed as u64` covers the asUintN two's-complement treatment of
/// negative seeds (the (7, -3, 'NEG') vector pins it); `i as u64` is a no-op
/// asUintN for the non-negative loop index. Uniform up to the negligible
/// `u64 mod (i+1)` bias; pure in (seed, deck id).
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

/// ts: src/content/content.ts:472-478 — serve the card at `cursor`, shared by
/// play (the DRAW op) and fold (the DRAWN row's assertion) so the two can
/// never drift: below the period it is `order[cursor]`; past it, `reset`
/// wraps (periodic with period len(order)) and otherwise `defaultcard` serves
/// forever; with neither, the draw FAILS — a §9.19 step rejection at play
/// (nothing logged, cursor unmoved) and a loud fold error there. Requires a
/// non-empty order (compileDecks rejects an empty spec, W2).
///
/// A negative cursor is unreachable (cursors start at 0 and only advance on
/// the engine's DRAW path); TS would hand back `undefined` from the first
/// branch in that case.
pub fn draw_from(deck: &DeckSpec, cursor: i64) -> Result<DeckCard> {
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
        "deck '{}' is exhausted — no 'reset' and no 'defaultcard' (§9.42)",
        deck.id
    ))) // ts: content.ts:476
}
