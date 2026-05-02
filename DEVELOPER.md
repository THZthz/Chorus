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

---

## 2. Project Structure

```
src/
├── client/
│   ├── main.tsx              # React entry point
│   ├── App.tsx               # Main orchestrator: state machine, SSE consumer
│   └── index.css             # Global styles (Tailwind + noise filters)
├── components/
│   ├── CharacterPanel.tsx    # Sidebar: character stats, world entity browser
│   ├── DebugPanel.tsx        # Developer toolbox: LLM traces, console, editors
│   ├── DialogueMessage.tsx   # Message rendering (speaker types, object links)
│   ├── DialogueOptions.tsx   # Player choices (actions, skill checks)
│   ├── DiceRoller.tsx        # Skill check simulation (2D6 + stat)
│   ├── ObjectLink.tsx        # Hoverable entity references in text
│   ├── ObjectTooltip.tsx     # Entity lore popup
│   ├── TypingIndicator.tsx   # Animated dots during AI generation
│   └── debug/
│       ├── ConsoleViewer.tsx       # Intercepted browser console log viewer
│       ├── CopyButton.tsx          # One-click JSON copy utility
│       ├── DialogueTreeGraph.tsx   # Canvas node graph: recursive layout, pan/zoom, Jump to Replay
│       ├── HistoryEditor.tsx       # Visual message timeline with inline editing
│       ├── JsonExplorer.tsx        # Resizable, collapsible JSON tree viewer
│       ├── JsonNode.tsx            # Single JSON node renderer (string/number/object/array)
│       ├── LlmTraceViewer.tsx      # Parsed LLM exchange timeline with step breakdown
│       ├── WorldEditor.tsx         # Grouped entity editor with stat bars and opinion pills
│       └── shared.tsx              # Shared debug UI utilities
├── context/
│   └── CharacterContext.tsx  # Global character stats (React Context)
├── server/
│   ├── main.ts               # Express + Vite middleware entry
│   ├── api.ts                # REST API + SSE streaming endpoints
│   ├── db.ts                 # SQLite connection + schema (9 tables)
│   ├── llm/
│   │   ├── index.ts          # GameMaster: model init, system prompt, generateTurn()
│   │   ├── tools.ts          # All 7 LLM tool definitions (once)
│   │   ├── events.ts         # TurnEventEmitter: typed SSE dispatch
│   │   └── debug.ts          # LLM request/response/step logging
│   └── models/
│       ├── debug.ts          # llm_logs + console_logs CRUD
│       ├── dialogue.ts       # Dialogue tree CRUD (steps, branches, alternatives)
│       ├── history.ts        # Narrative message persistence
│       ├── plot.ts           # Plot tree CRUD + tree validation + buildActivePlotTree()
│       └── world.ts          # Entity CRUD + seed data + entity query helpers
├── shared/
│   └── events.ts             # SSE event type definitions (shared backend/frontend)
├── services/
│   ├── ConsoleLogger.ts      # Browser console.log interception
│   ├── SseClient.ts          # Browser SSE streaming consumer
│   └── WorldManager.ts       # Client-side world/plot cache; replay snapshot override; subscriber pattern
└── types/
    ├── dialogue.ts           # Message, DialogueOption, DialogueStep interfaces
    ├── entities.ts           # WorldEntity, Character, Location, WorldObject
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
┌────────────────────────────────────────────────┐
│  GameMaster.generateTurn()                     │
│                                                │
│  streamText({                                  │
│    tools: {                                    │
│      getAllEntitiesName,   ──► returns JSON    │
│      queryEntity,         ──► returns JSON     │
│      editEntity,          ──► DB + SSE event   │
│      createPlot,          ──► DB + SSE event   │
│      editPlot,            ──► DB + SSE event   │
│      getPlot,             ──► returns JSON     │
│      generateDialogueStep ──► SSE streaming    │
│    }                                           │
│  })                                            │
│                                                │
│  fullStream iteration:                         │
│    text-delta          → discard               │
│    tool-input-delta    → progressive           │
│    tool-call           → definitive            │
└──────────┬─────────────────────────────────────┘
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
│    world_update        → refresh     │
│    plot_update/create  → refresh     │
│    parsed              → final       │
│    done                → end turn    │
└──────────────────────────────────────┘
```

### 3.2 SSE Events

Defined in `src/shared/events.ts` (single source of truth for both backend and frontend):

