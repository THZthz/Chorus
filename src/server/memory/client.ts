/**
 * Chorus — cinematic dialogue engine
 * Copyright (C) 2026 Amias
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

import { Neo4jClient } from "@/server/memory/neo4j";
import { getEmbedder } from "@/server/memory/embedder";
import { ShortTermMemory } from "@/server/memory/shortTerm";
import { MemorySearch } from "@/server/memory/search";
import { Notes } from "@/server/memory/notes";
import { Plots } from "@/server/memory/plots";
import { RelationshipManager } from "@/server/relationshipManager";
import { NodeManager } from "@/server/nodeManager";

export { ShortTermMemory } from "@/server/memory/shortTerm";
export { MemorySearch } from "@/server/memory/search";
export { Notes } from "@/server/memory/notes";
export { Plots } from "@/server/memory/plots";
export * from "@/server/memory/types";

export class MemoryClient {
  readonly neo4j: Neo4jClient;
  readonly shortTerm: ShortTermMemory;
  readonly search: MemorySearch;
  readonly notes: Notes;
  readonly plots: Plots;

  private constructor(neo4j: Neo4jClient) {
    this.neo4j = neo4j;
    this.shortTerm = new ShortTermMemory(neo4j);
    this.search = new MemorySearch(neo4j);
    this.notes = new Notes(neo4j);
    this.plots = new Plots(neo4j);
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
    const _embedder = getEmbedder();
    MemoryClient.instance = new MemoryClient(client);

    // Initialize RelationshipManager singleton (eager, idempotent)
    RelationshipManager.getCachedInstance();

    // Initialize NodeManager singleton (eager, idempotent)
    NodeManager.getCachedInstance();

    return MemoryClient.instance;
  }

  // TODO: Should be "getMemoryClient" just like "getEmbedder" or "getReranker".
  static getCachedInstance(): MemoryClient {
    if (!MemoryClient.instance) {
      throw new Error("MemoryClient not initialized. Call getInstance() first.");
    }
    return MemoryClient.instance;
  }

  // TODO: We should have a global event emitter to broadcast events like "server_start" or "server_close" to handle this automatically.
  //  But where should the event get registered?
  static async closeInstance(): Promise<void> {
    if (MemoryClient.instance) {
      await MemoryClient.instance.close();
      MemoryClient.instance = null;
    }
  }
}
