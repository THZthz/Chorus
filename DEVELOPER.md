# Developer Documentation: Elysian Dialogue

Architecture, core systems, and data structures of the **Elysian Dialogue** application.

---

## 1. Project Overview

**Elysian Dialogue** is a cinematic RPG-style dialogue engine with a vertical-scrolling "thought stream" aesthetic, branching dialogue paths, and probabilistic skill checks influenced by character attributes.

- **Stack:** TypeScript, Node.js
- **Backend:** Express + Neo4j (via local `src/server/memory/` module)
- **AI:** Single-LLM Game Master (Gemini/DeepSeek via Vercel AI SDK v6)
- **SSE:** Server-Sent Events for real-time streaming of LLM output
- **Console client:** Standalone Node.js REPL with chalk rendering
- **Deployment:** Local-only — runs on localhost, no authentication required by design

---

## 2. Core Architecture

```
┌─────────────────────────────────────────────────────────────────────┐
│                        CONSOLE CLIENT                               │
│  src/console/main.ts  ── SSE stream ──►  chalk rendering + REPL     │
└──────────────────────────────┬──────────────────────────────────────┘
                               │ POST /api/chat/stream
                               ▼
┌──────────────────────────────────────────────────────────────────────┐
│                        EXPRESS SERVER                                │
│  src/server/main.ts  ── port 3000                                    │
│  src/server/api.ts   ── /api/chat/stream, /api/history, /api/reset   │
└──────────────────────────────┬───────────────────────────────────────┘
                               │
                               ▼
┌──────────────────────────────────────────────────────────────────────┐
│                      LLM GAME MASTER                                 │
│  src/server/llm/index.ts  ── generateTurn()                          │
│                                                                      │
│  streamText({                                                        │
│    tools: {                                                          │
│      getScene, updateWorld, remember, getConversation,               │
│      searchMemory, advancePlot,    ← memory/tools.ts (6 tools)       │
│      generateDialogueStep,         ← llm/tools/ (Elysian tool)       │
│      advanceTime                   ← llm/tools/ (Elysian tool)       │
│    }                                                                 │
│  })                                                                  │
│                                                                      │
│  stopWhen: generateDialogueStep passes validation                    │
│  prepareStep: nudges if GM forgets dialogue output                   │
└──────────────────────────────┬───────────────────────────────────────┘
                               │ tool calls read/write Neo4j
                               ▼
┌──────────────────────────────────────────────────────────────────────┐
│                     MEMORY LAYER (Neo4j-backed)                      │
│  src/server/memory/client.ts  ── MemoryClient singleton              │
│                                                                      │
│  ┌─────────────┐  ┌─────────────┐  ┌─────────────┐                   │
│  │ ShortTerm   │  │  LongTerm   │  │ Reasoning   │                   │
│  ├─────────────┤  ├─────────────┤  ├─────────────┤                   │
│  │ messages    │  │ entities    │  │ traces      │                   │
│  │ conversation│  │ facts       │  │ steps       │                   │
│  │             │  │ preferences │  │ tool calls  │                   │
│  └──────┬──────┘  └──────┬──────┘  └──────┬──────┘                   │
│         └────────────────┼───────────────┘                           │
│                          ▼                                           │
│  ┌─────────────┐  ┌─────────────┐  ┌──────────────┐                  │
│  │  Search     │  │  Context    │  │  Observer    │                  │
│  ├─────────────┤  ├─────────────┤  ├──────────────┤                  │
│  │  parallel   │  │  assemble   │  │  deltas +    │                  │
│  │  vector     │  │  markdown   │  │  compression │                  │
│  └─────────────┘  └─────────────┘  └──────────────┘                  │
│                                                                      │
│  embedder.ts ── local ONNX (384d) or OpenAI-compatible API           │
│  neo4j.ts    ── driver wrapper with value normalization              │
│  schema.ts   ── constraints + indexes (7 unique, 6 vector)           │
└──────────────────────────────┬───────────────────────────────────────┘
                               │
                               ▼
┌──────────────────────────────────────────────────────────────────────┐
│                         NEO4J DATABASE                               │
│  Node labels: Conversation, Message, Entity, Preference, Fact,       │
│  NPCDisposition, PlayerFlag, ReasoningTrace, ReasoningStep, ToolCall │
└──────────────────────────────────────────────────────────────────────┘
```

