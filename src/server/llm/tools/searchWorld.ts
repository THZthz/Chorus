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

import { tool } from "ai";
import { z } from "zod";
import { MemoryClient } from "@/server/memory/client";
import { stripHiddenProperties } from "@/server/memory/neo4j";
import { wrapSafe } from "@/server/llm/tools/shared";
import { TOOL_NAMES } from "@/shared/constants";
import { NodeManager } from "@/server/memory/nodeManager";

function getSearchableLabels(): string[] {
  const all = NodeManager.getCachedInstance().getAll()
    .filter((def) => def.properties.some((p) => p.name === "_embedding"));

  // Filter out subtype labels: labels whose property definitions (names + tags)
  // are identical to another label's — they share the same vector index.
  const seen = new Map<string, string>(); // property-fingerprint → first label name
  const primary: string[] = [];
  for (const def of all) {
    const fingerprint = def.properties
      .map((p) => `${p.name}:${[...p.tags].sort().join(",")}`)
      .sort()
      .join("|");
    const existing = seen.get(fingerprint);
    if (existing === undefined) {
      seen.set(fingerprint, def.name);
      primary.push(def.name);
    }
    // else: def.name is a subtype of existing — shares the same vector index
  }
  return primary;
}

export const searchWorld = tool({
  title: TOOL_NAMES.SEARCH_WORLD,
  description: `
Search the archive by semantic MEANING (vector similarity + reranking).
Use this when you don't know the exact name or Cypher pattern — ${
    TOOL_NAMES.SEARCH_WORLD
  } finds things by what they're ABOUT.

Pass one or more node labels as 'labels' to choose which domain to search
(e.g. ["Entity", "Message", "Note", "Plot"]). Omit to search all searchable types.
`.trim(),
  inputSchema: z.object({
    query: z
      .string()
      .describe(
        "Natural language search query, should be a list of keywords, keep short and focused.",
      ),
    labels: z
      .array(z.string())
      .default(getSearchableLabels())
      .describe("Node labels to search. Omit to search all searchable types."),
    limit: z.number().default(10).describe("Max results per label."),
  }),
  execute: wrapSafe(async (args) => {
    const searchableLabels = new Set(getSearchableLabels());
    const labels = args.labels && args.labels.length > 0 ? args.labels : getSearchableLabels();

    for (const label of labels) {
      if (!searchableLabels.has(label)) {
        const available = [...searchableLabels].join(", ");
        return `ERROR: "${label}" is not a searchable node label. Available: ${available}`;
      }
    }

    const client = MemoryClient.getCachedInstance();
    const result: Record<string, Record<string, unknown>[]> = {};

    const tasks = labels.map(async (label) => {
      const rows = await client.search.searchByLabel(label, args.query, {
        limit: args.limit,
      });
      result[label] = stripHiddenProperties(rows) as Record<string, unknown>[];
    });
    await Promise.all(tasks);

    return JSON.stringify(result, null, 2);
  }, TOOL_NAMES.SEARCH_WORLD),
});
