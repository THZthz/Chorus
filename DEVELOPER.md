# Developer Documentation: Elysian Dialogue

Architecture, core systems, and data structures of the **Elysian Dialogue** application.

---

## 1. Project Overview

**Elysian Dialogue** is a cinematic RPG-style dialogue engine. Vertical-scrolling "thought stream" aesthetic, branching
dialogue paths, and probabilistic skill checks influenced by character attributes.

- **Stack:** React 19, TypeScript, Vite
- **Backend:** Express + SQLite (`better-sqlite3`)
- **AI:** Single-LLM Game Master (Gemini/DeepSeek via Vercel AI SDK v6)
- **Styling:** Tailwind CSS v4, `motion` (formly `framer-motion`), Lucide icons, CodeMirror (debug)
- **SSE:** Server-Sent Events for real-time streaming of LLM output and world mutations

---

## 2. Project Structure

```
src/
├── client/
│   ├── main.tsx              # React entry point
│   ├── App.tsx               # Main orchestrator: state machine, SSE consumer, replay mode
│   └── index.css             # Global styles (Tailwind + noise filters)
├── components/
│   ├── CharacterPanel.tsx    # Sidebar: character stats, world entity browser, quest tree
│   ├── DebugPanel.tsx        # Developer toolbox: 4 visible tabs + More dropdown (Logs, Console, World, Graphs, Prompt, Scene)
│   ├── DialogueMessage.tsx   # Message rendering (speaker types, object links, roll tooltips)
│   ├── DialogueOptions.tsx   # Player choices (actions, skill checks, unexplored branches)
│   ├── DiceRoller.tsx        # Skill check simulation (2D6 + stat) — modal with animations
│   ├── ObjectLink.tsx        # Hoverable entity references in text
│   ├── ObjectTooltip.tsx     # Entity lore popup (auto-positioning, expandable)
│   ├── TypingIndicator.tsx   # Animated dots during AI generation
│   └── debug/
│       ├── ConsoleViewer.tsx       # Intercepted browser console log viewer (throttled, filterable)
│       ├── CopyButton.tsx          # One-click JSON copy utility
│       ├── HistoryEditor.tsx       # Visual message timeline with inline editing (unused in tabs)
│       ├── JsonExplorer.tsx        # Resizable, collapsible JSON tree viewer
│       ├── JsonNode.tsx            # Single JSON node renderer (string/number/object/array)
│       ├── LlmTraceViewer.tsx      # Parsed LLM exchange timeline with step breakdown
│       ├── NodeGraph.tsx           # Generic canvas node graph: layout, pan/zoom, edge rendering
│       ├── NodeGraphConfigs.tsx    # Dialogue and plot tree configs, node cards, inspectors
│       ├── WorldEditor.tsx         # Grouped entity editor with stat bars and opinion pills
│       ├── SceneViewer.tsx          # Current scene viewer: location, characters, objects (live/replay)
│       ├── SystemPromptEditor.tsx  # Live markdown editor (CodeMirror + codemirror-rich-markdoc)
│       └── shared.tsx              # Shared debug UI utilities (CustomSelect, ResizableTextarea)
├── context/
│   └── CharacterContext.tsx  # Global character stats (React Context) with default fantasy-steampunk stats
├── server/
│   ├── main.ts               # Express + Vite middleware entry
│   ├── api.ts                # REST API + SSE streaming endpoints (world, plots, history, chat, debug)
│   ├── db.ts                 # SQLite connection + schema (9 tables + idempotent migrations)
│   ├── llm/
│   │   ├── index.ts          # GameMaster: model init, system prompt, generateTurn(), generateTurnBatch()
│   │   ├── tools.ts          # All 10 LLM tool definitions (schemas + executors)
│   │   ├── events.ts         # TurnEventEmitter: typed SSE dispatch for a single turn
│   │   └── debug.ts          # LlmDebugIntegration: request/response/step logging
│   └── models/
│       ├── debug.ts          # llm_logs + llm_steps + console_logs CRUD
│       ├── dialogue.ts       # Dialogue tree CRUD (steps, branches, alternatives, snapshots)
│       ├── history.ts        # Narrative message persistence (with metadata, skillCheck, rollResult)
│       ├── plot.ts           # Plot tree CRUD + tree validation + buildActivePlotTree()
│       ├── scene.ts           # Time + scene state CRUD (system_state keys)
│       └── world.ts          # Entity CRUD + seed data + entity query helpers
├── services/
│   ├── ConsoleLogger.ts      # Browser console.log interception (batched persistence, safe serialization)
│   ├── SseClient.ts          # Browser SSE streaming consumer with AbortController support
│   └── WorldManager.ts       # Client-side world/plot cache; replay snapshot override; subscriber pattern
├── shared/
│   └── events.ts             # SSE event type definitions (shared backend/frontend, typed event map)
└── types/
    ├── codemirror-rich-markdoc.d.ts  # Module declaration for untyped package
    ├── dialogue.ts           # Message, DialogueOption, DialogueStep interfaces
    ├── entities.ts           # WorldEntity, Character, Location, WorldObject, CharacterStats
    └── plot.ts               # Plot, PlotOption interfaces
```

