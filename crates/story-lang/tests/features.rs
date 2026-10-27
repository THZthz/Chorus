//! Positive-path features: predicate inlining shapes, effectset expansion,
//! spawn handles, trigger books with `subject`, query terms (`kind:`,
//! negation, comparisons), dynamic roll operands, targeting, and the
//! compile-time subset of the kernel invariants (T2, T13).

use story_content::ir::{EffectNode, Expr, QueryTerm, Scope, Target};

const BASE: &str = "\
game \"t\"
  seed host
  turn 1m
  home yard

zones
  yard
  vault

aspects
  charge: 0
  spark: 0

cards
  card core
    kind engine  unique
    aspects charge 1
    start in yard

  card cell
    kind part  stackable
    aspects spark 2

legacies
  favor: 0

endings
  ending done \"Done\" grand
    text \"ok\"

on event tick
  rule t
    effects
      narrate \"tick\"
";

fn compile_body(body: &str) -> story_content::Content {
    let src = format!("{BASE}\n{body}\n");
    let out = story_lang::compile_source("t.sw", &src);
    assert!(
        out.diagnostics.is_empty(),
        "expected clean compile, source:\n---\n{src}---\n{}",
        out.diagnostics
            .iter()
            .map(|d| d.to_string())
            .collect::<Vec<_>>()
            .join("\n")
    );
    out.content.unwrap()
}

/// Effect lines are passed unindented and lowered into the recipe body.
fn verb_with_effects(effects: &str) -> story_content::Content {
    let indented: String = effects.lines().map(|l| format!("        {l}\n")).collect();
    compile_body(&format!(
        "verbs\n  verb v committable instances 1\n    recipe r\n      craftable\n      duration 0m\n      effects\n{indented}"
    ))
}

#[test]
fn predicates_inline_to_expression_ir() {
    let content = compile_body(
        "\
predicates
  predicate lit = core.charge >= 1
  predicate over(p, n) = p.charge > n

verbs
  verb v committable instances 1
    recipe r
      craftable
      duration 0m
      when lit and over(core, 0)
      effects
        narrate \"ok\"
",
    );
    let when = content.verbs["v"].recipes["r"].when.clone().unwrap();
    let debug = format!("{when:?}");
    // `beacon.heat`-style reads and the bound parameter both lower to
    // NamePath; no predicate reference survives.
    assert!(debug.contains("NamePath"), "{debug}");
    assert!(!debug.contains("pred"), "{debug}");
    assert!(matches!(when, Expr::Logic { .. }));
}

#[test]
fn unary_predicate_inlines_into_query_position() {
    let content = compile_body(
        "\
predicates
  predicate sparky(p) = p.spark >= 2

verbs
  verb v committable instances 1
    recipe r
      craftable
      duration 0m
      slots
        s: 1..1 sparky
      effects
        narrate \"ok\"
",
    );
    let slot = &content.verbs["v"].recipes["r"].slots[0];
    assert_eq!(slot.query.len(), 1);
    assert!(matches!(slot.query[0], QueryTerm::Pred(_)));
}

#[test]
fn effectset_expands_at_the_call_site() {
    let src = format!(
        "{BASE}\neffectset stoke(n)\n  effects\n    consume aspect:spark n\n    set core.charge += n\n\nverbs\n  verb v committable instances 1\n    recipe r\n      craftable\n      duration 0m\n      effects\n        stoke(2)\n        narrate \"end\"\n"
    );
    let out = story_lang::compile_source("t.sw", &src);
    assert!(out.diagnostics.is_empty(), "{:?}", out.diagnostics);
    let effects = &out.content.unwrap().verbs["v"].recipes["r"].effects;
    // Expansion splices inline: consume (count spliced to 2), set, narrate.
    assert_eq!(effects.len(), 3);
    assert!(matches!(
        &effects[0],
        EffectNode::Consume {
            count: Expr::Lit(2),
            ..
        }
    ));
    assert!(matches!(
        &effects[1],
        EffectNode::Set {
            expr: Expr::Lit(2),
            ..
        }
    ));
    assert!(matches!(&effects[2], EffectNode::Cue { .. }));
}

#[test]
fn handles_are_visible_after_their_spawn_line() {
    let content = verb_with_effects(
        "spawn cell count 2 as fresh\ndestroy fresh count 1\nnarrate \"minted {fresh}.\"",
    );
    let effects = &content.verbs["v"].recipes["r"].effects;
    assert!(matches!(
        &effects[0],
        EffectNode::Spawn { handle: Some(h), .. } if h == "fresh"
    ));
    assert!(matches!(
        &effects[1],
        EffectNode::Destroy { target: Target::Handle(h), .. } if h == "fresh"
    ));
}

