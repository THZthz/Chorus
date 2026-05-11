# Developer Documentation: Elysian Dialogue

Architecture, core systems, and data structures of the **Elysian Dialogue** application.

---

## 1. Project Overview

**Elysian Dialogue** is a cinematic RPG-style dialogue engine with a vertical-scrolling "thought stream" aesthetic, branching dialogue paths, and probabilistic skill checks influenced by character attributes.

- **Stack:** TypeScript, Node.js
- **Backend:** Express + SQLite (`better-sqlite3`) + Neo4j (via local `src/server/memory/` module)
- **AI:** Single-LLM Game Master (Gemini/DeepSeek via Vercel AI SDK v6)
- **SSE:** Server-Sent Events for real-time streaming of LLM output
- **Console client:** Standalone Node.js REPL with chalk rendering
- **Deployment:** Local-only — runs on localhost, no authentication required by design

---

## 2. Project Structure

```
.
├── Makefile                   # Unified dev commands (Neo4j, server, console, lint)
├── docker-compose.test.yml    # Neo4j test container
├── package.json               # TypeScript project config
├── pyproject.toml             # Python project config (agent-memory, dev dependency)
├── tsconfig.json
└── src/
    ├── console/
    │   ├── main.ts            # Standalone Node.js REPL client for dialogue interaction
    │   └── SseClient.ts       # Lightweight SSE consumer for the console
    ├── server/
    │   ├── main.ts            # Express entry (port 3000), MemoryClient init, seed on startup
    │   ├── api.ts             # REST API + SSE streaming endpoints
    │   ├── db.ts              # SQLite connection + schema
    │   ├── validation.ts      # Zod schemas for API endpoints
    │   ├── llm/
    │   │   ├── index.ts       # generateTurn(): full-stream SSE turn loop
    │   │   ├── model.ts       # getModel(): lazy-init provider model (Gemini → DeepSeek)
    │   │   ├── prompt.ts      # System prompt template + buildSystemPrompt()
    │   │   ├── events.ts      # TurnEventEmitter: typed SSE dispatch
    │   │   ├── debug.ts       # LlmDebugIntegration: request/response/step logging
    │   │   └── tools/
    │   │       ├── advanceTime.ts           # Advance in-game clock by segments/days
    │   │       ├── generateDialogueStep.ts  # Produce messages + options with validation
    │   │       └── shared.ts               # Helpers: checkText (character filter)
    │   ├── memory/
    │   │   ├── client.ts      # MemoryClient singleton — wires all memory layers
    │   │   ├── types.ts       # Shared types (Entity, Message, Fact, Preference, etc.)
    │   │   ├── neo4j.ts       # Neo4jClient — thin wrapper over neo4j-driver
    │   │   ├── schema.ts      # Index/constraint/vector index creation
    │   │   ├── embedder.ts    # Local embeddings (Xenova/ONNX) + OpenAI-compatible fallback
    │   │   ├── short-term.ts  # Conversations & messages with sequential linking
    │   │   ├── long-term.ts   # Entities (POLE+O), preferences, facts, relationships
    │   │   ├── reasoning.ts   # Reasoning traces, steps, tool calls
    │   │   ├── observer.ts    # Observational memory — token-threshold compression
    │   │   ├── search.ts      # Hybrid vector + graph search across memory types
    │   │   ├── context.ts     # Assembled context for GM consumption
    │   │   └── tools.ts       # 16 AI SDK tool definitions
    │   ├── mcp/
    │   │   ├── seed.ts        # Seed Neo4j via MemoryClient
    │   │   └── reset.ts       # Clear Neo4j via MemoryClient
    │   ├── models/
    │   │   ├── time.ts        # Game time CRUD (read/write/advance from system_state)
    │   │   ├── ids.ts         # Base62-encoded 4-char unique ID generation
    │   │   ├── debug.ts       # LLM interaction log query and management
    │   │   └── shared.ts      # safeJsonParse utility
    │   └── seed-stories/
    │       ├── index.ts       # Story registry + ACTIVE_SEED_STORY constant
    │       ├── types.ts       # SeedStory, SeedPlot interfaces
    │       └── magic-awakening.ts  # Default seed story
    ├── shared/
    │   ├── events.ts          # SSE event type definitions (typed event map)
    │   ├── sse.ts             # Shared SSE stream parser (async generator)
    │   ├── colors.ts          # VOICE_COLORS: 12 inner-voice → hex color map
    │   └── constants.ts       # SKILL_NAMES, TOOL_NAMES, SEGMENT_LABELS, etc.
    └── types/
        ├── dialogue.ts        # Message, DialogueOption, NotificationType
        └── entities.ts        # CharacterStats, Character, GameTime
```