---

## 3. Architecture: Event-Driven Tool Execution

The LLM is a pure tool-calling Game Master. Every meaningful output comes through tool calls. The backend streams tool
execution results to the frontend as typed SSE events.

### 3.1 Turn Lifecycle

```
POST /api/chat/stream
        │
        ▼
┌─────────────────────────────────────────────────┐
│  GameMaster.generateTurn()                      │
│                                                 │
│  streamText({                                   │
│    tools: {                                     │
│      getAllEntitiesName,   ──► returns JSON     │
│      queryEntity,         ──► returns JSON      │
│      editEntity,          ──► DB + SSE event    │
│      createPlot,          ──► DB + SSE event    │
│      editPlot,            ──► DB + SSE event    │
│      getPlot,             ──► returns JSON      │
│      getScene,            ──► returns JSON      │
│      updateScene,         ──► DB + SSE event    │
│      advanceTime,         ──► DB + SSE event    │
│      generateDialogueStep ──► SSE streaming     │
│    },                                           │
│    stopWhen: generates once + passes validation │
│    prepareStep: nudges if GM forgets dialogue   │
│  })                                             │
│                                                 │
│  fullStream iteration:                          │
│    text-delta          → discard                │
│    tool-input-delta    → progressive            │
│    tool-call           → definitive             │
└──────────┬──────────────────────────────────────┘
           │ SSE events
           ▼
┌──────────────────────────────────────┐
│  Frontend (App.tsx)                  │
│                                      │
│  State: idle → streaming → idle      │
│                                      │
│  Event handlers:                     │
│    step_start          → begin turn  │
│    streaming_messages  → progressive │
│    streaming_reset     → retry guard │
│    world_update        → refresh     │
│    plot_update/create  → refresh     │
│    time_update         → refresh     │
│    scene_update        → refresh     │
│    parsed              → final       │
│    done                → end turn    │
└──────────────────────────────────────┘
```

### 3.2 SSE Events

Defined in `src/shared/events.ts` (single source of truth for both backend and frontend):

| Event                | Direction       | Payload                           | Trigger                                    |
|----------------------|-----------------|-----------------------------------|--------------------------------------------|
| `step_start`         | Server → Client | `{ stepId }`                      | Turn begins                                |
| `streaming_messages` | Server → Client | `{ messages }`                    | Progressive during `generateDialogueStep`  |
| `streaming_reset`    | Server → Client | `{}`                              | LLM retried — previous streaming discarded |
| `world_update`       | Server → Client | `{ entityId, changes }`           | `editEntity` tool executes                 |
| `plot_update`        | Server → Client | `{ plotId, status }`              | `updatePlotStatus` tool executes           |
| `plot_create`        | Server → Client | `{ plotId, title, parentPlotId }` | `createPlot` tool executes                 |
| `plot_edit`          | Server → Client | `{ plotId, changes }`             | `editPlot` tool executes                   |
| `time_update`        | Server → Client | `{ day, segment, segmentsAdvanced }` | `advanceTime` tool executes       |
| `scene_update`       | Server → Client | `{ scene }`                        | `updateScene` tool executes        |
| `options`            | Server → Client | `{ options }`                     | Options available mid-stream               |
| `parsed`             | Server → Client | `{ messages, options }`           | Final structured output                    |
| `error`              | Server → Client | `{ message }`                     | Error during generation                    |
| `done`               | Server → Client | `{}`                              | Turn complete                              |

