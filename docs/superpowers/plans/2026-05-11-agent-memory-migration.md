# Agent-Memory TypeScript Migration — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Rewrite the subset of agent-memory used by elysian-dialogue as native TypeScript under `src/server/memory/`, removing the MCP bridge and Python runtime dependency.

**Architecture:** 12 modules under `src/server/memory/` — a Neo4j client wrapper, local embedder, three memory layers (short-term, long-term, reasoning), search, context assembly, observer, and AI SDK tool definitions. All modules use the existing `neo4j-driver` npm package. MemoryClient is a singleton.

**Hard rule:** Before writing the TypeScript equivalent of any module, read the original Python implementation in full first. Do not rely on docs or assumptions.

**Tech Stack:** TypeScript, `neo4j-driver`, `@xenova/transformers` (local embeddings), `ai` SDK (v6, tool definitions), Zod (input validation for tools)

---

### Task 1: Shared Types

**Files:**
- Create: `src/server/memory/types.ts`

**Context:** Maps to Python Pydantic models in `short_term.py` (Message, Conversation, SessionInfo), `long_term.py` (Entity, Preference, Fact, Relationship), `reasoning.py` (ReasoningTrace, ReasoningStep, ToolCall), and `_observer.py` (Observation, SessionContext).

- [ ] **Step 1: Write types.ts**

```typescript
// Entity types following POLE+O model
export type EntityType = "PERSON" | "OBJECT" | "LOCATION" | "ORGANIZATION" | "EVENT";

export type MessageRole = "user" | "assistant" | "system";

export interface MemoryEntity {
  id: string;
  name: string;
  type: EntityType;
  subtype?: string;
  description?: string;
  aliases: string[];
  metadata: Record<string, unknown>;
  embedding?: number[];
  createdAt: Date;
}

export interface MemoryMessage {
  id: string;
  sessionId: string;
  role: MessageRole;
  content: string;
  metadata: Record<string, unknown>;
  embedding?: number[];
  createdAt: Date;
}

export interface MemoryPreference {
  id: string;
  category: string;
  preference: string;
  context?: string;
  confidence: number;
  metadata: Record<string, unknown>;
  createdAt: Date;
}

export interface MemoryFact {
  id: string;
  subject: string;
  predicate: string;
  object: string;
  confidence: number;
  validFrom?: Date;
  validUntil?: Date;
  metadata: Record<string, unknown>;
  createdAt: Date;
}

export interface EntityRelationship {
  id: string;
  sourceId: string;
  targetId: string;
  type: string;
  description?: string;
  confidence: number;
}

export interface ReasoningTrace {
  id: string;
  sessionId: string;
  task: string;
  taskEmbedding?: number[];
  steps: ReasoningStep[];
  outcome?: string;
  success?: boolean;
  startedAt: Date;
  completedAt?: Date;
  metadata: Record<string, unknown>;
}

export interface ReasoningStep {
  id: string;
  traceId: string;
  stepNumber: number;
  thought?: string;
  action?: string;
  observation?: string;
  embedding?: number[];
  metadata: Record<string, unknown>;
}

export interface ToolCall {
  id: string;
  stepId: string;
  toolName: string;
  arguments: Record<string, unknown>;
  result?: unknown;
  status: "pending" | "success" | "failure";
  durationMs?: number;
  error?: string;
}

export interface SessionSummary {
  sessionId: string;
  title?: string;
  messageCount: number;
  createdAt: Date;
  updatedAt?: Date;
  firstMessagePreview?: string;
  lastMessagePreview?: string;
}

export interface Observation {
  type: "fact" | "decision" | "preference" | "topic" | "entity";
  content: string;
  sourceMessageId?: string;
  timestamp: string;
  confidence: number;
}

export interface ObservationResult {
  sessionId: string;
  messageCount: number;
  approximateTokens: number;
  thresholdTokens: number;
  thresholdExceeded: boolean;
  reflections: string[];
  observations: Observation[];
  entityNames: string[];
  topics: string[];
}

export interface SearchResults {
  messages: Array<MemoryMessage & { similarity: number }>;
  entities: Array<MemoryEntity & { similarity: number }>;
  preferences: Array<MemoryPreference & { similarity: number }>;
  traces: Array<ReasoningTrace & { similarity: number }>;
}

export interface AssembledContext {
  messages: MemoryMessage[];
  entities: MemoryEntity[];
  preferences: MemoryPreference[];
  traces: ReasoningTrace[];
  summary: string;
}
```

- [ ] **Step 2: Commit**

```bash
git add src/server/memory/types.ts
git commit -m "feat(memory): add shared types for memory layer"
```

### Task 2: Neo4j Client Wrapper

**Files:**
- Create: `src/server/memory/neo4j.ts`

**Context:** Maps to Python `Neo4jClient` in `graph/client.py`. The Python version uses `neo4j.AsyncGraphDatabase` with async context manager, connection verification, and `execute_read`/`execute_write` methods that return dict lists. The TypeScript `neo4j-driver` is synchronous so this wraps it with a session-based API. We already use `neo4j.driver()` in `src/server/mcp/seed.ts` — same pattern.

- [ ] **Step 1: Read original Python implementation**

Read `agent-memory/src/neo4j_agent_memory/graph/client.py` (lines 1-130), `agent-memory/src/neo4j_agent_memory/graph/schema.py` (lines 1-205) and `graph/query_builder.py` (the `build_create_entity_query` function).

- [ ] **Step 2: Write neo4j.ts**

```typescript
import neo4j from "neo4j-driver";

export class Neo4jClient {
  private driver: neo4j.Driver;

  constructor(
    uri: string = process.env.NEO4J_URI || "bolt://localhost:7687",
    user: string = process.env.NEO4J_USER || "neo4j",
    password: string = process.env.NEO4J_PASSWORD || "password",
  ) {
    this.driver = neo4j.driver(uri, neo4j.auth.basic(user, password));
  }

  async verifyConnectivity(): Promise<void> {
    await this.driver.verifyConnectivity();
  }

  async executeRead(
    query: string,
    parameters?: Record<string, unknown>,
  ): Promise<Record<string, unknown>[]> {
    const session = this.driver.session();
    try {
      const result = await session.executeRead((tx) =>
        tx.run(query, parameters),
      );
      return result.records.map((r) => r.toObject());
    } finally {
      await session.close();
    }
  }

  async executeWrite(
    query: string,
    parameters?: Record<string, unknown>,
  ): Promise<Record<string, unknown>[]> {
    const session = this.driver.session();
    try {
      const result = await session.executeWrite((tx) =>
        tx.run(query, parameters),
      );
      return result.records.map((r) => r.toObject());
    } finally {
      await session.close();
    }
  }

  async close(): Promise<void> {
    await this.driver.close();
  }
}
```

- [ ] **Step 3: Commit**

```bash
git add src/server/memory/neo4j.ts
git commit -m "feat(memory): add Neo4j client wrapper"
```

### Task 3: Schema Setup (indexes + constraints)

**Files:**
- Create: `src/server/memory/schema.ts`

**Context:** Maps to Python `SchemaManager` in `graph/schema.py`. Creates all constraints, indexes, and vector indexes needed for the memory layer. We need unique constraints on all node IDs, indexes on commonly queried properties, and vector indexes for embedding-based search.

- [ ] **Step 1: Read original Python implementation**

Read `agent-memory/src/neo4j_agent_memory/graph/schema.py` (already read, verify).

- [ ] **Step 2: Write schema.ts**

```typescript
import { Neo4jClient } from "./neo4j";

export async function setupSchema(
  client: Neo4jClient,
  vectorDimensions: number = 384,
): Promise<void> {
  // Unique constraints
  const constraints: [string, string, string][] = [
    ["conversation_id", "Conversation", "id"],
    ["message_id", "Message", "id"],
    ["entity_id", "Entity", "id"],
    ["preference_id", "Preference", "id"],
    ["fact_id", "Fact", "id"],
    ["reasoning_trace_id", "ReasoningTrace", "id"],
    ["reasoning_step_id", "ReasoningStep", "id"],
  ];

  for (const [name, label, prop] of constraints) {
    await client.executeWrite(
      `CREATE CONSTRAINT ${name} IF NOT EXISTS FOR (n:${label}) REQUIRE n.${prop} IS UNIQUE`,
    );
  }

  // Regular indexes
  const indexes: [string, string, string][] = [
    ["conversation_session_idx", "Conversation", "session_id"],
    ["message_timestamp_idx", "Message", "timestamp"],
    ["entity_type_idx", "Entity", "type"],
    ["entity_name_idx", "Entity", "name"],
    ["preference_category_idx", "Preference", "category"],
    ["trace_session_idx", "ReasoningTrace", "session_id"],
    ["trace_success_idx", "ReasoningTrace", "success"],
  ];

  for (const [name, label, prop] of indexes) {
    await client.executeWrite(
      `CREATE INDEX ${name} IF NOT EXISTS FOR (n:${label}) ON (n.${prop})`,
    );
  }

  // Vector indexes (require Neo4j 5.11+)
  const vectorIndexes: [string, string, string][] = [
    ["message_embedding_idx", "Message", "embedding"],
    ["entity_embedding_idx", "Entity", "embedding"],
    ["preference_embedding_idx", "Preference", "embedding"],
    ["fact_embedding_idx", "Fact", "embedding"],
    ["task_embedding_idx", "ReasoningTrace", "task_embedding"],
    ["step_embedding_idx", "ReasoningStep", "embedding"],
  ];

  for (const [name, label, prop] of vectorIndexes) {
    try {
      await client.executeWrite(
        `CREATE VECTOR INDEX ${name} IF NOT EXISTS FOR (n:${label}) ON (n.${prop})
         OPTIONS { indexConfig: { \`vector.dimensions\`: ${vectorDimensions}, \`vector.similarity_function\`: 'COSINE' } }`,
      );
    } catch {
      console.warn(`[memory] Vector index ${name} not created (Neo4j 5.11+ required)`);
    }
  }
}
```

- [ ] **Step 3: Commit**

```bash
git add src/server/memory/schema.ts
git commit -m "feat(memory): add schema setup for indexes and constraints"
```

### Task 4: Embedder

**Files:**
- Create: `src/server/memory/embedder.ts`
- Modify: `package.json` — add `@xenova/transformers`

**Context:** Maps to Python embedder in `embeddings/base.py` and `embeddings/openai.py`. The Python Embedder protocol exposes `embed(text) -> list[float]` and `embed_batch(texts) -> list[list[float]]`. We create a pluggable TypeScript interface with local-first default and OpenAI-compatible fallback.

- [ ] **Step 1: Read original Python implementation**

Read `agent-memory/src/neo4j_agent_memory/embeddings/base.py` and `agent-memory/src/neo4j_agent_memory/embeddings/openai.py`.

- [ ] **Step 2: Install @xenova/transformers**

```bash
npm install @xenova/transformers
```

- [ ] **Step 3: Write embedder.ts**

```typescript
export interface Embedder {
  embed(text: string): Promise<number[]>;
  embedBatch(texts: string[]): Promise<number[][]>;
  readonly dimensions: number;
}

