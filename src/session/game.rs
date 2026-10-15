//! Play interface — a text script becomes a playable game.
//! Game = script + log; handle_line is the whole turn loop (testable without a
//! TTY); the REPL (bin/chorus.rs) renders it.
//!
//! The full player surface: handle_line, undo
//! (+ `turn_lengths` bookkeeping), wait's ceil-to-halfhour quantization,
//! visible/suggestions/story/journal/look/cards/inspect, and BOTH game-over
//! texts (this file speaks the UI notice; the engine's rejected short
//! text stays in engine.rs).

use indexmap::IndexMap;
use std::collections::HashSet;

use crate::content::content::{CompiledContent, CompiledVerb};
use crate::content::script::{compile_script, ScriptMeta};
use crate::core::clock::{clock_from_abs_min, Clock, MIN_PER_HALFHOUR};
use crate::core::dict::Dictionary;
use crate::core::predicate::{eval_pred, PredCtx, RoleBindings};
use crate::core::query::match_query;
use crate::core::r#ref::Ref;
use crate::core::roles::Role;
use crate::core::sentence::Arg;
use crate::core::types::{clone_state, AttrValue, Entity, OpenWindow, State};
use crate::engine::engine::{execute_step, ExecOptions};
use crate::engine::fold::fold;
use crate::engine::store::Log;
use crate::js::{js_trim, Result};

use super::lenient::{parse_player_input, LenientCtx, ParsedInput};
use super::render::{ending_notice, flatten_aspects, journal, pretty, prose_lines};

/// The kind of turn: an action, an info notice, or an input error.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TurnKind {
    Action,
    Info,
    Error,
}

/// One turn's rendered output.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Turn {
    pub kind: TurnKind,
    pub lines: Vec<String>,
}

fn turn(kind: TurnKind, lines: Vec<String>) -> Turn {
    Turn { kind, lines }
}

/// The game. `base` and
/// `turn_lengths` are private; log/state/meta/
/// content/dict are the public surface the tests read.
pub struct Game {
    pub dict: Dictionary,
    pub content: CompiledContent,
    pub meta: ScriptMeta,
    pub log: Log,
    base: State,
    pub state: State,
    /// the log length at each turn's start — undo truncates to the previous
    /// entry. Turn boundaries are bookkeeping, not log archaeology: a turn may
    /// be [advance…, begin] or a bare advance, and no suffix rule can tell
    /// them apart once appended (v0.5).
    turn_lengths: Vec<usize>,
}

impl Game {
    /// Compile a text script into a Game; the dictionary must be supplied
    /// explicitly.
    pub fn from_script(text: &str, dict: Dictionary) -> Result<Game> {
        let compiled = compile_script(text, &dict)?;
        Ok(Self::new(dict, compiled.content, compiled.meta, compiled.state))
    }

    /// The private constructor.
    fn new(dict: Dictionary, content: CompiledContent, meta: ScriptMeta, base: State) -> Game {
        Game {
            log: Log::new(clone_state(&base)),
            state: clone_state(&base),
            base,
            dict,
            content,
            meta,
            turn_lengths: Vec::new(),
        }
    }

    /// The game title, defaulting to "Untitled".
    #[must_use]
    pub fn title(&self) -> String {
        self.meta
            .title
            .clone()
            .unwrap_or_else(|| "Untitled".to_string())
    }

