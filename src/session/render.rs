//! L3 projection (spec §5.3, §6.7, §8, §4.4). Pure
//! functions over records/state.

use crate::content::CompiledContent;
use crate::core::clock::{format_duration, MIN_PER_TICK, TICKS_PER_DAY};
use crate::core::dict::Dictionary;
use crate::core::parse::parse_sentence;
use crate::core::r#ref::Ref;
use crate::core::sentence::{Arg, Clause};
use crate::core::types::{AspectVal, Aspects, Ending};
use crate::engine::ActionRecord;
use crate::error::Result;

/// One row's play-surface text: the canonical sentence, then the executed
/// recipe ids in a trailing bracket — `0:42 @ MARCH READ THE_MANIFEST
/// [read.manifest]`. A REFUSED row's bracket names the recipe that refused;
/// a step no recipe matched never becomes a row (the engine's rejection
/// line says `<verb>: no recipe matched`). A §6.4 deferred clause's BEGIN
/// entry is the EMPTY id — the deferral mark — and renders nothing (the
/// COMPLETE row carries the selected id).
#[must_use]
pub fn row_text(r: &ActionRecord) -> String {
    let ids: Vec<&str> = r.recipes.iter().map(String::as_str).filter(|id| !id.is_empty()).collect();
    if ids.is_empty() {
        return r.sentence.clone();
    }
    format!("{} [{}]", r.sentence, ids.join(", "))
}

/// The canonical journal: one canonical sentence per line — a row that
/// executed recipes carries their ids in the same trailing bracket the
/// play echo shows (§11).
#[must_use]
pub fn journal(records: &[ActionRecord]) -> String {
    records.iter().map(row_text).collect::<Vec<_>>().join("\n")
}

/// the status-area banner once the story has ended: — NO MORE · melancholy —
#[must_use]
pub fn ending_banner(e: &Ending) -> String {
    format!("— {} · {} —", e.title.to_uppercase(), e.flavour)
}

/// the notice shown when the player acts after the ending
#[must_use]
pub fn ending_notice(e: &Ending) -> String {
    format!("the story has ended — {} ({}).", e.title, e.flavour)
}

/// display form of an authored name: THE_FORGETTING → THE FORGETTING
#[must_use]
pub fn pretty(name: &str) -> String {
    name.replace('_', " ")
}

/// The player's clock: `day 0, 21:00` from absolute minutes — the
/// canonical `d:u` form (144 ticks a day) is the log's spelling, this
/// is the one a reader keeps time by.
#[must_use]
pub fn clock_text(abs_min: i64) -> String {
    let per_day = TICKS_PER_DAY * MIN_PER_TICK;
    format!("day {}, {}", abs_min.div_euclid(per_day), hhmm(abs_min))
}

/// The time of day alone: `21:00`.
#[must_use]
pub fn hhmm(abs_min: i64) -> String {
    let per_day = TICKS_PER_DAY * MIN_PER_TICK;
    let m = abs_min.rem_euclid(per_day);
    format!("{:02}:{:02}", m / 60, m % 60)
}

/// the display form of a registered verb lexeme (§11's banner list): USE_ON
/// → "use on" — lowercased, underscores spaced
#[must_use]
pub fn display_name(name: &str) -> String {
    name.to_lowercase().replace('_', " ")
}

/// nested aspects as flat "path level" strings: tome.cylinder 1. Q13's flat
/// one-level model: sub-values are plain ints, so one pass is the whole shape.
#[must_use]
pub fn flatten_aspects(a: &Aspects) -> Vec<String> {
    let mut out = Vec::new();
    for (k, v) in a {
        match v {
            AspectVal::Num(n) => {
                out.push(format!("{k} {n}"));
            }
            // sub-values are plain ints (Q13's flat model), so the recursion
            // into `<prefix><k>.` yields exactly this one level
            AspectVal::Sub(sub) => {
                for (sk, sv) in sub {
                    out.push(format!("{k}.{sk} {sv}"));
                }
            }
        }
    }
    out
}

