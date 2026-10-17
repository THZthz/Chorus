# Cultist Simulator — reverse-engineering dossier

Compiled 2026-10-12 for a Chorus re-implementation. Three sources, cross-checked, then
**independently verified** (see §8): six adversarial fact-checkers re-derived every claim
from the game files, the IL, the wiki, and this repo's source.

1. **The installed game itself** (Steam, `version.txt` = `2026.1.g.2`; codename
   "Guinefort" appears only in `elements/versionnews.json`) — all content is **loose
   JSON** under `cultistsimulator_Data\StreamingAssets\content\core` (160 files,
   3,007,452 bytes ≈ 3.0 MB), plus `SecretHistories.Main.dll` for engine constants.
2. **The Fandom wiki** and Weather Factory blog posts (mechanics rules).
3. **The Chorus spec** (`docs/spec/text-engine-v0.7.md`) for the capability map.

> Ethics note: this document records *mechanics, schemas and numbers* — facts about how
> the game works, fine to re-implement. The game's label/description strings and art are
> Weather Factory's copyrighted content; do not copy them into this repo. Original
> flavor text for the clone must be written fresh.

---

## 1. Where the content lives

```
Cultist Simulator\                                  version.txt = "2026.1.g.2"
  cultistsimulator_Data\
    Managed\  (134 DLLs)                             SecretHistories.Main.dll 944,128 B ← the engine code
      (Assembly-CSharp.dll 113,152 B = analytics/tweening only, ZERO game types;
       SecretHistories.Constants/Enums/Interfaces.dll, Newtonsoft.Json.dll 691,712 B)
    resources.assets / sharedassets*.assets          (art/audio only — ignore)
    StreamingAssets\
      MODDING_README.txt                             (official mod-format doc)
      UnityServicesProjectConfiguration.json
      edition\sempr.txt                              JSON config: {"config":[{"id":"displayPerpetual","value":true}]}
      edition\store.txt                              "Steam"
      content\
        core\                                        ← ALL GAME CONTENT, 160 loose JSON files, 3.0 MB
          recipes\ 79 files   2,084 recipes          ← the rulebook
          elements\ 54 files  1,566 elements         ← cards AND aspects (isAspect:true)
          verbs\ 1 file       7 verbs                work dream study talk explore time portal.mansus
          decks\ 7 files      67 decks               draw piles incl. 15 mansus_* decks
          endings\ 1 file     48 endings             16 Grand / 32 Melancholy
          legacies\ 2 files   14 + 1 orphan          (the orphan: key typo'd "legcies" in shipped JSON)
          levers\ 1 file      8 levers               end-of-run selectors feeding the next legacy
          portals\ 1 file      7 portals             Mansus map transitions
          achievements\ 5 files  90                  (incl. DLC: dancer/exile/ghoul/priest achievements)
          cultures\ 7 lang dirs, settings\ 29, dicta\ 1
        loc_de|es|fr|jp|ru|zh-hans\                  SAME LEVEL as core\; a 6-dir LOCALIZATION layer; NOT a uniform subset.
                                                     de/jp/ru/zh-hans: strict core subsets missing
                                                     the same 20 files each (no cultures/dicta/levers/
                                                     portals/settings; no legacies.json; also credits,
                                                     tlg, versionnews, xperimental, z_levers, _debug,
                                                     __debug, study_research). es/fr: NOT subsets —
                                                     they ADD portals\, use suffixed names (-spa/-fre)
                                                     and carry ~160 files core lacks (DLC content);
                                                     167/168 files vs core 160. Loc entries are FULL
                                                     entity copies that can differ beyond label/
                                                     description AND can DROP functional fields (loc_de
                                                     lockscaredge loses burnTo) — core is authoritative
```

DLC content is **present in core**: `achievements_{dancer,exile,ghoul,priest}.json`,
`legacy_dancer_elements.json`, `legacy_dancer_recipes.json`, `priest_elements.json`.

Every file is `{"<entitytype>": [ {...}, ... ]}` in *lenient JSON*. Quirk census (all 160
scanned): BOM in 9 files, unquoted keys in 3 (`decks/mansus.json`, `elements/_aspects_uq.json`,
`recipes/talk_l_romance.json`), trailing commas in 33, and **raw newlines inside strings in
exactly 2 files** (`decks/mansus.json`, `elements/versionnews.json`) — json5 alone fails on
those two; escape newlines first (see `loadlib.py`). Extract:

```sh
scp -r "12899@192.168.3.18:E:/programs/steam/steamapps/common/Cultist Simulator/cultistsimulator_Data/StreamingAssets/content/core" /tmp/cs-re/
```

## 2. Schema

### element (cards, aspects, micro-tokens)

