//! Recursive-descent sentence parser
//! (spec §5.1). The one semantic decision point: where the actor list ends
//! and the verb begins. That needs the verb registry (both are uppercase
//! tokens), so the parser consults the Dictionary (spec §9.14).
//!
//! The function-macro layer (FnTemplate/FnTable) and the compile-
//! time expansion live in [`crate::core::fn_expand`] (the seam types —
//! `Outcome`, `Arg`, `FnTable` — are shared, not broken).

use std::collections::HashSet;

use crate::core::clock::Clock;
use crate::core::dict::Dictionary;
use crate::core::fn_expand::{unknown_call_error, FnTable};
use crate::core::lexer::{lex, Tok};
use crate::core::predicate::{parse_predicate_toks, parse_value_expr_toks, ValExpr};
use crate::core::r#ref::Ref;
use crate::core::sentence::{Arg, Clause, IfArm, IntoTarget, Outcome, Role, Sentence, SentenceKind, Sign};
use crate::error::{ChorusError, Result};

/// Clause keywords that end the actor/ref scan.
const CLAUSE_KEYWORDS: [&str; 4] = ["TO", "WITH", "AT", "ABOUT"];

/// 'ACTORS' → 'actors' (role refs in outcome
/// args are uppercase). QUIRK(Q12): `as_role` intercepts BEFORE any
/// bare-ref fallback in outcome-arg positions, but the role names are NOT in
/// RESERVED_WORDS — a soft reservation whose interception order is
/// load-bearing (an entity named `object` cannot be bare-referenced in an
/// arg position, but can in a clause position).
#[must_use]
fn as_role(v: &str) -> Option<Role> {
    Role::from_lower(&v.to_lowercase())
}

/// Parse one full sentence.
pub fn parse_sentence(src: &str, dict: &Dictionary) -> Result<Sentence> {
    Parser::new(lex(src)?, dict, None, None).sentence()
}

/// Parse an outcome list for recipe `out`
/// strings at content load: authored outcome strings must parse COMPLETELY
/// (§5.1).
pub fn parse_outcomes(src: &str, dict: &Dictionary, fns: Option<&FnTable>) -> Result<Vec<Outcome>> {
    let mut p = Parser::new(lex(src)?, dict, fns, None);
    let out = p.outcome_list_or_empty()?;
    p.expect_end()?;
    Ok(out)
}

/// Parse one function BODY into its template
/// (§9.40): parameters are active, nested calls become FN nodes, NOTHING
/// expands yet.
pub fn parse_function_body(
    src: &str,
    dict: &Dictionary,
    fns: &FnTable,
    params: &[String],
) -> Result<Vec<Outcome>> {
    let mut p = Parser::new(lex(src)?, dict, Some(fns), Some(params.iter().cloned().collect()));
    let out = p.outcome_list_or_empty()?;
    p.expect_end()?;
    Ok(out)
}

/// The recursive-descent parser over one sentence's tokens.
pub struct Parser<'a> {
    toks: Vec<Tok>,
    pos: usize,
    dict: &'a Dictionary,
    fns: Option<&'a FnTable>,
    params: Option<HashSet<String>>,
}

impl<'a> Parser<'a> {
    pub fn new(
        toks: Vec<Tok>,
        dict: &'a Dictionary,
        fns: Option<&'a FnTable>,
        params: Option<HashSet<String>>,
    ) -> Parser<'a> {
        Parser {
            toks,
            pos: 0,
            dict,
            fns,
            params,
        }
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

