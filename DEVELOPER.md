# Developer Documentation: Chorus

Architecture, core systems, and data structures of the **Chorus** application.

---

## 1. Project Overview

**Chorus** is a cinematic RPG-style dialogue engine with a vertical-scrolling "thought stream" aesthetic, branching dialogue paths, and probabilistic skill checks influenced by character attributes.

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
│      queryWorld, mutateWorld, manageSchema, searchWorld,             │
│      editNote, searchNotes, editPlot, searchPlots,                   │
│      ← llm/tools/ (9 GM tools)                                       │
│      generateDialogueStep,              ← llm/tools/ (Chorus tool)   │
│      advanceTime                        ← llm/tools/ (Chorus tool)   │
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
│  ┌─────────────┐  ┌─────────────┐  ┌─────────────┐  ┌─────────────┐  │
│  │ ShortTerm   │  │  LongTerm   │  │   Notes     │  │   Plots     │  │
│  ├─────────────┤  ├─────────────┤  ├─────────────┤  ├─────────────┤  │
│  │ messages    │  │ entities    │  │ GM notes    │  │ beats       │  │
│  │ conversation│  │ facts       │  │ embeddings  │  │ branches    │  │
│  │             │  │ preferences │  │ CRUD        │  │ flags       │  │
│  └──────┬──────┘  └──────┬──────┘  └──────┬──────┘  └──────┬──────┘  │
│         └────────────────┴───────┬────────┴────────────────┘         │
│                                  ▼                                   │
│                           ┌─────────────┐                            │
│                           │  Search     │                            │
│                           ├─────────────┤                            │
│                           │  parallel   │                            │
│                           │  vector     │                            │
│                           └─────────────┘                            │
│                                                                      │
│  embedder.ts ── llama-server embeddings (LLAMA_EMBED_URL)           │
│  neo4j.ts    ── driver wrapper with value normalization              │
│  schema.ts   ── constraints + indexes (6 unique, 4 vector)           │
└──────────────────────────────┬───────────────────────────────────────┘
                               │
                               ▼
┌──────────────────────────────────────────────────────────────────────┐
│                         NEO4J DATABASE                               │
│  Node labels: Conversation, Message, Entity, NPCDisposition,         │
│  Note, Plot, TimeAnchor, TimePoint, GMTurnMessage, RelationshipType, │
│  NodeType, IdCounter                                                 │
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
    │   │   ├── gmMessages.ts # Persist AI SDK messages as :GMTurnMessage nodes for multi-turn continuity
    │   │   ├── conditionEvaluator.ts  # Safe expression evaluator for skill check conditions
    │   │   ├── rollSkillCheck.ts           # Server-side skill check resolver (not a tool)
    │   │   ├── sceneContext.ts             # Pre-loads scene data (location, NPCs, plots) for the GM prompt
    │   │   ├── sceneObserver.ts            # Tracks which entities/plots the GM has seen across turns
    │   │   └── tools/
    │   │       ├── advanceTime.ts           # Advance in-game clock by segments/days
    │   │       ├── generateDialogueStep.ts  # Produce messages + options with validation
    │   │       ├── queryWorld.ts            # Read-only Cypher queries (label-confined)
    │   │       ├── mutateWorld.ts           # Write Cypher queries (label+rel-confined)
    │   │       ├── searchWorld.ts          # Vector search across entities + messages
    │   │       ├── editNote.ts              # Create/update/delete GM notes
    │   │       ├── searchNotes.ts           # Vector search across notes
    │   │       ├── editPlot.ts              # Plot lifecycle management (beats, branches, flags)
    │   │       ├── searchPlots.ts           # Vector search across plots
    │   │       └── shared.ts               # Helpers: checkText (character filter), wrapSafe
    │   ├── memory/
    │   │   ├── client.ts      # MemoryClient singleton — wires all memory layers
    │   │   ├── types.ts       # Shared types (MemoryEntity, MemoryMessage, MemoryPlot, etc.)
    │   │   ├── neo4j.ts       # Neo4jClient — thin wrapper over neo4j-driver
    │   │   ├── schema.ts      # Index/constraint/vector index creation
    │   │   ├── embedder.ts    # llama-server embeddings via /v1/embeddings
    │   │   ├── reranker.ts    # Cross-encoder reranker for two-stage retrieval (optional, API-configured)
    │   │   ├── relationshipManager.ts  # RelationshipManager singleton — three-tier relationship type registry
    │   │   ├── nodeManager.ts  # NodeManager singleton — node label registry mirroring RelationshipManager
    │   │   ├── shortTerm.ts   # Conversation messages with sequential NEXT_MESSAGE linking
    │   │   ├── longTerm.ts    # Entities (COLE+O variant of POLE+O — CHARACTER replaces PERSON), preferences, facts, relationships
    │   │   ├── search.ts      # Parallel hybrid vector search across memory types
    │   │   ├── gameState.ts   # Game save/resume via options on :Conversation node
    │   │   ├── notes.ts       # GM note CRUD with vector embedding
    │   │   ├── plots.ts       # Plot lifecycle management (beats, branches, flags)
    │   │   ├── validation.ts  # Cypher query allowlist validation (labels + relationships)
    │   │   └── reset.ts       # Clear Neo4j database (MATCH (n) DETACH DELETE n)
    │   ├── models/
    │   │   ├── time.ts        # Game time CRUD via Neo4j :TimePoint nodes
    │   │   ├── entity.ts      # Entity formatting helpers (compact/full, aliases, conditions)
    │   │   ├── plot.ts        # Plot tree builder, flag parsing
    │   │   └── schema.ts      # Schema introspection via db.schema.visualization()
    │   └── seed-stories/
    │       ├── index.ts       # Story registry + ACTIVE_SEED_STORY constant
    │       ├── types.ts       # SeedStory, SeedPlot interfaces
    │       ├── seed.ts        # Apply active seed story to Neo4j via MemoryClient
    │       └── magic-awakening.toml  # Default seed story (TOML format)
    ├── shared/
    │   ├── events.ts          # SSE event type definitions (typed event map)
    │   ├── sse.ts             # Shared SSE stream parser (async generator)
    │   ├── colors.ts          # VOICE_COLORS: 12 inner-voice → hex color map
    │   └── constants.ts       # SKILL_NAMES, TOOL_NAMES, SEGMENT_LABELS, etc.
    └── types/
        └── dialogue.ts        # Message, DialogueOption, NotificationType
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
│      ← 9 Neo4j-backed tools from llm/tools/          │
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

