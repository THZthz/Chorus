//! Recursive-descent sentence parser
//! (spec §5.1). The one semantic decision point: where the actor list ends
//! and the verb begins. That needs the verb registry (both are uppercase
//! tokens), so the parser consults the Dictionary (spec §5.1).
//!
//! This module owns the entry points (`parse_sentence` / `parse_outcomes` /
//! `parse_function_body`), the [`Parser`] token primitives, and the
//! sentence-side grammar: clock prefix, kind dispatch (EVENT / COMPLETE /
//! WAIT / action), clause, and ref. The outcome-side grammar — the op
//! dispatch, IF/FN calls, ROLL, RUN, SPAWN, and the generic arg list — is a
//! second `impl Parser` block in [`outcome`]. Parse and fn_expand share the
//! `Outcome`/`Arg` sentence types by design (there are no separate
//! template/concrete types), which is what lets the parser dispatch §10
//! calls directly; the expansion itself lives in
//! [`crate::core::fn_expand`].

use std::collections::HashSet;

use crate::core::clock::Clock;
use crate::core::dict::Dictionary;
use crate::core::fn_expand::FnTable;
use crate::core::lexer::{lex, Tok};
use crate::core::r#ref::Ref;
use crate::core::sentence::{Clause, Outcome, Sentence, SentenceKind};
use crate::error::{ChorusError, Result};

mod outcome;

/// Clause keywords that end the actor/ref scan.
const CLAUSE_KEYWORDS: [&str; 4] = ["TO", "WITH", "AT", "ABOUT"];

/// Parse one full sentence.
pub fn parse_sentence(src: &str, dict: &Dictionary) -> Result<Sentence> {
    Parser::new(lex(src)?, dict, None, None, false).sentence()
}

/// Parse an outcome list for recipe `out`
/// strings at content load: authored outcome strings must parse COMPLETELY
/// (§5.1). Calls to declared functions AND script ops (§10) both
/// dispatch.
pub fn parse_outcomes(src: &str, dict: &Dictionary, fns: Option<&FnTable>) -> Result<Vec<Outcome>> {
    let mut p = Parser::new(lex(src)?, dict, fns, None, true);
    let out = p.outcome_list_or_empty()?;
    p.expect_end()?;
    Ok(out)
}

/// Parse one function BODY into its template
/// (§10): parameters are active, nested calls become FN nodes, NOTHING
/// expands yet. A function body may NOT call script ops (§10) —
/// functions parse before the ops bodies exist, so an op call here is the
/// specific ban error, not a generic unknown-call.
pub fn parse_function_body(
    src: &str,
    dict: &Dictionary,
    fns: &FnTable,
    params: &[String],
) -> Result<Vec<Outcome>> {
    let mut p =
        Parser::new(lex(src)?, dict, Some(fns), Some(params.iter().cloned().collect()), false);
    let out = p.outcome_list_or_empty()?;
    p.expect_end()?;
    Ok(out)
}

/// Parse one OP BODY into its template (§10): the same grammar as a
/// function body, but op calls are LEGAL here — an op body may call other
/// ops and functions alike (a function may never call an op).
pub fn parse_op_body(
    src: &str,
    dict: &Dictionary,
    fns: &FnTable,
    params: &[String],
) -> Result<Vec<Outcome>> {
    let mut p =
        Parser::new(lex(src)?, dict, Some(fns), Some(params.iter().cloned().collect()), true);
    let out = p.outcome_list_or_empty()?;
    p.expect_end()?;
    Ok(out)
}

/// The recursive-descent parser over one sentence's tokens.
struct Parser<'a> {
    toks: Vec<Tok>,
    pos: usize,
    dict: &'a Dictionary,
    fns: Option<&'a FnTable>,
    params: Option<HashSet<String>>,
    /// May a `NAME(` dispatch to a DECLARED SCRIPT OP? True for outcome
    /// lists and op bodies; false inside a function body, where an op call
    /// is the §10 composition ban (a specific load error). Built-in ops
    /// dispatch regardless — they are grammar, not table entries.
    ops_call: bool,
}

