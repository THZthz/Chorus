//! The `--world` global dashboard: the whole-world face of the analyzer.
//! Every section is a re-render of facts [`WorldFacts`]/[`Census`] already
//! hold after the fixpoint — ZERO new analysis runs here; the module only
//! copies, groups and renders. The contract is determinism: section data
//! follows IndexMap/IndexSet insertion order (content order underneath), so
//! two runs over the same (content, initial) render byte-identical text.

use std::fmt::Write as _;

use indexmap::IndexMap;

use crate::content::{CompiledContent, CompiledTrigger};
use crate::core::types::State;

use super::census::MANY;
use super::render::query_text;
use super::world::{Analyzer, INFINITY};

// ---------------------------------------------------------------------------
// the report shape
// ---------------------------------------------------------------------------

/// The overview counts (always rendered — a world dashboard has them even
/// when every count is zero).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorldOverview {
    pub live_sites: usize,
    pub refused_sites: usize,
    pub verbs: usize,
    pub prototypes: usize,
    pub decks: usize,
    pub slot_triggers: usize,
    pub expire_triggers: usize,
    pub complete_triggers: usize,
}

/// The freeze section: the frozen bit, the bounded freeze distance and the
/// risky owners. The owners are listed by name — [`WorldFacts`] records the
/// demotion verdict, not its cause category, and the dashboard does not
/// re-derive one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FreezeInfo {
    pub frozen: bool,
    /// first-crossing minutes to the freeze boundary; `None` when unbounded
    /// (unfrozen worlds carry [`INFINITY`])
    pub freeze_dist_min: Option<i64>,
    /// risky owners, insertion order (the risky block's demotion order)
    pub risky_owners: Vec<String>,
}

/// One dead verb and the slot that killed it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeadVerbRow {
    pub verb: String,
    /// the role of the first min>0 slot whose query can never match —
    /// exactly the predicate `build_post_fixpoint` condemned the verb with
    pub role: String,
    /// that slot's query, design-time spelling
    pub query: String,
}

/// One deck's capacity from the proven draw sites.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeckCapRow {
    pub deck: String,
    /// `None` = the [`MANY`] sentinel — the deck draws unbounded
    pub cap: Option<i64>,
}

/// One SET-written path with the owners that write it (the threshold-pump
/// map).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SetPumpRow {
    pub key: String,
    /// first-occurrence order, exact (owner, flags) duplicates dropped
    pub sites: Vec<SetPumpSite>,
}

/// One owner writing a SET path, with the census annotations.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SetPumpSite {
    pub owner: String,
    /// the SET sits behind an authored REFUSE — it can never persist
    pub refused: bool,
    /// an earlier when-less recipe of the same verb shadows it
    pub shadowed: bool,
    /// an enclosing IF condition is roll/role-dependent
    pub path_non_static: bool,
}

/// The whole-world report: nine sections, each omitted entirely when its
/// data is empty (overview and freeze always render).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorldReport {
    pub overview: WorldOverview,
    pub freeze: FreezeInfo,
    pub dead_verbs: Vec<DeadVerbRow>,
    pub deck_caps: Vec<DeckCapRow>,
    pub collision_names: Vec<String>,
    /// name → the owners (list display strings) that destroy/replace it,
    /// insertion order throughout
    pub destroyed_by: Vec<(String, Vec<String>)>,
    pub set_pumps: Vec<SetPumpRow>,
    /// the owners carrying at least one refused site, first-mention order
    pub refused_owners: Vec<String>,
    pub notes: Vec<String>,
}

// ---------------------------------------------------------------------------
// world_report — copy + group, zero analysis
// ---------------------------------------------------------------------------

