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

import type { ShortTermMemory } from "@/server/memory/shortTerm";
import type { LongTermMemory } from "@/server/memory/longTerm";
import type { AssembledContext } from "@/server/memory/types";

export class ContextAssembler {
  constructor(
    private shortTerm: ShortTermMemory,
    private longTerm: LongTermMemory,
  ) {}

  async assemble(options?: {
    query?: string;
    maxItems?: number;
    includeShortTerm?: boolean;
    includeLongTerm?: boolean;
  }): Promise<AssembledContext> {
    const {
      query,
      maxItems = 10,
      includeShortTerm = true,
      includeLongTerm = true,
    } = options || {};

    const context: AssembledContext = {
      messages: [],
      entities: [],
      summary: "",
    };

    const tasks: Promise<void>[] = [];

    if (includeShortTerm) {
      tasks.push(
        this.shortTerm.getConversation(maxItems).then((msgs) => {
          context.messages = msgs;
        }),
      );
    }

    if (includeLongTerm && query) {
      tasks.push(
        this.longTerm.searchEntities(query, { limit: maxItems }).then((entities) => {
          context.entities = entities.map(({ similarity: _, ...e }) => e);
        }),
      );
    }

    await Promise.all(tasks);

    const parts: string[] = [];
    if (context.messages.length > 0) {
      parts.push("### Recent Conversation");
      for (const msg of context.messages.slice(-maxItems)) {
        parts.push(`**${msg.role}**: ${msg.content}`);
      }
    }
    if (context.entities.length > 0) {
      parts.push("\n### Relevant Entities");
      for (const e of context.entities) {
        parts.push(
          `- ${e.name} (${e.type}${e.subtype ? `:${e.subtype}` : ""})${e.description ? `: ${e.description}` : ""}`,
        );
      }
    }
    context.summary = parts.join("\n");

    return context;
  }
}