/// Prose rendering: one template line per clause, the processing ticks (§6.7)
/// indented under them, then NARRATE lines. COMPLETE rows skip the clause
/// templates (the window already narrated them at BEGIN) and render their
/// NARRATE/REFUSE/ENDING outcomes through the shared path below; WAIT rows
/// narrate the passing of time so they are not prose-silent.
/// (the `Result` carries `format_duration`/parse's
/// error channel — both error paths are unreachable on engine-built records)
pub fn prose_lines(
    record: &ActionRecord,
    content: &CompiledContent,
    dict: &Dictionary,
) -> Result<Vec<String>> {
    let mut lines: Vec<String> = Vec::new();
    if record.kind == "wait" {
        return Ok(vec![format!("  (+{}) time passes.", format_duration(record.advanced_min)?)]);
    }
    if record.kind == "action" {
        let s = parse_sentence(&record.sentence, dict)?;
        for clause in &s.clauses {
            if let Some(template) = content.templates.get(&clause.verb) {
                lines.push(interpolate_clause(template, clause));
            }
        }
    }
    for o in &record.outcomes {
        let Some(narr) = o.args.iter().find_map(|a| match a {
            Arg::Str { v } => Some(v),
            _ => None,
        }) else {
            continue;
        };
        match o.op.as_str() {
            "PROGRESS" => {
                let off = match o.args.iter().find_map(|a| match a {
                    Arg::Dur { v } => Some(*v),
                    _ => None,
                }) {
                    Some(d) => format_duration(d)?,
                    None => "0m".to_string(),
                };
                lines.push(format!("  (+{off}) {}", interpolate_record(narr, record)));
            }
            "NARRATE" => lines.push(interpolate_record(narr, record)),
            "REFUSE" => lines.push(narr.clone()), // §6.2: refusal is always spoken
            _ => {}
        }
    }
    // §4.4: a step that ends the story closes with its ending line — first
    // ENDING wins, mirroring the state rule; the journal stays canonical (the
    // `ENDING <id>` outcome is already visible there). A REFUSED step's ending
    // was discarded with the whole draft (§6.5), so it never gets the line.
    let refused = record.outcomes.iter().any(|o| o.op == "REFUSE");
    let ending = if refused { None } else { record.outcomes.iter().find(|o| o.op == "ENDING") };
    if let Some(ending) = ending {
        let id = match ending.args.first() {
            Some(Arg::Ref { r#ref: Ref::Name { name, .. } }) => Some(name.clone()),
            _ => None,
        };
        let def = id.as_deref().and_then(|id| content.endings.get(id));
        let title = match (&def, &id) {
            (Some(d), _) => d.title.clone(),
            (None, Some(id)) => id.clone(),
            (None, None) => "the end".to_string(),
        };
        lines.push(format!("— {title} —"));
    }
    Ok(lines)
}

/// The prose as one string.
pub fn prose(
    record: &ActionRecord,
    content: &CompiledContent,
    dict: &Dictionary,
) -> Result<String> {
    Ok(prose_lines(record, content, dict)?.join("\n"))
}

/// A lone ref: the name's display form / #id / the em dash fallback
/// (param refs are authored-only and never reach a record)
fn ref_name(r: Option<&Ref>) -> String {
    match r {
        Some(Ref::Name { name, .. }) => pretty(name),
        Some(Ref::Id { id }) => format!("#{id}"),
        _ => "—".to_string(),
    }
}

/// Refs inside a list render display names / #ids (a param
/// ref yields its name).
fn ref_names(rs: &[Ref]) -> Vec<String> {
    rs.iter()
        .map(|r| match r {
            Ref::Name { name, .. } | Ref::Param { name } => pretty(name),
            Ref::Id { id } => format!("#{id}"),
        })
        .collect()
}

/// The role refs one interpolation draws from — the clause and record
/// variants share this projection (a record's instruments are the clause's
/// `with`, and a record's `held` is the §6.7 held binding; a clause never
/// carries one).
struct InterpRefs<'a> {
    actors: &'a [Ref],
    objects: &'a [Ref],
    at: Option<&'a Ref>,
    about: Option<&'a Ref>,
    to: Option<&'a Ref>,
    with: &'a [Ref],
    held: Option<&'a Ref>,
}

impl InterpRefs<'_> {
    fn of_clause(clause: &Clause) -> InterpRefs<'_> {
        InterpRefs {
            actors: &clause.actors,
            objects: &clause.objects,
            at: clause.at.as_ref(),
            about: clause.about.as_ref(),
            to: clause.to.as_ref(),
            with: &clause.with,
            held: None,
        }
    }

    fn of_record(record: &ActionRecord) -> InterpRefs<'_> {
        InterpRefs {
            actors: &record.actors,
            objects: &record.objects,
            at: record.at.as_ref(),
            about: record.about.as_ref(),
            to: record.to.as_ref(),
            with: &record.instruments,
            held: record.held.as_ref(),
        }
    }
}

