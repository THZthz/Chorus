# Developer Documentation: Elysian Dialogue

Architecture, core systems, and data structures of the **Elysian Dialogue** application.

---

## 1. Project Overview

**Elysian Dialogue** is a cinematic RPG-style dialogue engine. Vertical-scrolling "thought stream" aesthetic, branching dialogue paths, and probabilistic skill checks influenced by character attributes.

- **Stack:** React 19, TypeScript, Vite
- **Backend:** Express + SQLite (`better-sqlite3`)
- **AI:** Single-LLM Game Master (Gemini/DeepSeek via Vercel AI SDK v6)
- **Styling:** Tailwind CSS v4, `motion` (formerly `framer-motion`), Lucide icons, CodeMirror (debug)
- **SSE:** Server-Sent Events for real-time streaming of LLM output and world mutations
- **Deployment:** Local-only — runs on localhost, no authentication required by design

---

## 2. Project Structure

```
src/
├── client/
│   ├── main.tsx              # React entry point
│   ├── App.tsx               # Main orchestrator: state machine, SSE consumer, replay mode
│   ├── historyUtils.ts       # buildHistoryFromTree(): walk step tree to reconstruct message history
│   ├── hooks/
│   │   ├── useDialogueStreaming.ts  # SSE streaming state + event callbacks + handleRegenerate
│   │   ├── useReplayMode.ts         # Replay mode enter/exit, option navigation, staggered reveal
│   │   └── useSkillChecks.ts        # Skill check rolling (2D6+stat) with delay, result message creation
│   ├── idPool.ts             # Client-side ID batch fetcher (pre-allocates unique IDs)
│   └── index.css             # Global styles (Tailwind + noise filters + scrollbar + Markdoc editor CSS)
├── components/
│   ├── CharacterPanel.tsx    # Sidebar: character stats, world entity browser, quest tree
│   ├── DebugPanel.tsx        # Developer toolbox: 6 draggable-reorderable tabs (Logs, World, Graphs, Prompt, Scene, Facts)
│   ├── DialogueMessage.tsx   # Message rendering (speaker types, object links, roll tooltips, colors from shared/colors)
│   ├── DialogueOptions.tsx   # Player choices (actions, skill checks, unexplored branches, custom input)
│   ├── DiceRoller.tsx        # D6 die face renderer (dot-pattern, sizes xs–lg)
│   ├── ObjectLink.tsx        # Hoverable entity references in text (triggers ObjectTooltip)
│   ├── ObjectTooltip.tsx     # Entity info popup (auto-positioning, expandable, opinions, attributes)
│   ├── TypingIndicator.tsx   # Animated bouncing dots during AI generation (motion/react)
│   └── debug/
│       ├── CopyButton.tsx            # One-click JSON copy utility with "Copied" feedback
│       ├── FactsViewer.tsx           # Facts table with entity/plot filter, show-removed toggle, replay-aware
│       ├── HistoryEditor.tsx         # Visual message timeline with inline editing, drag-and-drop reorder
│       ├── JsonExplorer.tsx          # Resizable, collapsible JSON tree viewer (parses JSON → JsonNode)
│       ├── JsonNode.tsx              # Recursive JSON node renderer (collapsible, color-coded types)
│       ├── LlmTraceViewer.tsx        # Parsed LLM exchange timeline with step breakdown, auto-refresh
│       ├── NodeGraph.tsx             # Generic canvas node graph: tree layout, pan/zoom, SVG edges, inspector
│       ├── DialogueConfig.tsx        # createDialogueConfig factory: dialogue tree cards + inspector
│       ├── PlotConfig.tsx            # createPlotConfig factory: plot tree cards + replay-aware inspector
│       ├── ToolCallCard.tsx          # Reusable tool-call card (input preview, error, result) for LLM traces
│       ├── WorldEditor.tsx           # Grouped entity editor with stat bars, opinion pills, attribute table
│       ├── SceneViewer.tsx           # Current scene viewer: location, characters, objects (live/replay)
│       ├── SystemPromptEditor.tsx    # Live markdown editor (CodeMirror + codemirror-rich-markdoc)
│       └── shared.tsx                # Shared debug UI utilities (CustomSelect, ResizableTextarea)
├── console/
│   ├── main.ts               # Standalone Node.js REPL client for testing dialogue flows
│   └── SseClient.ts          # Lightweight SSE consumer for the console (core dialogue events only)
├── context/
│   └── CharacterContext.tsx  # Global character stats (React Context) with default fantasy-steampunk stats
├── server/
│   ├── api.ts                # REST API + SSE streaming endpoints (25+ routes, Zod-validated)
│   ├── db.ts                 # SQLite connection + schema (9 tables + idempotent migrations)
│   ├── validation.ts         # Zod schemas for all API endpoints (chatStream, upsertEntity, plots, dialogue, etc.)
│   ├── llm/
│   │   ├── index.ts          # Barrel: generateTurn(), generateTurnBatch(), generatePlotDefs(), buildSystemPrompt()
│   │   ├── model.ts          # getModel(): lazy-init provider model (Gemini → DeepSeek fallback)
│   │   ├── prompt.ts         # System prompt template (getter/setter/default), buildSystemPrompt(), buildSceneSummary()
│   │   ├── events.ts         # TurnEventEmitter + NoopEventEmitter: typed SSE dispatch for a single turn
│   │   ├── debug.ts          # LlmDebugIntegration: request/response/step logging
│   │   ├── persistStep.ts    # persistStep(): save step + world snapshot, link parent option, deactivate siblings
│   │   ├── pregeneratePlotTree.ts  # generatePlotDefs(): LLM-generated coherent plot tree with validation
│   │   ├── toolsFactory.ts   # createAllTools(): assemble all 18 LLM tools with event emitter
│   │   └── tools/
│   │       ├── index.ts                  # Barrel re-export of all tool factories + shared helpers
│   │       ├── shared.ts                 # Helpers: checkText (character filter), wrapSafe (error catching), mapToDialogueOption
│   │       ├── listEntities.ts           # createListEntitiesTool — list all entities (optional type filter)
│   │       ├── getEntity.ts              # createGetEntityTool — by ID, bulk IDs, or text search
│   │       ├── updateEntity.ts           # createUpdateEntityTool — mutate single entity attributes
│   │       ├── updateEntities.ts         # createUpdateEntitiesTool — bulk-update multiple entities
│   │       ├── createEntity.ts           # createCreateEntityTool — create new character/location/object
│   │       ├── getCharacterState.ts      # createGetCharacterStateTool — stats, conditions, inventory, location
│   │       ├── updateCharacterState.ts   # createUpdateCharacterStateTool — stats, conditions, inventory
│   │       ├── createPlot.ts             # createCreatePlotTool — create plot node, auto-link parent
│   │       ├── updatePlot.ts             # createUpdatePlotTool — update plot fields with tree validation
│   │       ├── getPlot.ts                # createGetPlotTool — by ID, bulk IDs, or status filter
│   │       ├── generateDialogueStep.ts   # createGenerateDialogueStepTool — produce messages + options with validation
│   │       ├── advanceTime.ts            # createAdvanceTimeTool — advance clock by segments/days
│   │       ├── updateScene.ts            # createUpdateSceneTool — move characters/objects between locations
│   │       ├── getScene.ts               # createGetSceneTool — get game time + full scene state
│   │       ├── addFact.ts                # createAddFactTool — record GM fact (key-value with entity/plot links)
│   │       ├── getFact.ts                # createGetFactTool — retrieve facts by ID, bulk, or filter
│   │       ├── updateFact.ts             # createUpdateFactTool — update existing fact key/value/links
│   │       └── removeFact.ts             # createRemoveFactTool — soft-delete a fact
│   ├── main.ts                          # Express + Vite middleware entry (port 3000)
│   ├── seed-stories/
│   │   ├── index.ts                     # Story registry + ACTIVE_SEED_STORY constant
│   │   ├── types.ts                     # SeedStory, SeedPlot interfaces
│   │   ├── romantic-magic-awakening.ts  # Default seed story (romantic urban-fantasy, Karavelle)
│   │   ├── celestial-athenaeum.ts       # Cosmic horror seed story (The Sleeper Beneath)
│   │   └── iron-serpent-murder.ts       # Noir murder mystery seed story (train mystery)
│   └── models/
│       ├── debug.ts          # LLM interaction log query and management (getLlmLogs, clearLlmLogs, addLlmLog)
│       ├── dialogue.ts       # Dialogue tree CRUD (steps, branches, alternatives, snapshots, tree traversal)
│       ├── facts.ts          # Facts CRUD (addFact, getFacts, updateFact, removeFact, getFactsSnapshot)
│       ├── history.ts        # Narrative message persistence (with metadata, skillCheck, rollResult)
│       ├── ids.ts            # Base62-encoded 4-char unique ID generation (nextId, nextIdBatch)
│       ├── plot.ts           # Plot tree CRUD + tree validation + buildActivePlotTree()
│       ├── scene.ts          # Time + scene state CRUD (system_state keys)
│       ├── shared.ts         # safeJsonParse utility for JSON column deserialization
│       └── world.ts          # Entity CRUD via active seed story + entity query helpers + seeding
├── services/
│   ├── SseClient.ts          # Browser SSE streaming consumer (15 event types, AbortController support)
│   └── WorldManager.ts       # Client-side world/plot/fact cache; replay snapshot override; subscriber pattern
├── shared/
│   ├── colors.ts             # VOICE_COLORS: 12 inner-voice → hex color map
│   ├── constants.ts          # TOOL_NAMES, SKILL_NAMES, PLAYER_ID, SEGMENT_LABELS, SEGMENT_HOURS
│   ├── events.ts             # SSE event type definitions (15 event interfaces, typed event map)
│   └── sse.ts                # Shared SSE stream parser (async generator, used by browser + console clients)
└── types/
    ├── codemirror-rich-markdoc.d.ts  # Module declaration for untyped package
    ├── dialogue.ts                   # Message, DialogueOption, DialogueStep interfaces
    ├── entities.ts                   # WorldEntity, Character, Location, WorldObject, CharacterStats, Fact, GameTime, SceneState, WorldSnapshot
    └── plot.ts                       # Plot, PlotOption, PlotPatch interfaces
```

