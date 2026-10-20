//! The site census. Enumerates every SPAWN/REPLACE/
//! DRAW site over the compiled content with the segment discipline that
//! mirrors the engine's draft semantics: each IF branch is its own SEGMENT
//! (an authored REFUSE kills exactly the sites whose ancestor chain contains
//! its segment), while a throwing op anywhere in a list demotes every site in
//! it (a completion refusal discards the whole draft).

use std::rc::Rc;

use indexmap::{IndexMap, IndexSet};

use crate::content::walk_outcomes;
use crate::content::{CompiledContent, CompiledDeck, CompiledTrigger};
use crate::core::predicate::Pred;
use crate::core::r#ref::Ref;
use crate::core::sentence::{Arg, Outcome};
use crate::core::types::Entity;

use super::world::pred_static;

/// `MANY = 1e9`, the "unbounded copies" sentinel: it feeds the `min(count,
/// 32)` weight cap, the draw export gate (`cap != MANY` means exportable)
/// and `cap > idx` position checks, so it must stay an exact
/// integer — i64 `1_000_000_000`.
pub const MANY: i64 = 1_000_000_000;

/// The `"\u{0000}derived"`
/// sentinel avail/by-name key for derive-mints behind object-mandatory verbs.
/// The literal string (not an enum) is load-bearing: the
/// three sentinel properties (occupies an avail/by-name key; slot queries hit
/// it via the intact kind/aspect descriptor; `exists(NAME)`/`count(name:)`
/// gates miss it because no authored name collides) are exactly strict string
/// identity on this value, and the raw string reproduces even the adversarial
/// corner where an author names a card with the sentinel itself.
pub const DERIVED_NAME: &str = "\u{0000}derived";

/// Ops that throw on an unresolvable target and therefore
/// defer to a completion-time window refusal.
const THROWING_OPS: [&str; 4] = ["MOVE", "RELATE", "UNRELATE", "REPLACE"];

/// Whether the op demotes its whole outcome list: a throwing op, or a DRAW
/// with an INTO clause (the drawn-into move is the same unresolvable-target
/// throw). `pub(crate)`: the fixpoint's completion-draft walk reuses the
/// same throw classification (a completion error refuses the window).
pub(crate) fn op_throws(o: &Outcome) -> bool {
    THROWING_OPS.contains(&o.op.as_str())
        || (o.op == "DRAW" && o.args.iter().any(|a| matches!(a, Arg::Into { .. })))
}

// ---------------------------------------------------------------------------
// the authored aspects= pair parse (the Q13 aspect model). The
// world-static shape tests (`pred_static`/`expr_static`) live in world.rs
// beside their evaluators; collect_sites imports pred_static for its site
// tagging (runtime_gated / path_non_static).
// ---------------------------------------------------------------------------

/// Parse a site's `aspects=` JSON string under the Q13 model: it takes integers
/// and one-level integer sub-maps; members outside the model are dropped
/// (they read as misses in aspect_get either way — see [`site_aspects`]). A
/// malformed JSON throws, which the universe build turns into the
/// `opaque_spawn_aspects` flag.
pub(crate) fn parse_aspects_json(json: &str) -> crate::error::Result<crate::core::types::Aspects> {
    let v: serde_json::Value = serde_json::from_str(json)
        .map_err(|e| crate::error::ChorusError::new(format!("bad aspects pair: {e}")))?;
    let serde_json::Value::Object(map) = v else {
        return Err(crate::error::ChorusError::new("bad aspects pair: not an object"));
    };
    let mut out = crate::core::types::Aspects::new();
    for (k, val) in map {
        if let Some(av) = json_to_aspect(&val) {
            out.insert(k, av);
        }
    }
    Ok(out)
}

fn json_to_aspect(v: &serde_json::Value) -> Option<crate::core::types::AspectVal> {
    match v {
        serde_json::Value::Number(n) => n.as_i64().map(crate::core::types::AspectVal::Num),
        serde_json::Value::Object(sub) => {
            let mut m = IndexMap::new();
            for (k2, v2) in sub {
                if let Some(n) = v2.as_i64() {
                    m.insert(k2.clone(), n);
                }
            }
            Some(crate::core::types::AspectVal::Sub(m))
        }
        _ => None,
    }
}

/// Per-segment walk flags.
#[derive(Debug, Default, Clone)]
pub(crate) struct SegFlags {
    pub refused: bool,
    pub throwing: i64,
}

/// One raw producer site, pre-fixpoint. Identity keys
/// are indices: `verb` is the content.verbs insertion index,
/// `deck` a shared reference (compiled decks are immutable).
#[derive(Debug, Clone)]
pub(crate) struct RawSite<'c> {
    pub kind: SiteKind,
    /// spawn/replace: the minted name; draw: the deck id
    pub name: String,
    /// SPAWN kind:X — overrides the minted kind (NOT a kind lookup)
    pub kind_override: Option<String>,
    pub deck: Option<&'c CompiledDeck>,
    /// authored `aspects=` pair — a static mint-time aspect override
    pub aspects_json: Option<String>,
    /// display: `recipe <id> of VERB` or `trigger <id>`
    pub owner: String,
    /// the owning verb's content.verbs index (None for trigger-owned sites)
    pub verb: Option<usize>,
    /// the owning recipe's index within its verb (None for trigger-owned
    /// sites) — the completion-draft rung re-derives the completing recipe
    /// from it (the owner string is display-only)
    pub recipe: Option<usize>,
    /// recipe selection guard (None → default)
    pub when: Option<Pred>,
    /// an earlier recipe of the same verb exists (first-match may shadow)
    pub shadow_risk: bool,
    /// enclosing IF conditions
    pub path: Vec<Pred>,
    /// the innermost IF segment containing this site
    pub seg: usize,
    /// position of the site within its list (walk order over ALL ops)
    pub pos: i64,
    /// ancestor chain of IF segments (0 = list top level)
    pub segs: Vec<usize>,
    /// inside an ELSE body — runs only when earlier arms are false
    pub fallback: bool,
    /// an enclosing IF condition is non-static (roll/role-dependent)
    pub runtime_gated: bool,
    /// throwing-capable ops in this list (list-wide demotion)
    pub throw_ops: i64,
    /// draws this site contributes to its deck (1 for a once trigger, MANY
    /// otherwise); MANY for non-draw sites
    pub cap: i64,
    /// REPLACE: the name-form ref being replaced
    pub replace_target: Option<String>,
    /// a later op in the list destroys/replaces this very mint
    pub killed_in_list: bool,
    /// this list can mint a name that is (or may become) already alive
    pub dup_risk: bool,
    /// a pin-capable RUN/FORK in this list is followed by another chain op —
    /// the second op hits the taken chain slot FIRST (§6.7) and refuses the
    /// parent window, so the whole draft dies (event-channel lists exempt —
    /// there the second op is a silent skip, §6.6)
    pub chain_refusal: bool,
    /// this list mints a name whose non-stackable holder may already be
    /// alive from OUTSIDE the list (an initial card, or another list's
    /// non-reusable mint) — §5.1's collision face across lists: the engine's
    /// mint check reads the LIVING HOLDERS' stackable flag, so the mint may
    /// refuse and the whole application (a firing step or a completion
    /// draft) dies with it. Content-static, computed once over the whole
    /// census (the cross-list join needs every list walked).
    pub collision_refusal: bool,
    /// this site mints a UNIQUE name an earlier unconditional op of the same
    /// list already mints — §4.2's mint face SKIPS while a copy lives, and
    /// the earlier mint in the same application always lands first, so this
    /// site never mints (its product never exists).
    pub unique_skipped: bool,
    /// §4.2 (v0.10.1): the ENGINE-TIMED holder sources of this site's
    /// group SIBLINGS — for each member name this site lands (a mint, or a
    /// draw's serve: the DRAWN row's SPAWN half rides the same mint path),
    /// every sibling member of its group with an engine-timed minter
    /// outside this list, as `(name, source)` pairs. The group
    /// living-check skips the landing while ANY member lives; whether the
    /// engine-timed sibling is alive at THIS site's landing is a timing
    /// question the fixpoint decides (the collision join's
    /// `collision_sources` twin). Same-list siblings are the ordering face
    /// (an earlier op's mint lands first; a later one cannot block the
    /// earlier landing), and a derive-renamed mint is not a member.
    /// Content-static, computed once over the whole census.
    pub group_sources: Vec<(String, MintSource)>,
    /// REPLACE: another op in this list also kills the replace-target
    pub target_killed_in_list: bool,
    /// non-stackable names this LIST mints (bootstrap-gate check); the
    /// post-pass assigns it to every site, so an empty set is the honest "no mints".
    /// `Rc`: every site of a list shares ONE read-only set (a per-site deep
    /// clone of the same set bought nothing — the sets are never mutated
    /// after the post-pass)
    pub list_gate_mints: Rc<IndexSet<String>>,
    /// STACKABLE names this LIST mints (a gate on them reads false at a
    /// once-trigger's single firing) — shared like [`RawSite::list_gate_mints`]
    pub list_stack_mints: Rc<IndexSet<String>>,
    /// SPAWN of a `deriving name from object` prototype
    pub derives: bool,
    /// v0.10.0: the authored `parked` mint modifier — the product is born
    /// inside the interior (`state.parked: true` at mint), so the fixpoint
    /// registers it into the PARKED pool, not the world pool (§13.1)
    pub parks: bool,
    /// v0.10.0: a later `SET <this name>, state.parked, true` op of the SAME
    /// list — with no later non-`true` `state.parked` write after it on the
    /// site's chain (parking is two-phase) — flips this product from the
    /// world pool to the parked pool — the `killed_in_list` machinery's
    /// twin, one application's face (a cross-list SET is player-timed and
    /// unmodeled: the product stays world-side, the sound under-claim)
    pub parked_in_list: bool,
    /// display owner (always equal to `owner`)
    pub where_: String,
}

