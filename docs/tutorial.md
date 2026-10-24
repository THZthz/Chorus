# Authoring and Analyzing Chorus² Scripts with Convergent Phases

This tutorial introduces how to write Chorus² (`.sw`) story scripts, how to structure narratives into **convergent phases** to prevent state-space explosion, and how to verify design contracts using [`story-cli`](file:///Users/amias/Projects/chorus/crates/story-cli/src/main.rs).

---

## 1. Core Language Concepts

A `.sw` script declares the static rules and initial state of a story world:

* **Header (`game`)**: Sets the content title, seed, turn step (advance granularity), home drop zone, and clock horizon.
* **`zones`**: Named containers for entities (e.g. `workshop`).
* **`aspects`**: Integer meters tracked by entities (e.g. `heat`, `purity`, `frequency`).
* **`cards`**: Entities and mint templates.
  * `unique`: Exactly one instance exists at a time; addressable by name path (`crucible.heat`).
  * `stackable`: Multiple identical copies allowed; queried via scope matching (`count(zone:workshop, aspect:ore >= 1)`).
  * `start`: Instantiates initial cards at t=0.
* **`verbs` & `recipes`**: The player's actions and story mechanics.
  * `committable`: Player-driven situations.
  * `slots`: Receptacles that accept candidate cards based on query filters. Configured with flags:
    * `consuming`: Cards in the slot burn upon settlement.
    * `open`: Allows mid-run card placement while the recipe timer runs.
    * `greedy`: Automatically grabs and locks matching cards from the world.
  * `when`: Preconditions for recipe activation.
  * `guard`: Blocks a commit with feedback if an unsafe threshold is exceeded.
  * `duration`: Minutes required to resolve (0 = instant).
  * `effects`: State modifications applied upon completion (`spawn`, `consume`, `set`, and `continue via <recipe>`).
* **`endings`**: Terminal story conclusions.

## 2. Dynamic Timed Situations (Cultist Simulator Style)

Chorus² natively supports the dynamic situation loop popularized by *Cultist Simulator* and *Book of Hours*: a situation window opens, starts ticking down, exposes new slots mid-flight (which may auto-grab cards), demands player intervention within a strict time limit, and branches or fails if the player does not react in time.

### Core Primitives

1. **Mid-Run Open Slots (`open`)**:
   * Slots marked `open` (e.g. `remedy: 0..1 open aspect:medicine`) do not require cards at the initial commit when `min = 0`.
   * While the situation timer is in flight, the player can dynamically place cards into the open slot using `bind <sit> <slot> <card>` in [`story-cli play`](file:///Users/amias/Projects/chorus/crates/story-cli/src/main.rs#L159) or via [`Engine::bind`](file:///Users/amias/Projects/chorus/crates/story-engine/src/lib.rs#L407).

2. **Automatic Card Grabbing (`greedy`)**:
   * Slots marked `greedy` (e.g. `threat: 0..1 greedy aspect:peril`) automatically pull matching cards from the table when the situation opens or adopts a new recipe, immediately locking them into the situation.
   * `greedy open` combines both: auto-pulls matching candidates if available, while still permitting manual player placement.

3. **Multi-Stage Continuation (`continue via <recipe>`)**:
   * Instead of completing and closing the situation, a recipe effect can trigger `continue via <next_recipe>` ([spec.md §4.2](file:///Users/amias/Projects/chorus/docs/spec.md#L81)).
   * The situation instance ([`Situation`](file:///Users/amias/Projects/chorus/crates/story-core/src/state.rs#L96-L107)) remains open on the board, non-consuming bindings are preserved as interior storage, the timer re-arms for the new recipe's `duration`, and the new recipe's slots become active.

4. **Timed Challenges & Failure Handling**:
   * The recipe's `duration` defines the player's response window.
   * When time expires, settlement runs the recipe's `effects`. An `if / else` block inspects whether the demanded slot was filled (`count(slot:remedy, ...)`), branching to a success recipe via `continue via success` or a penalty/failure recipe via `continue via failure`.

### Authoring Pattern

```sw
verbs
  verb expedition committable instances 1
    # Stage 1: Player commits initial scout and supplies
    recipe embark
      duration 30m
      slots
        scout: 1..1 aspect:mortal
        supplies: 1..2 consuming aspect:funds
      effects
        narrate "The expedition departs into the mist."
        continue via hazard

    # Stage 2: An obstacle appears with a 60-minute response window.
    # It auto-grabs a threat (greedy) and demands a ward (open).
    recipe hazard
      duration 60m
      slots
        threat: 0..1 greedy aspect:peril
        defense: 0..1 open consuming aspect:ward
      effects
        if count(slot:defense, aspect:ward) >= 1
          narrate "The ward repels the peril!"
          continue via triumph
        else
          narrate "No ward was placed in time. The expedition is overwhelmed!"
          continue via disaster

    recipe triumph
      duration 10m
      effects
        spawn relic
        narrate "Relic recovered safely."

    recipe disaster
      duration 0
      effects
        destroy slot:threat
        narrate "The expedition perished."
```

In interactive play (`story-cli play`), the player opens the situation with `commit expedition scout=alice supplies=gold_coin`, advances time into the hazard (`advance 30`), inspects the active open slot via `state`, binds the defense card before the timer runs out (`bind 0 defense talisman`), and advances to completion. The relic mints into the triumph interior; at completion the interior survivor releases to the verb's `releases` zone (the default `home`).

> Duration literals are `<n>m`, `<n>h`, or `<n>h<m>m` (a recipe's `duration` also takes a bare non-negative integer for minutes) — there is no seconds unit (language.md §1).

---

## 3. Advanced Phase Architecture: "Checkpoints Mostly-One-Way Graph"

### The Problem: State-Space Explosion
If an automated analyzer attempts to explore a whole game from start to finish across all verbs simultaneously, the number of possible states grows exponentially ($O(b^d)$). Exhaustive exploration quickly exhausts memory or time budgets.

### The Solution: Checkpoints Mostly-One-Way Graph
Beyond a simple linear pipeline (Phase 1 → Phase 2 → Phase 3), real game design uses a **macro graph of checkpoints** with local loops:

```
               ┌──[Alpine Route]──> Checkpoint 1 (Highlands) ──┐
Checkpoint 0 ──┤                                               ├──> Checkpoint 3 (Summit) ──> Ending
 (Basecamp)    └──[Grotto Route]──> Checkpoint 2 (Caverns)   ──┘
      ▲                                │               │
      └──────────[Retreat on Exhaustion]───────────────┘
```

1. **Explicit Checkpoints ($C_0, C_1, C_2, \dots$)**:
   * Each checkpoint represents a milestone anchor marked by an entity or aspect (e.g., `card milestone` with aspect `checkpoint: 1`).
   * When advancing to a new checkpoint, transient working inventory (e.g. intermediate raw scraps, scrap ore) is consumed or purged (`destroy ... all`), collapsing the state manifold down to the checkpoint anchor.
2. **Local Flexibility (Intra-checkpoint Loops)**:
   * Within each checkpoint region, players can loop freely: gathering, crafting, exploring, or upgrading.
3. **Macro Progression (Mostly-One-Way Directed Acyclic Graph)**:
   * **Forward Edges**: Transitions between checkpoints are one-way (e.g. crossing mountain gates, entering new chapters). Crossing locks previous region recipes (`when milestone.checkpoint == N`).
   * **Branching & Confluence**: Multiple paths can diverge (e.g. Highlands vs. Caverns) and re-converge at downstream milestones (Summit).
   * **Controlled Fallbacks ("Mostly" One-Way)**: If a retreat edge exists (e.g. falling back to Basecamp upon running out of stamina), it strictly resets to the canonical prior checkpoint state rather than branching arbitrarily, preventing infinite state-space divergence.
4. **Independent Edge Verification**:
   * Every directed edge $(C_i \to C_j)$ in the checkpoint graph is verified as an isolated slice (`--slice arc:<verbs> --assume "aspect:milestone.checkpoint=<i>"` — an aspect assumption seeding the target checkpoint's anchor).
   * Verifying each edge independently reduces a multi-million-state game graph to a few dozen bounded checks of ~300 to ~700 states each.

---

## 4. Complete Self-Contained Example

Save the following content to `runecraft.sw`:

```
game "Runecraft"
  seed host
  turn 5m
  home workshop
  meta horizon "6h"

zones
  workshop

aspects
  heat: 0
  fuel: 0
  ore: 0
  coolant: 0
  density: 0
  purity: 0
  precision: 0
  charge: 0
  frequency: 0
  resonance: 0

endings
  ending ascension "The Astral Gate Opens" grand
    text "The celestial conduit stabilizes the portal."
  ending meltdown "Furnace Rupture" somber
    text "The crucible cracked under extreme heat."

cards
  card crucible
    kind apparatus  unique
    aspects heat 0
    start in workshop count 1

  card coal
    kind fuel  stackable
    aspects fuel 1
    start in workshop count 4

  card raw_ore
    kind mineral  stackable
    aspects ore 1
    start in workshop count 2

  card brine
    kind coolant  stackable
    aspects coolant 1
    start in workshop count 2

  card base_plate
    kind metal  unique
    aspects density 0

  card tempered_plate
    kind metal  unique
    aspects purity 0

  card chisel
    kind tool  unique
    aspects precision 1
    start in workshop count 1

  card aether_shard
    kind gem  stackable
    aspects charge 1
    start in workshop count 2

  card runic_core
    kind artifact  unique
    aspects frequency 0

  card gateway
    kind structure  unique
    aspects resonance 0
    start in workshop count 1

verbs
  # --- Phase 1: Metallurgy Arc (arc:smelt,temper) ---
  verb smelt committable instances 1
    recipe melt
      craftable
      duration 10m
      guard "Crucible overheating" when crucible.heat >= 4
      when count(zone:workshop, name:base_plate) < 1
      slots
        fuel_slot: 1 consuming aspect:fuel
        ore_slot: 1 consuming aspect:ore
      effects
        set crucible.heat += 2
        spawn base_plate into workshop
        set base_plate.density = 2
        narrate "Melted ore into a base metallic plate."

    recipe rupture
      craftable
      duration 0m
      priority 10
      when crucible.heat >= 6
      effects
        ending meltdown

  verb temper committable instances 1
    recipe quench
      craftable
      duration 10m
      when crucible.heat >= 2 and base_plate.density >= 2
      slots
        plate_slot: 1 consuming name:base_plate
        coolant_slot: 1 consuming aspect:coolant
      effects
        set crucible.heat -= 2
        spawn tempered_plate into workshop
        set tempered_plate.purity = 2
        narrate "Quenched the plate into a pure tempered alloy."

  # --- Phase 2: Inscription Arc (arc:inscribe,charge) ---
  verb inscribe committable instances 1
    recipe carve
      craftable
      duration 15m
      when tempered_plate.purity >= 2
      slots
        plate_slot: 1 consuming name:tempered_plate
        tool_slot: 1 aspect:precision
      effects
        spawn runic_core into workshop
        set runic_core.frequency = 1
        narrate "Etched harmonic channels into the plate."

  verb charge committable instances 1
    recipe infuse
      craftable
      duration 15m
      max 2
      when runic_core.frequency >= 1 and runic_core.frequency < 3
      slots
        core_slot: 1 name:runic_core
        shard_slot: 1 consuming aspect:charge
      effects
        set runic_core.frequency += 1
        narrate "Infused an aether shard into the core."

  # --- Phase 3: Activation Arc (arc:attune,breach) ---
  verb attune committable instances 1
    recipe link
      craftable
      duration 10m
      when runic_core.frequency >= 3
      slots
        core_slot: 1 consuming name:runic_core
        gate_slot: 1 name:gateway
      effects
        set gateway.resonance = 3
        narrate "Synchronized the runic core with the gateway."

  verb breach committable instances 1
    recipe activate
      craftable
      duration 10m
      when gateway.resonance >= 3
      effects
        ending ascension
        narrate "The astral gateway awakens."
```

---

## 5. Analyzing Each Phase with `story-cli`

### Step 1: Diagnose Phase 1 in Isolation
Isolate verbs with `--slice arc:<v1,v2>` and test if the converged collection is reachable:

```bash
story-cli analyze runecraft.sw \
  --slice arc:smelt,temper \
  --reach "when tempered_plate.purity >= 2" \
  --envelope
```

* **Interface Inspection**: The tool automatically extracts the slice's [`InterfaceBoundary`](file:///Users/amias/Projects/chorus/crates/story-analyze/src/slice.rs#L81) and prints it as `interface facts:`:
  * `assumes (inputs from outside):` — here `(none — self-contained on inputs)`, boot state is sufficient.
  * `guarantees (outputs to outside):` — `card "tempered_plate" live`, `card "tempered_plate" has aspect "purity"`; an `internal:` list follows with the facts that never leave the slice.
  * The guarantee list is the causal graph's static over-approximation: it also lists `ending "meltdown" reached`, which the reach check below disproves.
* **Reachability**: `outcome: REACHABLE — 1 witness(es)` with a replay-verified witness — 2 actions (`commit smelt` → `advance 10m` → `commit temper` → `advance 10m`, final clock 20m).
* **Coverage Certificate**: [`ExploredEnvelope`](file:///Users/amias/Projects/chorus/crates/story-analyze/src/envelope.rs#L13) prints under `explored envelope:` — e.g. `states: 9 evaluated, 8 canonical states discovered, time: [0m, 20m]` with aspect ranges and landmark coverage.
* **Negative Safety Check**: Verify the crucible cannot rupture:
  ```bash
  story-cli analyze runecraft.sw \
    --slice arc:smelt,temper \
    --reach "ending:meltdown"
  ```
  Prints `outcome: UNREACHABLE — no state within the bounds satisfies the goal`, proving the heat guard reliably prevents rupture.

---

### Step 2: Diagnose Phase 2 with Precondition Assumptions
Phase 2 depends on Phase 1's output. Seed this boundary collection using [`Assumption`](file:///Users/amias/Projects/chorus/crates/story-analyze/src/slice.rs#L68) (`--assume`):

```bash
story-cli analyze runecraft.sw \
  --slice arc:inscribe,charge \
  --assume "card:tempered_plate=1, aspect:tempered_plate.purity=2" \
  --reach "when runic_core.frequency >= 3" \
  --envelope
```

The tool echoes the injected boundary under `applied assumptions:` and the slice's `assumes` list carries the seeded facts.

* **Reachability**: `REACHABLE` — a 3-action witness (carve, infuse, infuse; final clock 45m, replay-verified).
* **Precondition Threshold Verification**: Test with insufficient purity:
  ```bash
  story-cli analyze runecraft.sw \
    --slice arc:inscribe,charge \
    --assume "card:tempered_plate=1, aspect:tempered_plate.purity=1" \
    --reach "when runic_core.frequency >= 3"
  ```
  Returns `UNREACHABLE`, proving Phase 2 cannot proceed if Phase 1 underperforms.

---

### Step 3: Diagnose Phase 3 to Ending
Test Phase 3 assuming Phase 2's converged core:

```bash
story-cli analyze runecraft.sw \
  --slice arc:attune,breach \
  --assume "card:runic_core=1, aspect:runic_core.frequency=3" \
  --reach "ending:ascension" \
  --envelope
```

* **Reachability**: `REACHABLE` — a 2-action witness that closes with `final: the story ends "ascension"`.
* **Threshold Verification**: Rerun with `--assume "card:runic_core=1, aspect:runic_core.frequency=2"` to confirm it returns `UNREACHABLE` if the core is not fully charged.

---

## 6. CLI Command Reference

### Interactive Play (`story-cli play <file.sw>`)

`story-cli play <file.sw> [--seed N] [--record <file.json>]`: `--seed N` supplies the host seed when the content declares `seed host` (default 1); `--record <f>` writes the forward play trace to a JSON file on exit. The bare form `story-cli <file.sw>` is an alias for `play`.

| Command | Purpose | Example |
|---|---|---|
| `verbs` | List all declared verbs and recipes | `verbs` |
| `commit <verb> [slot=card ...]` | Open a situation with initial card bindings; `slot=#<id>` addresses an entity directly when several copies share a name | `commit smelt fuel_slot=coal ore_slot=raw_ore` |
| `bind <sit> <slot> <card>` | Bind a card into an `open` slot of an active situation mid-run | `bind 0 defense talisman` |
| `give <card> [n]` | Mint `n` copies (default 1) of a declared card into the home zone | `give coal 4` |
| `advance [n]` | Advance time by `n` turns (default 1; settles due timers and completions) | `advance 2` |
| `state` | Display current entities, zones, situations, and aspects | `state` |
| `trace <file>` | Write the play trace so far to a JSON file | `trace run.json` |
| `undo` | Revert to previous player action | `undo` |
| `help` | List the commands | `help` |
| `quit` (`q`) | Exit the player session | `quit` |

### Design Analysis Flags (`story-cli analyze <file.sw> [options]`)

Analyses (at least one required unless `--audit` is given):

| Flag | Purpose | Example |
|---|---|---|
| `--reach <goal>` | Reachability with replay-verified witness paths; goal forms `ending:<id>`, `card:<name>`, `when <condition>` | `--reach "when core.freq >= 3"` / `--reach ending:ascension` |
| `--classify` | Winnable states, points of no return, and softlocks (takes its goal from `--reach`) | `--classify --reach ending:ascension` |
| `--mdp [<goal>]` | Optimal-play values V_max / V_minimax / V_unif | `--mdp "when core.freq >= 3"` |
| `--audit` | Recipe shadowing, dead content, and reachable faults | `--audit` |
| `--pareto <objs>` | Trade-off frontier and counterfactual cuts; comma-separated `max(<expr>)` / `min(<expr>)` objectives; requires `--reach` | `--pareto "max(crucible.heat)"` |

Slicing and seeding:

| Flag | Purpose | Example |
|---|---|---|
| `--slice <target>` | Restricts exploration to one verb (`verb:<id>`) or one arc ([`SliceTarget`](file:///Users/amias/Projects/chorus/crates/story-analyze/src/slice.rs#L35)) | `--slice arc:smelt,temper` |
| `--assume <facts>` | Injects boundary facts ([`Assumption`](file:///Users/amias/Projects/chorus/crates/story-analyze/src/slice.rs#L68)): `card:<id>[=<n>]`, `legacy:<key>=<n>`, `aspect:<card>.<key>=<n>`, `when <condition>` | `--assume "card:core=1, aspect:core.freq=3"` |
| `--samples <n>` | Seeds the reachability search with `n` runtime rollouts | `--samples 20` |
| `--seed-trace <file>` | Seeds the search with an external play trace | `--seed-trace run.json` |
| `--radius <k>` | Neighborhood radius around seeds (default 1 when seeded) | `--radius 2` |

Modifiers:

| Flag | Purpose | Example |
|---|---|---|
| `--rng <mode>` | Chance-point resolution: `deterministic` (default), `optimistic`, `adversarial` | `--rng optimistic` |
| `--max-states <n>` | Expansion bound (default 100,000) | `--max-states 5000` |
| `--max-time <t>` | Clock horizon: bare minutes, `<n>m`, `<n>h`, or `<n>h<m>m`; overrides the content's `meta horizon` | `--max-time 2h` |
| `--turn <duration>` | Overrides the game's turn length | `--turn 10m` |
| `--diverse <k>` | Reports up to `k` structurally distinct reach witnesses | `--diverse 3` |
| `--seed N` | Host seed when the content declares `seed host` | `--seed 7` |
| `--prune` | Heuristic feasibility pruning (spec.md §8); a pruned empty answer reads `NO WITNESS UNDER PRUNING`, never `UNREACHABLE`; witness-bearing answers are unaffected | `--prune` |
| `--envelope` | Prints the explored envelope coverage report | `--envelope` |
| `--profile` | Prints accumulated span wall times to stderr when the run ends | `--profile` |
| `--jobs <n>` | Section concurrency cap (default 1; a full exploration can hold gigabytes — raise only for content whose state space fits memory twice over) | `--jobs 2` |

Exit codes: analyses exit 0 even when the answer is "unreachable"; 1 on compile or boot failure; 2 on usage errors.