### 3.3 LLM Tools

All 10 tools defined once in `src/server/llm/tools.ts`:

| Tool                   | Purpose                                                   | DB Operation              | SSE Event                       |
|------------------------|-----------------------------------------------------------|---------------------------|---------------------------------|
| `getAllEntitiesName`   | List entity IDs, names, types, shortDescriptions          | None (read query)         | None (returns JSON)             |
| `queryEntity`          | Get full entity by ID or text search                      | None (read query)         | None (returns JSON)             |
| `editEntity`           | Mutate a single entity's attributes/descriptions/opinions | `updateEntity()`          | `world_update`                  |
| `createPlot`           | Create a new plot node in the story tree                  | `addPlot()`               | `plot_create`                   |
| `editPlot`             | Update plot status, description, childPlots, etc.         | `updatePlot()`            | `plot_edit`                     |
| `getPlot`              | Retrieve plot(s) by ID, bulk IDs, or status filter        | None (read query)         | None (returns JSON)             |
| `getScene`             | Get current game time and full scene state               | None (read query)         | None (returns JSON)             |
| `updateScene`          | Move characters/objects between locations                | `setSceneState()`         | `scene_update`                  |
| `advanceTime`          | Advance in-game clock by N segments (2 hrs each)         | `setGameTime()`           | `time_update`                   |
| `generateDialogueStep` | Produce narrative messages + player options               | None (data via streaming) | `streaming_messages` + `parsed` |

All tool `execute` functions are wrapped with `wrapSafe` (in `tools.ts`) which catches any thrown exceptions and returns
an
`ERROR:` string to the LLM instead of propagating the exception. This keeps the agentic loop alive — the GM sees the
error
and can retry with different input. The `fullStream` loop in `generateTurn` also handles the `error` chunk type (emitted
by
the SDK when a tool throws) and surfaces the actual error message to the frontend rather than a generic failure.

`editEntity`, `createPlot`, `editPlot`, and `getPlot` report failure conditions (entity not found, plot not found, tree
validation error) in their return messages so the GM can retry.

`createGenerateDialogueStepTool` returns `{ tool, wasValid }`. The `execute` function validates the GM's output before
accepting it:

- **`speaker === "INNER_VOICE"`**: Rejected — the speaker must be the specific skill name (`"LOGIC"`, `"HALF LIGHT"`,
  etc.), not the type string.
- **`option.check && option.hintBefore`**: Rejected — the skill check already renders the skill name via
  `skillCheckHint` in `DialogueOptions.tsx`; `hintBefore` is redundant.

On validation failure, `execute` returns a `VALIDATION FAILED` string to the GM and keeps `wasValid()` false, so the
`stopWhen` condition in `streamText` does not trigger and the agentic loop continues for a retry.

The `prepareStep` callback in `streamText` tracks whether `generateDialogueStep` was called in any prior step. If not,
it injects an error message into the message array to nudge the model. A hard limit of 10 steps (`stepCountIs(10)`) acts
as a circuit breaker.

### 3.4 Key Design Decisions

1. **Tools defined once** — `src/server/llm/tools.ts` is the single source for all tool schemas/executors
2. **LLM text output silently discarded** — the system prompt instructs tool-only output; any text deltas are ignored
3. **No pre-generation** — turns are generated on-demand. Latency is acceptable for RPG pacing
4. **No static dialogue** — all narrative is AI-generated. No `sampleDialogue.ts`
5. **Shared event types** — `src/shared/events.ts` ensures backend/frontend event contracts match
6. **App.tsx state machine** — clean `idle → streaming → idle` cycle instead of scattered booleans
7. **Plot-first story architecture** — plots form a tree (one root, branches via `childPlots`): the GM creates/edits the
   plot tree first, then generates dialogue options that align with the active plot's branch options