/// The three producer-site kinds: SPAWN, REPLACE, DRAW.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SiteKind {
    Spawn,
    Replace,
    Draw,
}

/// The SET-site row (one shared shape for every SET encountered on the walk).
#[derive(Debug, Clone)]
pub(crate) struct SetSite {
    pub key: String,
    pub owner: String,
    pub verb_name: Option<String>,
    /// the owning recipe's index within its verb (None for trigger-owned
    /// lists) — the same key [`RawSite::recipe`] carries, so the bounded-
    /// horizon faces read the SET's own recipe's `dur` override (§6.4)
    pub recipe: Option<usize>,
    pub target_name: Option<String>,
    pub target_role: Option<String>,
    /// the innermost segment and walk position of the SET op — the
    /// ancestor-chain/position test the parked pool's `parked_in_list` flip
    /// runs against a SPAWN site (the kill log's same shape)
    pub seg: usize,
    pub pos: i64,
    pub segs: Vec<usize>,
    pub shadowed: bool,
    /// annotated after the walk: a SET whose own segment chain contains an
    /// authored REFUSE can never persist
    pub refused: bool,
    pub path_non_static: bool,
    pub when_pred: Option<Pred>,
    pub owner_slot: Option<i64>,
    /// v0.10.0: the write parks its target — key `state.parked`, value the
    /// exact boolean `true` (the flag's one true face)
    pub parks_target: bool,
}

/// Where an outside holder's copy can come from — the timing side of the
/// collision join. `Initial` is alive from minute zero; `ExpireTrigger`
/// fires at the arming's expiry (content-unknowable, so never orderable);
/// `SlotTrigger` fires first at its own first crossing; `AmbientRecipe`
/// lands at the sweep's first boundary plus the window length.
#[derive(Debug, Clone, Copy)]
pub(crate) enum MintSource {
    Initial,
    SlotTrigger { slot: i64 },
    ExpireTrigger,
    AmbientRecipe { verb: usize, recipe: Option<usize> },
}

/// The census result. Sites live in one arena Vec (walk order across all
/// lists); `live`/`refused` are arena indices in push order — the risky block
/// mutates sites through these indices (the shared per-list site objects,
/// flattened into the arena).
#[derive(Debug, Default)]
pub(crate) struct Census<'c> {
    pub sites: Vec<RawSite<'c>>,
    pub live: Vec<usize>,
    pub refused: Vec<usize>,
    pub set_sites: Vec<SetSite>,
    /// name → the owners (list display strings) that destroy/replace it
    pub destroyed_by: IndexMap<String, IndexSet<String>>,
    /// every non-stackable name a mint could collide with — minted
    /// non-stackable names anywhere in the content ∪ initial non-stackable
    /// entity names
    pub collision_names: IndexSet<String>,
    // ---- the cross-list interference map (§5.1/§6.7, one join) ----
    /// initial non-stackable entity names — a holder that is alive at every
    /// first application unless the play killed it
    pub initial_colliders: IndexSet<String>,
    /// every initial entity name — a name that can be living from minute
    /// zero (the alive side of the collision join)
    pub initial_names: IndexSet<String>,
    /// name → the owners whose mint of the name is NON-reusable (a mint that
    /// would refuse against a living copy) — the outside-holder side of the
    /// engine's mint check, joined per list in the post-pass
    pub nonstack_minters: IndexMap<String, IndexSet<String>>,
    /// owner → the candidate OWNER strings of each chain op (walk order, one
    /// entry per RUN/FORK; empty when the op names no candidate) — the pin
    /// side of the §6.7 chain-channel map
    pub chain_target_owners: IndexMap<String, Vec<IndexSet<String>>>,
    /// owners whose list carries a TOP-LEVEL (seg-0) REFUSE — the inline
    /// gate's refusal face: the §6.7 gate walk skips IF ops entirely, so a
    /// REFUSE nested in an arm never fires there, and any list pinning this
    /// list's recipe refuses its own window
    pub gate_refusers: IndexSet<String>,
    /// owners whose application may refuse on a cross-list mint collision
    /// (the [`RawSite::collision_refusal`] join, computed once)
    pub collision_refusers: IndexSet<String>,
    /// the refusing owner's per-name holder sources — the timing side of the
    /// join, consumed where the application time is content-decidable (a
    /// slot trigger's first crossing vs the holder's earliest landing)
    pub collision_sources: IndexMap<String, Vec<(String, MintSource)>>,
    /// owner → the candidate recipe ids of each chain op, as walked (raw,
    /// unresolved) — resolved into [`Census::chain_target_owners`] by the
    /// collect_sites post-pass
    pub pending_chain_targets: IndexMap<String, Vec<Vec<String>>>,
    /// owner → every name the owner's sites mint (any reusability) — the
    /// minting side of the collision join
    pub owner_minted: IndexMap<String, IndexSet<String>>,
    /// owners whose mints land WITHOUT the player's timing: slot/expire
    /// trigger bodies (engine-driven crossings) and AMBIENT verbs' recipes
    /// (the §6.9 sweep opens them itself) — plus the initial world, the
    /// holder sources whose aliveness the player cannot always outrun. A
    /// player-committed recipe's mint is dodgeable existentially (the player
    /// orders their own commits), so it is not a refusal source.
    pub engine_timed_owners: IndexSet<String>,
}

