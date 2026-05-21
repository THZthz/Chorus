# Developer Documentation: Chorus

Architecture, core systems, and data structures of the **Chorus** application.

---

## 1. Project Overview

**Chorus** is a cinematic dialogue engine with a vertical-scrolling "thought stream" aesthetic, branching dialogue paths, and probabilistic skill checks influenced by character attributes.

- **Stack:** TypeScript, Node.js
- **Backend:** Express + Neo4j (via local `src/server/memory/` module)
- **AI:** Single-LLM Game Master (Gemini/DeepSeek via Vercel AI SDK)
- **SSE:** Server-Sent Events for real-time streaming of LLM output
- **Console client:** Standalone Node.js REPL with chalk rendering
- **Deployment:** Local-only — runs on localhost, no authentication required

### Files Overview

```
src/
├── console/                              # REPL client
│   ├── main.ts                           # Entry point, SSE listener, chalk rendering, REPL loop
│   ├── SseClient.ts                      # SSE stream parser for server events
│   └── markdown.ts                       # Terminal markdown rendering
│
├── server/
│   ├── main.ts                           # Express server entry point (port 3000)
│   ├── mcp.ts                            # MCP server entry point — exposes GM tools via stdio
│   ├── api.ts                            # Routes: /api/chat/stream, /api/history, /api/reset, /api/debug/tools/*
│   ├── nodeManager.ts                    # Node label registry: schemas, embedding text, Neo4j sync
│   ├── relationshipManager.ts            # Rel type registry: composite key (name, sourceLabel, targetLabel)
│   ├── gameState.ts                      # Persists dialogue options on :Conversation node
│   ├── idGenerator.ts                    # Monotonic integer ID generator for Neo4j
│   ├── validation.ts                     # Zod schemas for API request validation
│   │
│   ├── llm/                              # Game Master AI
│   │   ├── index.ts                      # generateTurn(): streamText orchestration, stopWhen, prepareStep
│   │   ├── prompt.ts                     # System prompt: toolbox, turn rhythm, memory, plots, dialogue rules
│   │   ├── model.ts                      # LLM provider/model selection via env vars
│   │   ├── events.ts                     # TurnEventEmitter — SSE event emission
│   │   ├── gmMessages.ts                 # Persists/loads GM turn messages for multi-turn continuity
│   │   ├── sceneContext.ts               # Builds scene context, entity briefs, plot trees, relationship dumps
│   │   ├── conditionEvaluator.ts         # Evaluates skill-check conditional expressions
│   │   ├── rollSkillCheck.ts             # performSkillCheck(): dice roll + stat bonus resolution
│   │   │
│   │   └── tools/                        # LLM tool implementations
│   │       ├── queryWorld.ts             # Cypher READ/WRITE with schema-aware validation
│   │       ├── searchWorld.ts            # Dynamic vector search across node/relationship types
│   │       ├── editNode.ts               # CREATE/UPDATE/DELETE any node
│   │       ├── editRelationship.ts       # CREATE/UPDATE/DELETE any relationship
│   │       ├── editNote.ts               # CREATE/UPDATE/DELETE GM scratchpad notes; entity/message/plot linking
│   │       ├── editPlot.ts               # CREATE/UPDATE/DELETE plots; flags, branching, status transitions
│   │       ├── manageSchema.ts           # Register/unregister node types and relationship types
│   │       ├── getContext.ts             # Fetch scene context, briefs, schema/relationship dumps
│   │       ├── generateDialogueStep.ts   # Structured narrative output + player options
│   │       ├── advanceTime.ts            # Advance in-game clock
│   │       └── shared.ts                 # wrapSafe error wrapper, extractInternalAndUnknownKeys
│   │
│   ├── memory/                           # Neo4j persistence layer
│   │   ├── client.ts                     # MemoryClient singleton facade composing all subsystems
│   │   ├── neo4j.ts                      # Neo4j driver wrapper with value normalization
│   │   ├── shortTerm.ts                  # Conversation + :Message nodes as ordered linked list
│   │   ├── notes.ts                      # :Note CRUD with embedding, entity/message linking
│   │   ├── plots.ts                      # :Plot lifecycle: beats, branches, flags, time relationships
│   │   ├── search.ts                     # Vector search by label/rel-type with optional reranking
│   │   ├── embedder.ts                   # llama-server embedding (default: localhost:8080)
│   │   ├── reranker.ts                   # Cross-encoder reranking (optional, via LLAMA_RERANK_URL)
│   │   ├── validation.ts                 # CypherValidator: schema-aware Cypher validation
│   │   ├── reset.ts                      # clearNeo4jDatabase(): DETACH DELETE all nodes
│   │   └── types.ts                      # Shared types: MemoryEntity, MemoryNote, MemoryPlot, PlotFlag, etc.
│   │
│   ├── stories/                          # World seeding
│   │   ├── index.ts                      # Active seed story selection, getActiveSeedStory()
│   │   ├── seed.ts                       # seedDatabase(): idempotent entity/relationship/plot seeding
│   │   ├── types.ts                      # TOML story format types
│   │   ├── glass-cage.toml               # Default seed story (29 entities, 35 relationships)
│   │   └── magic-awakening.toml          # Alternate seed story
│   │
│   └── models/                           # Domain models
│       ├── entity.ts                     # Entity CRUD helpers
│       ├── plot.ts                       # Plot CRUD helpers
│       └── time.ts                       # Game time model: 12 segments/day, advanceGameTime()
│
├── shared/                               # Shared constants & types
│   ├── constants.ts                      # TOOL_NAMES, SKILL_NAMES, SEGMENT_LABELS, SEGMENT_HOURS
│   ├── events.ts                         # SSE event type definitions
│   ├── sse.ts                            # SSE formatting helpers
│   └── colors.ts                         # Chalk color wrappers for console output
│
└── types/                                # Frontend types
    └── dialogue.ts                       # Message, DialogueOption interfaces

tests/
├── setup.ts                              # Global test setup: init MemoryClient + seed
├── helpers.ts                            # resetDb(), exec(), parseToolOutput()
│
├── integration/                          # Integration tests (Neo4j-backed)
│   ├── editNode.test.ts                  # editNode CRUD + validation
│   ├── editNote.test.ts                  # editNote CRUD + entity/message linking
│   ├── editPlot.test.ts                  # editPlot CRUD + flags, branching, status transitions
│   ├── editRelationship.test.ts          # editRelationship CRUD
│   ├── queryWorld.test.ts                # queryWorld Cypher execution
│   ├── searchWorld.test.ts               # searchWorld vector search
│   ├── manageSchema.test.ts              # manageSchema node/rel-type registration
│   ├── getContext.test.ts                # getContext scene context fetch
│   └── advanceTime.test.ts               # advanceTime clock progression
│
├── scenarios/                            # End-to-end scenario tests
│   ├── correction-workflow.test.ts       # generateDialogueStep correction workflow
│   ├── entity-lifecycle.test.ts          # Entity creation → update → linking → deletion
│   └── gameplay-murder-mystery.test.ts   # Full gameplay scenario
│
└── unit/                                 # Unit tests (no Neo4j)
    ├── generateDialogueStep.test.ts      # generateDialogueStep validation rules
    └── shared.test.ts                    # Shared utility tests

scripts/
├── debug-endpoints.sh                    # curl examples for each GM tool debug endpoint
├── inspect-devtools.sh                   # LLM interaction viewer from .devtools/generations.json
└── add-license-header.mjs                # License header injection for source files
```

