# Design: Migrate agent-memory to TypeScript

Rewrite the subset of `agent-memory` that elysian-dialogue actually uses as a native TypeScript module under `src/server/memory/`, removing the MCP bridge and Python runtime dependency.

## Motivation

- elysian-dialogue connects to agent-memory (Python) over an SSE-based MCP bridge for 16 tools used by the Game Master
- elysian-dialogue already uses direct `neo4j-driver` for seed/reset, bypassing MCP
- The MCP bridge is unnecessary indirection: the Python library provides ~30 modules, but elysian-dialogue uses only ~12 core operations
- Benefits: simpler dev setup (no Python runtime), faster tool execution (no network hop), single codebase, cleaner debugging

## Architecture

```
src/server/memory/
├── client.ts          # MemoryClient singleton — lifecycle, connection management
├── neo4j.ts           # Neo4jClient — thin wrapper over neo4j-driver
├── embedder.ts        # Local embeddings (Xenova/ONNX) with OpenAI-compatible fallback
├── short-term.ts      # Conversations + messages with sequential linking
├── long-term.ts       # Entities (POLE+O), preferences, facts, relationships
├── reasoning.ts       # Reasoning traces, steps, tool calls
├── search.ts          # Hybrid vector + graph search across memory types
├── context.ts         # Assembled context for GM consumption
├── observer.ts        # Observational memory — token-threshold compression
├── tools.ts           # AI SDK tool definitions (replaces all 16 MCP tools)
└── types.ts           # Shared TypeScript types
```

### MemoryClient (singleton, lazy-initialized)

```ts
class MemoryClient {
  shortTerm: ShortTermMemory;
  longTerm: LongTermMemory;
  reasoning: ReasoningMemory;
  context: ContextAssembler;
  observer: MemoryObserver;
  search: MemorySearch;
  graph: { executeRead(query, params): Promise<Record[]> };

  static async getInstance(): Promise<MemoryClient>;
  async close(): Promise<void>;
}
```

## Embeddings

- **Default:** local ONNX model via `@xenova/transformers` (`all-MiniLM-L6-v2`, ~80MB, 384-dim)
- **Fallback:** any OpenAI-compatible endpoint (OpenRouter, OpenAI) when `EMBEDDING_API_URL` is set
- Pluggable interface: `Embedder { embed(text): number[]; embedBatch(texts[]): number[][]; dimensions: number }`

## Memory Layers

### ShortTermMemory

- `addMessage(sessionId, role, content, metadata?)` — store with sequential linking (FIRST_MESSAGE / NEXT_MESSAGE)
- `getConversation(sessionId, limit?)` — messages in chronological order
- `listSessions(limit?, offset?)` — sessions with previews

### LongTermMemory

- `addEntity(name, type, subtype?, description?, aliases?, metadata?)` — POLE+O entities with PascalCase labels (`:Entity:Person:Character`)
- `getEntity(name, type?)` — single entity lookup
- `searchEntities(query, types?, limit?)` — embedding-based search
- `addRelationship(sourceName, targetName, type, description?, confidence?)` — typed relationship
- `addPreference(category, preference, context?, confidence?)` / `getPreferences(category?, limit?)`
- `addFact(subject, predicate, objectValue, ...)` / `getFacts(subject?, predicate?, limit?)`

### ReasoningMemory

- `startTrace(sessionId, task, metadata?)` — begins a reasoning trace
- `addStep(traceId, thought?, action?, observation?)` — records a step
- `recordToolCall(stepId, toolName, args?, result?)` — records a tool call within a step
- `completeTrace(traceId, outcome?, success?)` — marks trace finished

### MemorySearch

- Hybrid vector + graph search across messages, entities, preferences, traces
- `search(query, memoryTypes?, sessionId?, limit?, threshold?)` — unified search entry point

### ContextAssembler

- `assemble(sessionId, query?, maxItems?)` — merges conversation history + relevant entities/preferences + similar traces into a single structured object

### MemoryObserver

- Token-threshold compression: tracks approximate token count per session
- Auto-generates keyword-based reflections when threshold exceeded (default 50k for elysian)
- `recordMessage(sessionId, content)` / `getObservations(sessionId)`

