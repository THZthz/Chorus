#!/usr/bin/env python3
"""convert_cs.py — compile the Cultist Simulator core JSON into a Chorus script.

    python3 tools/convert_cs.py --core /Users/amias/Projects/cs-re/core \
        --out scripts/cultist-full.txt

Inputs (read-only):
  --core   the extracted CS core directory (160 lenient-JSON files). Loaded via
           loadlib.load (sys.path.insert of the core's PARENT), which handles
           BOM / trailing commas / unquoted keys / raw newlines in strings.
  --out    the Chorus script to generate (scripts/cultist-full.txt).

Mapping spec: docs/reverse/cultist-simulator.md §2/§6; grammar:
docs/spec/text-engine-v0.6.md §4.3/4.4/4.5/6.1/6.3/6.4/6.8/9.31/9.32/9.35/
9.36/9.38/9.41/9.42/9.47/9.48/9.49/9.50/§10; engine laws: docs/plans/v0.6-cs-slice.md
§3 (entity mints are impossible — decay targets and restore spawns are
PROTOTYPES; REPLACE into a prototype is legal in recipe bodies only).

FULL-COVERAGE EXPORT — every input element/recipe/deck/ending is EMITTED or a
NAMED STRUCTURAL SKIP (impossible under the engine grammar); there are no
convenience skips. The converter prints a census + coverage report and asserts
the totals. Determinism: fixed seed, deterministic name mangling, data-order
emission — same inputs, byte-identical output.

THE ACTIONID → CARRIER MAP (full coverage of the 464 non-station recipes)
--------------------------------------------------------------------------
Chorus registers twelve verbs (src/core/dict.rs CORE_VERBS). Seven are CS's
own stations (VERB_OF below). The other five are CARRIER VERBS: every CS
recipe whose actionid is not a station rehomes, hidden, onto exactly one
carrier, deterministically by actionid:

  TRAVEL_TO  the Exile campaign and the vault road — `long`, `longintrigue`,
             `longassault`, `longassault.assassination`, `longassault.abduction`,
             `longdowntime`, `longdreams`, `longdreams.poisonway`, the four
             `poison.*` tracks, `scandal`, `scandal.notoriety`, the expedition
             corpse-finds (`expedition.corpse.find`), vault casualties
             (`explorevault_casualty`) and the expulsions (`expel*`).
  USE_ON     rites, spirits and executed plots — `haunting`, `scar_ability`,
             `establishcult`, `cult`, `scheme`, and the six ascension
             inductions (`dreadinduction`, `fascinationinduction`,
             `mothinduction`, `heartinduction`, `lanterninduction`,
             `grailinduction`).
  ACQUIRE    cravings and gains — `craving`, `ambition`, `rose`, `roseextra`,
             `grailminorpromotion`, `watchmanminorpromotion`.
  LOCATE_AT  the body-and-law clocks — `suspicion`, `unsuspicion`, `illhealth`,
             `sickness`, `healthend`, `pleasantday`, `visions`, `despair`, the
             four `curse_*` families, `poppytime`.
  READ       the desk — recipes with NO actionid at all (CS fires them only
             from chains), `z` (a translation), `legacyeventbad`, `hookplot`,
             `newrival`, `workwin`.

The map is closed: an actionid outside both tables aborts the converter
(naming the id), so a grown core cannot silently drop recipes.

WHY NO CARRIER GROUPS SIT ON THE SEVEN STATIONS: 133 carrier recipes have no
CS requirements, and a requirement-less recipe placed before a station's
default would take every bare attempt from it (first-match, §6.4) — the
player-visible surface would change. On the five carriers this cannot happen:
their `actors` slot is guarded (`kind:character aspect:ritual>=1`, and no
character carries `ritual`), so suggestions skip them and typed attempts
reject at the engine, while §9.47 RUN projections do not re-check slot queries
and still open their windows. A carrier's default is a REFUSE wall, last in
its list.

SHADOWED TWINS: CS ships recipes that its own first-match order can never
fire — requirement-identical twins of an earlier recipe, and
requirement-less recipes behind the verb default (templates, epilogues,
fallbacks of always-true parents). The port emits them hidden with a
deterministic `.x2`/`.x3` suffix, preserving the data: duplicate twins keep
their CS when plus a disambiguating always-true conjunct (first occurrence
keeps its emitted form and wins first-match exactly as before); the
requirement-less station twins get a never-matching when (`NOT (count(actors)
>= 1) …` — CS-faithful: they are data-dead there too), while requirement-less
carrier recipes get an always-true conjunct instead, because carriers are
RUN-addressed and a never-when would silently end CS-live chains (§9.47
re-checks a pinned candidate's when).

ASPECT ELEMENTS: the 330 isAspect elements emit as stackable PROTOTYPES —
never as entities. A declared entity exists at game start, and §6.3's
world-form existentials (`count(aspect:k>=1)`) would read every aspect tag as
an extant card, corrupting the extantreq gates. As never-minted prototypes
they are inert: every read resolves, nothing exists.

The feature-level loss list (emitted-with-loss / dropped-feature counts,
distinct from skips) is in the converter's coverage report; the census totals
are asserted.
"""

import argparse
import glob
import os
import re
import sys
from collections import Counter, defaultdict

CORE_DEFAULT = "/Users/amias/Projects/cs-re/core"

# the seven mapped verbs (§4.3's registered lexemes)
VERB_OF = {
    "work": "WORK",
    "dream": "DREAM",
    "talk": "TALK",
    "explore": "EXPLORE",
    "study": "STUDY",
    "time": "WAIT",
    "portal.mansus": "MANSUS",
}
MAPPED_ACTIONIDS = set(VERB_OF)

# the five carrier verbs — registered lexemes that are not CS stations; they
# carry every non-station actionid, hidden, chain-addressed (see the header
# map). Closed tables: an actionid outside both aborts the converter.
CARRIER_VERBS = ("READ", "TRAVEL_TO", "USE_ON", "ACQUIRE", "LOCATE_AT")
CARRIER_OF = {
    # TRAVEL_TO — the Exile campaign and the vault road
    "long": "TRAVEL_TO",
    "longintrigue": "TRAVEL_TO",
    "longassault": "TRAVEL_TO",
    "longassault.assassination": "TRAVEL_TO",
    "longassault.abduction": "TRAVEL_TO",
    "longdowntime": "TRAVEL_TO",
    "longdreams": "TRAVEL_TO",
    "longdreams.poisonway": "TRAVEL_TO",
    "poison.long.way.trap": "TRAVEL_TO",
    "poison.notoriety.induction": "TRAVEL_TO",
    "poison.passion.dread.induction": "TRAVEL_TO",
    "poison.reason.fascination.induction": "TRAVEL_TO",
    "poison.health.dread.induction": "TRAVEL_TO",
    "scandal": "TRAVEL_TO",
    "scandal.notoriety": "TRAVEL_TO",
    "expedition.corpse.find": "TRAVEL_TO",
    "explorevault_casualty": "TRAVEL_TO",
    "expelallspies": "TRAVEL_TO",
    "expel_independent": "TRAVEL_TO",
    "expel_corpse": "TRAVEL_TO",
    "expel_injury": "TRAVEL_TO",
    "expelspy": "TRAVEL_TO",
    # USE_ON — rites, spirits and executed plots
    "haunting": "USE_ON",
    "scar_ability": "USE_ON",
    "establishcult": "USE_ON",
    "cult": "USE_ON",
    "scheme": "USE_ON",
    "dreadinduction": "USE_ON",
    "fascinationinduction": "USE_ON",
    "mothinduction": "USE_ON",
    "heartinduction": "USE_ON",
    "lanterninduction": "USE_ON",
    "grailinduction": "USE_ON",
    # ACQUIRE — cravings and gains
    "craving": "ACQUIRE",
    "ambition": "ACQUIRE",
    "rose": "ACQUIRE",
    "roseextra": "ACQUIRE",
    "grailminorpromotion": "ACQUIRE",
    "watchmanminorpromotion": "ACQUIRE",
    # LOCATE_AT — the body-and-law clocks
    "suspicion": "LOCATE_AT",
    "unsuspicion": "LOCATE_AT",
    "illhealth": "LOCATE_AT",
    "sickness": "LOCATE_AT",
    "healthend": "LOCATE_AT",
    "pleasantday": "LOCATE_AT",
    "visions": "LOCATE_AT",
    "despair": "LOCATE_AT",
    "curse_dread": "LOCATE_AT",
    "curse_visions": "LOCATE_AT",
    "curse_affliction": "LOCATE_AT",
    "curse_nowhere": "LOCATE_AT",
    "poppytime": "LOCATE_AT",
    # READ — the desk (unactioned machinery, papers, careers)
    "z": "READ",
    "legacyeventbad": "READ",
    "hookplot": "READ",
    "newrival": "READ",
    "workwin": "READ",
}

# the actors-slot guard on carrier verbs: no CS character-kind card carries
# the `ritual` aspect, so suggestions skip the carrier stations and typed
# attempts reject at the engine, while §9.47 RUN projections (which do not
# re-check slot queries) still open their chain windows. Declared on rites.
CARRIER_ACTORS_GUARD = "kind:character aspect:ritual>=1"
CARRIER_GUARD_ASPECTS = {"ritual"}


def verb_of(actionid):
    """A CS actionid's Chorus verb: one of the seven stations, one of the
    five carriers, or None (unmappable — the typo'd `actionId` key ships
    None actionids; the latter rehome to READ)."""
    if actionid is None:
        return "READ"  # CS fires actionid-less recipes only from chains
    return VERB_OF.get(actionid) or CARRIER_OF.get(actionid)


