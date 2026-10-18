//! Shared CLI styling: the line-level prefix colorer the bins run over
//! their already-rendered stdout text when the user asks for color. The
//! library renderers (`reach::render`, `reach::world_report`, …) stay
//! untouched — their default output is byte-frozen — so coloring happens at
//! the bin layer, AFTER the frozen text exists: each line is matched against
//! the stable prefixes the renderers emit (`  verdict: `, `  path i/n:`,
//! `  note: `, `  bindable via `, the world panel's `  freeze: ` row, the
//! lint's `  - [warning] ` / `  - [reach] ` markers and `<N> file(s), `
//! summary, …) and wrapped in ANSI SGR sequences.
//!
//! The discipline:
//!   - [`Styles::disabled`] (the bins' default) is the IDENTITY transform —
//!     `colorize` returns the input bytes unchanged, which is what keeps the
//!     default faces byte-identical. Color is opt-in (`reach --color`).
//!   - Only crossterm's named foreground colors and the bold / dim / italic
//!     attributes are ever emitted — the named colors carry crossterm's own
//!     fixed 38;5;n encodings (green is `\x1b[38;5;10m`, red `\x1b[38;5;9m`,
//!     …); never a background color, never a raw `AnsiValue` palette pick.
//!     Every run ends with one full reset (`\x1b[0m`), so the styling never
//!     leaks past its line.
//!   - Machine-readable faces (the `--json` envelopes, the `--graph` DOT)
//!     are NEVER colorized — the bins pass them straight through. The
//!     matcher here would leave them untouched anyway (none of their lines
//!     carry a styled prefix), but the bin-level rule is the contract.
//!
//! crossterm rides with `default-features = false`: this module uses the
//! style API only, so the defaults' event/tty machinery (MIO, signal-hook,
//! the windows console backend) never builds — though crossterm's own
//! unconditional dependencies (rustix, parking_lot, libc, …) still ride in
//! Cargo.lock.

use std::fmt::Write as _;

use crossterm::style::{Attribute, Color, SetAttribute, SetForegroundColor};

/// The bins' styling switch: enabled by an explicit `--color` flag, disabled
/// (identity) by default.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Styles {
    enabled: bool,
}

impl Styles {
    /// `--color` given → enabled; anything else → the identity styler.
    #[must_use]
    pub fn new(enabled: bool) -> Self {
        Self { enabled }
    }

    /// The identity styler (what every bin uses when `--color` is absent).
    #[must_use]
    pub fn plain() -> Self {
        Self { enabled: false }
    }

    /// Whether ANSI sequences are emitted at all.
    #[must_use]
    pub fn enabled(self) -> bool {
        self.enabled
    }

    /// The one entry point: recolor a rendered face. Disabled → the input,
    /// byte for byte. Enabled → line-level prefix matching (see the module
    /// docs); lines that match no stable prefix pass through unchanged.
    #[must_use]
    pub fn colorize(&self, text: &str) -> String {
        if !self.enabled {
            return text.to_string();
        }
        let mut out = String::with_capacity(text.len() + 64);
        // split_inclusive keeps every '\n' exactly where it was, so the
        // recolored face differs from the plain one ONLY in the inserted
        // escape sequences (and a missing trailing newline survives).
        for chunk in text.split_inclusive('\n') {
            let (line, nl) = match chunk.strip_suffix('\n') {
                Some(l) => (l, "\n"),
                None => (chunk, ""),
            };
            out.push_str(&self.paint_line(line));
            out.push_str(nl);
        }
        out
    }

    /// One line through the rule table — first match wins. Each rule is a
    /// free function so the unit tests can pin them one by one.
    fn paint_line(&self, line: &str) -> String {
        verdict_line(line)
            .or_else(|| face_head_line(line))
            .or_else(|| world_line(line))
            .or_else(|| card_line(line))
            .or_else(|| lint_line(line))
            .unwrap_or_else(|| line.to_string())
    }
}

