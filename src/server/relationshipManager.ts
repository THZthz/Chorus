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

/** The meaning of those tags is basically the same with NODE_PROPERTY_TAGS. */
export const RELATIONSHIP_PROPERTY_TAGS = [
  "string",
  "number",
  "number[]",
  "json",
  "embedded",
  "index",
  "composite_index_1",
  "composite_index_2",
  "composite_index_3",
] as const;
export type RelationshipPropertyTag = (typeof RELATIONSHIP_PROPERTY_TAGS)[number];

export interface RelationshipPropertyDef {
  name: string;
  description: string;
  tags: RelationshipPropertyTag[];
}

export interface RelationshipDef {
  name: string;
  description: string;
  type: "INTERNAL" | "PREDEFINED" | "GM_DEFINED";
  sourceLabel: string;
  targetLabel: string;
  properties: RelationshipPropertyDef[];
}

function makeKey(name: string, sourceLabel: string, targetLabel: string): string {
  return `${name}||${sourceLabel}||${targetLabel}`;
}

const INTERNAL_TYPES: {
  name: string;
  description: string;
  sourceLabel: string;
  targetLabel: string;
}[] = [
  {
    name: "_HAS_GM_MESSAGE",
    description: "Links a Conversation node to its GMTurnMessage nodes.",
    sourceLabel: "Conversation",
    targetLabel: "GMTurnMessage",
  },
  {
    name: "_FIRST_GM_MESSAGE",
    description: "Points to the first GMTurnMessage in a Conversation's ordered linked list.",
    sourceLabel: "Conversation",
    targetLabel: "GMTurnMessage",
  },
  {
    name: "_NEXT_GM_MESSAGE",
    description: "Sequentially links GMTurnMessage nodes in conversation order.",
    sourceLabel: "GMTurnMessage",
    targetLabel: "GMTurnMessage",
  },
];

const PREDEFINED_TYPES: {
  name: string;
  description: string;
  sourceLabel: string;
  targetLabel: string;
  properties?: RelationshipPropertyDef[];
}[] = [
  {
    name: "HAS_MESSAGE",
    description: "Links a Conversation node to its Message nodes.",
    sourceLabel: "Conversation",
    targetLabel: "Message",
  },
  {
    name: "FIRST_MESSAGE",
    description: "Points to the first Message in a Conversation's ordered linked list.",
    sourceLabel: "Conversation",
    targetLabel: "Message",
  },
  {
    name: "NEXT_MESSAGE",
    description: "Sequentially links Message nodes in conversation order.",
    sourceLabel: "Message",
    targetLabel: "Message",
  },
  {
    name: "NEXT_TIMEPOINT",
    description: "Links TimePoint nodes in chronological sequence. Records the reason for the time advance.",
    sourceLabel: "TimePoint",
    targetLabel: "TimePoint",
    properties: [
      {
        name: "reason",
        description: "Narrative reason for advancing time from the previous TimePoint to this one.",
        tags: ["string"],
      },
    ],
  },
  {
    name: "CURRENT_TIMEPOINT",
    description: "Points to the current TimePoint from a TimeAnchor node.",
    sourceLabel: "TimeAnchor",
    targetLabel: "TimePoint",
  },
  {
    name: "AT_TIME",
    description: "Links a Message to the TimePoint when it was created.",
    sourceLabel: "Message",
    targetLabel: "TimePoint",
  },
  {
    name: "STARTED_AT",
    description: "Marks the TimePoint when a Plot started.",
    sourceLabel: "Plot",
    targetLabel: "TimePoint",
  },
  {
    name: "ACTIVE_AT",
    description: "Marks the TimePoint when a Plot became active.",
    sourceLabel: "Plot",
    targetLabel: "TimePoint",
  },
  {
    name: "COMPLETED_AT",
    description: "Marks the TimePoint when a Plot completed.",
    sourceLabel: "Plot",
    targetLabel: "TimePoint",
  },
  {
    name: "LOCATED_AT",
    description: "An entity is physically present at a location.",
    sourceLabel: "Entity",
    targetLabel: "Location",
  },
  {
    name: "CARRIES",
    description: "An entity is carrying or in possession of an object.",
    sourceLabel: "Entity",
    targetLabel: "Object",
  },
  {
    name: "ALLIED_WITH",
    description: "An entity is allied with or friendly toward another entity.",
    sourceLabel: "Entity",
    targetLabel: "Entity",
  },
  {
    name: "HOSTILE_TOWARDS",
    description: "An entity is hostile toward or in conflict with another entity.",
    sourceLabel: "Entity",
    targetLabel: "Entity",
  },
  {
    name: "LOCATED_IN",
    description: "A location or entity is contained within a larger location.",
    sourceLabel: "Entity",
    targetLabel: "Location",
  },
  {
    name: "HAS_DISPOSITION",
    description: "Links an Entity (NPC) to its NPCDisposition node.",
    sourceLabel: "Entity",
    targetLabel: "NPCDisposition",
  },
  {
    name: "ABOUT_ENTITY",
    description: "A Note is about or references an Entity.",
    sourceLabel: "Note",
    targetLabel: "Entity",
  },
  {
    name: "ABOUT_MESSAGE",
    description: "A Note is about or references a specific Message.",
    sourceLabel: "Note",
    targetLabel: "Message",
  },
  {
    name: "BRANCHES_TO",
    description: "A parent Plot branches to a child sub-plot.",
    sourceLabel: "Plot",
    targetLabel: "Plot",
  },
];