#[test]
fn handles_do_not_leak_across_effect_lists() {
    // A second recipe cannot see the first recipe's handle [E238].
    let src = format!(
        "{BASE}\nverbs\n  verb v committable instances 1\n    recipe r\n      craftable\n      duration 0m\n      effects\n        spawn cell as fresh\n    recipe r2\n      craftable\n      duration 0m\n      effects\n        destroy fresh\n"
    );
    let out = story_lang::compile_source("t.sw", &src);
    assert!(
        out.diagnostics
            .iter()
            .any(|d| d.number == story_content::code::E238),
        "T13: handle scope ends with its effect list: {:?}",
        out.diagnostics
    );
}

#[test]
fn card_and_expiry_books_address_subject() {
    let content = compile_body(
        "\
cards
  card relic
    kind thing  unique
    aspects charge 1
    on complete [charge]
      rule blessed when subject.charge >= 1
        effects
          set subject.charge += 1
          narrate \"The relic hums at {subject}.\"  # ERR-free: subject renders
    on expire
      rule fade
        effects
          set subject.charge = 0
",
    );
    let relic = &content.cards[&story_core::CardId::from("relic")];
    let (qual, book) = &relic.on_complete[0];
    assert_eq!(qual.as_deref(), Some("charge"));
    assert_eq!(book.rules.len(), 1);
    assert!(relic.on_expire.is_some());
}

#[test]
fn query_terms_cover_kind_negation_and_comparisons() {
    let content = verb_with_effects("consume kind:part -name:cell aspect:spark>=2 1");
    let effects = &content.verbs["v"].recipes["r"].effects;
    let EffectNode::Consume {
        target: Target::BareQuery(terms),
        ..
    } = &effects[0]
    else {
        panic!("expected a bare query consume");
    };
    assert!(terms
        .iter()
        .any(|t| matches!(t, QueryTerm::Kind(k) if k == "part")));
    assert!(terms
        .iter()
        .any(|t| matches!(t, QueryTerm::Not(inner) if matches!(**inner, QueryTerm::Name(_)))));
    assert!(terms.iter().any(
        |t| matches!(t, QueryTerm::Aspect(a) if a.op == Some(story_content::ir::CmpOp::Ge) && a.value == 2)
    ));
}

#[test]
fn dynamic_roll_operands_lower_to_expressions() {
    let content = verb_with_effects(
        "roll total(bound, aspect:spark)d6 + legacy.favor as storm\nroll 0d6 + 2 as flat",
    );
    let effects = &content.verbs["v"].recipes["r"].effects;
    let EffectNode::Roll {
        count,
        sides,
        bonus,
        bind,
    } = &effects[0]
    else {
        panic!("expected roll");
    };
    assert!(matches!(
        count,
        Expr::Aspect {
            scope: Scope::Bindings,
            ..
        }
    ));
    assert!(matches!(sides, Expr::Lit(6)));
    assert!(matches!(bonus, Expr::Legacy(_)));
    assert_eq!(bind, "storm");
    assert!(matches!(
        &effects[1],
        EffectNode::Roll {
            count: Expr::Lit(0),
            ..
        }
    ));
}

#[test]
fn targets_cover_slot_scoped_and_carrier_forms() {
    let content = verb_with_effects(
        "set slot:s aspect:spark += 1\nset world aspect:charge -= 1\nmove core into vault\nswap kind:part count 1 to core into vault",
    );
    let effects = &content.verbs["v"].recipes["r"].effects;
    assert!(matches!(
        &effects[0],
        EffectNode::Set { target: Target::Slot(s), key, .. } if s.0 == "s" && key == "spark"
    ));
    assert!(matches!(
        &effects[1],
        EffectNode::Set {
            target: Target::Query {
                scope: Scope::World,
                ..
            },
            ..
        }
    ));
    assert!(matches!(
        &effects[2],
        EffectNode::Move { into: IntoZone::Zone(z), .. } if z == "vault"
    ));
    assert!(matches!(
        &effects[3],
        EffectNode::Swap { target: Target::BareQuery(_), into: Some(IntoZone::Zone(_)), to, .. } if to.0 == "core"
    ));
}

use story_content::ir::Into as IntoZone;

#[test]
fn signal_matches_a_declared_event_book() {
    let content = verb_with_effects("signal tick\nending done");
    let effects = &content.verbs["v"].recipes["r"].effects;
    assert!(matches!(&effects[0], EffectNode::Signal { event } if event == "tick"));
    assert!(matches!(&effects[1], EffectNode::Ending { .. }));
}