/// Wrap `body` in the given foreground color and attributes, then one full
/// reset. The only place ANSI sequences are built.
fn wrap(body: &str, fg: Option<Color>, attrs: &[Attribute]) -> String {
    let mut out = String::new();
    if let Some(c) = fg {
        let _ = write!(out, "{}", SetForegroundColor(c));
    }
    for a in attrs {
        let _ = write!(out, "{}", SetAttribute(*a));
    }
    out.push_str(body);
    let _ = write!(out, "{}", SetAttribute(Attribute::Reset));
    out
}

/// The verdict row of a card report: the verdict word bold, green when the
/// card is obtainable, yellow when merely unproven, red for every negative
/// verdict (unobtainable / unusable — and the defensive NONE / REFUSED
/// spellings). The rest of the row stays plain.
fn verdict_line(line: &str) -> Option<String> {
    let rest = line.strip_prefix("  verdict: ")?;
    let word_end = rest.find(' ').unwrap_or(rest.len());
    let word = &rest[..word_end];
    let fg = match word {
        "OBTAINABLE" => Color::Green,
        "UNPROVEN" => Color::Yellow,
        "UNOBTAINABLE" | "UNUSABLE" | "NONE" | "REFUSED" => Color::Red,
        _ => return None,
    };
    Some(format!("  verdict: {}{}", wrap(word, Some(fg), &[Attribute::Bold]), &rest[word_end..]))
}

/// A face's first line (`scripts/… · card X`, `… · verb W`, `… · world`) and
/// the section headers (`… · card X — why`, `… — cost`): the whole line bold.
fn face_head_line(line: &str) -> Option<String> {
    let is_head = line.contains(" · card ")
        || line.contains(" · verb ")
        || line.ends_with(" · world")
        || line.ends_with(" — why")
        || line.ends_with(" — cost");
    is_head.then(|| wrap(line, None, &[Attribute::Bold]))
}

/// The `--world` dashboard's value rows: the freeze value red when frozen /
/// green when not, the risky-owner row yellow, and the dead / refused
/// counts red whenever they are non-zero.
fn world_line(line: &str) -> Option<String> {
    if let Some(rest) = line.strip_prefix("  freeze: ") {
        let fg = if rest.starts_with("frozen") {
            Color::Red
        } else if rest.starts_with("not frozen") {
            Color::Green
        } else {
            return None;
        };
        return Some(format!("  freeze: {}", wrap(rest, Some(fg), &[])));
    }
    if let Some(pos) = line.find(" · risky: ") {
        let split = pos + " · ".len();
        return Some(format!(
            "{}{}",
            &line[..split],
            wrap(&line[split..], Some(Color::Yellow), &[])
        ));
    }
    if let Some(painted) = refused_overview_line(line) {
        return Some(painted);
    }
    // both section rows render only when their count is non-zero
    // (world_report omits empty sections whole), so presence IS non-zero
    if line == "  dead verbs:" || line == "  refused sites:" {
        return Some(wrap(line, Some(Color::Red), &[]));
    }
    None
}

/// The overview row's refused count (`… live / N refused site(s), …`),
/// colored red when N is non-zero. Zero (the common case) stays plain.
fn refused_overview_line(line: &str) -> Option<String> {
    let rest = line.strip_prefix("  overview: ")?;
    let slash = rest.find(" / ")?;
    let after = &rest[slash + " / ".len()..];
    let digits = after.bytes().take_while(u8::is_ascii_digit).count();
    if digits == 0 || after[..digits].parse::<usize>() == Ok(0) {
        return None;
    }
    let site_end = after.find("site(s)")? + "site(s)".len();
    Some(format!(
        "  overview: {}{}{}",
        &rest[..slash + " / ".len()],
        wrap(&after[..site_end], Some(Color::Red), &[]),
        &after[site_end..]
    ))
}

/// The card report's rows: notes dim + italic, candidate-source lines and
/// the whole path tree (`path i/n:` heads and the `·` steps under them)
/// dim, `bindable via` rows cyan.
fn card_line(line: &str) -> Option<String> {
    if line.contains("  note: ") {
        return Some(wrap(line, None, &[Attribute::Dim, Attribute::Italic]));
    }
    if line.contains("  candidate source: ") {
        return Some(wrap(line, None, &[Attribute::Dim]));
    }
    if line.contains("  bindable via ") {
        return Some(wrap(line, Some(Color::Cyan), &[]));
    }
    if is_path_head(line) || is_path_step(line) {
        return Some(wrap(line, None, &[Attribute::Dim]));
    }
    None
}