| Event                | Direction       | Payload                           | Trigger                                   |
|----------------------|-----------------|-----------------------------------|-------------------------------------------|
| `step_start`         | Server → Client | `{ stepId }`                      | Turn begins                               |
| `streaming_messages` | Server → Client | `{ messages }`                    | Progressive during `generateDialogueStep` |
| `world_update`       | Server → Client | `{ entityId, changes }`           | `editEntity` tool executes                |
| `plot_update`        | Server → Client | `{ plotId, status }`              | `updatePlotStatus` tool executes          |
| `plot_create`        | Server → Client | `{ plotId, title, parentPlotId }` | `createPlot` tool executes                |
| `plot_edit`          | Server → Client | `{ plotId, changes }`             | `editPlot` tool executes                  |
| `options`            | Server → Client | `{ options }`                     | Options available mid-stream              |
| `parsed`             | Server → Client | `{ messages, options }`           | Final structured output                   |
| `error`              | Server → Client | `{ message }`                     | Error during generation                   |
| `done`               | Server → Client | `{}`                              | Turn complete                             |

### 3.3 LLM Tools

All 7 tools defined once in `src/server/llm/tools.ts`:

| Tool                   | Purpose                                                   | DB Operation              | SSE Event                       |
|------------------------|-----------------------------------------------------------|---------------------------|---------------------------------|
| `getAllEntitiesName`   | List entity IDs, names, types, shortDescriptions          | None (read query)         | None (returns JSON)             |
| `queryEntity`          | Get full entity by ID or text search                      | None (read query)         | None (returns JSON)             |
| `editEntity`           | Mutate a single entity's attributes/descriptions/opinions | `updateEntity()`          | `world_update`                  |
| `createPlot`           | Create a new plot node in the story tree                  | `addPlot()`               | `plot_create`                   |
| `editPlot`             | Update plot status, description, childPlots, etc.         | `updatePlot()`            | `plot_edit`                     |
| `getPlot`              | Retrieve plot(s) by ID or status filter                   | None (read query)         | None (returns JSON)             |
| `generateDialogueStep` | Produce narrative messages + player options               | None (data via streaming) | `streaming_messages` + `parsed` |

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

### 3.4 Key Design Decisions

1. **Tools defined once** — `src/server/llm/tools.ts` is the single source for all tool schemas/executors
2. **LLM text output silently discarded** — the system prompt instructs tool-only output; any text deltas are ignored
3. **No pre-generation** — turns are generated on-demand. Latency is acceptable for RPG pacing
4. **No static dialogue** — all narrative is AI-generated. No `sampleDialogue.ts`
5. **Shared event types** — `src/shared/events.ts` ensures backend/frontend event contracts match
6. **App.tsx state machine** — clean `idle → streaming → idle` cycle instead of 11 scattered booleans
7. **Plot-first story architecture** — plots form a tree (one root, branches via `childPlots`): the GM creates/edits the
   plot tree first, then generates dialogue options that align with the active plot's branch options
8. **Entity lazy loading** — world entities are described compactly in the system prompt (id + displayName +
   shortDescription); full details fetched via `queryEntity`

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
- **Start from any step**: The "Jump to Replay" button in `DialogueTreeGraph` calls `handleJumpToStep(stepId)` which
  fetches the tree, calls `buildHistoryFromTree` to reconstruct history with YOU messages, and sets `lastStepId` +
  `canRegenerate = true` so REGENERATE is immediately available.
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
- `POST /api/regenerate-all` — Bulk regenerate all leaf steps in the dialogue tree

### 4.2 Dialogue Tree

- `GET /api/dialogue/:id` — Step + children + alternatives
- `GET /api/dialogue/:id/children` — Child steps
- `GET /api/dialogue/:id/path` — Branch path from root
- `GET /api/dialogue/:id/alternatives` — Alternative versions
- `POST /api/dialogue/:id/alternatives/:altId/select` — Switch to alternative
- `POST /api/branches/activate` — Activate a branch (deactivates siblings)
- `GET /api/dialogue/tree` — Full dialogue tree (root, all active steps, leaf IDs, stats)
- `PATCH /api/dialogue/:id` — Update dialogue step (messages, options, skill checks)
- `POST /api/dialogue/traverse` — Navigate from step to child via option `{ stepId, optionId }`

### 4.3 State

- `GET /api/session/current` — Latest active leaf step (options + stepId) for page-reload resume
- `GET /api/world` — All entities
- `POST /api/world/entity` — Upsert entity
- `GET /api/plots` — All plots
- `GET /api/history` / `POST /api/history` — Dialogue history

### 4.4 Debug

- `GET /api/debug/logs` — LLM interaction logs
- `POST /api/debug/logs/clear` — Clear all LLM logs
- `GET /api/debug/console` — Browser console logs
- `POST /api/debug/console` — Upload a console log entry from the browser
- `POST /api/debug/console/clear` — Clear all console logs
- `POST /api/reset` — Wipe DB and re-seed

---

## 5. Database Schema

9 tables in SQLite (`game.db`, WAL mode):

| Table                   | Purpose                                                              |
|-------------------------|----------------------------------------------------------------------|
| `entities`              | World entities (characters, locations, objects) with JSON attributes |
| `history_messages`      | Persisted narrative message history                                  |
| `plots`                 | Quest/objective tree with JSON childPlots, entity links, status      |
| `dialogue_steps`        | Generated dialogue tree nodes                                        |
| `dialogue_alternatives` | Archived alternative versions (regeneration)                         |
| `llm_logs`              | LLM request/response logging                                         |
| `llm_steps`             | Per-step LLM metrics (tool calls, token usage, timings)              |
| `console_logs`          | Intercepted browser console logs                                     |
| `system_state`          | Key-value system state storage                                       |