---

## 3. Project File Listing

```
.
├── Makefile                   # Unified dev commands (Neo4j, server, console, lint)
├── docker-compose.test.yml    # Neo4j test container
├── package.json               # TypeScript project config
├── tsconfig.json
└── src/
    ├── console/
    │   ├── main.ts            # Standalone Node.js REPL client for dialogue interaction
    │   ├── SseClient.ts       # Lightweight SSE consumer for the console
    │   └── markdown.ts        # Terminal markdown → chalk-styled text
    ├── server/
    │   ├── main.ts            # Express entry (port 3000), MemoryClient init, seed on startup
    │   ├── api.ts             # REST API + SSE streaming endpoints
    │   ├── validation.ts      # Zod request validation (chatStreamSchema)
    │   ├── llm/
    │   │   ├── index.ts       # generateTurn(): full-stream SSE turn loop
    │   │   ├── model.ts       # getModel(): lazy-init provider model (Gemini → DeepSeek)
    │   │   ├── prompt.ts      # System prompt template + buildSystemPrompt()
    │   │   ├── events.ts      # TurnEventEmitter: typed SSE dispatch
    │   │   └── tools/
    │   │       ├── advanceTime.ts           # Advance in-game clock by segments/days
    │   │       ├── generateDialogueStep.ts  # Produce messages + options with validation
    │   │       └── shared.ts               # Helpers: checkText (character filter), wrapSafe
    │   ├── memory/
    │   │   ├── client.ts      # MemoryClient singleton — wires all memory layers
    │   │   ├── types.ts       # Shared types (Entity, Message, Fact, Preference, etc.)
    │   │   ├── neo4j.ts       # Neo4jClient — thin wrapper over neo4j-driver
    │   │   ├── schema.ts      # Index/constraint/vector index creation
    │   │   ├── embedder.ts    # Local embeddings (Xenova/ONNX) + OpenAI-compatible fallback
    │   │   ├── shortTerm.ts   # Conversation messages with sequential NEXT_MESSAGE linking
    │   │   ├── longTerm.ts    # Entities (POLE+O), preferences, facts, relationships
    │   │   ├── reasoning.ts   # Reasoning traces, steps, tool calls
    │   │   ├── observer.ts    # World delta tracking + token-threshold context compression
    │   │   ├── search.ts      # Parallel hybrid vector search across memory types
    │   │   ├── context.ts     # Assembled GM context (markdown summary from all layers)
    │   │   ├── gameState.ts   # Game save/resume via options on :Conversation node
    │   │   ├── tools.ts       # 6 GM-verb tool definitions (AI SDK tools)
    │   │   └── reset.ts       # Clear Neo4j database (MATCH (n) DETACH DELETE n)
    │   ├── models/
    │   │   ├── time.ts        # Game time CRUD via Neo4j :GameTime node
    │   │   └── shared.ts      # safeJsonParse utility
    │   └── seed-stories/
    │       ├── index.ts       # Story registry + ACTIVE_SEED_STORY constant
    │       ├── types.ts       # SeedStory, SeedPlot interfaces
    │       ├── seed.ts        # Apply active seed story to Neo4j via MemoryClient
    │       └── magic-awakening.ts  # Default seed story
    ├── shared/
    │   ├── events.ts          # SSE event type definitions (typed event map)
    │   ├── sse.ts             # Shared SSE stream parser (async generator)
    │   ├── colors.ts          # VOICE_COLORS: 12 inner-voice → hex color map
    │   └── constants.ts       # SKILL_NAMES, TOOL_NAMES, SEGMENT_LABELS, etc.
    └── types/
        ├── dialogue.ts        # Message, DialogueOption, NotificationType
        └── entities.ts        # CharacterStats, Character, EntityType, GameEntitySubtype
```

---

## 4. Turn Lifecycle