/// `  path i/n:` — the head of one proven path.
fn is_path_head(line: &str) -> bool {
    let Some(rest) = line.strip_prefix("  path ") else {
        return false;
    };
    let Some(rest) = rest.strip_suffix(':') else {
        return false;
    };
    let Some((a, b)) = rest.split_once('/') else {
        return false;
    };
    !a.is_empty()
        && !b.is_empty()
        && a.bytes().all(|c| c.is_ascii_digit())
        && b.bytes().all(|c| c.is_ascii_digit())
}

/// A `·` step INSIDE the path tree: the renderers use two-space bullets for
/// every other row (verb slots, world rows, why blockers), while
/// `render_path` indents its steps four spaces or deeper — indent ≥ 4 plus
/// a `· ` head is a path step and only a path step.
fn is_path_step(line: &str) -> bool {
    let trimmed = line.trim_start();
    trimmed.starts_with("· ") && line.len() - trimmed.len() >= 4
}

/// lint-scripts' faces through the same rule table (the bin feeds its
/// already-printed stdout chunks through the one switch exactly like
/// reach): the per-file title, the severity-marked finding rows and the
/// summary line. Tried AFTER the reach face rules — the grammars are
/// disjoint (a title is a column-0 line ending in ':', a finding row starts
/// `  - `, a summary starts `<N> file(s), `), so neither face can shadow
/// the other.
fn lint_line(line: &str) -> Option<String> {
    lint_title_line(line).or_else(|| lint_finding_line(line)).or_else(|| lint_summary_line(line))
}

/// The per-file title (`scripts/…txt:`): the whole line bold. The
/// `…: clean` row and the summary never end in ':' at column 0, and every
/// finding row is indented — a column-0 colon ending IS the title and only
/// the title.
fn lint_title_line(line: &str) -> Option<String> {
    (!line.starts_with(' ') && line.ends_with(':')).then(|| wrap(line, None, &[Attribute::Bold]))
}

/// The finding rows: the bullet + severity marker carries the severity
/// color — the bare bullet red (the ERROR class keeps today's line shape),
/// the `[warning]` marker yellow, the `[reach]` marker magenta — and the
/// message text after it passes through untouched.
fn lint_finding_line(line: &str) -> Option<String> {
    let rest = line.strip_prefix("  - ")?;
    if let Some(msg) = rest.strip_prefix("[warning] ") {
        return Some(format!("  - {} {msg}", wrap("[warning]", Some(Color::Yellow), &[])));
    }
    if let Some(msg) = rest.strip_prefix("[reach] ") {
        return Some(format!("  - {} {msg}", wrap("[reach]", Some(Color::Magenta), &[])));
    }
    Some(format!("{}{rest}", wrap("  - ", Some(Color::Red), &[])))
}

/// The summary line (`N file(s), X finding(s)[, Y warning(s)]
/// [, Z reach finding(s)][ — advice]`, the bin's three shapes): each
/// non-zero count phrase takes its severity color — findings red, warnings
/// yellow, reach findings magenta; the file count and the advice tail stay
/// plain — and an all-zero line renders green whole (the everything-clean
/// face).
fn lint_summary_line(line: &str) -> Option<String> {
    let (n, rest) = take_digits(line)?;
    let rest = rest.strip_prefix(" file(s), ")?;
    let (x, rest) = take_digits(rest)?;
    let mut rest = rest.strip_prefix(" finding(s)")?;
    let (mut y, mut z) = (None, None);
    if let Some(after) = rest.strip_prefix(", ") {
        let (digits, after) = take_digits(after)?;
        y = Some(digits);
        rest = after.strip_prefix(" warning(s)")?;
        if let Some(after) = rest.strip_prefix(", ") {
            let (digits, after) = take_digits(after)?;
            z = Some(digits);
            rest = after.strip_prefix(" reach finding(s)")?;
        }
    }
    // only the bin's three summary shapes carry this grammar; what follows
    // is the end of the line or the em-dash advice
    if !(rest.is_empty() || rest.starts_with(" — ")) {
        return None;
    }
    let zero = |d: &str| d.bytes().all(|b| b == b'0');
    if zero(x) && y.is_none_or(zero) && z.is_none_or(zero) {
        return Some(wrap(line, Some(Color::Green), &[]));
    }
    let phrase = |digits: &str, unit: &str, fg: Color| -> String {
        if zero(digits) {
            format!("{digits}{unit}")
        } else {
            wrap(&format!("{digits}{unit}"), Some(fg), &[])
        }
    };
    let mut out = String::with_capacity(line.len() + 48);
    out.push_str(n);
    out.push_str(" file(s), ");
    out.push_str(&phrase(x, " finding(s)", Color::Red));
    if let Some(yy) = y {
        out.push_str(", ");
        out.push_str(&phrase(yy, " warning(s)", Color::Yellow));
    }
    if let Some(zz) = z {
        out.push_str(", ");
        out.push_str(&phrase(zz, " reach finding(s)", Color::Magenta));
    }
    out.push_str(rest);
    Some(out)
}