/// Owner string ('trigger t3') → (trigger, declaration
/// index). Built once per world computation (one per `Analyzer`); duplicate
/// trigger ids keep later-wins
/// semantics — the stored trigger is replaced, the original slot retained.
#[derive(Debug)]
pub(crate) struct TrigIndex<'c> {
    entries: IndexMap<String, (&'c CompiledTrigger, i64)>,
}

impl<'c> TrigIndex<'c> {
    pub(crate) fn build(content: &'c CompiledContent) -> Self {
        let mut entries = IndexMap::new();
        for (i, t) in content.triggers.iter().enumerate() {
            entries.insert(trigger_owner(t.id()), (t, i as i64));
        }
        TrigIndex { entries }
    }

    /// (trigger, decl) — decl is the -1 sentinel (callers tolerate a missing
    /// declaration index), so the stored index is i64.
    pub(crate) fn get(&self, owner: &str) -> Option<(&'c CompiledTrigger, i64)> {
        self.entries.get(owner).map(|(t, d)| (*t, *d))
    }
}

/// The display/key owner string of a trigger (`trigger t3`) — the ONE format
/// shared by `TrigIndex`'s key and the `risky_owners`/`dirty_owners`/
/// `destroyed_by`/`trigger_mint_names` owner keys.
pub(crate) fn trigger_owner(id: &str) -> String {
    format!("trigger {id}")
}

