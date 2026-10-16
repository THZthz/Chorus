//! Tokenizer for the sentence grammar (spec §5.1).
//! Quote-aware and paren-aware: strings and outcome args carry
//! commas/brackets.
//!
//! The five regex-shaped scans (NAME, digits, DUR
//! with its load-bearing `halfhour|h|m|d` alternation order (Q9), the frozen
//! whitespace skip, and `[0-9]`) are HAND-WRITTEN over `Vec<u16>` code units: the
//! regex crate cannot run on a UTF-16 code-unit domain, and code-unit
//! indexing is what makes the error offsets and the `->`/punct decisions
//! byte-identical.

use crate::core::clock::parse_duration;
use crate::core::num_parse::parse_authored_int;
use crate::core::text::is_dsl_ws;
use crate::error::{ChorusError, Result};

/// One lexed token.
#[derive(Debug, Clone, PartialEq)]
pub enum Tok {
    Name { v: String },
    Int { v: i64 },
    Dur { v: i64 }, // minutes
    Str { v: String },
    IdRef { v: String }, // #c7f3 (without '#')
    Punct { v: String },
}

impl Tok {
    /// The token-kind discriminator used by `show` and the error paths.
    #[must_use]
    pub fn t(&self) -> &'static str {
        match self {
            Tok::Name { .. } => "name",
            Tok::Int { .. } => "int",
            Tok::Dur { .. } => "dur",
            Tok::Str { .. } => "str",
            Tok::IdRef { .. } => "idref",
            Tok::Punct { .. } => "punct",
        }
    }

    /// The token's value text for every variant.
    #[must_use]
    pub fn v_str(&self) -> String {
        match self {
            Tok::Name { v } | Tok::Str { v } | Tok::IdRef { v } | Tok::Punct { v } => v.clone(),
            Tok::Int { v } | Tok::Dur { v } => v.to_string(),
        }
    }
}

/// PUNCT. '+'/'-' are punct so a signed SET value
/// (+1/-2, §9.31) lexes as sign + int ('->' is matched before the punct set,
/// so the minus is unambiguous); '.' is punct for dotted SET keys (§5.1
/// dotted names: `state.sealed`, `aspect:language.latin`).
#[must_use]
fn is_punct(c: u16) -> bool {
    // ,  ;  :  (  )  @  =  !  >  <  +  -  .
    matches!(
        c,
        0x2C | 0x3B | 0x3A | 0x28 | 0x29 | 0x40 | 0x3D | 0x21 | 0x3E | 0x3C | 0x2B | 0x2D | 0x2E
    )
}

/// NAME scan (`/^[A-Za-z_][A-Za-z0-9_]*/`) at `s[i..]`:
/// returns the match LENGTH in code units. The class is matched against the
/// CODE UNIT directly — never through `c as u8 as char`, whose truncation
/// would admit any non-ASCII unit whose LOW BYTE lands in the class (U+0161
/// 'š' low byte 'a', U+0430 'а' low byte '0'); the literal class rejects
/// every unit >0x7F.
#[must_use]
fn scan_name(s: &[u16], i: usize) -> Option<usize> {
    let c = *s.get(i)?;
    if !matches!(c, 0x41..=0x5A | 0x61..=0x7A | 0x5F) {
        return None;
    }
    let mut j = i + 1;
    while j < s.len() {
        let c = s[j];
        if matches!(c, 0x41..=0x5A | 0x61..=0x7A | 0x30..=0x39 | 0x5F) {
            j += 1;
        } else {
            break;
        }
    }
    Some(j - i)
}

/// `[0-9]+` scan at `s[i..]`.
#[must_use]
fn scan_digits(s: &[u16], i: usize) -> Option<usize> {
    let mut j = i;
    while j < s.len() && (b'0' as u16..=b'9' as u16).contains(&s[j]) {
        j += 1;
    }
    if j == i {
        None
    } else {
        Some(j - i)
    }
}

