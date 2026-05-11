import { tool } from "ai";
import { z } from "zod";
import { MemoryClient } from "./client";

function getClient(): MemoryClient {
  return MemoryClient.getCachedInstance();
}

/** DeepSeek serializes integers as floats (e.g. 20 → 20.0). Neo4j rejects
 *  floats in LIMIT/SKIP clauses. Truncate to integer before Cypher use. */
function sanitizeInt(n: number, fallback: number = 1): number {
  if (typeof n !== "number" || !Number.isFinite(n)) return fallback;
  const int = Math.floor(n);
  return int >= 0 ? int : fallback;
}

/** Clamp confidence values to [0, 1] range for Neo4j storage. */
function sanitizeConfidence(n: number): number {
  if (typeof n !== "number" || !Number.isFinite(n)) return 1.0;
  if (n < 1e-6) return 0;
  if (n > 1) return 1;
  return n;
}

export function createMemoryTools() {
  const readTools = {
    searchMemory: tool({
      description: "Search across all memory types using hybrid vector + graph search.",
      inputSchema: z.object({
        query: z.string().describe("Natural language search query"),
        limit: z.number().default(10).describe("Max results per memory type"),
        memoryTypes: z.array(z.enum(["messages", "entities", "preferences", "traces"])).optional(),
        sessionId: z.string().optional(),
        threshold: z.number().default(0.7).describe("Similarity threshold 0-1"),
      }),
      execute: async (input) => {
        const client = getClient();
        const results = await client.search.search(input.query, {
          memoryTypes: input.memoryTypes,
          sessionId: input.sessionId,
          limit: sanitizeInt(input.limit, 10),
          threshold: input.threshold,
        });
        return JSON.stringify(results, null, 2);
      },
    }),

    getContext: tool({
      description: "Get assembled context from all memory types for the current session.",
      inputSchema: z.object({
        sessionId: z.string().optional().describe("Session to get context for"),
        query: z.string().optional().describe("Focus context retrieval"),
        maxItems: z.number().default(10).describe("Max items per memory type"),
        includeShortTerm: z.boolean().default(true),
        includeLongTerm: z.boolean().default(true),
        includeReasoning: z.boolean().default(true),
      }),
      execute: async (input) => {
        const client = getClient();
        const ctx = await client.context.assemble(input.sessionId, {
          query: input.query,
          maxItems: sanitizeInt(input.maxItems, 10),
          includeShortTerm: input.includeShortTerm,
          includeLongTerm: input.includeLongTerm,
          includeReasoning: input.includeReasoning,
        });
        return ctx.summary;
      },
    }),

    getEntity: tool({
      description: "Get detailed entity information with graph relationships.",
      inputSchema: z.object({
        name: z.string().describe("Entity name to look up"),
        entityType: z.string().optional().describe("Filter by POLE+O type"),
        includeNeighbors: z.boolean().default(true),
        maxHops: z.number().default(1).describe("Traversal depth 1-3"),
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
        sessionId: z.string().describe("Session ID to retrieve"),
        limit: z.number().default(50).describe("Max messages"),
      }),
      execute: async (input) => {
        const client = getClient();
        const messages = await client.shortTerm.getConversation(input.sessionId, sanitizeInt(input.limit, 50));
        return JSON.stringify({ sessionId: input.sessionId, messageCount: messages.length, messages }, null, 2);
      },
    }),

    listSessions: tool({
      description: "List available conversation sessions with previews.",
      inputSchema: z.object({
        limit: z.number().default(20).describe("Max sessions"),
        offset: z.number().default(0).describe("Pagination offset"),
      }),
      execute: async (input) => {
        const client = getClient();
        const sessions = await client.shortTerm.listSessions(sanitizeInt(input.limit, 20), sanitizeInt(input.offset, 0));
        return JSON.stringify({ sessionCount: sessions.length, sessions }, null, 2);
      },
    }),

    getObservations: tool({
      description: "Get observations and extracted insights for a session.",
      inputSchema: z.object({
        sessionId: z.string().describe("Session ID"),
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
        limit: z.number().default(500).describe("Max nodes"),
      }),
      execute: async (input) => {
        const client = getClient();
        const entities = await client.longTerm.searchEntities("", { limit: sanitizeInt(input.limit, 500), threshold: 0 });
        return JSON.stringify({ nodeCount: entities.length, nodes: entities }, null, 2);
      },
    }),

    queryGraph: tool({
      description: "Execute a read-only Cypher query against the knowledge graph.",
      inputSchema: z.object({
        query: z.string().describe("Cypher query (read-only)"),
        parameters: z.record(z.string(), z.unknown()).optional().describe("Query parameters"),
      }),
      execute: async (input) => {
        const upper = input.query.toUpperCase();
        const writePattern = /\b(CREATE|MERGE|DELETE|DETACH\s+DELETE|SET|REMOVE|DROP|LOAD\s+CSV|FOREACH)\b/;
        if (writePattern.test(upper)) {
          return JSON.stringify({ error: "Only read-only queries are allowed." });
        }
        const client = getClient();
        const rows = await client.executeReadOnlyCypher(
          input.query,
          input.parameters as Record<string, unknown> | undefined,
        );
        return JSON.stringify({ success: true, rowCount: rows.length, rows }, null, 2);
      },
    }),
  };

  const writeTools = {
    storeMessage: tool({
      description: "Store a message in conversation memory.",
      inputSchema: z.object({
        content: z.string().describe("Message text content"),
        role: z.enum(["user", "assistant", "system"]).default("user"),
        sessionId: z.string().optional().describe("Session ID (defaults to elysian-game)"),
        metadata: z.record(z.string(), z.unknown()).optional(),
      }),
      execute: async (input) => {
        const client = getClient();
        const sessionId = input.sessionId || "elysian-game";
        const msg = await client.shortTerm.addMessage(
          sessionId, input.role, input.content,
          input.metadata as Record<string, unknown> | undefined,
        );
        await client.observer.onMessageStored(sessionId, input.content, msg.id, input.role);
        return JSON.stringify({ stored: true, id: msg.id }, null, 2);
      },
    }),

    saveEntity: tool({
      description:
        "Create or update an entity in the knowledge graph. Uses POLE+O types: PERSON, OBJECT, LOCATION, ORGANIZATION, EVENT.",
      inputSchema: z.object({
        name: z.string().describe("Entity name"),
        entityType: z
          .enum(["PERSON", "OBJECT", "LOCATION", "ORGANIZATION", "EVENT"])
          .describe("POLE+O type"),
        subtype: z.string().optional().describe("Optional subtype (e.g., VEHICLE, CHARACTER)"),
        description: z.string().optional().describe("Entity description"),
        aliases: z.array(z.string()).optional().describe("Alternative names"),
        metadata: z.record(z.string(), z.unknown()).optional().describe("Additional metadata"),
      }),
      execute: async (input) => {
        const client = getClient();
        const entity = await client.longTerm.addEntity(input.name, input.entityType, {
          subtype: input.subtype,
          description: input.description,
          aliases: input.aliases,
          metadata: input.metadata as Record<string, unknown> | undefined,
        });
        return JSON.stringify(
          { stored: true, id: entity.id, name: entity.name, type: entity.type },
          null, 2,
        );
      },
    }),

    setPreference: tool({
      description: "Record a user preference for personalization.",
      inputSchema: z.object({
        category: z.string().describe("Preference category (e.g., food, music)"),
        preference: z.string().describe("The preference text"),
        context: z.string().optional().describe("When/where preference applies"),
        confidence: z.number().default(1.0).describe("Confidence 0-1"),
      }),
      execute: async (input) => {
        const client = getClient();
        const pref = await client.longTerm.addPreference(input.category, input.preference, {
          context: input.context,
          confidence: sanitizeConfidence(input.confidence),
        });
        return JSON.stringify(
          { stored: true, id: pref.id, category: pref.category },
          null, 2,
        );
      },
    }),

    recordFact: tool({
      description: "Store a subject-predicate-object fact triple.",
      inputSchema: z.object({
        subject: z.string().describe("Fact subject"),
        predicate: z.string().describe("Fact predicate/relationship"),
        objectValue: z.string().describe("Fact object"),
        confidence: z.number().default(1.0).describe("Confidence 0-1"),
        validFrom: z.string().optional().describe("ISO date for validity start"),
        validUntil: z.string().optional().describe("ISO date for validity end"),
        metadata: z.record(z.string(), z.unknown()).optional(),
      }),
      execute: async (input) => {
        const client = getClient();
        const fact = await client.longTerm.addFact(input.subject, input.predicate, input.objectValue, {
          confidence: sanitizeConfidence(input.confidence),
          validFrom: input.validFrom ? new Date(input.validFrom) : undefined,
          validUntil: input.validUntil ? new Date(input.validUntil) : undefined,
          metadata: input.metadata as Record<string, unknown> | undefined,
        });
        return JSON.stringify(
          { stored: true, id: fact.id, subject: fact.subject, predicate: fact.predicate },
          null, 2,
        );
      },
    }),

    linkEntities: tool({
      description:
        "Create a typed relationship between two entities. Use UPPER_SNAKE_CASE types: LOCATED_AT, CARRIES, HOSTILE_TOWARDS, ALLIED_WITH, etc.",
      inputSchema: z.object({
        sourceName: z.string().describe("Source entity name"),
        targetName: z.string().describe("Target entity name"),
        relationshipType: z.string().describe("Relationship type in UPPER_SNAKE_CASE"),
        description: z.string().optional().describe("Relationship description"),
        confidence: z.number().default(1.0).describe("Confidence 0-1"),
      }),
      execute: async (input) => {
        const client = getClient();
        const relResult = await client.longTerm.addRelationship(
          input.sourceName, input.targetName, input.relationshipType,
          { description: input.description, confidence: sanitizeConfidence(input.confidence) },
        );
        return JSON.stringify(
          { stored: true, created: relResult.created, source: input.sourceName, target: input.targetName, type: input.relationshipType },
          null, 2,
        );
      },
    }),

    startTrace: tool({
      description: "Begin recording a reasoning trace for a complex task.",
      inputSchema: z.object({
        sessionId: z.string().describe("Session ID"),
        task: z.string().describe("Task description"),
        metadata: z.record(z.string(), z.unknown()).optional(),
      }),
      execute: async (input) => {
        const client = getClient();
        const trace = await client.reasoning.startTrace(input.sessionId, input.task, {
          metadata: input.metadata as Record<string, unknown> | undefined,
        });
        return JSON.stringify(
          { started: true, traceId: trace.id, task: trace.task },
          null, 2,
        );
      },
    }),

    recordStep: tool({
      description: "Record a reasoning step within a trace.",
      inputSchema: z.object({
        traceId: z.string().describe("Parent trace ID"),
        thought: z.string().optional().describe("Agent's reasoning"),
        action: z.string().optional().describe("Action taken"),
        observation: z.string().optional().describe("Observation from action"),
        toolName: z.string().optional().describe("Name of tool called in this step"),
        toolArgs: z.record(z.string(), z.unknown()).optional().describe("Tool arguments"),
        toolResult: z.string().optional().describe("Tool call result"),
      }),
      execute: async (input) => {
        const client = getClient();
        const step = await client.reasoning.addStep(input.traceId, {
          thought: input.thought,
          action: input.action,
          observation: input.observation,
        });
        if (input.toolName) {
          await client.reasoning.recordToolCall(
            step.id, input.toolName, input.toolArgs || {},
            { result: input.toolResult },
          );
        }
        return JSON.stringify(
          { recorded: true, stepId: step.id, traceId: input.traceId },
          null, 2,
        );
      },
    }),

    completeTrace: tool({
      description: "Complete a reasoning trace with the final outcome.",
      inputSchema: z.object({
        traceId: z.string().describe("Trace ID to complete"),
        outcome: z.string().optional().describe("Final outcome description"),
        success: z.boolean().default(true).describe("Whether the task succeeded"),
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
