//! Play interface — a text script becomes a playable game.
//! Game = script + log; handle_line is the whole turn loop (testable without a
//! TTY); the REPL (bin/chorus.rs) renders it.
//!
//! The full player surface: handle_line, undo
//! (+ `turn_lengths` bookkeeping), wait's ceil-to-halfhour quantization,
//! visible/suggestions/story/journal/look/cards/inspect, and BOTH game-over
//! texts (this file speaks the UI notice; the engine's rejected short
//! text stays in `engine::exec`).

use indexmap::IndexMap;
use std::collections::HashSet;

use crate::content::script::{compile_script, ScriptMeta};
use crate::content::{CompiledContent, CompiledSlot, CompiledVerb};
use crate::core::clock::{clock_from_abs_min, Clock, MIN_PER_HALFHOUR};
use crate::core::dict::Dictionary;
use crate::core::predicate::{eval_pred, PredCtx, RoleBindings};
use crate::core::query::{match_query, QueryTerm};
use crate::core::r#ref::Ref;
use crate::core::roles::Role;
use crate::core::sentence::{Arg, IntoTarget};
use crate::core::text::{cmp_ascii_ci, dsl_trim};
use crate::core::types::{clone_state, is_truthy, AttrValue, Entity, OpenWindow, State};
use crate::engine::fold::fold;
use crate::engine::store::Log;
use crate::engine::{execute_step, ExecOptions};
use crate::error::Result;

use super::lenient::{parse_player_input, LenientCtx, ParsedInput};
use super::render::{
    clock_text, display_name, ending_notice, flatten_aspects, hhmm, journal, pretty, prose_lines,
};

/// the idle wait advance: one hour when nothing is in flight (§11)
const IDLE_WAIT_MIN: i64 = 60;

/// the round-robin scan cap over the per-verb suggestion groups — the frozen
/// [`SUGGEST_LIMIT`] (3) always bites before it does
const ROUND_ROBIN_CAP: usize = 64;

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

// ---- suggestion machinery: pools, bindings, phrases (§11's try row) ----

/// the candidate pools per slot: living visible cards other than the
/// player, not carried by the player and not locked in a window, that match
/// the slot's query — the `actors` pool holds the player's possible
/// companions (same kind as the player when the slot has no query). `None`
/// when a min-count slot cannot fill (the verb is skipped outright); the
/// actors slot counts the player toward its minimum.
fn verb_pools<'a>(
    verb: &CompiledVerb,
    alive: &[&'a Entity],
    me: &Entity,
    busy_cards: &HashSet<&str>,
) -> Option<IndexMap<String, Vec<&'a Entity>>> {
    // same-container cards rank first (the Option
    // equality models strict equality over possibly-missing attrs)
    let me_container = me.attrs.get("container");
    let rank = |e: &Entity| if e.attrs.get("container") == me_container { 0 } else { 1 };
    let mut pools: IndexMap<String, Vec<&Entity>> = IndexMap::new();
    for slot in &verb.slots {
        let is_actors = slot.role == "actors";
        let pool: Vec<&Entity> = alive
            .iter()
            .copied()
            .filter(|e| {
                e.name != me.name
                    && !matches!(me.attrs.get("container"),
                        Some(AttrValue::Str(c)) if *c == e.id)
                    && !busy_cards.contains(e.id.as_str())
                    && match &slot.query {
                        Some(q) => match_query(e, q),
                        None => !is_actors || e.kind == me.kind,
                    }
            })
            .collect();
        // rank ascending, then name — the frozen comparator:
        // rank ascending, ties broken by `cmp_ascii_ci`
        // (§11's frozen name order — ASCII-lowercase then byte order)
        let mut pool = pool;
        pool.sort_by(|a, b| rank(a).cmp(&rank(b)).then_with(|| cmp_ascii_ci(&a.name, &b.name)));
        let have = pool.len() as i64 + i64::from(is_actors);
        if slot.min >= 1 && have < slot.min {
            return None;
        }
        pools.insert(slot.role.clone(), pool);
    }
    Some(pools)
}

