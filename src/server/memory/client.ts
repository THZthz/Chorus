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

import { Neo4jClient } from "./neo4j";
import { setupSchema } from "./schema";
import { getEmbedder } from "./embedder";
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
    this.search = new MemorySearch(this.shortTerm, this.longTerm, this.reasoning);
    this.context = new ContextAssembler(this.shortTerm, this.longTerm, this.reasoning);
    this.observer = new MemoryObserver(this.shortTerm);
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

    const client = new Neo4jClient(options?.neo4jUri, options?.neo4jUser, options?.neo4jPassword);

    await client.verifyConnectivity();
    const embedder = getEmbedder();
    await setupSchema(client, embedder.dimensions);

    MemoryClient.instance = new MemoryClient(client);
    return MemoryClient.instance;
  }

  static getCachedInstance(): MemoryClient {
    if (!MemoryClient.instance) {
      throw new Error("MemoryClient not initialized. Call getInstance() first.");
    }
    return MemoryClient.instance;
  }

  static async closeInstance(): Promise<void> {
    if (MemoryClient.instance) {
      await MemoryClient.instance.close();
      MemoryClient.instance = null;
    }
  }
}