8. **Entity lazy loading** — world entities are described compactly in the system prompt (id + displayName +
   shortDescription); full details fetched via `queryEntity`
9. **World snapshots on steps** — each `dialogue_step` persists a `world_snapshot` (entities + plots + playerCharacter
   + gameTime + scene) so replay mode shows historical world state including time and scene composition
10. **Replay-safe plot editing** — during replay, plot edits go to the step's snapshot (local + DB via `PATCH snapshot`)
    not the live plot table

### 3.5 Dialogue Branching & Alternatives

- **Steps**: A single interaction "moment" stored in `dialogue_steps` table
- **Branches**: When a user selects an option, a new child step is created. The parent option's `nextStepId` is updated
  to link forward to the child, creating a doubly-linked tree (parent → child via `parent_step_id`, child ← parent via
  `nextStepId` on the option).
- **Alternatives**: When a user clicks "Regenerate", the current step is archived as an alternative, and a new one is
  generated. The UI allows "swiping" between versions

### 3.6 Dialogue Replay

Replay mode allows navigating the existing dialogue tree and expanding it with new branches.

- **Enter replay**: Click the Git Branch button (visible after starting a game). Fetches the full tree from
  `GET /api/dialogue/tree`, loads the root step, and applies its `worldSnapshot` to `worldManager` so CharacterPanel
  shows historical state.
- **Navigation**: Clicking a previously-explored option (one with `nextStepId`) navigates to its child step, injecting a
  YOU message then revealing child messages one-by-one (120ms stagger via `revealMessagesStaggered`). In the `onDone`
  callback the child's `worldSnapshot` is applied to worldManager. Fast path uses local `treeSteps`; slow path falls
  back to `POST /api/dialogue/traverse`. Both paths set `lastStepId` + `canRegenerate = true` so REGENERATE is
  available. Options are hidden during reveal; `isRevealingRef` blocks rapid re-selection.
- **New branches**: Options without a child step are styled with a dashed border and a `GitBranch` icon (see
  `DialogueOptions.tsx`). Clicking one triggers LLM generation (`POST /api/chat/stream`) using history reconstructed
  from `buildHistoryFromTree()`. On completion, the new step is fetched via `GET /api/dialogue/:id` and added to
  `treeSteps`; the parent option's `nextStepId` is updated in local state.
- **Regenerate in replay**: Works for any navigated or newly-generated step since `lastStepId` is set on every
  navigation. YOU messages are injected in replay navigation, so `trimmedHistory` in `handleRegenerate` correctly
  captures the last player choice.
- **Start from any step**: The "Jump to Replay" button in `NodeGraph (dialogue)` calls `handleJumpToStep(stepId)` which
  fetches the tree, calls `buildHistoryFromTree` to reconstruct history with YOU messages, and sets `lastStepId` +
  `canRegenerate = true` so REGENERATE is immediately available.
- **Plot tree sync**: During replay, the plot `NodeGraph` reads plots from `worldManager`'s replay snapshot (not the
  live
  DB),
  so the plot tree reflects the state at the current dialogue step. Editing a plot in the inspector during replay
  updates
  the step's `world_snapshot.plots` in the DB via `PATCH /api/dialogue/:id/snapshot` and the local replay override
  immediately.
- **Exit replay**: Click the Return button. Calls `worldManager.clearReplayState()` (immediate visual restore to cached
  live entities/plots), then `worldManager.loadState()` (refreshes from DB), then fetches history from
  `history_messages`.
- **`buildHistoryFromTree(stepId, treeSteps)`**: Pure function (top of `App.tsx`) that walks the parent chain from root
  to the given step and injects YOU messages between steps using each child's `parentOptionId` to find the option text.

---

## 4. API Endpoints

### 4.1 Chat

