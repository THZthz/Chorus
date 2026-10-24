//! The completion cascades: the alt override ladder, the core body, the
//! linked walk, and the side-situation starts.

use serde_json::Value;

use super::source::{entries, entry_id, num, pairs, s, truthy};
use super::{Class, Converter};

impl<'a> Converter<'a> {
    /// The non-additional alt cascade: the first executable entry REPLACES the
    /// recipe being executed, in place, in the same situation with the same
    /// spheres (CS RequiresExecutionState.Continue: `situation.CurrentRecipe =
    /// validOverride; situation.ExecuteCurrentRecipe();` — the override's body
    /// runs INSTEAD of the original's, whose effects never fire). Each arm is
    /// therefore a gated `continue via` — the same-situation adoption — and
    /// the deepest `else` carries the recipe's own body, linked cascade
    /// included, which runs only when no override adopts. The decision sits
    /// ahead of the body because CS resolves the override as the completion's
    /// first statement, before ExecuteCurrentRecipe. A target with a warmup
    /// re-arms the timer for it (a 0-warmup target — the ladder case —
    /// resolves inline at once); CS's override executes without a warmup, so
    /// the re-arm is the mapped shape's one pacing deviation.
    pub(super) fn emit_alt_cascade(
        &mut self,
        body: &mut Vec<(usize, String)>,
        alts: &[Value],
        i: usize,
        ind: usize,
        r: &Value,
        rid: &str,
    ) {
        if i >= alts.len() {
            self.emit_core(body, ind, r, rid);
            return;
        }
        let e = alts[i].clone();
        let Some(tid) = entry_id(&e) else {
            self.gaps.hit(
                "alt entry without an id",
                Class::NonGoal,
                "the entry names no recipe; it is dropped",
                rid,
            );
            self.emit_alt_cascade(body, alts, i + 1, ind, r, rid);
            return;
        };
        let Some(target) = self.continue_target(&tid) else {
            self.emit_alt_cascade(body, alts, i + 1, ind, r, rid);
            return;
        };
        let plan = self.plans.get(&tid).cloned().unwrap_or_default();
        let target_recipe = self.d.recipe(&tid);
        let gate = target_recipe
            .and_then(|t| self.when_of(t, &plan));
        let (rolls, cond) = self.gate_of(&e);
        let gates: Vec<String> = gate.into_iter().chain(cond).collect();
        for rl in rolls {
            body.push((ind, rl));
        }
        let target_local = if self.alt_targets_with_warmup.contains(&tid) {
            format!("{}__instant", target.1)
        } else {
            target.1.clone()
        };
        if gates.is_empty() {
            // An unconditional alternative adopts the situation outright; the
            // core body never runs — exactly CS's own semantics (the override
            // replaces the recipe being executed), so nothing is lost.
            body.push((ind, format!("continue via {}.{target_local}", target.0)));
            self.note_continue(&tid);
            return;
        }
        if gates.len() > 1 {
            body.push((ind, format!("if ({})", gates.join(") and ("))));
        } else {
            body.push((ind, format!("if {}", gates[0])));
        }
        body.push((
            ind + 2,
            format!("continue via {}.{target_local}", target.0),
        ));
        self.note_continue(&tid);
        body.push((ind, "else".into()));
        let mark = body.len();
        self.emit_alt_cascade(body, alts, i + 1, ind + 2, r, rid);
        if body.len() == mark {
            // The cascade is exhausted past this arm: the else has no
            // branch to fill, so the arm ends the walk.
            body.pop();
        }
    }