/// `<digits>` at the head of `s` — the digits and what follows them.
fn take_digits(s: &str) -> Option<(&str, &str)> {
    let d = s.bytes().take_while(|b| b.is_ascii_digit()).count();
    (d > 0).then(|| (&s[..d], &s[d..]))
}

#[cfg(test)]
mod tests {
    use super::*;

    // the exact sequences crossterm emits for the named colors (its fixed
    // 38;5;n mapping) and the three attributes — pinned here so a crossterm
    // re-encoding shows up as a test diff, not a silent byte drift
    const GREEN: &str = "\x1b[38;5;10m";
    const YELLOW: &str = "\x1b[38;5;11m";
    const RED: &str = "\x1b[38;5;9m";
    const CYAN: &str = "\x1b[38;5;14m";
    const MAGENTA: &str = "\x1b[38;5;13m";
    const BOLD: &str = "\x1b[1m";
    const DIM: &str = "\x1b[2m";
    const ITALIC: &str = "\x1b[3m";
    const RESET: &str = "\x1b[0m";

    #[test]
    fn disabled_is_the_identity_transform() {
        let s = Styles::plain();
        assert!(!s.enabled());
        for text in [
            "",
            "\n",
            "  verdict: OBTAINABLE  (existence: proven)\n",
            "scripts/the-nightjar-flyer.txt · card THE_WAX_CYLINDER\n  \
             verdict: UNPROVEN  (existence: candidate)\n  path 1/2:\n    \
             · SPAWN mints 'X' — recipe r of V\n\n  note: midgame analysis\n",
            "digraph reach {\n  \"A\" -> \"B\" [label=\"mints\"];\n}\n",
            "no trailing newline",
        ] {
            assert_eq!(s.colorize(text), text, "disabled must be identity");
        }
    }

    #[test]
    fn enabled_colors_the_verdict_words() {
        let s = Styles::new(true);
        assert_eq!(
            s.colorize("  verdict: OBTAINABLE  (existence: proven)\n"),
            format!("  verdict: {GREEN}{BOLD}OBTAINABLE{RESET}  (existence: proven)\n")
        );
        assert_eq!(
            s.colorize("  verdict: UNPROVEN  (existence: candidate)\n"),
            format!("  verdict: {YELLOW}{BOLD}UNPROVEN{RESET}  (existence: candidate)\n")
        );
        assert_eq!(
            s.colorize("  verdict: UNOBTAINABLE  (existence: none)\n"),
            format!("  verdict: {RED}{BOLD}UNOBTAINABLE{RESET}  (existence: none)\n")
        );
        assert_eq!(
            s.colorize("  verdict: UNUSABLE  (existence: proven)\n"),
            format!("  verdict: {RED}{BOLD}UNUSABLE{RESET}  (existence: proven)\n")
        );
    }