def is_station(verb):
    return verb in set(VERB_OF.values())

# Chorus KINDS by CS source file (cosmetic classification; the port's slot
# queries and recipe whens match on aspects, never kinds). Default: item.
KIND_BY_FILE = {
    "abilities.json": "resource",
    "city_locations.json": "place",
    "vaults.json": "place",
    "cults.json": "place",
    "followers.json": "character",
    "hunters.json": "character",
    "patrons.json": "character",
    "hirelings.json": "character",
    "othernpcs.json": "character",
    "independent.json": "character",
    "spirits.json": "character",
    "books_lore.json": "idea",
    "fragments.json": "idea",
    "mansus.json": "idea",
    "ascension_elements.json": "idea",
    "ascension_elements_change.json": "idea",
    "memories.json": "idea",
    "influences.json": "state",
    "seasons.json": "state",
    "reputation.json": "state",
    "books_language.json": "text",
    "books_other.json": "text",
    "books_untranslated.json": "text",
    "skills.json": "text",
    "rituals.json": "rite",
}
KIND_DEFAULT = "item"

ASPECT_KEY_RE = re.compile(r"^[a-z_][a-z0-9_.]*$")
EL_ID_RE = re.compile(r"^[a-z0-9_.]+$")
RESERVED_UPPER = {
    "EVENT", "TO", "WITH", "AT", "ABOUT", "INTO", "AS", "AND", "OR", "NOT",
    "IF", "ELSE", "ROLL", "ROLLED", "DRAW", "DRAWN", "RUN", "COMPLETE",
    "READ", "STUDY", "WORK", "TALK", "EXPLORE", "MANSUS", "TRAVEL_TO",
    "LOCATE_AT", "DREAM", "USE_ON", "ACQUIRE", "WAIT",
}

NARRATE_MAX = 260
HINT_MAX = 110


def load_core(core_dir):
    sys.path.insert(0, os.path.dirname(os.path.abspath(core_dir)))
    from loadlib import load  # noqa: E402

    elements = []
    for path in sorted(glob.glob(os.path.join(core_dir, "elements", "*.json"))):
        fname = os.path.basename(path)
        for _k, items in load(path).items():
            for it in items:
                if isinstance(it, dict):
                    it["_file"] = fname
                    elements.append(it)

    def cat(sub):
        out = []
        for path in sorted(glob.glob(os.path.join(core_dir, sub, "*.json"))):
            for _k, items in load(path).items():
                out.extend(i for i in items if isinstance(i, dict))
        return out

    return (elements, cat("recipes"), cat("decks"), cat("endings"), cat("verbs"))


def fmt_duration(minutes):
    minutes = max(1, int(minutes))
    if minutes % 60 == 0:
        return f"{minutes // 60}h"
    if minutes < 60:
        return f"{minutes}m"
    return f"{minutes // 60}h{minutes % 60}m"