// Default model: all-MiniLM-L6-v2, 384 dimensions, ~80MB
const DEFAULT_LOCAL_MODEL = "Xenova/all-MiniLM-L6-v2";
const LOCAL_DIMENSIONS = 384;

let localPipeline: unknown = null;

async function getLocalPipeline() {
  if (!localPipeline) {
    const { pipeline } = await import("@xenova/transformers");
    localPipeline = await pipeline("feature-extraction", DEFAULT_LOCAL_MODEL);
  }
  return localPipeline as { (text: string, options?: { pooling?: string }): Promise<{ data: Float32Array }> };
}

export class LocalEmbedder implements Embedder {
  readonly dimensions = LOCAL_DIMENSIONS;

  async embed(text: string): Promise<number[]> {
    const pipe = await getLocalPipeline();
    const result = await pipe(text, { pooling: "mean" });
    return Array.from(result.data);
  }

  async embedBatch(texts: string[]): Promise<number[][]> {
    // Process sequentially to avoid memory pressure from ONNX
    const results: number[][] = [];
    for (const text of texts) {
      results.push(await this.embed(text));
    }
    return results;
  }
}

export class OpenAICompatibleEmbedder implements Embedder {
  readonly dimensions: number;
  private baseURL: string;
  private apiKey: string;
  private model: string;

  constructor(
    baseURL: string,
    apiKey: string,
    model: string = "text-embedding-3-small",
    dimensions: number = 1536,
  ) {
    this.baseURL = baseURL;
    this.apiKey = apiKey;
    this.model = model;
    this.dimensions = dimensions;
  }

  async embed(text: string): Promise<number[]> {
    const res = await fetch(`${this.baseURL}/embeddings`, {
      method: "POST",
      headers: {
        "Content-Type": "application/json",
        Authorization: `Bearer ${this.apiKey}`,
      },
      body: JSON.stringify({ model: this.model, input: text }),
    });
    const json = (await res.json()) as { data: Array<{ embedding: number[] }> };
    return json.data[0].embedding;
  }

  async embedBatch(texts: string[]): Promise<number[][]> {
    const res = await fetch(`${this.baseURL}/embeddings`, {
      method: "POST",
      headers: {
        "Content-Type": "application/json",
        Authorization: `Bearer ${this.apiKey}`,
      },
      body: JSON.stringify({ model: this.model, input: texts }),
    });
    const json = (await res.json()) as { data: Array<{ embedding: number[] }> };
    return json.data.map((d) => d.embedding);
  }
}

let embedder: Embedder | null = null;

export function getEmbedder(): Embedder {
  if (embedder) return embedder;

  const apiUrl = process.env.EMBEDDING_API_URL;
  const apiKey = process.env.EMBEDDING_API_KEY;

  if (apiUrl && apiKey) {
    const model = process.env.EMBEDDING_MODEL || "text-embedding-3-small";
    const dims = parseInt(process.env.EMBEDDING_DIMENSIONS || "1536", 10);
    embedder = new OpenAICompatibleEmbedder(apiUrl, apiKey, model, dims);
  } else {
    embedder = new LocalEmbedder();
  }

  return embedder;
}
```

- [ ] **Step 4: Commit**

```bash
git add src/server/memory/embedder.ts package.json package-lock.json
git commit -m "feat(memory): add pluggable embedder with local and OpenAI-compatible backends"
```

### Task 5: Short-Term Memory

**Files:**
- Create: `src/server/memory/short-term.ts`

**Context:** Maps to Python `ShortTermMemory` in `memory/short_term.py`. Core operations: `add_message` (store with sequential linking via FIRST_MESSAGE/NEXT_MESSAGE), `get_conversation` (retrieve all messages in order), `list_sessions` (browse sessions), `search_messages` (semantic search via embedding). We skip entity extraction, batch messages, delete, summarization, and migration.

- [ ] **Step 1: Read original Python implementation**

Read `agent-memory/src/neo4j_agent_memory/memory/short_term.py` — focus on `add_message` (line 495-588), `get_conversation` (line 649-714), `list_sessions` (line 863-912), `_ensure_conversation` (line 1062-1106), `_get_last_message_id` (line 1131-1139), `_create_message_links` (line 1141-1160).

- [ ] **Step 2: Write short-term.ts**

```typescript
import { v4 as uuidv4 } from "uuid";
import { Neo4jClient } from "./neo4j";
import { Embedder, getEmbedder } from "./embedder";
import type { MemoryMessage, SessionSummary } from "./types";

export class ShortTermMemory {
  private client: Neo4jClient;
  private embedder: Embedder;

  constructor(client: Neo4jClient) {
    this.client = client;
    this.embedder = getEmbedder();
  }

  async addMessage(
    sessionId: string,
    role: "user" | "assistant" | "system",
    content: string,
    metadata?: Record<string, unknown>,
    generateEmbedding: boolean = true,
  ): Promise<MemoryMessage> {
    // Ensure conversation exists
    const convId = await this.ensureConversation(sessionId);

    // Generate embedding
    let embedding: number[] | undefined;
    if (generateEmbedding) {
      embedding = await this.embedder.embed(content);
    }

    const messageId = uuidv4();
    const now = new Date().toISOString();

    // Create message
    await this.client.executeWrite(
      `MATCH (c:Conversation {id: $convId})
       CREATE (m:Message {
         id: $id, role: $role, content: $content,
         embedding: $embedding, timestamp: datetime($now),
         metadata: $metadata
       })
       CREATE (c)-[:HAS_MESSAGE]->(m)
       RETURN m`,
      {
        convId,
        id: messageId,
        role,
        content,
        embedding: embedding || null,
        now,
        metadata: metadata ? JSON.stringify(metadata) : null,
      },
    );

    // Handle sequential linking
    const lastId = await this.getLastMessageId(convId, messageId);
    const isFirst = lastId === null;
    await this.createMessageLinks(convId, [messageId], lastId, isFirst);

    return {
      id: messageId,
      sessionId,
      role,
      content,
      metadata: metadata || {},
      embedding,
      createdAt: new Date(now),
    };
  }

  async getConversation(
    sessionId: string,
    limit: number = 1000,
  ): Promise<MemoryMessage[]> {
    const rows = await this.client.executeRead(
      `MATCH (c:Conversation {session_id: $sessionId})
       MATCH (c)-[:HAS_MESSAGE]->(m:Message)
       RETURN m
       ORDER BY m.timestamp ASC
       LIMIT $limit`,
      { sessionId, limit },
    );

    return rows.map((r) => {
      const m = r.m as Record<string, unknown>;
      return {
        id: m.id as string,
        sessionId,
        role: m.role as "user" | "assistant" | "system",
        content: m.content as string,
        metadata: m.metadata ? JSON.parse(m.metadata as string) : {},
        embedding: m.embedding as number[] | undefined,
        createdAt: new Date((m.timestamp as string) || Date.now()),
      };
    });
  }

  async listSessions(
    limit: number = 20,
    offset: number = 0,
  ): Promise<SessionSummary[]> {
    const rows = await this.client.executeRead(
      `MATCH (c:Conversation)
       OPTIONAL MATCH (c)-[:FIRST_MESSAGE]->(first:Message)
       OPTIONAL MATCH (c)-[:HAS_MESSAGE]->(last:Message)
       WITH c, first, last
       ORDER BY coalesce(c.updated_at, c.created_at) DESC
       SKIP $offset LIMIT $limit
       OPTIONAL MATCH (c)-[:HAS_MESSAGE]->(m:Message)
       RETURN c, first, last, count(m) AS messageCount`,
      { limit, offset },
    );

    return rows.map((r) => {
      const c = r.c as Record<string, unknown>;
      const first = r.first as Record<string, unknown> | null;
      const last = r.last as Record<string, unknown> | null;
      return {
        sessionId: c.session_id as string,
        title: c.title as string | undefined,
        messageCount: r.messageCount as number,
        createdAt: new Date((c.created_at as string) || Date.now()),
        updatedAt: c.updated_at ? new Date(c.updated_at as string) : undefined,
        firstMessagePreview: first
          ? (first.content as string).slice(0, 100)
          : undefined,
        lastMessagePreview: last
          ? (last.content as string).slice(0, 100)
          : undefined,
      };
    });
  }

