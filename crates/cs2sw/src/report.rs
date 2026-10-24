//! The machine-readable gap report: per named gap — the CS construct, the
//! affected ids (bounded sample plus count), the classification, and the
//! one-line disposition.

use std::fmt::Write as _;

use crate::convert::{Class, Conversion, Stats};

/// Render the full report as a markdown table document.
pub fn render_markdown(c: &Conversion) -> String {
    let mut out = String::new();
    out.push_str("# cs2sw conversion report\n\n");
    let s: &Stats = &c.stats;
    out.push_str("## Conversion counts\n\n");
    out.push_str("| direction | count |\n|---|---|\n");
    let _ = write!(out, "| elements in | {} |\n", s.elements_in);
    let _ = write!(out, "| aspect elements (no card) | {} |\n", s.aspect_elements);
    let _ = write!(out, "| cards out | {} |\n", s.cards_out);
    let _ = write!(out, "| aspect registry keys | {} |\n", s.aspect_keys);
    let _ = write!(out, "| recipes in | {} |\n", s.recipes_in);
    let _ = write!(out, "| recipes out | {} |\n", s.recipes_out);
    let _ = write!(out, "| slots out | {} |\n", s.slots_out);
    let _ = write!(out, "| verbs out | {} |\n", s.verbs_out);
    let _ = write!(out, "| decks out | {} |\n", s.decks_out);
    let _ = write!(out, "| endings out | {} |\n", s.endings_out);
    let _ = write!(out, "| output lines | {} |\n", s.output_lines);

    out.push_str("\n## Dead census\n\n");
    out.push_str("Emitted recipes on uncommittable, non-ambient verbs that no route reaches: no incoming `start` and no adoption as a `continue` target. Player-verb recipes commit, ambient-verb recipes ride the heartbeat.\n\n");
    out.push_str("| census | count |\n|---|---|\n");
    let _ = write!(out, "| dead recipes | {} |\n", s.dead_recipes);
    let _ = write!(
        out,
        "| dead recipes referenced in CS (every reference dropped or gated off) | {} |\n",
        s.dead_recipes_referenced
    );
    let _ = write!(out, "| dead verbs (every recipe dead) | {} |\n", s.dead_verbs);
    if !c.dead_verb_detail.is_empty() {
        let list: Vec<String> = c
            .dead_verb_detail
            .iter()
            .map(|(v, n)| format!("{v} ({n} recipes)"))
            .collect();
        let _ = write!(out, "| dead verbs, individually | {} |\n", list.join(", "));
    }

    out.push_str("\n## Named gaps\n\n");
    out.push_str("| construct | affected ids | classification | disposition |\n");
    out.push_str("|---|---|---|---|\n");
    for row in &c.gaps.rows {
        let mut distinct: Vec<String> = row.ids.clone();
        distinct.sort();
        distinct.dedup();
        let sample: Vec<String> = distinct.iter().take(8).cloned().collect();
        let ids = if row.ids.len() > distinct.len() {
            format!(
                "{} ({} distinct / {} hits)",
                sample.join(", "),
                distinct.len(),
                row.ids.len()
            )
        } else if distinct.len() > sample.len() {
            format!("{} ({} of {})", sample.join(", "), sample.len(), distinct.len())
        } else {
            sample.join(", ")
        };
        let ids = if ids.is_empty() { String::from("-") } else { ids };
        let _ = write!(
            out,
            "| {} | {} | {} | {} |\n",
            row.construct,
            ids,
            row.classification.label(),
            row.note
        );
    }
    if c.gaps.rows.is_empty() {
        out.push_str("| none | - | - | - |\n");
    }

    out.push_str("\n## Load exclusions\n\n");
    out.push_str("| kind | name | reason |\n|---|---|---|\n");
    let mut reasons: std::collections::BTreeMap<(String, String), usize> = Default::default();
    for (kind, _, reason) in &c.skipped {
        *reasons.entry((kind.clone(), reason.clone())).or_default() += 1;
    }
    for ((kind, reason), n) in reasons {
        let _ = write!(out, "| {kind} | {n} item(s) | {reason} |\n");
    }

    if !c.collisions.is_empty() {
        out.push_str("\n## Naming collisions\n\n");
        out.push_str("| CS id | assigned name |\n|---|---|\n");
        for (raw, name) in c.collisions.iter().take(50) {
            let _ = write!(out, "| {raw} | {name} |\n");
        }
        if c.collisions.len() > 50 {
            let _ = write!(out, "| ... | {} more |\n", c.collisions.len() - 50);
        }
    }
    out
}

/// Render the report as JSON (machine-readable form).
pub fn render_json(c: &Conversion) -> String {
    let mut out = String::from("{\n  \"gaps\": [\n");
    for (i, row) in c.gaps.rows.iter().enumerate() {
        let class = match row.classification {
            Class::ExtensionSeam => "extension-seam",
            Class::NonGoal => "non-goal",
            Class::ConverterIdiom => "converter-idiom",
        };
        let _ = write!(out, "    {{\n      \"construct\": \"{}\",\n", escape(&row.construct));
        let mut distinct = row.ids.clone();
        distinct.sort();
        distinct.dedup();
        let _ = write!(out, "      \"affected_ids\": {},\n", distinct.len());
        let _ = write!(out, "      \"classification\": \"{class}\",\n");
        let _ = write!(out, "      \"disposition\": \"{}\",\n", escape(&row.note));
        let sample: Vec<String> = row.ids.iter().take(8).map(|x| format!("\"{}\"", escape(x))).collect();
        let _ = write!(out, "      \"ids_sample\": [{}]\n    }}{}\n", sample.join(", "), if i + 1 < c.gaps.rows.len() { "," } else { "" });
    }
    out.push_str("  ]\n}\n");
    out
}

fn escape(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"")
}