---

## 2. Core Architecture

```
┌──────────────────────────────────────────────────────────────────────┐
│                        CONSOLE CLIENT                                │
│  src/console/main.ts  ── SSE stream ──►  chalk rendering + REPL      │
└──────────────────────────────┬───────────────────────────────────────┘
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
│      queryWorld, manageSchema, searchWorld,                          │
│      editNode, editRelationship, editNote, editPlot,                 │
│      getContext, generateDialogueStep, advanceTime                   │
│    }                                                                 │
│  })                                                                  │
│                                                                      │
│  stopWhen: generateDialogueStep passes validation                    │
│  prepareStep: nudges if GM forgets dialogue output                   │
└──────────────────────────────┬───────────────────────────────────────┘
                               │ tool calls read/write Neo4j
                               ▼
┌───────────────────────────────────────────────────────────────────────────┐
│                     MEMORY LAYER (Neo4j-backed)                           │
│  src/server/memory/client.ts  ── MemoryClient singleton                   │
│                                                                           │
│  ┌─────────────┐  ┌─────────────┐  ┌─────────────┐  ┌─────────────┐       │
│  │ ShortTerm   │  │  LongTerm   │  │   Notes     │  │   Plots     │       │
│  ├─────────────┤  ├─────────────┤  ├─────────────┤  ├─────────────┤       │
│  │ messages    │  │ entities    │  │ GM notes    │  │ beats       │       │
│  │ conversation│  │ facts       │  │ CRUD        │  │ branches    │       │
│  └──────┬──────┘  └──────┬──────┘  └──────┬──────┘  └──────┬──────┘       │
│         └────────────────┴───────┬────────┴────────────────┘              │
│                                  ▼                                        │
│                      ┌───────────────────────────┐                        │
│                      │           Search          │                        │
│                      │  searchWorld              │                        │
│                      │  searchByLabel            │                        │
│                      │  searchByRelationshipType │                        │
│                      └──────┬──────────────┬─────┘                        │
│                             │              │                              │
│                  ┌──────────┘              └──────────┐                   │
│                  ▼                                    ▼                   │
│        ┌───────────────────┐                ┌───────────────────┐         │
│        │    embedder.ts    │                │   reranker.ts     │         │
│        │ llama-server      │                │ llama-server      │         │
│        │ LLAMA_EMBED_URL   │                │ LLAMA_RERANK_URL  │         │
│        │ (query→vector)    │                │ (cross-encoder)   │         │
│        └────────┬──────────┘                └────────┬──────────┘         │
│                 │                                    │                    │
│                 └─────────────────┬──────────────────┘                    │
│                                   ▼                                       │
│  ┌─────────────────────────────────────────────────────────────────┐      │
│  │  neo4j.ts ── driver wrapper with value normalization            │      │
│  │  nodeManager.ts ── node label registry + getEmbeddingText       │      │
│  │  relationshipManager.ts ── rel type registry + getEmbeddingText │      │
│  │  validation.ts ── CypherValidator (schema-aware)                │      │
│  └─────────────────────────────────────────────────────────────────┘      │
└──────────────────────────────┬────────────────────────────────────────────┘
                               │
                               ▼
┌──────────────────────────────────────────────────────────────────────┐
│                         NEO4J DATABASE                               │
│  Node labels: Conversation, Message, Entity (+ subtype labels),      │
│  Disposition, Note, Plot, TimeAnchor, TimePoint, GMTurnMessage,   │
│  RelationshipType, NodeType, IdCounter                               │
│                                                                      │
│  Indexes: node property indexes, rel property indexes, composite,    │
│  vector indexes (node & relationship)                                │
└──────────────────────────────────────────────────────────────────────┘
```