- `POST /api/chat/stream` — Primary AI turn (SSE streaming)
- `POST /api/regenerate` — Archive current step as alternative, generate new response

### 4.2 Dialogue Tree

- `GET /api/dialogue/:id` — Step + children + alternatives
- `GET /api/dialogue/:id/children` — Child steps
- `GET /api/dialogue/:id/path` — Branch path from root
- `GET /api/dialogue/:id/alternatives` — Alternative versions
- `POST /api/dialogue/:id/alternatives/:altId/select` — Switch to alternative
- `POST /api/branches/activate` — Activate a branch (deactivates siblings)
- `GET /api/dialogue/tree` — Full dialogue tree (root, all steps, leaf IDs, stats)
- `PATCH /api/dialogue/:id` — Update dialogue step (messages, options, skill checks)
- `PATCH /api/dialogue/:id/snapshot` — Update a step's `worldSnapshot` (replay plot editing)
- `POST /api/dialogue/traverse` — Navigate from step to child via option `{ stepId, optionId }`

### 4.3 State

- `GET /api/session/current` — Latest active leaf step (options + stepId) for page-reload resume
- `GET /api/world` — All entities (grouped by type: characters, locations, objects)
- `POST /api/world/entity` — Upsert entity
- `GET /api/plots` — All plots
- `PATCH /api/plots/:id` — Update a plot's fields (with tree validation)
- `GET /api/scene` — Current game time and scene state
- `GET /api/history` / `POST /api/history` — Dialogue history (GET reads; POST replaces all)

### 4.4 Debug

- `GET /api/debug/logs` — LLM interaction logs (with nested steps)
- `POST /api/debug/logs/clear` — Clear all LLM logs
- `GET /api/debug/console` — Persisted browser console logs
- `POST /api/debug/console` — Upload console log entries (single or batch array)
- `POST /api/debug/console/clear` — Clear all console logs
- `POST /api/reset` — Wipe DB (entities, plots, dialogue_steps, alternatives, history) and re-seed

---

## 5. Database Schema

9 tables in SQLite (`game.db`, WAL mode):

| Table                   | Purpose                                                                                  |
|-------------------------|------------------------------------------------------------------------------------------|
| `entities`              | World entities (characters, locations, objects) with JSON attributes                     |
| `history_messages`      | Persisted narrative message history (with metadata, skillCheck, rollResult JSON columns) |
| `plots`                 | Quest/objective tree with JSON childPlots, entity links, status                          |
| `dialogue_steps`        | Generated dialogue tree nodes (with world_snapshot JSON for replays)                     |
| `dialogue_alternatives` | Archived alternative versions (regeneration)                                             |
| `llm_logs`              | LLM request/response logging (with parent_id + label for child traces)                   |
| `llm_steps`             | Per-step LLM metrics (tool calls, token usage, timings, user_prompt, reasoning)          |
| `console_logs`          | Intercepted browser console logs                                                         |
| `system_state`          | Key-value system state storage                                                           |

### 5.1 Plot Tree Architecture

Plots form a single-rooted tree (`parentPlotId = null` for the root). Each plot holds an array of `childPlots` — branch
options that guide the GM when generating dialogue. The tree is validated on every `createPlot`/`editPlot` call.

**`PlotOption`** (branch slot in `src/types/plot.ts`)

**`Plot`** (stored in `plots` table, defined in `src/types/plot.ts`)

**Tree validation rules** (in `validatePlotTree()` in `src/server/models/plot.ts`):

- Exactly one root plot (`parentPlotId === null`)
- Every non-root plot references an existing parent
- Every non-null `childPlot.plotId` references an existing plot
- Validation failure returns the error string in the tool result; the GM can retry

**`buildActivePlotTree()`**: Formats the plot tree as a text representation included in the system prompt. Shows
status tags, involved entities, and the childPlots options tree.

**GM workflow** (explicitly guided by system prompt in `src/server/llm/index.ts`):