---

## 3. Architecture: Memory-Backed Tool Execution

The LLM is a pure tool-calling Game Master. World state lives in Neo4j via the local memory module (`src/server/memory/`), while SQLite holds only logs and time state. The backend streams tool execution results to the console as typed SSE events.

### 3.1 Turn Lifecycle

```
POST /api/chat/stream
        │
        ▼
┌──────────────────────────────────────────────────────┐
│  generateTurn()                                      │
│                                                      │
│  streamText({                                        │
│    tools: {                                          │
│      ← 16 Neo4j-backed tools from createMemoryTools()│
│      generateDialogueStep  ──► SSE streaming         │
│      advanceTime           ──► DB + SSE event        │
│    },                                                │
│    stopWhen: generates once + passes validation      │
│    prepareStep: nudges if GM forgets dialogue        │
│  })                                                  │
│                                                      │
│  fullStream iteration:                               │
│    text-delta          → discard                     │
│    tool-input-delta    → progressive streaming       │
│    tool-call           → definitive output           │
└──────────┬───────────────────────────────────────────┘
           │ SSE events
           ▼
┌──────────────────────────────────────┐
│  Console Client (console/main.ts)    │
│                                      │
│  State: idle → streaming → idle      │
│                                      │
│  Event handlers:                     │
│    step_start          → begin turn  │
│    streaming_messages  → progressive │
│    streaming_reset     → retry guard │
│    time_update         → refresh     │
│    options             → mid-stream  │
│    parsed              → final       │
│    error               → display     │
│    done                → end turn    │
└──────────────────────────────────────┘
```

### 3.2 SSE Events

Defined in `src/shared/events.ts` (single source of truth):

| Event                | Direction       | Payload                              | Trigger                                   |
|----------------------|-----------------|--------------------------------------|-------------------------------------------|
| `step_start`         | Server → Client | `{ stepId }`                         | Turn begins                               |
| `streaming_messages` | Server → Client | `{ messages }`                       | Progressive during `generateDialogueStep` |
| `streaming_reset`    | Server → Client | `{}`                                 | LLM retried — discard previous            |
| `time_update`        | Server → Client | `{ day, segment, segmentsAdvanced }` | `advanceTime` tool executes               |
| `options`            | Server → Client | `{ options }`                        | Options available mid-stream              |
| `parsed`             | Server → Client | `{ messages, options }`              | Final structured output                   |
| `error`              | Server → Client | `{ message }`                        | Error during generation                   |
| `done`               | Server → Client | `{}`                                 | Turn complete                             |

### 3.3 LLM Tools

Two layers of tools:

**Elysian tools** (defined in `src/server/llm/`):

| Tool                   | Purpose                                     | SSE Event                                 |
|------------------------|---------------------------------------------|-------------------------------------------|
| `generateDialogueStep` | Produce narrative messages + player options | `streaming_messages`, `options`, `parsed` |
| `advanceTime`          | Advance in-game clock by N segments         | `time_update`                             |

**Neo4j-backed tools** (16 tools defined in `src/server/memory/tools.ts`):

| Tool              | Purpose                                                             |
|-------------------|---------------------------------------------------------------------|
| `searchMemory`    | Hybrid vector + graph search across all memory                      |
| `getContext`      | Auto-assembled context for the current session                      |
| `storeMessage`    | Store a message in conversation history                             |
| `saveEntity`      | Create/update an entity (PERSON/OBJECT/LOCATION/ORGANIZATION/EVENT) |
| `setPreference`   | Record a user preference                                            |
| `recordFact`      | Store a subject-predicate-object fact triple                        |
| `getEntity`       | Get entity details with graph traversal                             |
| `getConversation` | Get full conversation history for a session                         |
| `listSessions`    | List available conversation sessions                                |
| `linkEntities`    | Create a typed relationship between entities                        |
| `startTrace`      | Begin recording a reasoning trace                                   |
| `recordStep`      | Record a reasoning step within a trace                              |
| `completeTrace`   | Complete a reasoning trace with outcome                             |
| `getObservations` | Get session observations and reflections                            |
| `exportGraph`     | Export subgraph as JSON for visualization                           |
| `queryGraph`      | Execute read-only Cypher queries                                    |