| field | meaning |
|---|---|
| `id, label, description, icon` | identity (label/description = copyrighted text) |
| `aspects {id:int}` | the matching currency |
| `slots [slot]` | slots that open when this card is placed in a verb (`id, actionid` — verb to open, `required{}` any-of, `forbidden{}`, `consumes`, `essential`, **`greedy`** — auto-grab; all 108 uses in the dataset sit in *recipe* slots, none in element slots) |
| `xtriggers {eventName: recipeId}` | event→transform hooks fired *on* the card (element-only; see §3.3) |
| `induces [{id:recipeId, chance:%}]` | chance to auto-run a recipe after any recipe finishes involving this card (element-only; 14 elements have it) |
| `lifetime:int` / `decayTo:elementId` | seconds until decay/transform — **13 distinct values in data**: `{1:98, 10:2, 30:1, 59:1, 60:47, 90:7, 100:2, 120:31, 150:1, 180:35, 240:1, 300:19, 600:12}`; absent = permanent |
| `unique:bool`, `uniquenessgroup:str` | 373 elements `unique:true`; 254 elements in 36 uniqueness groups; one instance/one-per-group |
| `isAspect:true, isHidden, burnTo, noartneeded, sort, lever, comments, verbicon, …` | metadata |

Aspects live in `elements/_aspects.json` (290 with `isAspect:true`); **330 total**
isAspect elements (+36 in `_aspects_uq.json`, +4 in `vault_locks.json`). Only **4 of the
8 principles** carry `induces`: lantern→`lanterninduction` 5%, heart→`heartinduction`
10%, grail→`grailinduction` 5%, moth→`mothinduction` 10% (forge/edge/winter/knock have
none). `dreadful`→dreadinduction 30%, `fascinating`→fascinationinduction 30%, and
`job`/`ingredient` induce `{needs, 100%}` — that's what hooks every work/consumable into
the Time-Passes expense cycle. The full induces-carrier set is **14 elements**, not 8:
`dance` (→`heartinduction` 10% + `mothglimmeringinduction` 10%), `secret`
(→`secret.induction` 30%) and four `poison.*` elements (30–70%) also induce; the
mappings listed above are individually correct. All 8 principles carry `sort:"_p.0"`.

The entire Health/Fatigue/Injury machine is *one element + xtriggers* (in
`abilities.json`; the doc previously quoted an abridged version):

```json
{"id":"health","label":"Health","aspects":{"heart":2,"ingredient":1,"ability":1},
 "xtriggers":{"damagehealth":"injury","fallsick":"affliction","destroyhealth":"decrepitude",
              "fatiguing":"fatigue","employment":"fatigue","job":"fatigue","ritual":"fatigue",
              "society":"fatigue","scar_ability":"scar_heart"},
 "decayTo":"fatigue"}
```

Other verbatim examples: `{"id":"notoriety","label":"Notoriety","aspects":{"reputation":2},
"lifetime":300,"xtriggers":{"scandalous":"notoriety"}}`; `influencemothg`
`{moth:15, influence:1}, lifetime 60, decayTo influencemothe`; `toolwinterb` "Bone Flute"
`{winter:4, auctionable:3, tool:1}, unique:true`. Desire cards share
`uniquenessgroup:"uqdesire"` (26 cards) without `unique:true`.

### verb (7 total — the stations)

```json
{"id":"work","slot":{"required":{"ability":1,"job":1,"ritual":1,"desire":1,"benefactor":1},
                     "forbidden":{"reasonskill":1}}}
```

One big input slot whose `required` is **any-of** (a card matching any listed aspect opens
the station). `time` has a slot with no `required` key at all; `portal.mansus` is
`{id, label, description, icon:"map"}` — no slot ("icon-only").

### recipe (2,084 — the rulebook)

| field | meaning |
|---|---|
| `id, label, startdescription, description, comments` | identity + narrated start/end (text = copyrighted) |
| `actionid` | which verb/situation it belongs to (4 recipes misspell it `actionId` and are ignored) |
| `requirements {key:int}` | **AND**-match vs the situation dictionary; **791 distinct keys — 592 are element ids, 199 aspect ids** (elements count as their own implicit aspect); `k>0` needs total ≥ k; **`k<0` means "total < \|k\|" — i.e. `-1` = "must be absent"** (IL-verified, §3.3); 596 negative entries; values span −7…50 |
| `warmup:int` | seconds (see §4); **787 recipes have none = instant**; distribution `{0:47, 1:9, 2:4, 3:2, 5:55, 10:251, 15:16, 20:2, 30:396, 40:1, 45:3, 50:2, 55:1, 60:496, 70:5, 90:3, 120:2, 180:1, 300:1}` — 60 is the modal "standard action" (496 = 38% of timed recipes) |
| `effects {id:±int}` | applied at completion: `+` create, `−` consume (`funds:-999` = destroy all in the situation) |
| `aspects {id:int}` | hidden aspects added to the situation *while running* — feeds xtriggers on contained cards (e.g. `{"fatiguing":1}` decays health) |
| `slots [slot]` | 238 recipes have one; opens only after the recipe starts (rite offering slots, expedition slots) |
| `linked [{id, chance:%, additional?, challenges?}]` | chain of what runs *after* completion; each entry rolled by its chance (absent = 100; 1,473 linked entries). **No linked entry carries `expulsion` — it lives on `alt` entries** (27 of them, expedition casualties; see next row). 21 linked entries carry a `challenges` map (the only content uses of the ChallengeArbiter — all `"base"`, Exile long-recipe files). 8 linked entries are full inline recipe definitions |
| `alt [ …same shape… ]` | alternatives considered in addition; first requirements-match wins (748 entries, 3 inline). 27 alt entries carry `expulsion {filter, limit}` (removes a filtered card — expedition casualties) |
| `mutations [{filter{}, mutate, level, additive}]` | rewrite aspects on cards matching the filter (164 recipes) |
| `deckeffects {deckId:count}` (74) / `internaldeck {spec[], draws, resetonexhaustion, defaultcard}` (37) | draw cards onto the table |
| `extantreqs {elementId:int}` (42 recipes) / `tablereqs` (18) | existence checks anywhere-in-play / on-table only (not consumed; negatives forbid) |
| `ending:endingId` | fires an ending — 56 recipes, referencing 47 of the 48 defined endings |
| `signalEndingFlavour` | Grand 29 / Melancholy 13 / **Vile 1** (on `ending.minorcrownedgrowthvictory` — whose `ending` id is *not defined* in endings.json: a shipped data bug, like the `legcies` typo) |
| `craftable` / `hintonly` | player-startable (573 true + 1 string `"true"`; 489 false; 1,021 absent) vs grey suggestion (169 hintonly) |
| `maxexecutions:int` (26), `inductions`, `portaleffect` (6: wood/whitedoor/stagdoor/spiderdoor/peacockdoor×2), `burnimage`, `achievements[]`, `signalimportantloop` (exactly 1: `needs`) | misc |