---

## 3. Architecture: Event-Driven Tool Execution

The LLM is a pure tool-calling Game Master. Every meaningful output comes through tool calls. The backend streams tool execution results to the frontend as typed SSE events. The `parseSseStream` async generator in `src/shared/sse.ts` provides the shared SSE parser used by both the browser and console SSE clients.

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
│      listEntities,        ──► returns JSON      │
│      getEntity,           ──► returns JSON      │
│      updateEntity,        ──► DB + SSE event    │
│      createPlot,          ──► DB + SSE event    │
│      updatePlot,          ──► DB + SSE event    │
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

| Event                | Direction       | Payload                              | Trigger                                      |
|----------------------|-----------------|--------------------------------------|----------------------------------------------|
| `step_start`         | Server → Client | `{ stepId }`                         | Turn begins                                  |
| `streaming_messages` | Server → Client | `{ messages }`                       | Progressive during `generateDialogueStep`    |
| `streaming_reset`    | Server → Client | `{}`                                 | LLM retried — previous streaming discarded   |
| `world_update`       | Server → Client | `{ entityId, changes }`              | `updateEntity` tool executes                 |
| `plot_update`        | Server → Client | `{ plotId, status }`                 | Reserved (defined but not currently emitted) |
| `plot_create`        | Server → Client | `{ plotId, title, parentPlotId }`    | `createPlot` tool executes                   |
| `plot_edit`          | Server → Client | `{ plotId, changes }`                | `updatePlot` tool executes                   |
| `time_update`        | Server → Client | `{ day, segment, segmentsAdvanced }` | `advanceTime` tool executes                  |
| `scene_update`       | Server → Client | `{ scene }`                          | `updateScene` tool executes                  |
| `entity_create`      | Server → Client | `{ entityId, entityType, displayName }` | `createEntity` tool executes             |
| `fact_add`           | Server → Client | `{ fact }`                           | `addFact` tool executes                      |
| `fact_update`        | Server → Client | `{ factId, changes }`                | `updateFact` tool executes                   |
| `fact_remove`        | Server → Client | `{ factId }`                         | `removeFact` tool executes                   |
| `options`            | Server → Client | `{ options }`                        | Options available mid-stream                 |
| `parsed`             | Server → Client | `{ messages, options }`              | Final structured output                      |
| `error`              | Server → Client | `{ message }`                        | Error during generation                      |
| `done`               | Server → Client | `{}`                                 | Turn complete                                |

