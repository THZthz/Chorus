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
import { RelationshipManager } from "@/server/memory/relationshipManager";
import { getReranker, extractSearchTexts, applyRerank } from "@/server/memory/reranker";

export class MemorySearch {
  constructor() {}

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

  async searchByRelationshipType(
    type: string,
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
    const indexName = `rel_${type.toLowerCase()}_embedding_idx`;
    const rows = await client.executeRead(
      `CALL db.index.vector.queryRelationships('${indexName}', $limit, $embedding)
       YIELD relationship, score WHERE score >= $threshold
       RETURN relationship, score ORDER BY score DESC`,
      { embedding: queryEmbedding, limit: int(fetchLimit), threshold: effectiveThreshold },
    );

    const results = rows.map((r) => {
      const rel = r.relationship as Record<string, unknown>;
      const clean: Record<string, unknown> = {};
      for (const [k, v] of Object.entries(rel)) {
        if (!k.startsWith("_")) clean[k] = v;
      }
      return { ...clean, similarity: r.score as number };
    });

    if (useRerank && results.length > 0) {
      const relManager = RelationshipManager.getCachedInstance();
      const items = results.map((r) => ({
        ...r,
        text: relManager.getEmbeddingText(type, r),
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