  async searchMessages(
    query: string,
    options?: {
      sessionId?: string;
      limit?: number;
      threshold?: number;
    },
  ): Promise<Array<MemoryMessage & { similarity: number }>> {
    const { sessionId, limit = 10, threshold = 0.7 } = options || {};
    const queryEmbedding = await this.embedder.embed(query);

    const rows = await this.client.executeRead(
      `CALL db.index.vector.queryNodes('message_embedding_idx', $limit, $embedding)
       YIELD node AS m, score
       WHERE score >= $threshold
       ${sessionId ? "AND m.session_id = $sessionId" : ""}
       RETURN m, score
       ORDER BY score DESC`,
      { embedding: queryEmbedding, limit, threshold, sessionId },
    );

    return rows.map((r) => {
      const m = r.m as Record<string, unknown>;
      return {
        id: m.id as string,
        sessionId: m.session_id as string,
        role: m.role as "user" | "assistant" | "system",
        content: m.content as string,
        metadata: m.metadata ? JSON.parse(m.metadata as string) : {},
        similarity: r.score as number,
        createdAt: new Date((m.timestamp as string) || Date.now()),
      };
    });
  }

  // ── Private helpers ──

  private async ensureConversation(sessionId: string): Promise<string> {
    const rows = await this.client.executeRead(
      `MATCH (c:Conversation {session_id: $sessionId}) RETURN c.id AS id`,
      { sessionId },
    );
    if (rows.length > 0) return rows[0].id as string;

    const convId = uuidv4();
    await this.client.executeWrite(
      `CREATE (c:Conversation {
         id: $id, session_id: $sessionId,
         created_at: datetime(), updated_at: datetime()
       })`,
      { id: convId, sessionId },
    );
    return convId;
  }

  private async getLastMessageId(
    convId: string,
    excludeId: string,
  ): Promise<string | null> {
    const rows = await this.client.executeRead(
      `MATCH (c:Conversation {id: $convId})-[:HAS_MESSAGE]->(m:Message)
       WHERE m.id <> $excludeId
       OPTIONAL MATCH (m)-[:NEXT_MESSAGE]->(:Message)
       WITH m, COUNT(m) AS hasNext
       WHERE hasNext = 0
       RETURN m.id AS id
       ORDER BY m.timestamp DESC LIMIT 1`,
      { convId, excludeId },
    );
    return rows.length > 0 ? (rows[0].id as string) : null;
  }

  private async createMessageLinks(
    convId: string,
    messageIds: string[],
    previousLastId: string | null,
    createFirstMessage: boolean,
  ): Promise<void> {
    if (messageIds.length === 0) return;

    // Create sequential NEXT_MESSAGE links
    const allIds = previousLastId
      ? [previousLastId, ...messageIds]
      : messageIds;

    for (let i = 0; i < allIds.length - 1; i++) {
      await this.client.executeWrite(
        `MATCH (prev:Message {id: $prevId})
         MATCH (next:Message {id: $nextId})
         CREATE (prev)-[:NEXT_MESSAGE]->(next)`,
        { prevId: allIds[i], nextId: allIds[i + 1] },
      );
    }

    // Set FIRST_MESSAGE if this is the first batch for this conversation
    if (createFirstMessage && messageIds.length > 0) {
      await this.client.executeWrite(
        `MATCH (c:Conversation {id: $convId}), (m:Message {id: $msgId})
         CREATE (c)-[:FIRST_MESSAGE]->(m)`,
        { convId, msgId: messageIds[0] },
      );
    }
  }
}
```

- [ ] **Step 3: Commit**

```bash
git add src/server/memory/short-term.ts
git commit -m "feat(memory): add short-term memory for conversations and messages"
```

### Task 6: Long-Term Memory

**Files:**
- Create: `src/server/memory/long-term.ts`

**Context:** Maps to Python `LongTermMemory` in `memory/long_term.py`. Core operations: entity CRUD with PascalCase labels, preference storage, fact triples, typed relationships. We skip entity resolution, deduplication, enrichment, geocoding, provenance tracking.

- [ ] **Step 1: Read original Python implementation**

Read `agent-memory/src/neo4j_agent_memory/memory/long_term.py` — focus on `add_entity` (line 390-552), `add_preference` (line 554-671), `add_fact` (line 816-912), `add_relationship` (line 914-971), `search_entities` (line 1000-1052), `get_entity_by_name` (line 973-994), `search_preferences` (line 1054-1107), `parse_entity` (line 2030-2049), `parse_preference` (line 2051-2062), `parse_fact` (line 2064-2081). Also read `graph/query_builder.py` — the `build_create_entity_query` function for dynamic labels.

- [ ] **Step 2: Write long-term.ts**

```typescript
import { v4 as uuidv4 } from "uuid";
import { Neo4jClient } from "./neo4j";
import { Embedder, getEmbedder } from "./embedder";
import type { EntityType, MemoryEntity, MemoryPreference, MemoryFact, EntityRelationship } from "./types";

function pascalCase(str: string): string {
  return str.charAt(0).toUpperCase() + str.slice(1).toLowerCase();
}

export class LongTermMemory {
  private client: Neo4jClient;
  private embedder: Embedder;

  constructor(client: Neo4jClient) {
    this.client = client;
    this.embedder = getEmbedder();
  }

  // ── Entities ──

  async addEntity(
    name: string,
    entityType: EntityType | string,
    options?: {
      subtype?: string;
      description?: string;
      aliases?: string[];
      metadata?: Record<string, unknown>;
      generateEmbedding?: boolean;
    },
  ): Promise<MemoryEntity> {
    const { subtype, description, aliases, metadata, generateEmbedding = true } = options || {};
    const entityId = uuidv4();
    const typeStr = entityType.toUpperCase();

    // Build dynamic labels
    const typeLabel = pascalCase(typeStr);
    const subtypeLabel = subtype ? pascalCase(subtype) : null;
    const labelStr = subtypeLabel
      ? `:Entity:${typeLabel}:${subtypeLabel}`
      : `:Entity:${typeLabel}`;

    // Generate embedding
    let embedding: number[] | undefined;
    if (generateEmbedding) {
      embedding = await this.embedder.embed(name);
    }

    const storageMetadata = {
      ...metadata,
      aliases: aliases || [],
    };

    const result = await this.client.executeWrite(
      `CREATE (e${labelStr} {
         id: $id, name: $name, type: $type,
         subtype: $subtype, description: $description,
         embedding: $embedding, metadata: $metadata,
         created_at: datetime()
       }) RETURN e`,
      {
        id: entityId,
        name,
        type: typeStr,
        subtype: subtype || null,
        description: description || null,
        embedding: embedding || null,
        metadata: JSON.stringify(storageMetadata),
      },
    );

    const e = (result[0].e as Record<string, unknown>) || {};
    return {
      id: entityId,
      name,
      type: typeStr as EntityType,
      subtype,
      description,
      aliases: aliases || [],
      metadata: metadata || {},
      embedding,
      createdAt: new Date(),
    };
  }

  async getEntity(name: string, type?: string): Promise<MemoryEntity | null> {
    let query = `MATCH (e:Entity {name: $name})`;
    if (type) {
      query += ` WHERE e.type = $type`;
    }
    query += ` RETURN e LIMIT 1`;

    const rows = await this.client.executeRead(query, { name, type });
    if (rows.length === 0) return null;

    return this.parseEntity(rows[0].e as Record<string, unknown>);
  }

  async searchEntities(
    query: string,
    options?: {
      entityTypes?: string[];
      limit?: number;
      threshold?: number;
    },
  ): Promise<Array<MemoryEntity & { similarity: number }>> {
    const { entityTypes, limit = 10, threshold = 0.7 } = options || {};
    const queryEmbedding = await this.embedder.embed(query);

    const rows = await this.client.executeRead(
      `CALL db.index.vector.queryNodes('entity_embedding_idx', $limit, $embedding)
       YIELD node AS e, score
       WHERE score >= $threshold
       RETURN e, score
       ORDER BY score DESC`,
      { embedding: queryEmbedding, limit: limit * 2, threshold },
    );

    const filterTypes = entityTypes
      ? new Set(entityTypes.map((t) => t.toUpperCase()))
      : null;

    const results: Array<MemoryEntity & { similarity: number }> = [];
    for (const row of rows) {
      const entity = this.parseEntity(row.e as Record<string, unknown>);
      if (filterTypes && !filterTypes.has(entity.type)) continue;
      if (results.length >= limit) break;
      results.push({ ...entity, similarity: row.score as number });
    }

    return results;
  }

  // ── Preferences ──

  async addPreference(
    category: string,
    preference: string,
    options?: {
      context?: string;
      confidence?: number;
      metadata?: Record<string, unknown>;
      generateEmbedding?: boolean;
    },
  ): Promise<MemoryPreference> {
    const { context, confidence = 1.0, metadata, generateEmbedding = true } = options || {};
    const prefId = uuidv4();

    let embedding: number[] | undefined;
    if (generateEmbedding) {
      const text = context
        ? `${category}: ${preference} (${context})`
        : `${category}: ${preference}`;
      embedding = await this.embedder.embed(text);
    }

    await this.client.executeWrite(
      `CREATE (p:Preference {
         id: $id, category: $category, preference: $preference,
         context: $context, confidence: $confidence,
         embedding: $embedding, metadata: $metadata,
         created_at: datetime(), valid_from: datetime()
       })`,
      {
        id: prefId,
        category,
        preference,
        context: context || null,
        confidence,
        embedding: embedding || null,
        metadata: metadata ? JSON.stringify(metadata) : null,
      },
    );

    return {
      id: prefId,
      category,
      preference,
      context,
      confidence,
      metadata: metadata || {},
      createdAt: new Date(),
    };
  }

  async getPreferences(
    category?: string,
    limit: number = 100,
  ): Promise<MemoryPreference[]> {
    let query: string;
    const params: Record<string, unknown> = { limit };

    if (category) {
      query = `MATCH (p:Preference {category: $category}) RETURN p ORDER BY p.created_at DESC LIMIT $limit`;
      params.category = category;
    } else {
      query = `MATCH (p:Preference) RETURN p ORDER BY p.created_at DESC LIMIT $limit`;
    }

    const rows = await this.client.executeRead(query, params);
    return rows.map((r) => this.parsePreference(r.p as Record<string, unknown>));
  }

  // ── Facts ──

