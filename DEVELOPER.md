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
│      queryWorld, mutateWorld, searchMemory, editNote,                │
│      searchNotes, editPlot, searchPlots, ← llm/tools/ (7 GM tools)   │
│      generateDialogueStep,              ← llm/tools/ (Elysian tool)  │
│      advanceTime                        ← llm/tools/ (Elysian tool)  │
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
│          ┌─────────────┐                                                  │
│          │  Search     │                                                  │
│          ├─────────────┤                                                  │
│          │  parallel   │                                                  │
│          │  vector     │                                                  │
│          └─────────────┘                                                  │
│                                                                      │
│  embedder.ts ── local ONNX (384d) or OpenAI-compatible API           │
│  neo4j.ts    ── driver wrapper with value normalization              │
│  schema.ts   ── constraints + indexes (7 unique, 6 vector)           │
└──────────────────────────────┬───────────────────────────────────────┘
                               │
                               ▼
┌──────────────────────────────────────────────────────────────────────┐
│                         NEO4J DATABASE                               │
│  Node labels: Conversation, Message, Entity, NPCDisposition,         │
│  Note, Plot, GameTime                                                │
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
    │   │   ├── conditionEvaluator.ts  # Safe expression evaluator for skill check conditions
    │   │   └── tools/
    │   │       ├── advanceTime.ts           # Advance in-game clock by segments/days
    │   │       ├── generateDialogueStep.ts  # Produce messages + options with validation
    │   │       ├── queryWorld.ts            # Read-only Cypher queries (label-confined)
    │   │       ├── mutateWorld.ts           # Write Cypher queries (label+rel-confined)
    │   │       ├── searchMemory.ts          # Vector search across entities + messages
    │   │       ├── editNote.ts              # Create/update/delete GM notes
    │   │       ├── searchNotes.ts           # Vector search across notes
    │   │       ├── editPlot.ts              # Plot lifecycle management (beats, branches, flags)
    │   │       ├── searchPlots.ts           # Vector search across plots
    │   │       ├── rollSkillCheck.ts       # Dice rolling for skill checks
    │   │       └── shared.ts               # Helpers: checkText (character filter), wrapSafe
    │   ├── memory/
    │   │   ├── client.ts      # MemoryClient singleton — wires all memory layers
    │   │   ├── types.ts       # Shared types (MemoryEntity, MemoryMessage, MemoryPlot, etc.)
    │   │   ├── neo4j.ts       # Neo4jClient — thin wrapper over neo4j-driver
    │   │   ├── schema.ts      # Index/constraint/vector index creation
    │   │   ├── embedder.ts    # Local embeddings (Xenova/ONNX) + OpenAI-compatible fallback
    │   │   ├── shortTerm.ts   # Conversation messages with sequential NEXT_MESSAGE linking
    │   │   ├── longTerm.ts    # Entities (POLE+O), preferences, facts, relationships
    │   │   ├── search.ts      # Parallel hybrid vector search across memory types
    │   │   ├── gameState.ts   # Game save/resume via options on :Conversation node
    │   │   ├── notes.ts       # GM note CRUD with vector embedding
    │   │   ├── plots.ts       # Plot lifecycle management (beats, branches, flags)
    │   │   ├── validation.ts  # Cypher query allowlist validation (labels + relationships)
    │   │   └── reset.ts       # Clear Neo4j database (MATCH (n) DETACH DELETE n)
    │   ├── models/
    │   │   └── time.ts        # Game time CRUD via Neo4j :GameTime node
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
        ├── dialogue.ts        # Message, DialogueOption, NotificationType
        └── entities.ts        # CharacterStats, Character (unused — pending character sheet system)
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
│      ← 7 Neo4j-backed tools from llm/tools/          │
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
| `roll_result`        | Server → Client | `{ skill, difficulty, dice[], total, statBonus, success, matchedConditions }` | Skill check dice rolled |

---

## 6. LLM Tools

Two layers of tools, all defined in `src/server/llm/tools/`:

**Elysian tools**:

| Tool                   | Purpose                                     | SSE Event                                 |
|------------------------|---------------------------------------------|-------------------------------------------|
| `generateDialogueStep` | Produce narrative messages + player options | `streaming_messages`, `options`, `parsed` |
| `advanceTime`          | Advance in-game clock by N segments         | `time_update`                             |
| `rollSkillCheck`       | Roll dice for skill checks                  | `roll_result`                             |