#[test]
fn compile_time_invariant_t2_zero_divisor_refused() {
    let src = format!(
        "{BASE}\nverbs\n  verb v committable instances 1\n    recipe r\n      craftable\n      duration 0m\n      effects\n        set core.charge += core.charge / 0\n"
    );
    let out = story_lang::compile_source("t.sw", &src);
    assert!(out
        .diagnostics
        .iter()
        .any(|d| d.number == story_content::code::E242));
}

#[test]
fn guard_prose_tokens_resolve_against_recipe_slots() {
    let content = compile_body(
        "\
verbs
  verb v committable instances 1
    recipe r
      craftable
      duration 0m
      guard \"The {s} slot is loaded.\" when core.charge >= 1
      slots
        s: 1..1 aspect:spark
      effects
        narrate \"ok\"
",
    );
    assert_eq!(content.verbs["v"].recipes["r"].guards.len(), 1);
}

#[test]
fn recipe_book_and_declares_are_stored() {
    let content = compile_body(
        "\
verbs
  verb v committable instances 1
    recipe r
      craftable
      duration 0m
      declares charge
      on complete
        rule final once priority 7 when core.charge >= 1
          effects
            legacy favor += 1
      effects
        narrate \"ok\"
",
    );
    let recipe = &content.verbs["v"].recipes["r"];
    assert_eq!(recipe.declares, vec!["charge".to_string()]);
    let book = recipe.on_complete.as_ref().unwrap();
    assert_eq!(book.rules.len(), 1);
    assert!(book.rules[0].once);
    assert_eq!(book.rules[0].priority, 7);
}

#[test]
fn duration_literals_denote_minutes_and_zero_is_instant() {
    let content = compile_body(
        "\
verbs
  verb v committable instances 1
    recipe instant
      craftable
      priority 10
      duration 0
      effects
        narrate \"flash\"
    recipe long
      craftable
      priority 20
      duration 1h30m
      when clock >= 6h
      effects
        narrate \"slow\"
",
    );
    assert_eq!(content.verbs["v"].recipes["instant"].duration, 0);
    assert_eq!(content.verbs["v"].recipes["long"].duration, 90);
    let debug = format!("{:?}", content.verbs["v"].recipes["long"].when);
    assert!(debug.contains("Lit(360)"), "{debug}");
}

#[test]
fn verb_eligibility_defaults_follow_the_mode() {
    let content = compile_body(
        "\
verbs
  verb a committable instances 1
    recipe r
      duration 0m
      effects
        narrate \"x\"
    recipe chain_r
      priority 90
      chain-only
      duration 0m
      effects
        narrate \"x\"
  verb b autonomous instances any
    recipe r
      duration 0m
      effects
        narrate \"x\"
  verb c uncommittable instances 1
    recipe r
      duration 0m
      effects
        narrate \"x\"
",
    );
    let (a, b, c) = (
        &content.verbs["a"].recipes["r"],
        &content.verbs["b"].recipes["r"],
        &content.verbs["c"].recipes["r"],
    );
    assert!(a.craftable && !a.chain_only);
    let chain_r = &content.verbs["a"].recipes["chain_r"];
    assert!(!chain_r.craftable && chain_r.chain_only);
    assert!(!b.craftable && !b.chain_only);
    assert!(!c.craftable && c.chain_only);
    assert!(matches!(
        content.verbs["b"].instances,
        story_content::ir::InstanceCap::Any
    ));
}

/// `ping()` passes zero arguments: the empty paren group contributes none.
#[test]
fn zero_arg_effectset_call_expands() {
    let content = compile_body(
        "\
effectset ping()
  effects
    narrate \"pong\"

verbs
  verb v committable instances 1
    recipe r
      craftable
      duration 0m
      effects
        ping()
        narrate \"after\"
",
    );
    let effects = &content.verbs["v"].recipes["r"].effects;
    assert_eq!(effects.len(), 2);
    assert!(matches!(&effects[0], EffectNode::Cue { .. }));
}

/// `{handle.aspect}` resolves against the enclosing spawn's handle.
#[test]
fn prose_tokens_accept_handle_aspects() {
    let content =
        verb_with_effects("spawn cell count 1 as fresh\nnarrate \"fresh spark: {fresh.spark}\"");
    let effects = &content.verbs["v"].recipes["r"].effects;
    assert!(matches!(&effects[0], EffectNode::Spawn { handle: Some(h), .. } if h == "fresh"));
    assert!(matches!(&effects[1], EffectNode::Cue { .. }));
}