All 16 tools are defined as AI SDK tools in `src/server/memory/tools.ts` and registered in `generateTurn()`. The GM has full access to entity CRUD, observations, relationships, conversation history, and semantic search — all backed by Neo4j.

### 3.4 Seed System

On startup, `main.ts` seeds Neo4j with initial world data from the active seed story. The `seedDatabase()` function in `src/server/mcp/seed.ts` uses `MemoryClient.longTerm` to create entity nodes with proper POLE+O labels (`:Entity:Person:Character`, `:Entity:Location`, `:Entity:Object`, `:Entity:Event`), typed relationships (`LOCATED_AT`, `CARRIES`, `ALLIED_WITH`, `CHILD_PLOT`), and initial game time in SQLite.

The `/api/reset` endpoint clears Neo4j (via `client.neo4j.executeWrite("MATCH (n) DETACH DELETE n")` in `src/server/mcp/reset.ts`) then re-seeds.

Seed stories live in `src/server/seed-stories/`. The active story is set via `ACTIVE_SEED_STORY` in `index.ts`.

### 3.5 Key Design Decisions

1. **World state in Neo4j** — entities, observations, and relationships stored in Neo4j via local memory module; SQLite holds only logs and time
2. **Tools statically defined** — all 18 tools (2 Elysian + 16 Neo4j-backed) registered in `generateTurn()`; no dynamic discovery
3. **LLM text output silently discarded** — the system prompt instructs tool-only output; text deltas are ignored
4. **No static dialogue** — all narrative is AI-generated
5. **Shared event types** — `src/shared/events.ts` ensures backend/console event contracts match
6. **Lightweight console client** — standalone REPL with chalk rendering; no entity editor or debug panel
7. **SSE progressive streaming** — `generateDialogueStep` streams messages/options incrementally via `partial-json` parsing

---

## 4. API Endpoints

### 4.1 Chat

- `POST /api/chat/stream` — Primary AI turn (SSE streaming)

### 4.2 State

- `GET /api/history` — Returns empty array (placeholder; GM uses `getConversation`)
- `GET /api/session/current` — Returns null (no dialogue tree persistence)

### 4.3 ID Generation

- `GET /api/ids/batch?count=N` — Generate a batch of unique base62-encoded IDs

### 4.4 Debug

- `GET /api/debug/logs` — LLM interaction logs
- `POST /api/debug/logs/clear` — Clear all LLM logs

### 4.5 System Prompt

- `GET /api/debug/system-prompt` — Get current system prompt template
- `PUT /api/debug/system-prompt` — Update system prompt template
- `GET /api/debug/system-prompt/default` — Get default system prompt template
- `POST /api/debug/system-prompt/reset` — Reset system prompt to default

### 4.6 Reset

- `POST /api/reset` — Clear SQLite logs, clear Neo4j, re-seed

---

## 5. Database Schema

3 tables in SQLite (`game.db`, WAL mode):

| Table          | Purpose                                                            |
|----------------|--------------------------------------------------------------------|
| `system_state` | Key-value storage (game time, system prompt template)              |
| `llm_logs`     | LLM request/response logging (with parent_id + label)              |
| `llm_steps`    | Per-step LLM metrics (tool calls, token usage, timings, reasoning) |

World state (entities, observations, relationships, conversation history) is stored in Neo4j via the Neo4j driver, not in SQLite.

---

## 6. Core Systems

### 6.1 Internal Voices (Inner Skills)

Fantasy-steampunk inner monologue — each skill is a distinct voice in the player's mind. Voices: `LOGIC`, `RHETORIC`, `EMPATHY`, `PERCEPTION`, `VOLITION`, `ENDURANCE`, `SORCERY`, `SUGGESTION`, `INSTINCT`, `MIGHT`, `CLOCKWORK`, `ALCHEMY`.

These map to character stats in `src/types/entities.ts` (`CharacterStats` interface). The system prompt in `src/server/llm/prompt.ts` instructs the LLM about voice personalities and includes the active plot tree.

The system prompt is runtime-configurable via the system prompt API endpoints. The template is stored in the `system_state` table (key `gm_system_prompt`) and supports `{{setting_description}}`, `{{tone_description}}`, and `{{game_time}}` variables that are replaced with live data by `buildSystemPrompt()`. Setting and tone come from the active seed story. World state and plots are not dumped into the prompt — the GM fetches them on demand via `getContext`. If no custom template is stored, the `DEFAULT_SYSTEM_PROMPT_TEMPLATE` constant is used.