export class RelationshipManager {
  private registry = new Map<string, RelationshipDef>();

  private constructor() {
    for (const t of INTERNAL_TYPES) {
      this.registry.set(makeKey(t.name, t.sourceLabel, t.targetLabel), {
        ...t,
        type: "INTERNAL",
        properties: [],
      });
    }
    for (const t of PREDEFINED_TYPES) {
      this.registry.set(makeKey(t.name, t.sourceLabel, t.targetLabel), {
        ...t,
        type: "PREDEFINED",
        properties: [],
      });
    }
  }

  register(
    name: string,
    description: string,
    type: "INTERNAL" | "PREDEFINED" | "GM_DEFINED",
    sourceLabel: string,
    targetLabel: string,
    properties?: RelationshipPropertyDef[],
  ): void {
    const key = makeKey(name, sourceLabel, targetLabel);
    const existing = this.registry.get(key);
    if (existing) {
      if (existing.type !== type) {
        console.warn(
          `[RelationshipManager] "${key}" already registered as ${existing.type}, ignoring re-registration as ${type}`,
        );
      }
      return;
    }
    this.registry.set(key, {
      name,
      description,
      type,
      sourceLabel,
      targetLabel,
      properties: properties ?? [],
    });
  }

  get(name: string, sourceLabel: string, targetLabel: string): RelationshipDef | undefined {
    // Exact match first, then try wildcard ("" sentinel)
    const exact = this.registry.get(makeKey(name, sourceLabel, targetLabel));
    if (exact) return exact;
    // Fall back to wildcard entry if one exists
    return this.registry.get(makeKey(name, "", ""));
  }

  getByName(name: string): RelationshipDef[] {
    const prefix = `${name}||`;
    const results: RelationshipDef[] = [];
    for (const [key, def] of this.registry) {
      if (key.startsWith(prefix)) {
        results.push(def);
      }
    }
    return results;
  }

  getAll(): RelationshipDef[] {
    return [...this.registry.values()];
  }

  getByType(type: "INTERNAL" | "PREDEFINED" | "GM_DEFINED"): RelationshipDef[] {
    return [...this.registry.values()].filter((r) => r.type === type);
  }

  isAllowedForWrite(name: string, sourceLabel: string, targetLabel: string): boolean {
    const def = this.get(name, sourceLabel, targetLabel);
    if (!def) return false;
    return def.type === "PREDEFINED" || def.type === "GM_DEFINED";
  }

  isAllowedForRead(name: string, sourceLabel: string, targetLabel: string): boolean {
    return this.get(name, sourceLabel, targetLabel) !== undefined;
  }

  /** Build embedding text by concatenating all "embedded"-tagged properties from the first matching definition. */
  getEmbeddingText(name: string, props: Record<string, unknown>): string {
    const defs = this.getByName(name);
    const def = defs[0];
    if (!def) return "";
    const embeddedProps = def.properties.filter((p) => p.tags.includes("embedded"));
    return embeddedProps
      .map((p) => {
        const val = props[p.name];
        return val ? `## ${p.name}\n${val}` : "";
      })
      .filter((v) => v.length > 0)
      .join("\n");
  }