    /// The recipe's own resolution body, in CS completion order: mutations,
    /// deck draws, and the effects table (RecipeCompletionEffectCommand.
    /// Execute), then the ending, then the linked cascade and the inductions.
    /// It runs only when no non-additional alt adopted the situation (the
    /// alt cascade guards it as its deepest else — CS executes the override's
    /// body instead of this one).
    fn emit_core(&mut self, body: &mut Vec<(usize, String)>, ind: usize, r: &Value, rid: &str) {
        // Mutations.
        for m in r
            .get("mutations")
            .and_then(|x| x.as_array())
            .into_iter()
            .flatten()
        {
            let (Some(filt), Some(mutate)) = (s(m, "filter"), s(m, "mutate")) else {
                self.gaps.hit(
                    "mutation without a filter or mutate key",
                    Class::NonGoal,
                    "nothing identifies the mutated cards; the mutation is dropped",
                    rid,
                );
                continue;
            };
            let level = num(m, "level").unwrap_or(1);
            let additive = truthy(m, "additive");
            let fk = self.key(filt);
            let mk = self.key(mutate);
            let op = if additive {
                if level >= 0 {
                    format!("+= {level}")
                } else {
                    format!("-= {}", -level)
                }
            } else {
                format!("= {level}")
            };
            body.push((ind, format!("set interior aspect:{fk} aspect:{mk} {op}")));
        }

        // Deck draws: CS RunDeckEffect deals into the situation's storage
        // sphere after the mutation and xtrigger passes and before the
        // recipe's own effects — so draws come here, after mutations and
        // before the effects table (and after the alt gates above, which CS
        // decides in GetValidOverride before ExecuteCurrentRecipe runs).
        for line in self.draws_of(r, rid) {
            body.push((ind, line));
        }

        // The effects table.
        for (k, v) in pairs(r, "effects") {
            if k.starts_with("lever_") {
                self.emit_lever(body, ind, rid, &k, v);
                continue;
            }
            if v == 0 {
                continue;
            }
            if v > 0 {
                if self.is_aspect(&k) {
                    self.gaps.hit(
                        "positive effect on an aspect",
                        Class::NonGoal,
                        "CS NullElement semantics: the write is inert",
                        &format!("{rid}:{k}"),
                    );
                    continue;
                }
                // CS RunRecipeEffects writes the situation's storage sphere:
                // the spawn lands in the interior, feeds the completion's
                // books and any continuation's `when` out of the same pool,
                // and rides the interior-leftover release at completion.
                // Tree-absent element ids were minted stub cards before
                // conversion, so every non-aspect id resolves here.
                if let Some(pname) = self.target_card(&k) {
                    let n = v.clamp(1, 20);
                    body.push((ind, format!("spawn {pname} count {n} into interior")));
                }
            } else {
                // A negative effect value drains the executing situation's
                // interior only (CS RunRecipeEffects -> Sphere.ModifyElementQuantity
                // -> ReduceElement): it consumes up to |n| units across the
                // interior's stacks in arrival order, retiring stacks that
                // reach zero, and stops silently at the last matching unit
                // (the elementStack == null early return) — so -1 and -999
                // differ observably whenever the interior holds more than
                // one unit. The .sw destroy count is the same AtMost cap
                // (story-engine effects.rs takes min(count, resolved)).
                let kk = self.key(&k);
                body.push((ind, format!("destroy interior aspect:{kk} count {}", -v)));
            }
        }

        // Ending.
        if let Some(end) = s(r, "ending").map(String::from) {
            match self.ending_name(&end, r) {
                Some(en) => body.push((ind, format!("ending {en}"))),
                None => {
                    self.gaps.hit(
                        "recipe ending with no ending table entry",
                        Class::NonGoal,
                        "the ending is unknown; the effect is dropped",
                        &format!("{rid}:{end}"),
                    );
                }
            }
        }

        // linked: CS scans the entries in order after the effects; the first
        // roll-passing, requirement-satisfying, non-additional entry takes
        // over the same situation, and `additional: true` entries spawn side
        // situations instead.
        let linked = entries(r, "linked");
        self.emit_linked_cascade(body, &linked, ind, rid);

        // Inductions: CS gates them after the effects (the inductions pass
        // runs post-ExecuteCurrentRecipe), so they stay behind the linked
        // cascade.
        self.emit_induction_starts(body, ind, r);
    }

    /// The linked list: the `additional: true` entries spawn gated side
    /// situations (CS GetNextValidLink's spawnAdditionals), then the
    /// non-additional entries walk an exclusive if/else cascade of
    /// `continue via` arms — the same-situation continuation. Each arm gates
    /// the target's `when` plus its chance/challenges, rolling its own d100
    /// (CS rolls per entry examined); challenged entries pass at the max
    /// banded chance (CS's arbiter rolls once against the max). An
    /// unconditional entry ends the walk. Loops are the engine's to bound:
    /// each step re-arms the timer, and the gated arms make every CS cycle
    /// strictly progress (the needs/discardseason/drawseason heartbeat).
    fn emit_linked_cascade(
        &mut self,
        body: &mut Vec<(usize, String)>,
        linked: &[Value],
        ind: usize,
        rid: &str,
    ) {
        // Side situations first: they never take over the walk.
        for e in linked {
            if e.is_object() && truthy(e, "additional") {
                self.emit_side_start(body, ind, e);
            }
        }
        let walk: Vec<&Value> = linked
            .iter()
            .filter(|e| !(e.is_object() && truthy(e, "additional")))
            .collect();
        self.emit_linked_walk(body, &walk, 0, ind, rid);
    }