| Event                | Direction       | Payload                                                                       | Trigger                                           |
|----------------------|-----------------|-------------------------------------------------------------------------------|---------------------------------------------------|
| `step_start`         | Server → Client | `{ stepId }`                                                                  | Turn begins                                       |
| `streaming_messages` | Server → Client | `{ messages }`                                                                | Progressive during `generateDialogueStep`         |
| `streaming_reset`    | Server → Client | `{}`                                                                          | LLM retried — discard previous                    |
| `time_update`        | Server → Client | `{ day, segment, segmentsAdvanced }`                                          | `advanceTime` tool executes                       |
| `options`            | Server → Client | `{ options }`                                                                 | Options available mid-stream                      |
| `parsed`             | Server → Client | `{ messages, options }`                                                       | Final structured output                           |
| `error`              | Server → Client | `{ message }`                                                                 | Error during generation                           |
| `done`               | Server → Client | `{}`                                                                          | Turn complete                                     |
| `roll_result`        | Server → Client | `{ skill, difficulty, dice[], total, statBonus, success, matchedConditions }` | Skill check resolved server-side before GM prompt |

---

## 6. LLM Tools

Two layers of tools, all defined in `src/server/llm/tools/`:

**Chorus tools**:

| Tool                   | Purpose                                                                                                        | SSE Event                                 |
|------------------------|----------------------------------------------------------------------------------------------------------------|-------------------------------------------|
| `generateDialogueStep` | Produce narrative messages + player options; supports `isCorrection` flag for targeted retries with auto-merge | `streaming_messages`, `options`, `parsed` |
| `advanceTime`          | Advance in-game clock by N segments                                                                            | `time_update`                             |

**Neo4j-backed GM tools**:

| Tool           | Purpose                                                                                           |
|----------------|---------------------------------------------------------------------------------------------------|
| `queryWorld`   | Read-only Cypher queries, confined to allowed labels via CypherValidator. Supports `instruction` + `rawResult` for local LLM formatting of results via llama-server. |
| `mutateWorld`  | Write Cypher queries, confined to allowed labels + relationships                                  |
| `manageSchema` | Register/unregister node types (with property schemas) and relationship types (with descriptions) |
| `searchWorld`  | Vector search across entities and messages                                                        |
| `editNote`     | Create/update/delete GM notes with vector embedding                                               |
| `searchNotes`  | Vector search across notes                                                                        |
| `editPlot`     | Plot lifecycle management (beats, branches, flags)                                                |
| `searchPlots`  | Vector search across plots                                                                        |

All 11 tools are defined as AI SDK `tool()` definitions and registered in `generateTurn()` via the `allTools` object. `generateDialogueStep` supports an `isCorrection` flag that auto-merges corrections with previously stored valid content — the LLM only sends failing items with their index and the tool patches them into the stored base. Skill checks are resolved server-side (not a tool) — the result is injected into the GM's prompt.

---

## 7. API Endpoints

| Method | Path                | Purpose                                                                      |
|--------|---------------------|------------------------------------------------------------------------------|
| `POST` | `/api/chat/stream`  | Primary AI turn (SSE streaming)                                              |
| `GET`  | `/api/history`      | Full conversation history from ShortTermMemory                               |
| `GET`  | `/api/game/current` | Current dialogue options from `:Conversation` node                           |
| `GET`  | `/api/debug/dump`   | Full world state (entities, plots, notes, dispositions, time, relationships) |
| `POST` | `/api/reset`        | Clear Neo4j and re-seed                                                      |
| `GET`  | `/api/debug/search/world` | Vector search across messages + entities (params: `query`, `types`, `limit`, `threshold`) |
| `GET`  | `/api/debug/search/plots` | Vector search across plots (params: `query`, `limit`, `threshold`)           |
| `GET`  | `/api/debug/search/notes` | Vector search across notes (params: `query`, `limit`, `threshold`)           |