### 3.3 LLM Tools

All 18 tools, each defined in its own file under `src/server/llm/tools/` with a barrel re-export at `tools/index.ts`:

| Tool                   | Purpose                                                   | DB Operation              | SSE Event                        |
|------------------------|-----------------------------------------------------------|---------------------------|----------------------------------|
| `listEntities`         | List entity IDs, names, types, shortDescriptions          | None (read query)         | None (returns JSON)              |
| `getEntity`            | Get full entity by ID or text search                      | None (read query)         | None (returns JSON)              |
| `updateEntity`         | Mutate a single entity's attributes/descriptions/opinions | `updateEntity()`          | `world_update`                   |
| `updateEntities`       | Bulk-update multiple entities at once                     | `updateEntity()`          | `world_update`                   |
| `createEntity`         | Create a new world entity (character/location/object)     | `upsertEntity()` + scene  | `entity_create` + `scene_update` |
| `getCharacterState`    | Get character stats, conditions, carried objects, location| None (read query)         | None (returns JSON)              |
| `updateCharacterState` | Update character stats, conditions, or inventory          | `updateEntity()` + scene  | `world_update` + `scene_update`  |
| `createPlot`           | Create a new plot node in the story tree                  | `addPlot()`               | `plot_create`                    |
| `updatePlot`           | Update plot status, description, childPlots, flags, etc.  | `updatePlot()`            | `plot_edit`                      |
| `getPlot`              | Retrieve plot(s) by ID, bulk IDs, or status filter        | None (read query)         | None (returns JSON)              |
| `getScene`             | Get current game time and full scene state                | None (read query)         | None (returns JSON)              |
| `updateScene`          | Move characters/objects between locations                 | `setSceneState()`         | `scene_update`                   |
| `advanceTime`          | Advance in-game clock by N segments (2 hrs each)          | `advanceGameTime()`       | `time_update`                    |
| `generateDialogueStep` | Produce narrative messages + player options               | None (data via streaming) | `streaming_messages` + `parsed`  |
| `addFact`              | Record a new GM fact (key-value with entity/plot links)   | `addFact()`               | `fact_add`                       |
| `getFact`              | Retrieve facts by ID, bulk IDs, or entity/plot filter     | None (read query)         | None (returns JSON)              |
| `updateFact`           | Update an existing fact's key/value/links                 | `updateFact()`            | `fact_update`                    |
| `removeFact`           | Soft-delete a fact by ID (sets `is_valid = 0`)            | `removeFact()`            | `fact_remove`                    |

