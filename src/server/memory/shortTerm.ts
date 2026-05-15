/**
 * Chorus — cinematic RPG-style dialogue engine
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
import { int } from "neo4j-driver";
import { Neo4jClient } from "@/server/memory/neo4j";
import { Embedder, getEmbedder } from "@/server/memory/embedder";
import { GAME_ID } from "@/server/memory/gameState";
import type { MemoryMessage } from "@/server/memory/types";

export class ShortTermMemory {
  private client: Neo4jClient;
  private embedder: Embedder;

  constructor(client: Neo4jClient) {
    this.client = client;
    this.embedder = getEmbedder();
  }

  async addMessage(
    role: "user" | "assistant" | "system",
    content: string,
    metadata?: Record<string, unknown>,
    generateEmbedding: boolean = true,
    linkToCurrentTime: boolean = true,
  ): Promise<MemoryMessage> {
    const convId = await this.ensureConversation();

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
         _embedding: $embedding, timestamp: datetime($now),
         metadata: $metadata
       })
       CREATE (c)-[r:HAS_MESSAGE {created_at: datetime()}]->(m)
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

    if (linkToCurrentTime) {
      try {
        await this.client.executeWrite(
          `MATCH (a:TimeAnchor {id: 'anchor'})-[:CURRENT_TIMEPOINT]->(tp:TimePoint)
           MATCH (m:Message {id: $msgId})
           MERGE (m)-[r:AT_TIME]->(tp)
           ON CREATE SET r.created_at = datetime()`,
          { msgId: messageId },
        );
      } catch (err) {
        // TimePoint system not yet initialized — skip
        const msg = err instanceof Error ? err.message : String(err);
        if (!msg.includes("not found")) {
          console.warn("[shortTerm] AT_TIME link failed:", msg);
        }
      }
    }

    return {
      id: messageId,
      role,
      content,
      metadata: metadata || {},
      _embedding: embedding,
      createdAt: new Date(now),
    };
  }

  async getConversation(limit: number = 1000): Promise<MemoryMessage[]> {
    const rows = await this.client.executeRead(
      `MATCH (c:Conversation {session_id: $gameId})
       MATCH (c)-[:HAS_MESSAGE]->(m:Message)
       RETURN m ORDER BY m.timestamp DESC LIMIT $limit`,
      { gameId: GAME_ID, limit: int(limit) },
    );

    return rows.reverse().map((r) => {
      const m = r.m as Record<string, unknown>;
      return {
        id: m.id as string,
        role: m.role as "user" | "assistant" | "system",
        content: m.content as string,
        metadata: m.metadata ? JSON.parse(m.metadata as string) : {},
        _embedding: m._embedding as number[] | undefined,
        createdAt: toDate(m.timestamp),
      };
    });
  }

  async searchMessages(
    query: string,
    options?: {
      limit?: number;
      threshold?: number;
    },
  ): Promise<Array<MemoryMessage & { similarity: number }>> {
    const { limit = 10, threshold = 0.7 } = options || {};
    const queryEmbedding = await this.embedder.embed(query);

    const rows = await this.client.executeRead(
      `CALL db.index.vector.queryNodes('message_embedding_idx', $limit, $embedding)
       YIELD node AS m, score
       WHERE score >= $threshold
       OPTIONAL MATCH (c:Conversation)-[:HAS_MESSAGE]->(m)
       WHERE c.session_id = $gameId
       RETURN m, score
       ORDER BY score DESC`,
      { embedding: queryEmbedding, limit: int(limit), threshold, gameId: GAME_ID },
    );

    return rows.map((r) => {
      const m = r.m as Record<string, unknown>;
      return {
        id: m.id as string,
        role: m.role as "user" | "assistant" | "system",
        content: m.content as string,
        metadata: m.metadata ? JSON.parse(m.metadata as string) : {},
        similarity: r.score as number,
        createdAt: toDate(m.timestamp),
      };
    });
  }

  // ── Private helpers ──

  private async ensureConversation(): Promise<string> {
    const rows = await this.client.executeRead(
      `MATCH (c:Conversation {session_id: $gameId}) RETURN c.id AS id`,
      { gameId: GAME_ID },
    );
    if (rows.length > 0) return rows[0].id as string;

    const convId = uuidv4();
    const now = new Date().toISOString();
    await this.client.executeWrite(
      `CREATE (c:Conversation {
         id: $id, session_id: $gameId,
         created_at: datetime($now), updated_at: datetime($now)
       })`,
      { id: convId, gameId: GAME_ID, now },
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

    if (previousLastId && messageIds.length > 0) {
      await this.client.createRelationship(
        "Message",
        "id",
        previousLastId,
        "Message",
        "id",
        messageIds[0],
        "NEXT_MESSAGE",
      );
    }

    for (let i = 0; i < messageIds.length - 1; i++) {
      await this.client.createRelationship(
        "Message",
        "id",
        messageIds[i],
        "Message",
        "id",
        messageIds[i + 1],
        "NEXT_MESSAGE",
      );
    }

    if (createFirstMessage && messageIds.length > 0) {
      await this.client.createRelationship(
        "Conversation",
        "id",
        convId,
        "Message",
        "id",
        messageIds[0],
        "FIRST_MESSAGE",
      );
    }
  }
}

function toDate(val: unknown): Date {
  if (val instanceof Date) return val;
  if (typeof val === "string") return new Date(val);
  if (val && typeof val === "object" && "year" in (val as Record<string, unknown>)) {
    const d = val as Record<string, unknown>;
    const n = (v: unknown, fallback: number): number => {
      if (typeof v === "bigint") return Number(v);
      return (v as number) || fallback;
    };
    return new Date(
      Date.UTC(
        n(d.year, 1970),
        n(d.month, 1) - 1,
        n(d.day, 1),
        n(d.hour, 0),
        n(d.minute, 0),
        n(d.second, 0),
        Math.floor(n(d.nanosecond, 0) / 1_000_000),
      ),
    );
  }
  return new Date();
}