/// SPAWN's first arg as a (name, kind?) name ref.
pub(crate) fn spawn_name_ref(o: &Outcome) -> Option<(&str, Option<&str>)> {
    if o.op != "SPAWN" {
        return None;
    }
    match o.args.first() {
        Some(Arg::Ref { r#ref: Ref::Name { name, kind, .. } }) => Some((name, kind.as_deref())),
        _ => None,
    }
}

/// REPLACE's second arg as a name ref.
pub(crate) fn replace_with_ref(o: &Outcome) -> Option<&str> {
    if o.op != "REPLACE" {
        return None;
    }
    match o.args.get(1) {
        Some(Arg::Ref { r#ref: Ref::Name { name, .. } }) => Some(name),
        _ => None,
    }
}

/// DRAW's first arg as a name ref (the deck id).
pub(crate) fn draw_deck_id(o: &Outcome) -> Option<&str> {
    if o.op != "DRAW" {
        return None;
    }
    match o.args.first() {
        Some(Arg::Ref { r#ref: Ref::Name { name, .. } }) => Some(name),
        _ => None,
    }
}

/// The per-branch traversal context of [`walk_sites`]: the enclosing IF
/// conditions, the ancestor segment chain and the ELSE flag — the
/// `(path, segs, fallback)` triple threaded through the recursion.
#[derive(Debug, Clone)]
pub(crate) struct WalkFrame {
    pub path: Vec<Pred>,
    pub segs: Vec<usize>,
    pub fallback: bool,
}

/// A DESTROY/REPLACE kill recorded during the walk.
#[derive(Debug)]
pub(crate) struct Kill {
    pub seg: usize,
    pub pos: i64,
    pub name: String,
    pub throws: bool,
}

/// The walker state threaded through [`walk_outcomes`] by `&mut`: the
/// per-list segment-flag table, the seg allocator, the op counter, the raw
/// sites and the kill log — one shared state object so every traversal
/// callback sees the same bookkeeping.
#[derive(Debug, Default)]
pub(crate) struct WalkState<'c> {
    pub flags: IndexMap<usize, SegFlags>,
    pub seg_counter: usize,
    pub op_counter: i64,
    pub sites: Vec<RawSite<'c>>,
    pub kills: Vec<Kill>,
    /// the list's RUN/FORK ops in walk order, each carrying its PIN
    /// capability: does it name ≥1 name-form candidate (candidates resolve
    /// against the GLOBAL recipe table at load, so a named candidate always
    /// resolves). The post-pass folds it into the list's chain-slot refusal.
    pub chain_ops: Vec<bool>,
    /// the same ops' named candidate recipe ids (walk order) — the raw form
    /// the census post-pass resolves into [`Census::chain_target_owners`].
    pub chain_targets: Vec<Vec<String>>,
}

/// The per-list identity of the list being walked — the list context the
/// census walker threads through `walk_list` (the owner's trigger slot is
/// looked up once and folded into the [`RecordCx`]).
pub(crate) struct ListCx<'a> {
    pub owner: &'a str,
    pub verb_idx: Option<usize>,
    pub recipe_idx: Option<usize>,
    pub when: Option<&'a Pred>,
    pub shadow_risk: bool,
    pub shadowed: bool,
}

/// The immutable per-list context of one list's walk.
/// `'c` is the compiled content's lifetime
/// (the only lifetime a RawSite borrows — its deck); `'a` covers the
/// per-list strings/preds, which the sites CLONE.
pub(crate) struct RecordCx<'c, 'a> {
    pub content: &'c CompiledContent,
    pub owner: &'a str,
    pub verb_idx: Option<usize>,
    pub recipe_idx: Option<usize>,
    pub when: Option<&'a Pred>,
    pub shadow_risk: bool,
    pub shadowed: bool,
    pub own_slot: Option<i64>,
}

/// Built on the ONE authored-outcome
/// traversal shared with content's load guards. Each IF branch
/// gets its own SEGMENT allocated per arm, left to right, before that arm
/// descends (the ELSE body after
/// all arms); a REFUSE marks its OWN segment and a throwing op marks its
/// segment too, but demotion is list-wide. The seg-flag bookkeeping lives
/// inside the traversal (never outside it), so the census's exhaustive
/// enumeration and the content guards' cannot drift.
pub(crate) fn walk_sites<'c>(
    outs: &[Outcome],
    cx: &RecordCx<'c, '_>,
    set_sites: &mut Vec<SetSite>,
    destroyed_by: &mut IndexMap<String, IndexSet<String>>,
) -> WalkState<'c> {
    let mut st = WalkState::default();
    let root = WalkFrame { path: Vec::new(), segs: vec![0], fallback: false };
    let _ = walk_outcomes(
        outs,
        &root,
        &mut st,
        // visit + the seg-flag marking for this op:
        // REFUSE/throwing marks the INNERMOST segment of the active frame —
        // the same segment a nested arm's sites carry in their ancestor chain
        &mut |st, o, frame| {
            record_visit(st, o, frame, cx, set_sites, destroyed_by);
            let seg = match frame.segs.last() {
                Some(&s) => s,
                None => unreachable!("every WalkFrame carries at least seg 0"),
            };
            if o.op == "REFUSE" {
                st.flags.entry(seg).or_default().refused = true;
            } else if op_throws(o) {
                st.flags.entry(seg).or_default().throwing += 1;
            }
            Ok(())
        },
        // an IF arm: [...path, arm.when] + a FRESH segment
        &mut |st, arm, frame| {
            st.seg_counter += 1;
            let seg = st.seg_counter;
            let mut path = frame.path.clone();
            path.push(arm.when.clone());
            let mut segs = frame.segs.clone();
            segs.push(seg);
            WalkFrame { path, segs, fallback: false }
        },
        // the ELSE body: same path, a fresh segment, fallback = true
        &mut |st, frame| {
            st.seg_counter += 1;
            let seg = st.seg_counter;
            let mut segs = frame.segs.clone();
            segs.push(seg);
            WalkFrame { path: frame.path.clone(), segs, fallback: true }
        },
    );
    // walk_sites creates the flags entry for every segment
    // it enters, including arms with empty bodies. Segments are dense from 0,
    // so ensuring them after the walk is entry-for-entry identical.
    for seg in 0..=st.seg_counter {
        st.flags.entry(seg).or_default();
    }
    st
}

/// A mint's stackability is the engine's §4.2 rule
/// ([`CompiledContent::mint_is_stackable`]) over the RESOLVED kind: the
/// site's kind override when it carries one, else the prototype's own kind;
/// a name with neither only stacks by declaration. Uniqueness does NOT gate
/// this flag — the REPEAT-COUNT consumers test [`mint_name_reusable`] /
/// [`CompiledContent::mint_is_unique`] beside it (a unique mint in a
/// stackable KIND carries the kind's flag, but its pool never outlives one
/// living copy: the mint face skips).
pub(crate) fn mint_stacks(content: &CompiledContent, name: &str, kind: Option<&str>) -> bool {
    let resolved = kind.or_else(|| content.prototypes.get(name).map(|p| p.kind.as_str()));
    match resolved {
        Some(k) => content.mint_is_stackable(name, k),
        None => content.stackable_names.contains(name),
    }
}

/// A mint's NAME-REUSE face (§4.2): may it share its name with living
/// copies? A declared stackable mint stacks; a UNIQUE mint is skipped by the
/// engine's mint face while a copy lives, so it never name-collides either —
/// and the same holds for a uniquegroup MEMBER (the mint face skips while
/// any member of its group lives, so the repeat mint deals nothing). Every
/// collision/duplication consumer (the census mint counters, the
/// co-mint freeze scan through `collision_names`, the duplicate-collision
/// risk, the window/draft refusal rungs) reads this, not [`mint_stacks`].
/// Uniqueness is name-level (the declared unique NAMES, §4.2); stackability
/// resolves the kind like [`mint_stacks`] does.
pub(crate) fn mint_name_reusable(
    content: &CompiledContent,
    name: &str,
    kind: Option<&str>,
) -> bool {
    content.mint_is_unique(name)
        || content.mint_is_group_member(name)
        || match kind.or_else(|| content.prototypes.get(name).map(|p| p.kind.as_str())) {
            Some(k) => content.mint_is_stackable(name, k),
            None => content.stackable_names.contains(name),
        }
}

/// The verb binds an object on EVERY
/// commit (its object slot is mandatory), so a `deriving name from object`
/// mint never carries the bare name. `verb` is the content.verbs index.
pub(crate) fn object_always(content: &CompiledContent, verb: Option<usize>) -> bool {
    let Some(vi) = verb else {
        return false;
    };
    let Some((_, v)) = content.verbs.get_index(vi) else {
        unreachable!("verb index {vi} came from content.verbs enumeration");
    };
    v.slot_by_role.get("object").is_some_and(|slot| slot.min >= 1)
}

/// A DRAW behind an object-mandatory
/// verb never serves the BARE name of a deck card whose prototype derives.
pub(crate) fn draw_blinds_bare(content: &CompiledContent, site: &RawSite, name: &str) -> bool {
    site.kind == SiteKind::Draw
        && site.verb.is_some()
        && object_always(content, site.verb)
        && content
            .prototypes
            .get(name)
            .and_then(|p| p.derive.as_ref())
            .and_then(|d| d.name_from.as_deref())
            == Some("object")
}

/// Authored `aspects=` pair on a SPAWN
/// site, parsed; unparseable pairs are the caller's opaque case (None).
///
/// JSON values outside the Q13 aspect model (strings/bools/floats/deeper
/// nesting) parse fine and would be carried along; they are invisible to
/// aspect_get (non-number reads miss) and can only surface through the
/// serialized demand descriptor. They are kept OUT of the merged map —
/// same `match_query` results (miss == absent), and no pinned surface
/// authors such pairs (a deliberate engine choice).
pub(crate) fn site_aspects(site: &RawSite) -> Option<crate::core::types::Aspects> {
    let json = site.aspects_json.as_deref()?;
    parse_aspects_json(json).ok()
}

/// The census build over the whole compiled content.
pub(crate) fn collect_sites<'c>(
    content: &'c CompiledContent,
    init_names: &IndexMap<String, &'c Entity>,
    trig_idx: &TrigIndex<'c>,
) -> Census<'c> {
    let mut census = Census::default();
    // initial non-stackable names poison the collision set
    for e in init_names.values() {
        census.initial_names.insert(e.name.clone());
        if !e.stackable {
            census.collision_names.insert(e.name.clone());
            census.initial_colliders.insert(e.name.clone());
        }
    }
    // verbs first, recipes in declaration order
    for (vi, (_name, verb)) in content.verbs.iter().enumerate() {
        for (i, r) in verb.recipes.iter().enumerate() {
            // first-match (§6.4): any earlier recipe can shadow this one; an
            // earlier when-less recipe shadows PERMANENTLY
            let shadowed = verb.recipes[..i].iter().any(|rr| rr.when.is_none());
            walk_list(
                content,
                trig_idx,
                &r.out,
                ListCx {
                    owner: &format!("recipe {} of {}", r.id, verb.name),
                    verb_idx: Some(vi),
                    recipe_idx: Some(i),
                    when: r.when.as_ref(),
                    shadow_risk: i > 0,
                    shadowed,
                },
                &mut census,
            );
        }
    }
    // triggers after verbs
    for t in &content.triggers {
        walk_list(
            content,
            trig_idx,
            t.out(),
            ListCx {
                owner: &trigger_owner(t.id()),
                verb_idx: None,
                recipe_idx: None,
                when: None,
                shadow_risk: false,
                shadowed: false,
            },
            &mut census,
        );
    }
    // the engine-timed minters: slot/expire bodies (crossings fire them
    // without the player) and ambient recipes (the §6.9 sweep opens them
    // itself). Complete bodies and player recipes are player-timed.
    for t in &content.triggers {
        if matches!(t, CompiledTrigger::Slot { .. } | CompiledTrigger::Expire { .. }) {
            census.engine_timed_owners.insert(trigger_owner(t.id()));
        }
    }
    for (_name, verb) in content.verbs.iter() {
        if verb.ambient {
            for r in &verb.recipes {
                census.engine_timed_owners.insert(format!("recipe {} of {}", r.id, verb.name));
            }
        }
    }
    resolve_cross_list_map(content, &mut census, trig_idx);
    census
}