`chances`/`challenges`: expedition tier chances are **hard-coded** in `linked` entries
(90/70/30); the `challenges {aspect:"base"|"advanced"}` mechanism exists in the engine
(`ChallengeArbiter`) and IS used — but only at the linked-entry level: 21 linked entries,
all in `recipes/long_recipes_attacks.json` + `recipes/long_recipes.json` (Exile
long-recipe content), every one `"base"` (the top-level `challenges` key: 0 uses).
IL-verified tables (three independent decodes; raw aspect total in, integer percent out,
inclusive thresholds; result = **max** across challenge entries):

| aspect total | base | advanced |
|---|---|---|
| ≤ 0 | 0% | 0% |
| 1–4 | 30% | 0% |
| 5–9 | 70% | 10% |
| 10–14 | 90% | 30% |
| 15–19 | 90% | 70% |
| ≥ 20 | 90% | 90% |

(An earlier IL pass in this doc claimed base 3–9→70 / 1–2→30 — **wrong**; the
re-decode agrees with the wiki's "1→30, 5→70, 10→90". The 2026-10-15 third decode
(§9) confirmed both tables — this table's advanced column had previously kept stale
one-bucket-shifted values that contradicted §8 correction 1's own text.)

### deck (67)

`spec:[elementIds…]` (30/67 decks repeat ids to weight the draw), `resetonexhaustion`
(key on 57: 49 true, 8 false), `defaultcard` (28; fillers incl. fleeting, rumour,
contentment), `drawmessages` (15), `label`/`description` (51). Key decks: `seasonevents`, `hunters`,
`acquaintances`, `hirelings`, `scholarpatrons`, `stagriddles`, `schemeevents`,
`talkevents`, `exploreevents`, **6** vault-region decks (`vaultscontinent`,
`vaultseveningisles`, `vaultslandbeyondforest`, `vaultsloneandlevelsands`,
`vaultsrendingmountains`, `vaultsshires`), `vaultrewards` a..g, and 15 `mansus_*`
door decks.

### ending / legacy / lever / portal

- **ending (48):** flavour only Grand (16) / Melancholy (32) in `endings.json`; two are
  defined but never referenced (`rivalascensionapostle`, `turnasidevictory`). Victory
  pattern: major {forge, grail, lantern} + minor {forge, grail, heart, lantern,
  meniscate, moth}, each with a `withrisen` twin, plus Exile/Dancer-character victories.
- **legacy (14 + 1 orphan):** `effects {elementId:count}` = starting inventory of the
  next run, `fromending` (lowercase; aspirant = `"default"`; all 15 carry it),
  `startingverbid:"work"` on all 14 loadable ones, `statusbarelements` (aspirant only),
  `availableWithoutEndingMatch` (key on 14, but true only on 4: aspirant, detective,
  brightyoungthing, physician).
- **lever (8):** score your end-of-run cards against `weights{aspect:score}` — e.g.
  `lastfollower`: `{follower:1000, hireling:-1000, summoned:-1000, disciple:10,
  exalted:20, romanticinterest:99}`, `requiredScore` 1000. `z_levers.json` binds each
  lever to a hidden card (`{"id":"lever_lasttool","lever":"lasttool"}`).
- **portal (7):** `otherworldid` (6× `"mansus"`, `mysteries` → `"mysteries"`),
  `egressid`, `consequences:[{id, topath:"/wood0", deckeffects:{mansus_wood:1}}]` (5 of
  7) — entering a door draws from that door's deck.
- **dicta (1):** scene ids S1Logo…S6NewGame, `NoteElementId:"tlg.note"`,
  `DefaultLongTravelDuration:"1"` / `DefaultTravelDuration:"0.5"` /
  `DefaultQuickTravelDuration:"0.1"`, `DefaultGameSpeed:"0"`.

## 3. Core loop

1. **Verb stations.** Drag any card matching a verb's any-of `required` into Work /
   Dream / Study / Talk / Explore. The situation's aspect pool = sum of everything in it
   (elements count as their own aspect).
2. **Recipe selection.** Among craftable recipes with `actionid` = verb whose
   `requirements` (AND, totals, negative = below) are satisfied, one runs. `alt` lists
   give precedence tiers. **Unresolved:** the tie-break when several craftable recipes
   all match is engine-side, not in data (community bots assume "most specific wins").
   `hintonly` recipes are the grey "you almost have" suggestions.
3. **Execution.** `warmup` ticks down in seconds × speed multiplier (§4); running
   `aspects` fire xtriggers on contained cards. At completion the engine applies
   mutations → xtriggers → induces → deckeffects → effects → ending → `linked` chain
   (each entry by chance; failure falls through). Note: `xtriggers`/`induces` live on
   *elements*, not recipes.
4. **Everything is chains.** `dream_router` (actionid dream, warmup 10, no
   requirements, `deckeffects {dreamreasonresults:1}`) links to `dreamcontentment`,
   `dreamrestlessness`, `dreamvitality`, `dreamglimmering`, `nightmarehersault`,
   `nightmarecleansingdawn`; the deck spec is `[dread, fascination, glimmering,
   vitality, contentment, restlessness]`. Mansus: `dreamenterwood {waywood:1,passion:1}`
   (portaleffect wood) → draw door deck → `waywhite` → `dreamenterwhite
   {waywhite:1,health:1}` (portaleffect whitedoor) → stag-door riddle cards
   (`waystagbefore_1..5`, answered by recipes `dream_answerstagriddle_1..5`, which grant
   the `waystag_after` element; `dreamenterstag` then requires `{waystag_after:1,
   reason:1}`) → Spider → Peacock → ascension lore. Expeditions: obstacle recipe with an interleaved `alt` ladder
   `[highknock{knock:10}, highedge{edge:10}, midknock{knock:5}, midedge{edge:5},
   lowknock{knock:1}, lowedge{edge:1}]`; each tier's `linked` hard-codes
   `[{success, chance 90/70/30}, {failure, chance 100}]` (high/mid/low); both outcomes
   chain `explorevaulttick` (`effects {funds:-1}` — the per-attempt cost); failure's
   `alt` carries `[{explorevault_casualty, chance 70, additional:true, expulsion
   {limit:1, filter:{follower:1}}}]`. Summoning: rite cards (`ritual:1`, 10 of them;
   e.g. slots Invocation `{lore:1}`, Offering `{ingredient:1, consumes:true}`, Desire
   `{desire:6}`) pool aspects vs `summon_spirit_heartc_edge {ritual:1, knock:2, heart:6,
   edge:2, desire:-1}`; `linked [{summoninglosingcontrol, 30}, …success]`;
   `summoninglosingcontrol` → `alt [{summoningshutitdown 100}, {summoningfightformastery
   100}]` (Effort slot {passion,reason}) → `summoningfightformastery` linked
   `[{summoningmasteryregained, 70}, {summoningescaped, 100}]`. Victory: desire ladders
   ascend via recipe effects (a→b) and `xtriggers {ascend:next}` (c→e); the craftable
   trigger `minorlanternvictory_trigger {ritual:1, ascensionenlightenmentf:1,
   lantern:36}` → `minorlanternvictory_overture`, whose linked branches three ways
   (`minorlanternvictory`, `minorlanternvictorywithrisen`, `belovedfarewell`; the first
   fires `ending:"minorlanternvictory"`) wins the game —
   note `lantern:36` matches the wiki's "magnitude 36" final-rite rule.
5. **Seasons = the pressure system.** `needs` (actionid time, warmup 60, the only
   `signalimportantloop` recipe) drains its greedy Expenses slot, then `linked` →
   `discardseason` (`effects {season:-1, funds:-999}` — sweeps whatever Funds the
   greedy slot grabbed) → `drawseason {deckeffects {seasonevents:1}}` → back to `needs`.
   The fan-out is **`needs`' nine `additional:true` alt entries** (forlornbloodsickness at
   70%, then sickness, pleasantday, suspicion, ambition, despairrisklink, visionsrisklink,
   starvation, rosenext), each target recipe gated on the drawn season card
   (season cards themselves have no xtriggers/induces; all are `lifetime:1` instant
   markers, 98 elements at lifetime 1 incl. `strategy.prep`): **Despair**
   (`despairrisklink` → `despairrisk` with a greedy `{inducesdespair:1}` slot grabbing
   Dread → `despairabate {contentment:1}` destroys one, or **`despairdeath` with
   `requirements {inducesdespair:3}` → `ending:"despairending"`** — the "3 processed
   Dread" is literally a requirement), **Visions** (`visionsdeath {fascination:3}` →
   `visionsending`), **Sickness** (runs `aspects {fallsick:1}` so health xtriggers into
   affliction; no health/fatigue → `deathofthebody`), **Suspicion** (`linked
   [suspiciongrabreputation 100, suspiciontrycreatehunter 70]`), **Ambitions** (the
   ascension step — see §5), Serenity (`pleasantday`, gated `seasonserenities:1`),
   Ardours (`rosenext`). **No Silence entry** — a `seasonsilence` element exists but is
   wired nowhere in `needs`; gating lives on each target recipe's requirements (e.g.
   `despairrisklink` needs `seasondespair:1`, `starvation` needs `funds:-1`), not on
   the alt entries.