impl<'a> Parser<'a> {
    fn new(
        toks: Vec<Tok>,
        dict: &'a Dictionary,
        fns: Option<&'a FnTable>,
        params: Option<HashSet<String>>,
        ops_call: bool,
    ) -> Parser<'a> {
        Parser { toks, pos: 0, dict, fns, params, ops_call }
    }

    fn peek(&self, o: usize) -> Option<&Tok> {
        self.toks.get(self.pos + o)
    }

    fn next(&mut self) -> Result<Tok> {
        match self.peek(0) {
            Some(t) => {
                let t = t.clone();
                self.pos += 1;
                Ok(t)
            }
            None => Err(ChorusError::new("unexpected end of sentence")),
        }
    }

    fn expect_punct(&mut self, v: &str) -> Result<()> {
        let t = self.next()?;
        if t.t() != "punct" || t.v_str() != v {
            return Err(ChorusError::new(format!("expected '{v}', got '{}'", show(&t))));
        }
        Ok(())
    }

    fn at_punct(&self, v: &str, o: usize) -> bool {
        match self.peek(o) {
            Some(t) => t.t() == "punct" && t.v_str() == v,
            None => false,
        }
    }

    fn at_keyword(&self, w: &str) -> bool {
        match self.peek(0) {
            Some(t) => t.t() == "name" && t.v_str() == w,
            None => false,
        }
    }

    /// Is `name` a §10 parameter of the function body being parsed? `None`
    /// params (a top-level sentence) has no scope at all.
    fn is_param(&self, name: &str) -> bool {
        self.params.as_ref().is_some_and(|p| p.contains(name))
    }

    /// The parameter's name when the token at offset `o` is a bare §10
    /// parameter.
    fn param_at(&self, o: usize) -> Option<String> {
        match self.peek(o) {
            Some(Tok::Name { v }) if self.is_param(v) => Some(v.clone()),
            _ => None,
        }
    }

    /// The pre-verbal clock prefix `day:halfhour @` (§5.1), or `None`.
    ///
    /// (clippy's `manual_range_contains` lint is silenced — the explicit
    /// bound check is deliberate)
    #[allow(clippy::manual_range_contains)]
    fn clock_prefix(&mut self) -> Result<Option<Clock>> {
        if !(self.at_punct(":", 1) && matches!(self.peek(0), Some(Tok::Int { .. }))) {
            return Ok(None);
        }
        let day_tok = self.next()?;
        let Tok::Int { v: day } = day_tok else {
            return Err(ChorusError::new("bad clock"));
        };
        self.expect_punct(":")?;
        let halfhour_tok = self.next()?;
        // the 47 bound written HALFHOURS_PER_DAY - 1 (same value,
        // self-documenting)
        let Tok::Int { v: halfhour } = halfhour_tok else {
            return Err(ChorusError::new("bad halfhour (0..47)"));
        };
        if halfhour < 0 || halfhour > crate::core::clock::HALFHOURS_PER_DAY - 1 {
            return Err(ChorusError::new("bad halfhour (0..47)"));
        }
        let clock = Some(Clock { day, halfhour });
        self.expect_punct("@")?;
        Ok(clock)
    }

    /// The `-> outcomes` tail, empty when no arrow follows.
    fn outcomes_arrow(&mut self) -> Result<Vec<Outcome>> {
        if self.at_punct("->", 0) {
            self.next()?;
            self.outcome_list()
        } else {
            Ok(Vec::new())
        }
    }

    fn sentence(&mut self) -> Result<Sentence> {
        let clock = self.clock_prefix()?;

        if self.at_keyword("EVENT") {
            self.next()?;
            // the trigger id: a name, or a quoted string for an unsafe
            // spelling. Colon-joined ids reassemble segment by segment — the
            // built-in expiry path writes `expire:<NAME>` (§6.6)
            let trig = self.next()?;
            if trig.t() != "name" && trig.t() != "str" {
                return Err(ChorusError::new("expected event trigger name"));
            }
            let mut trigger = trig.v_str();
            loop {
                if !self.at_punct(":", 0) {
                    break;
                }
                self.next()?;
                let seg = match self.peek(0) {
                    Some(t) if t.t() == "name" || t.t() == "str" => t.clone(),
                    _ => {
                        return Err(ChorusError::new(
                            "expected a name after ':' in an event trigger id",
                        ));
                    }
                };
                self.next()?;
                trigger.push(':');
                trigger.push_str(&seg.v_str());
            }
            let mut s = Sentence::bare(SentenceKind::Event, clock);
            s.trigger = Some(trigger);
            s.outcomes = self.outcomes_arrow()?;
            self.expect_end()?;
            return Ok(s);
        }

        // COMPLETE rows (v0.5): the verb is a RAW name token, never ref():
        // verb lexemes are reserved and ref() would throw. The engine writes
        // these; they parse so every concrete row round-trips (law 2).
        if self.at_keyword("COMPLETE") {
            self.next()?;
            let verb_tok = self.next()?;
            if verb_tok.t() != "name" {
                return Err(ChorusError::new("expected a verb name after COMPLETE"));
            }
            let mut s = Sentence::bare(SentenceKind::Complete, clock);
            s.trigger = Some(verb_tok.v_str());
            s.outcomes = self.outcomes_arrow()?;
            self.expect_end()?;
            return Ok(s);
        }

        // WAIT rows (v0.5): WAIT is reserved (a core verb), so this must sit
        // before the action fallthrough, whose ref() would refuse the token.
        if self.at_keyword("WAIT") && matches!(self.peek(1), Some(Tok::Dur { .. })) {
            self.next()?;
            let dur_tok = self.next()?;
            let Tok::Dur { v } = dur_tok else {
                return Err(ChorusError::new("expected a duration after WAIT"));
            };
            self.expect_end()?;
            let mut s = Sentence::bare(SentenceKind::Wait, clock);
            s.span_min = Some(v);
            return Ok(s);
        }

        let mut clauses = vec![self.clause()?];
        while self.at_punct(";", 0) {
            self.next()?;
            clauses.push(self.clause()?);
        }
        let outcomes = self.outcomes_arrow()?;
        self.expect_end()?;
        let mut s = Sentence::bare(SentenceKind::Action, clock);
        s.clauses = clauses;
        s.outcomes = outcomes;
        Ok(s)
    }

    /// Error unless the token stream is fully consumed.
    fn expect_end(&mut self) -> Result<()> {
        if let Some(t) = self.peek(0) {
            return Err(ChorusError::new(format!("unexpected trailing token '{}'", show(t))));
        }
        Ok(())
    }

    /// clause = ref ("," ref)* [AT ref] VERB [TO ref] [objects] [WITH refs]
    /// [ABOUT ref] [AT ref].
    fn clause(&mut self) -> Result<Clause> {
        let mut actors = vec![self.ref_()?];
        while self.at_punct(",", 0) {
            self.next()?;
            actors.push(self.ref_()?);
        }

        let mut at: Option<Ref> = None;
        if self.at_keyword("AT") {
            self.next()?;
            at = Some(self.ref_()?);
        }

        let verb_tok = self.next()?;
        if verb_tok.t() != "name" || !self.dict.is_verb(&verb_tok.v_str()) {
            return Err(ChorusError::new(format!(
                "expected a registered verb, got '{}'",
                show(&verb_tok)
            )));
        }
        let mut clause = Clause {
            actors,
            at,
            verb: verb_tok.v_str(),
            to: None,
            objects: Vec::new(),
            with: Vec::new(),
            about: None,
        };

        if self.at_keyword("TO") {
            self.next()?;
            clause.to = Some(self.ref_()?);
        }

        if self.starts_ref() {
            clause.objects.push(self.ref_()?);
            while self.at_punct(",", 0) {
                self.next()?;
                clause.objects.push(self.ref_()?);
            }
        }

        if self.at_keyword("WITH") {
            self.next()?;
            clause.with.push(self.ref_()?);
            while self.at_punct(",", 0) {
                self.next()?;
                clause.with.push(self.ref_()?);
            }
        }
        if self.at_keyword("ABOUT") {
            self.next()?;
            clause.about = Some(self.ref_()?);
        }

        // post-verbal AT is accepted as input sugar, canonicalized
        // pre-verbal (§5.1)
        if self.at_keyword("AT") {
            self.next()?;
            let place = self.ref_()?;
            if clause.at.is_some() {
                return Err(ChorusError::new("AT given twice in one clause"));
            }
            clause.at = Some(place);
        }
        Ok(clause)
    }

    /// Does the next token begin a ref? (name tokens excluded when
    /// verb/keyword/EVENT.)
    fn starts_ref(&self) -> bool {
        let Some(t) = self.peek(0) else {
            return false;
        };
        if t.t() == "str" || t.t() == "idref" {
            return true;
        }
        if t.t() != "name" {
            return false;
        }
        let v = t.v_str();
        !self.dict.is_verb(&v) && !CLAUSE_KEYWORDS.contains(&v.as_str()) && v != "EVENT"
    }

    /// ref = name | kind ":" name | "#" id | string
    /// (§5.1). Names in the dictionary's reserved set (keywords AND verb
    /// lexemes) must be quoted. §10: inside a function body a bare
    /// parameter is a ref-position placeholder (a param shadows any
    /// entity/role name); a parameter after `kind:` is refused — params
    /// substitute WHOLE refs.
    fn ref_(&mut self) -> Result<Ref> {
        let t = self.next()?;
        let Tok::Name { v } = t else {
            // idref and str return directly; anything
            // but a name is the `expected a ref` error
            return match t {
                Tok::IdRef { v } => Ok(Ref::Id { id: v }),
                Tok::Str { v } => Ok(Ref::Name { kind: None, name: v }),
                other => Err(ChorusError::new(format!("expected a ref, got '{}'", show(&other)))),
            };
        };
        if self.is_param(&v) {
            return Ok(Ref::Param { name: v });
        }
        if self.dict.reserved.contains(&v) {
            return Err(ChorusError::new(format!(
                "'{v}' is reserved and cannot be a name (quote it)"
            )));
        }
        if self.at_punct(":", 0) {
            self.next()?;
            let n = self.next()?;
            let Tok::Name { v: nv } = n else {
                return Err(ChorusError::new(format!(
                    "expected name after kind ':', got '{}'",
                    show(&n)
                )));
            };
            if self.is_param(&nv) {
                return Err(ChorusError::new(
                    "a parameter cannot follow 'kind:' — bind the whole ref, not the name part",
                ));
            }
            if self.dict.reserved.contains(&nv) {
                return Err(ChorusError::new(format!(
                    "'{nv}' is reserved and cannot be a name (quote it)"
                )));
            }
            return Ok(Ref::Name { kind: Some(v), name: nv });
        }
        Ok(Ref::Name { kind: None, name: v })
    }
}