```
POST /api/chat/stream
        │
        ▼
┌──────────────────────────────────────────────────────┐
│  generateTurn()                                      │
│                                                      │
│  streamText({                                        │
│    tools: {                                          │
│      ← 6 Neo4j-backed tools from createMemoryTools() │
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

---

## 5. SSE Events

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

---

## 6. LLM Tools

Two layers of tools:

**Elysian tools** (defined in `src/server/llm/tools/`):

| Tool                   | Purpose                                     | SSE Event                                 |
|------------------------|---------------------------------------------|-------------------------------------------|
| `generateDialogueStep` | Produce narrative messages + player options | `streaming_messages`, `options`, `parsed` |
| `advanceTime`          | Advance in-game clock by N segments         | `time_update`                             |

**Neo4j-backed tools** (6 tools defined in `src/server/memory/tools.ts`):

| Tool              | Purpose                                                                                                                               |
|-------------------|---------------------------------------------------------------------------------------------------------------------------------------|
| `getScene`        | Returns everything in-frame: location, NPCs with dispositions, objects, inventory, active plots with beats/branches, and player flags |
| `updateWorld`     | Change world state (move/change/create/relate/fact/disposition/condition actions)                                                     |
| `remember`        | Store a GM note tied to an entity                                                                                                     |
| `getConversation` | Retrieve recent dialogue history                                                                                                      |
| `searchMemory`    | Vector search across all memory (entities, facts, messages)                                                                           |
| `advancePlot`     | Story progression: beat lifecycle, branch management, player flag tracking                                                            |

All tools are defined as AI SDK `tool()` definitions and registered in `generateTurn()`.

---

## 7. API Endpoints

| Method | Path                | Purpose                                              |
|--------|---------------------|------------------------------------------------------|
| `POST` | `/api/chat/stream`  | Primary AI turn (SSE streaming)                      |
| `GET`  | `/api/history`      | Full conversation history from ShortTermMemory       |
| `GET`  | `/api/game/current` | Current dialogue options from `:Conversation` node   |
| `POST` | `/api/reset`        | Clear Neo4j and re-seed                              |

---

## 8. Game Time

Each in-game day is divided into 12 segments of 2 hours each (segment 0 = midnight–2am, segment 11 = 10pm–midnight). Time only advances when the GM calls `advanceTime`.

**Storage**: Singleton `:GameTime {id: "current"}` node in Neo4j with `day` and `segment` properties. Defaults to day 1, segment 2 (dawn).

**Model functions** (`src/server/models/time.ts`):

- `getGameTime()` / `setGameTime(time)` — read/write time from Neo4j
- `advanceGameTime(segments)` — adds segments (wraps days at 12), returns old and new times
- `describeTime(time)` — human-readable string: "Day 3, Dawn (~4am-6am)"
- `SEGMENT_LABELS` — constant map: `{ 0: "Midnight", 1: "Late Night", 2: "Dawn", ... }`

---

## 9. Memory Architecture

The memory layer (`src/server/memory/`) provides a Neo4j-backed persistent world model. All subsystems are wired through the `MemoryClient` singleton (`client.ts`), which owns the Neo4j connection, creates the schema, and initializes the embedder.

### 9.1 MemoryClient (Facade + Singleton)

`MemoryClient` (`client.ts`, ~100 lines) is the single entry point to all memory subsystems. It composes six subsystems and exposes them as readonly properties:

```
MemoryClient.getCachedInstance()
  .neo4j       → Neo4jClient          (driver wrapper)
  .shortTerm   → ShortTermMemory      (conversation + messages)
  .longTerm    → LongTermMemory       (entities, facts, preferences, dispositions, flags)
  .reasoning   → ReasoningMemory      (traces, steps, tool calls)
  .search      → MemorySearch         (parallel vector search across layers)
  .context     → ContextAssembler     (fetch + format markdown context)
  .observer    → MemoryObserver       (delta tracking + token-threshold compression)