    /// (clippy's `manual_range_contains` lint is silenced — the explicit
    /// bound check is deliberate)
    #[allow(clippy::manual_range_contains)]
    pub fn sentence(&mut self) -> Result<Sentence> {
        let mut clock: Option<Clock> = None;
        if self.at_punct(":", 1) && matches!(self.peek(0), Some(Tok::Int { .. })) {
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
            clock = Some(Clock { day, halfhour });
            self.expect_punct("@")?;
        }

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
                        ))
                    }
                };
                self.next()?;
                trigger.push(':');
                trigger.push_str(&seg.v_str());
            }
            let mut s = Sentence {
                kind: SentenceKind::Event,
                clock,
                trigger: Some(trigger),
                clauses: Vec::new(),
                outcomes: Vec::new(),
                span_min: None,
            };
            if self.at_punct("->", 0) {
                self.next()?;
                s.outcomes = self.outcome_list()?;
            }
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
            let mut s = Sentence {
                kind: SentenceKind::Complete,
                clock,
                trigger: Some(verb_tok.v_str()),
                clauses: Vec::new(),
                outcomes: Vec::new(),
                span_min: None,
            };
            if self.at_punct("->", 0) {
                self.next()?;
                s.outcomes = self.outcome_list()?;
            }
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
            return Ok(Sentence {
                kind: SentenceKind::Wait,
                clock,
                trigger: None,
                clauses: Vec::new(),
                outcomes: Vec::new(),
                span_min: Some(v),
            });
        }

        let mut clauses = vec![self.clause()?];
        while self.at_punct(";", 0) {
            self.next()?;
            clauses.push(self.clause()?);
        }
        let mut outcomes = Vec::new();
        if self.at_punct("->", 0) {
            self.next()?;
            outcomes = self.outcome_list()?;
        }
        self.expect_end()?;
        Ok(Sentence {
            kind: SentenceKind::Action,
            clock,
            trigger: None,
            clauses,
            outcomes,
            span_min: None,
        })
    }

    /// Error unless the token stream is fully consumed.
    pub fn expect_end(&mut self) -> Result<()> {
        if let Some(t) = self.peek(0) {
            return Err(ChorusError::new(format!(
                "unexpected trailing token '{}'",
                show(t)
            )));
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
        // pre-verbal (§9.10)
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
    /// lexemes) must be quoted. §9.40: inside a function body a bare
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
                other => Err(ChorusError::new(format!(
                    "expected a ref, got '{}'",
                    show(&other)
                ))),
            };
        };
        if self.params.as_ref().is_some_and(|p| p.contains(&v)) {
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
            if self.params.as_ref().is_some_and(|p| p.contains(&nv)) {
                return Err(ChorusError::new(
                    "a parameter cannot follow 'kind:' — bind the whole ref, not the name part",
                ));
            }
            if self.dict.reserved.contains(&nv) {
                return Err(ChorusError::new(format!(
                    "'{nv}' is reserved and cannot be a name (quote it)"
                )));
            }
            return Ok(Ref::Name {
                kind: Some(v),
                name: nv,
            });
        }
        Ok(Ref::Name { kind: None, name: v })
    }

    /// outcome ( "," outcome )*.
    fn outcome_list(&mut self) -> Result<Vec<Outcome>> {
        let mut out = vec![self.outcome()?];
        while self.at_punct(",", 0) {
            self.next()?;
            out.push(self.outcome()?);
        }
        Ok(out)
    }

    /// An outcome list, or empty when nothing follows.
    fn outcome_list_or_empty(&mut self) -> Result<Vec<Outcome>> {
        if self.peek(0).is_none() {
            return Ok(Vec::new());
        }
        self.outcome_list()
    }

    /// outcome = OPERATOR "(" args ")" | ref (bare
    /// ref = SPAWN, §4.4). IF (§9.39) is intercepted before op dispatch. A
    /// declared function's NAME( dispatches to a call BEFORE op parsing
    /// (§9.40); an unknown NAME( is a compile error listing the vocabulary.
    fn outcome(&mut self) -> Result<Outcome> {
        let t = self.peek(0).cloned();
        if let Some(Tok::Name { v }) = &t {
            if v == "IF" {
                return self.if_outcome();
            }
        }
        if let Some(Tok::Name { v }) = &t {
            if self.at_punct("(", 1) {
                if self.fns.is_some_and(|f| f.by_name.contains_key(v)) {
                    return self.fn_call();
                }
                if !self.dict.is_op(v) {
                    return Err(ChorusError::new(unknown_call_error(v, self.fns)));
                }
            }
            if self.dict.is_op(v) {
                return self.op_outcome(v.clone());
            }
        }
        // bare ref = SPAWN (§4.4)
        Ok(Outcome {
            op: "SPAWN".to_string(),
            args: vec![Arg::Ref { r#ref: self.ref_()? }],
            arms: None,
            fallback: None,
            call: None,
        })
    }

    /// The op dispatch arm (a NAME token that `is_op` accepts).
    fn op_outcome(&mut self, op: String) -> Result<Outcome> {
        self.next()?;
        // REPLACE is paren-less only: its WITH keyword would mis-parse as the
        // `with` role ref inside a generic arg list (§4.4)
        if op == "REPLACE" && self.at_punct("(", 0) {
            return Err(ChorusError::new(
                "REPLACE takes the paren-less form: REPLACE ref WITH prototype",
            ));
        }
        // ENDING is paren-less only: its id is a plain name, never an arg
        // list (§9.35)
        if op == "ENDING" && self.at_punct("(", 0) {
            return Err(ChorusError::new(
                "ENDING takes the paren-less form: ENDING ENDING_ID",
            ));
        }
        // ROLL is paren-only, and its args are NOT the generic arg grammar:
        // they are §6.3 value expressions, parsed by a depth-aware Tok scan
        if op == "ROLL" {
            if !self.at_punct("(", 0) {
                return Err(ChorusError::new(
                    "ROLL takes the paren form: ROLL(<count-expr>, <sides-expr>[, <bonus-expr>])",
                ));
            }
            return self.roll_outcome();
        }
        // DRAW is paren-less only (§9.42): the deck id is a plain ref; the
        // INTO target is the SPAWN grammar
        if op == "DRAW" {
            if self.at_punct("(", 0) {
                return Err(ChorusError::new(
                    "DRAW takes the paren-less form: DRAW <DECK> [INTO target]",
                ));
            }
            if self.peek(0).is_none() {
                return Err(ChorusError::new(
                    "DRAW needs a deck id: DRAW <DECK> [INTO target]",
                ));
            }
            let mut draw_args = vec![Arg::Ref { r#ref: self.ref_()? }];
            if self.at_keyword("INTO") {
                self.next()?;
                draw_args.push(Arg::Into {
                    r#ref: self.ref_target()?,
                });
            }
            return Ok(Outcome {
                op,
                args: draw_args,
                arms: None,
                fallback: None,
                call: None,
            });
        }
        if self.at_punct("(", 0) {
            self.next()?;
            let mut args: Vec<Arg> = Vec::new();
            if !self.at_punct(")", 0) {
                args.push(self.arg()?);
                while self.at_punct(",", 0) {
                    self.next()?;
                    args.push(self.arg()?);
                }
            }
            self.expect_punct(")")?;
            // ROLLED is the engine-written record of a ROLL (§9.41): the
            // parse entry exists so concrete rows re-parse, arity pinned
            if op == "ROLLED" {
                let two_ints = matches!(
                    (args.first(), args.get(1)),
                    (Some(Arg::Int { .. }), Some(Arg::Int { .. }))
                );
                if args.len() != 2 || !two_ints {
                    return Err(ChorusError::new(
                        "ROLLED takes exactly two integers: ROLLED(<total>, <ctrAfter>)",
                    ));
                }
            }
            // DRAWN is the engine-written record of a DRAW (§9.42): an unsafe
            // spelling formats re-quoted — a quoted string IS a name ref
            // (ref()'s rule), so coerce before the shape check or the
            // recorded row would not re-parse (law 2)
            if op == "DRAWN" {
                for a in args.iter_mut() {
                    if let Arg::Str { v } = a {
                        *a = Arg::Ref {
                            r#ref: Ref::Name {
                                kind: None,
                                name: v.clone(),
                            },
                        };
                    }
                }
                let two_names = matches!(
                    (args.first(), args.get(1)),
                    (
                        Some(Arg::Ref {
                            r#ref: Ref::Name { .. }
                        }),
                        Some(Arg::Ref {
                            r#ref: Ref::Name { .. }
                        })
                    )
                );
                if args.len() != 2 || !two_names {
                    return Err(ChorusError::new(
                        "DRAWN takes exactly two name refs: DRAWN(<DECK>, <name>)",
                    ));
                }
            }
            // NARRATE's canonical form is a single string (§4.4); §9.40: a
            // parameter in the string position is allowed — it binds to the
            // call's string at expansion
            if op == "NARRATE" {
                let one_str = matches!(
                    args.first(),
                    Some(Arg::Str { .. }) | Some(Arg::Param { .. })
                );
                if args.len() != 1 || !one_str {
                    return Err(ChorusError::new("NARRATE takes exactly one string"));
                }
            }
            return Ok(Outcome {
                op,
                args,
                arms: None,
                fallback: None,
                call: None,
            });
        }
        if op == "SPAWN" {
            return Ok(Outcome {
                op,
                args: self.spawn_args()?,
                arms: None,
                fallback: None,
                call: None,
            });
        }
        if op == "NARRATE" {
            let s = self.next()?;
            if let Tok::Name { v } = &s {
                if self.params.as_ref().is_some_and(|p| p.contains(v)) {
                    return Ok(Outcome {
                        op,
                        args: vec![Arg::Param {
                            name: v.clone(),
                            sign: None,
                        }],
                        arms: None,
                        fallback: None,
                        call: None,
                    });
                }
            }
            if s.t() != "str" {
                return Err(ChorusError::new(format!(
                    "NARRATE expects a string, got '{}'",
                    show(&s)
                )));
            }
            return Ok(Outcome {
                op,
                args: vec![Arg::Str { v: s.v_str() }],
                arms: None,
                fallback: None,
                call: None,
            });
        }
        if op == "DESTROY" || op == "REVEAL" {
            return Ok(Outcome {
                op,
                args: vec![self.role_or_ref_arg()?],
                arms: None,
                fallback: None,
                call: None,
            });
        }
        if op == "ENDING" {
            // ENDING id (§9.35): a plain ref-form id, so quoted ids
            // round-trip too
            return Ok(Outcome {
                op,
                args: vec![Arg::Ref { r#ref: self.ref_()? }],
                arms: None,
                fallback: None,
                call: None,
            });
        }
        if op == "REPLACE" {
            // REPLACE ref WITH prototype (§4.4): target may be a role ref
            let target = self.role_or_ref_arg()?;
            if !self.at_keyword("WITH") {
                return Err(ChorusError::new("REPLACE expects WITH"));
            }
            self.next()?;
            return Ok(Outcome {
                op,
                args: vec![target, Arg::Ref { r#ref: self.ref_()? }],
                arms: None,
                fallback: None,
                call: None,
            });
        }
        if op == "MOVE" {
            let mut args = vec![self.role_or_ref_arg()?];
            if !self.at_keyword("INTO") {
                return Err(ChorusError::new("MOVE expects INTO"));
            }
            self.next()?;
            args.push(Arg::Into {
                r#ref: self.ref_target()?,
            });
            return Ok(Outcome {
                op,
                args,
                arms: None,
                fallback: None,
                call: None,
            });
        }
        Err(ChorusError::new(format!("{op} requires parentheses")))
    }

    /// NAME(arg, …): a call to a declared function
    /// (§9.40). Arity is checked HERE, a load error.
    fn fn_call(&mut self) -> Result<Outcome> {
        let name_tok = self.next()?;
        let Tok::Name { v: name } = name_tok else {
            return Err(ChorusError::new(
                "internal: function dispatch on a non-name token",
            ));
        };
        let params = match self.fns.and_then(|f| f.by_name.get(&name)) {
            Some(tpl) => tpl.params.clone(),
            // the dispatch guard checked `by_name.contains_key(name)` above;
            // unreachable
            None => unreachable!("fn_call dispatched on a name absent from FnTable"),
        };
        self.expect_punct("(")?;
        let mut args: Vec<Arg> = Vec::new();
        if !self.at_punct(")", 0) {
            args.push(self.arg()?);
            while self.at_punct(",", 0) {
                self.next()?;
                args.push(self.arg()?);
            }
        }
        self.expect_punct(")")?;
        if args.len() != params.len() {
            return Err(ChorusError::new(format!(
                "function '{name}' expects {n} argument(s){list}, got {got}",
                n = params.len(),
                list = if !params.is_empty() {
                    format!(" ({})", params.join(", "))
                } else {
                    String::new()
                },
                got = args.len()
            )));
        }
        Ok(Outcome {
            op: "FN".to_string(),
            call: Some(name),
            args,
            arms: None,
            fallback: None,
        })
    }

    /// IF pred -> ( outcomes ) { ELSE IF … } [ELSE …]
    /// (§9.39). The ELSE chain is consumed greedily INSIDE this node — the
    /// `,`-loop of outcome_list must not strand an ELSE.
    fn if_outcome(&mut self) -> Result<Outcome> {
        self.next()?; // IF
        let mut arms = vec![self.if_arm()?];
        let mut fallback: Option<Vec<Outcome>> = None;
        while self.at_keyword("ELSE") {
            self.next()?;
            if self.at_keyword("IF") {
                self.next()?;
                arms.push(self.if_arm()?);
            } else {
                self.expect_punct("->")?;
                fallback = Some(self.branch_body()?);
                break; // ELSE is terminal
            }
        }
        Ok(Outcome {
            op: "IF".to_string(),
            args: Vec::new(),
            arms: Some(arms),
            fallback,
            call: None,
        })
    }

    /// One IF arm: the condition is a balanced unit up to the depth-0 `->`,
    /// then the branch body.
    fn if_arm(&mut self) -> Result<IfArm> {
        // the condition is parsed from the SENTENCE token stream as a
        // balanced unit up to the depth-0 `->` (predicates contain commas via
        // rel()/every() — never comma-split)
        let (pred, end) = parse_predicate_toks(&self.toks[self.pos..], self.params.as_ref())?;
        self.pos += end;
        self.expect_punct("->")?;
        let then = self.branch_body()?;
        Ok(IfArm { when: pred, then })
    }

    /// Branch body: `( outcomes )`.
    fn branch_body(&mut self) -> Result<Vec<Outcome>> {
        self.expect_punct("(")?;
        let out = self.outcome_list()?;
        self.expect_punct(")")?;
        Ok(out)
    }

    /// ROLL(<count-expr>, <sides-expr>[, <bonus-expr>])
    /// (§9.41). Each argument is consumed as a BALANCED Tok unit up to the
    /// depth-0 `,` (or the closing `)`), then parsed with the SAME §6.3
    /// value-expression grammar the IF cmp operands use.
    fn roll_outcome(&mut self) -> Result<Outcome> {
        self.next()?; // '('
        let mut args: Vec<Arg> = Vec::new();
        if !self.at_punct(")", 0) {
            loop {
                args.push(Arg::Vexpr {
                    v: self.roll_arg_expr()?,
                });
                if self.at_punct(",", 0) {
                    self.next()?;
                    continue;
                }
                break;
            }
        }
        self.expect_punct(")")?;
        if args.len() < 2 || args.len() > 3 {
            return Err(ChorusError::new(format!(
                "ROLL takes (count-expr, sides-expr[, bonus-expr]), got {n} argument(s)",
                n = args.len()
            )));
        }
        Ok(Outcome {
            op: "ROLL".to_string(),
            args,
            arms: None,
            fallback: None,
            call: None,
        })
    }

    /// One balanced ROLL argument: Toks up to the
    /// depth-0 ',' or ')'.
    fn roll_arg_expr(&mut self) -> Result<ValExpr> {
        let start = self.pos;
        let mut depth: i64 = 0;
        loop {
            let Some(t) = self.peek(0) else {
                return Err(ChorusError::new(
                    "unexpected end of ROLL arguments — ROLL takes (count-expr, sides-expr[, bonus-expr])",
                ));
            };
            if t.t() == "punct" {
                match t.v_str().as_str() {
                    "(" => depth += 1,
                    ")" => {
                        if depth == 0 {
                            break;
                        }
                        depth -= 1;
                    }
                    "," if depth == 0 => break,
                    _ => {}
                }
            }
            self.next()?;
        }
        let slice = &self.toks[start..self.pos];
        if slice.is_empty() {
            return Err(ChorusError::new("ROLL arguments cannot be empty"));
        }
        parse_value_expr_toks(slice, self.params.as_ref())
    }

    /// §9.40: a parameter wins before the role check
    /// (a param may shadow a role name inside a body exactly as it shadows
    /// an entity name); QUIRK(Q12): the role check intercepts BEFORE any
    /// bare-ref fallback — the soft-reservation order is load-bearing.
    fn role_or_ref_arg(&mut self) -> Result<Arg> {
        let t = self.peek(0).cloned();
        if let Some(Tok::Name { v }) = &t {
            if self.params.as_ref().is_some_and(|p| p.contains(v)) {
                self.next()?;
                return Ok(Arg::Param {
                    name: v.clone(),
                    sign: None,
                });
            }
        }
        if let Some(Tok::Name { v }) = &t {
            if let Some(role) = as_role(v) {
                self.next()?;
                return Ok(Arg::Role { role });
            }
        }
        Ok(Arg::Ref { r#ref: self.ref_()? })
    }

    /// SPAWN ref [INTO target] [AS #id] [k=v …]
    /// (canonical paren-less form). §9.40: a parameter as the pair's key OR
    /// value replaces the whole pair (these param checks are LIVE —
    /// unlike arg()'s dead key check, Q3).
    fn spawn_args(&mut self) -> Result<Vec<Arg>> {
        let mut args = vec![Arg::Ref { r#ref: self.ref_()? }];
        if self.at_keyword("INTO") {
            self.next()?;
            args.push(Arg::Into {
                r#ref: self.ref_target()?,
            });
        }
        if self.at_keyword("AS") {
            self.next()?;
            let id = self.next()?;
            let Tok::IdRef { v } = id else {
                return Err(ChorusError::new(format!(
                    "expected #id after AS, got '{}'",
                    show(&id)
                )));
            };
            args.push(Arg::AsId { id: v });
        }
        while self.at_punct("=", 1) {
            let k = self.next()?;
            let Tok::Name { v: key } = k else {
                return Err(ChorusError::new(format!(
                    "expected key=value pair, got '{}'",
                    show(&k)
                )));
            };
            self.next()?; // '='
            // §9.40: a parameter as the pair's key OR value replaces the
            // whole pair (the LIVE twin of arg()'s dead branch, Q3)
            let v_is_param = match self.peek(0) {
                Some(Tok::Name { v }) => self.params.as_ref().is_some_and(|p| p.contains(v)),
                _ => false,
            };
            let param_name = if self.params.as_ref().is_some_and(|p| p.contains(&key)) {
                Some(key.clone())
            } else if v_is_param {
                match self.peek(0) {
                    Some(Tok::Name { v }) => Some(v.clone()),
                    _ => None,
                }
            } else {
                None
            };
            if let Some(p_name) = param_name {
                self.next()?; // consumes the value token (even when the KEY was the param)
                args.push(Arg::Param {
                    name: p_name,
                    sign: None,
                });
                continue;
            }
            args.push(Arg::Pair {
                k: key,
                v: self.value_token()?,
            });
        }
        Ok(args)
    }

    /// The generic outcome arg.
    pub fn arg(&mut self) -> Result<Arg> {
        let Some(t) = self.peek(0).cloned() else {
            return Err(ChorusError::new("unexpected end of outcome args"));
        };
        // signed SET value (§9.31): '+'/'-' int is a delta on the entry's
        // current value. §9.40: the magnitude may be a parameter — the sign
        // is carried on the placeholder and folded into the call's number at
        // expansion.
        if t.t() == "punct" && (t.v_str() == "+" || t.v_str() == "-") {
            self.next()?;
            let n = self.next()?;
            if let Tok::Name { v } = &n {
                if self.params.as_ref().is_some_and(|p| p.contains(v)) {
                    return Ok(Arg::Param {
                        name: v.clone(),
                        sign: Some(if t.v_str() == "-" { Sign::Neg } else { Sign::Pos }),
                    });
                }
            }
            if n.t() == "dur" {
                return Err(ChorusError::new(
                    "deltas are integers only (no duration deltas)",
                ));
            }
            let Tok::Int { v } = n else {
                return Err(ChorusError::new(format!(
                    "expected an integer after '{}', got '{}'",
                    t.v_str(),
                    show(&n)
                )));
            };
            return Ok(Arg::Delta {
                v: if t.v_str() == "-" { -v } else { v },
            });
        }
        match t {
            Tok::Str { v } => {
                self.next()?;
                Ok(Arg::Str { v })
            }
            Tok::Dur { v } => {
                self.next()?;
                Ok(Arg::Dur { v })
            }
            Tok::Int { v } => {
                self.next()?;
                Ok(Arg::Int { v })
            }
            Tok::IdRef { v } => {
                self.next()?;
                Ok(Arg::Ref {
                    r#ref: Ref::Id { id: v },
                })
            }
            Tok::Name { v } => self.name_arg(v),
            other => Err(ChorusError::new(format!(
                "unexpected token in outcome args: '{}'",
                show(&other)
            ))),
        }
    }

    /// The name-token arm of arg().
    fn name_arg(&mut self, v: String) -> Result<Arg> {
        if v == "INTO" {
            self.next()?;
            return Ok(Arg::Into {
                r#ref: self.ref_target()?,
            });
        }
        if v == "AS" {
            self.next()?;
            let id = self.next()?;
            let Tok::IdRef { v: idv } = id else {
                return Err(ChorusError::new(format!(
                    "expected #id after AS, got '{}'",
                    show(&id)
                )));
            };
            return Ok(Arg::AsId { id: idv });
        }
        // QUIRK(Q3): the param check below returns the WHOLE arg as a param —
        // which makes a key-param check UNREACHABLE here (dead code,
        // deliberately absent). DO NOT add a param-key check
        // and DO NOT "fix" SET's refusal of `SET(myparam = 3)` — it reports
        // `expected ')', got '='` from the outcome loop, frozen.
        // NOTE: the bare-param branch IS reachable and lives
        // right below (the whole arg becomes a placeholder).
        if self.params.as_ref().is_some_and(|p| p.contains(&v)) {
            self.next()?;
            return Ok(Arg::Param {
                name: v,
                sign: None,
            });
        }
        let role = as_role(&v);
        if let Some(role) = role {
            // ACTORS/OBJECT/… in outcome args are role refs (e.g. MOVE ACTORS
            // INTO OBJECT)
            self.next()?;
            return Ok(Arg::Role { role });
        }
        if v == "true" || v == "false" {
            self.next()?;
            return Ok(Arg::Bool { v: v == "true" });
        }
        if self.at_punct("=", 1) {
            // §9.40: a parameter as the pair's VALUE replaces the whole pair
            // (reachable). The KEY check is dead (Q3).
            // Deliberate asymmetry vs spawn_args: the value token is NOT
            // consumed here (only key and '=' advance), so
            // `SET(x, k = p)` leaves `p` in the stream and the outcome loop
            // reports `expected ')', got 'p'` — frozen behavior.
            let value_is_param = match self.peek(2) {
                Some(Tok::Name { v: nv }) => self.params.as_ref().is_some_and(|p| p.contains(nv)),
                _ => false,
            };
            self.next()?;
            self.next()?;
            if value_is_param {
                if let Some(Tok::Name { v: nv }) = self.peek(0) {
                    return Ok(Arg::Param {
                        name: nv.clone(),
                        sign: None,
                    });
                }
                unreachable!("value_is_param checked the same peek"); // guarded by the peek above
            }
            let k = v;
            return Ok(Arg::Pair {
                k,
                v: self.value_token()?,
            });
        }
        // `aspect:` names the aspects map in SET keys (§9.31) — one key
        // token, like the query form (§4.5)
        if v == "aspect" && self.at_punct(":", 1) && matches!(self.peek(2), Some(Tok::Name { .. })) {
            self.next()?;
            self.next()?;
            let n = self.next()?;
            let mut key = format!("aspect:{}", n.v_str());
            while self.at_punct(".", 0) && matches!(self.peek(1), Some(Tok::Name { .. })) {
                self.next()?;
                let seg = self.next()?;
                key.push('.');
                key.push_str(&seg.v_str());
            }
            return Ok(Arg::Key { v: key });
        }
        // dotted names in arg position are SET keys: SET(x, state.sealed, true)
        if self.at_punct(".", 1) && matches!(self.peek(2), Some(Tok::Name { .. })) {
            let mut key = v;
            self.next()?; // the leading name
            while self.at_punct(".", 0) && matches!(self.peek(1), Some(Tok::Name { .. })) {
                self.next()?;
                let seg = self.next()?;
                key.push('.');
                key.push_str(&seg.v_str());
            }
            return Ok(Arg::Key { v: key });
        }
        Ok(Arg::Ref { r#ref: self.ref_()? })
    }

    /// The INTO target: param | role | ref.
    fn ref_target(&mut self) -> Result<IntoTarget> {
        let t = self.peek(0).cloned();
        if let Some(Tok::Name { v }) = &t {
            if self.params.as_ref().is_some_and(|p| p.contains(v)) {
                self.next()?;
                return Ok(IntoTarget::Ref(Ref::Param { name: v.clone() }));
            }
        }
        if let Some(Tok::Name { v }) = &t {
            if let Some(role) = as_role(v) {
                self.next()?;
                return Ok(IntoTarget::Role { role });
            }
        }
        Ok(IntoTarget::Ref(self.ref_()?))
    }

    /// A scalar value token for k=v pairs.
    fn value_token(&mut self) -> Result<crate::core::types::AttrValue> {
        use crate::core::types::AttrValue;
        let t = self.next()?;
        match t {
            Tok::Int { v } => Ok(AttrValue::Num(v)),
            Tok::Dur { v } => Ok(AttrValue::Num(v)), // minutes
            Tok::Str { v } => Ok(AttrValue::Str(v)),
            Tok::Name { v } => {
                if v == "true" {
                    Ok(AttrValue::Bool(true))
                } else if v == "false" {
                    Ok(AttrValue::Bool(false))
                } else {
                    Ok(AttrValue::Str(v))
                }
            }
            other => Err(ChorusError::new(format!(
                "expected a value, got '{}'",
                show(&other)
            ))),
        }
    }
}

/// `show(t)`: strings print re-quoted (RAW quotes, no
/// escaping), everything else prints its v.
#[must_use]
pub fn show(t: &Tok) -> String {
    if t.t() == "str" {
        format!("\"{}\"", t.v_str())
    } else {
        t.v_str()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::dict::core_dictionary;
    use crate::core::fn_expand::FnTable;
    use crate::core::sentence::Sign;

    fn dict() -> Dictionary {
        core_dictionary()
    }

    #[test]
    fn q3_set_refuses_a_param_key_while_spawn_accepts_one() {
        let d = dict();
        let mut params = HashSet::new();
        params.insert("myparam".to_string());
        // SET(myparam = 3) inside a body: arg() consumes myparam as a param,
        // then the outcome loop trips on '=' — the frozen error
        let mut p = Parser::new(lex("SET(myparam = 3)").unwrap(), &d, None, Some(params.clone()));
        let err = p.outcome_list_or_empty().unwrap_err();
        assert_eq!(err.msg, "expected ')', got '='");
        // SPAWN's own arg parse DOES accept the param key (the whole pair becomes
        // the param, and the value token is consumed) — the frozen asymmetry
        let mut p2 = Parser::new(lex("SPAWN X myparam = 3").unwrap(), &d, None, Some(params));
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
    fn q3_alive_value_param_in_arg_position_is_returned_unconsumed() {
        let d = dict();
        let mut params = HashSet::new();
        params.insert("p".to_string());
        // The value-param branch is reachable: a param as the pair's VALUE.
        // arg() consumes
        // key and '=' but NOT the param token — the outcome loop then
        // reports the frozen `expected ')', got 'p'` (asymmetric with
        // spawn_args, which consumes it)
        let mut p = Parser::new(lex("SET(x, k = p)").unwrap(), &d, None, Some(params));
        let err = p.outcome_list_or_empty().unwrap_err();
        assert_eq!(err.msg, "expected ')', got 'p'");
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
        );
        let s = p.sentence().unwrap();
        let o = &s.outcomes[0];
        assert_eq!(o.op, "IF");
        assert_eq!(o.arms.as_ref().unwrap().len(), 1);
        assert!(o.fallback.is_some());
        // engine-written rows re-parse (law 2)
        let mut p2 = Parser::new(lex("1:2 @ COMPLETE READ -> ROLLED(14, 3)").unwrap(), &d, None, None);
        let s2 = p2.sentence().unwrap();
        assert_eq!(s2.kind, SentenceKind::Complete);
        assert_eq!(s2.outcomes[0].op, "ROLLED");
        assert_eq!(s2.outcomes[0].args[0], Arg::Int { v: 14 });
        assert_eq!(s2.outcomes[0].args[1], Arg::Int { v: 3 });
        // ROLLED arity is pinned
        let mut p3 = Parser::new(lex("1:2 @ COMPLETE READ -> ROLLED(14)").unwrap(), &d, None, None);
        let err = p3.sentence().unwrap_err();
        assert_eq!(err.msg, "ROLLED takes exactly two integers: ROLLED(<total>, <ctrAfter>)");
    }

    #[test]
    fn fn_call_arity_error_is_byte_exact() {
        let d = dict();
        let fns = FnTable::default();
        let mut p = Parser::new(
            lex("GIVE(a, b)").unwrap(),
            &d,
            Some(&fns),
            None,
        );
        let err = p.outcome().unwrap_err();
        assert_eq!(
            err.msg,
            "unknown 'GIVE(...)' in outcome position — the outcome vocabulary is: SPAWN, DESTROY, REPLACE, SET, MOVE, REVEAL, ADVANCE, NARRATE, PROGRESS, RELATE, UNRELATE, REFUSE, ENDING, ROLL, ROLLED, DRAW, DRAWN; no functions declared (bare names spawn — an op's parens are part of its grammar; calls need a declared function)"
        );
    }

    #[test]
    fn wait_row_requires_a_dur_and_draw_is_parenless_only() {
        let d = dict();
        let mut p = Parser::new(lex("0:42 @ WAIT READ").unwrap(), &d, None, None);
        // WAIT followed by a non-dur falls through to the action grammar,
        // whose ref() refuses the reserved token
        let err = p.sentence().unwrap_err();
        assert_eq!(err.msg, "'WAIT' is reserved and cannot be a name (quote it)");
        let mut p2 = Parser::new(lex("DRAW(RUMOURS)").unwrap(), &d, None, None);
        let err2 = p2.outcome().unwrap_err();
        assert_eq!(err2.msg, "DRAW takes the paren-less form: DRAW <DECK> [INTO target]");
    }

    #[test]
    fn signed_param_carries_the_sign() {
        let d = dict();
        let mut params = HashSet::new();
        params.insert("n".to_string());
        let mut p = Parser::new(lex("SET(x, -n)").unwrap(), &d, None, Some(params));
        let out = p.outcome_list_or_empty().unwrap();
        assert_eq!(
            out[0].args[1],
            Arg::Param {
                name: "n".into(),
                sign: Some(Sign::Neg)
            }
        );
    }
}