All tool `execute` functions are wrapped with `wrapSafe` (in `tools/shared.ts`) which catches any thrown exceptions and returns an `ERROR:` string to the LLM instead of propagating the exception. This keeps the agentic loop alive — the GM sees the error and can retry with different input. The `fullStream` loop in `generateTurn` also handles the `error` chunk type (emitted by the SDK when a tool throws) and surfaces the actual error message to the frontend rather than a generic failure.

`updateEntity`, `createPlot`, `updatePlot`, and `getPlot` report failure conditions (entity not found, plot not found, tree validation error) in their return messages so the GM can retry.

`createGenerateDialogueStepTool` returns `{ tool, wasValid }`. The `execute` function validates the GM's output before accepting it:
- **`speaker === "INNER_VOICE"`**: Rejected — the speaker must be the specific skill name (`"LOGIC"`, `"HALF LIGHT"`, etc.), not the type string.
- **`option.check && option.hintBefore`**: Rejected — the skill check already renders the skill name via `skillCheckHint` in `DialogueOptions.tsx`; `hintBefore` is redundant.

On validation failure, `execute` returns a `VALIDATION FAILED` string to the GM and keeps `wasValid()` false, so the `stopWhen` condition in `streamText` does not trigger and the agentic loop continues for a retry.

The `prepareStep` callback in `streamText` tracks whether `generateDialogueStep` was called in any prior step. If not, it injects an error message into the message array to nudge the model. A hard limit of 10 steps (`stepCountIs(10)`) acts as a circuit breaker.

### 3.4 Key Design Decisions

1. **Tools defined once** — `src/server/llm/tools/` is the single source for all tool schemas/executors; each factory lives in its own file
2. **LLM text output silently discarded** — the system prompt instructs tool-only output; any text deltas are ignored
3. **Plot tree pre-generation** — a complete plot tree can be pre-generated in advance via `POST /api/plots/pregen` with a configurable size (2–50 nodes). The LLM generates a coherent tree structure in one shot, which is bulk-inserted into the plots table. This is accessible from the Debug Panel's Graphs → Plots tab.
4. **No static dialogue** — all narrative is AI-generated. No `sampleDialogue.ts`
5. **Shared event types** — `src/shared/events.ts` ensures backend/frontend event contracts match
6. **App.tsx state machine** — clean `idle → streaming → idle` cycle instead of scattered booleans
7. **Plot-first story architecture** — plots form a tree (one root, branches via `childPlots`): the GM creates/edits the plot tree first, then generates dialogue options that align with the active plot's branch options
8. **Entity lazy loading** — world entities are described compactly in the system prompt (id + displayName + shortDescription); full details fetched via `getEntity`
9. **World snapshots on steps** — each `dialogue_step` persists a `world_snapshot` (entities + plots + playerCharacter + gameTime + scene + facts) via `persistStep()` in `src/server/llm/persistStep.ts` so replay mode shows historical world state including time, scene, and facts
10. **Replay-safe plot editing** — during replay, plot edits go to the step's snapshot (local + DB via `PATCH snapshot`) not the live plot table
11. **Client-side ID pre-allocation** — `idPool.ts` fetches batches of unique IDs from `GET /api/ids/batch` so the frontend can assign IDs to new messages/snapshots without waiting for a server round-trip
12. **Shared SSE parser** — `src/shared/sse.ts` provides `parseSseStream`, a single async generator used by both the browser `SseClient` and the console `ConsoleSseClient`, avoiding duplication

### 3.5 Dialogue Branching & Alternatives

- **Steps**: A single interaction "moment" stored in `dialogue_steps` table
- **Branches**: When a user selects an option, a new child step is created. The parent option's `nextStepId` is updated to link forward to the child, creating a doubly-linked tree (parent → child via `parent_step_id`, child ← parent via `nextStepId` on the option).
- **Custom input**: When the player types their own dialogue instead of selecting a generated option, `persistStep()` in `src/server/llm/persistStep.ts` creates a synthetic `custom_*` option on the parent step via `addOptionToStep()`. This ensures custom-input branches are navigable in replay mode. Custom options are styled with italic text and a "custom" badge in `DialogueOptions.tsx`.
- **Alternatives**: When a user clicks "Regenerate", the current step is archived as an alternative, and a new one is generated. The UI allows "swiping" between versions

### 3.6 Dialogue Replay

Replay mode allows navigating the existing dialogue tree and expanding it with new branches.