/// base binding: each role filled to its minimum — the player first in
/// `actors`, companions after up to the slot's minimum
fn base_binding(
    me: &Entity,
    verb: &CompiledVerb,
    pools: &IndexMap<String, Vec<&Entity>>,
) -> RoleBindings {
    let mut base: RoleBindings = IndexMap::new();
    base.insert(Role::Actors, vec![me.id.clone()]);
    for (role_str, pool) in pools {
        let Some(slot) = verb.slot_by_role.get(role_str) else {
            // pool keys ⊆ slot roles ⊆ `slot_by_role` keys — the lookup
            // never misses (a miss there would panic the same way)
            unreachable!("pool role is always a declared slot role")
        };
        let Some(role) = Role::from_lower(role_str) else {
            // (a typo'd slot role (Q4) would add a junk binding key no
            // predicate or phrase ever reads — skipping it is exact)
            continue;
        };
        if role == Role::Actors {
            let mut ids = vec![me.id.clone()];
            ids.extend(pool.iter().take((slot.min - 1).max(0) as usize).map(|e| e.id.clone()));
            base.insert(role, ids);
            continue;
        }
        base.insert(
            role,
            pool.iter().take(std::cmp::min(slot.min, 3) as usize).map(|e| e.id.clone()).collect(),
        );
    }
    base
}

/// the per-role fill cap a suggestion may reach: the slot's minimum, or up
/// to three cards where the slot allows (the inquest wants three proofs
/// laid together — an alternative that stops at two could never light it)
fn fill_cap(slot: &CompiledSlot) -> i64 {
    std::cmp::max(slot.min, std::cmp::min(slot.max.unwrap_or(3), 3))
}

/// `NAME  [kind]` rows, pool copies grouped as `×N`, first-seen order
fn grouped_cards(cards: &[&Entity]) -> Vec<String> {
    let mut seen: IndexMap<String, (String, usize)> = IndexMap::new();
    for e in cards {
        let entry = seen.entry(e.name.clone()).or_insert((e.kind.clone(), 0));
        entry.1 += 1;
    }
    seen.iter()
        .map(|(name, (kind, n))| {
            let times = if *n > 1 { format!(" ×{n}") } else { String::new() };
            format!("{}  [{kind}]{times}", pretty(name))
        })
        .collect()
}

/// the one line a finished story answers with: the notice plus the way
/// out (`undo` reopens the night, `quit` leaves)
fn ended_line(e: &crate::core::types::Ending) -> String {
    format!("{} `undo` takes back the last turn; `quit` leaves.", ending_notice(e))
}

/// display names, pool copies grouped as `×N`, first-seen order
fn grouped_names(cards: &[&Entity]) -> Vec<String> {
    let mut seen: IndexMap<String, usize> = IndexMap::new();
    for e in cards {
        *seen.entry(e.name.clone()).or_insert(0) += 1;
    }
    seen.iter()
        .map(|(name, n)| {
            let times = if *n > 1 { format!(" ×{n}") } else { String::new() };
            format!("{}{times}", pretty(name))
        })
        .collect()
}

/// round-robin across the per-verb story groups so one greedy verb does not
/// own the list; [`ROUND_ROBIN_CAP`] stops the scan.
fn round_robin(groups: Vec<Vec<String>>, cap: usize) -> Vec<String> {
    let mut mixed: Vec<String> = Vec::new();
    let mut groups = groups;
    while mixed.len() < cap && groups.iter().any(|g| !g.is_empty()) {
        for g in &mut groups {
            if !g.is_empty() {
                mixed.push(g.remove(0));
            }
        }
    }
    mixed
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
    /// the ended line has been spoken since the story ended — one line, not
    /// one per turn (cleared by undo, which can reopen the book)
    ended_announced: bool,
}

