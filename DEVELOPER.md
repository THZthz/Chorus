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
│      editNode, editRelationship, getContext, resetSceneContext,      │
│      generateDialogueStep, advanceTime                               │
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
│  NPCDisposition, Note, Plot, TimeAnchor, TimePoint, GMTurnMessage,   │
│  RelationshipType, NodeType, IdCounter                               │
│                                                                      │
│  Indexes: node property indexes, rel property indexes, composite,    │
│  vector indexes (node & relationship)                                │
└──────────────────────────────────────────────────────────────────────┘
```

---

## 3. LLM Tools

All defined in `src/server/llm/tools/`. Registered in `generateTurn()`.

### GM tools (Neo4j-backed)

| Tool                | Purpose                                                                                                                                                                                 |
|---------------------|-----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------|
| `queryWorld`        | Cypher READ/WRITE validated via CypherValidator. Auto-registers unknown relationship types on WRITE.                                                                                    |
| `searchWorld`       | Dynamic vector search across any node type or relationship type with `_embedding`. Pass `domains` (labels/types) and optional `target` (`"node"`/`"relationship"`); omit to search all. |
| `manageSchema`      | Register/unregister node types (with `properties` schema using `tags` field) and relationship types (with required `sourceLabel`/`targetLabel`).                                        |
| `editNode`          | CREATE/UPDATE/DELETE any node. Validates properties against NodeManager schema. Auto-generates embeddings.                                                                              |
| `editRelationship`  | CREATE/UPDATE/DELETE relationships. Validates endpoint labels and properties against RelationshipManager schema. Auto-generates embeddings.                                             |
| `getContext`        | Fetch scene context, character/location/object briefs, plot tree, schema dump, relationship dump.                                                                                       |
| `resetSceneContext` | Reset the scene observer.                                                                                                                                                               |

### Chorus tools

| Tool                   | Purpose                                                                                        | SSE Event                                 |
|------------------------|------------------------------------------------------------------------------------------------|-------------------------------------------|
| `generateDialogueStep` | Produce narrative messages + player options; supports `isCorrection` flag for targeted retries | `streaming_messages`, `options`, `parsed` |
| `advanceTime`          | Advance in-game clock by N segments                                                            | `time_update`                             |

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

- **Categories**: `INTERNAL` (Conversation, GMTurnMessage, IdCounter — hidden), `PREDEFINED` (Entity, Message, Note, Plot, NPCDisposition, etc.), `GM_DEFINED`.
- **Properties**: `NodePropertyDef` with `name`, `description`, `tags` (array of tags: `string`, `number`, `json`, `embedded`, `unique`, `index`, etc.).
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
│  streamText({ tools: { ...9 tools } })               │
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
| `GET`  | `/api/debug/dump`            | Full world state (markdown)        |
| `POST` | `/api/debug/tools/:toolName` | Debug: invoke any GM tool directly |
| `POST` | `/api/reset`                 | Clear Neo4j and re-seed            |

---

## 10. Memory Architecture

`MemoryClient` (`client.ts`) is the singleton facade composing all subsystems: `neo4j`, `shortTerm`, `longTerm`, `search`, `notes`, `plots`.

### Subsystems

| Module                   | Responsibility                                                                                                                                                                                                      |
|--------------------------|---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------|
| `shortTerm.ts`           | Conversation + `:Message` nodes as ordered linked list (NEXT_MESSAGE).                                                                                                                                              |
| `longTerm.ts`            | `:Entity` nodes (COLE+O: CHARACTER, OBJECT, LOCATION, ORGANIZATION, EVENT). Relationships, NPC dispositions, player conditions/stats.                                                                               |
| `notes.ts`               | `:Note` CRUD with vector embedding. Links to entities/messages via ABOUT_ENTITY/ABOUT_MESSAGE. Uses `extractSearchTexts` for rerank text.                                                                           |
| `plots.ts`               | `:Plot` lifecycle: beats, branches, flags. Uses `extractSearchTexts` for rerank text.                                                                                                                               |
| `search.ts`              | `MemorySearch`: `searchByLabel(label, query)` for nodes, `searchByRelationshipType(type, query)` for relationships. Both support optional reranking.                                                                |
| `reranker.ts`            | Optional cross-encoder reranking (LLAMA_RERANK_URL). `extractSearchTexts` uses `NodeManager.getEmbeddingText` for node text extraction.                                                                             |
| `validation.ts`          | `CypherValidator`: validates Cypher queries against `NodeManager`/`RelationshipManager`. Auto-registers unknown relationship types with empty-string wildcard sentinel.                                             |
| `nodeManager.ts`         | Node label registry. `syncToNeo4j` creates constraints, indexes, and vector indexes dynamically. Has `getEmbeddingText()` for nodes.                                                                                |
| `relationshipManager.ts` | Relationship type registry with composite `(name, sourceLabel, targetLabel)` key. `syncToNeo4j` creates property, composite, and vector indexes for relationship types. Has `getEmbeddingText()` for relationships. |
| `gameState.ts`           | Persists dialogue options as JSON on `:Conversation` node.                                                                                                                                                          |

### Neo4j Schema

**Constraints**: Unique `_id` on Conversation, Message, Entity, Note, Plot, TimePoint. Unique `name` on Plot.

**Indexes**: Regular indexes on Entity.type, Entity.name, Message.timestamp, Plot.status. Composite indexes on NPCDisposition(npc_name, target_name) and TimePoint(day, segment). All created dynamically by `syncToNeo4j`.

**Vector indexes**: One per node type with `_embedding` (Entity, Message, Note, Plot). Naming: `{label_lower}_embedding_idx`. Also created for relationship types with `_embedding`, named `rel_{type_lower}_embedding_idx`.

### Embeddings

`embedder.ts` — llama-server via `LLAMA_EMBED_URL` (default `http://localhost:8080/v1/embeddings`). Default dimensions: 1024. Embedding text is built by `NodeManager.getEmbeddingText()` (for nodes) and `RelationshipManager.getEmbeddingText()` (for relationships) from `"embedded"`-tagged properties.

