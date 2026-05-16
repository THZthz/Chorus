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

import type { Neo4jClient } from "@/server/memory/neo4j";

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
  sourceLabels: string[];
  targetLabels: string[];
}

export interface SchemaVisualization {
  nodes: SchemaNode[];
  relationships: SchemaRelationship[];
}

export interface RelationshipTypeDescription {
  name: string;
  description: string;
  category: string;
}

// ── Labels to exclude from the schema display ──

const INTERNAL_LABELS = new Set(["GMTurnMessage", "IdCounter", "Conversation"]);

// ── Queries ──

export async function getSchemaVisualization(db: Neo4jClient): Promise<SchemaVisualization> {
  const rows = await db.executeRead("CALL db.schema.visualization()");
  const row = rows[0];
  if (!row) return { nodes: [], relationships: [] };

  const rawNodes = (row.nodes as Array<Record<string, unknown>>) || [];
  const rawRels = (row.relationships as Array<Record<string, unknown>>) || [];

  // Build elementId → labels map for resolving relationship directions
  const labelsByElementId = new Map<string, string[]>();
  for (const n of rawNodes) {
    const id = n._elementId as string | undefined;
    const labels = (n._labels as string[]) || [];
    if (id) labelsByElementId.set(id, labels);
  }

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
    .filter((n) => !n.labels.some((l) => INTERNAL_LABELS.has(l)));

  const relationships: SchemaRelationship[] = rawRels.map((r) => {
    const props = r as Record<string, unknown>;
    const startId = r._startNodeElementId as string | undefined;
    const endId = r._endNodeElementId as string | undefined;
    return {
      type: (r._type as string) || (props.name as string) || "Unknown",
      properties: {
        name: (props.name as string) || (r._type as string) || "Unknown",
      },
      sourceLabels: (startId && labelsByElementId.get(startId)) || [],
      targetLabels: (endId && labelsByElementId.get(endId)) || [],
    };
  });

  return { nodes, relationships };
}

export async function getRelationshipTypeDescriptions(
  db: Neo4jClient,
): Promise<RelationshipTypeDescription[]> {
  const rows = await db.executeRead(
    `MATCH (rt:RelationshipType)
     RETURN rt.name AS name, rt.description AS description, rt.category AS category
     ORDER BY rt.name`,
  );
  return rows.map((r) => ({
    name: r.name as string,
    description: (r.description as string) || "",
    category: (r.category as string) || "PREDEFINED",
  }));
}

const PROPS_EXCLUDE = new Set([
  "GMTurnMessage", "IdCounter", "Conversation",
]);

export async function getNodeProperties(
  db: Neo4jClient,
): Promise<Map<string, string[]>> {
  const labelRows = await db.executeRead("CALL db.labels() YIELD label RETURN label");
  const allLabels = labelRows.map((r) => r.label as string);
  const visible = allLabels.filter((l) => !PROPS_EXCLUDE.has(l));

  const results = await Promise.all(
    visible.map(async (label) => {
      try {
        const rows = await db.executeRead(`MATCH (n:\`${label}\`) RETURN n LIMIT 1`);
        if (rows.length > 0) {
          const n = rows[0].n as Record<string, unknown>;
          const props = Object.keys(n)
            .filter((k) => k !== "_elementId" && k !== "_labels")
            .sort();
          return { label, props };
        }
      } catch { /* label may not support direct matching */ }
      return { label, props: [] };
    }),
  );

  const map = new Map<string, string[]>();
  for (const { label, props } of results) {
    map.set(label, props);
  }
  return map;
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
      parts.push(`- **${rel.type}**: ${desc.description}`);
    } else {
      parts.push(`- **${rel.type}**`);
    }
  }
  parts.push("");

  return parts.join("\n");
}
