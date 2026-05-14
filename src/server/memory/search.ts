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

import type { ShortTermMemory } from "@/server/memory/shortTerm";
import type { LongTermMemory } from "@/server/memory/longTerm";
import type { SearchResults } from "@/server/memory/types";

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
    },
  ): Promise<SearchResults> {
    const { memoryTypes, limit = 10, threshold = 0.7 } = options || {};
    const types = memoryTypes || ["messages", "entities"];

    const results: SearchResults = {
      messages: [],
      entities: [],
    };

    const tasks: Promise<void>[] = [];

    if (types.includes("messages")) {
      tasks.push(
        this.shortTerm.searchMessages(query, { limit, threshold }).then((msgs) => {
          results.messages = msgs;
        }),
      );
    }

    if (types.includes("entities")) {
      tasks.push(
        this.longTerm.searchEntities(query, { limit, threshold }).then((entities) => {
          results.entities = entities;
        }),
      );
    }

    await Promise.all(tasks);
    return results;
  }
}