- **Enter replay**: Click the Git Branch button (visible after starting a game). Fetches the full tree from `GET /api/dialogue/tree`, loads the root step, and applies its `worldSnapshot` to `worldManager` so CharacterPanel shows historical state.
- **Navigation**: Clicking a previously-explored option (one with `nextStepId`) navigates to its child step, injecting a YOU message then revealing child messages one-by-one (120ms stagger via `revealMessagesStaggered`). In the `onDone` callback the child's `worldSnapshot` is applied to worldManager. Fast path uses local `treeSteps`; slow path falls back to `POST /api/dialogue/traverse`. Both paths set `lastStepId` + `canRegenerate = true` so REGENERATE is available. Options are hidden during reveal; `isRevealingRef` blocks rapid re-selection.
- **New branches**: Options without a child step are styled with a dashed border and a `GitBranch` icon (see `DialogueOptions.tsx`). Clicking one triggers LLM generation (`POST /api/chat/stream`) using history reconstructed from `buildHistoryFromTree()`. On completion, the new step is fetched via `GET /api/dialogue/:id` and added to `treeSteps`; the parent option's `nextStepId` is updated in local state.
- **Regenerate in replay**: Works for any navigated or newly-generated step since `lastStepId` is set on every navigation. YOU messages are injected in replay navigation, so `trimmedHistory` in `handleRegenerate` correctly captures the last player choice.
- **Start from any step**: The "Jump to Replay" button in `NodeGraph (dialogue)` calls `handleJumpToStep(stepId)` which fetches the tree, calls `buildHistoryFromTree` to reconstruct history with YOU messages, and sets `lastStepId` + `canRegenerate = true` so REGENERATE is immediately available.
- **Plot tree sync**: During replay, the plot `NodeGraph` reads plots from `worldManager`'s replay snapshot (not the live DB), so the plot tree reflects the state at the current dialogue step. Editing a plot in the inspector during replay updates the step's `world_snapshot.plots` in the DB via `PATCH /api/dialogue/:id/snapshot` and the local replay override immediately.
- **Exit replay**: Click the Return button. Calls `worldManager.clearReplayState()` (immediate visual restore to cached live entities/plots), then `worldManager.loadState()` (refreshes from DB), then fetches history from `history_messages`.
- **`buildHistoryFromTree(stepId, treeSteps)`**: Pure function (top of `App.tsx`) that walks the parent chain from root to the given step and injects YOU messages between steps using each child's `parentOptionId` to find the option text.

### 3.7 Console Client

A standalone Node.js REPL client (`src/console/main.ts`) that validates the SSE protocol is client-agnostic. It implements the full dialogue loop — begin story, select options, regenerate, and resume — through the same SSE endpoints as the browser frontend.