    /// The exclusive walk over the non-additional linked entries.
    fn emit_linked_walk(
        &mut self,
        body: &mut Vec<(usize, String)>,
        walk: &[&Value],
        i: usize,
        ind: usize,
        rid: &str,
    ) {
        if i >= walk.len() {
            return;
        }
        let e = walk[i].clone();
        let Some(tid) = entry_id(&e) else {
            self.gaps.hit(
                "linked entry without an id",
                Class::NonGoal,
                "the entry names no recipe; it is dropped",
                rid,
            );
            self.emit_linked_walk(body, walk, i + 1, ind, rid);
            return;
        };
        let Some(target) = self.continue_target(&tid) else {
            self.emit_linked_walk(body, walk, i + 1, ind, rid);
            return;
        };
        let plan = self.plans.get(&tid).cloned().unwrap_or_default();
        let gate = self
            .d
            .recipe(&tid)
            .and_then(|t| self.when_of(t, &plan));
        let (rolls, cond) = self.gate_of(&e);
        let gates: Vec<String> = gate.into_iter().chain(cond).collect();
        for rl in rolls {
            body.push((ind, rl));
        }
        if gates.is_empty() {
            // The first executable entry always runs; later entries never do.
            body.push((ind, format!("continue via {}.{}", target.0, target.1)));
            self.note_continue(&tid);
            return;
        }
        if gates.len() > 1 {
            body.push((ind, format!("if ({})", gates.join(") and ("))));
        } else {
            body.push((ind, format!("if {}", gates[0])));
        }
        body.push((ind + 2, format!("continue via {}.{}", target.0, target.1)));
        self.note_continue(&tid);
        body.push((ind, "else".into()));
        let mark = body.len();
        self.emit_linked_walk(body, walk, i + 1, ind + 2, rid);
        if body.len() == mark {
            // The walk is exhausted past this arm: the else has no branch
            // to fill, so the arm ends the walk.
            body.pop();
        }
    }

    /// The additional-alt fan-out: every matching entry starts its own
    /// situation (gates decided pre-effects, CS GetValidOverride order).
    pub(super) fn emit_additional_starts(&mut self, body: &mut Vec<(usize, String)>, ind: usize, r: &Value) {
        let additional: Vec<Value> = entries(r, "alt")
            .into_iter()
            .filter(|e| e.is_object() && truthy(e, "additional"))
            .collect();
        for e in additional {
            self.emit_side_start(body, ind, &e);
        }
    }

    /// The induction fan-out: every matching entry starts its own situation
    /// (gates decided post-effects, CS inductions-pass order).
    fn emit_induction_starts(&mut self, body: &mut Vec<(usize, String)>, ind: usize, r: &Value) {
        let mut inductions = entries(r, "inductions");
        inductions.retain(|e| e.is_object() || e.is_string());
        for e in inductions {
            self.emit_side_start(body, ind, &e);
        }
    }

    /// One side-situation start: chance gate, then the start line.
    fn emit_side_start(&mut self, body: &mut Vec<(usize, String)>, ind: usize, e: &Value) {
        let Some(tid) = entry_id(e) else {
            return;
        };
        let Some(target) = self.recipe_target(&tid) else {
            return;
        };
        let (rolls, cond) = self.gate_of(e);
        for rl in rolls {
            body.push((ind, rl));
        }
        let plan = self.plans.get(&tid).cloned().unwrap_or_default();
        let target_gate = self.d.recipe(&tid).and_then(|t| self.when_of(t, &plan));
        let conds: Vec<String> = cond.into_iter().chain(target_gate).collect();
        if conds.is_empty() {
            body.push((ind, format!("start {} via {}", target.0, target.1)));
        } else {
            let expr = if conds.len() > 1 {
                format!("({})", conds.join(") and ("))
            } else {
                conds[0].clone()
            };
            body.push((ind, format!("if {expr}")));
            body.push((ind + 2, format!("start {} via {}", target.0, target.1)));
        }
        self.note_start(&tid);
    }

    fn emit_lever(
        &mut self,
        body: &mut Vec<(usize, String)>,
        ind: usize,
        rid: &str,
        key: &str,
        v: i64,
    ) {
        let lever_id = key.trim_start_matches("lever_").to_lowercase();
        let default = self
            .d
            .levers
            .get(&lever_id)
            .and_then(|l| s(l, "defaultValue"))
            .and_then(|dv| self.target_card(dv));
        // CS writes the lever's value against cross-run history; the map
        // spawns the lever's defaultValue card into the table, count = the
        // site's own effect value.
        match default {
            Some(cname) if v > 0 => {
                let n = v.clamp(1, 20);
                body.push((ind, format!("spawn {cname} count {n} into table")));
            }
            _ => {
                self.gaps.hit(
                    "lever effect with no resolvable default",
                    Class::ExtensionSeam,
                    "the lever's defaultValue names no card (or the effect value is not positive); nothing to spawn and the effect is dropped",
                    &format!("{rid}:{key}"),
                );
            }
        }
    }

    fn ending_name(&self, end: &str, r: &Value) -> Option<String> {
        let base = self.d.endings.get(end)?;
        let base_flavour = s(base, "flavour").unwrap_or("Melancholy").to_string();
        if let Some(f) = s(r, "signalEndingFlavour") {
            if f != base_flavour {
                return self
                    .names
                    .ending_variant
                    .get(&(end.to_string(), f.to_string()))
                    .cloned();
            }
        }
        self.names.ending.get(end).cloned()
    }
}