---

## 8. Game Time

Each in-game day is divided into 12 segments of 2 hours each (segment 0 = midnight–2am, segment 11 = 10pm–midnight). Time only advances when the GM calls `advanceTime`.

**Storage**: `:TimeAnchor {id: "anchor"}` node linked via `CURRENT_TIMEPOINT` to a chain of `:TimePoint` nodes linked by `NEXT_TIMEPOINT`. Each `TimePoint` stores `day`, `segment`, and `label`. A legacy `:GameTime` node is migrated automatically on startup. Defaults to day 1, segment 2 (dawn) unless the seed story overrides it.

**Model functions** (`src/server/models/time.ts`):

- `getGameTime()` — read current time from Neo4j (falls back to legacy `:GameTime` node)
- `advanceGameTime(segments)` — adds segments (wraps days at 12), creates a new `:TimePoint` and advances the `CURRENT_TIMEPOINT` pointer
- `describeTime(time)` — human-readable string: "Day 3, Dawn (~4am-6am)"
- `migrateToTimePoints(defaultDay, defaultSegment)` — one-time migration from legacy `:GameTime` to `:TimeAnchor`/`:TimePoint` system
- `SEGMENT_LABELS` — constant map: `{ 0: "Midnight", 1: "Late Night", 2: "Dawn", ... }`

---

## 9. Memory Architecture

The memory layer (`src/server/memory/`) provides a Neo4j-backed persistent world model. All subsystems are wired through the `MemoryClient` singleton (`client.ts`), which owns the Neo4j connection, creates the schema, and initializes the embedder.

### 9.1 MemoryClient (Facade + Singleton)

`MemoryClient` (`client.ts`) is the single entry point to all memory subsystems. It composes six subsystems and exposes them as readonly properties:

```
MemoryClient.getCachedInstance()
  .neo4j       → Neo4jClient          (driver wrapper)
  .shortTerm   → ShortTermMemory      (conversation + messages)
  .longTerm    → LongTermMemory       (entities, relationships, dispositions, conditions)
  .search      → MemorySearch         (parallel vector search across layers)
  .notes       → Notes                (GM note CRUD with vector embedding)
  .plots       → Plots                (plot lifecycle: beats, branches, flags)
```

Boot sequence: `getInstance()` → creates `Neo4jClient` → `verifyConnectivity()` → `getEmbedder()` → `setupSchema()` → constructs all subsystems.

### 9.2 Type System

All memory types are defined in `types.ts` (type-only):

| Type                 | Key Fields                                                                                                             | Neo4j Node                  |
|----------------------|------------------------------------------------------------------------------------------------------------------------|-----------------------------|
| `MemoryEntity`       | id, name, type (COLE+O — CHARACTER replaces PERSON), subtype?, description?, aliases[], metadata, _embedding[], isNew? | `:Entity`                   |
| `MemoryMessage`      | id, role (user/assistant/system), content, metadata, _embedding[], createdAt                                           | `:Message`                  |
| `EntityRelationship` | id, sourceId, targetId, type, description?, confidence                                                                 | (dynamic relationship)      |
| `NPCDisposition`     | id, npcName, targetName, sentiment, summary, createdAt, updatedAt                                                      | `:NPCDisposition`           |
| `PlayerCondition`    | description, effects[] (stat/modifier pairs), duration?, source?                                                       | (stored in Entity metadata) |
| `MemoryNote`         | id, content, _embedding[], createdAt, updatedAt                                                                        | `:Note`                     |
| `MemoryPlot`         | id, name, description, status, triggerCondition?, flags[], _embedding[], createdAt, updatedAt                          | `:Plot`                     |
| `PlotFlag`           | flagId, description                                                                                                    | (stored in Plot.flags JSON) |

Types for cross-layer data flow: `SearchResults` (`messages[]` and `entities[]` arrays with `similarity`). `PlotStatus` is a union: `"PENDING" | "ACTIVE" | "IN_PROGRESS" | "COMPLETED" | "ABANDONED"`.

### 9.3 Neo4j Schema

Managed by `schema.ts`, called once at startup:

**Unique constraints (7):** `_id` on `:Conversation`, `:Message`, `:Entity`, `:Note`, `:Plot`, `:TimePoint`; `session_id` on `:IdCounter`

**Regular indexes (5):** `Message.timestamp`, `Entity.type`, `Entity.name`, `Plot.name`, `Plot.status`

**Composite indexes (3):** `TimePoint(day, segment)`, `NPCDisposition(npc_name, target_name)`, `NPCDisposition(target_name)` — the NPCDisposition indexes are wrapped in try/catch for Neo4j version compat

**Vector indexes (4, require Neo4j 5.11+, COSINE similarity):**