    fn lenient_ctx(&self) -> LenientCtx<'_> {
        LenientCtx {
            dict: &self.dict,
            content: &self.content,
            state: &self.state,
            player: self.meta.player.as_deref(),
        }
    }

    /// one turn of play — pure enough to drive from tests
    /// (`Result` carries prose rendering's error channel)
    pub fn handle_line(&mut self, line: &str) -> Result<Turn> {
        let t = js_trim(line);
        if t.is_empty() {
            return Ok(turn(TurnKind::Info, vec![]));
        }
        // game-over (§9.35): the notice instead of execution; undo still reopens
        if let Some(ended) = self.state.ended.clone() {
            return Ok(turn(TurnKind::Info, vec![ending_notice(&ended)]));
        }
        let parsed = parse_player_input(t, &self.lenient_ctx());
        let step = match parsed {
            ParsedInput::Ok { step, .. } => step,
            ParsedInput::Fail { error, suggestions } => {
                let mut lines = vec![error];
                if !suggestions.is_empty() {
                    lines.push(format!("did you mean: {}?", suggestions.join(", ")));
                }
                return Ok(turn(TurnKind::Error, lines));
            }
        };
        // the engine consumes its input state, so the clone IS the draft
        // hand-off
        let result = execute_step(
            &self.content,
            &self.dict,
            clone_state(&self.state),
            step,
            ExecOptions {
                first_record_id: self.log.next_record_id(),
                id_gen: None,
            },
        )?;
        // a rejection leaves log and state untouched
        if let Some(rejected) = result.rejected {
            return Ok(turn(TurnKind::Error, vec![rejected]));
        }
        self.turn_lengths.push(self.log.records.len());
        let mut lines: Vec<String> = Vec::new();
        for r in &result.records {
            lines.extend(prose_lines(r, &self.content, &self.dict)?);
        }
        // the « … » canonical echo
        lines.push(String::new());
        lines.push(format!(
            "« {} »",
            result
                .records
                .iter()
                .map(|r| r.sentence.clone())
                .collect::<Vec<_>>()
                .join(" | ")
        ));
        self.log.append_all(result.records);
        self.state = result.state;
        Ok(turn(TurnKind::Action, lines))
    }

    // ---- meta views for the player interface (rendered as buttons) ----

    /// the cards this player can see: places; the player; cards sharing the
    /// player's location; carried cards; contents of an explored (named)
    /// place; and anything the log has named through a role or a
    /// spawn/destroy — a projection over (state, log), never stored.
    pub fn visible(&self) -> Vec<&Entity> {
        let alive: Vec<&Entity> = self
            .state
            .entities
            .values()
            .filter(|e| e.dead_action.is_none())
            .collect();
        let me = self.meta.player.as_ref().and_then(|p| {
            alive
                .iter()
                .copied()
                .find(|e| &e.name == p)
        });
        let mut named: HashSet<String> = HashSet::new();
        for r in &self.log.records {
            let mut refs: Vec<&Ref> = r
                .actors
                .iter()
                .chain(r.objects.iter())
                .chain(r.instruments.iter())
                .collect();
            if let Some(at) = &r.at {
                refs.push(at);
            }
            if let Some(about) = &r.about {
                refs.push(about);
            }
            if let Some(to) = &r.to {
                refs.push(to);
            }
            for rf in refs {
                if let Ref::Name { name, .. } = rf {
                    named.insert(name.clone());
                }
            }
            for o in &r.outcomes {
                for a in &o.args {
                    // INTO targets name a destination, not a discovery — keep
                    // them out (the `{ role }` into-target shape is skipped)
                    match a {
                        Arg::Ref {
                            r#ref: Ref::Name { name, .. },
                        }
                        | Arg::Into {
                            r#ref:
                                crate::core::sentence::IntoTarget::Ref(Ref::Name { name, .. }),
                        } => {
                            named.insert(name.clone());
                        }
                        _ => {}
                    }
                }
            }
        }
        let here: Option<&AttrValue> = me.and_then(|m| m.attrs.get("container"));
        let mut out: Vec<&Entity> = Vec::new();
        for e in alive {
            let keep = e.kind == "place"
                || me.is_some_and(|m| e.id == m.id)
                || e.attrs.get("container") == here
                || me.is_some_and(|m| {
                    matches!(e.attrs.get("container"), Some(AttrValue::Str(c)) if *c == m.id)
                })
                || named.contains(&e.name)
                || match e.attrs.get("container") {
                    Some(AttrValue::Str(c)) => self
                        .state
                        .entities
                        .get(c)
                        .is_some_and(|container| container.kind == "place" && named.contains(&container.name)),
                    _ => false,
                };
            if keep {
                out.push(e);
            }
        }
        out
    }

    /// what is around the player
    pub fn look(&self) -> Vec<String> {
        let player = self.meta.player.as_ref().and_then(|p| {
            self.state
                .entities
                .values()
                .find(|e| e.dead_action.is_none() && &e.name == p)
        });
        let Some(player) = player else {
            return vec!["(no player character — open the cards view)".to_string()];
        };
        let here: Option<&AttrValue> = player.attrs.get("container");
        let place = match here {
            Some(AttrValue::Str(c)) => self.state.entities.get(c),
            _ => None,
        };
        let mut lines = vec![format!(
            "You are at {}.",
            place.map_or("nowhere", |p| p.name.as_str())
        )];
        let things: Vec<&Entity> = self
            .state
            .entities
            .values()
            .filter(|e| {
                e.dead_action.is_none() && e.attrs.get("container") == here && e.id != player.id
            })
            .collect();
        for thing in &things {
            lines.push(format!("  {}  [{}]", thing.name, thing.kind));
        }
        if things.is_empty() {
            lines.push("  (nothing else here)".to_string());
        }
        lines
    }

    /// every visible card — the table the player actually sees
    pub fn cards(&self) -> Vec<String> {
        let lines: Vec<String> = self
            .visible()
            .iter()
            .map(|e| format!("{}  [{}]", e.name, e.kind))
            .collect();
        if lines.is_empty() {
            vec!["(no cards)".to_string()]
        } else {
            lines
        }
    }

    /// a card's face: kind, aspects, where it sits, when it entered play.
    /// Visible cards only — the table has no face-down deck to fish from.
    /// QUIRK(Q11): bare `inspect` (an empty prefix always matches) shows
    /// the first visible card — no test pins it; frozen as-is.
    pub fn inspect(&self, raw_name: &str) -> Vec<String> {
        let q = js_trim(raw_name).to_lowercase().replace('_', " ");
        let table = self.visible();
        let face = |e: &Entity| e.name.to_lowercase().replace('_', " ");
        let card = table
            .iter()
            .copied()
            .find(|e| face(e) == q)
            .or_else(|| table.iter().copied().find(|e| face(e).starts_with(&q)));
        let Some(card) = card else {
            return vec![format!(
                "nothing on the table answers to '{}' — press 2 for the table",
                js_trim(raw_name)
            )];
        };
        let mut lines = vec![format!("{}  [{}]", card.name, card.kind)];
        let aspects = flatten_aspects(&card.aspects);
        if !aspects.is_empty() {
            lines.push(format!("  {}", aspects.join(" · ")));
        }
        if let Some(AttrValue::Str(c)) = card.attrs.get("container") {
            if let Some(container) = self.state.entities.get(c) {
                lines.push(format!("  at: {}", pretty(&container.name)));
            }
        }
        // `if (card.attrs['zone'])` is JS truthiness:
        // '' / 0 / false are silent, everything else prints
        if is_truthy(card.attrs.get("zone")) {
            lines.push(format!(
                "  zone: {}",
                card.attrs
                    .get("zone")
                    .map_or(String::new(), |v| v.to_js_string())
            ));
        }
        if let Some(n) = card.born_action {
            if n > 0 {
                lines.push(format!("  entered play at action {n}"));
            }
        }
        lines
    }

    /// the log as prose
    pub fn story(&self) -> Result<Vec<String>> {
        let mut lines = Vec::new();
        for r in &self.log.records {
            lines.push(format!("· {}", prose_lines(r, &self.content, &self.dict)?.join("\n  ")));
        }
        if lines.is_empty() {
            lines.push("(nothing has happened yet)".to_string());
        }
        Ok(lines)
    }

    /// the canonical log
    #[must_use]
    pub fn journal(&self) -> Vec<String> {
        let j = journal(&self.log.records);
        if j.is_empty() {
            vec!["(the journal is empty)".to_string()]
        } else {
            vec![j]
        }
    }

    /// take back the last turn: truncate the log to where it started and
    /// re-fold (spec §7 — state = fold of the surviving log)
    pub fn undo(&mut self) -> Result<Turn> {
        let Some(length) = self.turn_lengths.pop() else {
            return Ok(turn(TurnKind::Info, vec!["nothing to undo".to_string()]));
        };
        self.log.truncate(length);
        self.state = fold(
            &self.log.records,
            &clone_state(&self.base),
            &self.dict,
            &self.content,
        )?;
        Ok(turn(TurnKind::Info, vec!["undone.".to_string()]))
    }

    /// the open processing windows, soonest-completing first — the live
    /// area's "processing:" line (v0.5).
    #[must_use]
    pub fn processing(&self) -> Vec<String> {
        let mut windows: Vec<&OpenWindow> = self.state.windows.values().collect();
        windows.sort_by(|a, b| a.end.cmp(&b.end).then(a.begin_id.cmp(&b.begin_id)));
        windows
            .iter()
            .map(|w| {
                let end = clock_from_abs_min(w.end);
                format!(
                    "{} — until {}:{}",
                    w.verb.to_lowercase().replace('_', " "),
                    end.day,
                    end.halfhour
                )
            })
            .collect()
    }

    /// let time pass: advance to the earliest window's end, or one hour when
    /// nothing is in flight (the `wait` button; typing `1:12` does the same).
    /// The clock input is halfhour-quantized, so round the target UP — the
    /// advance then resolves everything due at or before it.
    pub fn wait(&mut self) -> Result<Turn> {
        let now = self.state.now;
        // `windows.size > 0 ? Math.min(...) : now + 60` — min of an empty set
        // is None, which lands on the same now + 60 arm
        let due_at = self
            .state
            .windows
            .values()
            .map(|w| w.end)
            .min()
            .unwrap_or(now + 60);
        // ceil to the halfhour — the JS `Math.ceil(dueAt / 30) * 30` spelling;
        // the manual ceil-division is exact for
        // the non-negative minutes the engine ever holds (i64::div_ceil is
        // still feature-gated on this toolchain)
        let target =
            (due_at + MIN_PER_HALFHOUR - 1) / MIN_PER_HALFHOUR * MIN_PER_HALFHOUR;
        let clock = clock_from_abs_min(target);
        let Clock { day, halfhour } = clock;
        self.handle_line(&format!("{day}:{halfhour}"))
    }

    /// concrete actions the player can take right now: verb × card bindings
    /// that fit the slots, those lighting up a non-default recipe first —
    /// every suggestion is lenient-parseable, so typing it just works.
    /// Verbs with an open station and cards locked in a window are excluded
    /// (they would only reject at the engine, v0.5). The limit is
    /// frozen as [`SUGGEST_LIMIT`]
    /// (`Result` carries eval_pred's error channel).
    pub fn suggestions(&self) -> Result<Vec<String>> {
        const SUGGEST_LIMIT: usize = 3;
        if self.state.ended.is_some() {
            return Ok(vec![]); // §9.35: nothing left to suggest after the ending
        }
        let alive = self.visible();
        let me = self.meta.player.as_ref().and_then(|p| {
            alive
                .iter()
                .copied()
                .find(|e| &e.name == p)
        });
        let Some(me) = me else {
            return Ok(vec![]);
        };
        let busy_verbs: HashSet<&str> = self
            .state
            .windows
            .values()
            .map(|w| w.verb.as_str())
            .collect();
        let busy_cards: HashSet<&str> = self
            .state
            .windows
            .values()
            .flat_map(|w| w.locked.iter().map(String::as_str))
            .collect();
        // same-container cards rank first (the Option
        // equality models JS `===` over possibly-undefined attrs)
        let me_container = me.attrs.get("container");
        let rank = |e: &Entity| {
            if e.attrs.get("container") == me_container {
                0
            } else {
                1
            }
        };
        let lights = |verb: &CompiledVerb, bindings: &RoleBindings| -> Result<bool> {
            let p = PredCtx {
                b: bindings,
                state: &self.state,
                strict: false,
                roll: None,
            };
            // a REFUSE gate that matches is a wall, not an invitation
            // (`.some()` over the recipes)
            for r in &verb.recipes {
                let Some(w) = &r.when else { continue };
                if r.out.iter().any(|o| o.op == "REFUSE") {
                    continue;
                }
                if eval_pred(w, &p)? {
                    return Ok(true);
                }
            }
            Ok(false)
        };
        // the display phrase; bindings hold ids,
        // resolved through the live state here
        let phrase = |display: &str, b: &RoleBindings| -> String {
            let mut parts: Vec<String> = vec![display.to_string()];
            for role in [Role::Object, Role::To, Role::With, Role::At, Role::About] {
                let Some(ids) = b.get(&role) else { continue };
                if ids.is_empty() {
                    continue;
                }
                let names: Vec<String> = ids
                    .iter()
                    .map(|id| match self.state.entities.get(id) {
                        Some(e) => pretty(&e.name),
                        // suggestion bindings are built from living cards below
                        None => unreachable!("suggestion bindings hold living ids"),
                    })
                    .collect();
                parts.push(match role {
                    Role::Object => names.join(" "),
                    Role::To => format!("with {}", names.join(" ")),
                    _ => format!("{} {}", role.as_str(), names.join(" ")),
                });
            }
            parts.join(" ")
        };

        let mut story: Vec<String> = Vec::new();
        let mut flavor: Vec<String> = Vec::new();
        for verb in self.content.verbs.values() {
            if busy_verbs.contains(verb.name.as_str()) {
                continue; // the station is processing (v0.5)
            }
            let Some(actors_slot) = verb.slot_by_role.get("actors") else {
                continue;
            };
            if actors_slot.min < 1 {
                continue;
            }
            if actors_slot
                .query
                .as_ref()
                .is_some_and(|q| !match_query(me, q))
            {
                continue;
            }
            let display = verb.name.to_lowercase().replace('_', " ");

            let mut pools: IndexMap<String, Vec<&Entity>> = IndexMap::new();
            let mut missing = false;
            for slot in &verb.slots {
                if slot.role == "actors" {
                    continue;
                }
                let pool: Vec<&Entity> = alive
                    .iter()
                    .copied()
                    .filter(|e| {
                        e.name != me.name
                            && !matches!(me.attrs.get("container"),
                                Some(AttrValue::Str(c)) if *c == e.id)
                            && !busy_cards.contains(e.id.as_str())
                            && slot.query.as_ref().is_none_or(|q| match_query(e, q))
                    })
                    .collect();
                // rank ascending, then name — the JS comparator
                // `rank(a) - rank(b) || a.name.localeCompare(b.name)`;
                // the localeCompare tail is §3.21's frozen approximation
                // (cmp_ascii_ci — ASCII-lowercase then byte order)
                let mut pool = pool;
                pool.sort_by(|a, b| {
                    rank(a)
                        .cmp(&rank(b))
                        .then_with(|| crate::js::cmp_ascii_ci(&a.name, &b.name))
                });
                if slot.min >= 1 && (pool.len() as i64) < slot.min {
                    missing = true;
                    break;
                }
                pools.insert(slot.role.clone(), pool);
            }
            if missing {
                continue;
            }

            // base binding: each role filled to its minimum
            let mut base: RoleBindings = IndexMap::new();
            base.insert(Role::Actors, vec![me.id.clone()]);
            for (role_str, pool) in &pools {
                let Some(slot) = verb.slot_by_role.get(role_str) else {
                    // pool keys ⊆ slot roles ⊆ `slot_by_role` keys — the lookup
                    // never misses (a miss there would panic the same way)
                    unreachable!("pool role is always a declared slot role")
                };
                let role = Role::from_lower(role_str);
                if let Some(role) = role {
                    base.insert(
                        role,
                        pool.iter()
                            .take(std::cmp::min(slot.min, 3) as usize)
                            .map(|e| e.id.clone())
                            .collect(),
                    );
                }
                // (a typo'd slot role (Q4) would add a junk binding key no
                // predicate or phrase ever reads — skipping it is exact)
            }
            let mut verb_story: Vec<String> = Vec::new();
            if lights(verb, &base)? {
                verb_story.push(phrase(&display, &base));
            } else {
                flavor.push(phrase(&display, &base));
            }

            // alternative bindings: swap or extend role cards; keep what lights
            for (role_str, pool) in &pools {
                let Some(slot) = verb.slot_by_role.get(role_str) else {
                    unreachable!("pool role is always a declared slot role")
                };
                    // cap = max(slot.min, min(slot.max, 2)) — a JS Infinity max
                    // (None) loses to the 2
                let cap = std::cmp::max(slot.min, std::cmp::min(slot.max.unwrap_or(2), 2));
                let role = Role::from_lower(role_str);
                let base_es: Vec<String> = role
                    .as_ref()
                    .and_then(|r| base.get(r))
                    .cloned()
                    .unwrap_or_default();
                for e in pool {
                    if base_es.iter().any(|x| x == &e.id) {
                        continue;
                    }
                    let mut combined = base_es.clone();
                    combined.push(e.id.clone());
                    if combined.len() as i64 > cap {
                        combined = vec![e.id.clone()];
                    }
                    let mut alt = base.clone();
                    if let Some(role) = role {
                        alt.insert(role, combined);
                    }
                    if lights(verb, &alt)? {
                        verb_story.push(phrase(&display, &alt));
                    }
                }
            }
            story.extend(verb_story);
        }
        // round-robin across verbs so one greedy verb does not own the list
        let mut by_verb: Vec<Vec<String>> = Vec::new();
        let mut i = 0usize;
        while i < story.len() {
            let first_word = story[i].split(' ').next().unwrap_or("").to_string();
            let mut group = Vec::new();
            while i < story.len()
                && story[i].split(' ').next().unwrap_or("") == first_word
            {
                group.push(story[i].clone());
                i += 1;
            }
            if !group.is_empty() {
                by_verb.push(group);
            }
        }
        let mut mixed: Vec<String> = Vec::new();
        while mixed.len() < 64 && by_verb.iter().any(|g| !g.is_empty()) {
            for g in &mut by_verb {
                if let Some(v) = if g.is_empty() { None } else { Some(g.remove(0)) } {
                    mixed.push(v);
                }
            }
        }
        // dedupe, keep the lenient-parseable ones, cap at the limit
        let mut seen: HashSet<String> = HashSet::new();
        let mut out: Vec<String> = Vec::new();
        for s in mixed.into_iter().chain(flavor) {
            if seen.contains(&s) {
                continue;
            }
            seen.insert(s.clone());
            if parse_player_input(&s, &self.lenient_ctx()).is_ok() {
                out.push(s);
            }
        }
        Ok(out.into_iter().take(SUGGEST_LIMIT).collect())
    }
}

/// JS truthiness over a JSON primitive or a missing value.
fn is_truthy(v: Option<&AttrValue>) -> bool {
    match v {
        None => false,                                  // undefined
        Some(AttrValue::Str(s)) => !s.is_empty(),       // ''
        Some(AttrValue::Num(n)) => *n != 0,             // 0 (and NaN's domain is empty here)
        Some(AttrValue::Bool(b)) => *b,                 // false
    }
}
