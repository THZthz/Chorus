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
import { NodeDef, NodeManager } from "@/server/nodeManager";
import { RelationshipDef, RelationshipManager } from "@/server/relationshipManager";

function getVectorSearchable(type: "relationship" | "label") {
  const all: RelationshipDef[] | NodeDef[] = (
    type === "relationship"
      ? RelationshipManager.getCachedInstance()
      : NodeManager.getCachedInstance()
  )
    .getAll()
    .filter(
      (def) =>
        def.properties.some((p) => p.name === "_embedding") &&
        def.properties.some((p) => p.tags.includes("embedded")),
    );

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
Search the archive by semantic MEANING (vector similarity search with optional reranking).

Find things by what they're ABOUT, not by exact name or Cypher pattern. Pass one or
more domains (node labels or relationship types) via 'domains' to scope the search
(e.g. ["Entity", "Message"]). Omit to search all searchable types. Use 'target' to
restrict to only nodes or only relationships.

Search your notes at the start of every turn with domains: ["Note"].
`.trim(),
  inputSchema: z.object({
    query: z
      .string()
      .describe(
        "Natural language search query. Keep short and focused — a few keywords is enough.",
      ),
    target: z
      .array(z.enum(["node", "relationship"]))
      .default(["node", "relationship"])
      .describe("Search nodes, relationships, or both. Defaults to both."),
    domains: z
      .array(z.string())
      .optional()
      .describe(
        "Node labels or relationship types to search (e.g. ['Entity', 'Message', 'ALLIED_WITH']). Omit to search all searchable types.",
      ),
    limit: z.number().default(3).describe("Max results per domain."),
  }),
  execute: wrapSafe(async (args) => {
    const target = args.target ?? ["node", "relationship"];
    const searchNodes = target.includes("node");
    const searchRels = target.includes("relationship");

    const searchableLabels = searchNodes
      ? new Set(getVectorSearchable("label"))
      : new Set<string>();
    const searchableRelTypes = searchRels
      ? new Set(getVectorSearchable("relationship"))
      : new Set<string>();

    // Resolve domains: filter user-provided values to what's searchable.
    // If none provided, use all searchable node labels and relationship types.
    const nodeDomains: string[] = [];
    const relDomains: string[] = [];

    if (args.domains && args.domains.length > 0) {
      for (const d of args.domains) {
        const isNode = searchableLabels.has(d);
        const isRel = searchableRelTypes.has(d);
        if (!isNode && !isRel) {
          const available = [...searchableLabels, ...searchableRelTypes].join(", ");
          return `ERROR: "${d}" is not a searchable node label or relationship type. Available: ${available}`;
        }
        if (isNode && searchNodes) nodeDomains.push(d);
        if (isRel && searchRels) relDomains.push(d);
      }
    } else {
      // Search all labels and relationships.
      if (searchNodes) nodeDomains.push(...searchableLabels);
      if (searchRels) relDomains.push(...searchableRelTypes);
    }

    const client = MemoryClient.getCachedInstance();
    const result: Record<string, Record<string, unknown>[]> = {};

    const tasks: Promise<void>[] = [];

    for (const label of nodeDomains) {
      tasks.push(
        client.search.searchByLabel(label, args.query, { limit: args.limit }).then((rows) => {
          result[label] = stripHiddenProperties(rows) as Record<string, unknown>[];
        }),
      );
    }

    for (const type of relDomains) {
      tasks.push(
        client.search
          .searchByRelationshipType(type, args.query, { limit: args.limit })
          .then((rows) => {
            result[type] = stripHiddenProperties(rows) as Record<string, unknown>[];
          }),
      );
    }

    await Promise.all(tasks);

    return JSON.stringify(result, null, 2);
  }, TOOL_NAMES.SEARCH_WORLD),
});