---

## 11. Game Time

Each day: 12 segments of 2 hours (segment 0 = midnight, segment 11 = 10pm–midnight). Only advances via `advanceTime`.

**Storage**: `:TimeAnchor {_id: "anchor"}` → `:CURRENT_TIMEPOINT` → `:TimePoint` chain via `NEXT_TIMEPOINT`. Each TimePoint stores `day`, `segment`, `label`.

**Model** (`src/server/models/time.ts`): `getGameTime()`, `advanceGameTime(segments)`, `describeTime()`, `migrateToTimePoints()`, `SEGMENT_LABELS`.

---

## 12. Seed Story System

Stories under `src/server/seed-stories/` (TOML format, `types.ts` interface). Active story set via `ACTIVE_SEED_STORY` in `index.ts`.

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
5. **Properties use snake_case in Neo4j** — `_created_at`, `trigger_condition`, `npc_name`.
6. **Composite key for relationship types** — `(name, sourceLabel, targetLabel)` uniquely identifies a `RelationshipDef`.
7. **Dynamic vector search** — `searchWorld` queries `NodeManager` and `RelationshipManager` at runtime for node labels and relationship types with `_embedding`, not a hardcoded enum.
8. **Embedding text from schema** — `getEmbeddingText()` reads `"embedded"`-tagged properties from `NodeManager` (nodes) and `RelationshipManager` (relationships).
9. **Two-stage retrieval** — when `LLAMA_RERANK_URL` is set: relaxed vector search + cross-encoder rerank.
10. **GM message history persisted** — `:GMTurnMessage` nodes for multi-turn continuity.
11. **COLE+O entity model** — CHARACTER, OBJECT, LOCATION, ORGANIZATION, EVENT with dynamic Neo4j sub-labels.
12. **Skill checks resolved server-side** — dice rolls computed automatically, result injected into prompt.

---

## Debugging

`scripts/inspect-devtools.sh` renders LLM interactions from `.devtools/generations.json`. Supports `--run`, `--step`, `--tool-result`, `--full` flags.

`scripts/debug-endpoints.sh` provides curl examples for each GM tool endpoint.
