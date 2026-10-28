//! Pass 3: recipe emission — the recipe header and slot block, the `when`
//! gate, deck draws, the chance gate, and the chain targets.

use std::collections::BTreeSet;

use serde_json::Value;

use super::source::{
    entries, escape_sw, num, pairs, s, truthy, verb_slot_of, GUARDED_SLOTS, PLAYER_VERBS,
};
use super::{Class, Converter, RecipePlan, SlotKind};

impl<'a> Converter<'a> {
    pub(super) fn emit_recipe(&mut self, out: &mut Vec<String>, verb: &str, rid: &str, local: &str, index: usize, r: &Value) {
        let warmup = num(r, "warmup").unwrap_or(0);

        out.push(format!("    recipe {local}"));
        if let Some(l) = s(r, "label") {
            let l = escape_sw(l);
            if !l.is_empty() {
                out.push(format!("      label \"{l}\""));
            }
        }
        if let Some(desc) = s(r, "description") {
            let d = escape_sw(desc);
            if !d.is_empty() {
                out.push(format!("      meta description \"{d}\""));
            }
        }
        if PLAYER_VERBS.contains(&verb) && !self.craftable(r) {
            out.push("      chain-only".into());
        }
        out.push(format!("      priority {index}"));
        if let Some(mx) = num(r, "maxexecutions") {
            if mx > 0 {
                out.push(format!("      max {mx}"));
            }
        }
        out.push(format!("      duration {warmup}m"));

        let declares = self.declares_of(r);
        if !declares.is_empty() {
            out.push(format!("      declares {}", declares.join(", ")));
        }

        let plan = self.plans.get(rid).cloned().unwrap_or_else(RecipePlan::default);

        let when = self.when_of(r, &plan);
        if let Some(ref gate) = when {
            out.push(format!("      when {gate}"));
        }

        // The doctrine guard: the one recipe whose completion demonstrably
        // requires a bound card in a named slot (the founding alt cascade
        // keys on the doctrine's aspects) refuses the doctrine-less opening.
        for (cs_id, slot, aspect, prose) in GUARDED_SLOTS {
            if *cs_id == rid {
                let planned = plan.slots.iter().any(|p| p.name == *slot);
                if planned {
                    out.push(format!(
                        "      guard \"{prose}\" when count(slot:{slot}) < 1 and total(interior, aspect:{aspect}) < 1"
                    ));
                }
            }
        }

        // Each entry is one slot: its main line first, then its `shown when`
        // sub-line if the slot carries a presence condition.
        let mut slot_lines: Vec<Vec<String>> = Vec::new();
        let source: Vec<Value> = r
            .get("slots")
            .and_then(|x| x.as_array())
            .cloned()
            .unwrap_or_default();
        for (sl, planned) in source.iter().zip(plan.slots.iter()) {
            let mut line = format!("{}: 0..1", planned.name);
            if truthy(sl, "greedy") {
                line.push_str(" greedy");
            }
            if truthy(sl, "consumes") {
                line.push_str(" consuming");
            }
            line.push_str(" open");
            let terms = self.slot_filter_terms(rid, sl);
            if !terms.is_empty() {
                line.push(' ');
                line.push_str(&terms.join(" "));
            }
            slot_lines.push(vec![line]);
        }
        // The verb slot and the synthesized slots follow the source slots, in
        // plan order (verb slot first, then merged, cost, and requirement
        // slots).
        for planned in plan.slots.iter().skip(source.len()) {
            let entry = match planned.kind {
                SlotKind::Source => continue,
                SlotKind::Verb => {
                    let action = self.action_of(rid);
                    let mut line = format!("{}: 0..1 open", planned.name);
                    if let Some(sl) = verb_slot_of(self.d, &action) {
                        let terms = self.verb_slot_filter_terms(&action, sl);
                        if !terms.is_empty() {
                            line.push(' ');
                            line.push_str(&terms.join(" "));
                        }
                    }
                    vec![line]
                }
                SlotKind::Cost => {
                    // `cost_<key>: 1..N consuming internal <term>` — the
                    // payment route is machinery: the host stages the bind at
                    // commit (the player never drags a cost), the paired
                    // negative effect drains the interior at the effects
                    // pass, and the consuming settlement burns the bound
                    // payers.
                    let k = planned.key.clone().unwrap_or_default();
                    let n = pairs(r, "effects")
                        .into_iter()
                        .find(|(ek, _)| *ek == k)
                        .map(|(_, v)| -v)
                        .unwrap_or(1)
                        .clamp(1, 12);
                    let term = self.arm_term(&k, 1);
                    vec![format!(
                        "{}: 1..{n} consuming internal {term}",
                        planned.name
                    )]
                }
                SlotKind::Req => {
                    // A synthesized requirement slot: the key's own term.
                    // Machinery like the cost slots — the host stages the
                    // bind, so the slot is `internal` and `open` is dead.
                    let k = planned.key.clone().unwrap_or_default();
                    let term = self.arm_term(&k, 1);
                    vec![format!("{}: 0..1 internal {term}", planned.name)]
                }
                SlotKind::Merged => {
                    // A merged element slot: its line is fixed at plan time,
                    // with the donor-presence sub-line under it.
                    let mut entry = vec![planned.line.clone().unwrap_or_else(|| {
                        format!("{}: 0..1 open", planned.name)
                    })];
                    if let Some(shown) = &planned.shown {
                        entry.push(shown.clone());
                    }
                    entry
                }
            };
            slot_lines.push(entry);
        }
        if !slot_lines.is_empty() {
            self.stats.slots_out += slot_lines.len();
            out.push("      slots".into());
            for entry in slot_lines {
                for (i, l) in entry.iter().enumerate() {
                    if i == 0 {
                        out.push(format!("        {l}"));
                    } else {
                        out.push(format!("          {l}"));
                    }
                }
            }
        }

        out.push("      effects".into());
        let mut body: Vec<(usize, String)> = Vec::new();
        let ind = 8;
        if let Some(sd) = s(r, "startdescription") {
            let t = escape_sw(sd);
            if !t.is_empty() {
                body.push((ind, format!("narrate \"{t}\"")));
            }
        }

        // Additional alts decide ahead of everything (CS GetValidOverride, the
        // completion's first statement) and their queued spawns fire at the
        // inductions pass regardless of which recipe the override installed —
        // so their gated starts sit ahead of the override cascade, evaluated
        // whether or not an override adopts.
        self.emit_additional_starts(&mut body, ind, r);

        let plain_alts: Vec<Value> = entries(r, "alt")
            .into_iter()
            .filter(|e| !(e.is_object() && truthy(e, "additional")))
            .collect();
        self.emit_alt_cascade(&mut body, &plain_alts, 0, ind, r, rid);

        for (i, line) in &body {
            out.push(format!("{}{}", " ".repeat(*i), line));
        }

        if self.alt_targets_with_warmup.contains(rid) {
            let instant_local = format!("{local}__instant");
            out.push(format!("    recipe {instant_local}"));
            if let Some(l) = s(r, "label") {
                let l = escape_sw(l);
                if !l.is_empty() {
                    out.push(format!("      label \"{l}\""));
                }
            }
            if let Some(desc) = s(r, "description") {
                let d = escape_sw(desc);
                if !d.is_empty() {
                    out.push(format!("      meta description \"{d}\""));
                }
            }
            out.push("      chain-only".into());
            out.push("      allow shadow".into());
            out.push(format!("      priority {index}"));
            if let Some(mx) = num(r, "maxexecutions") {
                if mx > 0 {
                    out.push(format!("      max {mx}"));
                }
            }
            out.push("      duration 0m".into());
            if !declares.is_empty() {
                out.push(format!("      declares {}", declares.join(", ")));
            }
            if let Some(ref gate) = when {
                out.push(format!("      when {gate}"));
            }
            out.push("      effects".into());
            for (i, line) in &body {
                out.push(format!("{}{}", " ".repeat(*i), line));
            }
            self.stats.recipes_out += 1;
        }
    }