```

Boot sequence: `getInstance()` → creates `Neo4jClient` → `verifyConnectivity()` → `getEmbedder()` → `setupSchema()` → constructs all subsystems.

### 9.2 Type System

All memory types are defined in `types.ts` (~170 lines, type-only):

| Type               | Key Fields                                                                                | Neo4j Node                  |
|--------------------|-------------------------------------------------------------------------------------------|-----------------------------|
| `MemoryEntity`     | id, name, type (POLE+O), subtype?, description?, aliases[], metadata, embedding[], isNew? | `:Entity`                   |
| `MemoryMessage`    | id, role (user/assistant/system), content, metadata, embedding[]                          | `:Message`                  |
| `MemoryPreference` | id, category, preference, context?, confidence, embedding[]                               | `:Preference`               |
| `MemoryFact`       | id, subject, predicate, object, confidence, validFrom?/Until?                             | `:Fact`                     |
| `NPCDisposition`   | id, npcName, targetName, sentiment, summary                                               | `:NPCDisposition`           |
| `PlayerFlag`       | id, flagId, description, source                                                           | `:PlayerFlag`               |
| `PlayerCondition`  | description, effects[], duration?, source?                                                | (stored in Entity metadata) |
| `ReasoningTrace`   | id, task, taskEmbedding[], steps[], outcome?, success?                                    | `:ReasoningTrace`           |
| `ReasoningStep`    | id, traceId, stepNumber, thought?, action?, observation?                                  | `:ReasoningStep`            |
| `ToolCall`         | id, stepId, toolName, arguments, result?, status, durationMs?                             | `:ToolCall`                 |

Composite types: `SearchResults` (arrays with `similarity`), `AssembledContext` (arrays stripped of similarity + markdown `summary`), `ObservationResult` (messageCount, reflections[], observations[]).

### 9.3 Neo4j Schema

Managed by `schema.ts` (~100 lines), called once at startup:

**Unique constraints (7):** `id` on `:Conversation`, `:Message`, `:Entity`, `:Preference`, `:Fact`, `:ReasoningTrace`, `:ReasoningStep`

**Unique constraint (1):** `flagId` on `:PlayerFlag`

**Regular indexes (5):** `Message.timestamp`, `Entity.type`, `Entity.name`, `Preference.category`, `ReasoningTrace.success`

**Composite indexes (2):** `NPCDisposition(npcName, targetName)`, `NPCDisposition(targetName)`

**Vector indexes (6, require Neo4j 5.11+, COSINE similarity):**

| Index                      | Label           | Property        | Dims |
|----------------------------|-----------------|-----------------|------|
| `message_embedding_idx`    | Message         | embedding       | 384  |
| `entity_embedding_idx`     | Entity          | embedding       | 384  |
| `preference_embedding_idx` | Preference      | embedding       | 384  |
| `fact_embedding_idx`       | Fact            | embedding       | 384  |
| `task_embedding_idx`       | ReasoningTrace  | task_embedding  | 384  |
| `step_embedding_idx`       | ReasoningStep   | embedding       | 384  |

**Relationship types:**

| Type              | Direction                    | Purpose                       |
|-------------------|------------------------------|-------------------------------|
| `HAS_MESSAGE`     | `(Conversation)→(Message)`   | Conversation membership       |
| `FIRST_MESSAGE`   | `(Conversation)→(Message)`   | Head pointer for ordered list |
| `NEXT_MESSAGE`    | `(Message)→(Message)`        | Sequential linked list        |
| `HAS_STEP`        | `(ReasoningTrace)→(Step)`    | Trace decomposition           |
| `HAS_TOOL_CALL`   | `(ReasoningStep)→(ToolCall)` | Tool invocation record        |
| `HAS_DISPOSITION` | `(Entity)→(NPCDisposition)`  | NPC attitude toward a target  |
| `LOCATED_AT`      | `(Entity)→(Entity)`          | Spatial placement (dynamic)   |
| `LOCATED_IN`      | `(Entity)→(Entity)`          | Container hierarchy (dynamic) |
| `CARRIES`         | `(Entity)→(Entity)`          | Inventory (dynamic)           |

Dynamic relationships (`LOCATED_AT`, `CARRIES`, `ALLIED_WITH`, etc.) are created by `tools.ts` via `longTerm.addRelationship()` with sanitized type names.

### 9.4 Embeddings

`embedder.ts` (~140 lines) provides two strategies behind an `Embedder` interface:

- **`LocalEmbedder`**: `@xenova/transformers` with `Xenova/all-MiniLM-L6-v2` (384-dim, ~80MB ONNX). Uses mean pooling, processes sequentially to avoid ONNX memory pressure.
- **`OpenAICompatibleEmbedder`**: Any OpenAI-compatible API (configurable via `EMBEDDING_API_URL`/`EMBEDDING_API_KEY`/`EMBEDDING_MODEL` env vars). Default model `text-embedding-3-small` (1536-dim).

**Strategy pattern + Factory**: `getEmbedder()` returns a singleton, preferring API if credentials are set, otherwise local ONNX. The embedder is used by `ShortTermMemory`, `LongTermMemory`, and `ReasoningMemory` for vector search indexing.

### 9.5 ShortTermMemory

`shortTerm.ts` (~230 lines). Manages conversation history as an ordered linked list of `:Message` nodes under a singleton `:Conversation` node (keyed by `session_id: "elysian-game"`).

| Method                   | Behavior                                                                                          |
|--------------------------|---------------------------------------------------------------------------------------------------|
| `addMessage()`           | Creates `:Message`, links via `HAS_MESSAGE` + `NEXT_MESSAGE` + `FIRST_MESSAGE`, optionally embeds |
| `getConversation(limit)` | Returns messages ordered oldest-first (reverse of timestamp sort)                                 |
| `searchMessages(query)`  | Vector similarity search on `message_embedding_idx`                                               |

Message linking algorithm: find the last message (no outgoing `NEXT_MESSAGE`), create `(prev)-[:NEXT_MESSAGE]→(new)`. First message also gets `(conv)-[:FIRST_MESSAGE]→(msg)`.

### 9.6 LongTermMemory

`longTerm.ts` (~590 lines). Persistent world state — the largest subsystem. Manages entities, facts, preferences, relationships, NPC dispositions, player flags, player conditions, and player stats.

**Entity operations (POLE+O model):**
- `addEntity(name, type, options?)` — MERGE on name, apply dynamic labels (`:Entity:Person:Character` via `SET e:${typeLabel}`), store aliases in metadata JSON
- `getEntity(name, type?)` — lookup by name with optional type filter
- `searchEntities(query)` — vector search on `entity_embedding_idx`, post-filter by entityTypes

**Fact triples:** `addFact(subject, predicate, object, options?)` — CREATE with embedding of `"subject predicate object"`, optional temporal validity (`validFrom`/`validUntil`).

**Preferences:** `addPreference(category, preference, options?)` — CREATE with embedding of `"category: preference (context)"`.

**Relationships:** `addRelationship(sourceName, targetName, type, options?)` — MERGE dynamic relationship `(a)-[r:${safeType}]→(b)`. Type name is sanitized to `[A-Za-z0-9_]`.

**NPC dispositions:** `setDisposition(npcName, targetName, sentiment, summary)` — MERGE `:NPCDisposition` node linked via `HAS_DISPOSITION` from NPC entity. Composite key on `(npcName, targetName)`.

**Player flags:** `setPlayerFlag` / `hasPlayerFlag` / `getPlayerFlags` / `removePlayerFlag` — MERGE/DELETE on `:PlayerFlag` nodes with unique `flagId`.

**Player conditions:** `updatePlayerCondition(playerName, conditionId, condition | null)` — reads/writes the `conditions` dict inside the player entity's `metadata` JSON.

**Player stats:** `getPlayerStats(playerName?)` — reads `metadata.stats` from player entity.

### 9.7 ReasoningMemory

`reasoning.ts` (~280 lines). Chain-of-thought trace storage for LLM reasoning introspection.

| Method               | Behavior                                                                  |
|----------------------|---------------------------------------------------------------------------|
| `startTrace(task)`   | CREATE `:ReasoningTrace` with optional `task_embedding`                   |
| `addStep(traceId)`   | Append `:ReasoningStep` via `HAS_STEP`, embeds thought/action/observation |
| `recordToolCall()`   | Attach `:ToolCall` to step via `HAS_TOOL_CALL`                            |
| `completeTrace()`    | Set outcome, success, completed_at                                        |
| `getSimilarTraces()` | Vector search on `task_embedding_idx`, optional success filter            |
| `searchSteps(query)` | Vector search on `step_embedding_idx`, joins parent trace                 |

### 9.8 MemoryObserver

`observer.ts` (~170 lines). Pure in-memory (not persisted) — tracks world deltas and manages token budget.

- **`onWorldChange(delta)`** — called by `tools.ts` after each `updateWorld` mutation. Pushes a `WorldDelta` (action + summary + timestamp) to an in-memory buffer.
- **`onMessageStored(content)`** — tracks character count. When approximate token count exceeds **30K tokens** (configurable via `thresholdTokens`), triggers `generateReflection()`:
  1. Fetches last 100 messages from ShortTermMemory
  2. Takes messages older than the `recentWindow` (last 20)
  3. Builds a reflection from last 10 world deltas + first 100 chars of 5 oldest messages
  4. Stores the reflection string for later context injection
- **`getObservations()`** — returns `ObservationResult` with latest 20 deltas as `Observation` objects plus accumulated reflection strings.

### 9.9 MemorySearch

`search.ts` (~90 lines). Parallel hybrid search facade across memory layers.

```
search(query, { types: ["messages", "entities", "preferences", "traces"] })
  ├── shortTerm.searchMessages(query)     → vector similarity
  ├── longTerm.searchEntities(query)       → vector similarity
  ├── longTerm.getPreferences(category?)   → (no vector, similarity=1.0)
  └── reasoning.getSimilarTraces(query)    → vector similarity
