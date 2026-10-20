//! The §4.4 deck machinery: the seed-fixed position permutation, the
//! deck-table compiler, and the shared draw rule. The deck TYPES live in
//! `model.rs` beside the rest of the content model.

use indexmap::{IndexMap, IndexSet};

use super::compile::upper_snake_re;
use super::model::{
    CompiledContent, CompiledDeck, DeckCard, DeckRaw, DeclaredEntity, PrototypeDef,
};
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
/// DRAW op) and fold (the DRAWN row's assertion) so the two can never drift.
///
/// The §4.2 unique deck filter is LAZY, applied here at serve time (CS's
/// `Dealer.Shuffle` filters at shuffle time — B9.4/B10): `manifested` is the
/// state's manifested-unique set, `manifested_groups` the manifested-group
/// set (v0.10.1), and an order entry is a DEAD position when its name is a
/// unique that is already manifested, a member of a group that is already
/// manifested (one member's manifestation kills every member's entries), or
/// a later occurrence of any unique OR member name — the earliest copy's
/// serve (or skip) always manifests first, so a later occurrence is dead in
/// every execution. The scan consumes dead positions within the serving
/// draw.
///
/// The cursor discipline: `cursor` counts CONSUMED POSITIONS. A served
/// position lands the cursor past it — dead positions in between consumed
/// within the same draw — and a `defaultcard` serve lands `cursor + 1`; the
/// returned value is what the caller puts on State, so the fold's
/// re-derivation of this same rule lands the identical number. With an
/// empty manifested set every draw consumes exactly one position (the
/// raw one-step discipline, `reset` wrap included).
///
/// True exhaustion — no live entry in the tail (`reset`-less decks) or in a
/// full period (`reset`) — falls to the deck's own config: `defaultcard`
/// serves forever (that face itself is never filtered — it is not a spec
/// entry), and with neither the draw FAILS — a §9.2 step rejection at play
/// (nothing logged, cursor unmoved) and a loud fold error there.
///
/// A negative cursor is unreachable (cursors start at 0 and only advance on
/// the engine's DRAW path). The debug_assert documents that
/// unreachability; the release-side `Err` is a guard so the unreachable
/// branch can never become an index panic. Requires a non-empty order
/// (compile_decks rejects an empty spec).
pub fn draw_from(
    content: &CompiledContent,
    deck: &CompiledDeck,
    cursor: i64,
    manifested: &IndexSet<String>,
    manifested_groups: &IndexSet<String>,
) -> Result<(DeckCard, i64)> {
    if cursor < 0 {
        debug_assert!(false, "negative deck cursor {cursor} is unreachable");
        return Err(ChorusError::new(format!("internal: negative deck cursor {cursor}")));
    }
    let n = deck.order.len() as i64;
    // dead = a manifested unique, a member of a manifested group, or a later
    // occurrence of any unique or member name (§4.2)
    let dead = |idx: i64| -> bool {
        let c = &deck.order[idx as usize];
        let unique = content.mint_is_unique(&c.name);
        let member = content.mint_is_group_member(&c.name);
        let name_dead = unique && manifested.contains(&c.name)
            || member && content.mint_group(&c.name).is_some_and(|g| manifested_groups.contains(g));
        let later = (unique || member) && (0..idx).any(|q| deck.order[q as usize].name == c.name);
        name_dead || later
    };
    // the first LIVE position in `from..n`
    let scan = |from: i64| -> Option<i64> { (from..n).find(|p| !dead(*p)) };
    // (scan origin, served position): in the period below the end, else the
    // reset wrap — the reshuffle skips dead positions too (manifested
    // uniques and manifested-group members alike, CS B9.4)
    let serve: Option<(i64, i64)> = if cursor < n {
        scan(cursor).map(|p| (cursor, p)).or_else(|| {
            if deck.reset {
                scan(0).map(|p| (cursor, p))
            } else {
                None
            }
        })
    } else if deck.reset {
        let p0 = cursor % n;
        scan(p0).or_else(|| scan(0)).map(|p| (p0, p))
    } else {
        None
    };
    if let Some((origin, p)) = serve {
        let steps = (p - origin + n) % n;
        let next = cursor
            .checked_add(steps + 1)
            .ok_or_else(|| ChorusError::new(format!("deck '{}' cursor overflow", deck.id)))?;
        return Ok((deck.order[p as usize].clone(), next));
    }
    // true exhaustion — the deck's own config:
    if let Some(dc) = &deck.defaultcard {
        let next = cursor
            .checked_add(1)
            .ok_or_else(|| ChorusError::new(format!("deck '{}' cursor overflow", deck.id)))?;
        return Ok((dc.clone(), next));
    }
    Err(ChorusError::new(format!(
        "deck '{}' is exhausted — no 'reset' and no 'defaultcard' (§4.4)",
        deck.id
    )))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::content::{compile_content, ContentRaw};

    /// A compiled one-deck content carrying `spec` under deck id `D`,
    /// every prototype `idea`-kind; `uniques` names the unique ones,
    /// `groups` maps a group id to its member names.
    fn rig(spec: &[&str], uniques: &[&str], groups: &[(&str, &[&str])]) -> CompiledContent {
        let dict = crate::core::dict::core_dictionary();
        let raw = ContentRaw {
            decks: Some(vec![DeckRaw {
                id: "D".into(),
                spec: spec.iter().map(|s| (*s).to_string()).collect(),
                ..Default::default()
            }]),
            prototypes: Some(
                spec.iter()
                    .map(|s| {
                        (
                            s.to_string(),
                            PrototypeDef {
                                kind: "idea".into(),
                                unique: uniques.contains(s),
                                unique_group: groups
                                    .iter()
                                    .find(|(_, ms)| ms.contains(s))
                                    .map(|(g, _)| (*g).to_string()),
                                ..Default::default()
                            },
                        )
                    })
                    .collect(),
            ),
            ..ContentRaw::default()
        };
        compile_content(&raw, &dict, &[]).unwrap()
    }

    #[test]
    fn deck_machinery_matches_the_w0_vectors_and_the_draw_rules() {
        // the frozen deck vectors stay pinned
        assert_eq!(permute_positions(4, 0, "RUMOURS"), vec![0, 2, 1, 3]);
        assert_eq!(permute_positions(4, 42, "RUMOURS"), vec![3, 0, 1, 2]);
        assert_eq!(permute_positions(7, 0, "SEVEN"), vec![0, 6, 3, 1, 4, 5, 2]);
        assert_eq!(permute_positions(7, -3, "NEG"), vec![4, 0, 2, 6, 3, 1, 5]);
        let content = rig(&["HEARSAY"], &[], &[]);
        let deck = content.decks.get("D").unwrap();
        let none = IndexSet::new();
        let err = draw_from(&content, deck, 1, &none, &none).unwrap_err();
        assert_eq!(err.msg, "deck 'D' is exhausted — no 'reset' and no 'defaultcard' (§4.4)");
    }

    #[test]
    fn the_unique_filter_skips_manifested_positions_and_lands_the_consumed_cursor() {
        // spec [A(unique), B] — a manifested A is a dead position: the serve
        // walks to the first live one and the cursor lands past it
        let content = rig(&["A", "B"], &["A"], &[]);
        let deck = content.decks.get("D").unwrap();
        let manifested: IndexSet<String> = ["A".to_string()].into_iter().collect();
        let none = IndexSet::new();
        let b_position = deck
            .order
            .iter()
            .position(|c| c.name == "B")
            .map(|p| p as i64)
            .unwrap_or_else(|| unreachable!("the rig writes one B"));
        let (card, next) = draw_from(&content, deck, 0, &manifested, &none).unwrap();
        assert_eq!(card.name, "B");
        assert_eq!(next, b_position + 1); // the dead A consumed within the draw
                                          // true exhaustion with no reset and no defaultcard
        let err = draw_from(&content, deck, next, &manifested, &none).unwrap_err();
        assert!(err.msg.contains("is exhausted"), "{err}");
    }

    #[test]
    fn the_group_filter_kills_every_members_position_once_the_group_is_manifested() {
        // spec [G1A, B, G1B] — unmanifested, the full period serves (3
        // draws); once the GROUP is manifested, BOTH member positions are
        // dead (the first serve included): B serves and the deck is truly
        // exhausted after it
        let content = rig(&["G1A", "B", "G1B"], &[], &[("G1", &["G1A", "G1B"])]);
        let deck = content.decks.get("D").unwrap();
        let none = IndexSet::new();
        let groups: IndexSet<String> = ["G1".to_string()].into_iter().collect();
        let drain = |manifested_groups: &IndexSet<String>| -> Vec<String> {
            let mut served: Vec<String> = Vec::new();
            let mut cursor = 0i64;
            while let Ok((card, next)) = draw_from(&content, deck, cursor, &none, manifested_groups)
            {
                served.push(card.name);
                cursor = next;
            }
            served
        };
        assert_eq!(drain(&none).len(), 3); // the unfiltered period
        assert_eq!(drain(&groups), ["B"]); // every member position dead
    }
}