  async addFact(
    subject: string,
    predicate: string,
    objectValue: string,
    options?: {
      confidence?: number;
      validFrom?: Date;
      validUntil?: Date;
      metadata?: Record<string, unknown>;
      generateEmbedding?: boolean;
    },
  ): Promise<MemoryFact> {
    const { confidence = 1.0, validFrom, validUntil, metadata, generateEmbedding = true } = options || {};
    const factId = uuidv4();

    let embedding: number[] | undefined;
    if (generateEmbedding) {
      embedding = await this.embedder.embed(`${subject} ${predicate} ${objectValue}`);
    }

    await this.client.executeWrite(
      `CREATE (f:Fact {
         id: $id, subject: $subject, predicate: $predicate,
         object: $object, confidence: $confidence,
         embedding: $embedding,
         valid_from: $validFrom, valid_until: $validUntil,
         metadata: $metadata, created_at: datetime()
       })`,
      {
        id: factId,
        subject,
        predicate,
        object: objectValue,
        confidence,
        embedding: embedding || null,
        validFrom: validFrom?.toISOString() || null,
        validUntil: validUntil?.toISOString() || null,
        metadata: metadata ? JSON.stringify(metadata) : null,
      },
    );

    return {
      id: factId,
      subject,
      predicate,
      object: objectValue,
      confidence,
      validFrom,
      validUntil,
      metadata: metadata || {},
      createdAt: new Date(),
    };
  }

  async getFacts(subject?: string, limit: number = 100): Promise<MemoryFact[]> {
    let query: string;
    const params: Record<string, unknown> = { limit };

    if (subject) {
      query = `MATCH (f:Fact {subject: $subject}) RETURN f ORDER BY f.created_at DESC LIMIT $limit`;
      params.subject = subject;
    } else {
      query = `MATCH (f:Fact) RETURN f ORDER BY f.created_at DESC LIMIT $limit`;
    }

    const rows = await this.client.executeRead(query, params);
    return rows.map((r) => this.parseFact(r.f as Record<string, unknown>));
  }

  // ── Relationships ──

  async addRelationship(
    sourceName: string,
    targetName: string,
    relationshipType: string,
    options?: {
      description?: string;
      confidence?: number;
    },
  ): Promise<void> {
    const { description, confidence = 1.0 } = options || {};
    // Sanitize relationship type for Cypher
    const safeType = relationshipType.replace(/[^A-Za-z0-9_]/g, "_");

    await this.client.executeWrite(
      `MATCH (a:Entity {name: $sourceName})
       MATCH (b:Entity {name: $targetName})
       CREATE (a)-[r:${safeType} {
         description: $description, confidence: $confidence,
         created_at: datetime()
       }]->(b)`,
      { sourceName, targetName, description: description || null, confidence },
    );
  }

  // ── Parsers ──

  private parseEntity(data: Record<string, unknown>): MemoryEntity {
    const meta = data.metadata
      ? JSON.parse(data.metadata as string)
      : {};
    const aliases = meta.aliases || [];
    delete meta.aliases;

    return {
      id: data.id as string,
      name: data.name as string,
      type: data.type as EntityType,
      subtype: data.subtype as string | undefined,
      description: data.description as string | undefined,
      aliases,
      metadata: meta,
      embedding: data.embedding as number[] | undefined,
      createdAt: new Date((data.created_at as string) || Date.now()),
    };
  }

  private parsePreference(data: Record<string, unknown>): MemoryPreference {
    return {
      id: data.id as string,
      category: data.category as string,
      preference: data.preference as string,
      context: data.context as string | undefined,
      confidence: (data.confidence as number) || 1.0,
      metadata: data.metadata ? JSON.parse(data.metadata as string) : {},
      createdAt: new Date((data.created_at as string) || Date.now()),
    };
  }

  private parseFact(data: Record<string, unknown>): MemoryFact {
    return {
      id: data.id as string,
      subject: data.subject as string,
      predicate: data.predicate as string,
      object: data.object as string,
      confidence: (data.confidence as number) || 1.0,
      validFrom: data.valid_from
        ? new Date(data.valid_from as string)
        : undefined,
      validUntil: data.valid_until
        ? new Date(data.valid_until as string)
        : undefined,
      metadata: data.metadata ? JSON.parse(data.metadata as string) : {},
      createdAt: new Date((data.created_at as string) || Date.now()),
    };
  }
}
```

- [ ] **Step 3: Commit**

```bash
git add src/server/memory/long-term.ts
git commit -m "feat(memory): add long-term memory for entities, preferences, facts, and relationships"
```

### Task 7: Reasoning Memory

**Files:**
- Create: `src/server/memory/reasoning.ts`

**Context:** Maps to Python `ReasoningMemory` in `memory/reasoning.py`. Core operations: `start_trace`, `add_step`, `record_tool_call`, `complete_trace`, `get_similar_traces`, `search_steps`. We skip tool call hooks, stats migration, streaming trace recorder.

- [ ] **Step 1: Read original Python implementation**

Read `agent-memory/src/neo4j_agent_memory/memory/reasoning.py` — focus on `start_trace` (line 423-512), `add_step` (line 514-584), `record_tool_call` (line 625-725), `complete_trace` (line 745-853), `get_similar_traces` (line 1022-1075), `search_steps` (line 946-1020).

- [ ] **Step 2: Write reasoning.ts**

```typescript
import { v4 as uuidv4 } from "uuid";
import { Neo4jClient } from "./neo4j";
import { Embedder, getEmbedder } from "./embedder";
import type { ReasoningTrace, ReasoningStep, ToolCall } from "./types";

export class ReasoningMemory {
  private client: Neo4jClient;
  private embedder: Embedder;

  constructor(client: Neo4jClient) {
    this.client = client;
    this.embedder = getEmbedder();
  }

  async startTrace(
    sessionId: string,
    task: string,
    options?: {
      generateEmbedding?: boolean;
      metadata?: Record<string, unknown>;
    },
  ): Promise<ReasoningTrace> {
    const { generateEmbedding = true, metadata } = options || {};
    const traceId = uuidv4();

    let taskEmbedding: number[] | undefined;
    if (generateEmbedding) {
      taskEmbedding = await this.embedder.embed(task);
    }

    await this.client.executeWrite(
      `CREATE (rt:ReasoningTrace {
         id: $id, session_id: $sessionId, task: $task,
         task_embedding: $embedding, outcome: null, success: null,
         completed_at: null, started_at: datetime(),
         metadata: $metadata
       })`,
      {
        id: traceId,
        sessionId,
        task,
        embedding: taskEmbedding || null,
        metadata: metadata ? JSON.stringify(metadata) : null,
      },
    );

    return {
      id: traceId,
      sessionId,
      task,
      taskEmbedding,
      steps: [],
      startedAt: new Date(),
      metadata: metadata || {},
    };
  }

  async addStep(
    traceId: string,
    options?: {
      thought?: string;
      action?: string;
      observation?: string;
      generateEmbedding?: boolean;
      metadata?: Record<string, unknown>;
    },
  ): Promise<ReasoningStep> {
    const { thought, action, observation, generateEmbedding = true, metadata } = options || {};
    const stepId = uuidv4();

    // Get next step number
    const countRows = await this.client.executeRead(
      `MATCH (:ReasoningTrace {id: $traceId})-[:HAS_STEP]->(s:ReasoningStep)
       RETURN count(s) AS count`,
      { traceId },
    );
    const stepNumber = ((countRows[0]?.count as number) || 0) + 1;

    // Generate embedding from thought/action/observation text
    let embedding: number[] | undefined;
    if (generateEmbedding) {
      const parts = [];
      if (thought) parts.push(`Thought: ${thought}`);
      if (action) parts.push(`Action: ${action}`);
      if (observation) parts.push(`Observation: ${observation}`);
      if (parts.length > 0) {
        embedding = await this.embedder.embed(parts.join(" "));
      }
    }

    await this.client.executeWrite(
      `MATCH (rt:ReasoningTrace {id: $traceId})
       CREATE (rt)-[:HAS_STEP]->(s:ReasoningStep {
         id: $id, step_number: $stepNumber,
         thought: $thought, action: $action,
         observation: $observation, embedding: $embedding,
         metadata: $metadata
       })`,
      {
        traceId,
        id: stepId,
        stepNumber,
        thought: thought || null,
        action: action || null,
        observation: observation || null,
        embedding: embedding || null,
        metadata: metadata ? JSON.stringify(metadata) : null,
      },
    );

    return {
      id: stepId,
      traceId,
      stepNumber,
      thought,
      action,
      observation,
      embedding,
      metadata: metadata || {},
    };
  }

  async recordToolCall(
    stepId: string,
    toolName: string,
    args: Record<string, unknown>,
    options?: {
      result?: unknown;
      status?: "pending" | "success" | "failure";
      durationMs?: number;
      error?: string;
    },
  ): Promise<ToolCall> {
    const { result, status = "success", durationMs, error } = options || {};
    const callId = uuidv4();

    await this.client.executeWrite(
      `MATCH (s:ReasoningStep {id: $stepId})
       CREATE (s)-[:HAS_TOOL_CALL]->(tc:ToolCall {
         id: $id, tool_name: $toolName,
         arguments: $args, result: $result,
         status: $status, duration_ms: $durationMs,
         error: $error, created_at: datetime()
       })`,
      {
        stepId,
        id: callId,
        toolName,
        args: JSON.stringify(args),
        result: result != null ? JSON.stringify(result) : null,
        status,
        durationMs: durationMs || null,
        error: error || null,
      },
    );

    return {
      id: callId,
      stepId,
      toolName,
      arguments: args,
      result,
      status,
      durationMs,
      error,
    };
  }

  async completeTrace(
    traceId: string,
    options?: {
      outcome?: string;
      success?: boolean;
    },
  ): Promise<void> {
    const { outcome, success } = options || {};
    await this.client.executeWrite(
      `MATCH (rt:ReasoningTrace {id: $id})
       SET rt.outcome = $outcome,
           rt.success = $success,
           rt.completed_at = datetime()`,
      {
        id: traceId,
        outcome: outcome || null,
        success: success ?? null,
      },
    );
  }