## Tools

Each tool is an AI SDK `tool()` definition, following the existing pattern in `src/server/llm/tools/`. Cleaned-up names:

| Old MCP Name | New Name | Read/Write |
|---|---|---|
| `memory_search` | `searchMemory` | Read |
| `memory_get_context` | `getContext` | Read |
| `memory_store_message` | `storeMessage` | Write |
| `memory_add_entity` | `saveEntity` | Write |
| `memory_get_entity` | `getEntity` | Read |
| `memory_create_relationship` | `linkEntities` | Write |
| `memory_add_fact` | `recordFact` | Write |
| `memory_add_preference` | `setPreference` | Write |
| `memory_get_conversation` | `getConversation` | Read |
| `memory_list_sessions` | `listSessions` | Read |
| `memory_start_trace` | `startTrace` | Write |
| `memory_record_step` | `recordStep` | Write |
| `memory_complete_trace` | `completeTrace` | Write |
| `memory_get_observations` | `getObservations` | Read |
| `memory_export_graph` | `exportGraph` | Read |
| `graph_query` | `queryGraph` | Read |

All 16 tools are passed to the LLM each turn alongside `generateDialogueStep` and `advanceTime`.

## Integration with generateTurn()

Before:
```ts
const mcpTools = await getMcpTools(); // SSE to Python MCP server
const allTools = { ...mcpTools, generateDialogueStep, advanceTime };
```

After:
```ts
import { createMemoryTools } from "@/server/memory/tools";
const allTools = { ...createMemoryTools(), generateDialogueStep, advanceTime };
```

## Startup Flow Changes

Before:
```
start Neo4j → start MCP server (Python, :8080) → start Express (:3000)
→ connect MCP client via SSE → seed via direct Neo4j driver
```

After:
```
start Neo4j → start Express (:3000)
→ init MemoryClient → run schema setup (indexes/constraints) → seed via MemoryClient
```

## What Gets Removed

- `src/server/mcp/client.ts` — MCP bridge
- `@ai-sdk/mcp` from `package.json`
- Python runtime requirement for elysian-dialogue
- `agent-memory/run_elysian_mcp.py` launcher
- MCP-related Makefile targets (`mcp-start`, `mcp-dev`, `mcp-install`)
- `make dev` simplifies to `neo4j-start` + `server` (no separate MCP process)

The `agent-memory/` directory and `pyproject.toml` stay (upstream library, not runtime).

## What We Explicitly Skip

Entity extraction pipeline (GLiNER, spaCy, LLM), entity resolution/deduplication, entity enrichment (Wikipedia, Diffbot), framework integrations, schema persistence/management, geocoding, observability (OpenTelemetry, Opik), multi-tenant, buffered writes, eval harness, CLI, provenance tracking.

## Migration Order

1. Create `src/server/memory/types.ts` — shared types
2. Create `src/server/memory/neo4j.ts` — Neo4j client wrapper
3. Create `src/server/memory/embedder.ts` — local embeddings with fallback
4. Create `src/server/memory/short-term.ts` — conversations & messages
5. Create `src/server/memory/long-term.ts` — entities, preferences, facts, relationships
6. Create `src/server/memory/reasoning.ts` — traces & steps
7. Create `src/server/memory/observer.ts` — observational memory
8. Create `src/server/memory/search.ts` — hybrid search
9. Create `src/server/memory/context.ts` — context assembly
10. Create `src/server/memory/client.ts` — MemoryClient singleton
11. Create `src/server/memory/tools.ts` — AI SDK tool definitions
12. Update `src/server/mcp/seed.ts` — use MemoryClient instead of direct driver
13. Update `src/server/mcp/reset.ts` — use MemoryClient instead of direct driver
14. Update `src/server/llm/prompt.ts` — new tool names in system prompt
15. Update `src/server/llm/index.ts` — replace getMcpTools() with createMemoryTools()
16. Update `src/server/main.ts` — replace MCP init with MemoryClient init
17. Remove `src/server/mcp/client.ts`, `@ai-sdk/mcp`, update Makefile