| Index                   | Label   | Property   | Dims                       |
|-------------------------|---------|------------|----------------------------|
| `message_embedding_idx` | Message | _embedding | 384 (or API embedder dims) |
| `entity_embedding_idx`  | Entity  | _embedding | 384 (or API embedder dims) |
| `note_embedding_idx`    | Note    | _embedding | 384 (or API embedder dims) |
| `plot_embedding_idx`    | Plot    | _embedding | 384 (or API embedder dims) |

Vector dimensions are passed from the llama-server embedder at startup (`embedder.dimensions`).

**Relationship types:**

| Type                | Direction                         | Purpose                          |
|---------------------|-----------------------------------|----------------------------------|
| `HAS_MESSAGE`       | `(Conversation)→(Message)`        | Conversation membership          |
| `FIRST_MESSAGE`     | `(Conversation)→(Message)`        | Head pointer for ordered list    |
| `NEXT_MESSAGE`      | `(Message)→(Message)`             | Sequential linked list           |
| `HAS_DISPOSITION`   | `(Entity)→(NPCDisposition)`       | NPC attitude toward a target     |
| `LOCATED_AT`        | `(Entity)→(Location)`             | Spatial placement                |
| `LOCATED_IN`        | `(Entity)→(Location)`             | Container hierarchy              |
| `CARRIES`           | `(Entity)→(Object)`               | Inventory                        |
| `ALLIED_WITH`       | `(Entity)→(Entity)`               | Alliance (dynamic)               |
| `HOSTILE_TOWARDS`   | `(Entity)→(Entity)`               | Hostility (dynamic)              |
| `BRANCHES_TO`       | `(Plot)→(Plot)`                   | Plot branching                   |
| `ABOUT_ENTITY`      | `(Note)→(Entity)`                 | Note-to-entity linkage           |
| `ABOUT_MESSAGE`     | `(Note)→(Message)`                | Note-to-message linkage          |
| `CURRENT_TIMEPOINT` | `(TimeAnchor)→(TimePoint)`        | Current game time pointer        |
| `NEXT_TIMEPOINT`    | `(TimePoint)→(TimePoint)`         | TimePoint sequential chain       |
| `AT_TIME`           | `(Message)→(TimePoint)`           | Message's in-game timestamp      |
| `STARTED_AT`        | `(Plot)→(TimePoint)`              | Plot start time                  |
| `ACTIVE_AT`         | `(Plot)→(TimePoint)`              | Plot activation time             |
| `COMPLETED_AT`      | `(Plot)→(TimePoint)`              | Plot completion time             |
| `_HAS_GM_MESSAGE`   | `(Conversation)→(GMTurnMessage)`  | GM message persistence           |
| `_FIRST_GM_MESSAGE` | `(Conversation)→(GMTurnMessage)`  | Head pointer for GM message list |
| `_NEXT_GM_MESSAGE`  | `(GMTurnMessage)→(GMTurnMessage)` | Sequential GM message list       |

Dynamic relationships (`LOCATED_AT`, `CARRIES`, `ALLIED_WITH`, `HOSTILE_TOWARDS`, `LOCATED_IN`) are created by `mutateWorld` via `longTerm.addRelationship()` with sanitized type names.

**Centralized relationship creation:** `Neo4jClient` provides two helpers that all subsystems use to create relationships between existing nodes. Both set `created_at` on the relationship:

- **`createRelationship(srcLabel, srcKey, srcVal, tgtLabel, tgtKey, tgtVal, relType)`** — CREATE with `created_at`
- **`mergeRelationship(srcLabel, srcKey, srcVal, tgtLabel, tgtKey, tgtVal, relType, opts?)`** — MERGE with `ON CREATE SET created_at` plus optional `onCreateProps` (e.g. `confidence`)

For relationships created inline with node creation (e.g. `HAS_MESSAGE` alongside a new `:Message`), the `created_at` property is set directly in the Cypher rather than using the helpers. Relationship type descriptions are stored as `:RelationshipType` nodes, not on relationship instances — see below.

**:RelationshipType nodes:** Relationship type descriptions are stored as dedicated `:RelationshipType` nodes in Neo4j with properties `name` (the type name, e.g. `LOCATED_AT`), `description` (human-readable meaning), `category` (`INTERNAL`, `PREDEFINED`, or `GM_DEFINED`), `source_labels` (JSON array of allowed source node labels), and `target_labels` (JSON array of allowed target node labels). These are synced from the `RelationshipManager` singleton on every server startup (before the seed guard, so the sync always runs) and after `/api/reset` (after clearing and re-seeding). The GM can query `:RelationshipType` nodes via `queryWorld` to discover available relationship types, their meanings, and which node types can sit on each end.