/// THE cross-list interference map — one join, computed once, after every
/// list has walked (§5.1/§6.7: the analyzer's census and refusal model are
/// per-list, but the engine shares the living world and, within a
/// completion, ONE chain channel across the completing recipe and every
/// firing `on complete` body; the fixpoint consumes this map per
/// completion instead of re-deriving it):
///
///   - `chain_target_owners`: each list's RUN/FORK candidates resolved to
///     their owning lists (the pin side of the channel map);
///   - `gate_refusers`: lists whose TOP-LEVEL REFUSE the §6.7 inline gate
///     would fire when pinned — any list pinning them refuses its own
///     window (the ledger's pinned-candidate channel);
///   - `collision_refusers`: lists that mint a name whose non-stackable
///     holder may already be alive from OUTSIDE the list (an initial card
///     or another list's non-reusable mint) — the engine's mint check reads
///     the living holders' stackable flag (mint.rs `check_mint_name_free`),
///     so such an application may refuse and its whole draft dies.
///
/// The list's own non-reusable mints are NOT outside holders: the first
/// application creates the holder, so its mints land — only the cross-list
/// (or initial) side refuses an application whose mints never landed
/// anywhere.
fn resolve_cross_list_map<'c>(
    content: &'c CompiledContent,
    census: &mut Census<'c>,
    trig_idx: &TrigIndex<'c>,
) {
    // resolve the raw candidate ids to owner strings
    for (owner, ops) in census.pending_chain_targets.drain(..) {
        let resolved: Vec<IndexSet<String>> = ops
            .iter()
            .map(|op| {
                op.iter()
                    .filter_map(|id| {
                        content
                            .recipe_verbs
                            .get(id.as_str())
                            .map(|vn| format!("recipe {id} of {vn}"))
                    })
                    .collect()
            })
            .collect();
        census.chain_target_owners.insert(owner, resolved);
    }
    // the collision join (the engine's two-sided mint check, mint.rs
    // `check_mint_name_free`): a mint of `name` refuses when the name is
    // LIVING at the application and (the minted copy itself is
    // non-reusable, or some living holder is non-stackable). The aliveness
    // is the ENGINE-TIMED side only: the initial world, or a holder minted
    // without the player's timing (a slot/expire firing, an ambient sweep
    // completion) — the player can order their own commits, so a
    // player-timed outside mint is dodgeable existentially and no refusal
    // follows from it. A hostile holder/mint exists when the name is minted
    // non-reusably by anyone (this owner's own non-reusable mint refuses
    // against ANY living pool; another owner's non-reusable mint is the
    // non-stackable holder) or the initial card is non-stackable.
    // the holder sources (outside engine-timed minters), resolved once
    let holder_sources: Vec<(&String, MintSource)> = census
        .engine_timed_owners
        .iter()
        .filter_map(|o| {
            match trig_idx.get(o) {
                Some((CompiledTrigger::Slot { slot, .. }, _)) => {
                    return Some((o, MintSource::SlotTrigger { slot: *slot }));
                }
                Some((CompiledTrigger::Expire { .. }, _)) => {
                    return Some((o, MintSource::ExpireTrigger));
                }
                _ => {}
            }
            // an ambient recipe: resolve its verb/recipe indices
            content.verbs.iter().enumerate().find_map(|(vi, (_, v))| {
                v.recipes.iter().enumerate().find_map(|(ri, r)| {
                    (format!("recipe {} of {}", r.id, v.name) == *o)
                        .then_some((o, MintSource::AmbientRecipe { verb: vi, recipe: Some(ri) }))
                })
            })
        })
        .collect();
    for (owner, names) in &census.owner_minted {
        let mut sources: Vec<(String, MintSource)> = Vec::new();
        let refuses = names.iter().any(|name| {
            let mut alive = false;
            if census.initial_names.contains(name) {
                alive = true;
                // the initial holder is unorderable (alive from minute zero)
                if census.initial_colliders.contains(name) {
                    sources.push((name.clone(), MintSource::Initial));
                }
            }
            for (o, src) in &holder_sources {
                if *o == owner {
                    continue;
                }
                if census.owner_minted.get(*o).is_some_and(|ns| ns.contains(name)) {
                    alive = true;
                    sources.push((name.clone(), *src));
                }
            }
            let hostile = census.nonstack_minters.contains_key(name)
                || census.initial_colliders.contains(name);
            alive && hostile
        });
        if refuses {
            census.collision_refusers.insert(owner.clone());
            census.collision_sources.insert(owner.clone(), sources);
        }
    }
    // assign the per-site flag from the map
    for s in census.sites.iter_mut() {
        s.collision_refusal = census.collision_refusers.contains(&s.owner);
    }
    // §4.2 (v0.10.1): the GROUP living-check's cross-list face — a member
    // landing (a mint, or a draw's serve: the DRAWN row's SPAWN half rides
    // the same mint path) SKIPS while ANY member of its group has a LIVING
    // copy. An INITIAL living member (a replayed midgame world's entity — a
    // fresh initial world never carries a member name: entity lines reject
    // `uniquegroup`) is alive from minute zero and blocks unorderably, the
    // collision join's Initial discipline. A sibling member minted by an
    // ENGINE-TIMED owner outside this list is alive at landings the player
    // cannot outrun (the holder-source discipline above — a player-timed
    // sibling is dodgeable existentially and never blocks), so such a
    // site's landing deals nothing and it cannot prove its product.
    // Same-list siblings are the ordering face (an earlier op's mint lands
    // first — the unique_skipped face; a later op's mint cannot block the
    // earlier landing), and a derive-renamed mint is not a member (the
    // resolved name escapes the group, §4.2).
    let mut member_engine_minters: IndexMap<&str, IndexSet<&str>> = IndexMap::new();
    for (o, _) in &holder_sources {
        if let Some(ns) = census.owner_minted.get(*o) {
            for n in ns {
                if content.mint_is_group_member(n) {
                    member_engine_minters.entry(n.as_str()).or_default().insert(*o);
                }
            }
        }
    }
    for s in census.sites.iter_mut() {
        // a derive-renamed mint carries a different name — never a member
        if s.kind != SiteKind::Draw && s.derives && object_always(content, s.verb) {
            continue;
        }
        let names: Vec<String> = if s.kind == SiteKind::Draw {
            site_mints(s).iter().map(|(n, _)| (*n).to_string()).collect()
        } else {
            vec![s.name.clone()]
        };
        for n in &names {
            let Some(g) = content.mint_group(n) else { continue };
            let Some(members) = content.unique_groups.get(g) else { continue };
            for m in members {
                if m == n {
                    continue;
                }
                // the initial sibling: alive before every landing, no owner
                // to exclude (one row per blocking member name)
                if census.initial_names.contains(m) {
                    s.group_sources.push((m.clone(), MintSource::Initial));
                }
                if let Some(os) = member_engine_minters.get(m.as_str()) {
                    for o in os {
                        if *o != s.owner {
                            // the sibling's earliest landing source (one row
                            // per blocking owner)
                            if let Some((_, src)) = holder_sources.iter().find(|(ho, _)| ho == o) {
                                s.group_sources.push((m.clone(), *src));
                            }
                        }
                    }
                }
            }
        }
    }
}

/// The per-name mint counter row.
struct MintCount {
    count: i64,
    all_stack: bool,
}