**Neo4j-backed GM tools**:

| Tool           | Purpose                                                                  |
|----------------|--------------------------------------------------------------------------|
| `queryWorld`   | Read-only Cypher queries, confined to allowed labels via CypherValidator |
| `mutateWorld`  | Write Cypher queries, confined to allowed labels + relationships         |
| `searchMemory` | Vector search across entities and messages                               |
| `editNote`     | Create/update/delete GM notes with vector embedding                      |
| `searchNotes`  | Vector search across notes                                               |
| `editPlot`     | Plot lifecycle management (beats, branches, flags)                       |
| `searchPlots`  | Vector search across plots                                               |

All 10 tools are defined as AI SDK `tool()` definitions and registered in `generateTurn()` via the `allTools` object.

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

`MemoryClient` (`client.ts`, ~90 lines) is the single entry point to all memory subsystems. It composes six subsystems and exposes them as readonly properties:

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

All memory types are defined in `types.ts` (~130 lines, type-only):

| Type                 | Key Fields                                                                                         | Neo4j Node                  |
|----------------------|----------------------------------------------------------------------------------------------------|-----------------------------|
| `MemoryEntity`       | id, name, type (POLE+O), subtype?, description?, aliases[], metadata, embedding[], isNew?          | `:Entity`                   |
| `MemoryMessage`      | id, role (user/assistant/system), content, metadata, embedding[], createdAt                        | `:Message`                  |
| `EntityRelationship` | id, sourceId, targetId, type, description?, confidence                                             | (dynamic relationship)      |
| `NPCDisposition`     | id, npcName, targetName, sentiment, summary, createdAt, updatedAt                                  | `:NPCDisposition`           |
| `PlayerCondition`    | description, effects[] (stat/modifier pairs), duration?, source?                                   | (stored in Entity metadata) |
| `MemoryNote`         | id, content, embedding[], createdAt, updatedAt                                                     | `:Note`                     |
| `MemoryPlot`         | id, name, description, status, triggerCondition?, flags[], embedding[], createdAt, updatedAt       | `:Plot`                     |
| `PlotFlag`           | flagId, description                                                                                | (stored in Plot.flags JSON) |
Types for cross-layer data flow: `SearchResults` (`messages[]` and `entities[]` arrays with `similarity`). `PlotStatus` is a union: `"PENDING" | "ACTIVE" | "IN_PROGRESS" | "COMPLETED" | "ABANDONED"`.

### 9.3 Neo4j Schema

Managed by `schema.ts` (~85 lines), called once at startup:

**Unique constraints (5):** `id` on `:Conversation`, `:Message`, `:Entity`, `:Note`, `:Plot`

**Regular indexes (5):** `Message.timestamp`, `Entity.type`, `Entity.name`, `Plot.name`, `Plot.status`

**Composite indexes (2):** `NPCDisposition(npcName, targetName)`, `NPCDisposition(targetName)` — wrapped in try/catch for Neo4j version compat

**Vector indexes (4, require Neo4j 5.11+, COSINE similarity):**

| Index                   | Label   | Property  | Dims                       |
|-------------------------|---------|-----------|----------------------------|
| `message_embedding_idx` | Message | embedding | 384 (or API embedder dims) |
| `entity_embedding_idx`  | Entity  | embedding | 384 (or API embedder dims) |
| `note_embedding_idx`    | Note    | embedding | 384 (or API embedder dims) |
| `plot_embedding_idx`    | Plot    | embedding | 384 (or API embedder dims) |

Vector dimensions are passed from the active embedder at startup (`embedder.dimensions`), so they adapt to the configured embedding provider.

**Relationship types:**

