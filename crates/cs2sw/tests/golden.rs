//! Golden test: a tiny verbatim slice of CS data converts, compiles cleanly,
//! and shows the key output shapes (cards with xtrigger books, a deck with
//! its relief path, a craftable recipe with `when` and synthesized
//! requirement slots, a cost slot on the recipe that pays). Also asserts
//! byte determinism.

use std::path::PathBuf;

fn fixture_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/core")
}

#[test]
fn golden_slice_converts_and_compiles() {
    let (conversion, diags) = cs2sw::run(&fixture_root());
    let errors: Vec<_> = diags.iter().filter(|d| d.is_error()).collect();
    assert!(
        errors.is_empty(),
        "fixture conversion must compile cleanly, got: {errors:#?}"
    );
    assert!(diags.is_empty(), "unexpected warnings: {diags:?}");

    let sw = &conversion.sw;

    // Cards: one per non-aspect element, bearing their own id as an aspect.
    assert!(sw.contains("  card health\n"), "health card emitted");
    assert!(sw.contains("  card affliction\n"), "affliction card emitted");
    assert!(sw.contains("  card fleeting\n"), "fleeting card emitted");
    // health carries its CS aspects.
    assert!(
        sw.contains("aspects health 1, ability 1, heart 2, ingredient 1"),
        "health aspects line (sorted), got: {}",
        sw.lines()
            .find(|l| l.contains("aspects health 1"))
            .unwrap_or("<missing>")
    );

    // Element xtriggers -> aspect-qualified card books whose rules swap the
    // subject to the trigger target.
    assert!(
        sw.contains("on complete [fallsick]"),
        "health's fallsick xtrigger book"
    );
    assert!(
        sw.contains("swap subject.health to affliction"),
        "the xtrigger swap body"
    );
    // Unresolvable xtrigger targets (injury is not in the slice) are named
    // gaps, never silent.
    assert!(
        conversion
            .gaps
            .rows
            .iter()
            .any(|r| r.construct == "xtrigger target is an aspect or unknown element"),
        "the injury xtrigger target is reported"
    );

    // The deck from the CS dialect file (unquoted keys, tabs) with its
    // defaultcard as the relief path.
    assert!(sw.contains("  deck deck_stagriddles"), "deck emitted");
    assert!(
        sw.contains("entries waystagbefore_1\n") || sw.contains("entries waystagbefore_1\n"),
        "the in-slice spec entry resolves: {}",
        sw.lines()
            .find(|l| l.contains("entries "))
            .unwrap_or("<missing>")
    );
    assert!(
        conversion
            .gaps
            .rows
            .iter()
            .any(|r| r.construct == "deck spec entry with no card"),
        "spec entries outside the slice are reported"
    );
    assert!(sw.contains("    fallback fleeting"), "defaultcard fallback");

    // The craftable explore recipe: duration from warmup, when from
    // requirements, a synthesized requirement slot per key (every requirement
    // is bindable), and the startdescription narrate.
    assert!(
        sw.contains("    recipe explore_vaultfloating1_setup"),
        "recipe emitted"
    );
    let recipe = sw
        .split("    recipe explore_vaultfloating1_setup")
        .nth(1)
        .unwrap()
        .split("    recipe ")
        .next()
        .unwrap();
    assert!(recipe.contains("duration 30m"), "warmup 30 -> duration 30m");
    assert!(recipe.contains("priority 0"), "content-order priority");
    assert!(
        recipe.contains("req_funds: 0..1 open aspect:funds")
            && recipe.contains("req_follower: 0..1 open aspect:follower")
            && recipe.contains("req_vaultfloating1: 0..1 open aspect:vaultfloating1"),
        "every requirement key gets a synthesized slot, got: {}",
        recipe
    );
    assert!(
        recipe
            .contains("total(interior, aspect:funds) + total(slot:req_follower, aspect:funds)"),
        "the when sums the interior plus the bindable carriers (no world arm)",
    );
    assert!(
        !recipe.contains("total(world,"),
        "the world arm is retired"
    );
    assert!(
        recipe.contains("narrate \"It is rare for passenger vessels"),
        "startdescription narrated"
    );
    // The expedition setup's linked target is outside the slice: dropped and
    // reported.
    assert!(
        conversion
            .gaps
            .rows
            .iter()
            .any(|r| r.construct == "chain target is not an emitted recipe"),
        "the missing linked target is reported"
    );

    // The paying recipe: its requirement key pairs with a negative effect, so
    // the map emits a consuming cost slot instead of a bare requirement slot.
    let success = sw
        .split("    recipe explorevaultfloating1_success")
        .nth(1)
        .unwrap()
        .split("    recipe ")
        .next()
        .unwrap();
    assert!(
        success.contains("cost_vaultfloating1: 1..1 consuming open"),
        "the cost slot forces and burns the payer, got: {}",
        success
    );

    // Determinism: the same input yields identical bytes.
    let data = cs2sw::load_tree(&fixture_root());
    let again = cs2sw::convert(&data);
    assert_eq!(conversion.sw, again.sw, "conversion is deterministic");
}

#[test]
fn json_tolerances() {
    // Comments, trailing commas, unquoted keys, control characters.
    let src = "{\n// a comment\n/* block\ncomment */\n\"elements\": [{id: \"a_island\",\n\tlabel: \"An island.\",\n\tlifetime: 60,}\n,]}\n";
    let v = cs2sw::json::parse(src).expect("tolerant parse");
    let e = v.get("elements").and_then(|x| x.get(0)).unwrap();
    assert_eq!(e.get("id").and_then(|x| x.as_str()), Some("a_island"));
    assert_eq!(e.get("lifetime").and_then(|x| x.as_i64()), Some(60));
}

#[test]
fn naming_is_bijective_and_stable() {
    use cs2sw::naming::{mangle, Names};
    assert_eq!(mangle("portal.mansus"), "portal_mansus");
    // Allocation is bijective: two allocations never share a name, and the
    // same input yields the same sequence.
    let mut n = Names::new();
    let a = n.reserve_id("portal.mansus");
    let b = n.reserve_id("portal_mansus");
    assert_ne!(a, b, "distinct ids never share a name");
    let mut n2 = Names::new();
    assert_eq!(n2.reserve_id("portal.mansus"), a, "allocation is stable");
}