    #[test]
    fn enabled_bolds_the_face_heads_and_section_words() {
        let s = Styles::new(true);
        let head = "scripts/f.txt · card THE_WAX_CYLINDER  [usable semantics]";
        assert_eq!(s.colorize(head), format!("{BOLD}{head}{RESET}"));
        for h in [
            "scripts/f.txt · card matching 'aspect:winter>=2'",
            "scripts/f.txt · verb WORK",
            "scripts/f.txt · world",
            "scripts/f.txt · card THE_LATE_LORD — why",
            "scripts/f.txt · card THE_LATE_LORD — cost",
        ] {
            assert_eq!(s.colorize(h), format!("{BOLD}{h}{RESET}"), "{h}");
        }
    }

    #[test]
    fn enabled_colors_the_card_rows() {
        let s = Styles::new(true);
        assert_eq!(
            s.colorize("  note: midgame analysis: replayed from a log\n"),
            format!("{DIM}{ITALIC}  note: midgame analysis: replayed from a log{RESET}\n")
        );
        assert_eq!(
            s.colorize("  candidate source: SPAWN 'X' — guarded (recipe `when` guard)\n"),
            format!("{DIM}  candidate source: SPAWN 'X' — guarded (recipe `when` guard){RESET}\n")
        );
        assert_eq!(
            s.colorize("  bindable via WORK (object ×1)\n"),
            format!("{CYAN}  bindable via WORK (object ×1){RESET}\n")
        );
        // the path head and its `·` steps dim; a two-space bullet elsewhere
        // (verb slots, why rows) is NOT a path step and stays plain
        assert_eq!(s.colorize("  path 2/3:"), format!("{DIM}  path 2/3:{RESET}"));
        let step = "    · SPAWN mints 'X' — recipe r0 of WORK";
        assert_eq!(s.colorize(step), format!("{DIM}{step}{RESET}"));
        let deep = "        · WORK committable: actors ×1 ← initial entity ANA";
        assert_eq!(s.colorize(deep), format!("{DIM}{deep}{RESET}"));
        let verb_row = "  · actors ×1 — kind:character";
        assert_eq!(s.colorize(verb_row), verb_row);
        let guard = "    guard: count(name:X) >= 1";
        assert_eq!(s.colorize(guard), guard);
    }

    #[test]
    fn enabled_colors_the_world_rows() {
        let s = Styles::new(true);
        assert_eq!(
            s.colorize("  freeze: frozen — first risky crossing in 90 min"),
            format!("  freeze: {RED}frozen — first risky crossing in 90 min{RESET}")
        );
        assert_eq!(
            s.colorize("  freeze: not frozen"),
            format!("  freeze: {GREEN}not frozen{RESET}")
        );
        let risky = "  · risky: WORK, DREAM";
        assert_eq!(s.colorize(risky), format!("  · {YELLOW}risky: WORK, DREAM{RESET}"));
        // zero refused stays plain, non-zero turns red
        let zero = "  overview: 7 live / 0 refused site(s), 3 verb(s), 5 prototype(s), 1 deck(s)";
        assert_eq!(s.colorize(zero), zero);
        let some = "  overview: 7 live / 2 refused site(s), 3 verb(s), 5 prototype(s), 1 deck(s)";
        assert_eq!(
            s.colorize(some),
            format!("  overview: 7 live / {RED}2 refused site(s){RESET}, 3 verb(s), 5 prototype(s), 1 deck(s)")
        );
        assert_eq!(s.colorize("  dead verbs:"), format!("{RED}  dead verbs:{RESET}"));
        assert_eq!(s.colorize("  refused sites:"), format!("{RED}  refused sites:{RESET}"));
        // a dead-verb ROW is a two-space bullet — plain
        let row = "  · MANSUS — role at can never fill: kind:place";
        assert_eq!(s.colorize(row), row);
    }

    #[test]
    fn enabled_colors_the_lint_faces() {
        let s = Styles::new(true);
        // the per-file title bold; the `…: clean` row and blank lines plain
        let title = "scripts/some-script.txt:";
        assert_eq!(s.colorize(title), format!("{BOLD}{title}{RESET}"));
        assert_eq!(s.colorize("scripts/f.txt: clean"), "scripts/f.txt: clean");
        assert_eq!(s.colorize(""), "");
        // finding rows: the bullet + severity marker carries the severity
        // color, the message text passes through untouched
        let msg = "recipe r of WORK: unreachable — work.m already matches everything (§6.4)";
        assert_eq!(s.colorize(&format!("  - {msg}")), format!("{RED}  - {RESET}{msg}"));
        assert_eq!(
            s.colorize(&format!("  - [warning] {msg}")),
            format!("  - {YELLOW}[warning]{RESET} {msg}")
        );
        assert_eq!(
            s.colorize(&format!("  - [reach] {msg}")),
            format!("  - {MAGENTA}[reach]{RESET} {msg}")
        );
    }