/// One list's walk. A new
/// segment-flag table, op counter and kill log per list; the census
/// accumulators (`live`/`refused`/`set_sites`/`destroyed_by`/… ) ride
/// through `census`.
#[allow(clippy::too_many_arguments)]
fn walk_list<'c, 'a>(
    content: &'c CompiledContent,
    trig_idx: &TrigIndex<'c>,
    out: &'a [Outcome],
    list_cx: ListCx<'a>,
    census: &mut Census<'c>,
) {
    // the owner trigger's slot, if any
    let own_slot: Option<i64> = match trig_idx.get(list_cx.owner) {
        Some((CompiledTrigger::Slot { slot, .. }, _)) => Some(*slot),
        _ => None,
    };

    // the walk itself; the REFUSE/throwing marking rides
    // inside walk_sites
    let set_start = census.set_sites.len();
    let cx = RecordCx {
        content,
        owner: list_cx.owner,
        verb_idx: list_cx.verb_idx,
        recipe_idx: list_cx.recipe_idx,
        when: list_cx.when,
        shadow_risk: list_cx.shadow_risk,
        shadowed: list_cx.shadowed,
        own_slot,
    };
    let mut st = walk_sites(out, &cx, &mut census.set_sites, &mut census.destroyed_by);
    // this list's own SET ops (the slice pushed by the walk above) — the
    // parked pool's in-list flip reads them beside the kill log
    let list_sets = &census.set_sites[set_start..];
    let sites = &mut st.sites;
    let kills = &st.kills;
    let flags = &st.flags;

    // segment-scoped REFUSE death + list-wide throwing
    let dead_segs: IndexSet<usize> =
        flags.iter().filter(|(_, f)| f.refused).map(|(k, _)| *k).collect();
    let throw_ops: i64 = flags.values().map(|f| f.throwing).sum();

    // the §6.7 chain-slot refusal (fuzz seeds 103/170): the list REFUSES iff
    // some pin-capable chain op has ANOTHER chain op after it — the second
    // op checks the taken slot BEFORE it walks its candidates, so its own
    // gate is irrelevant, and the refusal discards the whole completion
    // draft. Event-channel lists (slot/expire trigger bodies) are EXEMPT:
    // there the second pinning op is a SILENT SKIP (§6.6 — an event has no
    // step to refuse; the first op's effects stand and the body continues).
    let event_channel = matches!(
        trig_idx.get(list_cx.owner),
        Some((CompiledTrigger::Slot { .. } | CompiledTrigger::Expire { .. }, _))
    );
    let chain_refusal = !event_channel && {
        let mut pinned = false;
        let mut refuses = false;
        for &capable in &st.chain_ops {
            if pinned {
                refuses = true;
                break;
            }
            pinned = capable;
        }
        refuses
    };

    // per-name mint counts, duplicates → dup_risk; the same walk feeds the
    // cross-list map (per-owner minted names, per-name non-reusable minters)
    let mut mint_counts: IndexMap<String, MintCount> = IndexMap::new();
    let mut owner_mints: IndexMap<String, IndexSet<String>> = IndexMap::new();
    for s in sites.iter() {
        if s.kind == SiteKind::Draw && s.deck.is_some() {
            for (name, kind) in site_mints(s) {
                add_mint(
                    content,
                    &mut mint_counts,
                    &mut census.collision_names,
                    &mut census.nonstack_minters,
                    &mut owner_mints,
                    list_cx.owner,
                    name,
                    kind,
                );
            }
        } else {
            add_mint(
                content,
                &mut mint_counts,
                &mut census.collision_names,
                &mut census.nonstack_minters,
                &mut owner_mints,
                list_cx.owner,
                &s.name,
                s.kind_override.as_deref(),
            );
        }
    }
    for k in kills.iter() {
        if !k.throws {
            continue; // a second DESTROY of a dead name is a silent no-op
        }
        if mint_stacks(content, &k.name, None) {
            continue;
        }
        // kill pairs always collide (1012)
        let e = mint_counts
            .entry(format!("kill:{}", k.name))
            .or_insert(MintCount { count: 0, all_stack: false });
        e.count += 1;
    }
    let dup_risk = mint_counts.values().any(|e| e.count >= 2 && !e.all_stack);
    let list_gate_mints: Rc<IndexSet<String>> =
        Rc::new(mint_counts.iter().filter(|(_, e)| !e.all_stack).map(|(n, _)| n.clone()).collect());
    let mut list_stack_mints: IndexSet<String> = IndexSet::new();
    for s in sites.iter() {
        if s.kind == SiteKind::Draw && s.deck.is_some() {
            for (name, kind) in site_mints(s) {
                stacky(content, &mut list_stack_mints, name, kind);
            }
        } else {
            stacky(content, &mut list_stack_mints, &s.name, s.kind_override.as_deref());
        }
    }

    // shared with every site of the list through one Rc (see RawSite's
    // field docs)
    let list_stack_mints = Rc::new(list_stack_mints);

    // the §4.2 unique/group mint face across a list (§5.1's skip, same
    // application): an earlier op of this list minting the SAME UNIQUE name
    // — or ANY member of this site's GROUP — unconditionally (its segment
    // chain is this site's ancestor chain — it always applies before this
    // site) lands the ONE copy first, and the mint face SKIPS for this site
    // — the site's product never exists. Draw sources count (a draw serves
    // a fresh copy of a not-yet-manifested unique or member like any mint).
    let mut unique_skipped_flags: IndexSet<usize> = IndexSet::new();
    for (i, s) in sites.iter().enumerate() {
        // the site's blocked-by set: its own name for a unique, every
        // member name of its group for a group member (a name is never
        // both — the grammar rejects `unique` + `uniquegroup`, §4.2)
        let blocked_by: Vec<String> = if content.mint_is_unique(&s.name) {
            vec![s.name.clone()]
        } else {
            match content.mint_group(&s.name) {
                Some(g) => content
                    .unique_groups
                    .get(g)
                    .map(|ms| ms.iter().cloned().collect())
                    .unwrap_or_default(),
                None => Vec::new(),
            }
        };
        if blocked_by.is_empty() {
            continue;
        }
        let earlier = sites[..i].iter().any(|o| {
            o.pos < s.pos
                && s.segs.starts_with(&o.segs)
                && site_mints(o).iter().any(|(n, _)| blocked_by.iter().any(|b| b == n))
        });
        if earlier {
            unique_skipped_flags.insert(i);
        }
    }

    // the census side of the §6.7 chain-channel map: this list's per-op
    // candidate ids (resolved against the global table in the collect_sites
    // post-pass, where every list has walked) and the inline-gate refusal
    // face — a TOP-LEVEL (seg-0) REFUSE. The §6.7 gate walk skips IF ops
    // entirely, so only a seg-0 REFUSE is decisive there; a REFUSE inside an
    // arm marks the arm's segment, not seg 0.
    let chain_targets: Vec<Vec<String>> = st.chain_targets.clone();
    let gate_refuses = st.flags.get(&0).is_some_and(|f| f.refused);
    census.pending_chain_targets.insert(list_cx.owner.to_string(), chain_targets);
    if gate_refuses {
        census.gate_refusers.insert(list_cx.owner.to_string());
    }
    // the minting side of the collision join: every name this list mints
    // UNCONDITIONALLY — an ELSE body runs only when earlier arms are false,
    // so its holder may never exist (whether the sibling arms decide false
    // is a world question), and counting it would demote lists no play ever
    // refuses. The dup/collision bookkeeping above stays complete.
    for s in sites.iter() {
        if !s.fallback {
            if s.kind == SiteKind::Draw && s.deck.is_some() {
                for (name, _) in site_mints(s) {
                    census
                        .owner_minted
                        .entry(list_cx.owner.to_string())
                        .or_default()
                        .insert(name.to_string());
                }
            } else {
                census
                    .owner_minted
                    .entry(list_cx.owner.to_string())
                    .or_default()
                    .insert(s.name.clone());
            }
        }
    }

    // the post-pass, site by site (co-mint reads see
    // the whole list, mutation goes to the one site)
    for i in 0..sites.len() {
        let cap = draw_cap(sites, i, census, trig_idx);
        let s = &mut sites[i];
        s.throw_ops = throw_ops;
        s.chain_refusal = chain_refusal;
        s.unique_skipped = unique_skipped_flags.contains(&i);
        s.killed_in_list = s.kind != SiteKind::Draw
            && kills.iter().any(|k| {
                s.segs.contains(&k.seg) && (k.seg != s.seg || k.pos > s.pos) && k.name == s.name
            });
        s.dup_risk = dup_risk;
        // v0.10.0: the parked pool's in-list flip — a later same-list
        // `SET <name>, state.parked, true` moves this product into the
        // parked pool (one application's face, the kill log's twin shape;
        // a SET itself doomed by a REFUSE'd segment never applies, so it
        // cannot flip anything). Parking is two-phase, death is not: a
        // LATER non-`true` `state.parked` write on the site's
        // ancestor-or-equal chain — after the park SET's own position —
        // un-parks the product, so the application ends world-side and
        // that park SET flips nothing.
        s.parked_in_list = s.kind == SiteKind::Spawn
            && list_sets.iter().any(|set| {
                set.parks_target
                    && set.target_name.as_deref() == Some(s.name.as_str())
                    && !set.segs.iter().any(|id| dead_segs.contains(id))
                    && s.segs.contains(&set.seg)
                    && (set.seg != s.seg || set.pos > s.pos)
                    && !list_sets.iter().any(|unset| {
                        !unset.parks_target
                            && unset.key == "state.parked"
                            && unset.target_name.as_deref() == Some(s.name.as_str())
                            && !unset.segs.iter().any(|id| dead_segs.contains(id))
                            && s.segs.contains(&unset.seg)
                            && (unset.seg != s.seg || unset.pos > set.pos)
                    })
            });
        s.list_gate_mints = Rc::clone(&list_gate_mints);
        s.list_stack_mints = Rc::clone(&list_stack_mints);
        s.target_killed_in_list = s.kind == SiteKind::Replace
            && s.replace_target.is_some()
            && kills.iter().any(|k| {
                Some(&k.name) == s.replace_target.as_ref() && (k.seg != s.seg || k.pos != s.pos)
            });
        s.cap = cap;
    }
    for s in std::mem::take(sites) {
        if s.segs.iter().any(|id| dead_segs.contains(id)) {
            let idx = census.sites.len();
            census.sites.push(s);
            census.refused.push(idx);
        } else {
            let idx = census.sites.len();
            census.sites.push(s);
            census.live.push(idx);
        }
    }
    // a SET inside a REFUSE-doomed segment chain can
    // never persist; it must not serve as an expiry-arming witness (41047)
    for set in census.set_sites.iter_mut() {
        if set.owner == list_cx.owner && set.segs.iter().any(|id| dead_segs.contains(id)) {
            set.refused = true;
        }
    }
}