/// A parameter forwarded to a card-typed callee parameter stays a card
/// reference: `outer(core)` is a legal call (language.md §2.5).
#[test]
fn predicate_parameter_forwards_as_card_reference() {
    let content = compile_body(
        "\
predicates
  predicate sparky(c) = c.spark >= 2
  predicate outer(q) = sparky(q)

verbs
  verb v committable instances 1
    recipe r
      craftable
      duration 0m
      when outer(core)
      effects
        narrate \"ok\"
",
    );
    assert!(content.verbs["v"].recipes["r"].when.is_some());
}

/// Declaration positions admit negative integers: aspect defaults, aspect
/// values, and priorities (language.md §2).
#[test]
fn negative_defaults_and_priorities_are_accepted() {
    let content = compile_body(
        "\
aspects
  depth: -3

cards
  card anchor
    unique
    aspects depth -2

verbs
  verb v committable instances 1
    recipe r
      craftable
      priority -5
      duration 0m
      effects
        narrate \"ok\"
",
    );
    assert_eq!(content.aspects["depth"].default, -3);
    let anchor = &content.cards[&story_core::CardId::from("anchor")];
    assert_eq!(anchor.aspects["depth"], -2);
    assert_eq!(content.verbs["v"].recipes["r"].priority, -5);
}

/// A duration literal too large for the clock range is [E246], not a silent 0.
#[test]
fn out_of_range_duration_literals_are_diagnosed() {
    let src = format!(
        "{BASE}\nverbs\n  verb v committable instances 1\n    recipe r\n      craftable\n      duration 99999999999999999999m\n      effects\n        narrate \"x\"\n"
    );
    let out = story_lang::compile_source("t.sw", &src);
    assert!(
        out.diagnostics
            .iter()
            .any(|d| d.number == story_content::code::E246
                && d.message.contains("duration literal out of range")),
        "{:?}",
        out.diagnostics
    );
}

/// A negative count is [E246], never a silent clamp to zero.
#[test]
fn negative_counts_are_diagnosed() {
    let src = format!(
        "{BASE}\nverbs\n  verb v committable instances 1\n    recipe r\n      craftable\n      duration 0m\n      effects\n        spawn cell count -1\n"
    );
    let out = story_lang::compile_source("t.sw", &src);
    assert!(
        out.diagnostics
            .iter()
            .any(|d| d.number == story_content::code::E246
                && d.message.contains("must be non-negative")),
        "{:?}",
        out.diagnostics
    );
}

/// `continue via` lowers to `EffectNode::Continue` with the recipe resolved
/// locally (a recipe of the same verb) or globally (`<verb>.<recipe>`);
/// a recipe may target itself (spec.md §4.2).
#[test]
fn continue_via_lowers_local_and_global_spellings() {
    let content = compile_body(
        "\
verbs
  verb hop committable instances 1
    recipe go
      priority 10
      duration 5m
      slots
        s: 0..1
      effects
        continue via back
    recipe back
      priority 20
      duration 5m
      effects
        continue via hop.go
  verb crosser uncommittable instances 1
    recipe cross
      chain-only
      duration 1m
      effects
        continue via hop.go
",
    );
    let go = &content.verbs["hop"].recipes["go"];
    assert!(matches!(
        &go.effects[0],
        EffectNode::Continue { recipe } if recipe.as_str() == "hop.back"
    ));
    let back = &content.verbs["hop"].recipes["back"];
    assert!(
        matches!(
            &back.effects[0],
            EffectNode::Continue { recipe } if recipe.as_str() == "hop.go"
        ),
        "self-continue is legal"
    );
    let cross = &content.verbs["crosser"].recipes["cross"];
    assert!(
        matches!(
            &cross.effects[0],
            EffectNode::Continue { recipe } if recipe.as_str() == "hop.go"
        ),
        "the global spelling addresses any verb's recipe"
    );
}

/// The `table` scope lowers to `Scope::Table` in expressions, conditions,
/// and targets (language.md §3).
#[test]
fn table_scope_lowers_to_scope_table() {
    let content = verb_with_effects(
        "narrate \"gone\"\nif count(table, aspect:spark) >= 1\n  destroy table aspect:spark count 1",
    );
    let effects = &content.verbs["v"].recipes["r"].effects;
    assert!(matches!(&effects[0], EffectNode::Cue { .. }));
    let EffectNode::Branch { cond, then, .. } = &effects[1] else {
        panic!("expected a branch");
    };
    let is_table_count = match cond {
        Expr::Cmp { left, .. } => matches!(
            **left,
            Expr::Count {
                scope: Scope::Table,
                ..
            }
        ),
        _ => false,
    };
    assert!(is_table_count, "{cond:?}");
    assert!(matches!(
        &then[0],
        EffectNode::Destroy {
            target: Target::Query {
                scope: Scope::Table,
                ..
            },
            ..
        }
    ));
}
