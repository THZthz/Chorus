# Chorus²

Chorus² is a rule-driven narrative card simulation engine and design analysis framework. Content is authored in a dedicated script language (`.sw`) and executed by a pure mechanical kernel, decoupled from presentation.

---

## Workspace Structure (`crates/`)

The workspace is organized into modular crates:

- [`crates/story-core`](crates/story-core): Foundational runtime data structures (`State`, `Entity`, `Situation`, `Zone`, `Deck`, canonical IDs, PRNG).
- [`crates/story-content`](crates/story-content): Intermediate representation (IR) of compiled story declarations and runtime diagnostics.
- [`crates/story-lang`](crates/story-lang): Compiler frontend, lexer, parser, and AST lowering for `.sw` story scripts.
- [`crates/story-engine`](crates/story-engine): Pure mechanical simulation kernel enforcing game semantics, recipe selection, commitment, settlement, and snapshot/restore (Kernel Law L6).
- [`crates/story-present`](crates/story-present): Thin presentation layer providing token interpolation, two-tier catalog cascade (`<game>.narrative.json` > DSL metadata), lifecycle event hooks, and presentation view models.
- [`crates/story-analyze`](crates/story-analyze): Design-time static analysis, causal graphs, reachability search, MDP verification, and replayable traces.
- [`crates/story-cli`](crates/story-cli): Developer CLI driver providing the interactive narrative REPL (`play`) and analysis runner (`analyze`).
- [`crates/cs2sw`](crates/cs2sw): Cultist Simulator content converter and main-story companion narrative catalog exporter.

---

## Quickstart & Common Usage

Build the workspace:
```bash
cargo build --release
```

Run tests across all crates:
```bash
cargo test --workspace
```

---

## Interactive Driver (`story-cli`)

`story-cli` runs interactive story play sessions and static design analyses.

### Playing a Story

Launch the interactive REPL:
```bash
# Play Emberwatch sample
cargo run -p story-cli -- play docs/emberwatch.sw

# Launch with verbose output and runnable recipe filtering
cargo run -p story-cli -- play docs/emberwatch.sw --verbose --available
```

When `<file>.narrative.json` or `<stem>.narrative.json` exists alongside the `.sw` file, `story-cli` automatically loads the companion presentation catalog.

### Common Interactive Commands

Within the `story-cli` prompt (`>`):

- `state`: Display current game clock, legacy variables, cards on the table, and active situations.
- `verbs [available|all]`: List declared verbs and recipe opportunities (optionally filtered by runnable status).
- `commit <verb> [slot=card ...]`: Start an action with slot bindings (e.g., `commit sup morsel=bread#10`).
- `bind <sit> <slot> <card>`: Bind a table card into an open situation slot (e.g., `bind 1 fuel coal#4`).
- `advance [n]`: Advance the clock by `n` turns (default: 1 turn).
- `inspect <card>`: Inspect card details, title, lore, aspects, lifetimes, and relationship edges (e.g., `inspect bread#10` or `inspect coal`).
- `undo`: Revert the last player action, bind, or advance step via engine snapshot surgery.
- `mode [verbose|normal]`: Toggle detailed state display and action feedback.
- `filter [on|off]`: Toggle runnable recipe filtering.
- `trace <file.json>`: Export the sequence of player actions to a JSON trace file.
- `quit` (or `q`): Exit the session.

### Running Design Analyses

Analyze goal reachability or state spaces:
```bash
# Verify witness path to a goal condition
cargo run -p story-cli -- analyze docs/emberwatch.sw --reach "legacy:dawns_seen >= 1"

# Run design audit
cargo run -p story-cli -- analyze docs/emberwatch.sw --audit
```

---

## Cultist Simulator Exporter (`cs2sw`)

`cs2sw` converts Cultist Simulator JSON core content into Chorus² `.sw` scripts and extracts main-story narrative text into a companion catalog.

### Usage

```bash
cargo run -p cs2sw -- <cs-core-dir> <out.sw> [--report <report.md>]
```

Example:
```bash
cargo run -p cs2sw -- /path/to/CultistSimulator/data/core cultist-simulator.sw
```

### Outputs

1. `<out.sw>`: Complete mechanical conversion containing elements, verbs, recipes, decks, and endings.
2. `<stem>.narrative.json`: Companion narrative presentation catalog containing main-story element labels/descriptions, verb titles/descriptions, recipe labels, start/completion prose, and slot prompts.
3. Optional `--report <path>`: Markdown and JSON gap report classifying any constructs outside engine scope.

Play the converted game with full presentation:
```bash
cargo run -p story-cli -- play cultist-simulator.sw
```

---

## Visualizer (`godot/`)

Chorus² includes a 2D tabletop visualizer built in Godot 4, styled after classic tabletop narrative card games like *Cultist Simulator* and *Book of Hours*.

### Running the Visualizer

Launch the tabletop client (defaults to `docs/emberwatch.sw`):
```bash
godot --path godot
```

Or pass a specific story script:
```bash
godot --path godot -- docs/emberwatch.sw
```

### Controls & Interactions

- **Tabletop:** Drag cards between zone trays and situation sockets. Dragging slotted cards back to the felt tabletop unstages them.
- **Auto-Slotting & Unstaging:** Right-click any card on the tabletop to auto-stage it into the open situation window. Right-click any slotted card to return it to the tabletop.
- **Lore Inspector:** Left-click any card to view its category, lore, aspect breakdown, and decay status in the pinned index card.
- **Keyboard Shortcuts:**
  - `Space`: Advance 1 turn.
  - `Escape`: Close the active situation window or inspector card.
  - `Cmd+Z` / `Ctrl+Z`: Undo the last turn or action.
