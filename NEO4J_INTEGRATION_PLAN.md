# Neo4j Integration Plan

## 1. Target Architecture

```
┌─────────────────────────────────────────────────────────┐
│  Elysian Dialogue (TypeScript / Express)                │
│                                                         │
│  ┌──────────┐  ┌─────────────┐  ┌────────────────────┐ │
│  │ Console  │  │ SSE Stream  │  │ GM Turn Loop       │ │
│  │ Client   │  │ (events.ts) │  │ (llm/index.ts)     │ │
│  └──────────┘  └─────────────┘  └──────┬─────────────┘ │
│                                         │                │
│                              ┌──────────▼──────────┐    │
│                              │ Combined Tool Set    │    │
│                              │ = MCP tools (16)     │    │
│                              │ + generateDialogue   │    │
│                              │ + advanceTime        │    │
│                              └──────────┬──────────┘    │
│                                         │                │
│  ┌──────────────────────┐   ┌──────────▼──────────┐    │
│  │ SQLite (kept)        │   │ MCP Client           │    │
│  │ - llm_logs           │   │ → http://127.0.0.1    │    │
│  │ - system_state       │   │   :8080/sse          │    │
│  │ - seed story config  │   └─────────────────────┘    │
│  └──────────────────────┘                              │
└─────────────────────────────────────────────────────────┘
                              │
                              │ MCP (SSE transport)
                              ▼
┌─────────────────────────────────────────────────────────┐
│  agent-memory MCP Server (Python / FastMCP)              │
│                                                          │
│  16 tools + 4 resources + 3 prompts                      │
│                                                          │
│  ┌──────────────────────────────────────────────────┐   │
│  │  Neo4j                                           │   │
│  │                                                  │   │
│  │  (:Entity {type: PERSON})  — characters          │   │
│  │  (:Entity {type: OBJECT})  — objects             │   │
│  │  (:Entity {type: LOCATION})— locations           │   │
│  │  (:Entity {type: ORGANIZATION}) — factions       │   │
│  │  (:Entity {type: EVENT})     — plots             │   │
│  │                                                  │   │
│  │  (:Fact) — notes, time state, scene state        │   │
│  │  (:Message) — dialogue history                   │   │
│  │  (:Conversation) — game session                  │   │
│  │                                                  │   │
│  │  Relationships: LOCATED_AT, CARRIES,             │   │
│  │    HOSTILE_TOWARDS, ALLIED_WITH,                  │   │
│  │    CHILD_PLOT, INVOLVES, OCCURRED_AT...          │   │
│  └──────────────────────────────────────────────────┘   │
└─────────────────────────────────────────────────────────┘
```

## 2. What Gets Removed

### 2.1 Entire Directories

| Directory | Reason |
|-----------|--------|
| `src/client/` | Remove web client |
| `src/components/` | All React components |
| `src/services/` | SseClient.ts, WorldManager.ts — client-side only |
| `src/types/` (remodeled) | entities.ts, dialogue.ts, plot.ts — replaced by agent-memory types + minimal dialogue types |

### 2.2 Server Files Removed

| File | Replaced by |
|------|-------------|
| `src/server/models/world.ts` | Neo4j entities via MCP |
| `src/server/models/plot.ts` | Plot EVENT nodes in Neo4j |
| `src/server/models/scene.ts` | Relationships in Neo4j (LOCATED_AT, CARRIES) |
| `src/server/models/notes.ts` | Facts in Neo4j |
| `src/server/models/dialogue.ts` (tree navigation, snapshots, alternatives) | Removed — no replay |
| `src/server/models/history.ts` | Messages in Neo4j via `memory_store_message` |

### 2.3 LLM Tools Removed

These 16 tools become agent-memory MCP tools directly:

| Old Tool | agent-memory Equivalent |
|----------|------------------------|
| `listEntities` | `memory_search` or `graph_query` |
| `getEntity` | `memory_get_entity` |
| `updateEntity` | `memory_add_entity` (upsert) |
| `updateEntities` | Multiple `memory_add_entity` calls |
| `createEntity` | `memory_add_entity` |
| `getCharacterState` | `memory_get_entity` |
| `updateCharacterState` | `memory_add_entity` (metadata merge) |
| `createPlot` | `memory_add_entity` (type: EVENT) + `memory_create_relationship` |
| `updatePlot` | `memory_add_entity` (upsert EVENT) |
| `getPlot` | `memory_get_entity` / `memory_get_context` |
| `getScene` | `memory_get_context` / `graph_query` |
| `updateScene` | `memory_create_relationship` (LOCATED_AT, CARRIES) |
| `addNote` | `memory_add_fact` |
| `getNote` | `memory_search` / `graph_query` |
| `updateNote` | `memory_add_fact` (upsert) |
| `removeNote` | Not needed — facts have temporal validity |

### 2.4 Database Tables Removed

| Table | Reason |
|-------|--------|
| `entities` | Moved to Neo4j |
| `plots` | Moved to Neo4j |
| `history_messages` | Moved to Neo4j |
| `dialogue_steps` | Removed — no tree, no replay |
| `dialogue_alternatives` | Removed — no replay |
| `notes` | Moved to Neo4j |

## 3. What Stays

### 3.1 Server Core

| File | Purpose |
|------|---------|
| `src/server/main.ts` | Express entry (no Vite middleware) |
| `src/server/api.ts` | Simplified — chat stream, system prompt, reset, IDs |
| `src/server/db.ts` | SQLite for `system_state`, `llm_logs`, `llm_steps` |
| `src/server/validation.ts` | Zod schemas for remaining endpoints |
| `src/server/models/ids.ts` | Base62 ID generation |
| `src/server/models/debug.ts` | LLM log queries |
| `src/server/models/shared.ts` | `safeJsonParse` |

### 3.2 LLM Layer

| File | Modified Purpose |
|------|-----------------|
| `src/server/llm/index.ts` | Turn loop — tools come from MCP + local |
| `src/server/llm/events.ts` | SSE event emitter (unchanged) |
| `src/server/llm/model.ts` | Model selection (unchanged) |
| `src/server/llm/prompt.ts` | System prompt — simplified, references MCP tools |
| `src/server/llm/debug.ts` | LLM debug logging (unchanged) |
| `src/server/llm/persistStep.ts` | Removed — no snapshots, no tree |

### 3.3 Shared

| File | Status |
|------|--------|
| `src/shared/events.ts` | Kept, simplified (remove plot_create, plot_edit, world_update, entity_create, note_* events) |
| `src/shared/sse.ts` | Kept as-is |
| `src/shared/colors.ts` | Kept as-is |
| `src/shared/constants.ts` | Simplified — keep TOOL_NAMES (only remaining tools), SKILL_NAMES, PLAYER_ID, SEGMENT_LABELS |

### 3.4 Console Client

| File | Status |
|------|--------|
| `src/console/main.ts` | Kept, simplified |
| `src/console/SseClient.ts` | Kept as-is |

### 3.5 Seed Stories

| File | Modified Purpose |
|------|-----------------|
| `src/server/seed-stories/types.ts` | Kept |
| `src/server/seed-stories/index.ts` | Kept |
| `src/server/seed-stories/*.ts` | Rewrite seed data to write into Neo4j instead of SQLite entities table |

### 3.6 Database Tables Kept

| Table | Purpose |
|-------|---------|
| `system_state` | `gm_system_prompt`, `active_seed_story`, `game_time_day`, `game_time_segment` (time tracked here for simplicity — the `advanceTime` tool writes here) |
| `llm_logs` | Debug request/response logging |
| `llm_steps` | Per-step metrics |

## 4. New Components

### 4.1 MCP Client Bridge (`src/server/mcp/`)