/// The token replaces run in the authored
/// order (actors, object, at, about, to, with, held); str::replace is a global
/// replace, the exact /g semantics.
fn interpolate_refs(template: &str, refs: &InterpRefs<'_>) -> String {
    template
        .replace("{actors}", &join_and(&ref_names(refs.actors)))
        .replace("{object}", ref_names(refs.objects).first().map_or("—", String::as_str))
        .replace("{at}", &ref_name(refs.at))
        .replace("{about}", &ref_name(refs.about))
        .replace("{to}", &ref_name(refs.to))
        .replace("{with}", &join_and(&ref_names(refs.with)))
        .replace("{held}", &ref_name(refs.held))
}

/// The clause variant.
fn interpolate_clause(template: &str, clause: &Clause) -> String {
    interpolate_refs(template, &InterpRefs::of_clause(clause))
}

/// The record variant (instruments = with, held = the §6.7 binding).
fn interpolate_record(template: &str, record: &ActionRecord) -> String {
    interpolate_refs(template, &InterpRefs::of_record(record))
}

/// English "A, B and C" joining.
fn join_and(names: &[String]) -> String {
    if names.len() <= 1 {
        return names.first().cloned().unwrap_or_default();
    }
    format!("{} and {}", names[..names.len() - 1].join(", "), names[names.len() - 1])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn banners_and_notice_render_title_plus_flavour() {
        let e = Ending {
            id: "DESPAIR_DEATH".into(),
            title: "No More".into(),
            flavour: "melancholy".into(),
        };
        assert_eq!(ending_banner(&e), "— NO MORE · melancholy —");
        assert_eq!(ending_notice(&e), "the story has ended — No More (melancholy).");
        let grand = Ending {
            id: "LANTERN_VICTORY".into(),
            title: "The Light in the Ruins".into(),
            flavour: "grand".into(),
        };
        assert_eq!(ending_banner(&grand), "— THE LIGHT IN THE RUINS · grand —");
        // flavour is open lowercase vocabulary — any word renders, no crash
        let quiet =
            Ending { id: "QUIET_END".into(), title: "Hush".into(), flavour: "quiet".into() };
        assert_eq!(ending_banner(&quiet), "— HUSH · quiet —");
    }

    #[test]
    fn clock_text_speaks_day_and_time_of_day() {
        assert_eq!(clock_text(0), "day 0, 00:00");
        assert_eq!(clock_text(126 * 10), "day 0, 21:00");
        assert_eq!(clock_text(144 * 10 + 6 * 60), "day 1, 06:00");
        assert_eq!(hhmm(126 * 10 + 10), "21:10");
    }

    #[test]
    fn pretty_and_flatten_and_join_and_match_shapes() {
        assert_eq!(pretty("THE_MANIFEST"), "THE MANIFEST");
        let mut sub = indexmap::IndexMap::new();
        sub.insert("cylinder".to_string(), 1);
        let a: Aspects = [
            ("secret".to_string(), crate::core::types::AspectVal::Num(4)),
            ("tome".to_string(), crate::core::types::AspectVal::Sub(sub)),
        ]
        .into_iter()
        .collect();
        assert_eq!(flatten_aspects(&a), ["secret 4", "tome.cylinder 1"]);
        assert_eq!(join_and(&[]), "");
        assert_eq!(join_and(&["A".to_string()]), "A");
        assert_eq!(join_and(&["A".to_string(), "B".to_string(), "C".to_string()]), "A, B and C");
    }
}