impl Game {
    /// Compile a text script into a Game; the dictionary must be supplied
    /// explicitly. The Game carries the LOAD-EFFECTIVE dictionary back
    /// (§4.1/§4.3): the supplied one plus the script's `kinds:` and `verbs:`
    /// declarations — relation types are unchanged, and the MECHANICS never
    /// read the word lists at runtime (verb lookup is content-driven), but
    /// the play SURFACE does change with the vocabulary: the sentence
    /// parser, the lenient input, the suggestions and the banner read this
    /// dictionary per call (§11) — which is exactly what makes a
    /// script-declared verb typeable, suggestable, and re-parseable in the
    /// fold (v0.7.6).
    pub fn from_script(text: &str, dict: Dictionary) -> Result<Game> {
        let compiled = compile_script(text, &dict)?;
        Ok(Self::new(compiled.dict, compiled.content, compiled.meta, compiled.state))
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
            ended_announced: false,
        }
    }

    /// The game title, defaulting to "Untitled".
    #[must_use]
    pub fn title(&self) -> String {
        self.meta.title.clone().unwrap_or_else(|| "Untitled".to_string())
    }

    fn lenient_ctx(&self) -> LenientCtx<'_> {
        LenientCtx {
            dict: &self.dict,
            content: &self.content,
            state: &self.state,
            player: self.meta.player.as_deref(),
        }
    }

    /// the living entity behind `meta.player`, if the script declares one
    fn living_player(&self) -> Option<&Entity> {
        let player = self.meta.player.as_ref()?;
        self.state.entities.values().find(|e| e.dead_action.is_none() && &e.name == player)
    }

    /// one turn of play — pure enough to drive from tests
    /// (`Result` carries prose rendering's error channel)
    pub fn handle_line(&mut self, line: &str) -> Result<Turn> {
        let t = dsl_trim(line);
        if t.is_empty() {
            return Ok(turn(TurnKind::Info, vec![]));
        }
        // game-over (§4.4): the notice instead of execution; undo still
        // reopens (the REPL's live area adds the way out once, `after_turn`)
        if let Some(ended) = self.state.ended.clone() {
            return Ok(turn(TurnKind::Info, vec![ending_notice(&ended)]));
        }
        let parsed = parse_player_input(t, &self.lenient_ctx());
        let step = match parsed {
            ParsedInput::Ok { step, .. } => step,
            ParsedInput::Fail { error, suggestions, hint } => {
                let mut lines = vec![error];
                if !suggestions.is_empty() {
                    lines.push(format!("did you mean: {}?", suggestions.join(", ")));
                }
                lines.extend(hint);
                return Ok(turn(TurnKind::Error, lines));
            }
        };
        // the verb the player reached for — a rejection below explains its
        // shape, a refusal below shows its authored hints
        let verb_typed: Option<String> = step.clauses.first().map(|c| c.verb.clone());
        // the engine consumes its input state, so the clone IS the draft
        // hand-off
        let result = execute_step(
            &self.content,
            &self.dict,
            clone_state(&self.state),
            step,
            ExecOptions { first_record_id: self.log.next_record_id(), id_gen: None },
        )?;
        // a rejection leaves log and state untouched; the verb's usage line
        // turns the engine's slot arithmetic into something to type
        if let Some(rejected) = result.rejected {
            let mut lines = vec![rejected];
            if let Some(verb) = verb_typed.as_deref() {
                lines.extend(self.usage(verb));
            }
            return Ok(turn(TurnKind::Error, lines));
        }
        self.turn_lengths.push(self.log.records.len());
        let mut lines: Vec<String> = Vec::new();
        for r in &result.records {
            lines.extend(prose_lines(r, &self.content, &self.dict)?);
        }
        // a REFUSED action shows the authored hint of the recipe that refused
        // (§6.2's `shown` requirements) — or, when that recipe carries none
        // (a bare default REFUSE), the verb's posted hints: the refusal said
        // no; the hint says what would do
        let refusing: Vec<&str> = result
            .records
            .iter()
            .filter(|r| r.kind == "action" && r.outcomes.iter().any(|o| o.op == "REFUSE"))
            .flat_map(|r| r.recipes.iter().map(String::as_str))
            .collect();
        if !refusing.is_empty() {
            if let Some(verb) = verb_typed.as_deref() {
                lines.extend(self.hints(verb, &refusing));
            }
        }
        // the « … » canonical echo of the player's OWN sentence — the BEGIN
        // row(s) the line produced; the engine-written COMPLETE / EVENT / WAIT
        // rows stay in the journal (they carry ids and materialized JSON
        // that read as noise beside the prose)
        let own: Vec<String> = result
            .records
            .iter()
            .filter(|r| r.kind == "action")
            .map(|r| r.sentence.clone())
            .collect();
        if !own.is_empty() {
            lines.push(String::new());
            lines.push(format!("« {} »", own.join(" | ")));
        }
        self.log.append_all(result.records);
        self.state = result.state;
        Ok(turn(TurnKind::Action, lines))
    }

    // ---- the live area (§11): banner, status, the try row, help ----

    /// the opening screen: title, the script's goal, where and when the
    /// player stands, what shares the place, the try row, the verb
    /// vocabulary, and the way to `help`
    pub fn banner(&self) -> Result<Vec<String>> {
        let mut lines = vec![format!("── {} ──", self.title())];
        if let Some(goal) = &self.meta.goal {
            lines.push(format!("§ {goal}"));
        }
        lines.push(self.status_line());
        lines.extend(self.here_line());
        lines.extend(self.try_row()?);
        lines.push(format!("verbs: {}", self.verb_list()));
        lines.push("type `help` for how to play".to_string());
        Ok(lines)
    }

    /// the configured verbs, display form, dot-separated (the banner row)
    fn verb_list(&self) -> String {
        self.content.verbs.keys().map(|v| display_name(v)).collect::<Vec<_>>().join(" · ")
    }

    /// `you are MARCH · at GUARD VAN · day 0, 21:00`
    #[must_use]
    pub fn status_line(&self) -> String {
        let when = clock_text(self.state.now);
        match self.living_player() {
            Some(me) => match self.container_of(me) {
                Some(place) => {
                    format!("you are {} · at {} · {when}", pretty(&me.name), pretty(&place.name))
                }
                None => format!("you are {} · {when}", pretty(&me.name)),
            },
            None => format!("no player character · {when}"),
        }
    }

    /// the living entity a card sits in, if any
    fn container_of(&self, e: &Entity) -> Option<&Entity> {
        match e.attrs.get("container") {
            Some(AttrValue::Str(c)) => self.state.entities.get(c),
            _ => None,
        }
    }

    /// the cards sharing the player's place, pool copies grouped —
    /// `here: THE GUARD · THE INQUEST · SUSPICION ×2`; nothing when empty
    fn here_line(&self) -> Option<String> {
        let me = self.living_player()?;
        let here = me.attrs.get("container");
        let things: Vec<&Entity> = self
            .state
            .entities
            .values()
            .filter(|e| {
                e.dead_action.is_none() && e.attrs.get("container") == here && e.id != me.id
            })
            .collect();
        if things.is_empty() {
            return None;
        }
        Some(format!("here: {}", grouped_names(&things).join(" · ")))
    }

    /// what follows a turn in the REPL: the stations still processing and
    /// the try row (`wait` joins it while anything is cooking); after the
    /// ending, the one line that says so
    pub fn after_turn(&mut self) -> Result<Vec<String>> {
        if let Some(e) = &self.state.ended.clone() {
            // the turn that ended the story gets the line here, under its
            // banner; every later turn already answered with it
            return Ok(if self.ended_announced {
                vec![]
            } else {
                self.ended_announced = true;
                vec![ended_line(e)]
            });
        }
        // the clock every turn — a timetable mystery is lost without it —
        // with the stations still processing beside it
        let mut now = format!("now: {}", clock_text(self.state.now));
        let processing = self.processing();
        if !processing.is_empty() {
            now.push_str(&format!(" · processing: {}", processing.join(" · ")));
        }
        let mut lines: Vec<String> = vec![now];
        lines.extend(self.try_row()?);
        Ok(lines)
    }

    /// `try: read the manifest · talk to the guard · wait` — the suggestions,
    /// each typeable as shown, plus `wait` while a station is processing
    fn try_row(&self) -> Result<Vec<String>> {
        let mut items = self.suggestions()?;
        if !self.state.windows.is_empty() && self.state.ended.is_none() {
            items.push("wait".to_string());
        }
        Ok(if items.is_empty() { vec![] } else { vec![format!("try: {}", items.join(" · "))] })
    }

    /// how to play, in the game's own vocabulary: the action shape with live
    /// examples from the try row, the people-before-the-verb rule, the role
    /// words, the clock, the views
    pub fn help(&self) -> Result<Vec<String>> {
        let examples = self.suggestions()?;
        let example = match examples.first() {
            Some(e) => e.clone(),
            None => format!(
                "{} <thing>",
                display_name(&self.content.verbs.keys().next().cloned().unwrap_or_default())
            ),
        };
        let people = self
            .living_player()
            .map(|me| display_name(&me.name))
            .unwrap_or_else(|| "someone".to_string());
        Ok(vec![
            "how to play".to_string(),
            format!("  act: <verb> <thing…> — e.g. {example}"),
            format!(
                "  people go BEFORE the verb: `{people} <someone> <verb> …` makes two of you act together"
            ),
            "  role words: to · with · at · about — `use on <thing> with <proof> <proof>`".to_string(),
            "  names forgive case, underscores and articles; one distinctive word is enough".to_string(),
            "  time: an action opens a station that finishes later — `wait` jumps to the next thing due; a clock like 1:4 jumps there".to_string(),
            format!("  verbs: {}", self.verb_list()),
            "  views: look · cards · inspect <card> · story · journal · undo · help · quit".to_string(),
        ])
    }

    /// one line of usage for a verb, built from its slots: `usage: use on
    /// <thing> [with <thing…>]`; multi-person verbs say where the people go
    fn usage(&self, verb: &str) -> Vec<String> {
        let Some(v) = self.content.verbs.get(verb) else { return vec![] };
        let placeholder = |role: &str, slot: &CompiledSlot| -> String {
            // the slot's own kind when its query names one kind in every
            // alternation group (`travel to <place>`, `read <text>`), else a
            // thing
            let kinds: Vec<&str> = slot
                .query
                .as_ref()
                .map(|q| {
                    q.iter()
                        .filter_map(|group| {
                            group.iter().find_map(|t| match t {
                                QueryTerm::Kind { neg: false, kind } => Some(kind.as_str()),
                                _ => None,
                            })
                        })
                        .collect()
                })
                .unwrap_or_default();
            let kind = match kinds.first() {
                Some(k) if kinds.iter().all(|x| x == k) => Some(*k),
                _ => None,
            };
            let noun = match kind {
                Some("character") => "<someone>".to_string(),
                Some(k) => format!("<{k}>"),
                None => "<thing>".to_string(),
            };
            let many = slot.max.is_none_or(|m| m > 1);
            let body = if many { format!("{noun}…") } else { noun.to_string() };
            let body = if role == "object" { body } else { format!("{role} {body}") };
            if slot.min == 0 {
                format!("[{body}]")
            } else {
                body
            }
        };
        let mut parts: Vec<String> = vec![display_name(verb)];
        for role in ["object", "to", "with", "at", "about"] {
            if let Some(slot) = v.slot_by_role.get(role) {
                parts.push(placeholder(role, slot));
            }
        }
        let mut lines = vec![format!("usage: {}", parts.join(" "))];
        if let Some(actors) = v.slot_by_role.get("actors") {
            let me = self
                .living_player()
                .map(|m| display_name(&m.name))
                .unwrap_or_else(|| "someone".into());
            if actors.min >= 2 {
                lines.push(format!(
                    "  {} needs {} people, named before the verb: `{me} <someone> {}`",
                    display_name(verb),
                    actors.min,
                    display_name(verb)
                ));
            } else if actors.max.is_none_or(|m| m > 1) {
                lines.push(format!(
                    "  a companion goes before the verb: `{me} <someone> {} …`",
                    display_name(verb)
                ));
            }
        }
        lines
    }

    /// the authored `shown` hints (§6.2) a refusal leaves behind: the hint
    /// of the recipe that refused when it carries one, else every posted
    /// hint of the verb (a bare default REFUSE points at the posted ones)
    fn hints(&self, verb: &str, refusing: &[&str]) -> Vec<String> {
        let Some(v) = self.content.verbs.get(verb) else { return vec![] };
        let own: Vec<String> = v
            .recipes
            .iter()
            .filter(|r| refusing.contains(&r.id.as_str()))
            .filter_map(|r| r.hint.as_ref())
            .map(|h| format!("hint: {h}"))
            .collect();
        if !own.is_empty() {
            return own;
        }
        v.recipes
            .iter()
            .filter(|r| r.shown)
            .filter_map(|r| r.hint.as_ref())
            .take(2)
            .map(|h| format!("hint: {h}"))
            .collect()
    }

    /// the player's exits: every place linked to the player's container by
    /// a living direction relation, labelled from the stored edge (`A is
    /// south of B` puts A south of B and B north of A) — exactly the
    /// adjacency TRAVEL_TO-style verbs gate on with `rel(<dir>, …)`
    fn exits(&self) -> Vec<(String, String)> {
        let Some(me) = self.living_player() else { return vec![] };
        let Some(here) = self.container_of(me) else { return vec![] };
        const DIRS: [(&str, &str); 4] =
            [("north", "south"), ("south", "north"), ("east", "west"), ("west", "east")];
        let mut out: Vec<(String, String)> = Vec::new();
        for (dir, opposite) in DIRS {
            for r in self.state.relations.values() {
                if r.dead_action.is_some() || r.type_ != dir {
                    continue;
                }
                // `a is <dir> of b`: from b, a lies <dir>; from a, b lies the
                // opposite way
                let (label, other) = if r.b == here.id {
                    (dir, &r.a)
                } else if r.a == here.id {
                    (opposite, &r.b)
                } else {
                    continue;
                };
                if let Some(place) = self.state.entities.get(other) {
                    if place.dead_action.is_none() {
                        out.push((label.to_string(), pretty(&place.name)));
                    }
                }
            }
        }
        // north, south, east, west — the table's reading order
        let rank = |d: &str| DIRS.iter().position(|(x, _)| *x == d).unwrap_or(4);
        out.sort_by_key(|(d, _)| rank(d));
        out
    }

    // ---- meta views: the typed words the player interface routes to ----

    /// the cards this player can see: places; the player; cards sharing the
    /// player's location; carried cards; contents of an explored (named)
    /// place; and anything the log has named through a role or a
    /// spawn/destroy — a projection over (state, log), never stored.
    pub fn visible(&self) -> Vec<&Entity> {
        let alive: Vec<&Entity> =
            self.state.entities.values().filter(|e| e.dead_action.is_none()).collect();
        let me = self.living_player();
        let mut named: HashSet<String> = HashSet::new();
        for r in &self.log.records {
            let mut refs: Vec<&Ref> =
                r.actors.iter().chain(r.objects.iter()).chain(r.instruments.iter()).collect();
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
                    // outcome refs AND INTO destinations join the named set
                    // — a place something was sent to is a place the player
                    // has heard of (§11); only the `{ role }` into-target
                    // shape is skipped (it names no card)
                    match a {
                        Arg::Ref { r#ref: Ref::Name { name, .. } }
                        | Arg::Into { r#ref: IntoTarget::Ref(Ref::Name { name, .. }) } => {
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
                || me.is_some_and(
                    |m| matches!(e.attrs.get("container"), Some(AttrValue::Str(c)) if *c == m.id),
                )
                || named.contains(&e.name)
                || match e.attrs.get("container") {
                    Some(AttrValue::Str(c)) => {
                        self.state.entities.get(c).is_some_and(|container| {
                            container.kind == "place" && named.contains(&container.name)
                        })
                    }
                    _ => false,
                };
            if keep {
                out.push(e);
            }
        }
        out
    }

    /// what is around the player: the place and the clock, the cards here,
    /// the exits, what the player carries, the stations still processing
    pub fn look(&self) -> Vec<String> {
        let Some(player) = self.living_player() else {
            return vec!["(no player character — open the cards view)".to_string()];
        };
        let here: Option<&AttrValue> = player.attrs.get("container");
        let place = self.container_of(player);
        let mut lines = vec![format!(
            "You are at {} — {}.",
            place.map_or_else(|| "nowhere".to_string(), |p| pretty(&p.name)),
            clock_text(self.state.now)
        )];
        let things: Vec<&Entity> = self
            .state
            .entities
            .values()
            .filter(|e| {
                e.dead_action.is_none() && e.attrs.get("container") == here && e.id != player.id
            })
            .collect();
        for line in grouped_cards(&things) {
            lines.push(format!("  {line}"));
        }
        if things.is_empty() {
            lines.push("  (nothing else here)".to_string());
        }
        let exits = self.exits();
        if !exits.is_empty() {
            let shown: Vec<String> = exits.iter().map(|(d, p)| format!("{d} — {p}")).collect();
            lines.push(format!("exits: {}", shown.join(" · ")));
        }
        let carried: Vec<&Entity> = self
            .state
            .entities
            .values()
            .filter(|e| {
                e.dead_action.is_none()
                    && matches!(e.attrs.get("container"), Some(AttrValue::Str(c)) if *c == player.id)
            })
            .collect();
        if !carried.is_empty() {
            lines.push(format!("carrying: {}", grouped_names(&carried).join(" · ")));
        }
        match &self.state.ended {
            Some(e) => lines.push(ended_line(e)),
            None => {
                let processing = self.processing();
                if !processing.is_empty() {
                    lines.push(format!("processing: {}", processing.join(" · ")));
                }
            }
        }
        lines
    }

    /// every visible card — the table the player actually sees, pool copies
    /// grouped (`SUSPICION  [state] ×2`)
    pub fn cards(&self) -> Vec<String> {
        let lines = grouped_cards(&self.visible());
        if lines.is_empty() {
            vec!["(no cards)".to_string()]
        } else {
            lines
        }
    }

    /// a card's face: kind, aspects, where it sits, when it entered play.
    /// Visible cards only — the table has no face-down deck to fish from.
    /// A bare `inspect` (empty name — the old empty-prefix startswith match
    /// showed the FIRST visible card, Q11) names the usage and lists the
    /// table instead; it never guesses a card.
    pub fn inspect(&self, raw_name: &str) -> Vec<String> {
        let q = display_name(dsl_trim(raw_name));
        if q.is_empty() {
            // Q11: usage + the card list (the same projection `cards`
            // prints), not the first visible card
            let mut lines = vec!["usage: inspect <card name>".to_string()];
            for c in self.cards() {
                lines.push(format!("  {c}"));
            }
            return lines;
        }
        let table = self.visible();
        let face = |e: &Entity| display_name(&e.name);
        let card = table
            .iter()
            .copied()
            .find(|e| face(e) == q)
            .or_else(|| table.iter().copied().find(|e| face(e).starts_with(&q)));
        let Some(card) = card else {
            // a card that left play is named as such — the corpse the
            // narration just described is not a typo
            let gone = self
                .state
                .entities
                .values()
                .filter(|e| e.dead_action.is_some())
                .find(|e| face(e) == q || face(e).starts_with(&q));
            if let Some(gone) = gone {
                let when = gone
                    .dead_action
                    .map(|n| format!(" (left play at action {n})"))
                    .unwrap_or_default();
                return vec![format!("{} is no longer in play{when}", pretty(&gone.name))];
            }
            return vec![format!(
                "nothing on the table answers to '{}' — try 'cards' for the table",
                dsl_trim(raw_name)
            )];
        };
        let mut lines = vec![format!("{}  [{}]", pretty(&card.name), card.kind)];
        let aspects = flatten_aspects(&card.aspects);
        if !aspects.is_empty() {
            lines.push(format!("  {}", aspects.join(" · ")));
        }
        if let Some(AttrValue::Str(c)) = card.attrs.get("container") {
            if let Some(container) = self.state.entities.get(c) {
                lines.push(format!("  at: {}", pretty(&container.name)));
            }
        }
        // `if (card.attrs['zone'])` reads through the frozen truthiness:
        // '' / 0 / false are silent, everything else prints
        if is_truthy(card.attrs.get("zone")) {
            lines.push(format!(
                "  zone: {}",
                card.attrs.get("zone").map_or(String::new(), |v| v.to_canonical_string())
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
            let prose = prose_lines(r, &self.content, &self.dict)?;
            // a prose-silent row (an EVENT that only moved state) has nothing
            // to tell; the journal keeps its canonical sentence
            if prose.is_empty() {
                continue;
            }
            lines.push(format!("· {}", prose.join("\n  ")));
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
        // name what goes: the turn's own sentence(s), or the wait it was
        let own: Vec<String> = self.log.records[length..]
            .iter()
            .filter(|r| r.kind == "action")
            .map(|r| r.sentence.clone())
            .collect();
        let what = if own.is_empty() {
            "the wait".to_string()
        } else {
            format!("« {} »", own.join(" | "))
        };
        self.log.truncate(length);
        self.state = fold(&self.log.records, &clone_state(&self.base), &self.dict, &self.content)?;
        if self.state.ended.is_none() {
            self.ended_announced = false;
        }
        Ok(turn(TurnKind::Info, vec![format!("undone: {what}"), self.status_line()]))
    }

    /// the open processing windows, soonest-completing first — the live
    /// area's "processing:" line (v0.5). The (end, begin_id) order is the
    /// engine's frozen due-window order (the advance pass's sort in
    /// `engine::advance`, minus the clause tiebreak a rendering never needs).
    #[must_use]
    pub fn processing(&self) -> Vec<String> {
        let mut windows: Vec<&OpenWindow> = self.state.windows.values().collect();
        windows.sort_by(|a, b| a.end.cmp(&b.end).then(a.begin_id.cmp(&b.begin_id)));
        windows.iter().map(|w| format!("{} until {}", display_name(&w.verb), hhmm(w.end))).collect()
    }

    /// let time pass: advance to the earliest window's end, or one hour when
    /// nothing is in flight (the `wait` meta word; typing `1:12` does the
    /// same). The clock input is halfhour-quantized, so round the target UP —
    /// the advance then resolves everything due at or before it.
    pub fn wait(&mut self) -> Result<Turn> {
        let now = self.state.now;
        let due_at =
            self.state.windows.values().map(|w| w.end).min().unwrap_or(now + IDLE_WAIT_MIN);
        // ceil to the halfhour — the manual ceil-division is exact for the
        // non-negative minutes the engine ever holds (i64::div_ceil is still
        // feature-gated on this toolchain)
        let target = (due_at + MIN_PER_HALFHOUR - 1) / MIN_PER_HALFHOUR * MIN_PER_HALFHOUR;
        let clock = clock_from_abs_min(target);
        let Clock { day, halfhour } = clock;
        self.handle_line(&format!("{day}:{halfhour}"))
    }

    /// the recipe these bindings would light, mirroring the engine's §6.4
    /// first-match selection: the first non-default recipe whose `when`
    /// holds decides — a REFUSE gate that matches is a wall (`None`), not an
    /// invitation, and nothing behind it counts; a spent `max` recipe is
    /// passed over as the engine passes it over.
    fn lit_recipe<'v>(&self, verb: &'v CompiledVerb, b: &RoleBindings) -> Result<Option<&'v str>> {
        let p = PredCtx { b, state: &self.state, strict: false, roll: None };
        for r in &verb.recipes {
            let Some(w) = &r.when else { continue };
            if let Some(max) = r.max {
                if self.state.executed.get(&r.id).is_some_and(|&n| n >= max) {
                    continue;
                }
            }
            if eval_pred(w, &p)? {
                let refuses = r.out.iter().any(|o| o.op == "REFUSE");
                return Ok(if refuses { None } else { Some(r.id.as_str()) });
            }
        }
        Ok(None)
    }

    /// does a non-default recipe light on these bindings?
    fn recipe_lights(&self, verb: &CompiledVerb, b: &RoleBindings) -> Result<bool> {
        Ok(self.lit_recipe(verb, b)?.is_some())
    }

    /// the display phrase, typeable as shown: companions before the verb
    /// when the binding has them (`march brack work`), the verb, then each
    /// role with its keyword and lowercase display names (`talk to the
    /// guard`, `use on the inquest with the embezzlement`); bindings hold
    /// ids, resolved through the live state here
    fn suggestion_phrase(&self, display: &str, b: &RoleBindings) -> String {
        let names = |ids: &[String]| -> Vec<String> {
            ids.iter()
                .map(|id| match self.state.entities.get(id) {
                    Some(e) => display_name(&e.name),
                    // suggestion bindings are built from living cards below
                    None => unreachable!("suggestion bindings hold living ids"),
                })
                .collect()
        };
        let mut parts: Vec<String> = Vec::new();
        if let Some(actors) = b.get(&Role::Actors) {
            if actors.len() > 1 {
                parts.push(names(actors).join(" "));
            }
        }
        parts.push(display.to_string());
        for role in [Role::Object, Role::To, Role::With, Role::At, Role::About] {
            let Some(ids) = b.get(&role) else { continue };
            if ids.is_empty() {
                continue;
            }
            let joined = names(ids).join(" ");
            parts.push(match role {
                Role::Object => joined,
                _ => format!("{} {joined}", role.as_str()),
            });
        }
        parts.join(" ")
    }

    /// alternative bindings: per role, extend the base by one more card (or
    /// swap once the cap is reached) and lay the whole hand up to the cap;
    /// a companion joins the actors the same way. Keep what lights a recipe
    /// the base does NOT already light — an extra card that changes nothing
    /// (`read the manifest at dining car`) is noise, not an option.
    fn alt_bindings(
        &self,
        verb: &CompiledVerb,
        base: &RoleBindings,
        pools: &IndexMap<String, Vec<&Entity>>,
    ) -> Result<Vec<RoleBindings>> {
        let base_lit = self.lit_recipe(verb, base)?;
        let mut alts: Vec<RoleBindings> = Vec::new();
        for (role_str, pool) in pools {
            let Some(slot) = verb.slot_by_role.get(role_str) else {
                unreachable!("pool role is always a declared slot role")
            };
            let Some(role) = Role::from_lower(role_str) else { continue };
            let cap = fill_cap(slot);
            let base_es: Vec<String> = base.get(&role).cloned().unwrap_or_default();
            let mut candidates: Vec<Vec<String>> = Vec::new();
            for e in pool {
                if base_es.iter().any(|x| x == &e.id) {
                    continue;
                }
                let mut combined = base_es.clone();
                combined.push(e.id.clone());
                if combined.len() as i64 > cap {
                    combined = if role == Role::Actors {
                        // the player always acts; the companion is what swaps
                        vec![base_es[0].clone(), e.id.clone()]
                    } else {
                        vec![e.id.clone()]
                    };
                }
                candidates.push(combined);
            }
            // the full hand: everything the pool offers, up to the cap
            let full: Vec<String> = if role == Role::Actors {
                let mut ids = vec![base_es[0].clone()];
                ids.extend(pool.iter().take((cap - 1).max(0) as usize).map(|e| e.id.clone()));
                ids
            } else {
                pool.iter().take(cap as usize).map(|e| e.id.clone()).collect()
            };
            if full.len() as i64 > 1 && full != base_es && !candidates.contains(&full) {
                candidates.push(full);
            }
            for combined in candidates {
                let mut alt = base.clone();
                alt.insert(role, combined);
                let lit = self.lit_recipe(verb, &alt)?;
                if lit.is_some() && lit != base_lit {
                    alts.push(alt);
                }
            }
        }
        Ok(alts)
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
            return Ok(vec![]); // §4.4: nothing left to suggest after the ending
        }
        let alive = self.visible();
        let Some(me) = self.living_player() else {
            return Ok(vec![]);
        };
        let busy_verbs: HashSet<&str> =
            self.state.windows.values().map(|w| w.verb.as_str()).collect();
        let busy_cards: HashSet<&str> =
            self.state.windows.values().flat_map(|w| w.locked.iter().map(String::as_str)).collect();
        let mut story: Vec<Vec<String>> = Vec::new();
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
            if actors_slot.query.as_ref().is_some_and(|q| !match_query(me, q)) {
                continue;
            }
            let display = display_name(&verb.name);
            let Some(pools) = verb_pools(verb, &alive, me, &busy_cards) else {
                continue;
            };
            let base = base_binding(me, verb, &pools);
            let mut verb_story: Vec<String> = Vec::new();
            if self.recipe_lights(verb, &base)? {
                verb_story.push(self.suggestion_phrase(&display, &base));
            } else {
                flavor.push(self.suggestion_phrase(&display, &base));
            }
            for alt in self.alt_bindings(verb, &base, &pools)? {
                verb_story.push(self.suggestion_phrase(&display, &alt));
            }
            story.push(verb_story);
        }
        let mixed = round_robin(story, ROUND_ROBIN_CAP);
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