/// The world dashboard over one (content, initial) pair. A fresh
/// [`Analyzer`] per call — the world facts are a pure function of the
/// inputs, so this is observably identical to sharing one.
#[must_use]
pub fn world_report(content: &CompiledContent, initial: &State) -> WorldReport {
    let analyzer = Analyzer::new(content, initial);
    let facts = analyzer.facts();

    // overview: content-level counts + the census split
    let (mut slot, mut expire, mut complete) = (0usize, 0usize, 0usize);
    for t in &content.triggers {
        match t {
            CompiledTrigger::Slot { .. } => slot += 1,
            CompiledTrigger::Expire { .. } => expire += 1,
            CompiledTrigger::Complete { .. } => complete += 1,
        }
    }
    let overview = WorldOverview {
        live_sites: facts.census.live.len(),
        refused_sites: facts.census.refused.len(),
        verbs: content.verbs.len(),
        prototypes: content.prototypes.len(),
        decks: content.decks.len(),
        slot_triggers: slot,
        expire_triggers: expire,
        complete_triggers: complete,
    };

    let freeze = FreezeInfo {
        frozen: facts.frozen,
        freeze_dist_min: (facts.freeze_dist != INFINITY).then_some(facts.freeze_dist),
        risky_owners: facts.risky_owners.iter().cloned().collect(),
    };

    // dead verbs: the killing slot is the FIRST (slot_by_role order) min>0
    // slot whose query can never match — the very predicate
    // `build_post_fixpoint` condemned the verb with, so the rendered reason
    // is the actual cause, not a re-derivation
    let mut dead_verbs: Vec<DeadVerbRow> = Vec::new();
    for &vi in &facts.dead_verbs {
        let Some((_, verb)) = content.verbs.get_index(vi) else {
            unreachable!("verb index {vi} came from content.verbs enumeration");
        };
        let Some((role, slot)) = verb
            .slot_by_role
            .iter()
            .find(|(_, s)| s.min > 0 && !facts.can_ever_match(s.query.as_ref()))
        else {
            continue; // the deadness predicate drifted (paths.rs) — omit, don't panic
        };
        let Some(q) = &slot.query else { continue };
        dead_verbs.push(DeadVerbRow {
            verb: verb.name.clone(),
            role: role.clone(),
            query: query_text(q),
        });
    }

    // deck caps: insertion order = proven-site order
    let deck_caps: Vec<DeckCapRow> = facts
        .deck_cap
        .iter()
        .map(|(deck, &cap)| DeckCapRow { deck: deck.clone(), cap: (cap != MANY).then_some(cap) })
        .collect();

    let collision_names: Vec<String> = facts.census.collision_names.iter().cloned().collect();

    let destroyed_by: Vec<(String, Vec<String>)> = facts
        .census
        .destroyed_by
        .iter()
        .map(|(name, owners)| (name.clone(), owners.iter().cloned().collect()))
        .collect();

    // the pump map: census walk order grouped by key; within a key, exact
    // (owner, flags) duplicates collapse (one SET per list is the common
    // shape, but an author may write the same path twice)
    let mut pumps: IndexMap<String, Vec<SetPumpSite>> = IndexMap::new();
    for s in &facts.census.set_sites {
        let row = SetPumpSite {
            owner: s.owner.clone(),
            refused: s.refused,
            shadowed: s.shadowed,
            path_non_static: s.path_non_static,
        };
        let list = pumps.entry(s.key.clone()).or_default();
        if !list.contains(&row) {
            list.push(row);
        }
    }
    let set_pumps: Vec<SetPumpRow> =
        pumps.into_iter().map(|(key, sites)| SetPumpRow { key, sites }).collect();

    let mut refused_owners: Vec<String> = Vec::new();
    for &i in &facts.census.refused {
        let owner = facts.census.sites[i].owner.clone();
        if !refused_owners.contains(&owner) {
            refused_owners.push(owner);
        }
    }

    WorldReport {
        overview,
        freeze,
        dead_verbs,
        deck_caps,
        collision_names,
        destroyed_by,
        set_pumps,
        refused_owners,
        notes: facts.notes.clone(),
    }
}

// ---------------------------------------------------------------------------
// render_report — the dashboard text
// ---------------------------------------------------------------------------