| Type              | Direction                    | Purpose                       |
|-------------------|------------------------------|-------------------------------|
| `HAS_MESSAGE`     | `(Conversation)→(Message)`   | Conversation membership       |
| `FIRST_MESSAGE`   | `(Conversation)→(Message)`   | Head pointer for ordered list |
| `NEXT_MESSAGE`    | `(Message)→(Message)`        | Sequential linked list        |
| `HAS_DISPOSITION` | `(Entity)→(NPCDisposition)`  | NPC attitude toward a target  |
| `LOCATED_AT`      | `(Entity)→(Entity)`          | Spatial placement (dynamic)   |
| `LOCATED_IN`      | `(Entity)→(Entity)`          | Container hierarchy (dynamic) |
| `CARRIES`         | `(Entity)→(Entity)`          | Inventory (dynamic)           |
| `ALLIED_WITH`     | `(Entity)→(Entity)`          | Alliance (dynamic)            |
| `HOSTILE_TOWARDS` | `(Entity)→(Entity)`          | Hostility (dynamic)           |
| `BRANCHES_TO`     | `(Plot)→(Plot)`              | Plot branching                |
| `ABOUT`           | `(Note)→(Entity)`            | Note-to-entity linkage        |
| `ABOUT_MESSAGE`   | `(Note)→(Message)`           | Note-to-message linkage       |

Dynamic relationships (`LOCATED_AT`, `CARRIES`, `ALLIED_WITH`, `HOSTILE_TOWARDS`, `LOCATED_IN`) are created by `mutateWorld` via `longTerm.addRelationship()` with sanitized type names.

### 9.4 Embeddings

`embedder.ts` (~140 lines) provides two strategies behind an `Embedder` interface:

- **`LocalEmbedder`**: `@xenova/transformers` with `Xenova/all-MiniLM-L6-v2` (384-dim, ~80MB ONNX). Uses mean pooling, processes sequentially to avoid ONNX memory pressure.
- **`OpenAICompatibleEmbedder`**: Any OpenAI-compatible API (configurable via `EMBEDDING_API_URL`/`EMBEDDING_API_KEY`/`EMBEDDING_MODEL` env vars). Default model `text-embedding-3-small` (1536-dim).

**Strategy pattern + Factory**: `getEmbedder()` returns a singleton, preferring API if credentials are set, otherwise local ONNX. The embedder is used by `ShortTermMemory`, `LongTermMemory`, `Notes`, and `Plots` for vector search indexing.

### 9.5 ShortTermMemory

`shortTerm.ts` (~230 lines). Manages conversation history as an ordered linked list of `:Message` nodes under a singleton `:Conversation` node (keyed by `session_id: "elysian-game"`).

| Method                   | Behavior                                                                                          |
|--------------------------|---------------------------------------------------------------------------------------------------|
| `addMessage()`           | Creates `:Message`, links via `HAS_MESSAGE` + `NEXT_MESSAGE` + `FIRST_MESSAGE`, optionally embeds |
| `getConversation(limit)` | Returns messages ordered oldest-first (reverse of timestamp sort)                                 |
| `searchMessages(query)`  | Vector similarity search on `message_embedding_idx`                                               |

Message linking algorithm: find the last message (no outgoing `NEXT_MESSAGE`), create `(prev)-[:NEXT_MESSAGE]→(new)`. First message also gets `(conv)-[:FIRST_MESSAGE]→(msg)`.

### 9.6 LongTermMemory

`longTerm.ts` (~350 lines). Persistent world state — manages entities, relationships, NPC dispositions, player conditions, and player stats.

**Entity operations (POLE+O model):**
- `addEntity(name, type, options?)` — MERGE on name, supports `"TYPE:SUBTYPE"` syntax. Applies dynamic Neo4j labels via PascalCase (`:Entity:Person:Character`). Stores aliases inside metadata JSON. Returns `MemoryEntity` with `isNew` flag.
- `getEntity(name, type?)` — lookup by name with optional type filter
- `searchEntities(query, options?)` — vector search on `entity_embedding_idx` with configurable `entityTypes` filter, `limit`, and `threshold`

**Relationships:** `addRelationship(sourceName, targetName, type, options?)` — MERGE dynamic relationship `(a)-[r:${safeType}]→(b)`. Type name is sanitized to `[A-Za-z0-9_]`. Returns `{ created: boolean }`.

**NPC dispositions:**
- `setDisposition(npcName, targetName, sentiment, summary)` — MERGE `:NPCDisposition` node linked via `HAS_DISPOSITION` from NPC entity. Composite key on `(npcName, targetName)`.
- `getDisposition(npcName, targetName)` — lookup a single disposition
- `getDispositionsToward(targetName)` — all dispositions toward a target, ordered by most recently updated