6. **State markers.** `unique:true` (373 elements), `uniquenessgroup` (36 groups),
   instant markers at `lifetime:1`, Notoriety decays at 300, Dread at 180, influence
   chains at 60 (`influencemothg(60)→influencemothe→influencemothc→influencemoth→
   restlessness(60)→dread(180)`). Summons have lifetimes of 120/180 (e.g.
   `spirit_wintera_edge` 120, `spirit_heartc_edge` 180, decaying to `influence*` or
   `.defunct`) — the wiki's "Risen 60 s" exception has **no element named Risen in
   core content** (probably DLC naming; flag for the clone).

## 4. Time system (IL-verified)

- Heartbeat loop: `Heart.FixedUpdate` → `ProcessBeatCounter` accumulates real time and
  fires a beat only when ≥ **0.1 s** (`minimumHeartBeatInterval = 0.1f`, decoded from
  the ctor bytes). `Beat(interval × multiplier, 0.1)` then decrements token lifetimes
  (`Timeshadow.SpendTime`) and situation warmups — so `warmup`/`lifetime` are
  numerically **seconds**, ticked at ≤0.1 s granularity.
- Speed multipliers (`GetTimerMultiplierForSpeed` switch): **0 = paused (0.0), 1 = 1.0,
  2 = 2.0, 3 = 4.0, 4 = 6.0**.
