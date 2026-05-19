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

import type { Neo4jClient } from "@/server/memory/neo4j";
import { NodeManager } from "@/server/memory/nodeManager";

// ── Types ──

export interface SchemaNode {
  labels: string[];
  properties: {
    name: string;
    indexes: string[];
    constraints: string[];
  };
}

export interface SchemaRelationship {
  type: string;
  properties: {
    name: string;
  };
}

export interface SchemaVisualization {
  nodes: SchemaNode[];
  relationships: SchemaRelationship[];
}

export interface RelationshipTypeDescription {
  name: string;
  description: string;
  category: string;
  sourceLabels?: string[];
  targetLabels?: string[];
}

// ── Queries ──

export async function getSchemaVisualization(db: Neo4jClient): Promise<SchemaVisualization> {
  const rows = await db.executeRead("CALL db.schema.visualization()");
  const row = rows[0];
  if (!row) return { nodes: [], relationships: [] };

  const rawNodes = (row.nodes as Array<Record<string, unknown>>) || [];
  const rawRels = (row.relationships as Array<Record<string, unknown>>) || [];

  const nodes: SchemaNode[] = rawNodes
    .map((n) => {
      const labels = (n._labels as string[]) || [];
      const props = n as Record<string, unknown>;
      return {
        labels,
        properties: {
          name: (props.name as string) || labels[0] || "Unknown",
          indexes: (props.indexes as string[]) || [],
          constraints: (props.constraints as string[]) || [],
        },
      };
    })
    .filter((n) => {
      const internalNames = new Set(
        NodeManager.getCachedInstance()
          .getByType("INTERNAL")
          .map((d) => d.name),
      );
      return !n.labels.some((l) => internalNames.has(l));
    });

  const relationships: SchemaRelationship[] = rawRels.map((r) => {
    const props = r as Record<string, unknown>;
    return {
      type: (r._type as string) || (props.name as string) || "Unknown",
      properties: {
        name: (props.name as string) || (r._type as string) || "Unknown",
      },
    };
  });

  return { nodes, relationships };
}

export async function getRelationshipTypeDescriptions(
  db: Neo4jClient,
): Promise<RelationshipTypeDescription[]> {
  const rows = await db.executeRead(
    `MATCH (rt:RelationshipType)
     RETURN rt.name AS name, rt.description AS description, rt.category AS category,
            rt.source_labels AS sourceLabels, rt.target_labels AS targetLabels
     ORDER BY rt.name`,
  );
  return rows.map((r) => {
    let sourceLabels: string[] | undefined;
    let targetLabels: string[] | undefined;
    if (typeof r.sourceLabels === "string") {
      try {
        sourceLabels = JSON.parse(r.sourceLabels) as string[];
      } catch {
        /* keep undefined */
      }
    }
    if (typeof r.targetLabels === "string") {
      try {
        targetLabels = JSON.parse(r.targetLabels) as string[];
      } catch {
        /* keep undefined */
      }
    }
    return {
      name: r.name as string,
      description: (r.description as string) || "",
      category: (r.category as string) || "PREDEFINED",
      sourceLabels,
      targetLabels,
    };
  });
}

// ── Formatters ──

export function formatSchemaMarkdown(
  schema: SchemaVisualization,
  descriptions: RelationshipTypeDescription[],
): string {
  const parts: string[] = [];
  parts.push("## Schema");
  parts.push("");

  // Node labels with indexes and constraints
  parts.push("### Node Labels");
  const sortedNodes = [...schema.nodes].sort((a, b) =>
    a.properties.name.localeCompare(b.properties.name),
  );
  for (const node of sortedNodes) {
    const name = node.properties.name;
    const indexList = node.properties.indexes.filter((i) => !i.startsWith("_")).join(", ");
    const constraintSummary = node.properties.constraints
      .map((c) => {
        const m = c.match(/type='(\w+)'/);
        return m ? m[1] : c;
      })
      .join(", ");
    const details: string[] = [];
    if (indexList) details.push(`indexes: ${indexList}`);
    if (constraintSummary) details.push(`constraints: ${constraintSummary}`);
    const detail = details.length > 0 ? ` (${details.join("; ")})` : "";
    parts.push(`- **${name}**${detail}`);
  }
  parts.push("");

  // Relationship types with descriptions from :RelationshipType nodes
  parts.push("### Relationship Types");
  const descMap = new Map(descriptions.map((d) => [d.name, d]));
  const seenTypes = new Set<string>();
  for (const rel of schema.relationships) {
    if (seenTypes.has(rel.type)) continue;
    seenTypes.add(rel.type);
    const desc = descMap.get(rel.type);
    if (desc) {
      const src = desc.sourceLabels?.length ? desc.sourceLabels.join("|") : "?";
      const tgt = desc.targetLabels?.length ? desc.targetLabels.join("|") : "?";
      const endpoints = `(${src})→(${tgt})`;
      const category = desc.category !== "PREDEFINED" ? ` — ${desc.category}` : "";
      parts.push(`- **${rel.type}** ${endpoints}${category}: ${desc.description}`);
    } else {
      parts.push(`- **${rel.type}**`);
    }
  }
  parts.push("");

  return parts.join("\n");
}