    /// The catalyst aspects this recipe declares for aspect-qualified card
    /// books: the CS trigger-gate analysis (recipe aspects, positive
    /// requirement keys, slot requirements, table reqs, mutation keys), plus
    /// the induce catalysts (recipe aspects plus positive-effect outputs and
    /// their aspects — CS induces from outputs, not requirement bearers).
    fn declares_of(&self, r: &Value) -> Vec<String> {
        let mut keys: BTreeSet<String> = BTreeSet::new();
        for (k, _) in pairs(r, "aspects") {
            keys.insert(self.key(&k));
        }
        for (k, v) in pairs(r, "requirements") {
            if v > 0 {
                keys.insert(self.key(&k));
            }
        }
        for sl in r
            .get("slots")
            .and_then(|x| x.as_array())
            .into_iter()
            .flatten()
        {
            for (k, _) in pairs(sl, "required") {
                keys.insert(self.key(&k));
            }
        }
        for (k, _) in pairs(r, "tablereqs") {
            keys.insert(self.key(&k));
        }
        for m in r
            .get("mutations")
            .and_then(|x| x.as_array())
            .into_iter()
            .flatten()
        {
            if let Some(k) = s(m, "mutate") {
                keys.insert(self.key(k));
            }
        }
        let mut catalysts: BTreeSet<String> =
            pairs(r, "aspects").into_iter().map(|(k, _)| k).collect();
        for (k, v) in pairs(r, "effects") {
            if v > 0 && !self.is_aspect(&k) && self.d.element(&k).is_some() {
                catalysts.insert(k.clone());
                for (a, _) in self.aspects_of(&k) {
                    catalysts.insert(a);
                }
            }
        }
        for ind in &self.induce_ids {
            let ik = self.key(ind);
            if catalysts.contains(&ik) {
                keys.insert(ik);
            }
        }
        keys.into_iter().collect()
    }