- Canonical durations: standard action 60 (modal, 38% of timed recipes); Bright Young
  Thing job 30; lore-challenge answer 30 (wiki-unverified); The Benefits of Privacy 180;
  Season of Rags 300; Dancer 45 (+70% ×10 s extra effort); begging with Decrepitude 90.
- No in-game-minute mapping exists in data — the game is pure real-time; UI shows
  second countdowns. (Chorus is free to keep its 30-min halfhour quantization.)
- `dicta` travel durations: long 1.0 / normal 0.5 / quick 0.1 (interval units);
  `DefaultGameSpeed "0"`.

## 5. Systems reference (rules the clone needs)

- **Ability cards:** Health / Reason / Passion + Funds ("four foundations of life").
  Drain variants: Health (Fatigued), Reason (Dimmed), Passion (Dimmed/Disillusioned);
  ruined forms Decrepitude/Injury/Affliction (Injury 150 s, Affliction 180 s →
  Decrepitude); funds starvation → Starvation verb eats 1 Health → Hunger, no Health →
  death.
- **Temptation → Desire (wiki-confirmed, data-consistent):** Dream + Reason →
  Temptation: Enlightenment (Lantern); Dream + Health → Sensation (Grail); Dream +
  Passion → Power (Forge); dreaming the Temptation with matching lore fixes a
  Dedication; Work "Through the Stag Door" + Dedication + magnitude-6 lore makes the
  full Desire.
- **Dread / Fascination:** spawned by `induces` rolls (30% on dreadful/fascinating),
  dream results, grim lore-challenges; defused by Dream + Contentment (→ Fleeting
  Reminiscence) or each other. Death math in §3.5 — the famous "Dread 9 = death" figure
  appears in **no** wiki source; the real rule is 3 processed by the season verb.
- **Funds:** Unskilled Labor 60 s → 1 Funds + Vitality, 30% injury; clerical ladder at
  Glover & Glover (best basic pay once Senior); commissions pay Silver Spintria;
  begging with Decrepitude 90 s → 1 Funds + Dread, with a 30% chance of 2 Funds and no
  Dread; expeditions/auctions/cult loot; Apostle jobs → Funds 7.
- **Rites:** the rite used doesn't affect the ritual — only which component types feed
  the Invocation/Offering slots; the ritual is picked by total aspect pool; **any Knock
  in the pool disqualifies everything but summoning** — and this is data-visible: all
  25 non-summoning rite-situation recipes carry `knock:-1` (13 `arcane_*` lore-crafting,
  12 `ritehinthas*`), while summons require knock ≥ 2. Summons are temporary — ordinary
  spirits 180 s in data (wiki agrees; only the two Risen last 60 s, and "Risen" is not
  in core JSON — see §3.6); dismissal = Reason (safe) or Passion (gamble).
  Transformation-rite failure rates (per wiki, quoting game files): Grand 10% / Major
  30% / Minor 70%.