**Player conditions:** `updatePlayerCondition(playerName, conditionId, condition | null)` — reads/writes the `conditions` dict inside the player entity's `metadata` JSON. Pass `null` to remove a condition.

**Player stats:** `getPlayerStats(playerName?)` — reads `metadata.stats` from player entity (defaults to `"Player"`).

### 9.7 Notes

`notes.ts`. GM note CRUD with vector embedding for semantic recall.

| Method                     | Behavior                                                    |
|----------------------------|-------------------------------------------------------------|
| `createNote(content)`      | CREATE `:Note` with UUID, content embedding, timestamps     |
| `getNote(noteId)`          | Read a single note by ID, returns `null` if not found       |
| `updateNote(id, opts)`     | MATCH by id, SET content + re-embed if changed              |
| `deleteNote(noteId)`       | MATCH by id, DETACH DELETE                                  |
| `searchNotes(query, opts)` | Vector similarity search on `note_embedding_idx`            |
| `getAllNotes()`            | Return all `:Note` nodes ordered by updatedAt               |
| `linkToEntity(id, name)`   | Create `[:ABOUT]` relationship from Note to Entity          |
| `linkToMessage(id, msgId)` | Create `[:ABOUT_MESSAGE]` relationship from Note to Message |
| `clearLinks(noteId)`       | Delete all `[:ABOUT]` and `[:ABOUT_MESSAGE]` relationships  |
| `getLinkedEntities(id)`    | Return entity names linked via `[:ABOUT]`                   |
| `getLinkedMessages(id)`    | Return message IDs linked via `[:ABOUT_MESSAGE]`            |

### 9.8 Plots

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

### 9.9 CypherValidator

`validation.ts`. Confines GM Cypher queries to allowed labels and relationship types to prevent schema abuse.

| Method            | Behavior                                                                                                                                                 |
|-------------------|----------------------------------------------------------------------------------------------------------------------------------------------------------|
| `validateRead()`  | Blocks write clauses, DDL, unbounded paths. Checks labels against `READ_ALLOWED_LABELS`.                                                                 |
| `validateWrite()` | Blocks DDL, enforces qualified MATCH before DELETE. Checks labels against `WRITE_ALLOWED_LABELS` and relationship types against `ALLOWED_RELATIONSHIPS`. |

**Allowlists** (private module-level constants):

| Constant                | Members                                                                                                                                    |
|-------------------------|--------------------------------------------------------------------------------------------------------------------------------------------|
| `READ_ALLOWED_LABELS`   | `Entity`, `Message`, `NPCDisposition`, `GameTime`                                                                                          |
| `WRITE_ALLOWED_LABELS`  | `Entity`, `Message`, `NPCDisposition`, `GameTime`                                                                                          |
| `ALLOWED_RELATIONSHIPS` | `LOCATED_AT`, `CARRIES`, `ALLIED_WITH`, `HOSTILE_TOWARDS`, `LOCATED_IN`, `HAS_DISPOSITION`, `HAS_MESSAGE`, `FIRST_MESSAGE`, `NEXT_MESSAGE` |

Additional validation rules: `validateWrite` requires DELETE/DETACH DELETE to be preceded by a qualified MATCH (with WHERE or property condition). Unbounded variable-length paths (`(*)`) are blocked. DDL statements (CREATE/DROP INDEX, ALTER, etc.) are blocked in both read and write validation.

### 9.10 MemorySearch

`search.ts` (~65 lines). Parallel hybrid search facade across memory layers.

```
search(query, { memoryTypes: ["messages", "entities"], limit: 10, threshold: 0.7 })
  ├── shortTerm.searchMessages(query)     → vector similarity (if "messages" in types)
  └── longTerm.searchEntities(query)       → vector similarity (if "entities" in types)
```

All selected searches run in parallel via `Promise.all`. Returns `SearchResults` with `messages` and `entities` arrays, each item bearing a `similarity` score.

### 9.11 Game State Persistence

`gameState.ts` (~36 lines). Save/resume support by persisting dialogue options as JSON on the `:Conversation` node.