/// The dashboard's stdout shape: one title line, then two-space-indented
/// sections in the report order — single-line sections render as one line,
/// multi-item sections as a header line plus `  · ` bullets; empty sections
/// are omitted whole. Deterministic: the data walk orders are insertion
/// orders, nothing here sorts or rehashes.
pub fn render_report(out: &mut String, file: &str, r: &WorldReport) {
    let _ = writeln!(out, "{file} · world");
    let o = &r.overview;
    let _ = writeln!(
        out,
        "  overview: {} live / {} refused site(s), {} verb(s), {} prototype(s), {} deck(s), triggers {} slot / {} expire / {} complete",
        o.live_sites,
        o.refused_sites,
        o.verbs,
        o.prototypes,
        o.decks,
        o.slot_triggers,
        o.expire_triggers,
        o.complete_triggers
    );
    // frozen worlds always carry a bounded distance (the min over the risky
    // slot triggers); the None arm is the defensive face
    match (r.freeze.frozen, r.freeze.freeze_dist_min) {
        (true, Some(d)) => {
            let _ = writeln!(out, "  freeze: frozen — first risky crossing in {d} min");
        }
        (true, None) => {
            let _ = writeln!(out, "  freeze: frozen");
        }
        (false, _) => {
            let _ = writeln!(out, "  freeze: not frozen");
        }
    }
    if !r.freeze.risky_owners.is_empty() {
        let _ = writeln!(out, "  · risky: {}", r.freeze.risky_owners.join(", "));
    }
    if !r.dead_verbs.is_empty() {
        let _ = writeln!(out, "  dead verbs:");
        for d in &r.dead_verbs {
            let _ = writeln!(out, "  · {} — role {} can never fill: {}", d.verb, d.role, d.query);
        }
    }
    if !r.deck_caps.is_empty() {
        let _ = writeln!(out, "  deck caps:");
        for c in &r.deck_caps {
            match c.cap {
                Some(n) => {
                    let _ = writeln!(out, "  · {} — {n}", c.deck);
                }
                None => {
                    let _ = writeln!(out, "  · {} — unbounded", c.deck);
                }
            }
        }
    }
    if !r.collision_names.is_empty() {
        let _ = writeln!(out, "  collision names: {}", r.collision_names.join(", "));
    }
    if !r.destroyed_by.is_empty() {
        let _ = writeln!(out, "  destroyed-by:");
        for (name, owners) in &r.destroyed_by {
            let _ = writeln!(out, "  · {name} ← {}", owners.join(", "));
        }
    }
    if !r.set_pumps.is_empty() {
        let _ = writeln!(out, "  set pumps:");
        for p in &r.set_pumps {
            let sites = p
                .sites
                .iter()
                .map(|s| {
                    let mut label = s.owner.clone();
                    let mut flags: Vec<&str> = Vec::new();
                    if s.refused {
                        flags.push("refused");
                    }
                    if s.shadowed {
                        flags.push("shadowed");
                    }
                    if s.path_non_static {
                        flags.push("non-static path");
                    }
                    if !flags.is_empty() {
                        label.push_str(&format!(" [{}]", flags.join(", ")));
                    }
                    label
                })
                .collect::<Vec<String>>()
                .join(", ");
            let _ = writeln!(out, "  · {} — {}", p.key, sites);
        }
    }
    if !r.refused_owners.is_empty() {
        let _ = writeln!(out, "  refused sites:");
        for owner in &r.refused_owners {
            let _ = writeln!(out, "  · {owner}");
        }
    }
    for n in &r.notes {
        let _ = writeln!(out, "  note: {n}");
    }
    out.push('\n');
}

// ---------------------------------------------------------------------------
// tests — inline scripts through compile_script, byte-exact sections
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::content::script::compile_script;
    use crate::core::dict::core_dictionary;

    /// A fixture exercising every section: a same-slot mint pair freezes the
    /// world at halfhour 8 (240 min), a once trigger draws a finite deck, a
    /// recipe draw stays unbounded, a REFUSE arm refuses one site, a DESTROY
    /// feeds destroyed-by, two SETs feed the pump map (one shadowed), and a
    /// verb with an unfillable slot dies.
    const RICH: &str = "\
script: World Report Fixture
player: MARCH
goal: exercise every dashboard section
seed: 7
start: 0:00