- **Expeditions:** Explore + SH lore reveals vaults (SH 2/4/6/8/10/12/14 tiers); vault
  card + 2 follower slots + 2 Funds slots; each obstacle attempt costs 1 Funds; success
  tiers in the data are hard-coded 90/70/30 (§3.4) — the wiki's prose tiers (1–5→30%,
  5–10→70%, >10→90%) agree with the engine's base convention table (≥5→70); failures
  "may injure a follower" (wiki, Explore) though A Warded Door is scripted harmless
  with a 30 s retry; completion → treasure + 1 Notoriety; curses 30% per 60 s.
- **Cult:** Talk + lore → Acquaintance (needs Passion) or Patron (needs Reason), +
  Mystique. Promotions: Believer ≥1 principle (30 s) → Disciple ≥7 (60 s) → Exalted
  ≥21 (own principle, named followers). Cult business: aspect totals 1→30% / 5→70% /
  10→90%; failure → Notoriety + 30% wound chance. Side effects: **Edge and Grail both
  capture prisoners**; Heart destroys reputations; Forge lab loot; Knock burglary; Moth
  cons; Lantern augury; Winter murder → corpse.
- **Hunter:** Season of Suspicion spawns one if reputations exist (first = Weary
  Detective). Each minute it grabs an evidence card until it finds Notoriety → converts
  to Tentative (lvl 1) → Damning (lvl 2) at 70% (Meticulous 100%, Erratic 10% even
  without Notoriety). Damning Evidence + court = arrest ending unless Favour from
  Authority. Counters: Moth destroys evidence (failure → wounds + Notoriety), Winter
  lore sway 70% (only 30% below Winter 8 for the Grim hunter → Rival; Idealist immune
  to Winter, Grim immune to Lantern), Lantern lore mirror-sway, Edge minion kill
  10/30/70/100% at Edge 0/1+/5+/10+, Winter minion 10/30/70%.
- **Endings (48) & legacies (14):** Grand victories per principle; Melancholy deaths
  (despair/visions/body/arrest); ascension = desire marks 3→6, Exotic cravings
  (prisoners for Enlightenment/Sensation, funds for Power; failing burns an ability and
  drops a mark, floor 3), final rite ≥36 magnitude (27 Grail with willing sacrifice) —
  matching the data's `minorlanternvictory_trigger {lantern:36}`. Legacies = New Game+
  starting inventories gated by the ending, Aspirant always available, three offered at
  a time; DLC legacies: Dancer, Priest, Ghoul (legacy name "Medium?"), Exile (replaces
  Seasons with a 60 s Reckoner roll: 10% at 0 Traces, 30% at 1, 70% at 2+; 7 Wounds →
  death). Levers carry "who/what survived" into the next run.

## 6. Mapping onto Chorus (rewritten 2026-10-15 against the Rust engine)

This section **supersedes the v1 mapping**, which was written against the retired
TypeScript prototype (`src/engine.ts`) and is obsolete. The engine is now Rust
(`src/engine/`, `src/core/…`), and every "engine gap" the v1 listed has since
been closed in code. Every claim below was verified against the current source on
2026-10-15 (same pass as §9); `src/core/types.rs` and the spec sections cited in the
code comments are ground truth.

**Maps directly (zero engine changes needed)**

| CS | Chorus (Rust, verified) |
|---|---|
| verb stations (7) + slots | `verbs:` script section + processing windows (§6.7, commit-time recipe snapshot); all 7 lexemes already in `CORE_VERBS`: WORK, DREAM, TALK, EXPLORE, STUDY, WAIT, MANSUS (`dict.rs:13`) |
| recipe requirements (AND, totals) | recipe `when:` predicates (`total(...) >= 2`, quantifiers) |
| negative requirements (absent) | negated terms / `< 1` comparisons |
| warmup + progress bar | `OpenWindow` start/end + authored `PROGRESS` |
| effects (+/− elements) | `SPAWN` / `DESTROY` / `REPLACE` outcomes (`OUTCOME_OPS`, `dict.rs:80`); `funds:-999` = DESTROY across the situation's bound copies |
| unique cards | unique-name law (§5.1) |
| card stacking (3 Dread) | `stackable` declarations + name pools + oldest-named addressing (§4.2) |
| lifetimes / `decayTo` transforms | `SET x, expires, Nm` + authored `on expire of NAME` triggers (§6.6) — a transform, not just destroy |
| chance (`linked` rolls, induces) | `ROLL`/`ROLLED` + the seeded `State.rng` counter; each draw lands in the log as `ROLLED(total, ctrAfter)` so the fold replays deterministically (§4.4) — v1's recommended option (b), shipped |
| `deckeffects` / `internaldeck` | `DRAW`/`DRAWN` + per-deck cursors + `draw_from` exhaustion rules (§4.4) |
| endings (48) | `ENDING` outcome + `State.ended` (§4.4) |
| `extantreqs` / `tablereqs` | `exists()` / `count()` predicates over the whole living world (§4.5; `core/pred_parse.rs`) — the v1 "not expressible" note is obsolete |
| conditional chains / challenge rolls | `IF`/`ELSE` outcome arms (§6.8) reading the last `ROLL`; strict arms reject the step |
| hintonly suggestions | `shown` + `hint` recipes |
| followers/cult/social graph | relations (8 registered types) + multi-actor roles |
| timetables pressing the player | once-triggers (`State.fired`, §6.6) |
| `signalEndingFlavour` | `Ending {id, title, flavour}` |