---

## 3. LLM Tools

All defined in `src/server/llm/tools/`. Registered in `generateTurn()`.

### GM tools — SENSE (understanding the world)

| Tool                | Purpose                                                                                                                                                                                 |
|---------------------|-----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------|
| `getContext`        | Fetch scene context, character/location/object briefs, plot tree, schema dump, relationship dump. Schema dump now served from in-memory NodeManager/RelationshipManager (no Neo4j query). |
| `searchWorld`       | Dynamic vector search across any node type or relationship type with `_embedding`. Pass `domains` (labels/types) and optional `target` (`"node"`/`"relationship"`); omit to search all. |
| `queryWorld`        | Cypher READ/WRITE validated via CypherValidator. READ auto-limited to 50 rows. WRITE with MERGE/SET/DELETE.                                                                             |

### GM tools — ACT (changing the world)

| Tool                   | Purpose                                                                                                                                                             |
|------------------------|---------------------------------------------------------------------------------------------------------------------------------------------------------------------|
| `editNode`             | CREATE/UPDATE/DELETE any node. Validates properties against NodeManager schema. Auto-generates embeddings. Use for Entity and Disposition; not for Note or Plot. |
| `editRelationship`     | CREATE/UPDATE/DELETE relationships. LOCATED_AT, CARRIES, ALLIED_WITH, HOSTILE_TOWARDS, LOCATED_IN now have `description` (string, embedded) property for narrative context. |
| `manageSchema`         | Register/unregister node types and relationship types. Must be called before creating instances of new types.                                                       |
| `advanceTime`          | Advance in-game clock by hours/days. Always include reason. Stored on NEXT_TIMEPOINT.reason.                                                                        |

### GM tools — TRACK (memory & plans)

| Tool                | Purpose                                                                                                                                                |
|---------------------|--------------------------------------------------------------------------------------------------------------------------------------------------------|
| `editNote`          | CREATE/UPDATE/DELETE a GM scratchpad note. Links to entities, messages, and plots for cross-referencing to world state, timeline, and story arcs. Partial overwrite on UPDATE. |
| `editPlot`          | CREATE/UPDATE/DELETE a plot. Manages status transitions (PENDING→ACTIVE→IN_PROGRESS→COMPLETED/ABANDONED), flags, and branching. Auto-wires time relationships on status change. |