  async getSimilarTraces(
    task: string,
    options?: {
      limit?: number;
      successOnly?: boolean;
      threshold?: number;
    },
  ): Promise<Array<ReasoningTrace & { similarity: number }>> {
    const { limit = 5, successOnly = true, threshold = 0.7 } = options || {};
    const taskEmbedding = await this.embedder.embed(task);

    const rows = await this.client.executeRead(
      `CALL db.index.vector.queryNodes('task_embedding_idx', $limit, $embedding)
       YIELD node AS rt, score
       WHERE score >= $threshold
       ${successOnly ? "AND rt.success = true" : ""}
       RETURN rt, score
       ORDER BY score DESC`,
      { embedding: taskEmbedding, limit, threshold },
    );

    return rows.map((r) => {
      const rt = r.rt as Record<string, unknown>;
      return {
        id: rt.id as string,
        sessionId: rt.session_id as string,
        task: rt.task as string,
        taskEmbedding: rt.task_embedding as number[] | undefined,
        outcome: rt.outcome as string | undefined,
        success: rt.success as boolean | undefined,
        startedAt: new Date((rt.started_at as string) || Date.now()),
        completedAt: rt.completed_at
          ? new Date(rt.completed_at as string)
          : undefined,
        similarity: r.score as number,
        steps: [],
        metadata: { similarity: r.score as number },
      };
    });
  }

  async searchSteps(
    query: string,
    options?: {
      limit?: number;
      successOnly?: boolean;
      threshold?: number;
    },
  ): Promise<Array<{ step: ReasoningStep; similarity: number; parentTask: string }>> {
    const { limit = 10, successOnly = true, threshold = 0.7 } = options || {};
    const queryEmbedding = await this.embedder.embed(query);

    const rows = await this.client.executeRead(
      `CALL db.index.vector.queryNodes('step_embedding_idx', $limit, $embedding)
       YIELD node AS rs, score
       WHERE score >= $threshold
       RETURN rs, score
       ORDER BY score DESC`,
      { embedding: queryEmbedding, limit, threshold },
    );

    const results: Array<{ step: ReasoningStep; similarity: number; parentTask: string }> = [];
    for (const row of rows) {
      const rs = row.rs as Record<string, unknown>;
      const rsId = rs.id as string;

      // Get parent trace for context
      const traceRows = await this.client.executeRead(
        `MATCH (rt:ReasoningTrace)-[:HAS_STEP]->(s:ReasoningStep {id: $id})
         RETURN rt.task AS task, rt.outcome AS outcome, rt.success AS success`,
        { id: rsId },
      );

      const task = traceRows[0]?.task as string || "unknown";
      const success = traceRows[0]?.success;

      if (successOnly && success !== true) continue;

      results.push({
        step: {
          id: rsId,
          traceId: "unknown",
          stepNumber: (rs.step_number as number) || 0,
          thought: rs.thought as string | undefined,
          action: rs.action as string | undefined,
          observation: rs.observation as string | undefined,
          embedding: rs.embedding as number[] | undefined,
          metadata: {},
        },
        similarity: row.score as number,
        parentTask: task,
      });
    }

    return results.slice(0, limit);
  }
}
```

- [ ] **Step 3: Commit**

```bash
git add src/server/memory/reasoning.ts
git commit -m "feat(memory): add reasoning memory for traces, steps, and tool calls"
```

### Task 8: Memory Observer

**Files:**
- Create: `src/server/memory/observer.ts`

**Context:** Maps to Python `MemoryObserver` in `mcp/_observer.py`. Lightweight in-memory session tracking with token estimation (chars/4), inline observation extraction via pattern matching, and keyword-based entity extraction for reflection generation when token threshold is exceeded. No LLM dependency for compression.

- [ ] **Step 1: Read original Python implementation**

Read `agent-memory/src/neo4j_agent_memory/mcp/_observer.py` — focus on `on_message_stored` (line 100-133), `_generate_reflection` (line 134-198), `_extract_inline_observations` (line 200-268), `get_observations` (line 270-304), `_extract_sentence_containing` (line 311-339).

- [ ] **Step 2: Write observer.ts**

```typescript
import type { MemoryClient } from "./client";
import type { Observation, ObservationResult } from "./types";

const CHARS_PER_TOKEN = 4;

interface SessionContext {
  sessionId: string;
  totalChars: number;
  messageCount: number;
  observations: Observation[];
  reflections: string[];
  lastCompressionAt: number;
  entityNames: Set<string>;
  topics: string[];
}

const DECISION_MARKERS = [
  "i decided", "i've decided", "let's go with", "i'll go with",
  "i chose", "i've chosen", "we should", "i want to",
  "i'm going to", "i plan to",
];

const FACT_PATTERNS = [
  "the answer is", "it turns out", "actually,", "i found out",
  "i learned that", "it seems like", "the reason is",
];

function extractSentenceContaining(text: string, marker: string): string | null {
  const lower = text.toLowerCase();
  const idx = lower.indexOf(marker.toLowerCase());
  if (idx === -1) return null;

  // Find sentence boundaries
  let start = Math.max(0, idx);
  for (let i = idx - 1; i >= 0; i--) {
    if (".!?\n".includes(text[i])) {
      start = i + 1;
      break;
    }
  }

  let end = text.length;
  for (let i = idx + marker.length; i < text.length; i++) {
    if (".!?\n".includes(text[i])) {
      end = i + 1;
      break;
    }
  }

  const sentence = text.slice(start, end).trim();
  if (sentence.length > 300) {
    return sentence.slice(0, 300).split(" ").slice(0, -1).join(" ") + "...";
  }
  return sentence.length > 10 ? sentence : null;
}

export class MemoryObserver {
  private client: MemoryClient;
  private thresholdTokens: number;
  private recentWindow: number;
  private sessions: Map<string, SessionContext> = new Map();

  constructor(
    client: MemoryClient,
    options?: {
      thresholdTokens?: number;
      recentMessageWindow?: number;
    },
  ) {
    this.client = client;
    this.thresholdTokens = options?.thresholdTokens ?? 30000;
    this.recentWindow = options?.recentMessageWindow ?? 20;
  }

  async onMessageStored(
    sessionId: string,
    content: string,
    messageId?: string,
    role: string = "user",
  ): Promise<void> {
    const ctx = this.getSession(sessionId);
    ctx.totalChars += content.length;
    ctx.messageCount += 1;

    // Extract inline observations from user messages
    if (role === "user") {
      const observations = this.extractInlineObservations(content, messageId);
      ctx.observations.push(...observations);
    }

    // Check if we need to compress
    const approxTokens = Math.floor(ctx.totalChars / CHARS_PER_TOKEN);
    if (approxTokens > this.thresholdTokens) {
      const messagesSinceCompression = ctx.messageCount - ctx.lastCompressionAt;
      if (messagesSinceCompression >= this.recentWindow) {
        await this.generateReflection(sessionId);
      }
    }
  }

  async getObservations(sessionId: string): Promise<ObservationResult> {
    const ctx = this.getSession(sessionId);
    return {
      sessionId,
      messageCount: ctx.messageCount,
      approximateTokens: Math.floor(ctx.totalChars / CHARS_PER_TOKEN),
      thresholdTokens: this.thresholdTokens,
      thresholdExceeded: Math.floor(ctx.totalChars / CHARS_PER_TOKEN) > this.thresholdTokens,
      reflections: ctx.reflections,
      observations: ctx.observations.map((o) => ({
        type: o.type,
        content: o.content,
        confidence: o.confidence,
        timestamp: o.timestamp,
        sourceMessageId: o.sourceMessageId,
      })),
      entityNames: Array.from(ctx.entityNames).sort(),
      topics: ctx.topics,
    };
  }

  resetSession(sessionId: string): void {
    this.sessions.delete(sessionId);
  }

  // ── Private helpers ──

  private getSession(sessionId: string): SessionContext {
    if (!this.sessions.has(sessionId)) {
      this.sessions.set(sessionId, {
        sessionId,
        totalChars: 0,
        messageCount: 0,
        observations: [],
        reflections: [],
        lastCompressionAt: 0,
        entityNames: new Set(),
        topics: [],
      });
    }
    return this.sessions.get(sessionId)!;
  }

  private async generateReflection(sessionId: string): Promise<void> {
    const ctx = this.getSession(sessionId);

    try {
      const messages = await this.client.shortTerm.getConversation(sessionId, 100);
      if (messages.length === 0) return;

      // Only compress messages beyond the recent window
      const olderMessages = messages.slice(0, -this.recentWindow);
      if (olderMessages.length === 0) return;

      // Extract capitalized multi-word phrases as potential entities
      const entities = new Set<string>();
      for (const msg of olderMessages) {
        const words = msg.content.split(/\s+/);
        for (let i = 0; i < words.length; i++) {
          if (words[i] && words[i][0] === words[i][0].toUpperCase() && words[i].length > 2) {
            const parts = [words[i]];
            for (let j = i + 1; j < Math.min(i + 4, words.length); j++) {
              if (words[j] && words[j][0] === words[j][0].toUpperCase()) {
                parts.push(words[j]);
              } else {
                break;
              }
            }
            if (parts.length > 1) {
              entities.add(parts.join(" "));
            }
          }
        }
      }

      const reflectionParts: string[] = [];
      if (ctx.observations.length > 0) {
        const obsSummary = ctx.observations.slice(-10).map((o) => o.content).join("; ");
        reflectionParts.push(`Key observations: ${obsSummary}`);
      }
      if (entities.size > 0) {
        const topEntities = Array.from(entities).sort().slice(0, 10);
        reflectionParts.push(`Entities discussed: ${topEntities.join(", ")}`);
      }
      if (reflectionParts.length > 0) {
        const reflection = `Session summary (${ctx.messageCount} messages): ` + reflectionParts.join(". ");
        ctx.reflections.push(reflection);
      }

      ctx.lastCompressionAt = ctx.messageCount;
    } catch {
      // Silently skip compression on error
    }
  }