/// One DUR unit at `s[j..]`, tried in the alternation order `halfhour|h|m|d`
/// — the ORDER IS LOAD-BEARING (Q9): trying `h`
/// first would split `1halfhour` into dur(1h)+name(alfhour).
#[must_use]
fn scan_dur_unit(s: &[u16], j: usize) -> Option<usize> {
    const HALF: [u16; 8] = [
        b'h' as u16,
        b'a' as u16,
        b'l' as u16,
        b'f' as u16,
        b'h' as u16,
        b'o' as u16,
        b'u' as u16,
        b'r' as u16,
    ];
    if s.len() >= j + 8 && s[j..j + 8] == HALF {
        return Some(8);
    }
    for u in [0x68u16, 0x6D, 0x64] {
        // 'h', 'm', 'd' — only reached when the 8-char "halfhour" missed
        if s.get(j) == Some(&u) {
            return Some(1);
        }
    }
    None
}

/// DUR scan (`/^([0-9]+)(halfhour|h|m|d)(?:[0-9]+(?:halfhour|h|m|d))*/`):
/// returns `(group1_len, total_len)` — the leading digit run and the TOTAL
/// match length of the whole digit+unit run (greedy over the
/// trailing `(?:…)*` groups, so `1h30m` is one match).
#[must_use]
fn scan_dur(s: &[u16], i: usize) -> Option<(usize, usize)> {
    let first_digits = scan_digits(s, i)?;
    let first_unit = scan_dur_unit(s, i + first_digits)?;
    let mut j = i + first_digits + first_unit;
    // greedy tail: `(?:[0-9]+(?:halfhour|h|m|d))*`
    while let Some(digits) = scan_digits(s, j) {
        let Some(unit) = scan_dur_unit(s, j + digits) else {
            break;
        };
        j += digits + unit;
    }
    Some((first_digits, j - i))
}

/// The decoded token text: the scan domain is UTF-16 code units, so a token
/// holding a LONE surrogate cannot become a Rust `String`; lossy conversion
/// (U+FFFD) applies — no test input exercises a lone surrogate.
fn units_to_string(units: &[u16]) -> String {
    String::from_utf16_lossy(units)
}