1. Read state: `getPlot()`, `getAllEntitiesName()`, `queryEntity()`
2. Structure story: `createPlot()` / `editPlot()` — update the plot tree *before* generating dialogue
3. Mutate world: `editEntity()` if descriptions or opinions changed
4. Generate: `generateDialogueStep` — options should map to active plot's `childPlots`

---

## 6. Core Systems

### 6.1 Internal Voices (Inner Skills)

Fantasy-steampunk inner monologue — each skill is a distinct voice in the player's mind. Voices: `LOGIC`, `RHETORIC`,
`EMPATHY`, `PERCEPTION`, `VOLITION`, `ENDURANCE`, `SORCERY`, `SUGGESTION`, `INSTINCT`, `MIGHT`, `CLOCKWORK`, `ALCHEMY`.

These map to character stats in `src/types/entities.ts` (`CharacterStats` interface) and the default player in
`src/context/CharacterContext.tsx`. The system prompt in `src/server/llm/index.ts` instructs the LLM about voice
personalities and includes a compact entity index (id + displayName + shortDescription per entity) and the active plot
tree (from `buildActivePlotTree()`). Full entity/plot details are fetched via `queryEntity`/`getPlot` tools, not dumped
into the prompt.

The system prompt is runtime-configurable via the Debug Panel's **Prompt** tab. The template is stored in the
`system_state` table (key `gm_system_prompt`) and supports `{{entities_brief}}` and `{{active_plots}}` variables
that are replaced with live data by `buildSystemPrompt()`. If no custom template is stored, the
`DEFAULT_SYSTEM_PROMPT_TEMPLATE` constant is used.

### 6.2 Skill Checks

- **White Checks**: Repeatable after stat increases
- **Red Checks**: High-stakes, one-time opportunities (`isRed` in `DialogueOption`)
- **Formula**: `2d6 + Stat >= Difficulty`
- **Probability display**: Arc SVG + percentage before rolling; color-coded thresholds (≥75% green, ≥50% yellow, etc.)
- **Client-side**: Dice rolling and probability calculation happen in `DiceRoller.tsx`
- **Narrative**: After a roll completes, the result is sent to the AI as user input for narrative integration
- **Special outcomes**: Natural 2 (critical failure) and Natural 12 (critical success) have distinct visual treatment
- **Conditional outcomes**: The `conditions` array on a check can define custom success/failure labels via JS expression
  evaluation

### 6.3 Debug Panel

The Debug Panel (`DebugPanel.tsx`) provides 6 draggable-reorderable tabs in a single tab bar:

- **LLM Trace Viewer** (`"logs"`): Parsed exchange timeline with per-step prompt display, model reasoning text (when
  available), step breakdown, resizable raw JSON viewers, auto-refresh, and child trace nesting
  (`src/components/debug/LlmTraceViewer.tsx`)
- **Console Logs** (`"console"`): Intercepted browser console output with filtering (by level, keyword/regex, date range),
  text wrap toggle, and sync/clear (`src/components/debug/ConsoleViewer.tsx`)
- **World Editor** (`"world"`): Visual entity editor — grouped sidebar by type (CHARACTER/LOCATION/OBJECT),
  inline-editable form with stat bars, opinion pills, attribute k/v table, and add-new-entity
  (`src/components/debug/WorldEditor.tsx`)
- **Graphs** (`"graphs"`): Merged Dialogue Tree + Plot Tree node graphs with internal mode toggle. Dialogue mode —
  recursive tree layout, pan/zoom, SVG edges, node states (active/inactive/leaf/root/now), bottom inspector panel
  with message/option editing and "Jump to Replay". Plot mode — canvas node graph for plot inspection and editing,
  reads from `worldManager`'s replay snapshot when replay is active. Both use
  `src/components/debug/NodeGraph.tsx` with their respective configs from `NodeGraphConfigs.tsx`.
- **System Prompt** (`"prompt"`): Obsidian-style live markdown editor for the GM system prompt template. Uses
  `@uiw/react-codemirror` + `codemirror-rich-markdoc`. GFM tables rendered as interactive widgets.
  Supports `{{entities_brief}}` and `{{active_plots}}` template variables.
  (`src/components/debug/SystemPromptEditor.tsx`)