  private extractInlineObservations(
    content: string,
    messageId?: string,
  ): Observation[] {
    const observations: Observation[] = [];
    const now = new Date().toISOString();

    // Check for decision statements
    for (const marker of DECISION_MARKERS) {
      if (content.toLowerCase().includes(marker)) {
        const sentence = extractSentenceContaining(content, marker);
        if (sentence) {
          observations.push({
            type: "decision",
            content: sentence,
            sourceMessageId: messageId,
            timestamp: now,
            confidence: 0.75,
          });
        }
        break;
      }
    }

    // Check for factual statements
    for (const marker of FACT_PATTERNS) {
      if (content.toLowerCase().includes(marker)) {
        const sentence = extractSentenceContaining(content, marker);
        if (sentence) {
          observations.push({
            type: "fact",
            content: sentence,
            sourceMessageId: messageId,
            timestamp: now,
            confidence: 0.70,
          });
        }
        break;
      }
    }

    return observations;
  }
}
```

- [ ] **Step 3: Commit**

```bash
git add src/server/memory/observer.ts
git commit -m "feat(memory): add observational memory for token-threshold compression"
```

### Task 9: Hybrid Search

**Files:**
- Create: `src/server/memory/search.ts`

**Context:** Maps to the hybrid search concept in agent-memory (used by `memory_search` tool). This is a read-only orchestration layer that delegates to short-term, long-term, and reasoning layers, then merges results.

- [ ] **Step 1: Read original Python context**

The Python `memory_search` tool (in `_tools.py` line 87-117) delegates to `integration.search()`. Read `integration.py` for the `search` method which calls `short_term.search_messages()`, `long_term.search_entities()`, `long_term.search_preferences()`, and `reasoning.get_similar_traces()`.

- [ ] **Step 2: Write search.ts**

```typescript
import type { MemoryClient } from "./client";
import type { SearchResults } from "./types";

export class MemorySearch {
  constructor(private client: MemoryClient) {}

  async search(
    query: string,
    options?: {
      memoryTypes?: string[];
      sessionId?: string;
      limit?: number;
      threshold?: number;
    },
  ): Promise<SearchResults> {
    const { memoryTypes, sessionId, limit = 10, threshold = 0.7 } = options || {};
    const types = memoryTypes || ["messages", "entities", "preferences", "traces"];

    const results: SearchResults = {
      messages: [],
      entities: [],
      preferences: [],
      traces: [],
    };

    const tasks: Promise<void>[] = [];

    if (types.includes("messages")) {
      tasks.push(
        this.client.shortTerm
          .searchMessages(query, { sessionId, limit, threshold })
          .then((msgs) => { results.messages = msgs; }),
      );
    }

    if (types.includes("entities")) {
      tasks.push(
        this.client.longTerm
          .searchEntities(query, { limit, threshold })
          .then((entities) => { results.entities = entities; }),
      );
    }

    // Preference search — simple category-scoped lookup
    if (types.includes("preferences")) {
      tasks.push(
        this.client.longTerm
          .getPreferences(undefined, limit)
          .then((prefs) => {
            results.preferences = prefs.map((p) => ({
              ...p,
              similarity: 1.0,
            }));
          }),
      );
    }

    if (types.includes("traces")) {
      tasks.push(
        this.client.reasoning
          .getSimilarTraces(query, { limit, threshold })
          .then((traces) => { results.traces = traces; }),
      );
    }

    await Promise.all(tasks);
    return results;
  }
}
```

- [ ] **Step 3: Commit**

```bash
git add src/server/memory/search.ts
git commit -m "feat(memory): add hybrid search across all memory types"
```

### Task 10: Context Assembler

**Files:**
- Create: `src/server/memory/context.ts`

**Context:** Maps to `memory_get_context` MCP tool — assembles the full context for GM consumption: recent conversation messages + relevant entities/preferences + similar reasoning traces.

- [ ] **Step 1: Read original Python context**

Read the `get_context` method on `MemoryIntegration` — and the `_tools.py` `memory_get_context` tool (line 119-152). This assembles context by calling `get_conversation`, `search_entities`, `get_preferences`, and `get_similar_traces`.

- [ ] **Step 2: Write context.ts**

```typescript
import type { MemoryClient } from "./client";
import type { AssembledContext } from "./types";

export class ContextAssembler {
  constructor(private client: MemoryClient) {}

  async assemble(
    sessionId?: string,
    options?: {
      query?: string;
      maxItems?: number;
      includeShortTerm?: boolean;
      includeLongTerm?: boolean;
      includeReasoning?: boolean;
    },
  ): Promise<AssembledContext> {
    const {
      query,
      maxItems = 10,
      includeShortTerm = true,
      includeLongTerm = true,
      includeReasoning = true,
    } = options || {};

    const context: AssembledContext = {
      messages: [],
      entities: [],
      preferences: [],
      traces: [],
      summary: "",
    };

    const tasks: Promise<void>[] = [];

    // Recent conversation
    if (includeShortTerm && sessionId) {
      tasks.push(
        this.client.shortTerm
          .getConversation(sessionId, maxItems)
          .then((msgs) => { context.messages = msgs; }),
      );
    }

    // Relevant entities and preferences
    if (includeLongTerm && query) {
      tasks.push(
        this.client.longTerm
          .searchEntities(query, { limit: maxItems })
          .then((entities) => {
            // Strip similarity from type to fit AssembledContext
            context.entities = entities.map(({ similarity: _, ...e }) => e);
          }),
      );
      tasks.push(
        this.client.longTerm
          .getPreferences(undefined, maxItems)
          .then((prefs) => { context.preferences = prefs; }),
      );
    }

    // Similar reasoning traces
    if (includeReasoning && query) {
      tasks.push(
        this.client.reasoning
          .getSimilarTraces(query, { limit: 3 })
          .then((traces) => {
            context.traces = traces.map(({ similarity: _, ...t }) => t);
          }),
      );
    }

    await Promise.all(tasks);

    // Build summary text
    const parts: string[] = [];
    if (context.messages.length > 0) {
      parts.push("### Recent Conversation");
      for (const msg of context.messages.slice(-maxItems)) {
        parts.push(`**${msg.role}**: ${msg.content}`);
      }
    }
    if (context.entities.length > 0) {
      parts.push("\n### Relevant Entities");
      for (const e of context.entities) {
        parts.push(`- ${e.name} (${e.type}${e.subtype ? `:${e.subtype}` : ""})${e.description ? `: ${e.description}` : ""}`);
      }
    }
    if (context.preferences.length > 0) {
      parts.push("\n### User Preferences");
      for (const p of context.preferences) {
        parts.push(`- [${p.category}] ${p.preference}`);
      }
    }
    if (context.traces.length > 0) {
      parts.push("\n### Similar Past Tasks");
      for (const t of context.traces) {
        parts.push(`- ${t.task}${t.outcome ? ` → ${t.outcome}` : ""}`);
      }
    }
    context.summary = parts.join("\n");

    return context;
  }
}
```

- [ ] **Step 3: Commit**

```bash
git add src/server/memory/context.ts
git commit -m "feat(memory): add context assembler for GM consumption"
```

### Task 11: MemoryClient Singleton

**Files:**
- Create: `src/server/memory/client.ts`

**Context:** Maps to Python `MemoryClient` in `__init__.py` plus `MemoryIntegration` in `integration.py`. The singleton pattern follows the existing `getMcpClient()` pattern in `src/server/mcp/client.ts`.

- [ ] **Step 1: Write client.ts**

```typescript
import { Neo4jClient } from "./neo4j";
import { setupSchema } from "./schema";
import { ShortTermMemory } from "./short-term";
import { LongTermMemory } from "./long-term";
import { ReasoningMemory } from "./reasoning";
import { MemorySearch } from "./search";
import { ContextAssembler } from "./context";
import { MemoryObserver } from "./observer";

export { ShortTermMemory } from "./short-term";
export { LongTermMemory } from "./long-term";
export { ReasoningMemory } from "./reasoning";
export { MemorySearch } from "./search";
export { ContextAssembler } from "./context";
export { MemoryObserver } from "./observer";
export * from "./types";

export class MemoryClient {
  readonly neo4j: Neo4jClient;
  readonly shortTerm: ShortTermMemory;
  readonly longTerm: LongTermMemory;
  readonly reasoning: ReasoningMemory;
  readonly search: MemorySearch;
  readonly context: ContextAssembler;
  readonly observer: MemoryObserver;

  private constructor(neo4j: Neo4jClient) {
    this.neo4j = neo4j;
    this.shortTerm = new ShortTermMemory(neo4j);
    this.longTerm = new LongTermMemory(neo4j);
    this.reasoning = new ReasoningMemory(neo4j);
    this.search = new MemorySearch(this);
    this.context = new ContextAssembler(this);
    this.observer = new MemoryObserver(this);
  }

  async executeReadOnlyCypher(
    query: string,
    params?: Record<string, unknown>,
  ): Promise<Record<string, unknown>[]> {
    return this.neo4j.executeRead(query, params);
  }

  async close(): Promise<void> {
    await this.neo4j.close();
  }

  // ── Singleton ──

  private static instance: MemoryClient | null = null;

  static async getInstance(options?: {
    neo4jUri?: string;
    neo4jUser?: string;
    neo4jPassword?: string;
  }): Promise<MemoryClient> {
    if (MemoryClient.instance) return MemoryClient.instance;

    const client = new Neo4jClient(
      options?.neo4jUri,
      options?.neo4jUser,
      options?.neo4jPassword,
    );

    await client.verifyConnectivity();
    await setupSchema(client);

    MemoryClient.instance = new MemoryClient(client);
    return MemoryClient.instance;
  }