/// One site's draw cap: a once trigger draws exactly 1, a repeatable one
/// draws unbounded — unless a collision-named co-mint freezes the cursor.
/// Non-draw sites (and hand-built deckless draws, which never enter the
/// census) are unbounded (`MANY`).
fn draw_cap<'c>(
    sites: &[RawSite<'c>],
    i: usize,
    census: &Census<'c>,
    trig_idx: &TrigIndex<'c>,
) -> i64 {
    let site = &sites[i];
    if site.kind != SiteKind::Draw || site.deck.is_none() {
        return MANY;
    }
    let deck = draw_deck(site);
    let once = match trig_idx.get(&site.owner) {
        Some((CompiledTrigger::Slot { once, .. }, _)) => *once,
        _ => false,
    };
    let mut cap = if once { 1 } else { MANY };
    if cap == MANY {
        // repeatable site: position k is served only if applications
        // 0..k-1 all succeeded — a non-stackable co-mint freezes the
        // cursor (fuzz seed 1963); not even position 0 when order[0]
        // itself is a co-mint
        let mut co_mints: IndexSet<String> = IndexSet::new();
        for (j, o) in sites.iter().enumerate() {
            if j == i {
                continue;
            }
            if o.kind == SiteKind::Draw && o.deck.is_some() {
                for (name, _) in site_mints(o) {
                    co_mints.insert(name.to_string());
                }
                continue;
            }
            if !census.collision_names.contains(&o.name) {
                continue;
            }
            co_mints.insert(o.name.clone());
        }
        if !co_mints.is_empty() {
            let first = match deck.order.first() {
                Some(c) => &c.name,
                None => unreachable!("compiled decks always carry a non-empty order"),
            };
            cap = if co_mints.contains(first) { 0 } else { 1 };
        }
    }
    cap
}

/// Per-name UNIFORMITY: a list re-minting X
/// collides unless EVERY mint of X shares its name safely (§5.1; seed 2654 —
/// stackable mints stack, unique mints skip, §4.2). The same walk feeds the
/// cross-list map: `owner_mints` records that `owner` mints `name` (any
/// reusability), and a NON-reusable mint records the owner beside the name
/// in `nonstack_minters` — the outside-holder side of the engine's mint
/// check (mint.rs reads the LIVING HOLDERS' stackable flag, so a non-stackable
/// holder from another list or the initial world makes any mint of the name
/// refuse).
#[allow(clippy::too_many_arguments)]
fn add_mint(
    content: &CompiledContent,
    mint_counts: &mut IndexMap<String, MintCount>,
    collision_names: &mut IndexSet<String>,
    nonstack_minters: &mut IndexMap<String, IndexSet<String>>,
    owner_mints: &mut IndexMap<String, IndexSet<String>>,
    owner: &str,
    name: &str,
    kind: Option<&str>,
) {
    let reusable = mint_name_reusable(content, name, kind);
    if !reusable {
        collision_names.insert(name.to_string());
        nonstack_minters.entry(name.to_string()).or_default().insert(owner.to_string());
    }
    owner_mints.entry(name.to_string()).or_default().insert(owner.to_string());
    let e = mint_counts.entry(name.to_string()).or_insert(MintCount { count: 0, all_stack: true });
    e.count += 1;
    e.all_stack = e.all_stack && reusable;
}

/// Collect the name-sharing (STACKABLE or UNIQUE) names this list mints.
fn stacky(content: &CompiledContent, set: &mut IndexSet<String>, name: &str, kind: Option<&str>) {
    if mint_name_reusable(content, name, kind) {
        set.insert(name.to_string());
    }
}

/// The `(name, kind?)` pairs one site can mint: a draw serves its deck's
/// `order` cards then the `defaultcard` — that sequence is load-bearing
/// (the census mint counters, the co-mint scans and the mint-source
/// registration all walk it in this order) — and any other site mints its
/// own name with its kind override. Sites must carry a resolved deck (see
/// [`draw_deck`]).
pub(crate) fn site_mints<'a>(site: &'a RawSite<'a>) -> Vec<(&'a str, Option<&'a str>)> {
    if site.kind == SiteKind::Draw {
        let deck = draw_deck(site);
        let mut mints: Vec<(&'a str, Option<&'a str>)> =
            deck.order.iter().map(|c| (c.name.as_str(), Some(c.kind.as_str()))).collect();
        if let Some(dc) = &deck.defaultcard {
            mints.push((dc.name.as_str(), Some(dc.kind.as_str())));
        }
        mints
    } else {
        vec![(site.name.as_str(), site.kind_override.as_deref())]
    }
}

/// The DRAW site's deck — a draw naming an unknown deck is a load error
/// (`check_named_ref`) and such sites never enter
/// the census, so a deckless draw site is unreachable.
pub(crate) fn draw_deck<'c>(site: &RawSite<'c>) -> &'c CompiledDeck {
    match site.deck {
        Some(d) => d,
        None => unreachable!("draw sites with unknown decks never enter the census"),
    }
}