```
src/server/mcp/
├── client.ts        # MCP client via @ai-sdk/mcp — connects to agent-memory at http://127.0.0.1:8080/sse
└── seed.ts          # Seed data importer — writes seed story entities/plots into Neo4j
```

**`client.ts`**: Uses `@ai-sdk/mcp`'s `createMCPClient()` with SSE transport to connect to the agent-memory MCP server. Maintains the connection for the lifetime of the Express server. Tools are auto-discovered — no manual wrapping needed.

```typescript
import { createMCPClient } from '@ai-sdk/mcp';

const mcpClient = await createMCPClient({
  transport: {
    type: 'sse',
    url: 'http://127.0.0.1:8080/sse',
  },
});

const mcpTools = await mcpClient.tools(); // auto-discovers all 16 agent-memory tools
// mcpTools is directly compatible with streamText({ tools: mcpTools })
```

**No `tools.ts` needed**: `mcpClient.tools()` returns Vercel AI SDK-compatible tools directly. The Elysian-specific tools (`generateDialogueStep`, `advanceTime`) are merged with the spread operator: `{ ...mcpTools, generateDialogueStep, advanceTime }`.

**`seed.ts`**: On first run (or reset), takes the active seed story's entities, plots, initial scene, and writes them into Neo4j via MCP tools:
- Characters → `memory_add_entity(type=PERSON, ...)`
- Locations → `memory_add_entity(type=LOCATION, ...)`
- Objects → `memory_add_entity(type=OBJECT, ...)`
- Plot root → `memory_add_entity(type=EVENT, ...)`
- Scene positions → `memory_create_relationship(LOCATED_AT/CARRIES)`
- Initial notes → `memory_add_fact(...)`

### 4.2 Elysian-Specific Tools (only 2 remain)

File: `src/server/llm/tools/generateDialogueStep.ts` — kept as-is, unchanged.
File: `src/server/llm/tools/advanceTime.ts` — moved to `src/server/llm/tools/`, writes to SQLite `system_state`.

Time is tracked in SQLite `system_state` for simplicity. The `advanceTime` tool reads/writes SQLite directly. Time state is also reflected in Neo4j as a Fact: `(session, has_current_time, "Day 3, Dawn")`.

### 4.3 Simplified System Prompt

The system prompt template shrinks significantly. Instead of listing 18 tools in detail, it references the available MCP tools and the 2 Elysian-specific tools:

```
You are the Game Master for a narrative-driven RPG.
SETTING: {{setting_description}}
TONE: {{tone_description}}

## YOUR TOOLS

You have two kinds of tools:

### Memory Tools (16 available)
These manage the game world — entities, relationships, facts, and your own memory:
- memory_search — search all world state
- memory_get_context — get relevant context for this moment
- memory_add_entity — create/update characters, locations, objects, factions, plots
- memory_get_entity — get entity details with relationships
- memory_create_relationship — link entities (LOCATED_AT, CARRIES, HOSTILE_TOWARDS, ALLIED_WITH, CHILD_PLOT...)
- memory_add_fact — record facts (notes, clues, suspicions, timeline events)
- memory_store_message — store dialogue messages
- memory_get_conversation — recall conversation history
- graph_query — execute Cypher queries for complex lookups
- ... (others as needed)

### Game Tools (2)
1. generateDialogueStep — THE ONLY WAY to communicate with the player. REQUIRED every turn.
2. advanceTime — advance the in-game clock

## Turn Order
1. If first turn or reset: seed the world with entities from seed story
2. Read context: memory_get_context() or memory_search()
3. Update world: memory_add_entity() / memory_create_relationship() / memory_add_fact()
4. Update time: advanceTime() if action takes time
5. ALWAYS end with generateDialogueStep — options should align with active plot's childPlots

[Entity types: PERSON=character, LOCATION=location, OBJECT=object,
 ORGANIZATION=faction, EVENT=plot]
[Relationship types: LOCATED_AT, CARRIES, HOSTILE_TOWARDS, ALLIED_WITH,
 CHILD_PLOT, INVOLVES, OCCURRED_AT, TRIGGERED_BY]
```