    /// CS CanExecuteInContext over the three requirement dicts, scoped per
    /// the verified semantics:
    /// - `requirements` read the situation's combined interior
    ///   (`Situation.GetAspects`: every interior sphere plus the verb's
    ///   aspects). The bound cards across ALL slots live in the interior, so
    ///   each positive key emits one sum arm — the interior total plus the
    ///   planned slots whose filter can carry the key (at commit selection
    ///   the interior is empty and only the submitted bindings, visible
    ///   through the slot scopes, can satisfy the arm; at a continuation the
    ///   released storage carries it). Anti-requirements (`-1`) read the
    ///   interior sum alone: the situation pool, where a bound card counts
    ///   once.
    /// - `tablereqs` read the `table` scope (public cards plus every
    ///   situation's non-greedy slot bindings — AspectsOnTable).
    /// - `extantreqs` read the `anywhere` scope (all live cards —
    ///   AspectsExtant, up to running-recipe aspects the language cannot
    ///   see).
    pub(super) fn when_of(&self, r: &Value, plan: &RecipePlan) -> Option<String> {
        let mut parts: Vec<String> = Vec::new();
        for (k, v) in pairs(r, "requirements") {
            let kk = self.key(&k);
            if v > 0 {
                let mut sum = format!("total(interior, aspect:{kk})");
                for c in plan.carriers() {
                    sum.push_str(&format!(" + total(slot:{c}, aspect:{kk})"));
                }
                parts.push(format!("({sum} >= {v})"));
            } else if v < 0 {
                let mut sum = format!("total(interior, aspect:{kk})");
                for c in plan.carriers() {
                    sum.push_str(&format!(" + total(slot:{c}, aspect:{kk})"));
                }
                parts.push(format!("({sum} < {})", -v));
            }
        }
        for dk in ["tablereqs", "extantreqs"] {
            let scope = if dk == "tablereqs" { "table" } else { "anywhere" };
            for (k, v) in pairs(r, dk) {
                let kk = self.key(&k);
                if v > 0 {
                    parts.push(format!("(total({scope}, aspect:{kk}) >= {v})"));
                } else if v < 0 {
                    parts.push(format!("(total({scope}, aspect:{kk}) < {})", -v));
                }
            }
        }
        if parts.is_empty() {
            return None;
        }
        Some(parts.join(" and "))
    }