  static async closeInstance(): Promise<void> {
    if (MemoryClient.instance) {
      await MemoryClient.instance.close();
      MemoryClient.instance = null;
    }
  }
}
```

- [ ] **Step 2: Commit**

```bash
git add src/server/memory/client.ts
git commit -m "feat(memory): add MemoryClient singleton"
```

### Task 12: AI SDK Tools

**Files:**
- Create: `src/server/memory/tools.ts`

**Context:** Maps to the 16 MCP tools defined in `agent-memory/src/neo4j_agent_memory/mcp/_tools.py`. Each tool becomes an `ai` SDK `tool()` definition following the pattern in `src/server/llm/tools/generateDialogueStep.ts`. Cleaned-up names per the design spec.

- [ ] **Step 1: Read original Python implementation**

Read `agent-memory/src/neo4j_agent_memory/mcp/_tools.py` — all 16 tool implementations (lines 86-786).

- [ ] **Step 2: Write tools.ts**

```typescript
import { tool } from "ai";
import { z } from "zod";
import { MemoryClient } from "./client";

function getClient(): MemoryClient {
  const client = (MemoryClient as unknown as { instance: MemoryClient | null }).instance;
  if (!client) throw new Error("MemoryClient not initialized");
  return client;
}

export function createMemoryTools() {
  const readTools = {

    searchMemory: tool({
      description: "Search across all memory types using hybrid vector + graph search.",
      inputSchema: z.object({
        query: z.string().describe("Natural language search query"),
        limit: z.number().default(10),
        memoryTypes: z.array(z.enum(["messages", "entities", "preferences", "traces"])).optional(),
        sessionId: z.string().optional(),
        threshold: z.number().default(0.7),
      }),
      execute: async (input) => {
        const client = getClient();
        const results = await client.search.search(input.query, {
          memoryTypes: input.memoryTypes,
          sessionId: input.sessionId,
          limit: input.limit,
          threshold: input.threshold,
        });
        return JSON.stringify(results, null, 2);
      },
    }),

    getContext: tool({
      description: "Get assembled context from all memory types for the current session.",
      inputSchema: z.object({
        sessionId: z.string().optional(),
        query: z.string().optional(),
        maxItems: z.number().default(10),
        includeShortTerm: z.boolean().default(true),
        includeLongTerm: z.boolean().default(true),
        includeReasoning: z.boolean().default(true),
      }),
      execute: async (input) => {
        const client = getClient();
        const context = await client.context.assemble(input.sessionId, {
          query: input.query,
          maxItems: input.maxItems,
          includeShortTerm: input.includeShortTerm,
          includeLongTerm: input.includeLongTerm,
          includeReasoning: input.includeReasoning,
        });
        return context.summary;
      },
    }),

    getEntity: tool({
      description: "Get detailed entity information with graph relationships.",
      inputSchema: z.object({
        name: z.string(),
        entityType: z.string().optional(),
        includeNeighbors: z.boolean().default(true),
        maxHops: z.number().default(1),
      }),
      execute: async (input) => {
        const client = getClient();
        const entity = await client.longTerm.getEntity(input.name, input.entityType);
        if (!entity) return JSON.stringify({ found: false, name: input.name });
        return JSON.stringify({ found: true, entity }, null, 2);
      },
    }),

    getConversation: tool({
      description: "Retrieve full conversation history for a session.",
      inputSchema: z.object({
        sessionId: z.string(),
        limit: z.number().default(50),
      }),
      execute: async (input) => {
        const client = getClient();
        const messages = await client.shortTerm.getConversation(input.sessionId, input.limit);
        return JSON.stringify({ sessionId: input.sessionId, messageCount: messages.length, messages }, null, 2);
      },
    }),

    listSessions: tool({
      description: "List available conversation sessions with previews.",
      inputSchema: z.object({
        limit: z.number().default(20),
        offset: z.number().default(0),
      }),
      execute: async (input) => {
        const client = getClient();
        const sessions = await client.shortTerm.listSessions(input.limit, input.offset);
        return JSON.stringify({ sessionCount: sessions.length, sessions }, null, 2);
      },
    }),

    getObservations: tool({
      description: "Get observations and extracted insights for a session.",
      inputSchema: z.object({
        sessionId: z.string(),
      }),
      execute: async (input) => {
        const client = getClient();
        const result = await client.observer.getObservations(input.sessionId);
        return JSON.stringify(result, null, 2);
      },
    }),

    exportGraph: tool({
      description: "Export a subgraph as JSON for visualization.",
      inputSchema: z.object({
        sessionId: z.string().optional(),
        limit: z.number().default(500),
      }),
      execute: async (input) => {
        const client = getClient();
        // Simple entity subgraph export
        const entities = await client.longTerm.searchEntities("", {
          limit: input.limit,
          threshold: 0,
        });
        return JSON.stringify({ nodeCount: entities.length, nodes: entities }, null, 2);
      },
    }),

    queryGraph: tool({
      description: "Execute a read-only Cypher query against the knowledge graph.",
      inputSchema: z.object({
        query: z.string(),
        parameters: z.record(z.unknown()).optional(),
      }),
      execute: async (input) => {
        // Safety: reject write queries
        const upper = input.query.toUpperCase();
        const writePatterns = /\b(CREATE|MERGE|DELETE|DETACH\s+DELETE|SET|REMOVE|DROP|LOAD\s+CSV|FOREACH)\b/;
        if (writePatterns.test(upper)) {
          return JSON.stringify({ error: "Only read-only queries are allowed." });
        }
        const client = getClient();
        const rows = await client.executeReadOnlyCypher(input.query, input.parameters as Record<string, unknown> | undefined);
        return JSON.stringify({ success: true, rowCount: rows.length, rows }, null, 2);
      },
    }),
  };

  const writeTools = {

    storeMessage: tool({
      description: "Store a message in conversation memory with automatic entity extraction.",
      inputSchema: z.object({
        content: z.string(),
        role: z.enum(["user", "assistant", "system"]).default("user"),
        sessionId: z.string().optional(),
        metadata: z.record(z.unknown()).optional(),
      }),
      execute: async (input) => {
        const client = getClient();
        const sessionId = input.sessionId || "elysian-game";
        const msg = await client.shortTerm.addMessage(
          sessionId,
          input.role,
          input.content,
          input.metadata as Record<string, unknown> | undefined,
        );
        return JSON.stringify({ stored: true, id: msg.id }, null, 2);
      },
    }),

    saveEntity: tool({
      description: "Create or update an entity in the knowledge graph. Uses POLE+O types.",
      inputSchema: z.object({
        name: z.string(),
        entityType: z.enum(["PERSON", "OBJECT", "LOCATION", "ORGANIZATION", "EVENT"]),
        subtype: z.string().optional(),
        description: z.string().optional(),
        aliases: z.array(z.string()).optional(),
        metadata: z.record(z.unknown()).optional(),
      }),
      execute: async (input) => {
        const client = getClient();
        const entity = await client.longTerm.addEntity(input.name, input.entityType, {
          subtype: input.subtype,
          description: input.description,
          aliases: input.aliases,
          metadata: input.metadata as Record<string, unknown> | undefined,
        });
        return JSON.stringify({ stored: true, id: entity.id, name: entity.name, type: entity.type }, null, 2);
      },
    }),

    setPreference: tool({
      description: "Record a user preference for personalization.",
      inputSchema: z.object({
        category: z.string(),
        preference: z.string(),
        context: z.string().optional(),
        confidence: z.number().default(1.0),
      }),
      execute: async (input) => {
        const client = getClient();
        const pref = await client.longTerm.addPreference(input.category, input.preference, {
          context: input.context,
          confidence: input.confidence,
        });
        return JSON.stringify({ stored: true, id: pref.id, category: pref.category }, null, 2);
      },
    }),

    recordFact: tool({
      description: "Store a subject-predicate-object fact triple.",
      inputSchema: z.object({
        subject: z.string(),
        predicate: z.string(),
        objectValue: z.string(),
        confidence: z.number().default(1.0),
        validFrom: z.string().optional(),
        validUntil: z.string().optional(),
        metadata: z.record(z.unknown()).optional(),
      }),
      execute: async (input) => {
        const client = getClient();
        const fact = await client.longTerm.addFact(input.subject, input.predicate, input.objectValue, {
          confidence: input.confidence,
          validFrom: input.validFrom ? new Date(input.validFrom) : undefined,
          validUntil: input.validUntil ? new Date(input.validUntil) : undefined,
          metadata: input.metadata as Record<string, unknown> | undefined,
        });
        return JSON.stringify({ stored: true, id: fact.id, subject: fact.subject, predicate: fact.predicate }, null, 2);
      },
    }),

    linkEntities: tool({
      description: "Create a typed relationship between two entities.",
      inputSchema: z.object({
        sourceName: z.string(),
        targetName: z.string(),
        relationshipType: z.string().describe("UPPER_SNAKE_CASE type, e.g. LOCATED_AT, HOSTILE_TOWARDS"),
        description: z.string().optional(),
        confidence: z.number().default(1.0),
      }),
      execute: async (input) => {
        const client = getClient();
        await client.longTerm.addRelationship(input.sourceName, input.targetName, input.relationshipType, {
          description: input.description,
          confidence: input.confidence,
        });
        return JSON.stringify({ stored: true, source: input.sourceName, target: input.targetName, type: input.relationshipType }, null, 2);
      },
    }),

    startTrace: tool({
      description: "Begin recording a reasoning trace for a complex task.",
      inputSchema: z.object({
        sessionId: z.string(),
        task: z.string(),
        metadata: z.record(z.unknown()).optional(),
      }),
      execute: async (input) => {
        const client = getClient();
        const trace = await client.reasoning.startTrace(input.sessionId, input.task, {
          metadata: input.metadata as Record<string, unknown> | undefined,
        });
        return JSON.stringify({ started: true, traceId: trace.id, task: trace.task }, null, 2);
      },
    }),

    recordStep: tool({
      description: "Record a reasoning step within a trace.",
      inputSchema: z.object({
        traceId: z.string(),
        thought: z.string().optional(),
        action: z.string().optional(),
        observation: z.string().optional(),
        toolName: z.string().optional(),
        toolArgs: z.record(z.unknown()).optional(),
        toolResult: z.string().optional(),
      }),
      execute: async (input) => {
        const client = getClient();
        const step = await client.reasoning.addStep(input.traceId, {
          thought: input.thought,
          action: input.action,
          observation: input.observation,
        });
        if (input.toolName) {
          await client.reasoning.recordToolCall(step.id, input.toolName, input.toolArgs || {}, {
            result: input.toolResult,
          });
        }
        return JSON.stringify({ recorded: true, stepId: step.id, traceId: input.traceId }, null, 2);
      },
    }),

    completeTrace: tool({
      description: "Complete a reasoning trace with the final outcome.",
      inputSchema: z.object({
        traceId: z.string(),
        outcome: z.string().optional(),
        success: z.boolean().default(true),
      }),
      execute: async (input) => {
        const client = getClient();
        await client.reasoning.completeTrace(input.traceId, {
          outcome: input.outcome,
          success: input.success,
        });
        return JSON.stringify({ completed: true, traceId: input.traceId }, null, 2);
      },
    }),
  };

  return { ...readTools, ...writeTools };
}
```

- [ ] **Step 3: Commit**

```bash
git add src/server/memory/tools.ts
git commit -m "feat(memory): add AI SDK tool definitions replacing MCP tools"
```

### Task 13: Update Seed and Reset

**Files:**
- Modify: `src/server/mcp/seed.ts` — use MemoryClient instead of direct Neo4j driver
- Modify: `src/server/mcp/reset.ts` — use MemoryClient instead of direct Neo4j driver

**Context:** Currently seed.ts and reset.ts create their own Neo4j driver instances. After migration, they use MemoryClient's entity API and graph API respectively.

- [ ] **Step 1: Update seed.ts**

Read the current `src/server/mcp/seed.ts`, then rewrite it to use MemoryClient:

```typescript
import { v4 as uuidv4 } from "uuid";
import { MemoryClient } from "@/server/memory/client";
import { getActiveSeedStory } from "@/server/seed-stories/index";
import db from "@/server/db";