**Relationship type governance:** `relationshipManager.ts` provides a `RelationshipManager` singleton — the single source of truth for all relationship types. Types are categorized as `INTERNAL` (system bookkeeping, GM write-blocked), `PREDEFINED` (world-modeling, GM write-allowed), or `GM_DEFINED` (declared via `manageSchema` or seed story TOML). Each type stores optional `sourceLabels` and `targetLabels` arrays that confine which node labels can sit on each end. The `CypherValidator` queries the manager instead of a hardcoded allowlist. New relationship types can be declared per seed story via `[[relationshipTypes]]` in the TOML, or at runtime via the `manageSchema` tool (which accepts `sourceLabels`/`targetLabels` for endpoint constraints). The `RelationshipManager` provides `syncToNeo4j(client)` to persist all types as `:RelationshipType` nodes, `updateDefinition(name, updates)` to update description/sourceLabels/targetLabels on GM_DEFINED types, `unregister(name)` to remove GM_DEFINED types, and `reset()` to clear GM_DEFINED types (called on `/api/reset`).

**Node type governance:** `nodeManager.ts` provides a `NodeManager` singleton that mirrors `RelationshipManager` for node labels. Each node type stores a `name`, `description`, optional property schema (`{name, description, type}[]`), and `category` (`INTERNAL`, `PREDEFINED`, or `GM_DEFINED`). INTERNAL types (`Conversation`, `GMTurnMessage`, `IdCounter`) are hidden from GM tools entirely. PREDEFINED types (`Entity`, `Message`, `Note`, `Plot`, `NPCDisposition`, `TimePoint`, `TimeAnchor`, `GameTime`) are readable and writable. `RelationshipType` and `NodeType` are readable but write-blocked (the GM uses `manageSchema` instead). Node types are synced to Neo4j as `:NodeType` nodes so the GM can discover available node types and their property schemas via `queryWorld`. The `CypherValidator` uses `NodeManager` instead of the previously hardcoded `READ_ALLOWED_LABELS` / `WRITE_ALLOWED_LABELS` sets.

### 9.4 Embeddings

`embedder.ts` provides an `Embedder` interface backed by llama-server.

| Variable | Purpose | Default |
|---|---|---|
| `LLAMA_EMBED_URL` | llama-server embeddings endpoint | `http://localhost:8080/v1/embeddings` |
| `LLAMA_FORMATTER_URL` | llama-server chat completions endpoint (for `queryWorld` result formatting) | `http://localhost:8082/v1/chat/completions` |
| `LLAMA_FORMATTER_MODEL` | Model name for formatter requests | `phi-4-mini-instruct` |
| `EMBEDDING_DIMENSIONS` | Vector dimension | `1024` |

**Factory**: `getEmbedder()` returns a singleton. Used by `ShortTermMemory`, `LongTermMemory`, `Notes`, and `Plots` for vector search indexing.

### 9.5 Reranker (Two-Stage Retrieval)

`reranker.ts` provides optional post-processing for vector search results using a cross-encoder model. When configured, all search functions (`searchWorld`, `searchPlots`, `searchNotes`) automatically use two-stage retrieval:

1. **Retrieve**: Vector search with relaxed threshold (0.4 instead of 0.7) fetches more candidates (3× limit, min 30)
2. **Rerank**: Each (query, document) pair is scored by a cross-encoder, producing precise relevance scores
3. **Filter**: Results sorted by relevance, trimmed to the requested limit

**Configuration** (env vars):

| Variable | Purpose | Default |
|---|---|---|
| `LLAMA_RERANK_URL` | llama-server reranking endpoint | (disabled if unset) |

**API protocol**: The `HttpReranker` sends `{ query, documents, top_n }` and expects `{ results: [{ index, relevance_score }] }` — compatible with llama-server, Jina, and Cohere rerank endpoints.

The debug search endpoints accept `?rerank=true` to force reranking for testing. Without the flag, reranking is automatic when the reranker is configured.

### 9.6 ShortTermMemory

`shortTerm.ts`. Manages conversation history as an ordered linked list of `:Message` nodes under a singleton `:Conversation` node (keyed by `session_id: "chorus-game"`).

| Method                   | Behavior                                                                                          |
|--------------------------|---------------------------------------------------------------------------------------------------|
| `addMessage()`           | Creates `:Message`, links via `HAS_MESSAGE` + `NEXT_MESSAGE` + `FIRST_MESSAGE`, optionally embeds |
| `getConversation(limit)` | Returns messages ordered oldest-first (reverse of timestamp sort)                                 |
| `searchMessages(query)`  | Vector similarity search on `message_embedding_idx`                                               |

Message linking algorithm: find the last message (no outgoing `NEXT_MESSAGE`), create `(prev)-[:NEXT_MESSAGE]→(new)`. First message also gets `(conv)-[:FIRST_MESSAGE]→(msg)`.

### 9.7 LongTermMemory

`longTerm.ts`. Persistent world state — manages entities, relationships, NPC dispositions, player conditions, and player stats.

**Entity operations (COLE+O — CHARACTER replaces PERSON):**
- `addEntity(name, type, options?)` — MERGE on name, supports `"TYPE:SUBTYPE"` syntax. Applies dynamic Neo4j labels via PascalCase (`:Entity:Character`). Stores aliases inside metadata JSON. Returns `MemoryEntity` with `isNew` flag.
- `getEntity(name, type?)` — lookup by name with optional type filter
- `searchEntities(query, options?)` — vector search on `entity_embedding_idx` with configurable `entityTypes` filter, `limit`, and `threshold`

