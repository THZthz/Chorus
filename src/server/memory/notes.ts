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
import { int } from "neo4j-driver";
import { Neo4jClient } from "@/server/memory/neo4j";
import { Embedder, getEmbedder } from "@/server/memory/embedder";
import type { MemoryNote } from "@/server/memory/types";

export class Notes {
  private client: Neo4jClient;
  private embedder: Embedder;

  constructor(client: Neo4jClient) {
    this.client = client;
    this.embedder = getEmbedder();
  }

  async createNote(content: string): Promise<MemoryNote> {
    const id = uuidv4();
    const embedding = await this.embedder.embed(content);
    const now = new Date().toISOString();

    await this.client.executeWrite(
      `CREATE (n:Note {id: $id, content: $content, _embedding: $embedding, created_at: datetime($now), updated_at: datetime($now)})`,
      { id, content, embedding, now },
    );

    return { id, content, _embedding: embedding, createdAt: new Date(now), updatedAt: new Date(now) };
  }

  async updateNote(noteId: string, options: { content?: string }): Promise<MemoryNote | null> {
    const existing = await this.getNote(noteId);
    if (!existing) return null;

    const content = options.content ?? existing.content;
    let embedding = existing._embedding;
    if (options.content) {
      embedding = await this.embedder.embed(options.content);
    }
    const now = new Date().toISOString();

    await this.client.executeWrite(
      `MATCH (n:Note {id: $id})
       SET n.content = $content, n._embedding = $embedding, n.updated_at = datetime($now)
       RETURN n`,
      { id: noteId, content, embedding: embedding || null, now },
    );

    return { ...existing, content, _embedding: embedding, updatedAt: new Date(now) };
  }

  async deleteNote(noteId: string): Promise<boolean> {
    const result = await this.client.executeWrite(
      `MATCH (n:Note {id: $id}) DETACH DELETE n RETURN count(n) AS deleted`,
      { id: noteId },
    );
    return (result[0]?.deleted as number) > 0;
  }

  async getNote(noteId: string): Promise<MemoryNote | null> {
    const rows = await this.client.executeRead(`MATCH (n:Note {id: $id}) RETURN n`, { id: noteId });
    if (rows.length === 0) return null;
    return this.parseNote(rows[0].n as Record<string, unknown>);
  }

  async searchNotes(
    query: string,
    options?: { limit?: number; threshold?: number },
  ): Promise<Array<MemoryNote & { similarity: number }>> {
    const { limit = 10, threshold = 0.7 } = options || {};
    const queryEmbedding = await this.embedder.embed(query);

    const rows = await this.client.executeRead(
      `CALL db.index.vector.queryNodes('note_embedding_idx', $limit, $embedding)
       YIELD node AS n, score WHERE score >= $threshold
       RETURN n, score ORDER BY score DESC`,
      { embedding: queryEmbedding, limit: int(limit), threshold },
    );

    return rows.map((r) => ({
      ...this.parseNote(r.n as Record<string, unknown>),
      similarity: r.score as number,
    }));
  }

  async linkToEntity(noteId: string, entityName: string): Promise<void> {
    try {
      await this.client.executeWrite(
        `MATCH (n:Note {id: $noteId}), (e:Entity {name: $entityName})
         MERGE (n)-[:ABOUT]->(e)`,
        { noteId, entityName },
      );
    } catch {
      // entity may not exist — skip
    }
  }

  async linkToMessage(noteId: string, messageId: string): Promise<void> {
    try {
      await this.client.executeWrite(
        `MATCH (n:Note {id: $noteId}), (m:Message {id: $messageId})
         MERGE (n)-[:ABOUT_MESSAGE]->(m)`,
        { noteId, messageId },
      );
    } catch {
      // message may not exist — skip
    }
  }

  async clearLinks(noteId: string): Promise<void> {
    await this.client.executeWrite(
      `MATCH (n:Note {id: $noteId})-[r:ABOUT|ABOUT_MESSAGE]->() DELETE r`,
      { noteId },
    );
  }

  async getLinkedEntities(noteId: string): Promise<string[]> {
    const rows = await this.client.executeRead(
      `MATCH (n:Note {id: $noteId})-[:ABOUT]->(e:Entity) RETURN e.name AS name`,
      { noteId },
    );
    return rows.map((r) => r.name as string);
  }

  async getLinkedMessages(noteId: string): Promise<string[]> {
    const rows = await this.client.executeRead(
      `MATCH (n:Note {id: $noteId})-[:ABOUT_MESSAGE]->(m:Message) RETURN m.id AS id`,
      { noteId },
    );
    return rows.map((r) => r.id as string);
  }

  private parseNote(data: Record<string, unknown>): MemoryNote {
    return {
      id: data.id as string,
      content: data.content as string,
      _embedding: data._embedding as number[] | undefined,
      createdAt: new Date((data.created_at as string | number) || Date.now()),
      updatedAt: new Date((data.updated_at as string | number) || Date.now()),
    };
  }
}