/// `show(t)`: strings print re-quoted (RAW quotes, no
/// escaping), everything else prints its v.
#[must_use]
fn show(t: &Tok) -> String {
    if t.t() == "str" {
        format!("\"{}\"", t.v_str())
    } else {
        t.v_str()
    }
}

/// The Q3 teach error: a parameter used as a `key=value` pair key or value
/// in generic outcome args. §10 pins the pair face on spawn_args (the
/// whole pair is replaced by the parameter); any other op's args strand
/// tokens there, so the error names the param, the forms a parameter MAY
/// take in outcome args, and every param in scope (sorted — the set's
/// iteration order is random). Acceptance is unchanged: the spelling was
/// and remains a parse error.
fn param_pair_error(side: &str, name: &str, params: Option<&HashSet<String>>) -> String {
    let mut names: Vec<&str> =
        params.map(|p| p.iter().map(String::as_str).collect()).unwrap_or_default();
    names.sort_unstable();
    let list = if names.is_empty() { "(none)".to_string() } else { names.join(", ") };
    format!(
        "param '{name}' can't be a `key=value` pair {side} in outcome args — a parameter \
         stands as its own arg (SET(x, {name})), as a signed delta (+{name} / -{name}), or \
         in a ref/INTO position; only a SPAWN pair takes a param as key or value (the whole \
         pair is replaced by the parameter); params in scope: {list}"
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::dict::core_dictionary;
    use crate::core::fn_expand::FnTable;
    use crate::core::sentence::{Arg, Sign};

    fn dict() -> Dictionary {
        core_dictionary()
    }

    #[test]
    fn q3_param_pair_key_is_a_teach_error_and_spawn_still_accepts_one() {
        let d = dict();
        let mut params = HashSet::new();
        params.insert("myparam".to_string());
        // SET(myparam = 3) inside a body: the param reads as a pair KEY —
        // rejected as before (acceptance face unchanged), but the error is
        // the Q3 teach message, not the loop's bare `expected ')', got '='`
        let mut p =
            Parser::new(lex("SET(myparam = 3)").unwrap(), &d, None, Some(params.clone()), true);
        let err = p.outcome_list_or_empty().unwrap_err();
        assert_eq!(
            err.msg,
            "param 'myparam' can't be a `key=value` pair key in outcome args — a parameter \
             stands as its own arg (SET(x, myparam)), as a signed delta (+myparam / -myparam), \
             or in a ref/INTO position; only a SPAWN pair takes a param as key or value (the whole \
             pair is replaced by the parameter); params in scope: myparam"
        );
        // SPAWN's own arg parse DOES accept the param key (the whole pair becomes
        // the param, and the value token is consumed) — the §10 pair face,
        // untouched by the teach
        let mut p2 = Parser::new(lex("SPAWN X myparam = 3").unwrap(), &d, None, Some(params), true);
        let out = p2.outcome_list_or_empty().unwrap();
        match &out[0].args[1] {
            Arg::Param { name, sign } => {
                assert_eq!(name, "myparam");
                assert_eq!(*sign, None);
            }
            other => panic!("expected param, got {other:?}"),
        }
        assert_eq!(out[0].args.len(), 2); // the pair was replaced wholesale
    }

    #[test]
    fn q3_param_pair_value_is_a_teach_error_listing_params_sorted() {
        let d = dict();
        let params: HashSet<String> = ["p", "aa", "b"].iter().map(|s| s.to_string()).collect();
        // A param as the pair's VALUE: rejected as before (acceptance face
        // unchanged), but the error is the Q3 teach message, not the loop's
        // bare `expected ')', got 'p'` — and it lists the scope, sorted
        let mut p = Parser::new(lex("SET(x, k = p)").unwrap(), &d, None, Some(params), true);
        let err = p.outcome_list_or_empty().unwrap_err();
        assert_eq!(
            err.msg,
            "param 'p' can't be a `key=value` pair value in outcome args — a parameter \
             stands as its own arg (SET(x, p)), as a signed delta (+p / -p), or in a \
             ref/INTO position; only a SPAWN pair takes a param as key or value (the whole \
             pair is replaced by the parameter); params in scope: aa, b, p"
        );
    }

    #[test]
    fn bare_params_roles_and_deltas_parse_in_arg_position() {
        let d = dict();
        let mut params = HashSet::new();
        params.insert("p".to_string());
        let mut p = Parser::new(
            lex("SET(x, p, +1, -2, aspect:dread, state.sealed, true, 1h)").unwrap(),
            &d,
            None,
            Some(params),
            true,
        );
        let out = p.outcome_list_or_empty().unwrap();
        let args = &out[0].args;
        assert_eq!(args[1], Arg::Param { name: "p".into(), sign: None });
        assert_eq!(args[2], Arg::Delta { v: 1 });
        assert_eq!(args[3], Arg::Delta { v: -2 });
        assert_eq!(args[4], Arg::Key { v: "aspect:dread".into() });
        assert_eq!(args[5], Arg::Key { v: "state.sealed".into() });
        assert_eq!(args[6], Arg::Bool { v: true });
        assert_eq!(args[7], Arg::Dur { v: 60 });
    }

    #[test]
    fn if_else_chains_and_rolled_drawn_round_trip() {
        let d = dict();
        let mut p = Parser::new(
            lex("ANA READ book -> IF count(actors) >= 2 -> ( NARRATE \"big\" ) ELSE -> ( NARRATE \"small\" )").unwrap(),
            &d,
            None,
            None,
            true,
        );
        let s = p.sentence().unwrap();
        let o = &s.outcomes[0];
        assert_eq!(o.op, "IF");
        assert_eq!(o.arms.as_ref().unwrap().len(), 1);
        assert!(o.fallback.is_some());
        // engine-written rows re-parse (law 2)
        let mut p2 =
            Parser::new(lex("1:2 @ COMPLETE READ -> ROLLED(14, 3)").unwrap(), &d, None, None, true);
        let s2 = p2.sentence().unwrap();
        assert_eq!(s2.kind, SentenceKind::Complete);
        assert_eq!(s2.outcomes[0].op, "ROLLED");
        assert_eq!(s2.outcomes[0].args[0], Arg::Int { v: 14 });
        assert_eq!(s2.outcomes[0].args[1], Arg::Int { v: 3 });
        // ROLLED arity is pinned
        let mut p3 =
            Parser::new(lex("1:2 @ COMPLETE READ -> ROLLED(14)").unwrap(), &d, None, None, true);
        let err = p3.sentence().unwrap_err();
        assert_eq!(err.msg, "ROLLED takes exactly two integers: ROLLED(<total>, <ctrAfter>)");
    }

    #[test]
    fn fn_call_arity_error_is_byte_exact() {
        let d = dict();
        let fns = FnTable::default();
        let mut p = Parser::new(lex("GIVE(a, b)").unwrap(), &d, Some(&fns), None, true);
        let err = p.outcome().unwrap_err();
        assert_eq!(
            err.msg,
            "unknown 'GIVE(...)' in outcome position — the outcome vocabulary is: SPAWN, DESTROY, REPLACE, SET, MOVE, REVEAL, ADVANCE, NARRATE, PROGRESS, RELATE, UNRELATE, REFUSE, ENDING, ROLL, ROLLED, DRAW, DRAWN, RUN; no ops declared; no functions declared (bare names spawn — an op's parens are part of its grammar; calls need a declared function or op)"
        );
    }

    #[test]
    fn wait_row_requires_a_dur_and_draw_is_parenless_only() {
        let d = dict();
        let mut p = Parser::new(lex("0:42 @ WAIT READ").unwrap(), &d, None, None, true);
        // WAIT followed by a non-dur falls through to the action grammar,
        // whose ref() refuses the reserved token
        let err = p.sentence().unwrap_err();
        assert_eq!(err.msg, "'WAIT' is reserved and cannot be a name (quote it)");
        let mut p2 = Parser::new(lex("DRAW(RUMOURS)").unwrap(), &d, None, None, true);
        let err2 = p2.outcome().unwrap_err();
        assert_eq!(err2.msg, "DRAW takes the paren-less form: DRAW <DECK> [INTO target]");
    }

    #[test]
    fn signed_param_carries_the_sign() {
        let d = dict();
        let mut params = HashSet::new();
        params.insert("n".to_string());
        let mut p = Parser::new(lex("SET(x, -n)").unwrap(), &d, None, Some(params), true);
        let out = p.outcome_list_or_empty().unwrap();
        assert_eq!(out[0].args[1], Arg::Param { name: "n".into(), sign: Some(Sign::Neg) });
    }
}