The `{{entities_brief}}`, `{{active_plots}}`, `{{current_scene}}` template variables are removed — the GM uses `memory_get_context` to get context on demand instead of having everything dumped into the prompt.

## 5. Data Model Mapping

### 5.1 Characters → PERSON Entity

```
Neo4j node:
(:Entity:Person {
  id: "veyla",
  name: "Veyla",
  type: "PERSON",
  subtype: "CHARACTER",
  description: "A young sorceress with a mysterious past...",
  metadata: JSON.stringify({
    stats: { LOGIC: 4, SORCERY: 6, ... },
    conditions: { wounded: true },
    shortDescription: "Mysterious young sorceress"
  })
})
```

### 5.2 Locations → LOCATION Entity

```
(:Entity:Location {
  id: "the_velvet_thorn",
  name: "The Velvet Thorn",
  type: "LOCATION",
  subtype: "TAVERN",
  description: "A dimly lit tavern...",
  metadata: JSON.stringify({
    shortDescription: "Dimly lit tavern in the old quarter"
  })
})
```

### 5.3 Objects → OBJECT Entity

```
(:Entity:Object {
  id: "soul_shard",
  name: "Soul Shard",
  type: "OBJECT",
  description: "A pulsing crystal that glows faintly blue...",
  metadata: JSON.stringify({
    shortDescription: "Pulsing blue crystal"
  })
})
```

### 5.4 Scene State → Relationships

Instead of a `system_state` JSON blob, scene state is the graph itself:

```cypher
// Characters at locations
(veyla)-[:LOCATED_AT]->(the_velvet_thorn)
(player)-[:LOCATED_AT]->(the_velvet_thorn)

// Objects carried by characters
(soul_shard)-[:CARRIES {carrier: "player"}]->(player)
// Or: (player)-[:CARRIES]->(soul_shard)

// Current scene location tracked via session fact
(session)-[:HAS_CURRENT_LOCATION]->(the_velvet_thorn)
```

### 5.5 Plots → EVENT Entity + CHILD_PLOT Relationships

```
(:Entity:Event {
  id: "plot_1",
  name: "The Awakening",
  type: "EVENT",
  description: "Strange magical disturbances...",
  status: "IN_PROGRESS",  // stored in metadata
  metadata: JSON.stringify({
    involvedCharacters: ["veyla", "magister_vex"],
    involvedLocations: ["the_velvet_thorn"],
    flags: { alarm_raised: true }
  })
})

// Branch structure via relationships
(plot_1)-[:CHILD_PLOT {
  triggerCondition: "Player sides with the Clockwrights' Guild",
  plotId: "plot_7"  // null until instantiated
}]->(plot_7)
```

### 5.6 Notes → Facts

```
(:Fact {
  subject: "GM",
  predicate: "suspects",
  object_value: "Madam Cressida is lying about the ledger",
  confidence: 0.8
})-[:RELATES_TO]->(:Entity {name: "Madam Cressida"})
```

### 5.7 Dialogue History → Messages

Each turn's messages stored via `memory_store_message`:

```
(:Conversation {session_id: "game-session", created_at: ...})
  -[:HAS_MESSAGE]-> (:Message {role: "user", content: "...", timestamp: ...})
  -[:HAS_MESSAGE]-> (:Message {role: "assistant", content: "...", ...})
```

The GM retrieves history via `memory_get_conversation` or `memory_get_context`.

### 5.8 Time → SQLite system_state + Neo4j Fact