    /// Deck draws for a recipe: deckeffects, then internaldeck wiring, then
    /// portal consequences. CS draws before effects, and RunDeckEffect deals
    /// into the situation's storage sphere: the drawn cards land in the
    /// interior, count toward the situation's aspects, and ride the normal
    /// interior-leftover release at completion (the table ejection is the
    /// completion's job, not the draw's).
    pub(super) fn draws_of(&mut self, r: &Value, rid: &str) -> Vec<String> {
        let mut out = Vec::new();
        let mut decks: Vec<(String, i64)> = pairs(r, "deckeffects");
        if decks.is_empty() {
            if let Some(idk) = r.get("internaldeck") {
                let draws = num(idk, "draws").unwrap_or(1).max(1);
                decks.push((rid.to_string(), draws));
            }
        }
        for (deck, n) in &decks {
            let Some(dname) = self.deck_name(deck) else {
                self.gaps.hit(
                    "deckeffect on an unresolvable deck",
                    Class::NonGoal,
                    "no deck is emitted under that id; the draw is dropped",
                    &format!("{rid}:{deck}"),
                );
                continue;
            };
            let count = (*n).clamp(1, 12);
            out.push(format!("draw from {dname} count {count} into interior"));
        }
        if let Some(pe) = s(r, "portaleffect").map(String::from) {
            match self.d.portals.get(&pe).cloned() {
                Some(portal) => {
                    for cons in portal
                        .get("consequences")
                        .and_then(|x| x.as_array())
                        .into_iter()
                        .flatten()
                    {
                        for (deck, n) in pairs(cons, "deckeffects") {
                            if let Some(dname) = self.deck_name(&deck) {
                                let count = n.clamp(1, 12);
                                out.push(format!("draw from {dname} count {count} into interior"));
                            }
                        }
                    }
                }
                None => {
                    self.gaps.hit(
                        "portaleffect with no portal table entry",
                        Class::NonGoal,
                        "the portal is unknown; its draws are dropped",
                        &format!("{rid}:{pe}"),
                    );
                }
            }
        }
        out
    }

    fn deck_name(&self, id: &str) -> Option<String> {
        self.names
            .deck
            .get(id)
            .cloned()
            .or_else(|| self.internal_deck.get(id).cloned())
    }

    /// A chain start for a recipe target: (verb name, local recipe name).
    /// Ambient-verb targets qualify: the explicit start opens the situation
    /// at the induction's own timing, and the verb's instance cap arbitrates
    /// against the heartbeat (best-effort skip when full).
    pub(super) fn recipe_target(&mut self, tid: &str) -> Option<(String, String)> {
        let host = match self.host.get(tid) {
            Some(h) => h.clone(),
            None => {
                self.gaps.hit(
                    "chain target is not an emitted recipe",
                    Class::NonGoal,
                    "no recipe with that id exists in the CS data; the chain entry is dropped",
                    tid,
                );
                return None;
            }
        };
        let vname = self.names.verb.get(&host).cloned()?;
        self.names
            .recipe
            .get(&(host, tid.to_string()))
            .cloned()
            .map(|l| (vname, l))
    }

