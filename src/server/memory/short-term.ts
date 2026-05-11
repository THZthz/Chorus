/**
 * Elysian Dialogue — cinematic RPG-style dialogue engine
 * Copyright (C) 2026  Amias
 *
 * This program is free software: you can redistribute it and/or modify
 * it under the terms of the GNU Affero General Public License as published by
 * the Free Software Foundation, either version 3 of the License, or
 * (at your option) any later version.
 *
 * This program is distributed in the hope that it will be useful,
 * but WITHOUT ANY WARRANTY; without even the implied warranty of
 * MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
 * GNU Affero General Public License for more details.
 *
 * You should have received a copy of the GNU Affero General Public License
 * along with this program.  If not, see <https://www.gnu.org/licenses/>.
 */

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
    const convId = await this.ensureConversation(sessionId);

    let embedding: number[] | undefined;
    if (generateEmbedding) {
      embedding = await this.embedder.embed(content);
    }

    const messageId = uuidv4();
    const now = new Date().toISOString();

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

  async getConversation(sessionId: string, limit: number = 1000): Promise<MemoryMessage[]> {
    const rows = await this.client.executeRead(
      `MATCH (c:Conversation {session_id: $sessionId})
       MATCH (c)-[:HAS_MESSAGE]->(m:Message)
       RETURN m ORDER BY m.timestamp DESC LIMIT $limit`,
      { sessionId, limit },
    );

    return rows.reverse().map((r) => {
      const m = r.m as Record<string, unknown>;
      return {
        id: m.id as string,
        sessionId,
        role: m.role as "user" | "assistant" | "system",
        content: m.content as string,
        metadata: m.metadata ? JSON.parse(m.metadata as string) : {},
        embedding: m.embedding as number[] | undefined,
        createdAt: toDate(m.timestamp),
      };
    });
  }

  async listSessions(limit: number = 20, offset: number = 0): Promise<SessionSummary[]> {
    const rows = await this.client.executeRead(
      `MATCH (c:Conversation)
       OPTIONAL MATCH (c)-[:FIRST_MESSAGE]->(first:Message)
       OPTIONAL MATCH (c)-[:HAS_MESSAGE]->(m:Message)
       WITH c, first, count(m) AS messageCount
       ORDER BY coalesce(c.updated_at, c.created_at) DESC
       SKIP $offset LIMIT $limit
       OPTIONAL MATCH (c)-[:HAS_MESSAGE]->(lastMsg:Message)
       WITH c, first, messageCount, lastMsg
       ORDER BY lastMsg.timestamp DESC
       WITH c, first, messageCount, head(collect(lastMsg)) AS last
       RETURN c, first, last, messageCount`,
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
        createdAt: toDate(c.created_at),
        updatedAt: c.updated_at ? toDate(c.updated_at) : undefined,
        firstMessagePreview: first ? (first.content as string).slice(0, 100) : undefined,
        lastMessagePreview: last ? (last.content as string).slice(0, 100) : undefined,
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
       OPTIONAL MATCH (c:Conversation)-[:HAS_MESSAGE]->(m)
       RETURN m, c.session_id AS session_id, score
       ORDER BY score DESC`,
      { embedding: queryEmbedding, limit, threshold },
    );

    return rows
      .filter((r) => !sessionId || (r.session_id as string) === sessionId)
      .map((r) => {
        const m = r.m as Record<string, unknown>;
        return {
          id: m.id as string,
          sessionId: (r.session_id as string) || "",
          role: m.role as "user" | "assistant" | "system",
          content: m.content as string,
          metadata: m.metadata ? JSON.parse(m.metadata as string) : {},
          similarity: r.score as number,
          createdAt: toDate(m.timestamp),
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
    const now = new Date().toISOString();
    await this.client.executeWrite(
      `CREATE (c:Conversation {
         id: $id, session_id: $sessionId,
         created_at: datetime($now), updated_at: datetime($now)
       })`,
      { id: convId, sessionId, now },
    );
    return convId;
  }

  private async getLastMessageId(convId: string, excludeId: string): Promise<string | null> {
    const rows = await this.client.executeRead(
      `MATCH (c:Conversation {id: $convId})-[:HAS_MESSAGE]->(m:Message)
       WHERE m.id <> $excludeId AND NOT (m)-[:NEXT_MESSAGE]->(:Message)
       RETURN m.id AS id ORDER BY m.timestamp DESC LIMIT 1`,
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

    // Link previous last message to the first new message
    if (previousLastId && messageIds.length > 0) {
      await this.client.executeWrite(
        `MATCH (prev:Message {id: $prevId}), (next:Message {id: $nextId})
         CREATE (prev)-[:NEXT_MESSAGE]->(next)`,
        { prevId: previousLastId, nextId: messageIds[0] },
      );
    }

    // Link messages within the batch
    for (let i = 0; i < messageIds.length - 1; i++) {
      await this.client.executeWrite(
        `MATCH (prev:Message {id: $prevId}), (next:Message {id: $nextId})
         CREATE (prev)-[:NEXT_MESSAGE]->(next)`,
        { prevId: messageIds[i], nextId: messageIds[i + 1] },
      );
    }

    // Create FIRST_MESSAGE relationship if this is the first message
    if (createFirstMessage && messageIds.length > 0) {
      await this.client.executeWrite(
        `MATCH (c:Conversation {id: $convId}), (m:Message {id: $msgId})
         CREATE (c)-[:FIRST_MESSAGE]->(m)`,
        { convId, msgId: messageIds[0] },
      );
    }
  }
}

/**
 * Convert a Neo4j temporal value or ISO string to a Date.
 */
function toDate(val: unknown): Date {
  if (val instanceof Date) return val;
  if (typeof val === "string") return new Date(val);
  // Neo4j driver may return a DateTime-like object with year/month/day etc.
  if (val && typeof val === "object" && "year" in (val as Record<string, unknown>)) {
    const d = val as Record<string, unknown>;
    return new Date(
      Date.UTC(
        (d.year as number) || 1970,
        ((d.month as number) || 1) - 1,
        (d.day as number) || 1,
        (d.hour as number) || 0,
        (d.minute as number) || 0,
        (d.second as number) || 0,
        Math.floor(((d.nanosecond as number) || 0) / 1_000_000),
      ),
    );
  }
  return new Date();
}