  updateDescription(
    name: string,
    sourceLabel: string,
    targetLabel: string,
    description: string,
  ): boolean {
    const def = this.registry.get(makeKey(name, sourceLabel, targetLabel));
    if (!def || def.type !== "GM_DEFINED") return false;
    def.description = description;
    return true;
  }

  updateDefinition(
    name: string,
    sourceLabel: string,
    targetLabel: string,
    updates: { description?: string; properties?: RelationshipPropertyDef[] },
  ): boolean {
    const def = this.registry.get(makeKey(name, sourceLabel, targetLabel));
    if (!def || def.type !== "GM_DEFINED") return false;
    if (updates.description !== undefined) def.description = updates.description;
    if (updates.properties !== undefined) def.properties = updates.properties;
    return true;
  }

  unregister(name: string, sourceLabel: string, targetLabel: string): boolean {
    const key = makeKey(name, sourceLabel, targetLabel);
    const def = this.registry.get(key);
    if (!def || def.type !== "GM_DEFINED") return false;
    this.registry.delete(key);
    return true;
  }

  reset(): void {
    for (const [key, def] of this.registry) {
      if (def.type === "GM_DEFINED") {
        this.registry.delete(key);
      }
    }
  }

  async syncToNeo4j(client: Neo4jClient): Promise<void> {
    for (const def of this.registry.values()) {
      await client.executeWrite(
        `MERGE (rt:RelationshipType {name: $name, source_label: $sourceLabel, target_label: $targetLabel})
         SET rt.description = $description,
             rt.category = $category,
             rt.properties = $properties`,
        {
          name: def.name,
          description: def.description,
          category: def.type,
          sourceLabel: def.sourceLabel,
          targetLabel: def.targetLabel,
          properties: def.properties.length > 0 ? JSON.stringify(def.properties) : null,
        },
      );

      // Create regular index for properties with tag "index".
      for (const propName of def.properties
        .filter((p) => p.tags.includes("index"))
        .map((p) => p.name)) {
        const indexName = `rel_${def.name.toLowerCase()}_${propName}_idx`;
        try {
          await client.executeWrite(
            `CREATE INDEX ${indexName} IF NOT EXISTS FOR ()-[r:\`${def.name}\`]-() ON (r.\`${propName}\`)`,
          );
        } catch (err) {
          const msg = err instanceof Error ? err.message : String(err);
          console.error(`[RelationshipManager] Index on ${indexName} not created: ${msg}`);
        }
      }

      // Create composite index for each composite_index group.
      for (const index of ["composite_index_1", "composite_index_2", "composite_index_3"]) {
        const props = def.properties
          .filter((p) => p.tags.includes(index as RelationshipPropertyTag))
          .map((p) => p.name);
        if (props.length < 2) continue;
        const indexName = `rel_${def.name.toLowerCase()}_${props.join("_")}_idx${index.at(-1)}`;
        try {
          await client.executeWrite(
            `CREATE INDEX ${indexName} IF NOT EXISTS FOR ()-[r:\`${def.name}\`]-() ON (${props.map((name) => `r.\`${name}\``).join(", ")})`,
          );
        } catch (err) {
          const msg = err instanceof Error ? err.message : String(err);
          console.error(
            `[RelationshipManager] Composite index on ${indexName} not created: ${msg}`,
          );
        }
      }

      // Create vector index for types that have _embedding property.
      if (def.properties.some((p) => p.name === "_embedding")) {
        const vectorIndexName = `rel_${def.name.toLowerCase()}_embedding_idx`;
        const dimensions = process.env.EMBEDDING_DIMENSIONS || 1024;
        try {
          await client.executeWrite(
            `CREATE VECTOR INDEX ${vectorIndexName} IF NOT EXISTS FOR ()-[r:\`${def.name}\`]-() ON (r._embedding)
            OPTIONS { indexConfig: { \`vector.dimensions\`: ${dimensions}, \`vector.similarity_function\`: 'COSINE' } }`,
          );
        } catch {
          console.error(
            `[RelationshipManager] Vector index ${vectorIndexName} not created (Neo4j 5.11+ required).`,
          );
        }
      }
    }
  }

  // ── Singleton ──

  private static instance: RelationshipManager | null = null;

  static getCachedInstance(): RelationshipManager {
    if (!RelationshipManager.instance) {
      RelationshipManager.instance = new RelationshipManager();
    }
    return RelationshipManager.instance;
  }
}