- **State machine**: `IDLE → WAITING → AWAITING_OPTION → WAITING → ...` (simpler than the browser's, no replay mode)
- **Rendering**: Terminal output via `chalk` (speaker colors mirrored from `src/shared/colors.ts`'s `VOICE_COLORS`) and `log-update` (progressive streaming updates). NPC speaker colors are derived from a hash of the character name.
- **SSE handling**: `ConsoleSseClient` (`src/console/SseClient.ts`) handles only core dialogue events (`step_start`, `streaming_messages`, `streaming_reset`, `options`, `parsed`, `error`, `done`). World/plot/time/scene events are intentionally ignored — the console has no entity editor or debug panel.
- **Session resume**: On startup, fetches `GET /api/history` + `GET /api/session/current` to restore the last dialogue state if an active session exists.
- **Regenerate**: Trims history to the last YOU message and calls `POST /api/regenerate`, identical flow to the browser frontend.

---

## 4. API Endpoints

### 4.1 Chat

- `POST /api/chat/stream` — Primary AI turn (SSE streaming)
- `POST /api/regenerate` — Archive current step as alternative, generate new response

### 4.2 Dialogue Tree

- `GET /api/dialogue/tree` — Full dialogue tree (root, all steps, leaf IDs, stats) — registered first to avoid `:id` route shadowing
- `GET /api/dialogue/:id` — Step + children + alternatives
- `GET /api/dialogue/:id/children` — Child steps
- `GET /api/dialogue/:id/path` — Branch path from root
- `GET /api/dialogue/:id/alternatives` — Alternative versions
- `POST /api/dialogue/:id/alternatives/:altId/select` — Switch to alternative
- `POST /api/branches/activate` — Activate a branch (deactivates siblings)
- `PATCH /api/dialogue/:id` — Update dialogue step (messages, options, skill checks)
- `PATCH /api/dialogue/:id/snapshot` — Update a step's `worldSnapshot` (replay plot editing)
- `POST /api/dialogue/traverse` — Navigate from step to child via option `{ stepId, optionId }`

### 4.3 State

- `GET /api/session/current` — Latest active leaf step (options + stepId) for page-reload resume
- `GET /api/world` — All entities (grouped by type: characters, locations, objects)
- `POST /api/world/entity` — Upsert entity
- `GET /api/facts` — All facts (with optional query filters: `relatedEntityId`, `relatedPlotId`, `relatedScene`, `relatedTime`, `includeInvalid`)
- `GET /api/plots` — All plots
- `PATCH /api/plots/:id` — Update a plot's fields (with tree validation)
- `POST /api/plots/pregen` — Pre-generate a complete plot tree `{ size: number (2-50, default 10) }`. Clears existing plots, calls LLM to generate a coherent tree, bulk-inserts all plots, returns `{ plots: Plot[] }`.
- `GET /api/scene` — Current game time and scene state
- `GET /api/history` / `POST /api/history` — Dialogue history (GET reads; POST replaces all)

### 4.4 Debug

- `GET /api/debug/logs` — LLM interaction logs (with nested steps)
- `POST /api/debug/logs/clear` — Clear all LLM logs
- `POST /api/reset` — Wipe DB (entities, plots, dialogue_steps, alternatives, history) and re-seed

### 4.5 ID Generation

- `GET /api/ids/batch` — Generate a batch of unique base62-encoded IDs

### 4.6 System Prompt

- `GET /api/debug/system-prompt` — Get current system prompt template
- `PUT /api/debug/system-prompt` — Update system prompt template
- `GET /api/debug/system-prompt/default` — Get default system prompt template
- `POST /api/debug/system-prompt/reset` — Reset system prompt to default

---

## 5. Database Schema

9 tables in SQLite (`game.db`, WAL mode):

| Table                   | Purpose                                                                                  |
|-------------------------|------------------------------------------------------------------------------------------|
| `entities`              | World entities (characters, locations, objects) with JSON attributes/stats/opinions      |
| `history_messages`      | Persisted narrative message history (with metadata, skillCheck, rollResult JSON columns) |
| `plots`                 | Quest/objective tree with JSON childPlots, entity links, status, flags                   |
| `dialogue_steps`        | Generated dialogue tree nodes (with world_snapshot JSON for replays)                     |
| `dialogue_alternatives` | Archived alternative versions (regeneration)                                             |
| `llm_logs`              | LLM request/response logging (with parent_id + label for child traces)                   |
| `llm_steps`             | Per-step LLM metrics (tool calls, token usage, timings, user_prompt, reasoning)          |
| `facts`                 | GM working memory: key-value facts with entity/plot/scene/time links and validity flag   |
| `system_state`          | Key-value system state storage (time, scene, counters, system prompt template)           |

### 5.1 Plot Tree Architecture

Plots form a single-rooted tree (`parentPlotId = null` for the root). Each plot holds an array of `childPlots` — branch options that guide the GM when generating dialogue. The tree is validated on every `createPlot`/`updatePlot` call.

**`PlotOption`** (branch slot in `src/types/plot.ts`): `{ plotId: string | null, triggerCondition: string }` — links a child plot node with its trigger condition text.

**`Plot`** (stored in `plots` table, defined in `src/types/plot.ts`): `{ id, title, description, status, involvedLocations, involvedCharacters, parentPlotId, parentOptionId, childPlots }` — `status` is one of `PENDING | IN_PROGRESS | RESOLVED`.

**Tree validation rules** (in `validatePlotTree()` in `src/server/models/plot.ts`):

- Exactly one root plot (`parentPlotId === null`)
- Every non-root plot references an existing parent
- Every non-null `childPlot.plotId` references an existing plot
- Validation failure returns the error string in the tool result; the GM can retry

**`buildActivePlotTree()`**: Formats the plot tree as a text representation included in the system prompt. Shows status tags, involved entities, and the childPlots options tree.

**GM workflow** (explicitly guided by system prompt in `src/server/llm/prompt.ts`):

1. Read state: `getPlot()`, `listEntities()`, `getEntity()`
2. Structure story: `createPlot()` / `updatePlot()` — update the plot tree _before_ generating dialogue
3. Mutate world: `updateEntity()` if descriptions or opinions changed
4. Generate: `generateDialogueStep` — options should map to active plot's `childPlots`

---

## 6. Core Systems

### 6.1 Internal Voices (Inner Skills)

Fantasy-steampunk inner monologue — each skill is a distinct voice in the player's mind. Voices: `LOGIC`, `RHETORIC`, `EMPATHY`, `PERCEPTION`, `VOLITION`, `ENDURANCE`, `SORCERY`, `SUGGESTION`, `INSTINCT`, `MIGHT`, `CLOCKWORK`, `ALCHEMY`.

These map to character stats in `src/types/entities.ts` (`CharacterStats` interface) and the default player in `src/context/CharacterContext.tsx`. The system prompt in `src/server/llm/prompt.ts` instructs the LLM about voice personalities and includes a compact entity index (id + displayName + shortDescription per entity) and the active plot tree (from `buildActivePlotTree()`). Full entity/plot details are fetched via `getEntity`/`getPlot` tools, not dumped into the prompt.

The system prompt is runtime-configurable via the Debug Panel's **Prompt** tab. The template is stored in the `system_state` table (key `gm_system_prompt`) and supports `{{setting_description}}`, `{{tone_description}}`, `{{entities_brief}}`, `{{active_plots}}`, `{{game_time}}`, and `{{current_scene}}` variables that are replaced with live data by `buildSystemPrompt()`. Setting and tone come from the active seed story. If no custom template is stored, the `DEFAULT_SYSTEM_PROMPT_TEMPLATE` constant is used.

### 6.2 Skill Checks

- **White Checks**: Repeatable after stat increases
- **Red Checks**: High-stakes, one-time opportunities (`isRed` in `DialogueOption`)
- **Formula**: `2d6 + Stat >= Difficulty`
- **Probability display**: Arc SVG + percentage before rolling; color-coded thresholds (≥75% green, ≥50% yellow, etc.)
- **Client-side**: Dice rolling and probability calculation happen in `DiceRoller.tsx`
- **Narrative**: After a roll completes, the result is sent to the AI as user input for narrative integration
- **Special outcomes**: Natural 2 (critical failure) and Natural 12 (critical success) have distinct visual treatment
- **Conditional outcomes**: The `conditions` array on a check can define custom success/failure labels via JS expression evaluation

### 6.3 Debug Panel

The Debug Panel (`DebugPanel.tsx`) provides 6 draggable-reorderable tabs in a single tab bar:

- **LLM Trace Viewer** (`"logs"`): Parsed exchange timeline with per-step prompt display, model reasoning text (when available), step breakdown, resizable raw JSON viewers, auto-refresh, and child trace nesting (`src/components/debug/LlmTraceViewer.tsx`)
- **World Editor** (`"world"`): Visual entity editor — grouped sidebar by type (CHARACTER/LOCATION/OBJECT), inline-editable form with stat bars, opinion pills, attribute k/v table, and add-new-entity (`src/components/debug/WorldEditor.tsx`)
- **Graphs** (`"graphs"`): Merged Dialogue Tree + Plot Tree node graphs with internal mode toggle. Dialogue mode — recursive tree layout, pan/zoom, SVG edges, node states (active/inactive/leaf/root/now), bottom inspector panel with message/option editing and "Jump to Replay". Plot mode — canvas node graph for plot inspection and editing, reads from `worldManager`'s replay snapshot when replay is active. Includes a **Pre-generate** button with a size selector (5–30 nodes) that calls the LLM to generate a complete plot tree in advance, then displays it in the graph. Both use `src/components/debug/NodeGraph.tsx` with config factories from `DialogConfig.tsx` and `PlotConfig.tsx`.
- **System Prompt** (`"prompt"`): Obsidian-style live markdown editor for the GM system prompt template. Uses `@uiw/react-codemirror` + `codemirror-rich-markdoc`. GFM tables rendered as interactive widgets. Supports `{{entities_brief}}` and `{{active_plots}}` template variables. (`src/components/debug/SystemPromptEditor.tsx`)
- **Scene Viewer** (`"scene"`): Current scene state — game time, current location, characters present, object positions. Aligns with replay mode: fetches `GET /api/scene` live or reads from `worldManager` during replay. (`src/components/debug/SceneViewer.tsx`)
- **Facts Viewer** (`"facts"`): Table of world facts with filter-by-entity-ID and filter-by-plot-ID inputs, show-removed toggle. Uses `worldManager` for live/replay switching. (`src/components/debug/FactsViewer.tsx`)

Tabs can be reordered by dragging (HTML5 native drag-and-drop with GripVertical handle on hover).

### 6.4 Facts System

The Facts system provides the GM with working memory — a persistent key-value store for narrative continuity across turns.

**`Fact`** (in `src/types/entities.ts`): `{ id, key, value, relatedEntityIds[], relatedPlotIds[], relatedScene, relatedTime, is_valid }`

**Model functions** (in `src/server/models/facts.ts`):

- `addFact(input)` — Creates a new fact with auto-ID (`fact_{nextId()}`)
- `getFactById(id)` — Single fact lookup
- `getFacts(filter?)` — Filter by relatedEntityId (LIKE), relatedPlotId, relatedScene, relatedTime; optional `includeInvalid` flag
- `getFactsByIds(ids)` — Bulk lookup (valid only)
- `updateFact(id, changes)` — Partial update of key/value/links
- `removeFact(id)` — Soft-delete (sets `is_valid = 0`)
- `getFactsSnapshot()` — Returns all valid facts for step snapshot persistence

**LLM tools** (in `src/server/llm/tools/`): `addFact`, `getFact`, `updateFact`, `removeFact` — allow the GM to record, query, update, and remove facts as the narrative unfolds.

**SSE events**: `fact_add`, `fact_update`, `fact_remove` — streamed to the frontend for live UI updates. The `WorldManager` subscriber pattern propagates these to `FactsViewer` and other consumers.

**In snapshots**: `WorldSnapshot.facts` stores the full facts state at each dialogue step for replay. During replay, `FactsViewer` reads from `worldManager`'s replay snapshot rather than the live API.

**Debug**: The Debug Panel's **Facts tab** (`FactsViewer.tsx`) provides a filterable table of all facts with entity/plot filter inputs and a show-removed toggle.

### 6.5 Seed Story System

Seed data (entities, locations, characters, root plot, initial time, initial scene) is organized into pluggable seed story modules under `src/server/seed-stories/`. Each module exports a `SeedStory` object conforming to the interface in `types.ts`.

**Available seed stories** (registered in `index.ts`):
| Story ID | File | Genre |
|---|---|---|
| `romantic-magic-awakening` | `romantic-magic-awakening.ts` | Romantic fantasy (default, active) |
| `celestial-athenaeum` | `celestial-athenaeum.ts` | Cosmic horror |
| `iron-serpent-murder` | `iron-serpent-murder.ts` | Murder mystery |

The active story is determined by the `ACTIVE_SEED_STORY` constant in `index.ts`. `getActiveSeedStory()` returns the active story's data, and `seedDatabase()` in `world.ts` reads from it to populate the database on first run.

**To add a new seed story:**
1. Create a new file in `src/server/seed-stories/` exporting a `SeedStory` object
2. Register it in the `STORIES` map in `index.ts`
3. Change `ACTIVE_SEED_STORY` to the new story ID

---

## 7. Time System

Each in-game day is divided into 12 segments of 2 hours each (segment 0 = midnight–2am, segment 11 = 10pm–midnight). Time only advances when the GM calls the `advanceTime` tool — the player cannot directly control time.

**Storage**: `game_time_day` and `game_time_segment` keys in `system_state` table. Defaults to day 1, segment 2 (dawn).

**`GameTime`** (in `src/types/entities.ts`): `{ day: number, segment: number }`

**Model functions** (in `src/server/models/scene.ts`):

- `getGameTime()` / `setGameTime(time)` — read/write time from system_state
- `advanceGameTime(segments)` — adds segments (wraps days at 12), returns old and new times
- `describeTime(time)` — human-readable string: "Day 3, Dawn (~4am-6am)"
- `SEGMENT_LABELS` — constant map: `{ 0: "Midnight", 1: "Late Night", 2: "Dawn", … }`

**Time in snapshots**: `WorldSnapshot.gameTime` stores the time at each dialogue step for replay.

---

## 8. Scene Management

The scene system tracks "who is where, with what" — character positions, object positions, and the current location. Objects can be at a location or carried by a character.

**Storage**: `current_scene` key in `system_state` table (JSON). Default scene: `the_velvet_thorn` with `veyla` and `madam_cressida` present, `soul_shard` and `veyllas_ribbon` carried by the player.

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
- `buildSceneSummary(scene)` (in `llm/prompt.ts`) — resolves entity IDs to display names for the system prompt

**Scene in snapshots**: `WorldSnapshot.scene` stores the full scene state at each dialogue step for replay.

**API**: `GET /api/scene` returns `{ gameTime, scene }` for live-mode frontend display.

---

## 9. Development Workflow

### 9.1 Adding a New Tool for the LLM

1. Create a new file in `src/server/llm/tools/` (camelCase, matching the export name) using the `tool()` function from `@ai-sdk`. Follow the existing pattern: extract the Zod schema as a module-level `const inputSchema`, use `z.infer<typeof inputSchema>` for execute args, and import helpers from `@/server/llm/tools/shared`.
2. Export it from `src/server/llm/tools/index.ts` and register it in `createAllTools()` in `src/server/llm/toolsFactory.ts`
3. Update the system prompt if the LLM needs guidance on when to use it
4. Add SSE event emission in the tool's `execute` function for immediate UI feedback

### 9.2 Adding a New Voice/Skill

1. Add the stat to `CharacterStats` in `src/types/entities.ts`
2. Add default value in `src/context/CharacterContext.tsx`
3. Add voice personality description to the system prompt in `src/server/llm/prompt.ts`
4. Add a color entry in `src/shared/colors.ts`'s `VOICE_COLORS` map

### 9.3 Managing the World

Initial world state is defined by the active seed story in `src/server/seed-stories/`. Each seed story module exports its own `objects`, `locations`, `characters`, `rootPlot`, `initialTime`, and `initialScene`. To change or add seed data, edit the active story's module (set by `ACTIVE_SEED_STORY` in `index.ts`). See section 6.5 for the full seed story system.

### 9.4 License Headers

All source files in `src/` require an AGPL v3 license header at the top of the file. Run `npm run add-license-header` to add headers to any new files that are missing them — it skips files that already have a header.

### 9.5 Debug Panel Tab Layout

Debug tabs are defined in `DebugPanel.tsx` with a `TAB_DEFS` map and `DEFAULT_TAB_ORDER` array. All 6 tabs are rendered in a single bar and can be reordered by dragging (HTML5 native drag-and-drop with GripVertical handle on hover). To add a new tab, extend the `TabId` type, add an entry to `TAB_DEFS`, add it to `DEFAULT_TAB_ORDER`, and add the corresponding content render branch.