- `saveCurrentOptions(options)` — writes `options` JSON to `(c:Conversation {session_id: "elysian-game"})`
- `getCurrentOptions()` — reads options back on resume

The Neo4j database is the authoritative world state — there is no separate session concept.

### 9.12 Data Flow Summary

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
  │     ├─► searchMemory ──► client.search.search()
  │     ├─► editNote / searchNotes ──► client.notes.*
  │     ├─► editPlot / searchPlots ──► client.plots.*
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
- **Skill Checks**: Probabilistic rolls (`2d6 + Stat >= Difficulty`), resolved via `rollSkillCheck` tool
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
2. **Tools statically defined** — all 10 tools (3 Elysian + 7 Neo4j-backed) registered in `generateTurn()`; no dynamic discovery
3. **LLM text output silently discarded** — the system prompt instructs tool-only output; text deltas are ignored
4. **No static dialogue** — all narrative is AI-generated
5. **Shared event types** — `src/shared/events.ts` ensures backend/console event contracts match
6. **Lightweight console client** — standalone REPL with chalk rendering; no entity editor or debug panel
7. **SSE progressive streaming** — `generateDialogueStep` streams messages/options incrementally via partial JSON parsing
8. **Singleton MemoryClient** — single entry point to all memory subsystems, lazy-init with caching
9. **POLE+O entity model** — entities have a type (PERSON/OBJECT/LOCATION/ORGANIZATION/EVENT) with dynamic Neo4j labels for efficient graph traversal
10. **Skill checks via LLM tool** — `rollSkillCheck` is a dedicated tool the GM calls to mechanically resolve dice rolls; results feed back into the narrative loop

---

## 14. Development Workflow

### 14.1 Adding a New Elysian Tool

1. Create a new file in `src/server/llm/tools/` following the existing pattern
2. Define the Zod input schema and `execute` function (wrap with `wrapSafe` from `shared.ts`)
3. Register it in `src/server/llm/index.ts` in the `allTools` object within `generateTurn()`
4. Update the system prompt in `src/server/llm/prompt.ts` if the LLM needs guidance

### 14.2 Adding a Neo4j-Backed Tool

Add a new tool definition in `src/server/llm/tools/` following existing patterns (see `queryWorld.ts` or `mutateWorld.ts` for examples). Wire it into the `allTools` object in `src/server/llm/index.ts`. Delegate to the appropriate memory subsystem via `MemoryClient.getCachedInstance()` (`client.longTerm.*`, `client.notes.*`, `client.plots.*`, etc.).

### 14.3 Adding a New Voice/Skill

1. Add the stat to `CharacterStats` in `src/types/entities.ts`
2. Add voice personality description to the system prompt in `src/server/llm/prompt.ts`
3. Add a color entry in `src/shared/colors.ts`'s `VOICE_COLORS` map

### 14.4 Managing Seed Data

Edit the active seed story in `src/server/seed-stories/` or create a new one. Change `ACTIVE_SEED_STORY` in `index.ts` to switch stories.

### 14.5 Debugging LLM Calls with DevTools

All `streamText` calls are captured to `.devtools/generations.json` via the `devToolsMiddleware()` wrapper in `src/server/llm/model.ts`.

**Data model:** One top-level entry per `streamText` call. `runs[]` = individual invocations of `generateTurn()`; `steps[]` = each tool-calling iteration within a run.

**Primary inspection tool** — `scripts/inspect-devtools.sh` renders readable LLM interactions for debugging:

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
```

The script displays for each step: user message context, model reasoning/thinking, text output (if any), tool calls with formatted arguments (including special rendering for `generateDialogueStep` messages and options), and token usage with cache-hit info.

**Alternative: raw jq** — for ad-hoc queries the script doesn't cover:

```bash
# Tool call results (appear in the NEXT step's input)
jq '.steps[1].input | fromjson | .prompt[] | select(.role == "tool")' .devtools/generations.json

# Full args for a specific tool call
jq '.steps[3].output | fromjson | .toolCalls[0].input | fromjson' .devtools/generations.json

# System prompt (first step only, truncated)
jq -r '.steps[0].input | fromjson | .prompt[0].content[0:500]' .devtools/generations.json
```