    /// CS chance/challenges gate: the roll line to emit and the success
    /// condition over its binding. With `challenges`, CS
    /// (ChallengeArbiter.GetArbitratedChance) reads each challenge's
    /// aspect-banded chance over the situation's interior (the table replaces
    /// the flat chance), takes the MAX across every challenge, and rolls ONCE
    /// — `Rolld100() > arbitrated` fails. The emission keeps that shape: one
    /// d100 per gated entry, and the condition ORs every `(challenge × band)`
    /// pair of `(aspect-level-band and roll <= bandChance)` over the shared
    /// binding, which collapses at runtime to `roll <= max band chance`. A
    /// flat `chance` without challenges is one d100 against the entry's
    /// percentage. Every gated entry rolls its own d100 (CS rolls per entry
    /// examined), so the binding is always freshly minted here.
    pub(super) fn gate_of(&mut self, e: &Value) -> (Vec<String>, Option<String>) {
        let chance = e.get("chance").and_then(|x| x.as_i64()).unwrap_or(100);
        let challenges = e.get("challenges").and_then(|x| x.as_object()).cloned();
        if chance >= 100 && challenges.is_none() {
            return (Vec::new(), None);
        }
        let mut ch: Vec<(String, String)> = challenges
            .map(|o| {
                o.into_iter()
                    .map(|(k, v)| (k, v.as_str().unwrap_or("stealth").to_string()))
                    .collect()
            })
            .unwrap_or_default();
        ch.sort();
        let mut rolls: Vec<String> = Vec::new();
        let mut conds: Vec<String> = Vec::new();
        if ch.is_empty() {
            let b = self.roll_bind();
            rolls.push(format!("roll 1d100 as {b}"));
            conds.push(format!("({b} <= {chance})"));
        } else {
            let bind = self.roll_bind();
            rolls.push(format!("roll 1d100 as {bind}"));
            let mut pairs: Vec<String> = Vec::new();
            for (ak, kind) in &ch {
                let k = self.key(ak);
                let table: &[(i64, i64)] = if kind == "base" {
                    &[(1, 30), (5, 70), (10, 90)]
                } else {
                    &[(5, 10), (10, 30), (15, 70), (20, 90)]
                };
                for (i, (lo, val)) in table.iter().enumerate() {
                    let mut band = format!("total(interior, aspect:{k}) >= {lo}");
                    if let Some((hi, _)) = table.get(i + 1) {
                        band.push_str(&format!(" and total(interior, aspect:{k}) < {hi}"));
                    }
                    pairs.push(format!("({band} and {bind} <= {val})"));
                }
            }
            conds.push(format!("({})", pairs.join(" or ")));
        }
        (rolls, Some(conds.join(" and ")))
    }

    /// Record a `start` target for the dead census.
    pub(super) fn note_start(&mut self, tid: &str) {
        if let Some(host) = self.host.get(tid) {
            self.started.insert((host.clone(), tid.to_string()));
        }
    }

    /// Record a `continue` target for the dead census.
    pub(super) fn note_continue(&mut self, tid: &str) {
        if let Some(host) = self.host.get(tid) {
            self.continued.insert((host.clone(), tid.to_string()));
        }
    }

    /// A continuation target: (verb name, local recipe name), spelled
    /// globally. Any emitted recipe qualifies — a continuation adopts the
    /// target's verb, so ambient-verb targets are the heartbeat loop itself.
    pub(super) fn continue_target(&mut self, tid: &str) -> Option<(String, String)> {
        let host = match self.host.get(tid) {
            Some(h) => h.clone(),
            None => {
                self.gaps.hit(
                    "chain target is not an emitted recipe",
                    Class::NonGoal,
                    "no recipe with that id exists in the CS data; the chain entry is dropped",
                    tid,
                );
                return None;
            }
        };
        let vname = self.names.verb.get(&host).cloned()?;
        self.names
            .recipe
            .get(&(host, tid.to_string()))
            .cloned()
            .map(|l| (vname, l))
    }
}