/// One visit of the census
/// walk. The REFUSE/throwing flag marking
/// stays in [`walk_sites`] (it is part of the shared traversal's contract).
fn record_visit<'c, 'a>(
    st: &mut WalkState<'c>,
    o: &Outcome,
    frame: &WalkFrame,
    cx: &RecordCx<'c, 'a>,
    set_sites: &mut Vec<SetSite>,
    destroyed_by: &mut IndexMap<String, IndexSet<String>>,
) {
    let content = cx.content;
    let owner = cx.owner;
    let verb_idx = cx.verb_idx;
    let when = cx.when;
    let shadowed = cx.shadowed;
    let own_slot = cx.own_slot;
    let seg = match frame.segs.last() {
        Some(&s) => s,
        None => unreachable!("every WalkFrame carries at least seg 0"),
    };
    let pos = st.op_counter;
    st.op_counter += 1;
    let path = &frame.path;
    let segs = &frame.segs;
    let sites = &mut st.sites;
    let kills = &mut st.kills;

    // §6.7 chain ops: the op joins the walk-order chain_ops log with its pin
    // capability (a name-form candidate the global table resolves — load-
    // checked, so a named candidate always resolves; a when-less default
    // candidate is always live, but the crude "any named candidate" rule is
    // the honest MAY-pin test — the busy/max/gate skips are not statically
    // resolvable, so "capable" not "guaranteed" is what the refusal rung
    // needs, and over-demotion costs precision only)
    if o.op == "RUN" || o.op == "FORK" {
        let mut targets: Vec<String> = Vec::new();
        let capable = o.args.iter().any(|a| match a {
            Arg::Ref { r#ref: Ref::Name { name, .. } }
                if content.recipe_verbs.contains_key(name.as_str()) =>
            {
                targets.push(name.clone());
                true
            }
            _ => false,
        });
        st.chain_ops.push(capable);
        st.chain_targets.push(targets);
    }

    // a name-form first arg kills that name
    let kill_name: Option<&str> = match o.args.first() {
        Some(Arg::Ref { r#ref: Ref::Name { name, .. } }) => Some(name),
        _ => None,
    };
    if o.op == "DESTROY" || o.op == "REPLACE" {
        if let Some(name) = kill_name {
            kills.push(Kill { seg, pos, name: name.to_string(), throws: o.op == "REPLACE" });
            destroyed_by.entry(name.to_string()).or_default().insert(owner.to_string());
        }
    }
    if o.op == "SET" {
        // SET(ref|role, key, value) — the key names the written path
        // ('aspect:X' or an attr); the target tells WHERE it can land
        let key: Option<String> = match o.args.get(1) {
            Some(Arg::Key { v }) | Some(Arg::Str { v }) => Some(v.clone()),
            Some(Arg::Ref { r#ref: Ref::Name { name, .. } }) => Some(name.clone()),
            _ => None,
        };
        if let Some(key) = key {
            let target_arg = o.args.first();
            let target_name = match target_arg {
                Some(Arg::Ref { r#ref: Ref::Name { name, .. } }) => Some(name.clone()),
                _ => None,
            };
            let target_role = match target_arg {
                Some(Arg::Role { role }) => Some(role.as_str().to_string()),
                _ => None,
            };
            // v0.10.0: the write parks its target — key `state.parked`,
            // value the exact boolean `true` (the flag's one true face)
            let parks_target =
                key == "state.parked" && matches!(o.args.get(2), Some(Arg::Bool { v: true }));
            set_sites.push(SetSite {
                key,
                owner: owner.to_string(),
                verb_name: verb_idx.map(|vi| match content.verbs.get_index(vi) {
                    Some((name, _)) => name.clone(),
                    None => unreachable!("verb index {vi} came from content.verbs enumeration"),
                }),
                recipe: cx.recipe_idx,
                target_name,
                target_role,
                seg,
                pos,
                segs: segs.clone(),
                shadowed,
                refused: false, // annotated after the walk, once dead_segs are known
                path_non_static: path_runtime_gated(path),
                when_pred: when.cloned(),
                owner_slot: own_slot,
                parks_target,
            });
        }
    }
    let aspects_pair = o.args.iter().find(|a| matches!(a, Arg::Pair { k, .. } if k == "aspects"));
    let aspects_json = match aspects_pair {
        Some(Arg::Pair { v: crate::core::types::AttrValue::Str(v), .. }) => Some(v.clone()),
        _ => None,
    };
    let spawn = spawn_name_ref(o);
    let replace = replace_with_ref(o);
    let draw = draw_deck_id(o);
    let site: Option<RawSite<'c>> = if let Some((name, kind)) = spawn {
        let mut site = base_site(SiteKind::Spawn, name, cx, frame, seg, pos);
        site.kind_override = kind.map(str::to_string);
        site.aspects_json = aspects_json;
        site.derives = content
            .prototypes
            .get(name)
            .and_then(|p| p.derive.as_ref())
            .and_then(|d| d.name_from.as_deref())
            == Some("object");
        // v0.10.0: the authored `parked` mint modifier — the product lands in
        // the parked pool (§13.1)
        site.parks = o.args.iter().any(|a| matches!(a, Arg::Parked));
        Some(site)
    } else if let Some(name) = replace {
        let mut site = base_site(SiteKind::Replace, name, cx, frame, seg, pos);
        site.replace_target = match o.args.first() {
            Some(Arg::Ref { r#ref: Ref::Name { name, .. } }) => Some(name.clone()),
            _ => None,
        };
        Some(site)
    } else {
        // (clippy manual_map: Option::map over the drawn deck id)
        draw.map(|id| {
            let mut site = base_site(SiteKind::Draw, id, cx, frame, seg, pos);
            site.deck = content.decks.get(id);
            // a draw's `name` is the DECK id, never a mint name, so the
            // runtime_gated mint-collision rules cannot apply to it —
            // runtime_gated stays false by design
            site.runtime_gated = false;
            site
        })
    };
    let Some(site) = site else {
        return;
    };
    // a DRAW naming an unknown deck is a load error
    // (`check_named_ref`), so deck can only be None for hand-built content
    if site.kind == SiteKind::Draw && site.deck.is_none() {
        return;
    }
    sites.push(site);
}

/// The site skeleton shared by every producer kind: the walk position and
/// the owning-list context. The per-kind arms in [`record_visit`] overwrite
/// the kind-specific fields (`kind_override`, `aspects_json`, `deck`,
/// `replace_target`, `derives`).
fn base_site<'c>(
    kind: SiteKind,
    name: &str,
    cx: &RecordCx<'c, '_>,
    frame: &WalkFrame,
    seg: usize,
    pos: i64,
) -> RawSite<'c> {
    let owner = cx.owner.to_string();
    RawSite {
        kind,
        name: name.to_string(),
        kind_override: None,
        deck: None,
        aspects_json: None,
        owner: owner.clone(),
        verb: cx.verb_idx,
        recipe: cx.recipe_idx,
        when: cx.when.cloned(),
        shadow_risk: cx.shadow_risk,
        path: frame.path.clone(),
        seg,
        pos,
        segs: frame.segs.clone(),
        fallback: frame.fallback,
        runtime_gated: path_runtime_gated(&frame.path),
        throw_ops: 0,
        cap: 0,
        replace_target: None,
        killed_in_list: false,
        dup_risk: false,
        chain_refusal: false,
        collision_refusal: false,
        unique_skipped: false,
        group_sources: Vec::new(),
        target_killed_in_list: false,
        list_gate_mints: Rc::new(IndexSet::new()),
        list_stack_mints: Rc::new(IndexSet::new()),
        derives: false,
        parks: false,
        parked_in_list: false,
        where_: owner,
    }
}

/// Whether any IF condition on a site's path is non-static (roll- or
/// role-dependent) — such a path can be durably false.
fn path_runtime_gated(path: &[Pred]) -> bool {
    path.iter().any(|p| !pred_static(p))
}
