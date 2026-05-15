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
import type { MemoryNote } from "@/server/memory/types";

export class Notes {
  private client: Neo4jClient;
  private embedder: Embedder;

  constructor(client: Neo4jClient) {
    this.client = client;
    this.embedder = getEmbedder();
  }

  async createNote(noteName: string, content: string): Promise<MemoryNote> {
    const id = uuidv4();
    const embedding = await this.embedder.embed(content);
    const now = new Date().toISOString();

    await this.client.executeWrite(
      `CREATE (n:Note {id: $id, content: $content, _embedding: $embedding, created_at: datetime($now), updated_at: datetime($now)})`,
      { id, content, embedding, now },
    );

    return {
      id,
      content,
      _embedding: embedding,
      createdAt: new Date(now),
      updatedAt: new Date(now),
    };
  }

  async updateNote(noteName: string, options: { content?: string }): Promise<MemoryNote | null> {
    const existing = await this.getNote(noteName);
    if (!existing) return null;

    const content = options.content ?? existing.content;
    let embedding = existing._embedding;
    if (options.content) {
      embedding = await this.embedder.embed(options.content);
    }
    const now = new Date().toISOString();

    await this.client.executeWrite(
      `MATCH (n:Note {name: $name})
       SET n.content = $content, n._embedding = $embedding, n.updated_at = datetime($now)
       RETURN n`,
      { name: noteName, content, embedding: embedding || null, now },
    );

    return { ...existing, content, _embedding: embedding, updatedAt: new Date(now) };
  }

  async deleteNote(noteName: string): Promise<boolean> {
    const result = await this.client.executeWrite(
      `MATCH (n:Note {name: $name}) DETACH DELETE n RETURN count(n) AS deleted`,
      { name: noteName },
    );
    return (result[0]?.deleted as number) > 0;
  }

  async getNote(noteName: string): Promise<MemoryNote | null> {
    const rows = await this.client.executeRead(`MATCH (n:Note {name: $name}) RETURN n`, {
      name: noteName,
    });
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

  async linkToEntity(noteName: string, entityName: string): Promise<void> {
    try {
      await this.client.mergeRelationship(
        "Note",
        "name",
        noteName,
        "Entity",
        "name",
        entityName,
        "ABOUT_ENTITY",
      );
    } catch (err) {
      console.warn(
        `[notes] linkToEntity(${noteName}, ${entityName}) failed:`,
        err instanceof Error ? err.message : String(err),
      );
    }
  }

  async linkToMessage(noteName: string, messageId: string): Promise<void> {
    try {
      await this.client.mergeRelationship(
        "Note",
        "name",
        noteName,
        "Message",
        "id",
        messageId,
        "ABOUT_MESSAGE",
      );
    } catch (err) {
      console.warn(
        `[notes] linkToMessage(${noteName}, ${messageId}) failed:`,
        err instanceof Error ? err.message : String(err),
      );
    }
  }

  async clearLinks(noteName: string, type: "ENTITY" | "MESSAGE" | "ALL"): Promise<void> {
    await this.client.executeWrite(
      `MATCH (n:Note {name: $noteName})-[r:ABOUT_ENTITY|ABOUT_MESSAGE]->() DELETE r`,
      { noteName },
    );
  }

  async getLinkedEntities(noteName: string): Promise<string[]> {
    const rows = await this.client.executeRead(
      `MATCH (n:Note {name: $noteName})-[:ABOUT_ENTITY]->(e:Entity) RETURN e.name AS name`,
      { noteName },
    );
    return rows.map((r) => r.name as string);
  }

  async getLinkedMessages(noteName: string): Promise<string[]> {
    const rows = await this.client.executeRead(
      `MATCH (n:Note {name: $noteName})-[:ABOUT_MESSAGE]->(m:Message) RETURN m.id AS id`,
      { noteName },
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