entities:
  HALL is place
  MARCH is character at HALL
  THE_RELIC is item at HALL

prototypes:
  GIFT_A is item
  GIFT_B is item
  GIFT_C is item
  THE_CHARM is item
  COIN is resource stackable
  TALISMAN is resource stackable

triggers:
  t_kill once at halfhour 2 -> DESTROY THE_RELIC
  t_draw once at halfhour 3 -> DRAW RELIC_DRAW
  t_pump at halfhour 6 -> SET(HALL, aspect:gleam, 1), SET(MARCH, expires, 1h)
  t_freeze_a at halfhour 8 -> SPAWN THE_CHARM
  t_freeze_b at halfhour 8 -> SPAWN THE_CHARM
  t_expire on expire of MARCH -> NARRATE \"the inspector sleeps\"

decks:
  POUCH spec COIN defaultcard COIN reset
  RELIC_DRAW spec TALISMAN

verbs:
  WORK:
    actors: 1 kind:character
    duration: 0m
    recipes:
      work.shift -> DRAW POUCH, NARRATE \"a shift's honest work\"
      work.gleam -> SET(ACTORS, aspect:gleam, 1), NARRATE \"polished\"
      work.moonlight -> SPAWN GIFT_A, IF exists(THE_RELIC) -> (SPAWN GIFT_B) ELSE -> (SPAWN GIFT_C, REFUSE(\"the hour is wrong\"))
  READ:
    actors: 1 kind:character
    object: 1 aspect:unwritable>=1
    duration: 1h
    recipes:
      read.never -> NARRATE \"never\"
";

    /// Compile a fixture and render its dashboard (two fresh compiles render
    /// byte-identically — the determinism contract).
    fn render_rich() -> String {
        let dict = core_dictionary();
        let cs = compile_script(RICH, &dict).expect("fixture compiles");
        let mut buf = String::new();
        render_report(&mut buf, "fixture.txt", &world_report(&cs.content, &cs.state));
        buf
    }

    #[test]
    fn rich_fixture_renders_every_section_byte_exact() {
        let expected = "\
fixture.txt · world
  overview: 6 live / 1 refused site(s), 2 verb(s), 6 prototype(s), 2 deck(s), triggers 5 slot / 1 expire / 0 complete
  freeze: frozen — first risky crossing in 240 min
  · risky: trigger t_freeze_a, trigger t_freeze_b
  dead verbs:
  · READ — role object can never fill: aspect:unwritable>=1
  deck caps:
  · POUCH — unbounded
  · RELIC_DRAW — 1
  collision names: HALL, MARCH, THE_RELIC, GIFT_A, GIFT_B, GIFT_C, THE_CHARM
  destroyed-by:
  · THE_RELIC ← trigger t_kill
  set pumps:
  · aspect:gleam — recipe work.gleam of WORK [shadowed], trigger t_pump
  · expires — trigger t_pump
  refused sites:
  · recipe work.moonlight of WORK
  note: 1 producer site(s) sit behind an authored REFUSE — they can never apply and are excluded

";
        assert_eq!(render_rich(), expected);
    }

    #[test]
    fn two_runs_render_byte_identically() {
        assert_eq!(render_rich(), render_rich());
    }

    #[test]
    fn bare_world_omits_empty_sections() {
        let dict = core_dictionary();
        let cs = compile_script(
            "\
script: Bare Room
player: SOLO
goal: nothing to reach

entities:
  SOLO is character at ROOM
  ROOM is place
",
            &dict,
        )
        .expect("fixture compiles");
        let mut buf = String::new();
        render_report(&mut buf, "bare.txt", &world_report(&cs.content, &cs.state));
        assert_eq!(
            buf,
            "\
bare.txt · world
  overview: 0 live / 0 refused site(s), 0 verb(s), 0 prototype(s), 0 deck(s), triggers 0 slot / 0 expire / 0 complete
  freeze: not frozen
  collision names: SOLO, ROOM

"
        );
    }
}