- **Scene Viewer** (`"scene"`): Current scene state — game time, current location, characters present, object
  positions. Aligns with replay mode: fetches `GET /api/scene` live or reads from `worldManager` during replay.
  (`src/components/debug/SceneViewer.tsx`)

Tabs can be reordered by dragging (HTML5 native drag-and-drop with GripVertical handle on hover).

---

## 7. Time System

Each in-game day is divided into 12 segments of 2 hours each (segment 0 = midnight–2am, segment 11 = 10pm–midnight).
Time only advances when the GM calls the `advanceTime` tool — the player cannot directly control time.

**Storage**: `game_time_day` and `game_time_segment` keys in `system_state` table. Defaults to day 1, segment 0.

**`GameTime`** (in `src/types/entities.ts`): `{ day: number, segment: number }`

**Model functions** (in `src/server/models/scene.ts`):
- `getGameTime()` / `setGameTime(time)` — read/write time from system_state
- `advanceGameTime(segments)` — adds segments (wraps days at 12), returns old and new times
- `describeTime(time)` — human-readable string: "Day 3, Dawn (~4am-6am)"
- `SEGMENT_LABELS` — constant map: `{ 0: "Midnight", 1: "Late Night", 2: "Dawn", … }`

**Time in snapshots**: `WorldSnapshot.gameTime` stores the time at each dialogue step for replay.

---

## 8. Scene Management

The scene system tracks "who is where, with what" — character positions, object positions, and the current location.
Objects can be at a location or carried by a character.

**Storage**: `current_scene` key in `system_state` table (JSON). Default scene: `rusted_cog` with `orin_fell` present.

**`SceneState`** (in `src/types/entities.ts`):
```
{
  currentLocationId: string,
  characterLocations: Record<string, string>,   // characterId → locationId
  objectPositions: Record<string, ObjectPosition> // objectId → { type, locationId/characterId }
}
```

**Model functions** (in `src/server/models/scene.ts`):
- `getSceneState()` / `setSceneState(scene)` — read/write parsed scene JSON
- `buildSceneSummary(scene)` (in `llm/index.ts`) — resolves entity IDs to display names for the system prompt

**Scene in snapshots**: `WorldSnapshot.scene` stores the full scene state at each dialogue step for replay.

**API**: `GET /api/scene` returns `{ gameTime, scene }` for live-mode frontend display.

---

## 9. Development Workflow

### 7.1 Adding a New Tool for the LLM

1. Define the tool in `src/server/llm/tools.ts` using the `tool()` function from `@ai-sdk`
2. Register it in the `tools` object inside `generateTurn()` in `src/server/llm/index.ts`
3. Update the system prompt if the LLM needs guidance on when to use it
4. Add SSE event emission in the tool's `execute` function for immediate UI feedback

### 7.2 Adding a New Voice/Skill

1. Add the stat to `CharacterStats` in `src/types/entities.ts`
2. Add default value in `src/context/CharacterContext.tsx`
3. Add voice personality description to the system prompt in `src/server/llm/index.ts`
4. Add a color entry in `DialogueMessage.tsx`'s `VOICE_COLORS` map

### 7.3 Managing the World

Initial world state is seeded in `src/server/models/world.ts`. Modify the `initialObjects`, `initialLocations`, and
`initialCharacters` records there. The root plot is also seeded with two childPlots branch options.

### 7.4 License Headers

All source files in `src/` require an AGPL v3 license header at the top of the file. Run `npm run add-license-header` to
add headers to any new files that are missing them — it skips files that already have a header.

### 7.5 Debug Panel Tab Layout

Debug tabs are defined in `DebugPanel.tsx` with a `TAB_DEFS` map and `DEFAULT_TAB_ORDER` array. All 6 tabs are
rendered in a single bar and can be reordered by dragging (HTML5 native drag-and-drop). To add a new tab,
extend the `TabId` type, add an entry to `TAB_DEFS`, add it to `DEFAULT_TAB_ORDER`, and add the corresponding
content render branch.