### GM tools — SPEAK (player output)

| Tool                   | Purpose                                                                                        | SSE Event                                 |
|------------------------|------------------------------------------------------------------------------------------------|-------------------------------------------|
| `generateDialogueStep` | Produce narrative messages + player options; supports `isCorrection` flag for targeted retries | `streaming_messages`, `options`, `parsed` |

---

## 4. Relationship Type Registry

`relationshipManager.ts` — singleton registry. Keyed by composite `(name, sourceLabel, targetLabel)`. Same name with different endpoint labels creates separate entries.

- **Categories**: `INTERNAL` (system, write-blocked), `PREDEFINED` (world-modeling, write-allowed), `GM_DEFINED` (user-declared).
- **Key methods**: `register(name, desc, type, sourceLabel, targetLabel, props)`, `get(name, sourceLabel, targetLabel)`, `getByName(name)`, `isAllowedForWrite(name, sourceLabel, targetLabel)`, `updateDefinition(...)`, `unregister(...)`, `getEmbeddingText(name, props)`.
- **Wildcard sentinel**: empty string `""` means unconstrained endpoint — used by `validation.ts` for auto-registered types.
- **Neo4j sync**: stored as `:RelationshipType` nodes with `source_label`/`target_label` (singular scalars). Also creates property indexes, composite indexes, and vector indexes for relationship types that have `_embedding`.
- **Property tags**: `string`, `number`, `number[]`, `json`, `embedded`, `index`, `composite_index_1`, `composite_index_2`, `composite_index_3`. (`unique` is excluded — Neo4j does not support uniqueness constraints on relationship properties.)

## 5. Node Type Registry

`nodeManager.ts` — singleton registry mirroring RelationshipManager for node labels.

- **Categories**: `INTERNAL` (Conversation, GMTurnMessage, IdCounter — hidden), `PREDEFINED` (Entity, Message, Note, Plot, Disposition, etc.), `GM_DEFINED`.
- **Properties**: `NodePropertyDef` with `name`, `description`, `tags` (array of tags: `string`, `number`, `number[]`, `json`, `embedded`, `unique`, `index`, `composite_unique_1/2/3`, `composite_index_1/2/3`).
- **`getEmbeddingText(label, props)`**: builds embedding text by concatenating all `"embedded"`-tagged property values. Used by `addEntity`, `addMessage`, `createNote`, `createPlot`, `editNode`, and the reranker.
- **Vector indexes**: created dynamically in `syncToNeo4j` for any type with `_embedding` property.
- **Embedded properties** (tag `"embedded"`): Entity.{name,description,brief}, Plot.{name,description,brief}, Note.{content}, Message.{content}.

## 6. Dynamic Vector Search

`searchWorld` discovers searchable node types and relationship types at runtime via `NodeManager` and `RelationshipManager`. Types with `_embedding` property are searchable. Subtype labels (Character, Location, Object, etc.) that share a parent's identical property schema are filtered out via fingerprint comparison, since they share the same vector index.

**Parameters**: `target` (array of `"node"`/`"relationship"`, defaults to both), `domains` (optional list of node labels or relationship types to search), `query`, `limit`.

`MemorySearch.searchByLabel(label, query)` and `MemorySearch.searchByRelationshipType(type, query)` provide generic single-type vector search. Node vector indexes are named `${label.toLowerCase()}_embedding_idx`; relationship vector indexes are named `rel_${type.toLowerCase()}_embedding_idx`.

---

## 7. Turn Lifecycle

```
POST /api/chat/stream
        │
        ▼
┌──────────────────────────────────────────────────────┐
│  generateTurn()                                      │
│  streamText({ tools: { ...10 tools } })              │
│    stopWhen: generates once + passes validation      │
│    prepareStep: nudges if GM forgets dialogue        │
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
│  State: idle → streaming → idle      │
│  Event handlers per SSE event type   │
└──────────────────────────────────────┘
```

---

## 8. SSE Events

Defined in `src/shared/events.ts`:

| Event                | Direction       | Payload                                                                       | Trigger                                   |
|----------------------|-----------------|-------------------------------------------------------------------------------|-------------------------------------------|
| `step_start`         | Server → Client | `{ stepId }`                                                                  | Turn begins                               |
| `streaming_messages` | Server → Client | `{ messages }`                                                                | Progressive during `generateDialogueStep` |
| `streaming_reset`    | Server → Client | `{}`                                                                          | LLM retried — discard previous            |
| `time_update`        | Server → Client | `{ day, segment, segmentsAdvanced }`                                          | `advanceTime` executes                    |
| `options`            | Server → Client | `{ options }`                                                                 | Options available mid-stream              |
| `parsed`             | Server → Client | `{ messages, options }`                                                       | Final structured output                   |
| `error`              | Server → Client | `{ message }`                                                                 | Error during generation                   |
| `done`               | Server → Client | `{}`                                                                          | Turn complete                             |
| `roll_result`        | Server → Client | `{ skill, difficulty, dice[], total, statBonus, success, matchedConditions }` | Skill check resolved                      |

---

## 9. API Endpoints

| Method | Path                         | Purpose                            |
|--------|------------------------------|------------------------------------|
| `POST` | `/api/chat/stream`           | Primary AI turn (SSE streaming)    |
| `GET`  | `/api/history`               | Full conversation history          |
| `GET`  | `/api/game/current`          | Current dialogue options           |
| `POST` | `/api/debug/tools/:toolName` | Debug: invoke any GM tool directly |
| `POST` | `/api/reset`                 | Clear Neo4j and re-seed            |
| `MCP`  | `src/server/mcp.ts`          | Stdio MCP server — all 10 GM tools |

---

## 10. Memory Architecture

`MemoryClient` (`client.ts`) is the singleton facade composing all subsystems: `neo4j`, `shortTerm`, `search`, `notes`, `plots`.

### Subsystems

| Module                   | Responsibility                                                                                                                                                                                                      |
|--------------------------|---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------|
| `shortTerm.ts`           | Conversation + `:Message` nodes as ordered linked list (NEXT_MESSAGE).                                                                                                                                              |
| `notes.ts`               | `:Note` CRUD with vector embedding. Links to entities/messages/plots via ABOUT_ENTITY/ABOUT_MESSAGE/ABOUT_PLOT. Uses `extractSearchTexts` for rerank text.                                                                           |
| `plots.ts`               | `:Plot` lifecycle: beats, branches, flags. Uses `extractSearchTexts` for rerank text.                                                                                                                               |
| `search.ts`              | `MemorySearch`: `searchByLabel(label, query)` for nodes, `searchByRelationshipType(type, query)` for relationships. Both support optional reranking.                                                                |
| `reranker.ts`            | Optional cross-encoder reranking (LLAMA_RERANK_URL). `extractSearchTexts` uses `NodeManager.getEmbeddingText` for node text extraction.                                                                             |
| `validation.ts`          | `CypherValidator`: validates Cypher queries against `NodeManager`/`RelationshipManager`. Auto-registers unknown relationship types with empty-string wildcard sentinel.                                             |
| `nodeManager.ts`         | Node label registry. `syncToNeo4j` creates constraints, indexes, and vector indexes dynamically. Has `getEmbeddingText()` for nodes.                                                                                |
| `relationshipManager.ts` | Relationship type registry with composite `(name, sourceLabel, targetLabel)` key. `syncToNeo4j` creates property, composite, and vector indexes for relationship types. Has `getEmbeddingText()` for relationships. |
| `gameState.ts`           | Persists dialogue options as JSON on `:Conversation` node.                                                                                                                                                          |

### Neo4j Schema

**Constraints**: Unique `_id` on Conversation, Message, Entity, Note, Plot, TimePoint. Unique `name` on Plot.

**Indexes**: Regular indexes on Entity.type, Entity.name, Message.timestamp, Plot.status. Composite indexes on Disposition(source_name, target_name) and TimePoint(day, segment). Composite unique constraints supported via `composite_unique_1/2/3` tags. All created dynamically by `syncToNeo4j`.

**Vector indexes**: One per node type with `_embedding` (Entity, Message, Note, Plot). Naming: `{label_lower}_embedding_idx`. Also created for relationship types with `_embedding`, named `rel_{type_lower}_embedding_idx`.

### Embeddings

`embedder.ts` — llama-server via `LLAMA_EMBED_URL` (default `http://localhost:8080/v1/embeddings`). Default dimensions: 1024. Embedding text is built by `NodeManager.getEmbeddingText()` (for nodes) and `RelationshipManager.getEmbeddingText()` (for relationships) from `"embedded"`-tagged properties.

---

## 11. Game Time

Minimum unit: 30 minutes (0.5 hours). Day is integer, hour is 0–23.5 in 0.5 increments. Only advances via `advanceTime`.