```

All selected searches run in parallel via `Promise.all`. Returns `SearchResults` with `similarity` on each item.

### 9.10 ContextAssembler

`context.ts` (~120 lines). Assembles GM-facing context from all memory layers.

`assemble({ query?, includeShortTerm?, includeLongTerm?, includeReasoning? })`:
1. Fetches from shortTerm (recent conversation), longTerm (entity vector search + all preferences), reasoning (similar traces, limit 3) — all in parallel
2. Builds a markdown `summary` string with sections: "Recent Conversation", "Relevant Entities", "User Preferences", "Similar Past Tasks"
3. Returns `AssembledContext` — raw arrays (stripped of `similarity`) + formatted `summary`

### 9.11 Game State Persistence

`gameState.ts` (~36 lines). Save/resume support by persisting dialogue options as JSON on the `:Conversation` node.

- `saveCurrentOptions(options)` — writes `options` JSON to `(c:Conversation {session_id: "elysian-game"})`
- `getCurrentOptions()` — reads options back on resume

The Neo4j database is the authoritative world state — there is no separate session concept.

### 9.12 Tools (AI SDK Definitions)

`tools.ts` (~460 lines). Defines 6 AI SDK tools as `tool()` factory calls with Zod input schemas.

**`getScene`**: Complex Cypher query joining the player's location, co-located NPCs, objects, inventory, parent locations, active plots (PENDING/IN_PROGRESS events), NPC dispositions toward the player, and all player flags. Returns a single map with all scene data.

**`updateWorld`**: Multiplexes 7 mutation actions via a discriminated union:
- `move` — deletes old `LOCATED_AT`, creates new one
- `change` — reads existing entity, re-adds with updated fields
- `create` — calls `longTerm.addEntity`
- `relate` — calls `longTerm.addRelationship`
- `fact` — calls `longTerm.addFact`
- `disposition` — calls `longTerm.setDisposition`
- `condition` — calls `longTerm.updatePlayerCondition` (pass null to delete)

After each mutation, calls `observer.onWorldChange()` to record the delta.

**`remember`**: Stores a GM note via `shortTerm.addMessage("system", ...)`.

**`getConversation`**: Delegates to `shortTerm.getConversation(limit)`.

**`searchMemory`**: Delegates to `client.search.search(query, { types })`.

**`advancePlot`**: Reads/writes metadata on EVENT entities — manages beat lifecycle (activate/complete), branch selection, and player flag tracking.

### 9.13 Data Flow Summary

```
User Input
  │
  ▼