### 6.2 Skill Checks

- **White Checks**: Repeatable after stat increases
- **Red Checks**: High-stakes, one-time opportunities (`isRed` in `DialogueOption`)
- **Formula**: `2d6 + Stat >= Difficulty`
- **Probability display**: Arc SVG + percentage before rolling; color-coded thresholds
- **Narrative**: After a roll completes, the result is sent to the AI as user input for narrative integration
- **Conditional outcomes**: The `conditions` array on a check can define custom success/failure labels via JS expression evaluation

### 6.3 Seed Story System

Seed data (entities, locations, characters, root plot, initial time, initial scene) is organized into pluggable seed story modules under `src/server/seed-stories/`. Each module exports a `SeedStory` object conforming to the interface in `types.ts`.

The active story is determined by the `ACTIVE_SEED_STORY` constant in `index.ts`. `getActiveSeedStory()` returns the active story's data, and `seedDatabase()` in `src/server/mcp/seed.ts` reads from it to populate Neo4j on startup.

**To add a new seed story:**
1. Create a new file in `src/server/seed-stories/` exporting a `SeedStory` object
2. Register it in the `STORIES` map in `index.ts`
3. Change `ACTIVE_SEED_STORY` to the new story ID

---

## 7. Time System

Each in-game day is divided into 12 segments of 2 hours each (segment 0 = midnight–2am, segment 11 = 10pm–midnight). Time only advances when the GM calls the `advanceTime` tool — the player cannot directly control time.

**Storage**: `game_time_day` and `game_time_segment` keys in `system_state` table. Defaults to day 1, segment 2 (dawn).

**`GameTime`** (in `src/types/entities.ts`): `{ day: number, segment: number }`

**Model functions** (in `src/server/models/time.ts`):

- `getGameTime()` / `setGameTime(time)` — read/write time from system_state
- `advanceGameTime(segments)` — adds segments (wraps days at 12), returns old and new times
- `describeTime(time)` — human-readable string: "Day 3, Dawn (~4am-6am)"
- `SEGMENT_LABELS` — constant map: `{ 0: "Midnight", 1: "Late Night", 2: "Dawn", ... }`

---

## 8. Console Client

A standalone Node.js REPL client (`src/console/main.ts`) that implements the full dialogue loop — begin story, select options, and resume — through the same SSE endpoints.

- **State machine**: `IDLE → WAITING → AWAITING_OPTION → WAITING → ...`
- **Rendering**: Terminal output via `chalk` (speaker colors mirrored from `src/shared/colors.ts`'s `VOICE_COLORS`) and `log-update` (progressive streaming updates)
- **SSE handling**: `ConsoleSseClient` (`src/console/SseClient.ts`) handles core dialogue events (`step_start`, `streaming_messages`, `streaming_reset`, `options`, `parsed`, `error`, `done`). World/plot events are intentionally ignored — the console has no world editor.
- **Session resume**: On startup, fetches `GET /api/history` + `GET /api/session/current` to attempt restore
- **Custom input**: Players can type free-form responses that get sent to the LLM

---

## 9. Development Workflow

### 9.1 Adding a New Elysian Tool

Elysian tools (custom LLM tools defined in this codebase) are created with the `tool()` function from `@ai-sdk`:

1. Create a new file in `src/server/llm/` following the existing pattern (e.g., `generateDialogueStep.ts`, `advanceTime.ts`)
2. Define the Zod input schema and `execute` function
3. Register it in `src/server/llm/index.ts` in the `allTools` object within `generateTurn()`
4. Update the system prompt in `src/server/llm/prompt.ts` if the LLM needs guidance on when to use it

### 9.2 Neo4j-Backed Tools

World manipulation tools are defined as AI SDK tools in `src/server/memory/tools.ts`. To add a new Neo4j-backed tool, add a new tool definition in `createMemoryTools()` following the existing patterns. To add a new Elysian tool, follow section 9.1.

### 9.3 Adding a New Voice/Skill

1. Add the stat to `CharacterStats` in `src/types/entities.ts`
2. Add voice personality description to the system prompt in `src/server/llm/prompt.ts`
3. Add a color entry in `src/shared/colors.ts`'s `VOICE_COLORS` map

### 9.4 Managing Seed Data

Initial world state is defined by the active seed story in `src/server/seed-stories/`. Edit the active story's module or create a new one. Change `ACTIVE_SEED_STORY` in `index.ts` to switch stories. See section 6.3 for the full seed story system.
