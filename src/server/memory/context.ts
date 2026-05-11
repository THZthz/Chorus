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

import type { ShortTermMemory } from "./short-term";
import type { LongTermMemory } from "./long-term";
import type { ReasoningMemory } from "./reasoning";
import type { AssembledContext } from "./types";

export class ContextAssembler {
  constructor(
    private shortTerm: ShortTermMemory,
    private longTerm: LongTermMemory,
    private reasoning: ReasoningMemory,
  ) {}

  async assemble(
    sessionId?: string,
    options?: {
      query?: string;
      maxItems?: number;
      includeShortTerm?: boolean;
      includeLongTerm?: boolean;
      includeReasoning?: boolean;
    },
  ): Promise<AssembledContext> {
    const {
      query,
      maxItems = 10,
      includeShortTerm = true,
      includeLongTerm = true,
      includeReasoning = true,
    } = options || {};

    const context: AssembledContext = {
      messages: [],
      entities: [],
      preferences: [],
      traces: [],
      summary: "",
    };

    const tasks: Promise<void>[] = [];

    if (includeShortTerm && sessionId) {
      tasks.push(
        this.shortTerm.getConversation(sessionId, maxItems).then((msgs) => {
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
      tasks.push(
        this.longTerm.getPreferences(undefined, maxItems).then((prefs) => {
          context.preferences = prefs;
        }),
      );
    }

    if (includeReasoning && query) {
      tasks.push(
        this.reasoning.getSimilarTraces(query, { limit: 3 }).then((traces) => {
          context.traces = traces.map(({ similarity: _, ...t }) => t);
        }),
      );
    }

    await Promise.all(tasks);

    // Build summary text
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
    if (context.preferences.length > 0) {
      parts.push("\n### User Preferences");
      for (const p of context.preferences) {
        parts.push(`- [${p.category}] ${p.preference}`);
      }
    }
    if (context.traces.length > 0) {
      parts.push("\n### Similar Past Tasks");
      for (const t of context.traces) {
        parts.push(`- ${t.task}${t.outcome ? ` → ${t.outcome}` : ""}`);
      }
    }
    context.summary = parts.join("\n");

    return context;
  }
}