**Relationships:** `addRelationship(sourceName, targetName, type, options?)` — MERGE dynamic relationship `(a)-[r:${safeType}]→(b)`. Type name is sanitized to `[A-Za-z0-9_]`. Returns `{ created: boolean }`.

**NPC dispositions:**
- `setDisposition(npcName, targetName, sentiment, summary)` — MERGE `:NPCDisposition` node linked via `HAS_DISPOSITION` from NPC entity. Composite key on `(npcName, targetName)`.
- `getDisposition(npcName, targetName)` — lookup a single disposition
- `getDispositionsToward(targetName)` — all dispositions toward a target, ordered by most recently updated

**Player conditions:** `updatePlayerCondition(playerName, conditionId, condition | null)` — reads/writes the `conditions` dict inside the player entity's `metadata` JSON. Pass `null` to remove a condition.

**Player stats:** `getPlayerStats(playerName?)` — reads `metadata.stats` from player entity (defaults to `"Player"`).

### 9.8 Notes

`notes.ts`. GM note CRUD with vector embedding for semantic recall.

| Method                          | Behavior                                                    |
|---------------------------------|-------------------------------------------------------------|
| `createNote(noteName, content)` | CREATE `:Note` with name, content embedding, timestamps     |
| `getNote(noteName)`             | Read a single note by name, returns `null` if not found     |
| `updateNote(id, opts)`          | MATCH by name, SET content + re-embed if changed            |
| `deleteNote(noteName)`          | MATCH by name, DETACH DELETE                                |
| `searchNotes(query, opts)`      | Vector similarity search on `note_embedding_idx`            |
| `getAllNotes()`                 | Return all `:Note` nodes ordered by updatedAt               |
| `linkToEntity(id, name)`        | Create `[:ABOUT_ENTITY]` relationship from Note to Entity   |
| `linkToMessage(id, msgId)`      | Create `[:ABOUT_MESSAGE]` relationship from Note to Message |
| `clearLinks(noteName, type)`    | Delete `[:ABOUT_ENTITY]`/`[:ABOUT_MESSAGE]` relationships   |
| `getLinkedEntities(id)`         | Return entity names linked via `[:ABOUT_ENTITY]`            |
| `getLinkedMessages(id)`         | Return message IDs linked via `[:ABOUT_MESSAGE]`            |

### 9.9 Plots

`plots.ts`. Plot lifecycle management — beats, branches, and player flags.

| Method                        | Behavior                                                  |
|-------------------------------|-----------------------------------------------------------|
| `createPlot(name, opts)`      | CREATE `:Plot` with UUID, description embedding, status   |
| `getPlot(name)`               | Read a single plot by name, returns `null` if not found   |
| `updatePlot(name, opts)`      | Update description, status, or trigger condition          |
| `deletePlot(name)`            | MATCH by name, DETACH DELETE                              |
| `searchPlots(query, opts)`    | Vector similarity search on `plot_embedding_idx`          |
| `getAllPlots()`               | Return all `:Plot` nodes ordered by updatedAt             |
| `setFlag(plot, flagId, desc)` | Add or update a flag (by flagId) in the plot's flags JSON |
| `removeFlag(plot, flagId)`    | Remove a flag by flagId                                   |
| `getFlags(plotName)`          | Return all flags for a plot                               |
| `branchTo(parent, child)`     | Create `[:BRANCHES_TO]` relationship between two plots    |
| `unbranch(parent, child)`     | Delete the `[:BRANCHES_TO]` relationship                  |
| `getChildPlots(plotName)`     | Return plots connected via outbound `[:BRANCHES_TO]`      |

### 9.10 CypherValidator

`validation.ts`. Confines GM Cypher queries to allowed labels via hardcoded sets and validates relationship types through the `RelationshipManager` singleton.

| Method            | Behavior                                                                                                                                                                                                                                 |
|-------------------|------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------|
| `validateRead()`  | Blocks write clauses, DDL, unbounded paths. Checks labels against `READ_ALLOWED_LABELS`.                                                                                                                                                 |
| `validateWrite()` | Blocks DDL, enforces qualified MATCH before DELETE. Checks labels against `WRITE_ALLOWED_LABELS`. Queries `RelationshipManager.isAllowedForWrite()` for relationship type validation. Unknown types are auto-registered as `GM_DEFINED`. |

**Label allowlists** (private module-level constants):

| Constant               | Members                                                                                          |
|------------------------|--------------------------------------------------------------------------------------------------|
| `READ_ALLOWED_LABELS`  | `Entity`, `Message`, `NPCDisposition`, `GameTime`, `TimePoint`, `TimeAnchor`, `RelationshipType` |
| `WRITE_ALLOWED_LABELS` | `Entity`, `Message`, `NPCDisposition`, `GameTime`, `TimePoint`, `TimeAnchor`                     |

**Relationship types** are governed by `RelationshipManager` (see §9.3), not a hardcoded allowlist. The manager categorizes types as `INTERNAL` (write-blocked), `PREDEFINED`, or `GM_DEFINED` (write-allowed).