    #[test]
    fn enabled_colors_the_lint_summary_counts() {
        let s = Styles::new(true);
        // the everything-clean face: green whole
        assert_eq!(
            s.colorize("1 file(s), 0 finding(s)"),
            format!("{GREEN}1 file(s), 0 finding(s){RESET}")
        );
        // errors only: the finding count red, the file count plain
        let two = "1 file(s), 2 finding(s)";
        assert_eq!(s.colorize(two), format!("1 file(s), {RED}2 finding(s){RESET}"));
        // the warning tail: red findings, yellow warnings, plain advice
        let full =
            "3 file(s), 76 finding(s), 76 warning(s) — warnings advise, the exit code stays 0";
        assert_eq!(
            s.colorize(full),
            format!(
                "3 file(s), {RED}76 finding(s){RESET}, {YELLOW}76 warning(s){RESET} — warnings \
                 advise, the exit code stays 0"
            )
        );
        // the bridge tail: a zero count phrase stays plain, the reach count
        // magenta
        let bridged = "1 file(s), 2 finding(s), 0 warning(s), 7 reach finding(s) — warnings and \
                       reach findings advise, the exit code stays 0";
        assert_eq!(
            s.colorize(bridged),
            format!(
                "1 file(s), {RED}2 finding(s){RESET}, 0 warning(s), {MAGENTA}7 reach \
                 finding(s){RESET} — warnings and reach findings advise, the exit code stays 0"
            )
        );
    }

    #[test]
    fn the_lint_face_differs_only_in_escapes() {
        let face = "scripts/f.txt:\n  - a hard error\n  - [warning] dead when\n\n1 file(s), 1 \
                    finding(s), 1 warning(s) — warnings advise, the exit code stays 0\n";
        let on = Styles::new(true).colorize(face);
        let off = Styles::plain().colorize(face);
        assert_eq!(off, face);
        assert!(on.contains("\x1b["), "the colored face carries escapes");
        assert_eq!(strip_ansi(&on), off, "removing escapes restores the plain face");
    }

    #[test]
    fn unknown_lines_pass_through_even_when_enabled() {
        let s = Styles::new(true);
        // the graph/json discipline lives at the bin layer, but none of
        // their lines carries a styled prefix either
        for text in [
            "digraph reach {\n  rankdir=LR;\n}\n",
            "{\n  \"file\": \"f.txt\",\n  \"report\": {}\n}\n",
            "  duration: 30 min\n  committable: yes\n",
            "",
        ] {
            assert_eq!(s.colorize(text), text);
        }
    }

    #[test]
    fn enabled_and_disabled_differ_only_in_escapes() {
        let face = "scripts/f.txt · card X\n  verdict: OBTAINABLE  (existence: proven)\n  \
                    path 1/1:\n    · initial 'X' — declared in the initial entities\n\n  \
                    bindable via WORK (object ×1)\n  note: a note\n";
        let on = Styles::new(true).colorize(face);
        let off = Styles::plain().colorize(face);
        assert_eq!(off, face);
        assert!(on.contains("\x1b["), "the colored face carries escapes");
        assert_eq!(strip_ansi(&on), off, "removing escapes restores the plain face");
    }

    /// A tiny SGR stripper for the equality above (test-only).
    fn strip_ansi(text: &str) -> String {
        let mut out = String::new();
        let mut chars = text.chars().peekable();
        while let Some(c) = chars.next() {
            if c == '\x1b' && chars.peek() == Some(&'[') {
                chars.next();
                for c in chars.by_ref() {
                    if c.is_ascii_alphabetic() {
                        break;
                    }
                }
            } else {
                out.push(c);
            }
        }
        out
    }
}