/// The tokenizer over one sentence's source text.
pub fn lex(src: &str) -> Result<Vec<Tok>> {
    let s: Vec<u16> = src.encode_utf16().collect();
    let mut toks: Vec<Tok> = Vec::new();
    let mut i = 0;
    while i < s.len() {
        let c = s[i];
        if is_dsl_ws(c) {
            // the frozen DSL whitespace set (Q8), hand-scanned
            i += 1;
            continue;
        }
        if c == b'-' as u16 && s.get(i + 1) == Some(&(b'>' as u16)) {
            toks.push(Tok::Punct {
                v: "->".to_string(),
            });
            i += 2;
            continue;
        }
        if c == b'"' as u16 {
            // Real escape semantics (Q10, fixed batch 3): the named
            // single-character escapes are the characters themselves (the
            // frozen set, Q8), an unknown escape keeps its literal
            // next unit (the frozen unknown-escape rule, Q10), and a trailing backslash
            // is unterminated — nothing follows for it to escape.
            let mut j = i + 1;
            let mut out: Vec<u16> = Vec::new();
            while j < s.len() && s[j] != b'"' as u16 {
                if s[j] == b'\\' as u16 {
                    match s.get(j + 1) {
                        Some(&next) => {
                            out.push(match next {
                                0x62 => 0x0008, // \b backspace
                                0x74 => 0x0009, // \t tab
                                0x6E => 0x000A, // \n line feed
                                0x76 => 0x000B, // \v vertical tab
                                0x66 => 0x000C, // \f form feed
                                0x72 => 0x000D, // \r carriage return
                                0x30 => 0x0000, // \0 NUL
                                // \" \' \\ and every non-escape unit: itself
                                _ => next,
                            });
                            j += 2;
                        }
                        None => j = s.len(),
                    }
                } else {
                    out.push(s[j]);
                    j += 1;
                }
            }
            if j >= s.len() {
                return Err(ChorusError::new("unterminated string literal"));
            }
            toks.push(Tok::Str {
                v: units_to_string(&out),
            });
            i = j + 1;
            continue;
        }
        if c == b'#' as u16 {
            // id ref: '#' followed by name characters
            let Some(len) = scan_name(&s, i + 1) else {
                return Err(ChorusError::new(format!(
                    "bad id ref at offset {i} (expected #id)"
                )));
            };
            toks.push(Tok::IdRef {
                v: units_to_string(&s[i + 1..i + 1 + len]),
            });
            i += 1 + len;
            continue;
        }
        if (b'0' as u16..=b'9' as u16).contains(&c) {
            // a duration only when the whole number is
            // the duration's digits ('1halfhour' and compound '1h30m' (§9.45)
            // are one token; '12' alone stays an int)
            let rest = &s[i..];
            let num_len = scan_digits(rest, 0).unwrap_or_else(|| {
                unreachable!("digit branch entered with a digit at i") // guarded by the branch
            });
            if let Some((g1_len, dur_len)) = scan_dur(rest, 0) {
                // the full greedy digit run must equal the DUR's first
                // group's text (both are digit runs from the
                // same start, so text equality is length equality)
                if units_to_string(&rest[..num_len]) == units_to_string(&rest[..g1_len]) {
                    let dur_text = units_to_string(&rest[..dur_len]);
                    toks.push(Tok::Dur {
                        v: parse_duration(&dur_text)?,
                    });
                    i += dur_len;
                    continue;
                }
            }
            let num_text = units_to_string(&rest[..num_len]);
            // authored integers parse DIRECTLY (B-22) — the bytes are the value
            toks.push(Tok::Int {
                v: parse_authored_int(&num_text),
            });
            i += num_len;
            continue;
        }
        if let Some(len) = scan_name(&s, i) {
            toks.push(Tok::Name {
                v: units_to_string(&s[i..i + len]),
            });
            i += len;
            continue;
        }
        if i + 1 < s.len() {
            let two = [s[i], s[i + 1]];
            if two == [b'>' as u16, b'=' as u16] || two == [b'<' as u16, b'=' as u16] {
                toks.push(Tok::Punct {
                    v: units_to_string(&two),
                });
                i += 2;
                continue;
            }
        }
        if is_punct(c) {
            toks.push(Tok::Punct {
                v: units_to_string(&[c]),
            });
            i += 1;
            continue;
        }
        // the character, not the code unit, prints. A lone
        // surrogate cannot print (lossy U+FFFD).
        let ch = char::from_u32(u32::from(c)).unwrap_or(char::REPLACEMENT_CHARACTER);
        return Err(ChorusError::new(format!(
            "unexpected character '{ch}' at offset {i}"
        )));
    }
    Ok(toks)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lexes_the_doc_shapes() {
        let toks = lex("1:2 @ READ TO place, kind:TEXT +1 -> SPAWN X").unwrap();
        assert_eq!(
            toks,
            vec![
                Tok::Int { v: 1 },
                Tok::Punct { v: ":".into() },
                Tok::Int { v: 2 },
                Tok::Punct { v: "@".into() },
                Tok::Name { v: "READ".into() },
                Tok::Name { v: "TO".into() }, // a plain NAME token; the parser/dict decide
                Tok::Name { v: "place".into() },
                Tok::Punct { v: ",".into() },
                Tok::Name { v: "kind".into() },
                Tok::Punct { v: ":".into() },
                Tok::Name { v: "TEXT".into() },
                Tok::Punct { v: "+".into() },
                Tok::Int { v: 1 },
                Tok::Punct { v: "->".into() },
                Tok::Name { v: "SPAWN".into() },
                Tok::Name { v: "X".into() },
            ]
        );
    }

    #[test]
    fn durations_vs_ints_and_the_q9_alternation_order() {
        // '1halfhour' is ONE token — 'h' must not win over 'halfhour' (Q9)
        assert_eq!(lex("1halfhour").unwrap(), vec![Tok::Dur { v: 30 }]);
        assert_eq!(lex("1h").unwrap(), vec![Tok::Dur { v: 60 }]);
        assert_eq!(lex("1h30m").unwrap(), vec![Tok::Dur { v: 90 }]);
        assert_eq!(lex("12").unwrap(), vec![Tok::Int { v: 12 }]);
        // '1hour': DUR_RE matches '1h', then 'our' lexes as a name
        assert_eq!(
            lex("1hour").unwrap(),
            vec![Tok::Dur { v: 60 }, Tok::Name { v: "our".into() }]
        );
        // '12halfhour' — both digits belong to the duration
        assert_eq!(lex("12halfhour").unwrap(), vec![Tok::Dur { v: 12 * 30 }]);
    }

    #[test]
    fn q10_real_escapes_and_unterminated_throws() {
        // real escape semantics (Q10, batch 3): the named escapes are the
        // characters themselves, an unknown escape keeps the literal next
        // unit, and `\"`/`\\` behave exactly as before
        assert_eq!(
            lex(r#""a\nb""#).unwrap(),
            vec![Tok::Str { v: "a\nb".into() }]
        );
        assert_eq!(
            lex(r#""a\tb""#).unwrap(),
            vec![Tok::Str { v: "a\tb".into() }]
        );
        // source `"a\\b"` (two backslashes) holds ONE backslash
        assert_eq!(
            lex(r#""a\\b""#).unwrap(),
            vec![Tok::Str { v: "a\\b".into() }]
        );
        // `\"` is the escaped quote — the bundled content's only escape,
        // byte-identical under the new semantics
        assert_eq!(
            lex("\"a\\\"b\"").unwrap(),
            vec![Tok::Str { v: "a\"b".into() }]
        );
        // a non-escape letter keeps the literal next unit
        // (the frozen unknown-escape rule, Q10)
        assert_eq!(
            lex(r#""a\qb""#).unwrap(),
            vec![Tok::Str { v: "aqb".into() }]
        );
        // unterminated
        let e = lex("\"abc").unwrap_err();
        assert_eq!(e.msg, "unterminated string literal");
        // trailing backslash: still unterminated — nothing follows to escape
        assert_eq!(
            lex("\"abc\\").unwrap_err().msg,
            "unterminated string literal"
        );
        // `\"` does not CLOSE the string either
        assert_eq!(
            lex("\"a\\\"").unwrap_err().msg,
            "unterminated string literal"
        );
    }

    #[test]
    fn offsets_and_bad_tokens_are_byte_exact() {
        // '@' is IN the PUNCT set — a genuinely unexpected char errors
        assert_eq!(lex("@").unwrap(), vec![Tok::Punct { v: "@".into() }]);
        let e = lex("$").unwrap_err();
        assert_eq!(e.msg, "unexpected character '$' at offset 0");
        let e = lex("ab é").unwrap_err();
        assert_eq!(e.msg, "unexpected character 'é' at offset 3");
        let e = lex("x #!").unwrap_err();
        assert_eq!(e.msg, "bad id ref at offset 2 (expected #id)");
    }

    #[test]
    fn non_ascii_code_units_never_enter_name_tokens() {
        // NAME is a LITERAL [A-Za-z0-9_] class — a code
        // unit >0x7F never matches, even when its LOW BYTE does (U+0161 'š'
        // low byte 0x61 'a'; an `as u8` truncation would wrongly absorb it).
        // `lex("abš")` throws `unexpected character 'š' at offset 2`.
        let e = lex("abš").unwrap_err();
        assert_eq!(e.msg, "unexpected character 'š' at offset 2");
        // U+0141 'Ł' (low byte 0x41 'A') rejected at a name START…
        let e = lex("Łat").unwrap_err();
        assert_eq!(e.msg, "unexpected character 'Ł' at offset 0");
        // …and after a name: only "a" is the NAME token, 'Ł' throws
        let e = lex("aŁb").unwrap_err();
        assert_eq!(e.msg, "unexpected character 'Ł' at offset 1");
        // U+0430 'а' (low byte 0x30 '0') rejected as a CONTINUATION char
        let e = lex("x9а").unwrap_err();
        assert_eq!(e.msg, "unexpected character 'а' at offset 2");
        // same class as an id-ref body: #aš is `bad id ref` at the '#'
        let e = lex("a #š").unwrap_err();
        assert_eq!(e.msg, "bad id ref at offset 2 (expected #id)");
    }
}