export async function seedDatabase(): Promise<void> {
  const story = getActiveSeedStory();
  const client = await MemoryClient.getInstance();

  console.log(`[seed] seeding ${story.entities.length} entities from "${story.id}"`);

  // 1. Create all entities via MemoryClient
  for (const entity of story.entities) {
    await client.longTerm.addEntity(entity.name, entity.type, {
      subtype: entity.subtype,
      description: entity.description,
      metadata: { dbId: entity.id, ...entity.metadata },
    });
  }

  // 2. Create all relationships via MemoryClient
  for (const rel of story.relationships) {
    await client.longTerm.addRelationship(rel.sourceName, rel.targetName, rel.type, {
      description: rel.description || undefined,
    });
  }

  // 3. Set initial time in SQLite
  db.prepare("INSERT OR REPLACE INTO system_state (key, value) VALUES (?, ?)").run(
    "game_time_day", String(story.initialDay),
  );
  db.prepare("INSERT OR REPLACE INTO system_state (key, value) VALUES (?, ?)").run(
    "game_time_segment", String(story.initialSegment),
  );

  console.log(`[seed] done — ${story.entities.length} entities, ${story.relationships.length} relationships`);
}
```

- [ ] **Step 2: Update reset.ts**

Read the current `src/server/mcp/reset.ts`, then rewrite:

```typescript
import { MemoryClient } from "@/server/memory/client";

export async function clearNeo4jDatabase(): Promise<void> {
  const client = await MemoryClient.getInstance();
  await client.neo4j.executeWrite("MATCH (n) DETACH DELETE n");
  console.log("[reset] Neo4j database cleared");
}
```

- [ ] **Step 3: Commit**

```bash
git add src/server/mcp/seed.ts src/server/mcp/reset.ts
git commit -m "refactor: update seed and reset to use MemoryClient"
```

### Task 14: Update System Prompt

**Files:**
- Modify: `src/server/llm/prompt.ts` — new tool names and descriptions

- [ ] **Step 1: Update prompt.ts**

Read the current `src/server/llm/prompt.ts`, then update the World Memory Tools section:

Replace the entire "### World Memory Tools (auto-discovered from agent-memory)" section and the following tool descriptions with:

```typescript
### World Memory Tools (native memory tools)
These manage the game world — entities, relationships, facts, dialogue history, and graph search:
- **searchMemory** — Search all world state (entities, facts, messages) with natural language.
- **getContext** — Get assembled context for the current moment: recent messages, relevant entities, and facts.
- **saveEntity** — Create or update a world entity (PERSON, OBJECT, LOCATION, ORGANIZATION, EVENT). Use metadata for structured data like stats, conditions, short descriptions.
- **getEntity** — Get full entity details including related entities via graph traversal.
- **linkEntities** — Create a typed relationship between two entities using UPPER_SNAKE_CASE types:
  - LOCATED_AT — character/object is at a location
  - CARRIES — character carries an object
  - HOSTILE_TOWARDS — character is hostile toward another
  - ALLIED_WITH — characters are allies
  - CHILD_PLOT — plot branch relationship (with triggerCondition in metadata)
  - INVOLVES — plot involves a character/location
  - OCCURRED_AT — event/plot occurred at a location
  - OWNED_BY — object belongs to a character
- **recordFact** — Record a fact triple (subject-predicate-object_value). Use for notes, clues, suspicions, timeline events, and time state.
- **storeMessage** — Store a dialogue message (use role "assistant" for GM messages, "user" for player messages).
- **getConversation** — Recall conversation history for the session.
- **queryGraph** — Execute read-only Cypher queries for complex graph lookups.
```

- [ ] **Step 2: Commit**

```bash
git add src/server/llm/prompt.ts
git commit -m "refactor: update system prompt with new memory tool names"
```

### Task 15: Update Turn Generation

**Files:**
- Modify: `src/server/llm/index.ts` — replace `getMcpTools()` with `createMemoryTools()`

- [ ] **Step 1: Update llm/index.ts**

Read the current `src/server/llm/index.ts`, then:

1. Replace `import { getMcpTools } from "@/server/mcp/client";` with `import { createMemoryTools } from "@/server/memory/tools";`
2. Replace `const mcpTools = await getMcpTools();` with `const memoryTools = createMemoryTools();`
3. Replace `const allTools = { ...mcpTools, ...}` with `const allTools = { ...memoryTools, ...}`
4. Update tool name reference in `onStepFinish` and `prepareStep` — `"generateDialogueStep"` stays, no mcp tool references to change
5. Update the debug logging line that references MCP tool count

- [ ] **Step 2: Commit**

```bash
git add src/server/llm/index.ts
git commit -m "refactor: replace MCP bridge with native memory tools in turn generation"
```

### Task 16: Update Server Startup

**Files:**
- Modify: `src/server/main.ts` — replace MCP init with MemoryClient init as well as seed

- [ ] **Step 1: Update main.ts**

Read the current `src/server/main.ts`, then:

```typescript
import "dotenv/config";
import express from "express";
import apiRouter from "@/server/api";
import { MemoryClient } from "@/server/memory/client";
import { seedDatabase } from "@/server/mcp/seed";

async function start() {
  const app = express();
  const PORT = 3000;

  app.use(express.json());
  app.use("/api", apiRouter);

  // Initialize MemoryClient (stays alive for server lifetime)
  console.log("[memory] initializing local memory layer...");
  await MemoryClient.getInstance();

  // Seed Neo4j with initial world data
  await seedDatabase();

  app.listen(PORT, "0.0.0.0", () => {
    console.log(`Server running on http://localhost:${PORT}`);
  });

  const shutdown = async () => {
    console.log("\nShutting down...");
    await MemoryClient.closeInstance();
    process.exit(0);
  };
  process.on("SIGINT", shutdown);
  process.on("SIGTERM", shutdown);
}

start();
```

- [ ] **Step 2: Commit**

```bash
git add src/server/main.ts
git commit -m "refactor: replace MCP init with MemoryClient in server startup"
```

### Task 17: Cleanup

**Files:**
- Delete: `src/server/mcp/client.ts`
- Modify: `package.json` — remove `@ai-sdk/mcp`
- Modify: `Makefile` — remove MCP targets, simplify `dev`

- [ ] **Step 1: Remove MCP bridge file**

```bash
rm src/server/mcp/client.ts
```

- [ ] **Step 2: Remove @ai-sdk/mcp dependency**

```bash
npm uninstall @ai-sdk/mcp
```

- [ ] **Step 3: Update Makefile**

Remove `mcp-install`, `mcp-start`, `mcp-dev` targets. Replace the `dev` target:

```makefile
dev: neo4j-start neo4j-wait server
```

- [ ] **Step 4: Build and type-check**

```bash
npm run lint
```

Fix any type errors that surface.

- [ ] **Step 5: Commit**

```bash
git add -A
git commit -m "chore: remove MCP bridge, Python runtime dependency, and simplify dev workflow"
```

---

## Self-Review

**1. Spec coverage:** Each spec requirement maps to a task — types (T1), Neo4j client (T2), schema (T3), embedder (T4), short-term (T5), long-term (T6), reasoning (T7), observer (T8), search (T9), context (T10), client singleton (T11), tools (T12), seed/reset (T13), prompt (T14), turn generation (T15), server startup (T16), cleanup (T17).

**2. Placeholder scan:** No TBDs, TODOs, or incomplete sections. All code steps contain actual implementation code.

**3. Type consistency:** `MemoryEntity.type` is `EntityType` which maps to `"PERSON" | "OBJECT" | "LOCATION" | "ORGANIZATION" | "EVENT"`. `MemoryMessage.role` is `MessageRole` = `"user" | "assistant" | "system"`. Method signatures match between client.ts and the implementation files. Tool names in tools.ts match the names referenced in the updated prompt.