Time is **dual-stored** for different purposes:
- **SQLite `system_state`** (`game_time_day`, `game_time_segment`): authoritative source for the `advanceTime` tool. Simple key-value, transactional.
- **Neo4j Fact**: `(:Fact {subject: "session", predicate: "has_current_time", object_value: "Day 3, Dawn"})` — allows the GM to reason about time via `memory_search` / `memory_get_context` without a separate tool call.

The `advanceTime` tool updates both.

## 6. GM Turn Loop (Modified)

```
POST /api/chat/stream
        │
        ▼
┌──────────────────────────────────────────┐
│  GameMaster.generateTurn()               │
│                                          │
│  streamText({                            │
│    system: buildSystemPrompt(),          │
│    messages: [{role: "user",             │
│      content: userInput + history}],     │
│    tools: {                              │
│      // 16 MCP tools (auto-discovered)   │
│      memory_search,                      │
│      memory_get_context,                 │
│      memory_add_entity,                  │
│      memory_get_entity,                  │
│      memory_create_relationship,         │
│      memory_add_fact,                    │
│      memory_store_message,               │
│      ...                                 │
│      // 2 Elysian tools                  │
│      generateDialogueStep,               │
│      advanceTime,                        │
│    },                                    │
│    stopWhen: generates once + valid,     │
│    prepareStep: nudges if no dialogue,   │
│  })                                      │
└──────────┬───────────────────────────────┘
           │ SSE events (simplified)
           ▼
┌──────────────────────────────────────┐
│  Console Client                      │
│                                      │
│  Events: step_start,                 │
│    streaming_messages,               │
│    streaming_reset,                  │
│    parsed,                           │
│    error, done                       │
│                                      │
│  (world/plot/scene events removed)   │
└──────────────────────────────────────┘
```

## 7. API Endpoints (Simplified)

| Method | Path | Purpose |
|--------|------|---------|
| `POST` | `/api/chat/stream` | Primary AI turn (SSE) |
| `POST` | `/api/reset` | Clear Neo4j + SQLite, re-seed |
| `GET` | `/api/history` | Recent messages from Neo4j (for console resume) |
| `GET` | `/api/session/current` | Latest step info |
| `GET` | `/api/debug/logs` | LLM interaction logs (SQLite) |
| `POST` | `/api/debug/logs/clear` | Clear logs |
| `GET` | `/api/debug/system-prompt` | Get system prompt template |
| `PUT` | `/api/debug/system-prompt` | Update template |
| `GET` | `/api/debug/system-prompt/default` | Get default template |
| `POST` | `/api/debug/system-prompt/reset` | Reset template |
| `GET` | `/api/ids/batch` | ID batch generation |

Removed endpoints: `/api/dialogue/*`, `/api/world`, `/api/world/entity`, `/api/notes`, `/api/plots`, `/api/plots/:id`, `/api/plots/pregen`, `/api/scene`, `/api/history` (POST), `/api/branches/activate`, `/api/regenerate`

## 8. agent-memory Modifications

### 8.1 Required Modifications

1. **Add Elysian-specific entity subtypes**:
   - `PERSON:CHARACTER` subtype
   - `LOCATION:TAVERN`, `LOCATION:TEMPLE`, etc. (optional, for richness)
   - Schema already supports subtypes — just add to the allowed subtypes list

2. **Add session strategy "persistent" support for SSE transport**:
   - Currently `per_conversation`, `per_day`, `persistent` strategies exist
   - We use `persistent` with a fixed user_id (e.g., `"elysian-game"`)
   - Verify it works correctly with SSE transport (currently stdio-focused)

3. **Increase `memory_get_context` context window**:
   - Default token threshold may be too small for RPG context
   - Expose this as configurable

### 8.2 Optional Modifications

4. **Custom Cypher initialization script**:
   - On first run, create Elysian-specific constraints/indexes
   - e.g., index on `Entity.status` for plot status queries

5. **Disable features we don't need**:
   - Entity extraction pipeline (we manage entities explicitly)
   - Preference detection (not relevant for GM)
   - Background enrichment (not needed)