generateTurn()
  │
  ├─► shortTerm.addMessage("user", input)
  ├─► streamText({ tools }) ──► LLM
  │     │
  │     ├─► getScene ──► direct Cypher (bypasses layers)
  │     ├─► updateWorld ──► longTerm.* + observer.onWorldChange()
  │     ├─► remember ──► shortTerm.addMessage("system")
  │     ├─► searchMemory ──► client.search.search()
  │     ├─► advancePlot ──► longTerm entity metadata r/w
  │     ├─► getConversation ──► shortTerm.getConversation()
  │     ├─► advanceTime ──► models/time.ts (Neo4j write)
  │     └─► generateDialogueStep ──► SSE + persist messages
  │
  └─► saveCurrentOptions(finalOptions) ──► Conversation node
```

---

## 10. Internal Voices (Inner Skills)

Fantasy-steampunk inner monologue — each skill is a distinct voice in the player's mind. Voices: `LOGIC`, `RHETORIC`, `EMPATHY`, `PERCEPTION`, `VOLITION`, `ENDURANCE`, `SORCERY`, `SUGGESTION`, `INSTINCT`, `MIGHT`, `CLOCKWORK`, `ALCHEMY`.

These map to `CharacterStats` in `src/types/entities.ts`. The system prompt in `src/server/llm/prompt.ts` instructs the LLM about voice personalities and includes the active plot tree.

The system prompt uses `DEFAULT_SYSTEM_PROMPT_TEMPLATE` from `src/server/llm/prompt.ts` with `{{setting_description}}`, `{{tone_description}}`, `{{game_time}}` variables replaced by `buildSystemPrompt()`. Setting and tone come from the active seed story. World state is **not** dumped into the prompt — the GM fetches it on demand via tools.

### Skill Checks

- **White Checks**: Repeatable after stat increases
- **Red Checks**: High-stakes, one-time opportunities (`isRed` in `DialogueOption`)
- **Formula**: `2d6 + Stat >= Difficulty`
- **Probability display**: Arc SVG + percentage before rolling; color-coded thresholds
- **Narrative**: After a roll completes, the result is sent to the AI as user input for narrative integration
- **Conditional outcomes**: The `conditions` array on a check can define custom success/failure labels via JS expression evaluation

---

## 11. Seed Story System

Seed data (entities, locations, characters, root plot, initial time, initial scene) is organized into pluggable seed story modules under `src/server/seed-stories/`. Each module exports a `SeedStory` object conforming to the interface in `types.ts`.

The active story is determined by the `ACTIVE_SEED_STORY` constant in `index.ts`. `getActiveSeedStory()` returns the active story's data, and `seedDatabase()` in `seed.ts` reads from it to populate Neo4j on startup. On `/api/reset`, the database is cleared and re-seeded.

**To add a new seed story:**
1. Create a new file in `src/server/seed-stories/` exporting a `SeedStory` object
2. Register it in the `STORIES` map in `index.ts`
3. Change `ACTIVE_SEED_STORY` to the new story ID

---

## 12. Console Client

A standalone Node.js REPL client (`src/console/main.ts`) that implements the full dialogue loop — begin story, select options, and resume — through the same SSE endpoints.

- **State machine**: `IDLE → WAITING → AWAITING_OPTION → WAITING → ...`
- **Rendering**: Terminal output via `chalk` (speaker colors from `src/shared/colors.ts`'s `VOICE_COLORS`) and `log-update` (progressive streaming updates)
- **SSE handling**: `ConsoleSseClient` (`src/console/SseClient.ts`) handles core dialogue events; world/plot events are intentionally ignored
- **Session resume**: On startup, fetches `GET /api/history` + `GET /api/game/current` to attempt restore
- **Markdown rendering**: `renderMarkdown()` in `src/console/markdown.ts` converts basic markdown to chalk-styled terminal output
- **Custom input**: Players can type free-form responses sent to the LLM

---

## 13. Key Design Decisions

1. **World state in Neo4j** — entities, observations, relationships, and game time stored in Neo4j via local memory module
2. **Tools statically defined** — all 8 tools (2 Elysian + 6 Neo4j-backed) registered in `generateTurn()`; no dynamic discovery
3. **LLM text output silently discarded** — the system prompt instructs tool-only output; text deltas are ignored
4. **No static dialogue** — all narrative is AI-generated
5. **Shared event types** — `src/shared/events.ts` ensures backend/console event contracts match
6. **Lightweight console client** — standalone REPL with chalk rendering; no entity editor or debug panel
7. **SSE progressive streaming** — `generateDialogueStep` streams messages/options incrementally via partial JSON parsing
8. **Singleton MemoryClient** — single entry point to all memory subsystems, lazy-init with caching
9. **POLE+O entity model** — entities have a type (PERSON/OBJECT/LOCATION/ORGANIZATION/EVENT) with dynamic Neo4j labels for efficient graph traversal
10. **Observer as in-memory compression** — world deltas tracked in memory only (not persisted); reflections generated at token threshold to keep LLM context manageable

---

## 14. Development Workflow

### 14.1 Adding a New Elysian Tool

1. Create a new file in `src/server/llm/tools/` following the existing pattern
2. Define the Zod input schema and `execute` function (wrap with `wrapSafe` from `shared.ts`)
3. Register it in `src/server/llm/index.ts` in the `allTools` object within `generateTurn()`
4. Update the system prompt in `src/server/llm/prompt.ts` if the LLM needs guidance

### 14.2 Adding a Neo4j-Backed Tool

Add a new tool definition in `createMemoryTools()` in `src/server/memory/tools.ts` following existing patterns. Delegate to the appropriate memory subsystem (`longTerm.*`, `shortTerm.*`, etc.).

### 14.3 Adding a New Voice/Skill

1. Add the stat to `CharacterStats` in `src/types/entities.ts`
2. Add voice personality description to the system prompt in `src/server/llm/prompt.ts`
3. Add a color entry in `src/shared/colors.ts`'s `VOICE_COLORS` map

### 14.4 Managing Seed Data

Edit the active seed story in `src/server/seed-stories/` or create a new one. Change `ACTIVE_SEED_STORY` in `index.ts` to switch stories.

### 14.5 Debugging LLM Calls with DevTools

All `streamText` calls are captured to `.devtools/generations.json` via the `devToolsMiddleware()` wrapper in `src/server/llm/model.ts`.

**Data model:** One top-level entry per `streamText` call. `runs[]` = individual invocations of `generateTurn()`; `steps[]` = each tool-calling iteration within a run.

**Viewing data:**

```bash
# Launch interactive web UI
npx @ai-sdk/devtools
# Open http://localhost:4983

