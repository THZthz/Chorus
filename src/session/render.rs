//! L3 projection (spec §5.3, §6.7, §8, §9.35). Pure
//! functions over records/state.

use crate::content::content::CompiledContent;
use crate::core::clock::format_duration;
use crate::core::dict::Dictionary;
use crate::core::parse::parse_sentence;
use crate::core::r#ref::Ref;
use crate::core::sentence::Arg;
use crate::core::types::{Aspects, Ending};
use crate::engine::engine::ActionRecord;
use crate::error::Result;

/// The canonical journal: one canonical sentence per line.
#[must_use]
pub fn journal(records: &[ActionRecord]) -> String {
    records.iter().map(|r| r.sentence.clone()).collect::<Vec<_>>().join("\n")
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

/// nested aspects as flat "path level" strings: tome.cylinder 1
/// (the `prefix` parameter is the recursion's
/// internal accumulator; the public form starts empty)
#[must_use]
pub fn flatten_aspects(a: &Aspects) -> Vec<String> {
    flatten_with_prefix(a, "")
}

fn flatten_with_prefix(a: &Aspects, prefix: &str) -> Vec<String> {
    let mut out = Vec::new();
    for (k, v) in a {
        match v {
            crate::core::types::AspectVal::Num(n) => {
                out.push(format!("{prefix}{k} {n}"));
            }
            // sub-values are plain ints (Q13's flat model), so the recursion
            // into `<prefix><k>.` yields exactly this one level
            crate::core::types::AspectVal::Sub(sub) => {
                for (sk, sv) in sub {
                    out.push(format!("{prefix}{k}.{sk} {sv}"));
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
    // §9.35: a step that ends the story closes with its ending line — first
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

/// A lone ref: name / #id / the em dash fallback
/// (param refs are authored-only and never reach a record)
fn ref_name(r: Option<&Ref>) -> String {
    match r {
        Some(Ref::Name { name, .. }) => name.clone(),
        Some(Ref::Id { id }) => format!("#{id}"),
        _ => "—".to_string(),
    }
}

/// Refs inside a list render names / #ids (a param
/// ref yields its name).
fn ref_names(rs: &[Ref]) -> Vec<String> {
    rs.iter()
        .map(|r| match r {
            Ref::Name { name, .. } | Ref::Param { name } => name.clone(),
            Ref::Id { id } => format!("#{id}"),
        })
        .collect()
}

/// The token replaces run in the authored
/// order (actors, object, at, about, to, with); str::replace is a global
/// replace, the exact /g semantics.
fn interpolate_clause(template: &str, clause: &crate::core::sentence::Clause) -> String {
    template
        .replace("{actors}", &join_and(&ref_names(&clause.actors)))
        .replace("{object}", ref_names(&clause.objects).first().map_or("—", String::as_str))
        .replace("{at}", &ref_name(clause.at.as_ref()))
        .replace("{about}", &ref_name(clause.about.as_ref()))
        .replace("{to}", &ref_name(clause.to.as_ref()))
        .replace("{with}", &join_and(&ref_names(&clause.with)))
}

/// The record variant (instruments = with).
fn interpolate_record(template: &str, record: &ActionRecord) -> String {
    template
        .replace("{actors}", &join_and(&ref_names(&record.actors)))
        .replace("{object}", ref_names(&record.objects).first().map_or("—", String::as_str))
        .replace("{at}", &ref_name(record.at.as_ref()))
        .replace("{about}", &ref_name(record.about.as_ref()))
        .replace("{to}", &ref_name(record.to.as_ref()))
        .replace("{with}", &join_and(&ref_names(&record.instruments)))
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