**Needs authoring translation — IMPLEMENTED (engine v0.3.6, `text-engine-v0.7.md`
§6.7/§4.5/§6.4: RUN chains, `on complete` triggers + recipe `aspects`, query
alternation, `max`; commits 3cc1ced §4.5, b1fce1a §6.4, eb350c3 §6.7,
69b2977 §6.7)**

- **`linked` chains** (needs→discardseason→drawseason→needs; expedition ticks): there
  is no "start another recipe" outcome — inline each chain into one outcome list,
  rolling every former link with `ROLL` and branching with `IF`. A chain loop becomes
  one repeating WAIT recipe.
- **`xtriggers`** (running recipe aspects transform contained cards, e.g.
  `fatiguing`→fatigue): no compiled hook — each recipe that runs with such aspects
  authors the REPLACE/SET outcomes itself. Cheap for the slice; the real cost centre
  of a full 2,084-recipe conversion.
- **`induces`** (post-recipe chance hook on a card): same inline treatment.
- **Any-of verb slot** (`ability OR job OR ritual…`): slot queries are conjunctions;
  gate via `when:` predicates with `any()`/`exists()` — one recipe per entry aspect
  (v1's idiom stands).
- **`maxexecutions`** → `SET used` flags + `attr < 1` gates (v1's idiom stands).

**The one remaining engine-side question: the background tick**

`State.windows` already holds several simultaneous stations, but time advances only
through player steps (`ADVANCE`/`WAIT`) — there is no auto-tick pump anywhere in
`src/session`/`src/engine`, and CS's Time-Passes runs unattended. Two routes:
(a) accept turn-driven time — Time Passes is an explicit repeating WAIT verb (the
Nightjar model, pure data, enough for the slice); (b) add a small pump that commits
synthetic WAIT steps for elapsed spans. (b) is a design decision, not a blocker —
and the v0.3.6 `job→needs` induce (§6.7 cross-verb RUN) makes it moot for
the slice: the first work-like completion bootstraps the self-rechaining seasonal
loop, which then suppresses later bootstraps exactly as CS's does.

**Scale note.** Core content is 2,084 recipes / 1,566 elements. With the v1 gap list
closed, both scopes survive in updated form:

- **Vertical slice (recommended):** unchanged in shape — Work/Funds, Time Passes +
  Seasons (Despair/Visions/Suspicion/Ambitions — Silence is not wired into `needs`,
  §9-6), Study with a lore ladder, Dream chain to one Door, Talk
  acquaintances→cult→one promotion, one rite + one summon, one desire ending +
  despair/visions endings. ~100–200 authored recipes, **zero engine changes**.
- **Converter:** the mechanical mapping is now genuinely close — every CS mechanism
  has a Chorus landing spot; the cost is the per-recipe inlining of
  linked/xtriggers/induces above. It would still carry copyrighted
  `label/description` strings — must re-text, not copy.

## 7. Artifacts

- Extracted content (working copy, not in repo). **Durable local copy:
  `/Users/amias/Projects/cs-re/`** (2026-10-15: `core/` 160 JSONs byte-verified
  per-file against the install over ssh — zero size mismatches — and all 160 parse
  via `loadlib.py`; entity counts match §2, including the `legcies` orphan; both game
  DLLs included). The 6 `loc/` dirs (20,646,951 bytes) were archived there the same
  day — reference only; core is authoritative (see §9-8). The original `/tmp/cs-re/`
  is disposable scratch. Contents:
  `core/` + `loadlib.py` (json5 loader with newline repair) + analysis outputs
  (`analysis_*.txt`, `samples.json`) + the two game DLLs for further IL reads.
- Verification tooling left by the audit (§8): `/tmp/cs-verify2..4/` (JSON census and
  IL-decoder scripts; `/tmp/cs-verify4/ildasm.py` is a reusable hand-written IL reader).
- Small engine bug found while surveying: `src/engine.ts:436-443` `toMinutes` (used for
  `expires`) accepts only h/m/d — not `halfhour`, unlike `parseDuration` (clock.ts:41).
  Blast radius is narrow: only *quoted-string* values hit the broken regex; bare
  duration literals are converted to minutes by the lexer first, so
  `SET(x, expires, 2halfhour)` works while `SET(x, expires, "2halfhour")` rejects the
  step. *(TS-era; moot since the Rust rewrite.)*

## 8. Verification audit (2026-10-12)

Six independent adversarial checkers re-derived the claims of v1 of this document:
two over the extracted JSON (schema census; chain tracing), one over the install via
SSH, one re-decoding the IL from scratch, one against the Fandom wiki (~50 pages), one
against `src/` + spec. Headline corrections applied above:

1. **Challenge convention table reversed.** v1's IL decode (base 3–9→70, 1–2→30) was
   wrong; the re-decode from raw bytes proves base ≥5→70 / 1–4→30 (advanced
   ≥5→10 / 10–14→30 / 15–19→70), matching the wiki. The compared value is the raw
   aspect total, and the result is the max across entries.
2. **Mansus door costs were shifted one door late.** Correct chain: Wood←Passion;
   White Door←Health (exhausted); Stag riddle←Desire/Dedication + magnitude-6 answer;
   Stag Door←Reason (exhausted); Spider's Door←consumed Prisoner; Peacock's Door←a
   tool (cracked mirror / Frangiclave). Data agrees (`dreamenterwhite {health:1}`).
3. **"DLC absent from core" was false** — 7 DLC files ship inside core.
4. **"0 recipes use `challenges`"** — the 90/70/30 expedition curve is hard-coded per
   tier; `ChallengeArbiter` exists but is unused in core content.
5. Counts fixed: craftable false = 489 (not 490); 6 vault regions (not 7); 13 distinct
   lifetime values (not 6); only 4 of 8 principles induce; core size 3.0 MB; loc dirs
   are full-copy subsets under `content\`, not label/description-only.
6. **`signalEndingFlavour` Vile occurs** (1 recipe) and points at an ending id that is
   never defined — a second shipped data bug, alongside the `legcies` legacy typo.
7. **"Risen" not in core content**; ordinary summons decay at 120/180 s.
8. Season fan-out runs through `needs`' additional alts (not card triggers);
   `despairdeath` encodes the "3 Dread" rule as `requirements {inducesdespair:3}`;
   Time-Passes sweeps Funds with `funds:-999` from a greedy slot.
9. Chorus mapping: `extantreqs`/`tablereqs` are **not** expressible (no
   world-existential predicate — predicates see bound roles only); prototype `derive`
   is the single literal-arithmetic exception to "no outcome math"; the `expires`
   unit bug only bites quoted strings. *(Superseded 2026-10-15: all three were
   TS-prototype findings — the Rust engine shipped `exists()`/`count()`, `SET`
   deltas, and authored expire triggers; see the §6 rewrite and §9.)*

Remaining unknowns (documented as unknown): the multi-match recipe tie-break (engine
code, not in data); season rotation order; where "Risen" content lives (likely DLC);
UI "sec"/"min" label strings (not in either DLL — localization assets); Exile's exact
Reckoner curve (wiki Trace page vs data, DLC not installed).

---

## 9. Re-verification (2026-10-15) — four independent checkers

Four subagents re-derived §1–§5 claims from the durable copy
(`/Users/amias/Projects/cs-re/core/`); one also re-decoded the IL with a fresh
decoder, one audited the remote install over ssh. All corrections are applied above;
the headline outcomes:

1. **Advanced challenge table fixed** — the §2 table had kept stale one-bucket-shifted
   values that contradicted §8 correction 1's own text. Third decode confirms:
   advanced ≤4→0 / 5–9→10 / 10–14→30 / 15–19→70 / ≥20→90. Base table and the
   max-across-entries rule reconfirmed, as were heartbeat 0.1 s and the speed
   multipliers.
2. **`expulsion` lives on `alt`, not `linked`** — 0 linked entries carry it; the 27
   expulsion entries are alt entries (expedition casualties).
3. **`challenges` IS used** — 21 linked entries (Exile long-recipe files, all
   `"base"`); the old "0 core recipes use it" holds only for the top-level key.
4. **`greedy` is recipe-only** — all 108 uses sit in recipe slots, 0 in element slots.
5. **induces carriers are 14, not 8** — `dance`, `secret` and four `poison.*` elements
   were missing from the earlier inventory.
6. **No Silence in the season fan-out** — `needs` has exactly nine `additional` alts
   (forlornbloodsickness 70%, sickness, pleasantday, suspicion, ambition,
   despairrisklink, visionsrisklink, starvation, rosenext); `seasonsilence` exists but
   is wired nowhere.
7. **Trailing-comma census: 33 files, not 50** (BOM 9 / unquoted keys 3 / raw
   newlines 2 — all reconfirmed).
8. **loc dirs: es/fr are not core subsets** — they add `portals\`, use suffixed
   filenames and carry ~160 DLC files core lacks; de/jp/ru/zh-hans each lack the same
   20 core files. Loc copies can DROP functional fields (loc_de `lockscaredge` loses
   `burnTo`); no loc copy of versionnews.json exists.
9. **Knock disqualification is data-visible** — 25 non-summoning rite-situation
   recipes carry `knock:-1`; summons require knock ≥ 2.
10. Everything else reconfirmed: the full §2 census (2084 recipes / 1566 elements /
    67 decks / 48 endings / 14+1 legacies / 8 levers / 7 portals / 90 achievements,
    warmup and lifetime histograms, craftable/hintonly splits, 791 requirement keys
    with the 592/199 split, 596 negatives, linked 1473 with 8 inline, alt 748 with 3
    inline), every §3 chain (needs cycle, despair/visions, dream router, full Mansus
    ladder, expedition alt ladder with 90/70/30 and the casualty expulsion, rite
    chain, lantern-36 trigger), the exemplar entities verbatim, version.txt, and the
    7 DLC files in core.