# Raw inspection
cat .devtools/generations.json | jq 'keys'          # ["runs", "steps"]
cat .devtools/generations.json | jq '.runs | length' # run count
cat .devtools/generations.json | jq '.steps | length' # step count
```

**Step overview:**

```bash
cat .devtools/generations.json | jq '[.steps[] | {
  step: .step_number, type, model: .model_id,
  duration_ms, error, usage: (.usage | fromjson | .outputTokens.total)
}]'
```

**Tool call inspection:**

```bash
# Tool call names and argument shapes per step
cat .devtools/generations.json | jq '[.steps[] | {
  step: .step_number,
  tool_calls: [.output | fromjson | .toolCalls[]? | {
    name: .toolName,
    args: (.input | fromjson | keys)
  }] | select(length > 0)
}]'

# Tool call results (appear in the NEXT step's input)
cat .devtools/generations.json | jq '
  .steps[1].input | fromjson | .prompt[] | select(.role == "tool")
'

# Full args for a specific tool call
cat .devtools/generations.json | jq '
  .steps[3].output | fromjson | .toolCalls[0].input | fromjson
'

# Finish reasons per step
cat .devtools/generations.json | jq '.steps[] | {
  step: .step_number,
  finishReason: (.output | fromjson | .finishReason.unified)
}'
```

**Messages flowing through steps:**

```bash
cat .devtools/generations.json | jq -r '
  .steps[] |
  "--- Step \(.step_number) ---",
  (.input | fromjson | .prompt[]? |
   "  [\(.role)] \(.content | tostring)[0:150]")