Additional validation rules: `validateWrite` requires DELETE/DETACH DELETE to be preceded by a qualified MATCH (with WHERE or property condition). Unbounded variable-length paths (`(*)`) are blocked. DDL statements (CREATE/DROP INDEX, ALTER, etc.) are blocked in both read and write validation.

### 9.11 MemorySearch

`search.ts`. Parallel hybrid search facade across memory layers.

```
search(query, { memoryTypes: ["messages", "entities"], limit: 10, threshold: 0.7 })
  ├── shortTerm.searchMessages(query)     → vector similarity (if "messages" in types)
  └── longTerm.searchEntities(query)       → vector similarity (if "entities" in types)
```

All selected searches run in parallel via `Promise.all`. Returns `SearchResults` with `messages` and `entities` arrays, each item bearing a `similarity` score.

### 9.12 Game State Persistence

`gameState.ts`. Save/resume support by persisting dialogue options as JSON on the `:Conversation` node.

- `saveCurrentOptions(options)` — writes `options` JSON to `(c:Conversation {session_id: "chorus-game"})`
- `getCurrentOptions()` — reads options back on resume

The Neo4j database is the authoritative world state — there is no separate session concept.

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
  │     ├─► queryWorld ──► CypherValidator.validateRead → Neo4j
  │     ├─► mutateWorld ──► CypherValidator.validateWrite → longTerm.*
  │     ├─► searchWorld ──► client.search.search()
  │     ├─► editNote / searchNotes ──► client.notes.*
  │     ├─► editPlot / searchPlots ──► client.plots.*
  │     ├─► advanceTime ──► models/time.ts (Neo4j write)
  │     └─► generateDialogueStep ──► SSE + persist messages (supports isCorrection flag for targeted retries)
  │
  ├─► saveCurrentOptions(finalOptions) ──► Conversation node
  └─► saveGMMessages(response.messages) ──► :GMTurnMessage nodes