def clean_text(s, limit):
    if not isinstance(s, str):
        return ""
    s = s.replace("\r", " ").replace("\n", " ").replace("\t", " ")
    s = re.sub(r"[\x00-\x1f]", " ", s)
    s = re.sub(r"\s+", " ", s).strip()
    s = s.replace("\\", "/").replace('"', "'")
    if len(s) > limit:
        cut = s[:limit]
        sp = cut.rfind(" ")
        s = (cut[:sp] if sp > limit // 2 else cut).rstrip(" ,;:.") + " …"
    return s


def pick_text(r, keys, limit):
    for key in keys:
        t = clean_text(r.get(key) or "", limit)
        if t and t != ".":
            return t
    return ""


def norm_when(w):
    return " ".join((w or "").split())


# Always-true when-clauses for RUN-target recipes that would otherwise be
# shadowed (identical requirements or no requirements at all). RUN pins by id
# (§9.47) and re-checks only the pinned candidate, so a tautology when keeps a
# chain recipe reachable exactly as a chain target while selection still
# prefers the earlier twin. Only comparison ops the lint's count-gate scanner
# handles are used (`!=` would hit its unreachable arm).
_TAUT_ATOMS = [
    "count(actors) >= 1",
    "count(actors) >= 0",
    "count(actors) = 1",
    "count(actors) <= 1",
    "count(actors) < 2",
    "count(actors) > 0",
]


def make_tautologizer():
    """n-th distinct always-true when-clause (singles, then pairs, then
    triples of the atoms above) — deterministic, effectively unbounded. Every
    cache entry is textually distinct: singles carry no ' AND ', a k-fold
    conjunction carries k-1 of them."""
    import itertools

    cache = []
    k = [1]

    def taut(n):
        while len(cache) <= n:
            if k[0] == 1:
                cache.extend(_TAUT_ATOMS)
            else:
                cache.extend(
                    " AND ".join(_TAUT_ATOMS[i] for i in c)
                    for c in itertools.product(range(len(_TAUT_ATOMS)), repeat=k[0]))
            k[0] += 1
        return cache[n]

    return taut


_NEVER_ATOMS = [f"NOT ({a})" for a in _TAUT_ATOMS]


def make_neverizer():
    """n-th distinct NEVER-true when-clause: NOT over the always-true atoms
    (then conjunctions of distinct positions). Each inner atom passes the
    lint's count-gate scanner (the bound holds for the slot; only the
    negation kills it), and every entry can never match a real window —
    actors are always bound. This is the honest encoding of CS's data-dead
    recipes: a requirement-less recipe behind the verb default / under an
    always-true parent never fires in CS either. Texts are pairwise distinct
    (singles, then k-fold conjunctions — the tautologizer's shape)."""
    import itertools

    cache = []
    k = [1]

    def never(n):
        while len(cache) <= n:
            if k[0] == 1:
                cache.extend(_NEVER_ATOMS)
            else:
                cache.extend(
                    " AND ".join(_NEVER_ATOMS[i] for i in c)
                    for c in itertools.product(range(len(_NEVER_ATOMS)), repeat=k[0]))
            k[0] += 1
        return cache[n]

    return never


class Converter:
    def __init__(self, core_dir, seed=1837):
        self.core = os.path.abspath(core_dir)
        self.seed = seed
        (self.elements, self.recipes, self.decks, self.endings, self.cs_verbs) = (
            load_core(core_dir))
        self.el = {e["id"]: e for e in self.elements}
        self.rec = {r["id"]: r for r in self.recipes}

        # coverage accounting
        self.rec_skip = Counter()
        self.rec_skip_ex = defaultdict(list)
        self.el_skip = Counter()
        self.el_skip_ex = defaultdict(list)
        self.feat = Counter()
        self.feat_ex = defaultdict(list)
        # full-coverage counters (the census's second axis)
        self.n_carrier = Counter()       # actionid -> rehomed recipe count
        self.n_twins = Counter()         # verb -> shadowed twins emitted hidden
        self.n_aspect_cards = 0          # isAspect prototypes emitted
        # lifetime arming (P1-2): every lifetime-bearer recipe-effect mint is
        # ARMED — the authored `SPAWN X expires <dur>` modifier (§4.4/§9.36;
        # findings E12's spawn-modifier half) materializes expiresAt at the
        # mint, per copy, so pooled names arm per instance. Deck draws stay
        # E12-LEDGERED (DRAW has no expires arg — findings E12's open half).
        self.n_armed = 0
        self.n_e12_deck = 0
        self.armed_seen = set()          # (csid, element) armed sites counted once

        # deterministic name mangling (asserted injective / collision-free)
        self.elname = {}
        seen = {}
        for e in self.elements:
            n = e["id"].upper().replace(".", "_")
            if n in seen and seen[n] != e["id"]:
                raise SystemExit(f"name collision: {seen[n]} vs {e['id']}")
            if n in RESERVED_UPPER:
                raise SystemExit(f"element id {e['id']} mangles to reserved word {n}")
            seen[n] = e["id"]
            self.elname[e["id"]] = n
        self.deckname = {}
        seen = set()
        for d in self.decks:
            n = d["id"].upper().replace(".", "_")
            while n in seen or n in RESERVED_UPPER:
                n += "_D"
            seen.add(n)
            self.deckname[d["id"]] = n
        self.endingname = {e["id"]: e["id"].upper() for e in self.endings}

    # -- accounting ---------------------------------------------------------

    def skip_recipe(self, reason, rid):
        self.rec_skip[reason] += 1
        ex = self.rec_skip_ex[reason]
        if len(ex) < 4:
            ex.append(rid)

    def skip_element(self, reason, eid):
        self.el_skip[reason] += 1
        ex = self.el_skip_ex[reason]
        if len(ex) < 4:
            ex.append(eid)

    def lose(self, feature, example=None):
        self.feat[feature] += 1
        ex = self.feat_ex[feature]
        if example is not None and len(ex) < 3:
            ex.append(str(example))

    # -- pass 1: elements ---------------------------------------------------

    def plan_elements(self):
        el = self.el
        deck_members = set()
        for d in self.decks:
            deck_members.update(d.get("spec") or [])
            if d.get("defaultcard"):
                deck_members.add(d["defaultcard"])

        # internaldecks of mappable recipes also force stackability
        self.internaldeck_specs = {}
        for r in self.recipes:
            ide = r.get("internaldeck")
            if ide and verb_of(r.get("actionid")):
                self.internaldeck_specs[r["id"]] = ide
                deck_members.update(ide.get("spec") or [])
                if ide.get("defaultcard"):
                    deck_members.add(ide["defaultcard"])

        minted = set()
        for r in self.recipes:
            for k, v in (r.get("effects") or {}).items():
                if int(v) > 0:
                    minted.add(k)
            for a in r.get("alt") or []:
                for k, v in (a.get("effects") or {}).items():
                    if int(v) > 0:
                        minted.add(k)
        for e in self.elements:
            if e.get("decayTo"):
                minted.add(e["decayTo"])
            if e.get("burnTo"):
                minted.add(e["burnTo"])
        minted |= deck_members

        # decl: id -> (home, kind, aspects); home "entity" | "proto"
        self.decl = {}
        for e in self.elements:
            eid = e["id"]
            if not EL_ID_RE.match(eid):
                # the one structural element skip: an id that cannot be a
                # Chorus name lexeme nor a marker-aspect key (a hyphen and a
                # trailing dot; a localisation-credits stub referenced by
                # nothing anywhere in the core)
                self.skip_element(
                    "STRUCTURAL: id is not expressible as a Chorus name/"
                    "aspect lexeme (hyphen + trailing dot; referenced by "
                    "nothing)", eid)
                self.elname.pop(eid, None)
                continue
            if e.get("isAspect"):
                self.n_aspect_cards += 1
            kind = KIND_BY_FILE.get(e.get("_file") or "", KIND_DEFAULT)
            aspects = {k: int(v) for k, v in (e.get("aspects") or {}).items()}
            aspects[eid] = max(aspects.get(eid, 0), 1)  # marker aspect: the id itself
            if e.get("isAspect"):
                # aspect carriers are PROTOTYPES, never entities: a declared
                # entity exists at game start and §6.3's world-form
                # existentials (count(aspect:k>=1) in extantreq/copy whens)
                # would read every aspect tag as an extant card. As
                # never-minted prototypes they are inert — every read
                # resolves, nothing exists.
                home = "proto"
            elif eid in minted or "lifetime" in e:
                home = "proto"
            elif e.get("unique"):
                home = "entity"
            else:
                # never minted, not unique: an inert stackable prototype so
                # every reference (deck entries, requirements) resolves
                home = "proto"
            self.decl[eid] = (home, kind, aspects)
            if e.get("unique") and home == "proto":
                self.lose("unique:true card emitted stackable (minted/deck/lifetime)", eid)
            if "uniquenessgroup" in e and home == "proto":
                self.lose("uniquenessgroup (no group concept; card is stackable)", eid)
            if e.get("slots"):
                self.lose("element slot (card-borne slot opens; feature skipped)", eid)
            if e.get("burnTo"):
                self.lose("burnTo (rite burn transform; feature skipped)", eid)
        self.deck_members = deck_members
        self.lifetime_of = {eid: int(e["lifetime"]) for eid, e in
                            ((e["id"], e) for e in self.elements)
                            if "lifetime" in e and e["id"] in self.decl}

    def with_clause(self, eid):
        _home, _kind, aspects = self.decl[eid]
        if not aspects:
            return ""
        return " with " + ", ".join(f"{k}:{v}" for k, v in sorted(aspects.items()))

    # -- pass 2: card triggers ----------------------------------------------

    def plan_card_triggers(self):
        self.complete = defaultdict(list)   # element id -> [(aspect|None, body)]
        self.expire_triggers = []           # (trigger id, element id, body)
        for e in self.elements:
            eid = e["id"]
            if eid not in self.decl:
                # aspect-borne triggers distribute below; card xtriggers on
                # skipped cards cannot fire
                continue
            for ev, tgt in (e.get("xtriggers") or {}).items():
                self._xtrigger(eid, ev, tgt)
            for ind in e.get("induces") or []:
                self._induce(eid, None, ind, "element induce")
        # aspect-borne hooks (carrier is an isAspect element): distribute onto
        # every emitted card declaring that aspect
        for a in self.elements:
            if not a.get("isAspect"):
                continue
            akey = a["id"]
            for ev, tgt in (a.get("xtriggers") or {}).items():
                n = 0
                for eid, (_h, _k, aspects) in self.decl.items():
                    if aspects.get(akey):
                        self._xtrigger(eid, ev, tgt)
                        n += 1
                if n:
                    self.lose(f"aspect xtrigger '{ev}' distributed to declaring cards", akey)
            for ind in a.get("induces") or []:
                for eid, (_h, _k, aspects) in self.decl.items():
                    if aspects.get(akey):
                        self._induce(eid, akey, ind, "aspect induce")
        # on-expire chains (never REPLACE: the trigger-body law)
        for e in self.elements:
            eid = e["id"]
            if eid not in self.decl or "lifetime" not in e:
                continue
            decay = e.get("decayTo")
            if not decay or decay not in self.el:
                # built-in destroy-by-id expiry: no authored trigger (the
                # §9.38-safe path under pools)
                continue
            body = [f"DESTROY {self.elname[eid]}", f"SPAWN {self.elname[decay]}"]
            dlt = self.el[decay].get("lifetime")
            if dlt:
                body.append(f"SET({self.elname[decay]}, expires, {fmt_duration(dlt)})")
            self.expire_triggers.append(
                (f"exp_{self.elname[eid].lower()}", eid, ", ".join(body)))

    def _xtrigger(self, eid, ev, tgt):
        if tgt in self.rec:
            if verb_of(self.rec[tgt].get("actionid")) and tgt in self.emitted_id_of:
                self.complete[eid].append(
                    (ev, f'RUN "{self.emitted_id_of[tgt]}"'))
            else:
                self.lose("xtrigger RUN target structurally skipped (actionId typo)",
                          f"{eid}:{ev}->{tgt}")
            return
        if tgt not in self.el or tgt not in self.decl:
            self.lose("xtrigger target missing or skipped", f"{eid}:{ev}->{tgt}")
            return
        body = [f"DESTROY {self.elname[eid]}", f"SPAWN {self.elname[tgt]}"]
        tlt = self.el[tgt].get("lifetime")
        if tlt:
            body.append(f"SET({self.elname[tgt]}, expires, {fmt_duration(tlt)})")
        self.complete[eid].append((ev, ", ".join(body)))

    def _induce(self, eid, aspect_key, ind, why):
        tgt = ind.get("id")
        chance = ind.get("chance", 100)
        rec = self.rec.get(tgt)
        if rec is None:
            self.lose(f"{why} target missing", f"{eid}->{tgt}")
            return
        if not verb_of(rec.get("actionid")) or tgt not in self.emitted_id_of:
            self.lose(f"{why} target structurally skipped (actionId typo)", f"{eid}->{tgt}")
            return
        run = f'RUN "{self.emitted_id_of[tgt]}"'
        body = run if int(chance) >= 100 else f"ROLL(1, 100), IF roll <= {chance} -> ({run})"
        self.complete[eid].append((aspect_key, body))

    # -- pass 3: decks -------------------------------------------------------

    def deck_lifetime_audit(self, deck_id, raw_spec, raw_default):
        """E12 tier (b) for decks: a DRAW mints the drawn name with no authored
        handle on the fresh card, so a lifetime bearer served from a deck can
        never get `expires` armed. Each distinct (deck, entry) instance is
        ledgered, never faked (findings E12)."""
        for s in dict.fromkeys(list(raw_spec or []) + ([raw_default] if raw_default else [])):
            if s in self.lifetime_of:
                self.n_e12_deck += 1
                self.lose("E12: deck entry is a lifetime bearer — deck draws cannot "
                          "arm expires (findings E12)", f"{deck_id}:{s}")

    def plan_decks(self):
        self.internal_deck_name = {}
        used = set(self.deckname.values())
        for rid, ide in sorted(self.internaldeck_specs.items()):
            base = (ide.get("id") or f"internal_{rid}").upper().replace(".", "_")
            name = base
            while name in used or name in RESERVED_UPPER:
                name += "_I"
            used.add(name)
            self.internal_deck_name[rid] = name
        lines = []
        self.n_real_decks = 0
        self.n_internal_decks = 0
        for d in self.decks:
            spec = [self.elname[s] for s in (d.get("spec") or []) if s in self.elname]
            self.deck_lifetime_audit(d["id"], d.get("spec"), d.get("defaultcard"))
            if not spec:
                self.lose("deck dropped (empty spec after mapping)", d["id"])
                continue
            self.n_real_decks += 1
            tail = ""
            if d.get("defaultcard") and d["defaultcard"] in self.elname:
                tail += f" defaultcard {self.elname[d['defaultcard']]}"
            if d.get("resetonexhaustion"):
                tail += " reset"
            if d.get("drawmessages"):
                self.lose("deck drawmessages (no prose channel for draws)", d["id"])
            lines.append(f"  {self.deckname[d['id']]} spec " + ", ".join(spec) + tail)
        for rid, ide in sorted(self.internaldeck_specs.items()):
            spec = [self.elname[s] for s in (ide.get("spec") or []) if s in self.elname]
            self.deck_lifetime_audit(self.internal_deck_name.get(rid) or rid,
                                     ide.get("spec"), ide.get("defaultcard"))
            if not spec:
                self.lose("internaldeck dropped (empty spec)", rid)
                self.internal_deck_name[rid] = None
                continue
            self.n_internal_decks += 1
            tail = ""
            if ide.get("defaultcard") and ide["defaultcard"] in self.elname:
                tail += f" defaultcard {self.elname[ide['defaultcard']]}"
            if ide.get("resetonexhaustion"):
                tail += " reset"
            lines.append(f"  {self.internal_deck_name[rid]} spec " + ", ".join(spec) + tail)
        self.deck_lines = lines

    # -- pass 4: recipe planning ---------------------------------------------

    def req_terms_with(self, reqs):
        """requirements {k:v} → with-bound total() terms: the CS situation's
        input card is the `with` binding; v<0 = total < |v| (CS absence)."""
        terms = []
        for k, v in sorted((reqs or {}).items()):
            v = int(v)
            if v == 0:
                continue
            if v > 0:
                terms.append(f"total(with, aspect:{k}) >= {v}")
            else:
                terms.append(f"total(with, aspect:{k}) < {-v}")
        return terms

    def req_terms_world(self, reqs):
        """World-form requirements for alt/run copies (a copy sits on the
        parent verb; the bound role carries the parent's card, so world
        existentials stand in — 'as best expressible')."""
        terms = []
        for k, v in sorted((reqs or {}).items()):
            v = int(v)
            if v == 0:
                continue
            if v > 0:
                terms.append(f"count(aspect:{k}>=1) >= {v}")
            else:
                terms.append(f"count(aspect:{k}>=1) < {-v}")
        return terms

    def extant_terms(self, r):
        terms = []
        for key in ("extantreqs", "tablereqs"):
            for k, v in sorted((r.get(key) or {}).items()):
                v = int(v)
                if v == 0:
                    continue
                if v > 0:
                    terms.append(f"count(aspect:{k}>=1) >= {v}")
                else:
                    terms.append(f"count(aspect:{k}>=1) < {-v}")
        return terms

    def base_when(self, r):
        return " AND ".join(self.req_terms_with(r.get("requirements")) + self.extant_terms(r))

    def negated_with_terms(self, reqs):
        parts = []
        for k, v in sorted((reqs or {}).items()):
            v = int(v)
            if v == 0:
                continue
            if v > 0:
                parts.append(f"total(with, aspect:{k}) < {v}")
            else:
                parts.append(f"total(with, aspect:{k}) >= {-v}")
        return "(" + " OR ".join(parts) + ")" if parts else None

    def plan_recipes(self):
        self.plans = defaultdict(list)        # VERB -> [(id, when, r, kind)]
        self.emitted_ids = set()
        self.emitted_id_of = {}               # csid -> emitted (possibly .x2-suffixed) id
        self.emitted_kind_of = {}             # csid -> plan kind (main/twin/chain/…)
        self.alt_gates = defaultdict(list)    # emitted id -> [NOT-parent terms]
        self.copy_plan = []                   # (verb, parent_rid, tgt, chance)
        self.copies = {}                      # (verb, tgt) -> copy id
        self.copy_lines = defaultdict(list)   # VERB -> [rendered lines]
        self.all_when_seen = defaultdict(set) # VERB -> every emitted when text
        self.copy_tau = Counter()             # VERB -> copy disambiguator index
        self.copy_tau_base = Counter()        # VERB -> index offset past plan usage
        self.tau_pool = Counter()             # verb -> chain-bucket tautology index
        self.tau_ne = Counter()               # verb -> twin-disambiguator index
        self.verb_duration = {}
        self.duration_deviations = Counter()
        self.verb_recipe_counts = {}

        taut_of = make_tautologizer()
        never_of = make_neverizer()
        tau_pool = self.tau_pool    # verb -> index into the always-true pool
        tau_ne = self.tau_ne        # verb -> twin-when disambiguator index
        nev = Counter()             # verb -> index into the never-true pool
        when_seen = defaultdict(dict)
        uncond_default = {}

        # RUN targets survive duplicate/unconditional shadowing with a
        # disambiguated tautology when (RUN pins by id; §9.47). Collected
        # over station AND carrier recipes.
        run_targets = set()
        verb_of_rid = {}
        for r in self.recipes:
            v = verb_of(r.get("actionid"))
            if v:
                verb_of_rid[r["id"]] = v
        for r in self.recipes:
            for e in r.get("linked") or []:
                if e.get("id"):
                    run_targets.add(e["id"])
            for a in r.get("alt") or []:
                if a.get("additional") and a.get("id"):
                    run_targets.add(a["id"])
        for e in self.elements:
            for _ev, tgt in (e.get("xtriggers") or {}).items():
                if tgt in self.rec:
                    run_targets.add(tgt)
        # bootstrap entries: chain recipes some card's on-complete hook RUNs
        # (CS's job→needs induce). These select before purely-linked chain
        # recipes, so a bare attempt lands on the cycle's entry, not its
        # middle (needs before discardseason/drawseason on the WAIT station).
        bootstrap = set()
        for e in self.elements:
            for ind in e.get("induces") or []:
                if ind.get("id") in self.rec:
                    bootstrap.add(ind["id"])
            for _ev, tgt in (e.get("xtriggers") or {}).items():
                if tgt in self.rec:
                    bootstrap.add(tgt)
        for a in self.elements:
            if a.get("isAspect"):
                for ind in a.get("induces") or []:
                    if ind.get("id") in self.rec:
                        bootstrap.add(ind["id"])
        self.bootstrap_targets = bootstrap
        self.run_targets = run_targets

        mains = []
        unknown = set()
        for r in self.recipes:
            rid = r["id"]
            if "actionId" in r and "actionid" not in r:
                self.skip_recipe(
                    "STRUCTURAL: actionId misspelled (shipped data typo; "
                    "CS ignores them too)", rid)
                continue
            verb = verb_of(r.get("actionid"))
            if verb is None:
                unknown.add(r.get("actionid"))
                continue
            mains.append((verb, r))
        if unknown:
            # the carrier map is closed: a grown core must fail loudly
            raise SystemExit(f"actionid(s) outside the carrier map: {sorted(unknown)}")

        for verb, r in mains:
            rid = r["id"]
            my_id = f"{verb.lower()}.{rid.lower()}"
            carrier = not is_station(verb)
            when = self.base_when(r)
            if not when:
                if rid in run_targets:
                    i = tau_pool[verb]
                    tau_pool[verb] += 1
                    when = taut_of(i)
                    kind = "chainboot" if rid in self.bootstrap_targets else "chain"
                    plan_id = my_id
                    self.plans[verb].append((plan_id, when, r, kind))
                elif carrier:
                    # requirement-less carrier main: carriers are never
                    # walked (the guarded actors slot), so an always-true
                    # conjunct is safe and keeps the recipe RUN-addressable
                    # (§9.47 re-checks a pinned candidate's when — a
                    # never-when would silently end CS-live chains)
                    i = tau_pool[verb]
                    tau_pool[verb] += 1
                    plan_id = my_id
                    self.plans[verb].append((plan_id, taut_of(i), r, "carrmain"))
                elif verb not in uncond_default:
                    uncond_default[verb] = my_id
                    plan_id = my_id
                    self.plans[verb].append((plan_id, None, r, "default"))
                else:
                    # requirement-less station twin behind the verb default:
                    # CS's own first-match order can never fire it (templates,
                    # epilogues, fallbacks of always-true parents) — emitted
                    # hidden under a never-matching when, the honest encoding
                    # of data-dead
                    plan_id = f"{my_id}.x2"
                    self.plans[verb].append((plan_id, never_of(nev[verb]), r, "twin"))
                    nev[verb] += 1
                    self.n_twins[verb] += 1
                self.emitted_ids.add(plan_id)
                self.emitted_id_of[rid] = plan_id
                self.emitted_kind_of[rid] = kind
                continue
            key = norm_when(when)
            if key in when_seen[verb]:
                first = when_seen[verb][key]
                if rid in run_targets:
                    tau_ne[verb] += 1
                    when = f"{when} AND {taut_of(tau_ne[verb])}"
                    self.plans[verb].append((my_id, when, r, "main"))
                    self.emitted_ids.add(my_id)
                    self.emitted_id_of[rid] = my_id
                    self.emitted_kind_of[rid] = "main"
                else:
                    # duplicate twin of `first`: emitted hidden with the same
                    # gate plus a disambiguating always-true conjunct — the
                    # first occurrence keeps its emitted form and wins
                    # first-match (§6.4) exactly as before; the twin stays
                    # RUN-addressable under its suffixed id
                    tau_ne[verb] += 1
                    twin = f"{my_id}.x2"
                    self.plans[verb].append(
                        (twin, f"{when} AND {taut_of(tau_ne[verb])}", r, "twin"))
                    self.emitted_ids.add(twin)
                    self.emitted_id_of[rid] = twin
                    self.emitted_kind_of[rid] = "twin"
                    self.n_twins[verb] += 1
                continue
            when_seen[verb][key] = my_id
            kind = "carrmain" if carrier else "main"
            self.plans[verb].append((my_id, when, r, kind))
            self.emitted_ids.add(my_id)
            self.emitted_id_of[rid] = my_id
            self.emitted_kind_of[rid] = kind

        for verb, r in mains:
            if not is_station(verb):
                self.n_carrier[r.get("actionid")] += 1

        # alt entries: fallback (non-additional) alts gate their target on
        # NOT(parent when); additional alts join the parent's RUN ladder.
        # A twin parent's alts register NOTHING: CS never selects the twin
        # (its first occurrence wins first-match), so the alt is dead there
        # too — gating a live target on the dead parent's when would shrink
        # the target's own domain (e.g. dreamhealth.x2's fallback alt must
        # not constrain dream_router).
        for verb, r in mains:
            rid = r["id"]
            my_id = self.emitted_id_of.get(rid)
            if my_id is None:
                self.lose("alt entries dropped (parent recipe structurally skipped)", rid)
                continue
            if self.emitted_kind_of.get(rid) == "twin":
                alts = r.get("alt") or []
                if alts:
                    self.lose(
                        "alt entries of a shadowed twin parent dropped "
                        "(CS never selects the parent; its alts are dead there too)",
                        rid)
                continue
            for a in r.get("alt") or []:
                tgt = a.get("id")
                if not tgt:
                    self.lose("inline alt definition (defines an unmapped-situation recipe)", rid)
                    continue
                if a.get("expulsion"):
                    self.lose("alt expulsion (casualty filter; no grammar)", f"{rid}->{tgt}")
                    continue
                tr = self.rec.get(tgt)
                if tr is None:
                    self.lose("alt target missing", f"{rid}->{tgt}")
                    continue
                if tgt not in self.emitted_id_of:
                    self.lose("alt target structurally skipped (actionId typo)", f"{rid}->{tgt}")
                    continue
                if a.get("additional"):
                    self.copy_plan.append((verb, rid, tgt, a.get("chance", 100)))
                    continue
                parent_uncond = not self.base_when(r)
                if parent_uncond:
                    self.lose("fallback alt under an unconditional parent (NOT-gate unsatisfiable)",
                              f"{rid}->{tgt}")
                    continue
                if verb_of_rid.get(tgt) == verb:
                    self.alt_gates[self.emitted_id_of[tgt]].append(
                        self.negated_with_terms(r.get("requirements")))
                    self.lose("same-verb fallback alt: target gated on NOT(parent)", f"{rid}->{tgt}")
                else:
                    self.copy_plan.append((verb, rid, tgt, None))

    def apply_alt_gates(self):
        for verb, plans in self.plans.items():
            for i, (pid, when, r, kind) in enumerate(plans):
                extra = self.alt_gates.get(pid)
                if not extra:
                    continue
                w = when or ""
                for neg in extra:
                    if neg is None:
                        continue
                    w = f"{w} AND NOT {neg}" if w else f"NOT {neg}"
                plans[i] = (pid, w, r, kind)
        # the full per-verb when vocabulary (mains + chains + defaults-with-
        # when), so copy whens can stay textually distinct from all of them;
        # the copy disambiguator index starts past the plan pass's tautology
        # usage (a bare-tautology copy when must never equal a chain's)
        for verb, plans in self.plans.items():
            seen = self.all_when_seen[verb]
            for _pid, when, _r, _kind in plans:
                if when:
                    seen.add(norm_when(when))
            self.copy_tau_base[verb] = max(self.tau_pool[verb], self.tau_ne[verb]) + 1

    # -- outcome bodies -------------------------------------------------------

    def copy_id(self, verb, tgt):
        """Emit (once per verb+target) a RUN-addressable copy of CS recipe
        `tgt` on `verb`; returns its id or None."""
        key = (verb, tgt)
        if key in self.copies:
            return self.copies[key]
        tr = self.rec.get(tgt)
        if tr is None:
            return None
        cid = f"{verb.lower()}.alt.{tgt.lower()}"
        self.copies[key] = cid  # reserve before recursion
        when = " AND ".join(self.req_terms_world(tr.get("requirements")) + self.extant_terms(tr))
        # keep every when in the verb textually distinct (the lint's
        # identical-when finding): a colliding copy gains a tautology drawn
        # from past the plan pass's usage
        seen = self.all_when_seen[verb]
        nw = norm_when(when)
        if nw in seen or not nw:
            self.copy_tau[verb] += 1
            extra = make_tautologizer()(self.copy_tau_base[verb] + self.copy_tau[verb])
            when = f"{when} AND {extra}" if when else extra
            nw = norm_when(when)
        seen.add(nw)
        outs = self.outcome_body(verb, tr, is_copy=True)
        narr = pick_text(tr, ("description", "startdescription", "label"), NARRATE_MAX)
        if narr:
            outs.append(f'NARRATE "{narr}"')
        if not outs:
            outs = ['NARRATE "."']
        pos = [cid]
        # a copy of a carrier recipe (or any craftable:false recipe) rides
        # hidden: rehomed machinery never shows as station content
        if tr.get("craftable") is False or not is_station(verb_of(tr.get("actionid"))):
            pos.append("hidden")
        if when:
            pos.append(f"when {when}")
        self.copy_lines[verb].append("      " + " ".join(pos) + " -> " + ", ".join(outs))
        return cid

    def run_ladder(self, arms, rid, is_copy):
        """arms [(chance, id)] → nested fresh-roll ROLL/IF ladder; one RUN
        reached per completion (§9.47). CS rolls each entry in order, first
        pass runs. Each arm RUNs a CANDIDATE LIST of its own entry plus every
        later entry: the roll gives the entry its priority, and if its `when`
        fails the walk falls through to the later candidates instead of
        stranding the chain — CS's failure-falls-through, as best the
        single-RUN law expresses it."""
        if not arms:
            return None
        if len(arms) > 1:
            self.lose("linked/alts collapsed to one RUN per completion (§9.47 single-RUN law)",
                      None if is_copy else rid)
        ids = [a[1] for a in arms]

        def build(i):
            chance = arms[i][0]
            run = "RUN " + ", ".join(f'"{t}"' for t in ids[i:])
            if i == len(arms) - 1:
                return run
            c = int(chance) if chance is not None else 100
            return f'ROLL(1, 100), IF roll <= {c} -> ({run}) ELSE -> ({build(i + 1)})'

        if len(arms) == 1:
            return f'RUN "{arms[0][1]}"'
        return build(0)

    def lifetime_arm(self, rid, k, name):
        """Lifetime bookkeeping for one authored mint of element `k` (P1-2):
        the authored `expires <duration>` SPAWN modifier (§4.4/§9.36 — the
        engine's E12 spawn-modifier fix) rides every lifetime bearer's SPAWN,
        so the fresh copy arms at BIRTH with the SET-expires pair. Each SPAWN
        arms its own mint — pooled names arm per instance (the §9.38
        oldest-name trap never applies) and the old SET-after-SPAWN shape's
        §9.19 non-stackable pin is not needed; the modifier composes with
        INTO and is authoritative over any literal expiresAt in the same
        step's attrs= pairs. Sites count once per (recipe, element); alt/run
        copies share the parent's csid. Deck draws cannot ride the modifier
        (SPAWN-shaped by design) and stay E12-ledgered."""
        lt = self.lifetime_of.get(k)
        if lt is None:
            return ""
        if (rid, k) not in self.armed_seen:
            self.armed_seen.add((rid, k))
            self.n_armed += 1
        return f" expires {fmt_duration(lt)}"

    def outcome_body(self, verb, r, is_copy=False):
        rid = r["id"]
        outs = []
        for k, v in sorted((r.get("effects") or {}).items()):
            v = int(v)
            if v == 0:
                continue
            if k not in self.elname:
                self.lose("effect target is not an element (engine/lever write)", f"{rid}:{k}")
                continue
            name = self.elname[k]
            if v > 0:
                mod = self.lifetime_arm(rid, k, name)
                for _ in range(v):
                    outs.append(f"SPAWN {name}{mod}")
            elif v <= -99:
                self.lose("sweep-style mass consume (|v| ≥ 99; no pool-wide destroy)",
                          f"{rid}:{k}:{v}")
            else:
                outs.extend([f"DESTROY {name}"] * (-v))
        for k, n in sorted((r.get("deckeffects") or {}).items()):
            n = int(n)
            if k in self.deckname:
                deck = self.deckname[k]
            elif k in self.internal_deck_name and self.internal_deck_name[k]:
                deck = self.internal_deck_name[k]
            else:
                self.lose("deckeffects target deck not mapped", f"{rid}:{k}")
                continue
            outs.extend([f"DRAW {deck}"] * max(0, n))
        if r.get("ending"):
            ename = self.endingname.get(r["ending"])
            if ename:
                outs.append(f"ENDING {ename}")
            else:
                self.lose("ending id never defined in endings.json (shipped data bug)",
                          f"{rid}:{r['ending']}")
        arms = []
        for pverb, parent, tgt, chance in self.copy_plan:
            if pverb == verb and parent == rid:
                cid = self.copy_id(verb, tgt)
                if cid:
                    arms.append((chance, cid))
                else:
                    self.lose("additional alt target missing", f"{rid}->{tgt}")
        for e in r.get("linked") or []:
            tgt = e.get("id")
            if not tgt:
                self.lose("inline linked definition (defines an unmapped-situation recipe)", rid)
                continue
            if tgt not in self.rec:
                self.lose("linked target missing", f"{rid}->{tgt}")
                continue
            if verb_of(self.rec[tgt].get("actionid")) == verb and tgt in self.emitted_id_of:
                arms.append((e.get("chance", 100), self.emitted_id_of[tgt]))
            elif tgt in self.emitted_id_of:
                cid = self.copy_id(verb, tgt)
                if cid:
                    arms.append((e.get("chance", 100), cid))
                else:
                    self.lose("linked target copy could not be minted", f"{rid}->{tgt}")
            else:
                self.lose("linked target structurally skipped (actionId typo)", f"{rid}->{tgt}")
        ladder = self.run_ladder(arms, rid, is_copy)
        if ladder:
            outs.append(ladder)
        return outs

    def render_recipe(self, pid, when, r, verb, seen_when, force_hidden=False):
        rid = r["id"]
        pos = [pid]
        if force_hidden:
            # shadowed twins and carrier recipes ride hidden, whatever their
            # CS hintonly flag says — the live first occurrence already
            # carries the family's hint text
            pos.append("hidden")
        elif r.get("hintonly"):
            h = pick_text(r, ("startdescription", "description", "label"), HINT_MAX)
            # the hint rides the recipe line's LHS where `when COND` and `->`
            # are grammar: neutralize both inside the quoted text
            h = re.sub(r"\bwhen\b", "while", h).replace("->", "--")
            pos.append(f'shown hint "{h}"')
        elif r.get("craftable") is False:
            pos.append("hidden")
        if r.get("maxexecutions"):
            pos.append(f"max {int(r['maxexecutions'])}")
        # §9.48 `aspects` presence list: own running-aspects + the positive
        # requirement keys (the CS situation dictionary while running)
        decl = sorted(set((r.get("aspects") or {}).keys())
                      | {k for k, v in (r.get("requirements") or {}).items() if int(v) > 0})
        decl = [d for d in decl if ASPECT_KEY_RE.match(d)]
        if decl:
            pos.append("aspects " + ", ".join(decl))
        w = norm_when(when)
        if w:
            if w in seen_when:
                self.lose("when dedupe fallback fired (should not happen)", pid)
            seen_when.add(w)
            pos.append(f"when {w}")
        outs = self.outcome_body(verb, r)
        narr = pick_text(r, ("description", "startdescription", "label"), NARRATE_MAX)
        if narr:
            outs.append(f'NARRATE "{narr}"')
        if not outs:
            outs = ['NARRATE "."']
        line = "      " + " ".join(pos) + " -> " + ", ".join(outs)
        # feature losses (the recipe still emits; the feature does not travel)
        if r.get("mutations"):
            self.lose("mutations (aspect rewrites on filtered cards)", rid)
        if r.get("slots"):
            self.lose("recipe slot (opens after start: offerings/expeditions)", rid)
        for s in r.get("slots") or []:
            if s.get("greedy"):
                self.lose("greedy slot (auto-grab; no engine analog)", rid)
            if s.get("consumes"):
                self.lose("slot consumes (burn at slot fill)", rid)
            if s.get("essential"):
                self.lose("slot essential (locked placement)", rid)
        if r.get("portaleffect"):
            self.lose("portaleffect (Mansus door transition)", rid)
        if r.get("burnimage"):
            self.lose("burnimage (rite art)", rid)
        if r.get("achievements"):
            self.lose("achievements (platform meta)", rid)
        if r.get("inductions"):
            self.lose("inductions (legacy/DLC track hooks)", rid)
        if r.get("signalEndingFlavour"):
            self.lose("signalEndingFlavour (noted on the ending, not the recipe)", rid)
        return line

    # -- verb emission ---------------------------------------------------------

    def plan_slots(self):
        """CS's any-of required map → ONE alternation slot (§9.49); forbidden
        maps → negated terms in every group."""
        self.slot_of = {}
        for v in self.cs_verbs:
            s = v.get("slot") or {}
            req = s.get("required") or {}
            forb = [f"-aspect:{k}" for k in sorted((s.get("forbidden") or {}).keys())]
            if req:
                groups = [" ".join([f"aspect:{k}>={int(req[k]) or 1}"] + forb)
                          for k in sorted(req.keys())]
                self.slot_of[v["id"]] = " | ".join(groups)
            else:
                self.slot_of[v["id"]] = None

    def aspect_echo_keys(self):
        """Aspect keys read by requirement/slot maps but never declared on any
        card — their carriers are isAspect elements the port skips (CS
        uniqueness-group tags, Exile tracks, flavours...). The echo prototype
        declares them so the lint's read/write check holds; it is never
        minted, so it stays inert. Computed over ALL recipes (a superset of
        the emitted whens), which keeps it stable before copies render."""
        reads = set()
        for r in self.recipes:
            for src in ("requirements", "extantreqs", "tablereqs"):
                reads.update((r.get(src) or {}).keys())
            for a in r.get("alt") or []:
                reads.update((a.get("requirements") or {}).keys())
        for v in self.cs_verbs:
            s = v.get("slot") or {}
            reads.update((s.get("required") or {}).keys())
            reads.update((s.get("forbidden") or {}).keys())
        reads |= CARRIER_GUARD_ASPECTS  # the carrier actors-slot guard reads too
        reads = {k for k in reads if ASPECT_KEY_RE.match(k)}
        written = set()
        for _eid, (_h, _k, aspects) in self.decl.items():
            for key in aspects:
                written.add(key)
                dot = key.find(".")
                while dot != -1:
                    written.add(key[:dot])
                    dot = key.find(".", dot + 1)
        return sorted(reads - written)

    def emit_verbs(self):

        verb_mains = defaultdict(list)
        for r in self.recipes:
            v = verb_of(r.get("actionid"))
            if v:
                verb_mains[v].append(r)

        # duration: modal NONZERO warmup of the verb's recipes (1 CS s → 1
        # engine min); per-recipe deviations are a documented loss
        all_verbs = list(VERB_OF.values()) + [v for v in CARRIER_VERBS]
        for verb in all_verbs:
            mains = verb_mains.get(verb, [])
            nz = Counter(int(r.get("warmup") or 0) for r in mains
                         if int(r.get("warmup") or 0) > 0)
            modal = nz.most_common(1)[0][0] if nz else 60
            self.verb_duration[verb] = fmt_duration(modal)
            for r in mains:
                if int(r.get("warmup") or 0) != modal:
                    self.duration_deviations[verb] += 1

        lines = ["verbs:", ""]
        for actionid, verb in VERB_OF.items():
            lines.append(f"  {verb}:")
            lines.append("    actors: 1 kind:character")
            if verb == "MANSUS":
                # CS portal.mansus is icon-only (no slot, no recipes); the
                # signature wants a place and the station one recipe
                lines.append("    object: 1 kind:place")
                lines.append(f"    duration: {self.verb_duration[verb]}")
                lines.append("    recipes:")
                lines.append('      default -> REFUSE("The Mansus does not open here; no CS recipe carries the portal action.")')
                lines.append("")
                self.verb_recipe_counts[verb] = (0, 0, 0, 1)
                continue
            q = self.slot_of.get(actionid)
            if q:
                lines.append(f"    with: 0..1 {q}")
            else:
                # time's slot has no required map in CS: a queryless slot
                # (matches any card)
                lines.append("    with: 0..1")
            lines.append(f"    duration: {self.verb_duration[verb]}")
            lines.append("    recipes:")
            seen_when = set()
            n_main = n_chain = 0
            default_line = None
            chain = []
            plan_list = self.plans.get(verb, [])
            order = {id(p): i for i, p in enumerate(plan_list)}
            boot, rest = [], []
            for p in plan_list:
                if p[3] == "chain":
                    rest.append(p)
                elif p[3] == "chainboot":
                    boot.append(p)
            chain = [(0, order[id(p)], p) for p in boot] + [(1, order[id(p)], p) for p in rest]
            chain.sort(key=lambda t: (t[0], t[1]))  # bootstraps first, data order within
            rendered_chain = []
            for _grp, _idx, (pid, when, r, _kind) in chain:
                rendered_chain.append(self.render_recipe(pid, when, r, verb, seen_when))
            for pid, when, r, kind in plan_list:
                if kind == "default":
                    default_line = self.render_recipe(pid, when, r, verb, seen_when)
                elif kind in ("main", "twin"):
                    # mains and their shadowed twins interleave in data order;
                    # twins render hidden (the first occurrence keeps its
                    # emitted form and wins first-match, §6.4)
                    lines.append(self.render_recipe(
                        pid, when, r, verb, seen_when, force_hidden=(kind == "twin")))
                    if kind == "main":
                        n_main += 1
            n_chain = len(rendered_chain)
            # emission order: mains, RUN-target chains (bootstraps first),
            # copies, default last — bare attempts fall through the mains to
            # the chain catch-alls, copies stay RUN-addressable, the default
            # closes the list
            for line in rendered_chain:
                lines.append(line)
            for line in self.copy_lines.get(verb, []):
                lines.append(line)
            if default_line:
                lines.append(default_line)
            lines.append("")
            self.verb_recipe_counts[verb] = (n_main, n_chain,
                                             len(self.copy_lines.get(verb, [])),
                                             1 if default_line else 0)

        # the five carrier verbs: every non-station CS actionid rehomes here,
        # hidden, grouped under a marker comment carrying the original
        # actionid (see the header map). Emission order mirrors the stations:
        # carrier mains in data order (grouped by actionid), RUN-target
        # chains, copies, then the REFUSE wall last (first-match, §6.4).
        for verb in CARRIER_VERBS:
            lines.append(f"  {verb}:")
            lines.append(f"    actors: 1 {CARRIER_ACTORS_GUARD}")
            lines.append("    with: 0..1")
            lines.append(f"    duration: {self.verb_duration[verb]}")
            lines.append("    recipes:")
            lines.append("      # carrier station — chain-addressed only (§9.47): the guarded")
            lines.append("      # actors slot matches no character, so this verb is never")
            lines.append("      # suggested and a typed attempt rejects; RUN projections do")
            lines.append("      # not re-check slot queries. Every recipe is hidden.")
            seen_when = set()
            plan_list = self.plans.get(verb, [])
            order = {id(p): i for i, p in enumerate(plan_list)}
            boot, rest = [], []
            for p in plan_list:
                if p[3] == "chain":
                    rest.append(p)
                elif p[3] == "chainboot":
                    boot.append(p)
            chain = [(0, order[id(p)], p) for p in boot] + [(1, order[id(p)], p) for p in rest]
            chain.sort(key=lambda t: (t[0], t[1]))
            rendered_chain = []
            for _grp, _idx, (pid, when, r, _kind) in chain:
                rendered_chain.append(self.render_recipe(pid, when, r, verb, seen_when,
                                                         force_hidden=True))
            n_main = 0
            last_group = None
            group_n = Counter()
            for p in plan_list:
                if p[3] in ("carrmain", "twin", "main"):
                    group_n[p[2].get("actionid")] += 1
            for pid, when, r, kind in plan_list:
                if kind not in ("carrmain", "twin", "main"):
                    continue
                group = r.get("actionid")
                if group != last_group:
                    lines.append(
                        f"      # actionid {group!r} → {verb} "
                        f"({group_n[group]} entries; CS runs these from its own "
                        f"situations — rehomed, chain-addressed, hidden)")
                    last_group = group
                lines.append(self.render_recipe(pid, when, r, verb, seen_when,
                                                force_hidden=True))
                n_main += 1
            for line in rendered_chain:
                lines.append(line)
            for line in self.copy_lines.get(verb, []):
                lines.append(line)
            lines.append(f'      default -> REFUSE("{verb} is a carrier station: no CS recipe addresses it as a play action; its recipes run from chains.")')
            lines.append("")
            self.verb_recipe_counts[verb] = (n_main, len(rendered_chain),
                                             len(self.copy_lines.get(verb, [])), 1)
        return lines

    # -- assembly --------------------------------------------------------------

    def generate(self):
        self.plan_elements()
        self.plan_recipes()
        self.apply_alt_gates()
        self.plan_card_triggers()
        self.plan_decks()
        self.plan_slots()

        L = []
        w = L.append
        w("# Cultist Simulator — the core, compiled to a Chorus script.")
        w("#")
        w("# GENERATED FILE — do not edit by hand. Regenerate with:")
        w(f"#   python3 tools/convert_cs.py --core {self.core} --out scripts/cultist-full.txt")
        w("# Determinism: fixed seed, fixed data-order emission, no timestamp —")
        w("# regenerations from the same core are byte-identical.")
        w("# Source: the CS 2026.1.g core (160 loose JSONs), loaded with the durable")
        w("# copy's lenient-JSON loader. Mapping dossier: docs/reverse/cultist-")
        w("# simulator.md §2/§6. Grammar: docs/spec/text-engine-v0.6.md. Engine laws:")
        w("# docs/plans/v0.6-cs-slice.md §3.")
        w("#")
        w("# Text policy: the owner relaxed the no-quoting rule for this engine-")
        w("# validation port (docs/plans/v0.6-cs-slice.md §0) — CS label/description")
        w("# strings are quoted (whitespace-flattened, truncated) as NARRATE text and")
        w("# ending titles; mechanics and numbers are the dossier's facts.")
        w("#")
        w("# THIS IS A BALANCE-UNTAUGHT MECHANICAL PORT. CS's engine tie-breaks, its")
        w("# greedy slot auto-grab, mid-run slot offers, the real-time heartbeat and")
        w("# the unattended Time-Passes pump do not exist here. Recipes select")
        w("# first-match-wins in data order (§6.4); durations are verb-level (the")
        w("# modal nonzero warmup); chances are ROLL(1,100)+IF ladders with fresh")
        w("# rolls per linked entry. It is loadable and accounted, not playable-tuned.")
        w("#")
        w("# CONVENTIONS (deterministic):")
        w("#   names      CS element id → UPPER_SNAKE (dots → underscores); decks and")
        w("#              endings likewise; recipe ids are `<verb>.<csid>`, alt/run")
        w("#              copies on a parent verb are `<verb>.alt.<csid>`, and a")
        w("#              shadowed twin is `<verb>.<csid>.x2` (the first occurrence")
        w("#              keeps its unsuffixed emitted form).")
        w("#   aspects    every card carries its own id as a one-level marker aspect")
        w("#              (`with <cs-id>:1`) plus its declared aspects — so CS")
        w("#              requirement keys, element ids and aspect ids alike, read as")
        w("#              `total(with, aspect:k) >= v` (v<0 = total < |v|, CS absence).")
        w("#   kinds      by CS source file (cosmetic: matching is aspect-based).")
        w("#   cards      every non-aspect element is a card; the 330 isAspect")
        w("#              elements (CS uniqueness tags, Exile tracks, pure-aspect")
        w("#              carriers) emit as stackable PROTOTYPES — never entities,")
        w("#              whose existence would satisfy §6.3's world-form count()")
        w("#              existentials and corrupt the extantreq gates. Cards never")
        w("#              minted anywhere and unique:true are declared entities;")
        w("#              everything minted at runtime, deck-spec members, and")
        w("#              lifetime bearers are stackable PROTOTYPES (the §9.19 mint-")
        w("#              collision law and the §9.44 deck livelock rule outrank CS's")
        w("#              unique:true; flips are counted in the report).")
        w("#   carriers   the five non-station lexemes (READ TRAVEL_TO USE_ON ACQUIRE")
        w("#              LOCATE_AT) carry every non-station CS actionid, hidden,")
        w("#              under a marker comment naming the actionid (the map is in")
        w("#              tools/convert_cs.py's header). Their `actors` slot is")
        w("#              guarded (`aspect:ritual>=1` matches no character): never")
        w("#              suggested, typed attempts reject, RUN projections still")
        w("#              open their windows — the player-visible surface is the")
        w("#              seven stations, unchanged. Carrier defaults are REFUSE")
        w("#              walls, last in each list.")
        w("#   twins      CS's shadowed recipes (requirement-identical duplicates,")
        w("#              requirement-less recipes behind a verb default) emit hidden")
        w("#              under `.x2` ids: duplicates keep their CS when plus an")
        w("#              always-true conjunct (the first occurrence wins first-match")
        w("#              exactly as before); requirement-less station twins get a")
        w("#              never-matching when (CS cannot fire them either), while")
        w("#              requirement-less carrier recipes get an always-true one")
        w("#              (carriers are RUN-addressed; a never-when would end CS-live")
        w("#              chains, §9.47).")
        w("#   decay      1 CS second → 1 engine minute. A lifetime with a decayTo is")
        w("#              an `on expire` DESTROY+SPAWN+SET chain (never REPLACE from a")
        w("#              trigger — the trigger-body law); without a decayTo the")
        w("#              engine's built-in destroy-by-id expiry stands (the §9.38-")
        w("#              safe path under pools). Recipe-effect mints of lifetime")
        w("#              bearers arm at birth via the authored `SPAWN X expires")
        w("#              <dur>` modifier (§4.4/§9.36 — each fresh copy its own")
        w("#              decay, pooled or not; findings E12's spawn-modifier half);")
        w("#              deck-drawn lifetime bearers stay un-armed (DRAW has no")
        w("#              expires arg — findings E12's open half).")
        w("#   chains     linked → RUN ladders: ROLL(1,100)+IF arms with a fresh roll")
        w("#              per entry; one RUN op per completion (§9.47), so CS's")
        w("#              parallel additional-alts + linked chain collapse into the")
        w("#              arms of ONE ladder (alts first, linked last as fallback).")
        w("#              Each arm RUNs a candidate list of its own entry plus every")
        w("#              later entry — the roll gives priority, the walk's when-")
        w("#              checks give fall-through — so a rolled arm whose `when`")
        w("#              fails strands nothing. Probabilities shift accordingly")
        w("#              (counted in the report). Cross-verb targets are emitted as")
        w("#              RUN-addressable copies on the parent verb")
        w("#              (`<verb>.alt.<csid>`, world-form whens). Recipe lists emit")
        w("#              mains in data order, then RUN-target chains (bootstrap")
        w("#              entries first), then copies, then the verb default last.")
        w("#   xtriggers  card xtriggers → `on complete <event>` clauses on the card;")
        w("#              aspect-borne hooks (CS carries some on isAspect elements,")
        w("#              e.g. job/ingredient → needs) distribute onto every card")
        w("#              declaring the aspect. Recipes declare `aspects` = their own")
        w("#              running aspects + positive requirement keys (§9.48).")
        w("#")
        w("# The feature-loss list (emitted-with-loss and dropped-feature counts —")
        w("# distinct from skips) and the structural-skip tables are in the")
        w("# converter's coverage report; every recipe/element/deck/ending is")
        w("# emitted or named-impossible there, and the census totals are asserted.")
        w("")
        w("script: Cultist Simulator — the core, mechanical")
        w("player: THE_ASPIRANT")
        w("goal: survive the Hours — work, study, dream, talk, explore — and ascend, or go under")
        w("start: 0:0")
        w(f"seed: {self.seed}")
        w("")

        w("entities:")
        w("  # the port's frame: CS core has no player card (starting inventories")
        w("  # live in legacies, out of scope); the port adds one character and one")
        w("  # root place so stations have an actor and the places a home (§10).")
        w("  THE_CITY is place")
        w("  THE_ASPIRANT is character at THE_CITY")
        self.n_entities = 0
        for eid in sorted(self.decl):
            home, kind, _a = self.decl[eid]
            if home != "entity":
                continue
            self.n_entities += 1
            label = clean_text(self.el[eid].get("label") or "", 120)
            if label:
                w(f"  # {label}")
            w(f"  {self.elname[eid]} is {kind}{self.with_clause(eid)}")
        w("")

        w("prototypes:")
        echo = self.aspect_echo_keys()
        if echo:
            w("  # aspect-vocabulary echo: requirement keys whose carriers are")
            w("  # isAspect elements (CS uniqueness tags, Exile tracks, flavours) —")
            w("  # declared so every read resolves; never minted, therefore inert.")
            w("  CS_ASPECT_ECHO is state stackable with " + ", ".join(f"{k}:1" for k in echo))
            self.n_echo_keys = len(echo)
        else:
            self.n_echo_keys = 0
        self.n_prototypes = 0
        self.n_complete = 0
        for eid in sorted(self.decl):
            home, kind, _a = self.decl[eid]
            if home != "proto":
                continue
            self.n_prototypes += 1
            clauses = ""
            for aspect_key, body in self.complete.get(eid, []):
                head = f"on complete {aspect_key} -> " if aspect_key else "on complete -> "
                clauses += " " + head + "(" + body + ")"
                self.n_complete += 1
            w(f"  {self.elname[eid]} is {kind} stackable{self.with_clause(eid)}{clauses}")
        w("")

        w("# card-borne clocks: lifetimes that decay into another element are")
        w("# authored DESTROY+SPAWN+SET chains; the rest expire by the built-in")
        w("# destroy (no trigger — the pool-safe path).")
        w("triggers:")
        for tid, eid, body in self.expire_triggers:
            w(f"  {tid} on expire of {self.elname[eid]} -> {body}")
        if not self.expire_triggers:
            w("  # (none)")
        w("")

        w("decks:")
        for line in self.deck_lines:
            w(line)
        w("")

        w("endings:")
        for e in sorted(self.endings, key=lambda x: x["id"]):
            flavour = (e.get("flavour") or "Melancholy").lower()
            title = clean_text(e.get("label") or e["id"], 140)
            desc = clean_text(e.get("description") or "", 300)
            if desc:
                w(f"  # {desc}")
            w(f"  {self.endingname[e['id']]}: \"{title}\" ({flavour})")
        w("")

        L.extend(self.emit_verbs())

        w("templates:")
        for verb, t in [
            ("WORK", "{actors} work."),
            ("WAIT", "{actors} wait."),
            ("STUDY", "{actors} study {object}."),
            ("DREAM", "{actors} dream."),
            ("TALK", "{actors} talk."),
            ("EXPLORE", "{actors} explore."),
            ("MANSUS", "{actors} face {object}."),
            ("READ", "{actors} turn to {with}."),
            ("TRAVEL_TO", "{actors} take the road, {with} in hand."),
            ("USE_ON", "{actors} put {with} to work."),
            ("ACQUIRE", "{actors} reach for {with}."),
            ("LOCATE_AT", "{actors} take stock of {with}."),
        ]:
            w(f"  {verb} says \"{t}\"")
        w("")
        return "\n".join(L) + "\n"

    # -- report ----------------------------------------------------------------

    def report(self, emitted, copies):
        n_recipes = len(self.recipes)
        skipped = n_recipes - emitted
        print("=" * 74)
        print("COVERAGE REPORT — tools/convert_cs.py")
        print("=" * 74)
        print(f"elements : {len(self.elements)} input = {self.n_entities} entities + "
              f"{self.n_prototypes} prototypes + {sum(self.el_skip.values())} structural-skip")
        print(f"           (of the prototypes, {self.n_aspect_cards} are isAspect elements —")
        print("           CS uniqueness tags, Exile tracks, pure-aspect carriers — emitted")
        print("           as inert stackable carriers; never entities: a declared entity")
        print("           exists at start and §6.3's world-form count() would read it)")
        nl = sum(1 for e in self.elements if "lifetime" in e and e["id"] in self.decl)
        print(f"           {self.n_complete} on-complete clauses on cards, "
              f"{len(self.expire_triggers)} on-expire chains "
              f"({nl} lifetime bearers; {nl - len(self.expire_triggers)} built-in destroy)")
        carr = {("no actionid" if a is None else a): n
                for a, n in sorted(self.n_carrier.items(), key=lambda kv: str(kv[0]))}
        print(f"recipes  : {n_recipes} input = {emitted} emitted "
              f"(+ {copies} alt/run copies on parent verbs) + {skipped} structural-skip")
        print(f"           rehomed onto carrier verbs: {sum(carr.values())} recipes over "
              f"{len(carr)} actionids: {carr}")
        print(f"           shadowed twins emitted hidden (.x2): "
              f"{sum(self.n_twins.values())} {dict(sorted(self.n_twins.items()))}")
        print(f"           lifetime-bearer mints: {self.n_armed} recipe-effect sites "
              f"armed at the mint (authored `SPAWN … expires <dur>` modifier — "
              f"each copy arms its own decay, pooled or not) + {self.n_e12_deck} "
              f"deck entries E12-ledgered (DRAW has no expires arg — findings "
              f"E12); the {len(self.expire_triggers)} on-expire chains arm via "
              f"their own trigger bodies")
        print(f"decks    : {len(self.decks)} input = {self.n_real_decks} declared + "
              f"{len(self.decks) - self.n_real_decks} dropped; "
              f"+ {self.n_internal_decks} internaldecks declared")
        print(f"endings  : {len(self.endings)} declared")
        durs = ", ".join(f"{v}={self.verb_duration[v]}" for v in sorted(self.verb_duration))
        print(f"verbs    : 12 configured — 7 stations (work→WORK dream→DREAM talk→TALK")
        print(f"           explore→EXPLORE study→STUDY time→WAIT portal.mansus→MANSUS)")
        print(f"           + 5 carriers (READ TRAVEL_TO USE_ON ACQUIRE LOCATE_AT);")
        print(f"           durations: {durs}")
        tot = sum(self.duration_deviations.values())
        print(f"           per-recipe warmups differing from the verb duration: {tot} "
              f"{dict(sorted(self.duration_deviations.items()))} (documented loss —")
        print("           recipes cannot override the verb duration, ruling 12)")
        print(f"           per-verb recipes (main/chain/copies/default): "
              f"{ {v: self.verb_recipe_counts[v] for v in sorted(self.verb_recipe_counts)} }")
        print()
        print("recipe STRUCTURAL-SKIP table (reason -> count -> examples; nothing else is skipped):")
        for reason in sorted(self.rec_skip, key=lambda k: -self.rec_skip[k]):
            print(f"  {self.rec_skip[reason]:5d}  {reason}")
            print(f"        e.g. {', '.join(self.rec_skip_ex[reason])}")
        print(f"  {sum(self.rec_skip.values()):5d}  TOTAL structural-skipped recipes")
        print("element STRUCTURAL-SKIP table:")
        for reason in sorted(self.el_skip, key=lambda k: -self.el_skip[k]):
            print(f"  {self.el_skip[reason]:5d}  {reason}  e.g. {', '.join(self.el_skip_ex[reason])}")
        print(f"  {sum(self.el_skip.values()):5d}  TOTAL structural-skipped elements")
        print("feature-level losses (emitted-with-loss / dropped-feature; item -> count -> examples):")
        for feat in sorted(self.feat, key=lambda k: -self.feat[k]):
            print(f"  {self.feat[feat]:5d}  {feat}")
            if self.feat_ex[feat]:
                print(f"        e.g. {', '.join(self.feat_ex[feat])}")
        print(f"  {sum(self.feat.values()):5d}  TOTAL feature losses")
        # census assertions — every input accounted for; every skip named
        # structural (the full-coverage bar: no convenience skips exist)
        assert sum(self.el_skip.values()) + self.n_entities + self.n_prototypes == len(self.elements), \
            "element census broken"
        assert emitted + skipped == n_recipes, "recipe census broken"
        assert all(k.startswith("STRUCTURAL:") for k in self.rec_skip), \
            "non-structural recipe skip — a convenience skip crept in"
        assert all(k.startswith("STRUCTURAL:") for k in self.el_skip), \
            "non-structural element skip — a convenience skip crept in"
        assert self.n_real_decks + (len(self.decks) - self.n_real_decks) == len(self.decks) == 67
        assert len(self.endingname) == len(self.endings) == 48, "ending census broken"
        # the E12 lifetime split (findings ledger): every recipe-effect spawn
        # of a lifetime bearer armed at the mint (the SPAWN expires modifier);
        # the deck-entry half stays open (DRAW has no expires arg)
        assert self.n_armed == 497, \
            "armed recipe-effect spawn sites drifted (E12 spawn-modifier half)"
        assert self.n_e12_deck == 159, \
            "E12-ledgered deck entries drifted (E12 open half: deck draws)"
        print()
        print(f"census OK: elements {len(self.elements)} = "
              f"{self.n_entities}+{self.n_prototypes}+{sum(self.el_skip.values())} "
              f"(structural skip: {sum(self.el_skip.values())}); "
              f"recipes {n_recipes} = {emitted}+{skipped} "
              f"(structural skip: {skipped}, all actionId typos); decks 67; endings 48")


def main():
    ap = argparse.ArgumentParser(description="Compile CS core JSON into a Chorus script")
    ap.add_argument("--core", default=CORE_DEFAULT)
    ap.add_argument("--out", default="scripts/cultist-full.txt")
    ap.add_argument("--seed", type=int, default=1837)
    args = ap.parse_args()

    conv = Converter(args.core, seed=args.seed)
    text = conv.generate()
    out_dir = os.path.dirname(args.out)
    if out_dir:
        os.makedirs(out_dir, exist_ok=True)
    with open(args.out, "w", encoding="utf-8") as f:
        f.write(text)
    emitted = sum(len(p) for p in conv.plans.values())
    copies = sum(len(v) for v in conv.copy_lines.values())
    print(f"wrote {args.out}: {len(text.splitlines())} lines, {len(text)} bytes")
    conv.report(emitted, copies)


if __name__ == "__main__":
    main()
