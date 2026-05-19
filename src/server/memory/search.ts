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

import type { ShortTermMemory } from "@/server/memory/shortTerm";
import type { LongTermMemory } from "@/server/memory/longTerm";
import type { SearchResults, MemoryMessage, MemoryEntity } from "@/server/memory/types";
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
              const items = extractSearchTexts(msgs, "message");
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
              const items = extractSearchTexts(entities, "entity");
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
}