```

---

## 10. Internal Voices (Inner Skills)

Fantasy-steampunk inner monologue — each skill is a distinct voice in the player's mind. Voices: `LOGIC`, `RHETORIC`, `EMPATHY`, `PERCEPTION`, `VOLITION`, `ENDURANCE`, `SORCERY`, `SUGGESTION`, `INSTINCT`, `MIGHT`, `CLOCKWORK`, `ALCHEMY`.

The system prompt in `src/server/llm/prompt.ts` instructs the LLM about voice personalities and includes the active plot tree.

The system prompt uses `DEFAULT_SYSTEM_PROMPT_TEMPLATE` from `src/server/llm/prompt.ts` with `{{setting_description}}`, `{{tone_description}}`, `{{game_time}}` variables replaced by `buildSystemPrompt()`. Setting and tone come from the active seed story. World state is **not** dumped into the prompt — the GM fetches it on demand via tools.

### Skill Checks

- **White Checks**: Repeatable after stat increases
- **Skill Checks**: Probabilistic rolls (`2d6 + Stat >= Difficulty`), resolved server-side automatically when a player selects a checked option
- **Formula**: `2d6 + Stat >= Difficulty`
- **Narrative**: The roll result is injected into the GM's prompt under "SKILL CHECK RESULT"; the GM narrates the outcome
- **Conditional outcomes**: The `conditions` array on a check can define custom success/failure labels via JS expression evaluation

---

## 11. Seed Story System

Seed data (entities, locations, characters, root plot, initial time, initial scene) is organized into pluggable seed story modules under `src/server/seed-stories/`. Each module exports a `SeedStory` object conforming to the interface in `types.ts`.

The active story is determined by the `ACTIVE_SEED_STORY` constant in `index.ts`. `getActiveSeedStory()` returns the active story's data, and `seedDatabase()` in `seed.ts` reads from it to populate Neo4j on startup.

Seed stories can optionally declare relationship types via `[[relationshipTypes]]` in the TOML. These are registered with the `RelationshipManager` as `GM_DEFINED` before relationship instances are created, so new relationship types (e.g., `CONNECTED_TO`) don't require TypeScript changes.

`seedDatabase()` checks for existing `:Entity` nodes before seeding — if any exist, it skips injection. This prevents duplicate data on server restart. On `/api/reset`, the database is cleared via `MATCH (n) DETACH DELETE n` and then re-seeded, which works because the clear brings the entity count to zero. `createPlot` uses `MERGE` (not `CREATE`) so that plot nodes are also idempotent.

**To add a new seed story:**
1. Create a new file in `src/server/seed-stories/` exporting a `SeedStory` object
2. Optionally declare custom relationship types via `[[relationshipTypes]]`
3. Register it in the `STORIES` map in `index.ts`
4. Change `ACTIVE_SEED_STORY` to the new story ID

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
2. **Tools statically defined** — all 11 tools (2 Chorus + 9 Neo4j-backed) registered in `generateTurn()`; no dynamic discovery
3. **LLM text output silently discarded** — the system prompt instructs tool-only output; text deltas are ignored
4. **No static dialogue** — all narrative is AI-generated
5. **Shared event types** — `src/shared/events.ts` ensures backend/console event contracts match
6. **Lightweight console client** — standalone REPL with chalk rendering; no entity editor or debug panel
7. **SSE progressive streaming** — `generateDialogueStep` streams messages/options incrementally via partial JSON parsing
8. **Singleton MemoryClient** — single entry point to all memory subsystems, lazy-init with caching
9. **COLE+O entity model** (variant of POLE+O — CHARACTER replaces PERSON) — entities have a type (CHARACTER/OBJECT/LOCATION/ORGANIZATION/EVENT) with dynamic Neo4j labels for efficient graph traversal
10. **Skill checks resolved server-side** — Dice rolls are computed automatically when a player selects a checked option; the result is injected into the GM's prompt for narrative integration
11. **`_` prefix = hidden property** — any Neo4j node/relationship property starting with `_` (e.g. `_embedding`) is internal and must never be exposed to the LLM. `stripHiddenProperties()` in `neo4j.ts` recursively strips `_`-prefixed keys. Applied at GM tool boundaries (`queryWorld`, `searchWorld`). Also auto-hides `_elementId`, `_labels`, `_type`, etc. injected by `unwrapRecord`.
12. **Neo4j properties use snake_case** — all node/relationship property names in Neo4j use `snake_case` (`created_at`, `trigger_condition`, `npc_name`, `target_name`). TypeScript interfaces use camelCase (`createdAt`, `triggerCondition`, `npcName`, `targetName`) — parsers map between them.
13. **GM message history persisted** — AI SDK messages (user prompts, assistant tool calls, tool results) are stored as `:GMTurnMessage` Neo4j nodes and passed to subsequent `streamText()` calls, giving the GM full context of its previous actions. `:GMTurnMessage` is excluded from CypherValidator allowlists, so the GM cannot see these nodes via its own tools.
14. **RelationshipManager governs relationship types** — a singleton registry (`relationshipManager.ts`) replaces the hardcoded `ALLOWED_RELATIONSHIPS` set. Types are categorized as `INTERNAL` (system bookkeeping, GM write-blocked), `PREDEFINED` (world-modeling, GM write-allowed), or `GM_DEFINED` (declared via `manageSchema` tool or TOML `[[relationshipTypes]]`). Seed stories can define new relationship types without TypeScript changes.
15. **NodeManager governs node labels** — a singleton registry (`nodeManager.ts`) mirrors `RelationshipManager` for node labels. Each node type stores a `name`, `description`, optional property schema, and `category`. INTERNAL types (`Conversation`, `GMTurnMessage`, `IdCounter`) are hidden from GM tools. `:NodeType` nodes in Neo4j let the GM discover available node types and their schemas via `queryWorld`. The `manageSchema` tool provides a structured interface for registering/unregistering GM-defined node and relationship types, replacing the previous regex-based auto-registration in `mutateWorld`.
16. **Two-stage retrieval with reranker** — vector similarity alone is a coarse signal. When `LLAMA_RERANK_URL` is configured, search functions use two-stage retrieval: relaxed vector search (threshold 0.4, 3× more candidates) followed by cross-encoder reranking. This improves precision significantly — the reranker reads query and document text together, catching semantic relationships that cosine similarity on compressed vectors misses. The reranker is optional; search degrades gracefully to single-stage when not configured.

---

## Debugging LLM Calls with DevTools

All `streamText` calls are captured to `.devtools/generations.json` via the `devToolsMiddleware()` wrapper in `src/server/llm/model.ts`.

**Data model:** One top-level entry per `streamText` call. `runs[]` = individual invocations of `generateTurn()`; `steps[]` = each tool-calling iteration within a run.

**Primary inspection tool** — `scripts/inspect-devtools.sh` (or `make inspect-generations`) renders readable LLM interactions for debugging:

```bash
# Summary of all runs (time, step count, token usage)
./scripts/inspect-devtools.sh --show-runs-summary

# Latest run, all steps (default)
./scripts/inspect-devtools.sh

# Specific run by index (0-based, negative from end)
./scripts/inspect-devtools.sh --run 0            # oldest run
./scripts/inspect-devtools.sh --run -1           # latest run

# Single step within a run
./scripts/inspect-devtools.sh --run -1 --step 5  # step 5 of latest run

# Show tool call results (from the next step's input)
./scripts/inspect-devtools.sh --run -1 --step 4 --tool-result

# Disable content truncation, use full terminal width
./scripts/inspect-devtools.sh --run -1 --step 5 --full --tool-result
```

The script displays for each step: user message context, model reasoning/thinking, text output (if any), tool calls with formatted arguments (including special rendering for `generateDialogueStep` messages and options), and token usage with cache-hit info. `--tool-result` adds a box showing each tool call's result. `--full` disables text truncation and uses wider terminal-width boxes.

**Alternative: raw jq** — for ad-hoc queries the script doesn't cover:

```bash
# Full args for a specific tool call
jq '.steps[3].output | fromjson | .toolCalls[0].input | fromjson' .devtools/generations.json

# System prompt (first step only, truncated)
jq -r '.steps[0].input | fromjson | .prompt[0].content[0:500]' .devtools/generations.json
```