6. **Add relationship type validation**:
   - Elysian uses specific relationship types: `LOCATED_AT`, `CARRIES`, `HOSTILE_TOWARDS`, `ALLIED_WITH`, `CHILD_PLOT`, `INVOLVES`, `OCCURRED_AT`
   - Could add these to a whitelist, or keep `memory_create_relationship` open-ended (current behavior)

### 8.3 What NOT to Modify

- The MCP server framework (FastMCP) — works as-is
- The core memory layers (`short_term.py`, `long_term.py`) — API is sufficient
- The graph client (`client.py`) — works as-is
- The schema manager (`schema.py`) — works as-is
- The resolution layer — not needed but harmless

## 9. Implementation Phases

### Phase 1: Prune (remove complexity)
1. Delete `src/client/`, `src/components/`, `src/services/`
2. Remove replay-related code from `src/server/models/dialogue.ts`, `src/server/models/history.ts`
3. Simplify `src/server/models/plot.ts` — remove tree validation, snapshots
4. Remove `src/server/models/world.ts`, `src/server/models/scene.ts`, `src/server/models/notes.ts`
5. Simplify `src/shared/events.ts` — keep only dialogue streaming events
6. Simplify `src/shared/constants.ts` — remove tool name constants
7. Remove 16 of 18 LLM tool files; keep only `generateDialogueStep.ts` and `advanceTime.ts`
8. Remove old seed story data (will re-import into Neo4j)
9. Simplify `src/server/api.ts` — remove world, plot, dialogue tree, notes, scene endpoints

### Phase 2: Connect (add MCP bridge)
1. Create `src/server/mcp/client.ts` — SSE MCP client
2. Create `src/server/mcp/tools.ts` — tool discovery + Vercel AI SDK wrapping
3. Create `src/server/mcp/seed.ts` — seed data → Neo4j via MCP
4. Update `src/server/llm/index.ts` — use MCP tools + 2 Elysian tools
5. Update `src/server/llm/prompt.ts` — simplified prompt referencing MCP tools
6. Update `src/server/llm/toolsFactory.ts` — removed, replaced by MCP discovery + local tools

### Phase 3: Test & Iterate
1. End-to-end flow: seed → generate turn → persist → resume
2. Console client validation
3. Verify reset/clear works correctly
4. Tune system prompt for MCP tool usage patterns

### Phase 4: agent-memory modifications (parallel with Phase 2-3)
1. Test agent-memory MCP server with persistent session strategy
2. Add CHARACTER subtype if needed
3. Configure for Elysian's needs (disable extraction, preferences, etc.)

## 10. Key Design Decisions

1. **Dual time storage**: Time stays in SQLite for transactional simplicity (the `advanceTime` tool is a single UPDATE), but is also reflected as a Neo4j Fact so the GM can discover it via `memory_get_context` without a dedicated tool call.

2. **MCP tools are wrapped, not rewritten**: The Elysian server doesn't re-implement entity CRUD. It discovers agent-memory's tools via MCP and passes them through to the LLM. This means Elysian benefits from agent-memory improvements automatically.

3. **No dialogue tree persistence**: Without replay mode, we don't need to store the dialogue tree structure. Each turn stores its messages in Neo4j via `memory_store_message`. History is retrieved via `memory_get_conversation`. The `persistStep` function is removed entirely.

4. **System prompt becomes leaner**: Template variables (`{{entities_brief}}`, `{{active_plots}}`, `{{current_scene}}`) are removed. The GM uses `memory_get_context` to get relevant world state on demand, rather than having everything dumped into the prompt. This reduces prompt bloat and lets the GM decide what context it needs.

5. **agent-memory runs as a separate process**: The MCP server (Python) and Elysian server (TypeScript) are separate processes communicating via SSE. This adds an operational dependency (two processes to start) but provides clean separation of concerns. A startup script or Docker Compose can manage this.