### 5.1 Plot Tree Architecture

Plots form a single-rooted tree (`parentPlotId = null` for the root). Each plot holds an array of `childPlots` — branch
options that guide the GM when generating dialogue. The tree is validated on every `createPlot`/`editPlot` call.

**`PlotOption`** (branch slot)

**`Plot`** (stored in `plots` table, defined in `src/types/plot.ts`)

**Tree validation rules** (in `validatePlotTree()`):

- Exactly one root plot (`parentPlotId === null`)
- Every non-root plot references an existing parent
- Every non-null `childPlot.plotId` references an existing plot
- Validation failure returns the error string in the tool result; the GM can retry

**GM workflow** (explicitly guided by system prompt):

1. Read state: `getPlot()`, `getAllEntitiesName()`, `queryEntity()`
2. Structure story: `createPlot()` / `editPlot()` — update the plot tree *before* generating dialogue
3. Mutate world: `editEntity()` if descriptions or opinions changed
4. Generate: `generateDialogueStep` — options should map to active plot's `childPlots`

---

## 6. Core Systems

### 6.1 Internal Voices (Ego Skills)

Disco Elysium-style internal monologue. Voices: `LOGIC`, `RHETORIC`, `EMPATHY`, `PERCEPTION`, `VOLITION`, `ENDURANCE`,
`INLAND EMPIRE`, `SUGGESTION`, `HALF LIGHT`, `PHYSICAL INSTRUMENT`, `INTERFACING`, `ELECTROCHEMISTRY`.

These map to character stats in `src/types/entities.ts` and `src/context/CharacterContext.tsx`. The system prompt in
`src/server/llm/index.ts` instructs the LLM about voice personalities and includes a compact entity index (id +
displayName + shortDescription per entity) and the active plot tree (from `buildActivePlotTree()`). Full entity/plot
details are fetched via `queryEntity`/`getPlot` tools, not dumped into the prompt.

### 6.2 Skill Checks

- **White Checks**: Repeatable after stat increases
- **Red Checks**: High-stakes, one-time opportunities (`isRed` in `DialogueOption`)
- **Formula**: `2d6 + Stat >= Difficulty`
- **Client-side**: Dice rolling and probability calculation happen in `DiceRoller.tsx`
- **Narrative**: After a roll completes, the result is sent to the AI as user input for narrative integration

### 6.3 Debug Panel

The Debug Panel (`DebugPanel.tsx`) provides 5 tabs:

- **LLM Trace Viewer**: Parsed exchange timeline with step breakdown, resizable raw JSON viewers, and child trace
  nesting (`src/components/debug/LlmTraceViewer.tsx`)
- **Console Logs**: Intercepted browser console output with filtering (`src/components/debug/ConsoleViewer.tsx`)
- **History Editor**: Visual message timeline — type-styled cards (YOU/CHARACTER/INNER_VOICE/SYSTEM/ROLL/NOTIFICATION),
  expand-in-place overlay edit, drag reorder, add/delete messages (`src/components/debug/HistoryEditor.tsx`)
- **World Editor**: Visual entity editor — grouped sidebar by type (CHARACTER/LOCATION/OBJECT), inline-editable form
  with stat bars, opinion pills, attribute k/v table (`src/components/debug/WorldEditor.tsx`)
- **Dialogue Tree**: Canvas node graph — recursive tree layout, pan/zoom, SVG edges, node states (
  active/inactive/leaf/root/now), bottom inspector panel with message/option editing and "Jump to Replay" (
  `src/components/debug/DialogueTreeGraph.tsx`). Uses `PATCH /api/dialogue/:id` to save edits. Receives `currentStepId`
  prop and highlights the actively-replaying node with a green "NOW" badge.

---

## 7. Development Workflow

### 7.1 Adding a New Tool for the LLM

1. Define the tool in `src/server/llm/tools.ts` using the `tool()` function from `@ai-sdk`
2. Register it in the `tools` object inside `generateTurn()` in `src/server/llm/index.ts`
3. Update the system prompt if the LLM needs guidance on when to use it
4. Add SSE event emission in the tool's `execute` function for immediate UI feedback

### 7.2 Adding a New Voice/Skill

1. Add the stat to `CharacterStats` in `src/types/entities.ts`
2. Add default value in `src/context/CharacterContext.tsx`
3. Add voice personality description to the system prompt in `src/server/llm/index.ts`

### 7.3 Managing the World

Initial world state is seeded in `src/server/models/world.ts`. Modify the `initialObjects`, `initialLocations`, and
`initialCharacters` records there.