**Storage**: `:TimeAnchor {_id: "anchor"}` → `:CURRENT_TIMEPOINT` → `:TimePoint` chain via `NEXT_TIMEPOINT`. Each TimePoint stores `day`, `hour`, `label`. NEXT_TIMEPOINT relationship stores `reason`.

**Model** (`src/server/models/time.ts`): `getCurrentTimePoint()`, `advanceGameTime(halfHours, reason?)`, `describeTime()`, `formatHour()`.

---

## 12. Seed Story System

Stories under `src/server/stories/` (TOML format, `types.ts` interface). Active story set via `ACTIVE_SEED_STORY` in `index.ts`.

`seedDatabase()` checks for existing `:Entity` nodes before seeding (idempotent on restart). On `/api/reset`, Neo4j is cleared then re-seeded.

Relationship types declared via `[[relationshipTypes]]` with `name`, `description`, `sourceLabel`, `targetLabel`.

---

## 13. Internal Voices (Inner Skills)

12 skills: LOGIC, RHETORIC, EMPATHY, PERCEPTION, VOLITION, ENDURANCE, SORCERY, SUGGESTION, INSTINCT, MIGHT, CLOCKWORK, ALCHEMY.

### Skill Checks

- **White Checks**: Repeatable after stat increases
- **Formula**: `2d6 + Stat >= Difficulty`
- **Server-side resolution**: Roll computed automatically when player selects a checked option; result injected into GM prompt
- **Conditional outcomes**: `conditions` array with JS expression evaluation

---

## 14. Key Design Decisions

1. **World state in Neo4j** — entities, messages, plots, notes, game time all persisted.
2. **Singleton memory layer** — `MemoryClient`, `RelationshipManager`, `NodeManager`.
3. **LLM text output silently discarded** — tool-only output; text deltas ignored.
4. **`_` prefix = hidden** — `stripHiddenProperties()` strips `_`-prefixed keys at tool boundaries.
5. **Properties use snake_case in Neo4j** — `_created_at`, `trigger_condition`, `source_name`.
6. **Composite key for relationship types** — `(name, sourceLabel, targetLabel)` uniquely identifies a `RelationshipDef`.
7. **Dynamic vector search** — `searchWorld` queries `NodeManager` and `RelationshipManager` at runtime for node labels and relationship types with `_embedding`, not a hardcoded enum.
8. **Embedding text from schema** — `getEmbeddingText()` reads `"embedded"`-tagged properties from `NodeManager` (nodes) and `RelationshipManager` (relationships).
9. **Two-stage retrieval** — when `LLAMA_RERANK_URL` is set: relaxed vector search + cross-encoder rerank.
10. **GM message history persisted** — `:GMTurnMessage` nodes for multi-turn continuity.
11. **COLE+O entity model** — CHARACTER, OBJECT, LOCATION, ORGANIZATION, EVENT with dynamic Neo4j sub-labels.
12. **Skill checks resolved server-side** — dice rolls computed automatically, result injected into prompt.
13. **Compact 4-layer GM prompt** — SENSE (getContext/searchWorld/queryWorld READ) → ACT (editNode/editRelationship/manageSchema/queryWorld WRITE/advanceTime) → TRACK (editNote/editPlot) → SPEAK (generateDialogueStep). Tool descriptions carry operational detail; prompt carries the mental model.
14. **Relationship description properties** — LOCATED_AT, CARRIES, ALLIED_WITH, HOSTILE_TOWARDS, and LOCATED_IN have `description` (string, embedded) for narrative context. Vector-indexed for semantic search via searchWorld.
15. **Schema dump from memory** — `getContext SCHEMA_DUMP` reads type definitions directly from `NodeManager`/`RelationshipManager` registries (no Neo4j round-trip), presenting full property schemas with tags and descriptions.
16. **MCP server** — `src/server/mcp.ts` exposes all 10 GM tools over stdio via `@modelcontextprotocol/sdk`. Wraps each tool's `execute(args) => Promise<string>` into MCP's `{ content: [{ type: "text", text }] }`. Two factory-based tools (`generateDialogueStep`, `advanceTime`) are instantiated with MCP-appropriate options.

---

## Debugging

`scripts/inspect-devtools.sh` renders LLM interactions from `.devtools/generations.json`. Supports `--run`, `--step`, `--tool-result`, `--full` parameters.

`scripts/debug-endpoints.sh` provides curl examples for each GM tool endpoint.