'

# System prompt (first step only, truncated)
cat .devtools/generations.json | jq -r '
  .steps[0].input | fromjson | .prompt[0].content[0:500]
'
```

**Common issues:**

| Symptom                              | What to check                                                                     |
|--------------------------------------|-----------------------------------------------------------------------------------|
| GM calls wrong tools or misses steps | Tool call results for validation errors; `getContext` returning garbage           |
| GM re-creates existing entities      | `getContext` returning empty — check for Neo4j LIMIT errors on float params       |
| Nudge messages too aggressive        | `prepareStep` injection pattern — look for "ERROR:" in user messages              |
| Dialogue never reaches player        | `finishReason` stuck on `tool-calls` — GM may be stuck in a loop                  |
| Messages not persisted               | `storeMessage` never appears in any step's tool calls                             |
| DeepSeek float weirdness             | Tool result errors containing `'20.0' is not a valid value` — use `sanitizeInt()` |
| GM calls same tool repeatedly        | Consecutive identical tool names in a single step — likely duplicate-creating     |

**Token usage:** Early steps have high cache-hit ratios. `usage.outputTokens.total` includes reasoning overhead — DeepSeek reports `completion_tokens_details.reasoning_tokens` separately. Zero `textParts` per step is expected (code discards LLM text output).
