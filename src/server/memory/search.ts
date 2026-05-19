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

import { int } from "neo4j-driver";
import type { ShortTermMemory } from "@/server/memory/shortTerm";
import type { LongTermMemory } from "@/server/memory/longTerm";
import type { SearchResults, MemoryMessage, MemoryEntity } from "@/server/memory/types";
import { getEmbedder } from "@/server/memory/embedder";
import { MemoryClient } from "@/server/memory/client";
import { NodeManager } from "@/server/memory/nodeManager";
import { getReranker, extractSearchTexts, applyRerank } from "@/server/memory/reranker";

export class MemorySearch {
  constructor(
    private shortTerm: ShortTermMemory,
    private longTerm: LongTermMemory,
  ) {}

  async search(
    query: string,
    options?: {
      memoryTypes?: string[];
      limit?: number;
      threshold?: number;
      rerank?: boolean;
    },
  ): Promise<SearchResults> {
    const { memoryTypes, limit = 10, threshold, rerank } = options || {};
    const types = memoryTypes || ["messages", "entities"];

    const useRerank = rerank !== false && getReranker() !== null;
    const effectiveThreshold = threshold ?? (useRerank ? 0.4 : 0.7);
    const fetchLimit = useRerank ? Math.max(limit * 3, 30) : limit;

    const results: SearchResults = {
      messages: [],
      entities: [],
    };

    const tasks: Promise<void>[] = [];

    if (types.includes("messages")) {
      tasks.push(
        this.shortTerm
          .searchMessages(query, { limit: fetchLimit, threshold: effectiveThreshold })
          .then(async (msgs) => {
            if (useRerank && msgs.length > 0) {
              const items = extractSearchTexts(msgs, "Message");
              const reranked = await applyRerank(query, items, limit);
              results.messages = reranked as unknown as (MemoryMessage & {
                similarity: number;
                relevance?: number;
              })[];
            } else {
              results.messages = msgs as (MemoryMessage & {
                similarity: number;
                relevance?: number;
              })[];
            }
          }),
      );
    }

    if (types.includes("entities")) {
      tasks.push(
        this.longTerm
          .searchEntities(query, { limit: fetchLimit, threshold: effectiveThreshold })
          .then(async (entities) => {
            if (useRerank && entities.length > 0) {
              const items = extractSearchTexts(entities, "Entity");
              const reranked = await applyRerank(query, items, limit);
              results.entities = reranked as unknown as (MemoryEntity & {
                similarity: number;
                relevance?: number;
              })[];
            } else {
              results.entities = entities as (MemoryEntity & {
                similarity: number;
                relevance?: number;
              })[];
            }
          }),
      );
    }

    await Promise.all(tasks);
    return results;
  }

  async searchByLabel(
    label: string,
    query: string,
    options?: {
      limit?: number;
      threshold?: number;
      rerank?: boolean;
    },
  ): Promise<Array<Record<string, unknown> & { similarity: number; relevance?: number }>> {
    const { limit = 10, threshold, rerank } = options || {};

    const useRerank = rerank !== false && getReranker() !== null;
    const effectiveThreshold = threshold ?? (useRerank ? 0.4 : 0.7);
    const fetchLimit = useRerank ? Math.max(limit * 3, 30) : limit;

    const embedder = getEmbedder();
    const queryEmbedding = await embedder.embed(query);

    const client = MemoryClient.getCachedInstance().neo4j;
    const indexName = `${label.toLowerCase()}_embedding_idx`;
    const rows = await client.executeRead(
      `CALL db.index.vector.queryNodes('${indexName}', $limit, $embedding)
       YIELD node, score WHERE score >= $threshold
       RETURN node, score ORDER BY score DESC`,
      { embedding: queryEmbedding, limit: int(fetchLimit), threshold: effectiveThreshold },
    );

    const results = rows.map((r) => {
      const node = r.node as Record<string, unknown>;
      const clean: Record<string, unknown> = {};
      for (const [k, v] of Object.entries(node)) {
        if (!k.startsWith("_")) clean[k] = v;
      }
      return { ...clean, similarity: r.score as number };
    });

    if (useRerank && results.length > 0) {
      const nodeManager = NodeManager.getCachedInstance();
      const items = results.map((r) => ({
        ...r,
        text: nodeManager.getEmbeddingText(label, r),
      }));
      const reranked = await applyRerank(query, items, limit);
      return reranked.map((r) => {
        const { text: _, ...rest } = r as Record<string, unknown>;
        return rest as Record<string, unknown> & { similarity: number; relevance?: number };
      });
    }

    return results;
  }
}
